use super::*;
use designcraft_doc::{EditionId, INDEX_MARK, index::IndexRange};

fn topic_path(root: &mut Publication, index: &mut Definition, parts: &[String], sort: &[String]) -> Result<u64> {
    let mut parent = None;
    for (level, display) in parts.iter().filter(|s| !s.trim().is_empty()).enumerate() {
        let id = match index.topics.values().find(|t| t.parent == parent && t.display == *display) {
            Some(t) => t.id,
            None => {
                let id = root.alloc()?;
                index.topics.insert(
                    id,
                    Topic {
                        id,
                        display: display.clone(),
                        parent,
                        sort: SortKey::Alphabetical { key: sort.get(level).cloned().unwrap_or_default() },
                        see: vec![],
                        see_also: vec![],
                    },
                );
                id
            }
        };
        parent = Some(id);
    }
    parent.ok_or_else(|| bad("publication.index.migrate", "legacy marker has no topic"))
}

fn migrate_text(
    root: &mut Publication,
    index: &mut Definition,
    component: ComponentId,
    content: designcraft_doc::ContentId,
    st: &mut Story,
    next_local: &mut u64,
) -> Result<()> {
    let markers: Vec<_> = st.text.match_indices(INDEX_MARK).zip(&st.index_refs).map(|((at, _), r)| (at, (**r).clone())).collect();
    for (at, r) in markers.into_iter().rev() {
        let topic = topic_path(root, index, &r.topics, &r.sort)?;
        match &r.range {
            IndexRange::See(name) | IndexRange::SeeAlso(name) => {
                let parts: Vec<_> = name.split('>').map(|s| s.trim().to_string()).collect();
                let target = topic_path(root, index, &parts, &[])?;
                let entry = index.topics.get_mut(&topic).ok_or_else(|| bad("publication.index.migrate", "topic missing"))?;
                let aliases = if matches!(r.range, IndexRange::See(_)) { &mut entry.see } else { &mut entry.see_also };
                if !aliases.contains(&target) {
                    aliases.push(target);
                }
            }
            _ => {}
        }
        let end = match r.range {
            IndexRange::ToEndOfStory => Some(st.len()),
            IndexRange::NextParagraphs(n) => {
                let ranges = st.para_ranges();
                ranges.get(st.para_at(at).saturating_add(n as usize).min(ranges.len().saturating_sub(1))).map(|r| r.end)
            }
            _ => None,
        };
        st.delete(at..at + INDEX_MARK.len_utf8());
        if matches!(r.range, IndexRange::See(_)) {
            continue;
        }
        let target = |anchor| TextTarget { component, content, anchor };
        let id = root.alloc()?;
        let end = match end.map(|p| p.saturating_sub(INDEX_MARK.len_utf8())).filter(|p| *p > at) {
            Some(end) => Some(Endpoint { id: root.alloc()?, target: target(st.insert_anchor(end, "Migrated index range end")) }),
            None => None,
        };
        let start = target(insert_anchor(st, at, "Migrated index occurrence", reserve_anchor(next_local)?));
        index.occurrences.insert(id, Occurrence { id, topic, start, end, suppress_pages: matches!(r.range, IndexRange::SuppressPageRange) });
    }
    Ok(())
}

/// Legacy documents are left intact on disk. Their in-memory publication copies gain explicit
/// ranges and named destinations. Repeating conversion after migration is a no-op.
pub(in super::super) fn migrate_legacy(root: &mut Publication, edition: EditionId) -> Result<usize> {
    let mut count = 0usize;
    for component in root.ordered_components()? {
        if root.indexes.values().any(|i| i.migrated.contains(&component)) {
            continue;
        }
        let mut d = root.project(edition, component)?;
        let mut found = false;
        for st in d.stories.values() {
            let mut pending = vec![st.as_ref()];
            while let Some(s) = pending.pop() {
                found |= !s.index_refs.is_empty();
                pending.extend(s.tables.values().flat_map(|t| &t.cells).map(|c| &c.text));
                pending.extend(s.notes.iter().chain(&s.endnotes).map(|n| &n.text));
            }
        }
        if !found && d.index.is_none() {
            continue;
        }
        let name = if root.indexes.contains_key("Index") { format!("Index ({} · {})", d.title, component.0) } else { "Index".into() };
        let mut index = Definition { name: name.clone(), ..Default::default() };
        if let Some(spec) = &d.index {
            index.format.title = spec.title.clone();
            index.format.run_in = spec.run_in;
            index.format.section_headings = spec.section_headings;
            index.format.following_topic = spec.following_topic.clone();
            index.format.between_pages = spec.between_pages.clone();
            index.format.page_range = spec.page_range.clone();
            index.format.between_entries = spec.between_entries.clone();
            index.format.before_cross_ref = spec.before_cross_ref.clone();
            index.format.title_style = "Index Title".into();
            index.format.section_style = "Index Section Head".into();
            index.format.level_styles = (1..=4).map(|n| (n, format!("Index Level {n}"))).collect();
            if let Some(content) = root.components.get(&component).and_then(|c| c.stories.get(&spec.story)) {
                index.destination = Some(Destination { component, story: spec.story, content: *content });
            }
        }
        let mut next_local = d.next_id;
        for (&sid, story) in &mut d.stories {
            let content = *root
                .components
                .get(&component)
                .and_then(|c| c.stories.get(&sid))
                .ok_or_else(|| bad("publication.index.migrate", "missing source story"))?;
            let mut error = None;
            Arc::make_mut(story).for_each_text_mut(&mut |text| {
                if error.is_none()
                    && let Err(e) = migrate_text(root, &mut index, component, content, text, &mut next_local)
                {
                    error = Some(e);
                }
            });
            if let Some(error) = error {
                return Err(error);
            }
        }
        d.next_id = next_local;
        index.migrated.insert(component);
        d.index = None;
        root.capture(edition, component, &d)?;
        root.indexes.insert(name, index);
        // Legacy generator metadata belongs to an edition's page model. Clear it in each
        // edition after the canonical markers have been migrated, keeping every frame.
        let editions: Vec<_> = root.editions.keys().copied().filter(|id| *id != edition).collect();
        for other in editions {
            let mut d = root.project(other, component)?;
            d.index = None;
            root.capture(other, component, &d)?;
        }
        root.revision = root.revision.saturating_add(1);
        count += 1;
    }
    Ok(count)
}
