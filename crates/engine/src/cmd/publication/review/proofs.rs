//! Immutable proof snapshots and identity-based comparison; no screenshots masquerading as text diff.
use super::*;
use designcraft_doc::{
    ComponentId, EditionId, ItemId,
    publication::{PublicationView, review::*},
};
use std::collections::BTreeMap;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "publication.proof.create",
            "Capture Proof Baseline",
            [],
            None,
            "{name, author?, edition?} → immutable snapshot of content, edition, composition and actual font inputs",
            has_publication,
            create
        ),
        cmd!(
            "publication.proof.approve",
            "Approve Proof Baseline",
            [],
            None,
            "{id,author?,note?} — approval is separate from the immutable baseline and cannot be replaced",
            has_publication,
            |s, p| {
                let id = required(p, "id")?;
                mutate(s, |root, _| {
                    if !root.review.proofs.contains_key(&id) {
                        return Err(bad("publication.proof.approve", "unknown proof"));
                    }
                    if root.review.approvals.contains_key(&id) {
                        return Err(bad("publication.proof.approve", "proof is already approved; capture another named proof"));
                    }
                    root.review.approvals.insert(
                        id,
                        Approval {
                            author: str_param(p, "author").map(str::to_owned).unwrap_or_else(|| root.review.author()),
                            revision: root.revision,
                            note: str_param(p, "note").unwrap_or("").into(),
                        },
                    );
                    root.revision = root.revision.saturating_add(1);
                    Ok(json!({"id":id}))
                })
            }
        ),
        cmd!(query "publication.proof.compare","Compare Proofs",[],None,"{before:proof, after?:proof} → navigable content, formatting, movedFigure, pagination, reference and indexPassageRemoved rows; omitted after composes current edition",has_publication,compare),
        cmd!(noundo "publication.proof.goTo","Go to Proof Difference",[],None,"{component,story?,start?,item?,page?} — locate current chapter/item/text from a comparison row",has_publication,go_to),
    ]
}

/// Stable FNV-1a byte fingerprint, labelled explicitly; used to detect changed inputs,
/// never as an authentication or archival integrity guarantee.
pub(super) fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for b in bytes {
        hash = (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}:{}", bytes.len())
}
fn signature(v: &impl serde::Serialize) -> Result<String> {
    Ok(fingerprint(&serde_json::to_vec(v).map_err(|e| bad("publication.proof", e.to_string()))?))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name")
        .filter(|s| !s.trim().is_empty() && s.len() <= 256)
        .ok_or_else(|| bad("publication.proof.create", "unique name of 1–256 bytes required"))?
        .to_owned();
    let mut v = view(s)?;
    v.edition = EditionId(optional_id(p, "edition")?.unwrap_or(v.edition.0));
    if v.root.review.proofs.values().any(|b| b.name == name) {
        return Err(bad("publication.proof.create", "proof names are immutable; choose a new name"));
    }
    let mut proof = snapshot(s, v)?;
    proof.name = name;
    proof.author =
        str_param(p, "author").map(str::to_owned).unwrap_or_else(|| view(s).map_or_else(|_| "Unattributed".into(), |v| v.root.review.author()));
    mutate(s, |root, _| {
        proof.id = root.alloc()?;
        let result = json!({"id":proof.id,"name":proof.name,"revision":proof.revision,"edition":proof.edition.id,"stories":proof.stories.len(),"fonts":proof.fonts,"diagnostics":proof.diagnostics});
        root.review.proofs.insert(proof.id, Arc::new(proof));
        root.revision = root.revision.saturating_add(1);
        Ok(result)
    })
}

fn snapshot(s: &Session, v: PublicationView) -> Result<Proof> {
    let mut d = v.project()?;
    super::super::update::refresh(&mut d, true, &s.cache)?;
    let v = d.publication.ok_or_else(|| bad("publication.proof", "no publication"))?;
    let root = &v.root;
    let edition = root.editions.get(&v.edition).ok_or_else(|| bad("publication.proof", "missing edition"))?.clone();
    let mut proof = Proof {
        id: 0,
        name: String::new(),
        author: root.review.author(),
        revision: root.revision,
        title: root.title.clone(),
        metadata: root.metadata.clone(),
        values: root.values.clone(),
        order: root.ordered_components()?,
        components: root.components.clone(),
        content: root.content.clone(),
        text_signatures: BTreeMap::new(),
        formatting: BTreeMap::new(),
        settings: BTreeMap::new(),
        edition,
        semantic: root.semantic.clone(),
        indexes: root.indexes.clone(),
        stories: Vec::new(),
        figures: Vec::new(),
        fonts: Vec::new(),
        inputs: BTreeMap::from([
            ("generator".into(), concat!("DesignCraft ", env!("CARGO_PKG_VERSION")).into()),
            ("composition".into(), "designcraft-compose/proof-v1".into()),
            ("fingerprint".into(), "FNV-1a 64-bit with byte length (non-cryptographic)".into()),
        ]),
        diagnostics: Vec::new(),
    };
    let mut fonts = BTreeMap::new();
    for component in &proof.order {
        let d = PublicationView { component: *component, ..v.clone() }.project()?;
        proof
            .settings
            .insert(*component, json!({"styles":d.styles,"settings":d.settings,"footnotes":d.footnote_options,"endnotes":d.endnote_options}));
        for sid in d.all_items() {
            let Some(item) = d.item(sid).filter(|i| i.text_frame().is_none()) else {
                continue;
            };
            let b = item.bounds();
            proof.figures.push(ProofFigure {
                component: *component,
                item: sid,
                content: None,
                page: d.page_of_item(sid),
                bounds: [b.x0, b.y0, b.x1, b.y1],
                description: if item.alt_text.is_empty() { item.name.clone() } else { item.alt_text.clone() },
                artwork: artwork(&d, item)?,
            });
        }
        let c = root.components.get(component).ok_or_else(|| bad("publication.proof", "missing component"))?;
        for (sid, content) in &c.stories {
            let Some(story) = d.story(*sid) else {
                continue;
            };
            let mut text = Vec::new();
            visit_text(story, &mut Vec::new(), &mut |leaf, path| {
                text.push(json!({"path":path,"text":leaf.text.replace(designcraft_doc::ANCHOR_MARK,""),"references":leaf.xrefs,"objects":leaf.objects.iter().map(|o|(o.item.id,&o.item.content,&o.item.alt_text)).collect::<Vec<_>>(),"notes":leaf.notes.iter().chain(&leaf.endnotes).map(|n|(n.id,&n.annotation,&n.apparatus)).collect::<Vec<_>>()}));
                Ok(())
            })?;
            proof.text_signatures.insert(*content, signature(&text)?);
            let mut visible = story.clone();
            visible.for_each_encoding_mut(&mut |s| {
                let anchors: Vec<_> = s.text.match_indices(designcraft_doc::ANCHOR_MARK).map(|(i, _)| i).collect();
                for i in anchors.into_iter().rev() {
                    s.delete(i..i + designcraft_doc::ANCHOR_MARK.len_utf8());
                }
                s.rev = 0;
            });
            proof.formatting.insert(*content, signature(&visible.content)?);
            if !d.publication_story_included(*sid) {
                continue;
            }
            let cs = s.cache.get(&d, *sid, None);
            let mut frames = Vec::new();
            for f in &cs.frames {
                let page = d.page_of_item(f.frame);
                frames.push(ProofFrame {
                    frame: f.frame,
                    page,
                    range: [f.range.start, f.range.end],
                    lines: lines(&f.lines, &cs.styles, &mut fonts)?,
                    nested: signature(&nested(f, &mut fonts)?)?,
                });
                if let Some(source) = cs.source.as_deref().or(Some(story)) {
                    let context = FigureContext {
                        component: *component,
                        content: *content,
                        page,
                        space: d.item(f.frame).map(|i| d.text_xf(i)).unwrap_or(designcraft_geom::Affine::IDENTITY),
                    };
                    figures(&d, source, f, context, &mut proof.figures, 0)?;
                }
            }
            proof.diagnostics.extend(cs.diagnostics.iter().cloned());
            if cs.overset_at.is_some() || !cs.annotation_overflow.is_empty() {
                proof.diagnostics.push(format!("Component {}, story {} has overset content", component.0, sid.0));
            }
            proof.stories.push(ProofStory {
                component: *component,
                content: *content,
                story: *sid,
                frames,
                overset: cs.overset_at,
                note_overflow: cs.annotation_overflow.clone(),
            });
        }
    }
    proof.fonts = fonts.into_values().collect();
    proof.diagnostics.extend(proof.edition.references.diagnostics.iter().cloned());
    proof.diagnostics.sort();
    proof.diagnostics.dedup();
    Ok(proof)
}

#[derive(Clone, Copy)]
struct FigureContext {
    component: ComponentId,
    content: designcraft_doc::ContentId,
    page: Option<usize>,
    space: designcraft_geom::Affine,
}
fn figures(
    d: &Document,
    story: &Story,
    frame: &designcraft_compose::FrameText,
    c: FigureContext,
    out: &mut Vec<ProofFigure>,
    depth: usize,
) -> Result<()> {
    if depth > 32 {
        return Err(bad("publication.proof", "nested figure depth exceeds 32"));
    }
    for o in &frame.objects {
        if let Some(item) = story.objects.get(o.index).map(|o| &o.item) {
            let r = c.space.transform_rect_bbox(designcraft_geom::Rect::new(o.origin.x, o.origin.y, o.origin.x + o.size.0, o.origin.y + o.size.1));
            out.push(ProofFigure {
                component: c.component,
                item: item.id,
                content: Some(c.content),
                page: c.page,
                bounds: [r.x0, r.y0, r.x1, r.y1],
                description: item.alt_text.clone(),
                artwork: artwork(d, item)?,
            });
        }
    }
    for table in &frame.tables {
        for cell in &table.cells {
            let leaf =
                cell.text.source.as_deref().or_else(|| story.tables.get(&table.table).and_then(|t| t.cell(cell.row, cell.col)).map(|c| &c.text));
            if let Some(leaf) = leaf {
                for f in &cell.text.frames {
                    figures(
                        d,
                        leaf,
                        f,
                        FigureContext { space: c.space * designcraft_geom::Affine::translate((cell.origin.x, cell.origin.y)), ..c },
                        out,
                        depth + 1,
                    )?;
                }
            }
        }
    }
    for note in &frame.notes {
        let leaf = note.text.source.as_deref().or(note.source_story.as_deref()).or_else(|| story.note(note.id).map(|n| &n.text));
        if let Some(leaf) = leaf {
            let d = note.text.render_document.as_deref().unwrap_or(d);
            let owner =
                note.annotation.and_then(|id| d.publication.as_ref()?.root.annotation_entries().ok()?.into_iter().find(|e| e.identity.id == id));
            let context = if let Some(owner) = owner {
                FigureContext {
                    component: owner.component,
                    content: owner.content,
                    page: if owner.component == c.component { c.page } else { None },
                    ..c
                }
            } else {
                c
            };
            for f in &note.text.frames {
                figures(
                    d,
                    leaf,
                    f,
                    FigureContext { space: c.space * designcraft_geom::Affine::translate((note.origin.x, note.origin.y)), ..context },
                    out,
                    depth + 1,
                )?;
            }
        }
    }
    Ok(())
}

fn artwork(d: &Document, item: &designcraft_doc::Item) -> Result<String> {
    let asset = item.graphic().and_then(|g| d.assets.get(&g.asset)).map(|a| fingerprint(&a.data));
    signature(&json!({"content":item.content,"path":item.path,"fill":item.fill,"stroke":item.stroke,"effects":item.effects,"asset":asset}))
}
fn lines(
    lines: &[designcraft_compose::Line],
    styles: &[designcraft_compose::RunStyle],
    fonts: &mut BTreeMap<String, FontInput>,
) -> Result<Vec<ProofLine>> {
    lines
        .iter()
        .map(|l| {
            let glyphs = l
                .glyphs
                .iter()
                .filter(|g| g.visible)
                .map(|g| {
                    let key = format!("{}:{}:{}:{:?}", g.face.family, g.face.style, g.face.index(), g.face.coords);
                    let font = fonts.entry(key).or_insert_with(|| FontInput {
                        family: g.face.family.clone(),
                        style: g.face.style.clone(),
                        index: g.face.index(),
                        coordinates: g.face.coords.clone(),
                        byte_length: g.face.data().len(),
                        fingerprint: fingerprint(g.face.data()),
                    });
                    json!([
                        font.fingerprint,
                        g.gid,
                        g.x,
                        g.y,
                        g.adv,
                        g.sx,
                        g.sy,
                        g.visible,
                        g.generated.as_ref().map(|v| (&v.text, v.target)),
                        styles.get(g.style as usize).map(|style| format!("{style:?}"))
                    ])
                })
                .collect::<Vec<_>>();
            Ok(ProofLine {
                start: l.range.start,
                end: l.range.end,
                paragraph: l.para,
                baseline: l.baseline,
                x: [l.x0, l.x1],
                glyphs: signature(&glyphs)?,
            })
        })
        .collect()
}
fn nested(f: &designcraft_compose::FrameText, fonts: &mut BTreeMap<String, FontInput>) -> Result<Value> {
    let tables = f
        .tables
        .iter()
        .map(|t| {
            let cells = t
                .cells
                .iter()
                .map(|c| Ok(json!({"row":c.row,"col":c.col,"rect":c.rect,"origin":c.origin,"overset":c.overset,"text":composition(&c.text,fonts)?})))
                .collect::<Result<Vec<_>>>()?;
            Ok(json!({"table":t.table,"rect":t.rect,"first":t.first,"last":t.last,"cells":cells}))
        })
        .collect::<Result<Vec<_>>>()?;
    let notes = f
        .notes
        .iter()
        .map(|n| Ok(json!({"id":n.annotation.unwrap_or(n.id),"label":n.label,"origin":n.origin,"rect":n.rect,"text":composition(&n.text,fonts)?})))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"tables":tables,"notes":notes,"objects":f.objects.iter().map(|o|json!([o.index,o.origin,o.size])).collect::<Vec<_>>() }))
}
fn composition(c: &designcraft_compose::ComposedStory, fonts: &mut BTreeMap<String, FontInput>) -> Result<Value> {
    c.frames
        .iter()
        .map(|f| {
            let mut lines = lines(&f.lines, &c.styles, fonts)?;
            for line in &mut lines {
                line.start = 0;
                line.end = 0;
            }
            Ok(json!({"lines":lines,"nested":nested(f,fonts)?}))
        })
        .collect::<Result<Vec<_>>>()
        .map(|v| json!(v))
}

fn compare(s: &mut Session, p: &Value) -> Result<Value> {
    let v = view(s)?;
    let before = v.root.review.proofs.get(&required(p, "before")?).ok_or_else(|| bad("publication.proof.compare", "unknown before proof"))?;
    let current;
    let after = if let Some(id) = optional_id(p, "after")? {
        v.root.review.proofs.get(&id).ok_or_else(|| bad("publication.proof.compare", "unknown after proof"))?.as_ref()
    } else {
        current = snapshot(s, v.clone())?;
        &current
    };
    let mut rows = Vec::new();
    let all: BTreeSet<_> = before.content.keys().chain(after.content.keys()).copied().collect();
    for content in all {
        let owner = after
            .components
            .values()
            .chain(before.components.values())
            .find_map(|c| c.stories.iter().find(|(_, id)| **id == content).map(|(sid, _)| (c.id, *sid)));
        let Some((component, story)) = owner else {
            continue;
        };
        let text_changed = before.text_signatures.get(&content) != after.text_signatures.get(&content);
        if text_changed {
            let (path, leaf_start, old, new) = changed_leaf(before.content.get(&content), after.content.get(&content))?;
            let start = after
                .content
                .get(&content)
                .map(|c| {
                    let mut s = Story::new(story);
                    s.content = c.clone();
                    outer_start(&s, &path, leaf_start)
                })
                .unwrap_or(0);
            rows.push(json!({"kind":"content","component":component,"story":story,"content":content,"start":start,"path":path,"leafStart":leaf_start,"before":old,"after":new}));
        }
        if !text_changed && before.formatting.get(&content) != after.formatting.get(&content) {
            rows.push(json!({"kind":"formatting","component":component,"story":story,"content":content}));
        }
        let a = before.stories.iter().find(|s| s.content == content);
        let b = after.stories.iter().find(|s| s.content == content);
        if pagination(a) != pagination(b) {
            rows.push(
                json!({"kind":"pagination","component":component,"story":story,"content":content,"before":pagination(a),"after":pagination(b)}),
            );
        }
    }
    for c in after.components.keys().chain(before.components.keys()).copied().collect::<BTreeSet<_>>() {
        if before.settings.get(&c) != after.settings.get(&c) {
            rows.push(json!({"kind":"formatting","component":c,"detail":"Styles or component composition settings changed"}));
        }
    }
    let figures: BTreeSet<_> = before.figures.iter().chain(&after.figures).map(|f| (f.component, f.item, f.content)).collect();
    for (component, item, content) in figures {
        let find = |p: &Proof| p.figures.iter().find(|f| (f.component, f.item, f.content) == (component, item, content)).cloned();
        let a = find(before);
        let b = find(after);
        if a != b {
            rows.push(json!({"kind":if a.as_ref().zip(b.as_ref()).is_some_and(|(a,b)|a.artwork==b.artwork){"movedFigure"}else{"figureContent"},"component":component,"item":item,"content":content,"page":b.as_ref().and_then(|f|f.page),"before":a,"after":b}));
        }
    }
    for id in before.edition.references.fields.keys().chain(after.edition.references.fields.keys()).copied().collect::<BTreeSet<_>>() {
        let a = before.edition.references.fields.get(&id);
        let b = after.edition.references.fields.get(&id);
        if a != b {
            let mut row =
                reference_source(after, id).or_else(|| reference_source(before, id)).unwrap_or_else(|| json!({"component":after.order.first()}));
            row["kind"] = json!("reference");
            row["id"] = json!(id);
            row["before"] = json!(a);
            row["after"] = json!(b);
            rows.push(row);
        }
    }
    for id in before.edition.references.locations.keys().chain(after.edition.references.locations.keys()).copied().collect::<BTreeSet<_>>() {
        let a = before.edition.references.locations.get(&id);
        let b = after.edition.references.locations.get(&id);
        let visible =
            |l: Option<&designcraft_doc::publication::references::Location>| l.map(|l| (l.component, l.page, l.global_page, l.label.clone()));
        if visible(a) != visible(b) {
            let loc = b.or(a);
            rows.push(
                json!({"kind":"referenceResolution","id":id,"component":loc.map(|l|l.component),"page":loc.map(|l|l.page),"before":a,"after":b}),
            );
        }
    }
    for (name, index) in &before.indexes {
        for o in index.occurrences.values() {
            let before_exists = passage_exists(before, &o.start, o.end.as_ref().map(|e| &e.target));
            let after_exists = passage_exists(after, &o.start, o.end.as_ref().map(|e| &e.target));
            if before_exists && !after_exists {
                rows.push(json!({"kind":"indexPassageRemoved","index":name,"occurrence":o.id,"component":o.start.component,"content":o.start.content,"anchor":o.start.anchor,"topic":index.topics.get(&o.topic).map(|t|&t.display)}));
            }
        }
    }
    if before.order != after.order {
        rows.push(json!({"kind":"componentOrder","before":before.order,"after":after.order}));
    }
    if before.fonts != after.fonts || before.inputs != after.inputs {
        rows.push(json!({"kind":"compositionInputs","before":before.fonts,"after":after.fonts}));
    }
    if before.title != after.title
        || before.metadata != after.metadata
        || before.values != after.values
        || before.edition.output != after.edition.output
        || before.edition.note_policies != after.edition.note_policies
        || before.edition.apparatus_witness != after.edition.apparatus_witness
    {
        rows.push(json!({"kind":"editionSettings","detail":"Publication metadata, values or edition output/note/witness settings changed"}));
    }
    for (i, r) in rows.iter_mut().enumerate() {
        r["row"] = json!(i);
        if let Some(c) =
            r["component"].as_u64().and_then(|id| after.components.get(&ComponentId(id)).or_else(|| before.components.get(&ComponentId(id))))
        {
            r["componentTitle"] = json!(c.title);
        }
    }
    Ok(
        json!({"before":before.id,"after":after.id,"beforeRevision":before.revision,"afterRevision":after.revision,"rows":rows,"diagnostics":after.diagnostics}),
    )
}
fn excerpt(s: Option<&Arc<designcraft_doc::StoryContent>>) -> String {
    s.map(|s| s.text.chars().filter(|c| !('\u{e000}'..='\u{e01f}').contains(c)).take(240).collect()).unwrap_or_default()
}
fn pagination(s: Option<&ProofStory>) -> Value {
    json!(s.map(|s| {
        (
            &s.overset,
            &s.note_overflow,
            s.frames
                .iter()
                .map(|f| json!({"frame":f.frame,"page":f.page,"lines":f.lines.iter().map(|l|(l.baseline,l.x)).collect::<Vec<_>>(),"nested":f.nested}))
                .collect::<Vec<_>>(),
        )
    }))
}
fn first_difference(before: Option<&Arc<designcraft_doc::StoryContent>>, after: Option<&Arc<designcraft_doc::StoryContent>>) -> usize {
    let Some(after) = after else { return 0 };
    let mut a = before.map(|s| s.text.as_str()).unwrap_or_default().chars().filter(|c| *c != designcraft_doc::ANCHOR_MARK);
    for (i, c) in after.text.char_indices().filter(|(_, c)| *c != designcraft_doc::ANCHOR_MARK) {
        if a.next() != Some(c) {
            return i;
        }
    }
    after.text.len()
}
fn changed_leaf(
    before: Option<&Arc<designcraft_doc::StoryContent>>,
    after: Option<&Arc<designcraft_doc::StoryContent>>,
) -> Result<(Vec<PathStep>, usize, String, String)> {
    let leaves = |content: Option<&Arc<designcraft_doc::StoryContent>>| -> Result<BTreeMap<Vec<PathStep>, Arc<designcraft_doc::StoryContent>>> {
        let mut map = BTreeMap::new();
        if let Some(content) = content {
            let mut s = Story::new(StoryId(0));
            s.content = content.clone();
            visit_text(&s, &mut Vec::new(), &mut |s, p| {
                map.insert(p.to_vec(), s.content.clone());
                Ok(())
            })?;
        }
        Ok(map)
    };
    let a = leaves(before)?;
    let b = leaves(after)?;
    for path in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
        let old = a.get(path);
        let new = b.get(path);
        if old.map(|s| s.text.replace(designcraft_doc::ANCHOR_MARK, "")) != new.map(|s| s.text.replace(designcraft_doc::ANCHOR_MARK, "")) {
            return Ok((path.clone(), first_difference(old, new), excerpt(old), excerpt(new)));
        }
    }
    Ok((Vec::new(), first_difference(before, after), excerpt(before), excerpt(after)))
}
fn outer_start(story: &Story, path: &[PathStep], leaf: usize) -> usize {
    match path.first() {
        None => leaf,
        Some(PathStep::Cell { table, .. }) => story.table_anchor(*table).unwrap_or(0),
        Some(PathStep::Footnote { id }) => story.note_anchor(*id).unwrap_or(0),
        Some(PathStep::Endnote { id }) => {
            story.text.match_indices(designcraft_doc::ENDNOTE_REF).zip(&story.endnotes).find(|(_, n)| n.id == *id).map(|((i, _), _)| i).unwrap_or(0)
        }
    }
}
fn reference_source(p: &Proof, id: u64) -> Option<Value> {
    for component in p.components.values() {
        for (sid, cid) in &component.stories {
            let Some(content) = p.content.get(cid) else { continue };
            let mut story = Story::new(*sid);
            story.content = content.clone();
            let mut found = None;
            let _ = visit_text(&story, &mut Vec::new(), &mut |s, path| {
                if found.is_none()
                    && let Some((offset, _)) = s.text.match_indices(designcraft_doc::XREF_MARK).zip(&s.xrefs).find(|(_, x)| x.field == Some(id))
                {
                    let start = outer_start(&story, path, offset.0);
                    found = Some(json!({"component":component.id,"story":sid,"start":start,"path":path,"leafStart":offset.0}));
                }
                Ok(())
            });
            if found.is_some() {
                return found;
            }
        }
    }
    None
}
fn passage_exists(p: &Proof, t: &TextTarget, end: Option<&TextTarget>) -> bool {
    let Some(content) = p.content.get(&t.content) else {
        return false;
    };
    let mut s = Story::new(StoryId(0));
    s.content = content.clone();
    let mut found = false;
    let _ = visit_text(&s, &mut Vec::new(), &mut |s, _| {
        if let Some(start) = s.anchor_pos(t.anchor) {
            found = if let Some(end) = end {
                s.anchor_pos(end.anchor)
                    .filter(|stop| *stop > start)
                    .is_some_and(|stop| !designcraft_doc::xref::clean(s.text.get(start..stop).unwrap_or_default()).trim().is_empty())
            } else {
                true
            };
        }
        Ok(())
    });
    found
}
fn go_to(s: &mut Session, p: &Value) -> Result<Value> {
    let mut v = view(s)?;
    v.component = ComponentId(required(p, "component")?);
    let d = v.project()?;
    let mut selection = Selection::default();
    let mut note_id = None;
    let mut page = offset(p, "page")?.filter(|p| *p < d.page_count());
    if let Some(item) = optional_id(p, "item")?.map(ItemId).filter(|i| d.item(*i).is_some()) {
        selection = Selection::items(vec![item]);
        page = d.page_of_item(item).or(page);
    }
    if let Some(story) = optional_id(p, "story")?.map(StoryId).and_then(|id| d.story(id)) {
        let start = designcraft_doc::story::floor_char_boundary(&story.text, offset(p, "start")?.unwrap_or(0).min(story.len()));
        let cs = s.cache.get(&d, story.id, None);
        let frame = cs.frames.iter().find(|f| f.range.contains(&start)).map(|f| f.frame).or_else(|| story.frames.first().copied());
        page = frame.and_then(|f| d.page_of_item(f)).or(page);
        selection = Selection::text(TextSel { frame, ..TextSel::caret(story.id, start) });
        let path: Vec<PathStep> =
            serde_json::from_value(p.get("path").cloned().unwrap_or(json!([]))).map_err(|e| bad("publication.proof.goTo", e.to_string()))?;
        let leaf_start = offset(p, "leafStart")?.unwrap_or(0);
        for entry in v.root.annotation_entries()?.into_iter().filter(|e| e.component == v.component && e.story == story.id) {
            let mut note_path = entry.path.clone();
            note_path.push(if entry.endnote { PathStep::Endnote { id: entry.note.id } } else { PathStep::Footnote { id: entry.note.id } });
            if path == note_path {
                let start = designcraft_doc::story::floor_char_boundary(&entry.note.text.text, leaf_start.min(entry.note.text.len()));
                note_id = Some(entry.identity.id);
                selection = Selection::text(TextSel {
                    frame,
                    cell: Some(designcraft_doc::CellAddr::annotation(entry.identity.id)),
                    ..TextSel::caret(story.id, start)
                });
                break;
            }
        }
        if let [PathStep::Cell { table, row, col }] = path.as_slice()
            && let Some(cell) = story.tables.get(table).and_then(|t| t.cell(*row, *col))
        {
            let start = designcraft_doc::story::floor_char_boundary(&cell.text.text, leaf_start.min(cell.text.len()));
            selection = Selection::text(TextSel {
                frame,
                cell: Some(designcraft_doc::CellAddr { table: *table, row: *row, col: *col }),
                ..TextSel::caret(story.id, start)
            });
        }
    }
    let state = s.doc_mut()?;
    state.doc = Arc::new(d);
    state.selection = selection;
    state.revision = state.revision.saturating_add(1);
    Ok(json!({"component":v.component,"page":page.unwrap_or(0),"noteOwnerActivated":note_id.is_some(),"id":note_id}))
}
