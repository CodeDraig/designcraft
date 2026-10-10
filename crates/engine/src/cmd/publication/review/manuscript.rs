//! Import once into an isolated candidate. Paragraph matches and every uncertain decision are
//! shown before selected rich paragraphs, attachments and styles enter one undo transaction.
use super::*;
use designcraft_doc::{ComponentId, EditionId, publication::Publication};
use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Clone, Debug)]
pub(crate) struct Preview {
    id: u64,
    root: Arc<Publication>,
    edition: EditionId,
    component: ComponentId,
    story: StoryId,
    imported: Story,
    candidate: Arc<Document>,
    rows: Vec<Row>,
    pairs: BTreeMap<usize, usize>,
}
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    row: usize,
    kind: String,
    confidence: String,
    requires_review: bool,
    before: Option<usize>,
    after: Option<usize>,
    before_text: String,
    after_text: String,
    source_ids: Vec<String>,
}
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "publication.manuscript.preview","Compare Revised Manuscript",[],None,"{path | base64 + name, story?, tei?, resources?, styleMap?, styleConflicts?} → preview and paragraph rows; source identities first, all other matches require explicit selection",has_publication,preview),
        cmd!(
            "publication.manuscript.apply",
            "Apply Manuscript Revisions",
            [],
            None,
            "{preview, rows:[row], reviewed?:[uncertain row], autoflow?:true} — explicit selected rows only; stale previews rejected; rich content and sources preserved; one undo step",
            has_publication,
            apply
        ),
    ]
}

fn clean(text: &str) -> String {
    text.chars().filter(|c| *c != designcraft_doc::ANCHOR_MARK).collect()
}
fn paragraphs(s: &Story) -> Vec<Range<usize>> {
    let starts: Vec<_> = s.para_ranges().into_iter().map(|r| r.start).collect();
    starts.iter().enumerate().map(|(i, start)| *start..starts.get(i + 1).copied().unwrap_or(s.len())).collect()
}
fn ids(story: &Story, ranges: &[Range<usize>], known: &BTreeSet<String>) -> Vec<BTreeSet<String>> {
    let mut result = vec![BTreeSet::new(); ranges.len()];
    for ((at, _), anchor) in story.text.match_indices(designcraft_doc::ANCHOR_MARK).zip(&story.anchors) {
        if known.contains(&anchor.name)
            && let Some(row) = result.get_mut(ranges.partition_point(|r| r.end <= at))
        {
            row.insert(anchor.name.clone());
        }
    }
    result
}
fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    let v = view(s)?;
    let d = &s.doc()?.doc;
    let sid = optional_id(p, "story")?
        .map(StoryId)
        .or(s.doc()?.selection.text.map(|t| t.story))
        .or(d.settings.primary_story)
        .or_else(|| d.stories.values().filter(|s| Some(s.id) != d.endnote_story).max_by_key(|s| s.len()).map(|s| s.id))
        .ok_or_else(|| bad("publication.manuscript.preview", "select an authored story"))?;
    if d.endnote_story == Some(sid) || v.root.generated_destinations().any(|(_, c, s, _)| c == v.component && s == sid) {
        return Err(bad("publication.manuscript.preview", "choose authored text, not generated output"));
    }
    let old = d.story(sid).ok_or_else(|| bad("publication.manuscript.preview", "unknown story"))?;
    let (bytes, name) = if let Some(b) = str_param(p, "base64") {
        (super::super::super::tei::decoded(b, 64 * 1024 * 1024)?, str_param(p, "name").unwrap_or("revision.txt").to_owned())
    } else {
        let path = str_param(p, "path").ok_or_else(|| bad("publication.manuscript.preview", "path or named bytes required"))?;
        (
            super::super::super::tei::read_path(path, 64 * 1024 * 1024)?,
            std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.into()),
        )
    };
    let tei = designcraft_textimport::tei::is_tei(&bytes) || name.ends_with(".tei") || name.ends_with(".xml");
    let imp = if tei {
        super::super::super::tei::import(&name, &bytes, p)?
    } else {
        designcraft_textimport::import(&name, &bytes).map_err(|e| bad("publication.manuscript.preview", e.to_string()))?
    };
    let known = if tei {
        designcraft_textimport::tei::source_ids(&bytes).map_err(|e| bad("publication.manuscript.preview", e.to_string()))?
    } else {
        BTreeSet::new()
    };
    let mut original_ids = BTreeSet::new();
    for src in &d.tei_sources {
        original_ids.extend(designcraft_textimport::tei::source_ids(&src.bytes).map_err(|e| bad("publication.manuscript.preview", e.to_string()))?);
    }
    let mut standalone = (**d).clone();
    standalone.publication = None;
    let mut candidate = Session::new();
    candidate.add_document(crate::DocState::new(standalone, None));
    let mut options = p.clone();
    if let Some(o) = options.as_object_mut() {
        o.remove("frame");
        o.remove("story");
    }
    options["autoflow"] = json!(false);
    let result = super::super::super::place_text::place_imported(&mut candidate, &options, imp)?;
    let imported = candidate
        .doc()?
        .doc
        .story(StoryId(result["story"].as_u64().ok_or_else(|| bad("publication.manuscript.preview", "import returned no story"))?))
        .ok_or_else(|| bad("publication.manuscript.preview", "imported story missing"))?
        .clone();
    let before = paragraphs(old);
    let after = paragraphs(&imported);
    if before.len().saturating_add(after.len()) > 100_000 {
        return Err(bad("publication.manuscript.preview", "comparison exceeds 100000 paragraphs; compare a smaller component"));
    }
    let old_ids = ids(old, &before, &original_ids);
    let new_ids = ids(&imported, &after, &known);
    let old_text: Vec<_> = before.iter().map(|r| clean(old.text.get(r.clone()).unwrap_or_default())).collect();
    let new_text: Vec<_> = after.iter().map(|r| clean(imported.text.get(r.clone()).unwrap_or_default())).collect();
    let mut by_id: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut by_text: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, ids) in old_ids.iter().enumerate() {
        for id in ids {
            by_id.entry(id.clone()).or_default().push(i);
        }
    }
    for (i, t) in old_text.iter().enumerate() {
        by_text.entry(t.clone()).or_default().push(i);
    }
    let mut used = BTreeSet::new();
    let mut pairs = BTreeMap::new();
    let mut confidence = BTreeMap::new();
    // Complete the identity pass before any text heuristic is allowed to consume a paragraph.
    for (i, ids) in new_ids.iter().enumerate() {
        let candidates: BTreeSet<_> = ids.iter().filter_map(|id| by_id.get(id)).flatten().copied().collect();
        if candidates.len() == 1
            && let Some(j) = candidates.first().copied().filter(|j| !used.contains(j))
        {
            pairs.insert(i, j);
            used.insert(j);
            confidence.insert(i, "sourceId");
        }
    }
    for (i, text) in new_text.iter().enumerate() {
        if pairs.contains_key(&i) {
            continue;
        }
        if let Some(exact) = by_text.get(text).filter(|c| c.len() == 1)
            && let Some(j) = exact.first().copied().filter(|j| !used.contains(j))
        {
            pairs.insert(i, j);
            used.insert(j);
            confidence.insert(i, "text");
        }
    }
    // Positional candidates are suggestions only, never implicit replacements.
    let mut unused: BTreeSet<_> = (0..before.len()).filter(|j| !used.contains(j)).collect();
    for i in 0..after.len() {
        if pairs.contains_key(&i) {
            continue;
        }
        let next = pairs.range(i + 1..).next().map(|(_, j)| *j).unwrap_or(before.len());
        let previous = pairs.range(..i).next_back().map(|(_, j)| j.saturating_add(1)).unwrap_or(0);
        if previous < next
            && let Some(j) = unused.range(previous..next).next().copied()
        {
            pairs.insert(i, j);
            used.insert(j);
            unused.remove(&j);
            confidence.insert(i, "position");
        }
    }
    let mut rows = Vec::new();
    let mut max_before = Vec::with_capacity(after.len());
    let mut max = None;
    for i in 0..after.len() {
        max_before.push(max);
        if let Some(j) = pairs.get(&i) {
            max = Some(max.map_or(*j, |v: usize| v.max(*j)));
        }
    }
    let mut min_after = vec![None; after.len()];
    let mut min = None;
    for i in (0..after.len()).rev() {
        min_after[i] = min;
        if let Some(j) = pairs.get(&i) {
            min = Some(min.map_or(*j, |v: usize| v.min(*j)));
        }
    }
    for i in 0..after.len() {
        let j = pairs.get(&i).copied();
        let moved = j.is_some_and(|j| max_before[i].is_some_and(|earlier| earlier > j) || min_after[i].is_some_and(|later| later < j));
        let equal =
            j.is_some_and(|j| old_text.get(j) == new_text.get(i) && rich_equal(&old.extract(before[j].clone()), &imported.extract(after[i].clone())));
        if equal && !moved {
            continue;
        }
        let confidence = confidence.get(&i).copied().unwrap_or("unmatched");
        rows.push(Row {
            row: rows.len(),
            kind: if j.is_none() {
                "insert"
            } else if moved {
                "moveOrReplace"
            } else {
                "replace"
            }
            .into(),
            confidence: confidence.into(),
            requires_review: confidence != "sourceId",
            before: j,
            after: Some(i),
            before_text: j.and_then(|j| old_text.get(j)).cloned().unwrap_or_default(),
            after_text: new_text.get(i).cloned().unwrap_or_default(),
            source_ids: new_ids.get(i).map(|s| s.iter().cloned().collect()).unwrap_or_default(),
        });
    }
    for j in 0..before.len() {
        if !used.contains(&j) {
            rows.push(Row {
                row: rows.len(),
                kind: "delete".into(),
                confidence: "unmatched".into(),
                requires_review: true,
                before: Some(j),
                after: None,
                before_text: old_text.get(j).cloned().unwrap_or_default(),
                after_text: String::new(),
                source_ids: old_ids.get(j).map(|s| s.iter().cloned().collect()).unwrap_or_default(),
            });
        }
    }
    let state = s.doc_mut()?;
    state.publication_preview_id =
        state.publication_preview_id.checked_add(1).ok_or_else(|| bad("publication.manuscript.preview", "preview identity exhausted"))?;
    let id = state.publication_preview_id;
    let output = json!({"preview":id,"component":v.component,"story":sid,"name":name,"rows":rows,"warnings":result["warnings"],"note":"Rows are rich paragraphs including their tables, illustrations and notes. Select uncertain rows explicitly in both rows and reviewed. Unselected source references may remain unresolved."});
    state.publication_manuscript_preview = Some(Preview {
        id,
        root: v.root,
        edition: v.edition,
        component: v.component,
        story: sid,
        imported,
        candidate: candidate.doc()?.doc.clone(),
        rows,
        pairs,
    });
    Ok(output)
}

fn rich_equal(a: &Story, b: &Story) -> bool {
    // Ignore allocation identities and navigation-only anchors, preserving authored typography,
    // nested text and payloads in the comparison. IDs in rich payloads may conservatively show
    // a review row; they cannot suppress a real difference.
    let mut a = a.clone();
    let mut b = b.clone();
    for s in [&mut a, &mut b] {
        s.for_each_text_mut(&mut |leaf| {
            let marks: Vec<_> = leaf.text.match_indices(designcraft_doc::ANCHOR_MARK).map(|(p, _)| p).collect();
            for p in marks.into_iter().rev() {
                leaf.delete(p..p + designcraft_doc::ANCHOR_MARK.len_utf8());
            }
        });
    }
    a.content == b.content
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let preview =
        s.doc()?.publication_manuscript_preview.clone().ok_or_else(|| bad("publication.manuscript.apply", "preview a revised manuscript first"))?;
    if required(p, "preview")? != preview.id || !Arc::ptr_eq(&view(s)?.root, &preview.root) {
        return Err(bad("publication.manuscript.apply", "stale preview: publication changed; compare again"));
    }
    let selected: Vec<usize> =
        serde_json::from_value(p.get("rows").cloned().ok_or_else(|| bad("publication.manuscript.apply", "explicit rows required"))?)
            .map_err(|e| bad("publication.manuscript.apply", e.to_string()))?;
    let reviewed: BTreeSet<usize> =
        serde_json::from_value(p.get("reviewed").cloned().unwrap_or(json!([]))).map_err(|e| bad("publication.manuscript.apply", e.to_string()))?;
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for i in selected {
        if !seen.insert(i) {
            return Err(bad("publication.manuscript.apply", "duplicate row"));
        }
        let row = preview.rows.get(i).ok_or_else(|| bad("publication.manuscript.apply", "row out of range"))?;
        if row.requires_review && !reviewed.contains(&i) {
            return Err(bad("publication.manuscript.apply", format!("row {i} has an uncertain match; explicitly include it in reviewed")));
        }
        rows.push(row.clone());
    }
    if rows.is_empty() {
        return Ok(json!({"applied":0}));
    }
    let applied = rows.len();
    let response = mutate(s, |root, v| {
        let mut d = root.project(preview.edition, preview.component)?;
        let original = d.story(preview.story).ok_or_else(|| bad("publication.manuscript.apply", "original story missing"))?.clone();
        let before = paragraphs(&original);
        let after = paragraphs(&preview.imported);
        for row in &rows {
            if let Some(i) = row.before {
                let selected = original.extract(before[i].clone());
                let mut tracked = false;
                designcraft_doc::publication::review::visit_text(&selected, &mut Vec::new(), &mut |s, _| {
                    tracked |= s.runs().any(|(_, f)| matches!(f.over.change, Some(ChangeMark::Inserted | ChangeMark::Deleted)));
                    Ok(())
                })?;
                if tracked {
                    return Err(bad(
                        "publication.manuscript.apply",
                        format!("paragraph {i} has pending tracked changes; resolve them and compare again"),
                    ));
                }
            }
        }
        let old_anchor_names = anchor_names(&original)?;
        let new_anchor_names = anchor_names(&preview.imported)?;
        let mut remap = BTreeMap::new();
        for (name, old) in &old_anchor_names {
            if old.len() == 1
                && let Some(new) = new_anchor_names.get(name).filter(|v| v.len() == 1)
            {
                remap.insert(new[0], old[0]);
            }
        }
        let mut incoming = preview.imported.clone();
        incoming.for_each_encoding_mut(&mut |s| {
            for a in &mut s.anchors {
                if let Some(id) = remap.get(&a.id) {
                    Arc::make_mut(a).id = *id;
                }
            }
            for x in &mut s.xrefs {
                if let Some(id) = remap.get(&x.target) {
                    Arc::make_mut(x).target = *id;
                }
            }
            let c = Arc::make_mut(&mut s.content);
            for n in c.notes.iter_mut().chain(&mut c.endnotes) {
                let n = Arc::make_mut(n);
                n.annotation = None;
                if let Some(a) = &mut n.apparatus {
                    if let Some(id) = remap.get(&a.start) {
                        a.start = *id;
                    }
                    if let Some(id) = remap.get(&a.end) {
                        a.end = *id;
                    }
                }
            }
        });
        let mut pieces: Vec<(Option<usize>, Story)> = before.iter().enumerate().map(|(i, r)| (Some(i), original.extract(r.clone()))).collect();
        // Replacements retain current order; explicit move rows follow revised source order.
        let mut inserts = Vec::new();
        for row in &rows {
            let at = row.before.and_then(|j| pieces.iter().position(|(old, _)| *old == Some(j)));
            let replacement = row.after.map(|i| incoming.extract(after[i].clone()));
            let mut replacement = replacement;
            if let (Some(j), Some(new)) = (row.before, replacement.as_mut()) {
                preserve_anchors(&original.extract(before[j].clone()), new)?;
            }
            if row.kind == "replace" {
                if let (Some(at), Some(new)) = (at, replacement)
                    && let Some(slot) = pieces.get_mut(at)
                {
                    slot.1 = new;
                }
            } else {
                if let Some(at) = at {
                    pieces.remove(at);
                }
                if let Some(new) = replacement {
                    inserts.push((row.after.unwrap_or(0), row.before, new));
                }
            }
        }
        inserts.sort_by_key(|(i, _, _)| *i);
        for (i, old, new) in inserts.into_iter().rev() {
            let at = preview.pairs.range(i + 1..).find_map(|(_, j)| pieces.iter().position(|(old, _)| *old == Some(*j))).unwrap_or(pieces.len());
            pieces.insert(at, (old, new));
        }
        let mut revised = Story::new(original.id);
        for (i, (_, mut piece)) in pieces.into_iter().enumerate() {
            if piece.text.ends_with('\n') {
                piece.delete(piece.len() - 1..piece.len());
            }
            if i > 0 {
                revised.insert(revised.len(), "\n");
            }
            let at = revised.len();
            let para = revised.para_at(at);
            revised.insert_story_preserving_anchors(at, &piece);
            if let Some(p) = piece.paras.first()
                && let Some(slot) = revised.paras.get_mut(para)
            {
                *slot = designcraft_doc::ParaFormat { table: slot.table, ..p.clone() };
            }
        }
        revised.frames = original.frames.clone();
        revised.direction = original.direction;
        revised.vertical = original.vertical;
        revised.rev = original.rev.saturating_add(1);
        revised.check().map_err(|e| bad("publication.manuscript.apply", e))?;
        d.stories.insert(preview.story, Arc::new(revised));
        d.styles = preview.candidate.styles.clone();
        d.assets = preview.candidate.assets.clone();
        d.next_id = preview.candidate.next_id;
        d.tei_sources = preview.candidate.tei_sources.clone();
        let mut links = preview.candidate.hyperlinks.clone();
        for link in &mut links {
            if let designcraft_doc::HyperlinkSource::Anchors { start, end } = &mut link.source {
                if let Some(id) = remap.get(start) {
                    *start = *id;
                }
                if let Some(id) = remap.get(end) {
                    *end = *id;
                }
            }
            if let designcraft_doc::HyperlinkDest::Anchor(a) = &mut link.dest
                && let Some(id) = remap.get(a)
            {
                *a = *id;
            }
        }
        d.hyperlinks = links;
        for src in &mut d.tei_sources {
            if !original_source(&preview.root, preview.component, src.id) {
                for nav in &mut Arc::make_mut(src).navigation {
                    if let Some(id) = remap.get(&nav.anchor) {
                        nav.anchor = *id;
                    }
                }
            }
        }
        d.sync_endnote_story();
        if p["autoflow"].as_bool().unwrap_or(true) && d.story(preview.story).is_some_and(|s| !s.frames.is_empty()) {
            super::super::super::place_text::autoflow(&mut d, preview.story, 2000)?;
        }
        d.check()?;
        root.capture(preview.edition, preview.component, &d)?;
        v.component = preview.component;
        v.edition = preview.edition;
        Ok(json!({"applied":applied,"component":preview.component,"story":preview.story,"sources":d.tei_sources.len()}))
    })?;
    s.doc_mut()?.publication_manuscript_preview = None;
    Ok(response)
}
fn original_source(root: &Publication, component: ComponentId, id: u64) -> bool {
    root.components.get(&component).is_some_and(|c| c.sources.iter().filter_map(|id| root.sources.get(id)).any(|s| s.id == id))
}
fn anchor_names(story: &Story) -> Result<BTreeMap<String, Vec<u64>>> {
    let mut out: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    designcraft_doc::publication::review::visit_text(story, &mut Vec::new(), &mut |s, _| {
        for a in &s.anchors {
            out.entry(a.name.clone()).or_default().push(a.id);
        }
        Ok(())
    })?;
    Ok(out)
}
fn preserve_anchors(old: &Story, new: &mut Story) -> Result<()> {
    let a = clean(&old.text);
    let b = clean(&new.text);
    let edits = designcraft_doc::publication::text_edits::edits(&a, &b)?;
    let mut add = Vec::new();
    for anchor in &old.anchors {
        if new.anchors.iter().any(|a| a.id == anchor.id) {
            continue;
        }
        let Some(at) = old.anchor_pos(anchor.id) else {
            continue;
        };
        let clean_at = clean(old.text.get(..at).unwrap_or_default()).len();
        let mut mapped = clean_at as i128;
        let mut deleted = false;
        for e in &edits {
            if e.before.start < clean_at && clean_at < e.before.end {
                deleted = true;
                break;
            }
            if e.before.end <= clean_at {
                mapped += e.after.len() as i128 - e.before.len() as i128;
            }
        }
        if deleted {
            continue;
        }
        let mapped = usize::try_from(mapped).map_err(|_| bad("publication.manuscript.apply", "anchor alignment overflow"))?.min(b.len());
        let mut n = 0;
        let pos = new
            .text
            .char_indices()
            .find_map(|(i, c)| {
                if n >= mapped {
                    Some(i)
                } else {
                    if c != designcraft_doc::ANCHOR_MARK {
                        n += c.len_utf8();
                    }
                    None
                }
            })
            .unwrap_or(new.len());
        add.push((pos, anchor.clone()));
    }
    add.sort_by_key(|(p, _)| *p);
    for (pos, a) in add.into_iter().rev() {
        super::insert_anchor(new, pos, &a.name, a.id);
    }
    Ok(())
}
