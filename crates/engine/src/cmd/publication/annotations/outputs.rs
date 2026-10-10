use super::*;
use designcraft_doc::{
    Document, EditionId, SpreadRef, Story,
    publication::references::{Location, ReferenceLayout},
};
use std::collections::BTreeMap;

fn key(stream: &str, source: Option<ComponentId>) -> String {
    format!("{}:{}:{}", stream.len(), stream, source.map_or("all".into(), |c| c.0.to_string()))
}

pub(in super::super) fn prepare(root: &mut Publication, edition: EditionId) -> Result<()> {
    // Capture performs identity allocation on legacy canonical notes as well as new insertions.
    for component in root.ordered_components()? {
        let d = root.project(edition, component)?;
        root.capture(edition, component, &d)?;
    }
    let entries = root.annotation_entries()?;
    let last = root.ordered_components()?.last().copied();
    let mut needed = BTreeMap::new();
    for e in &entries {
        let placement = root.note_policy(edition, &e.identity.stream).placement;
        if matches!(placement, Placement::ComponentEnd | Placement::PublicationEnd) {
            let source = (placement == Placement::ComponentEnd).then_some(e.component);
            let dest = source.or(last).ok_or_else(|| bad("publication.notes", "missing destination component"))?;
            needed.insert(key(&e.identity.stream, source), (e.identity.stream.clone(), source, dest));
        }
    }
    // Reuse existing legacy endnote frames, preserving their initial position and geometry.
    for component in root.ordered_components()? {
        let mut d = root.project(edition, component)?;
        if let Some(sid) = d.endnote_story {
            let stream = entries.iter().find(|e| e.component == component && e.endnote).map(|e| e.identity.stream.clone());
            if let Some(stream) = stream {
                let k = key(&stream, Some(component));
                if !root.note_outputs.contains_key(&k) {
                    let st = d.story_mut(sid).ok_or_else(|| bad("publication.notes", "legacy endnote story missing"))?;
                    let frames = st.frames.clone();
                    *st = Story { frames, ..Story::new(sid) };
                    d.endnote_story = None;
                    root.capture(edition, component, &d)?;
                    let content = *root
                        .components
                        .get(&component)
                        .and_then(|c| c.stories.get(&sid))
                        .ok_or_else(|| bad("publication.notes", "legacy output missing"))?;
                    root.note_outputs.insert(k, Output { stream, source: Some(component), component, story: sid, content });
                    root.clear_legacy_endnote_output(component);
                }
            }
        }
    }
    for (key, (stream, source, component)) in needed {
        if root.note_outputs.contains_key(&key) {
            continue;
        }
        let mut d = root.project(edition, component)?;
        let page = d.page_count();
        d.insert_pages(page.checked_sub(1), 1, None)?;
        let (si, pi) = d.page_loc(page).ok_or_else(|| bad("publication.notes", "note page missing"))?;
        let rect = d.spreads[si].pages[pi].margin_rect();
        let layer = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(si), rect, layer, "", ParaFormat::default())?;
        root.capture(edition, component, &d)?;
        let content =
            *root.components.get(&component).and_then(|c| c.stories.get(&sid)).ok_or_else(|| bad("publication.notes", "note output missing"))?;
        root.note_outputs.insert(key, Output { stream, source, component, story: sid, content });
        root.revision = root.revision.saturating_add(1);
    }
    Ok(())
}

pub(in super::super) fn number(root: &mut Publication, edition: EditionId, locations: &ReferenceLayout) -> Result<bool> {
    let mut counters = BTreeMap::new();
    let mut labels = BTreeMap::new();
    let mut streams = BTreeMap::new();
    for e in root.annotation_entries()? {
        let stream = root.note_streams.get(&e.identity.stream).ok_or_else(|| bad("publication.notes", "note stream missing"))?;
        let location = locations.locations.get(&e.identity.callout);
        let scope = match stream.restart {
            Restart::Publication => (0, 0),
            Restart::Component => (e.component.0, 0),
            Restart::Page => (e.component.0, location.map_or(0, |l| l.page)),
            Restart::Spread => {
                let page = location.map_or(0, |l| l.page);
                let d = root.project(edition, e.component)?;
                (e.component.0, d.page_loc(page).map_or(0, |(si, _)| si))
            }
            Restart::Section => {
                let page = location.map_or(0, |l| l.page);
                let d = root.project(edition, e.component)?;
                (e.component.0, d.section_of(page).map_or(0, |s| s.start))
            }
        };
        let n = counters.entry((stream.name.clone(), scope)).or_insert(stream.start_at);
        labels.insert(e.identity.id, stream.number(*n)?);
        streams.insert(e.identity.id, e.identity.stream.clone());
        *n = n.checked_add(1).ok_or_else(|| bad("publication.notes", "note numbering exhausted"))?;
    }
    let e = Arc::make_mut(root.editions.get_mut(&edition).ok_or_else(|| bad("publication.notes", "edition missing"))?);
    let changed = e.notes.labels != labels || e.notes.streams != streams;
    e.notes.labels = labels;
    e.notes.streams = streams;
    Ok(changed)
}

pub(in super::super) fn locations(
    d: &Document,
    cs: &designcraft_compose::ComposedStory,
    component: ComponentId,
    global: usize,
    out: &mut ReferenceLayout,
) {
    fn walk(
        d: &Document,
        ft: &designcraft_compose::FrameText,
        component: ComponentId,
        page: usize,
        global: usize,
        out: &mut ReferenceLayout,
        ids: &BTreeMap<u64, u64>,
        depth: usize,
    ) {
        if depth > 32 {
            return;
        }
        for line in &ft.lines {
            for g in &line.glyphs {
                if let Some(callout) = g.generated.as_ref().and_then(|g| g.target).and_then(|id| ids.get(&id)) {
                    out.locations.entry(*callout).or_insert_with(|| Location {
                        component,
                        page,
                        global_page: global + page,
                        label: d.page_name(page),
                        order: vec![g.byte],
                    });
                }
            }
        }
        for n in &ft.notes {
            if let Some(id) = n.annotation {
                out.locations.entry(id).or_insert_with(|| Location {
                    component,
                    page,
                    global_page: global + page,
                    label: d.page_name(page),
                    order: vec![n.source_range.start],
                });
            }
            for nested in &n.text.frames {
                walk(d, nested, component, page, global, out, ids, depth + 1);
            }
        }
        for table in &ft.tables {
            for cell in &table.cells {
                for f in &cell.text.frames {
                    walk(d, f, component, page, global, out, ids, depth + 1);
                }
            }
        }
    }
    let ids = d
        .publication
        .as_ref()
        .and_then(|v| v.root.annotation_entries().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|e| (e.identity.id, e.identity.callout))
        .collect();
    for ft in &cs.frames {
        if let Some(page) = d.page_of_item(ft.frame) {
            walk(d, ft, component, page, global, out, &ids, 0);
        }
    }
}
