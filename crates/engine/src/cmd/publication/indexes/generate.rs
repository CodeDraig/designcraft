use super::*;
use designcraft_doc::{EditionId, ParaFormat, SpreadRef};

pub(super) fn check_styles(d: &Document, f: &Formatting) -> Result<()> {
    for style in std::iter::once(&f.title_style).chain(std::iter::once(&f.section_style)).chain(f.level_styles.values()) {
        if d.styles.para(style).is_none() {
            return Err(bad("publication.index", format!("unknown paragraph style {style}")));
        }
    }
    Ok(())
}
pub(super) fn destination(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name(p)?.to_string();
    let requested = optional_id(p, "component")?.map(ComponentId);
    mutate(s, |r, v| {
        let mut index = get(r, &name)?.clone();
        if let Some(dest) = &index.destination {
            if requested.is_some_and(|c| c != dest.component) {
                return Err(bad("publication.index", "existing index retains its destination; detach first to create a new one"));
            }
        } else {
            let component = requested.unwrap_or(v.component);
            let mut d = r.project(v.edition, component)?;
            check_styles(&d, &index.format)?;
            let page = if let Some(page) = p.get("page") {
                page.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .map(|n| n - 1)
                    .ok_or_else(|| bad("publication.index", "page must be a positive page number"))?
            } else {
                let page = d.page_count();
                d.insert_pages(page.checked_sub(1), 1, None)?;
                page
            };
            let (si, pi) = d.page_loc(page).ok_or_else(|| bad("publication.index", "new page missing"))?;
            let rect = crate::cmd::rect_param(p, "rect").unwrap_or_else(|| d.spreads[si].pages[pi].margin_rect());
            let layer = d.default_layer();
            let (_, story) = d.add_text_frame(SpreadRef::Doc(si), rect, layer, "", Default::default())?;
            r.capture(v.edition, component, &d)?;
            let content =
                *r.components.get(&component).and_then(|c| c.stories.get(&story)).ok_or_else(|| bad("publication.index", "new content missing"))?;
            index.destination = Some(Destination { component, story, content });
        }
        let result = json!(index.destination);
        r.indexes.insert(name, index);
        r.revision = r.revision.saturating_add(1);
        Ok(result)
    })
}

pub(in super::super) fn regenerate(root: &mut Publication, edition: EditionId) -> Result<()> {
    let names: Vec<_> = root.indexes.keys().cloned().collect();
    for name in names {
        let mut index = get(root, &name)?.clone();
        let Some(dest) = index.destination.clone() else { continue };
        let mut d = root.project(edition, dest.component)?;
        let layout = &root.editions.get(&edition).ok_or_else(|| bad("publication.index", "missing edition"))?.references;
        let rows = render_entries(root, &index, layout)?;
        let previous = d.story(dest.story).cloned().ok_or_else(|| bad("publication.index", "index destination missing"))?;
        let mut story =
            Story::with_text(dest.story, &index.format.title, ParaFormat { style: index.format.title_style.clone(), ..Default::default() });
        for row in rows {
            if let Some(heading) = row.heading {
                let at = story.len();
                story.insert(at, &format!("\n{heading}"));
                if let Some(p) = story.paras.last_mut() {
                    p.style = index.format.section_style.clone();
                }
            }
            let field = match index.fields.get(&row.topic) {
                Some(id) => *id,
                None => root.alloc()?,
            };
            index.fields.insert(row.topic, field);
            let at = story.len();
            story.insert(at, "\n");
            let at = story.len();
            story.insert_xref(at, designcraft_doc::xref::CrossRef { target: 0, format: String::new(), field: Some(field) });
            if let Some(p) = story.paras.last_mut() {
                p.style = index.format.level_styles.get(&row.level).cloned().unwrap_or_else(|| designcraft_doc::story::BASIC_PARAGRAPH.into());
            }
        }
        carry_entry_formats(&previous, &mut story);
        index.destination = Some(dest.clone());
        root.indexes.insert(name, index);
        if previous.text != story.text || previous.xrefs != story.xrefs {
            let target = d.story_mut(dest.story).ok_or_else(|| bad("publication.index", "missing index story"))?;
            target.content = story.content;
            target.rev = target.rev.saturating_add(1);
            root.capture(edition, dest.component, &d)?;
        }
    }
    Ok(())
}

pub(in super::super) fn evaluate(root: &Publication, layout: &mut ReferenceLayout) -> Result<()> {
    for index in root.indexes.values() {
        let rows = render_entries(root, index, layout)?;
        let mut by_topic: std::collections::BTreeMap<u64, Vec<Dependency>> = std::collections::BTreeMap::new();
        for occurrence in index.occurrences.values() {
            let mut topic = occurrence.topic;
            if index.format.run_in {
                for _ in 0..16 {
                    match index.topics.get(&topic).and_then(|t| t.parent) {
                        Some(parent) => topic = parent,
                        None => break,
                    }
                }
            }
            let dependencies = by_topic.entry(topic).or_default();
            dependencies.push(Dependency::Entity(occurrence.id));
            dependencies.push(Dependency::Pagination(occurrence.start.component));
            if let Some(end) = &occurrence.end {
                dependencies.push(Dependency::Entity(end.id));
                dependencies.push(Dependency::Pagination(end.target.component));
            }
        }
        for row in rows {
            let Some(&field) = index.fields.get(&row.topic) else { continue };
            layout.diagnostics.extend(row.text.unresolved.iter().cloned());
            layout.fields.insert(field, FieldValues { text: row.text, ..Default::default() });
            let mut dependencies = by_topic.remove(&row.topic).unwrap_or_default();
            dependencies.push(Dependency::Index(index.name.clone()));
            layout.dependencies.insert(field, dependencies);
        }
    }
    if layout.fields.values().map(|v| v.text.runs.iter().map(|r| r.text.len()).sum::<usize>()).sum::<usize>() > 64 * 1024 * 1024 {
        return Err(bad("publication.index", "generated text exceeds 64 MiB in this edition"));
    }
    layout.diagnostics.sort();
    layout.diagnostics.dedup();
    Ok(())
}

pub(super) fn detach(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name(p)?.to_string();
    mutate(s, |r, v| {
        let index = get(r, &name)?.clone();
        let dest = index.destination.as_ref().ok_or_else(|| bad("publication.index", "no generated destination"))?;
        let layout = &r.editions.get(&v.edition).ok_or_else(|| bad("publication.index", "missing edition"))?.references;
        let values: Vec<_> = index.fields.values().filter_map(|id| layout.fields.get(id).map(|f| (*id, f.text.clone()))).collect();
        let mut d = r.project(v.edition, dest.component)?;
        let story = d.story_mut(dest.story).ok_or_else(|| bad("publication.index", "missing generated story"))?;
        let entries: Vec<_> = story.text.match_indices(designcraft_doc::XREF_MARK).zip(&story.xrefs).map(|((at, _), x)| (at, x.field)).collect();
        for (at, field) in entries.into_iter().rev() {
            let text = field
                .and_then(|id| values.iter().find(|(f, _)| *f == id))
                .map(|(_, text)| text)
                .ok_or_else(|| bad("publication.index", "update index before detaching"))?;
            story.replace(at..at + designcraft_doc::XREF_MARK.len_utf8(), &text.plain());
            let mut byte = at;
            for run in &text.runs {
                story.format_chars(byte..byte + run.text.len(), |f| *f = run.format.clone());
                byte += run.text.len();
            }
        }
        get_mut(r, &name)?.destination = None;
        r.capture(v.edition, dest.component, &d)?;
        r.revision = r.revision.saturating_add(1);
        Ok(json!({"story":dest.story}))
    })
}
