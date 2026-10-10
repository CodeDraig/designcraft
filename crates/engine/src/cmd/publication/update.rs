//! Bounded publication composition. All work happens on a candidate; callers publish only a
//! completed fixed point. Page fields, generated sections and running heads share this cycle.
use super::{Result, bad};
use designcraft_doc::{
    Document, EditionId, ParaFormat, Story,
    publication::{Publication, PublicationView, references::*},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

const PASSES: usize = 8;
const MAX_PAGES: usize = 10_000;

pub(crate) fn refresh(doc: &mut Document, force: bool, cache: &designcraft_compose::Cache) -> Result<Value> {
    let Some(v) = doc.publication.clone() else { return Ok(Value::Null) };
    let e = v.root.editions.get(&v.edition).ok_or_else(|| bad("publication.update", "missing edition"))?;
    if e.references.source_revision == Some(v.root.revision) || (!force && e.references.mode == UpdateMode::Deferred) {
        return Ok(json!({"stale":e.references.source_revision!=Some(v.root.revision)}));
    }
    if v.root.references.fields.is_empty() && v.root.references.sections.is_empty() && v.root.indexes.is_empty() && v.root.note_streams.is_empty() {
        return Ok(json!({"passes":0,"stale":false}));
    }
    let mut root = (*v.root).clone();
    super::annotations::prepare(&mut root, v.edition)?;
    regenerate(&mut root, v.edition)?;
    super::indexes::regenerate(&mut root, v.edition)?;
    let index = SemanticIndex::build(&root)?;
    let mut layout = root.editions.get(&v.edition).ok_or_else(|| bad("publication.update", "missing edition"))?.references.clone();
    evaluate(&root, v.edition, &index, &mut layout, cache)?;
    super::annotations::number(&mut root, v.edition, &layout)?;
    for pass in 1..=PASSES {
        Arc::make_mut(root.editions.get_mut(&v.edition).ok_or_else(|| bad("publication.update", "missing edition"))?).references = layout.clone();
        let snapshot = Arc::new(root.clone());
        let mut next = ReferenceLayout {
            mode: layout.mode,
            generation: layout.generation.saturating_add(1),
            diagnostics: super::annotations::diagnostics(&root, v.edition)?,
            ..Default::default()
        };
        let mut global_page = 0usize;
        for component in root.ordered_components()? {
            let mut d = PublicationView { root: snapshot.clone(), edition: v.edition, component }.project()?;
            let generated: Vec<_> = root
                .generated_destinations()
                .filter(|(_, c, _, _)| *c == component)
                .filter(|(_, c, s, _)| root.story_included(v.edition, *c, *s))
                .filter(|(_, c, s, _)| {
                    root.note_output(*c, *s).is_none_or(|o| {
                        let placement = root.note_policy(v.edition, &o.stream).placement;
                        matches!(
                            (placement, o.source),
                            (designcraft_doc::publication::annotations::Placement::ComponentEnd, Some(_))
                                | (designcraft_doc::publication::annotations::Placement::PublicationEnd, None)
                        )
                    })
                })
                .map(|(_, _, story, _)| story)
                .collect();
            for &sid in &generated {
                if d.story(sid).is_some_and(|s| s.frames.is_empty()) {
                    let page = d.page_count();
                    d.insert_pages(page.checked_sub(1), 1, None)?;
                    let (si, pi) = d.page_loc(page).ok_or_else(|| bad("publication.update", "missing generated page"))?;
                    let rect = d.spreads[si].pages[pi].margin_rect();
                    let layer = d.default_layer();
                    let (fid, blank) = d.add_text_frame(designcraft_doc::SpreadRef::Doc(si), rect, layer, "", Default::default())?;
                    d.stories.remove(&blank);
                    d.item_mut(fid).and_then(|i| i.text_frame_mut()).ok_or_else(|| bad("publication.update", "missing generated frame"))?.story = sid;
                    d.story_mut(sid).ok_or_else(|| bad("publication.update", "missing generated story"))?.frames.push(fid);
                }
            }
            let mut flow = generated;
            flow.extend(d.settings.primary_story);
            flow.extend(d.endnote_story);
            flow.extend(
                root.annotation_entries()?
                    .into_iter()
                    .filter(|e| e.component == component && d.story(e.story).is_some_and(|s| !s.frames.is_empty()))
                    .map(|e| e.story),
            );
            flow.sort();
            flow.dedup();
            for sid in flow {
                if !d.publication_story_included(sid) {
                    continue;
                }
                let remaining = MAX_PAGES.saturating_sub(global_page + d.page_count());
                super::super::place_text::autoflow_with_cache(&mut d, sid, remaining, cache)?;
            }
            if global_page.saturating_add(d.page_count()) > MAX_PAGES {
                return Err(bad("publication.update", "publication exceeds the 10000-page update limit"));
            }
            let c = root.components.get(&component).ok_or_else(|| bad("publication.update", "missing component"))?;
            let mut anchors = BTreeMap::new();
            let mut end_anchors = BTreeMap::new();
            let mut body = BTreeMap::new();
            for (order, sid) in c.story_order.iter().enumerate() {
                if !d.publication_story_included(*sid) {
                    continue;
                }
                let Some(story) = d.story(*sid) else { continue };
                if story.frames.is_empty() {
                    if !story.text.is_empty() {
                        next.diagnostics.push(format!("Unplaced story {} in component {}", sid.0, component.0));
                    }
                    continue;
                }
                let cs = cache.get(&d, *sid, None);
                next.diagnostics.extend(cs.diagnostics.iter().cloned());
                super::annotations::locations(&d, &cs, component, global_page, &mut next);
                if let Some(at) = cs.overset_at {
                    return Err(bad(
                        "publication.update",
                        format!("component {}, story {} has unplaced text at byte {at}; thread its frame or make it primary", component.0, sid.0),
                    ));
                }
                let is_body = d.settings.primary_story.map_or_else(
                    || {
                        !root.generated_destinations().any(|(_, c, s, _)| c == component && s == *sid)
                            && !story
                                .xrefs
                                .iter()
                                .any(|x| x.field.and_then(|id| root.references.fields.get(&id)).is_some_and(|f| f.use_ == FieldUse::RunningHead))
                    },
                    |primary| primary == *sid,
                );
                collect(&d, story, &cs, None, &[order], is_body, &mut anchors, &mut end_anchors, &mut body, 0)?;
            }
            next.first_body.insert(component, body);
            next.locations.insert(component.0, Location { component, page: 0, global_page, label: d.page_name(0), order: vec![] });
            for entity in index.entries.iter().filter(|e| e.component == component) {
                let Some(target) = &entity.target else { continue };
                if let Some((page, order)) = anchors.get(&target.anchor) {
                    next.locations.insert(
                        entity.id,
                        Location { component, page: *page, global_page: global_page + page, label: d.page_name(*page), order: order.clone() },
                    );
                }
            }
            for definition in root.indexes.values() {
                let end_ids: std::collections::BTreeSet<_> = definition.occurrences.values().filter_map(|o| o.end.as_ref().map(|e| e.id)).collect();
                for (id, target) in definition.targets().filter(|(_, t)| t.component == component) {
                    let positions = if end_ids.contains(&id) { &end_anchors } else { &anchors };
                    if let Some((page, order)) = positions.get(&target.anchor) {
                        next.locations.insert(
                            id,
                            Location { component, page: *page, global_page: global_page + page, label: d.page_name(*page), order: order.clone() },
                        );
                    }
                }
                if let Some(dest) = definition.destination.as_ref().filter(|dest| dest.component == component)
                    && let Some(story) = d.story(dest.story)
                {
                    let cs = cache.get(&d, dest.story, None);
                    for ((at, _), xref) in story.text.match_indices(designcraft_doc::XREF_MARK).zip(&story.xrefs) {
                        let Some(topic) = definition.fields.iter().find(|(_, id)| Some(**id) == xref.field).map(|(topic, _)| *topic) else {
                            continue;
                        };
                        if let Some(page) = cs.frames.iter().find(|f| f.range.contains(&at)).and_then(|f| d.page_of_item(f.frame)) {
                            next.locations.insert(
                                topic,
                                Location { component, page, global_page: global_page + page, label: d.page_name(page), order: vec![at] },
                            );
                        }
                    }
                    // Run-in descendants share their root topic's generated paragraph.
                    for (id, level) in definition.ordered_topics()? {
                        if definition.format.run_in
                            && level > 1
                            && let Some(parent) = definition.topics.get(&id).and_then(|t| t.parent)
                            && let Some(location) = next.locations.get(&parent).cloned()
                        {
                            next.locations.insert(id, location);
                        }
                    }
                }
            }
            global_page += d.page_count();
            root.capture(v.edition, component, &d)?;
        }
        evaluate(&root, v.edition, &index, &mut next, cache)?;
        let notes_changed = super::annotations::number(&mut root, v.edition, &next)?;
        let converged = !notes_changed && layout.locations == next.locations && layout.first_body == next.first_body && layout.fields == next.fields;
        next.passes = pass;
        layout = next;
        if converged {
            layout.source_revision = Some(root.revision);
            let report = json!({"passes":pass,"pages":global_page,"fields":layout.fields.len(),"diagnostics":layout.diagnostics,"stale":false});
            Arc::make_mut(root.editions.get_mut(&v.edition).ok_or_else(|| bad("publication.update", "missing edition"))?).references = layout;
            root.check()?;
            *doc = PublicationView { root: Arc::new(root), ..v }.project()?;
            return Ok(report);
        }
    }
    Err(bad(
        "publication.update",
        format!(
            "pagination and generated fields did not converge after {PASSES} passes; narrow a page-dependent template or switch to deferred updates while editing"
        ),
    ))
}

fn evaluate(
    root: &Publication,
    edition: EditionId,
    index: &SemanticIndex,
    layout: &mut ReferenceLayout,
    cache: &designcraft_compose::Cache,
) -> Result<()> {
    let context = RenderContext::new(root, &root.references, index, layout)?;
    let mut fields = BTreeMap::new();
    let mut bytes = 0usize;
    let mut dependencies = BTreeMap::new();
    let mut diagnostics = layout.diagnostics.clone();
    let used: std::collections::BTreeSet<_> = root.content.values().flat_map(|c| field_ids(c)).collect();
    for (&id, field) in root.references.fields.iter().filter(|(id, _)| used.contains(id)) {
        let signature = context.input_signature(field)?;
        let cached = cache.field_cached(edition, id, &signature);
        let reused = cached.is_some();
        let (values, deps) = if let Some(cached) = cached {
            cached
        } else {
            let (text, mut deps) = context.field(field, None)?;
            let mut values = FieldValues { text, ..Default::default() };
            if field.use_ == FieldUse::RunningHead {
                for (&component, pages) in &layout.first_body {
                    for &page in pages.keys() {
                        let (text, more) = context.field(field, Some((component, page)))?;
                        values.pages.entry(component).or_default().insert(page, text);
                        for d in more {
                            if !deps.contains(&d) {
                                deps.push(d);
                            }
                        }
                    }
                }
            }
            (values, deps)
        };
        for text in std::iter::once(&values.text).chain(values.pages.values().flat_map(|p| p.values())) {
            bytes = bytes.saturating_add(text.runs.iter().map(|r| r.text.len()).sum::<usize>());
            if bytes > 64 * 1024 * 1024 {
                return Err(bad("publication.update", "generated text exceeds 64 MiB in this edition; narrow its queries"));
            }
            diagnostics.extend(text.unresolved.iter().cloned());
        }
        let previous = layout.fields.get(&id).cloned().or_else(|| cache.previous_field(edition, id));
        cache.record_reference(reused, previous.as_ref() != Some(&values));
        if !reused {
            cache.remember_field(edition, id, signature, values.clone(), deps.clone());
        }
        fields.insert(id, values);
        dependencies.insert(id, deps);
    }
    diagnostics.sort();
    diagnostics.dedup();
    layout.fields = fields;
    layout.dependencies = dependencies;
    layout.diagnostics = diagnostics;
    super::indexes::evaluate(root, layout)?;
    Ok(())
}

/// Rebuild only generated destinations. Their marker identities survive reordering; authored
/// text elsewhere is never replaced. The regular editing boundary rejects accidental typing
/// into a still-bound generated story; detach is explicit.
fn regenerate(root: &mut Publication, edition: EditionId) -> Result<()> {
    let index = SemanticIndex::build(root)?;
    let names: Vec<_> = root.references.sections.keys().cloned().collect();
    for name in names {
        let mut g = root.references.sections.get(&name).cloned().ok_or_else(|| bad("publication.update", "missing section"))?;
        let entries = index.query(&g.query)?;
        let mut d = root.project(edition, g.component)?;
        let previous = d.story(g.story).cloned().ok_or_else(|| bad("publication.update", "missing section story"))?;
        let mut story = Story::with_text(g.story, &g.title, ParaFormat { style: g.title_style.clone(), ..Default::default() });
        for entry in entries {
            if entry.target.as_ref().is_some_and(|t| t.content == g.content) {
                return Err(bad("publication.update", format!("generated section {} includes itself", g.name)));
            }
            let id = match g.fields.get(&entry.id) {
                Some(id) => *id,
                None => root.alloc()?,
            };
            g.fields.insert(entry.id, id);
            root.references.fields.insert(
                id,
                Field {
                    id,
                    query: Query { ids: vec![entry.id], ..Default::default() },
                    template: g.template.clone(),
                    use_: FieldUse::First,
                    separator: String::new(),
                    suppress_opening: false,
                },
            );
            let at = story.len();
            story.replace(at..at, "\n");
            let at = story.len();
            story.insert_xref(at, designcraft_doc::xref::CrossRef { target: 0, format: String::new(), field: Some(id) });
            let style = g.level_styles.get(&entry.level).unwrap_or(&g.paragraph_style);
            if let Some(p) = story.paras.last_mut() {
                p.style = style.clone();
            }
        }
        carry_entry_formats(&previous, &mut story);
        // Keep paragraph/character overrides when membership and styles have not changed.
        if previous.text != story.text || previous.xrefs != story.xrefs {
            let target = d.story_mut(g.story).ok_or_else(|| bad("publication.update", "missing section"))?;
            target.content = story.content;
            target.rev = target.rev.saturating_add(1);
            root.capture(edition, g.component, &d)?;
        }
        root.references.sections.insert(name, g);
    }
    Ok(())
}

type Anchors = BTreeMap<u64, (usize, Vec<usize>)>;
#[allow(clippy::too_many_arguments)]
fn collect(
    d: &Document,
    story: &Story,
    cs: &designcraft_compose::ComposedStory,
    parent: Option<usize>,
    prefix: &[usize],
    body: bool,
    anchors: &mut Anchors,
    end_anchors: &mut Anchors,
    first: &mut BTreeMap<usize, Vec<usize>>,
    depth: usize,
) -> Result<()> {
    if depth > 32 {
        return Err(bad("publication.update", "nested reference content exceeds 32 levels"));
    }
    for ft in &cs.frames {
        let Some(page) = parent.or_else(|| d.page_of_item(ft.frame)) else { continue };
        for a in &story.anchors {
            if let Some(pos) = story.anchor_pos(a.id)
                && ft.range.contains(&pos)
            {
                let mut order = prefix.to_vec();
                order.push(pos);
                anchors.entry(a.id).or_insert((page, order.clone()));
                let previous = story
                    .text
                    .get(..pos)
                    .and_then(|text| text.char_indices().rev().find(|(_, ch)| *ch != designcraft_doc::ANCHOR_MARK).map(|(at, _)| at));
                let end_page = parent
                    .or_else(|| previous.and_then(|at| cs.frames.iter().rev().find(|f| f.range.contains(&at))).and_then(|f| d.page_of_item(f.frame)))
                    .unwrap_or(page);
                end_anchors.entry(a.id).or_insert((end_page, order));
            }
        }
        if body && let Some(line) = ft.lines.first() {
            let mut order = prefix.to_vec();
            order.push(line.range.start);
            first
                .entry(page)
                .and_modify(|old| {
                    if order < *old {
                        *old = order.clone()
                    }
                })
                .or_insert(order);
        }
        for fragment in &ft.tables {
            if let Some(table) = story.tables.get(&fragment.table) {
                let at = story.table_anchor(fragment.table).unwrap_or(ft.range.start);
                for cell in &fragment.cells {
                    if let Some(source) = table.cell(cell.row, cell.col) {
                        let index = table.cells.iter().position(|c| std::ptr::eq(c, source)).unwrap_or(0);
                        let mut path = prefix.to_vec();
                        path.extend([at, index]);
                        collect(d, &source.text, &cell.text, Some(page), &path, false, anchors, end_anchors, first, depth + 1)?;
                    }
                }
            }
        }
        for note in &ft.notes {
            if let Some(source) = note.source_story.as_deref().or_else(|| story.notes.get(note.index).map(|n| &n.text)) {
                let at = story.text.match_indices(designcraft_doc::FOOTNOTE_REF).nth(note.index).map_or(ft.range.start, |(at, _)| at);
                let mut path = prefix.to_vec();
                path.push(at);
                collect(d, source, &note.text, Some(page), &path, false, anchors, end_anchors, first, depth + 1)?;
            }
        }
    }
    Ok(())
}
