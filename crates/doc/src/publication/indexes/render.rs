use super::*;
use crate::publication::references::{FieldText, ReferenceLayout, RichRun};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrencePreview {
    pub id: u64,
    pub topic: u64,
    pub start: Option<Location>,
    pub end: Option<Location>,
    pub excerpt: String,
    pub problem: Option<String>,
    pub suppress_pages: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub name: String,
    pub topics: Vec<Topic>,
    pub occurrences: Vec<OccurrencePreview>,
    pub rows: Vec<Row>,
    pub stale: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub topic: u64,
    pub level: usize,
    pub heading: Option<String>,
    pub text: FieldText,
}

fn anchor_text<'a>(root: &'a Publication, target: &TextTarget) -> Option<(&'a crate::StoryContent, usize)> {
    let top = root.content.get(&target.content)?;
    let mut pending = vec![top.as_ref()];
    while let Some(content) = pending.pop() {
        if let Some(n) = content.anchors.iter().position(|a| a.id == target.anchor) {
            let (at, _) = content.text.match_indices(crate::ANCHOR_MARK).nth(n)?;
            return Some((content, at));
        }
        pending.extend(content.tables.values().flat_map(|t| &t.cells).map(|c| c.text.content.as_ref()));
        pending.extend(content.notes.iter().chain(&content.endnotes).map(|n| n.text.content.as_ref()));
    }
    None
}

fn occurrence(root: &Publication, o: &Occurrence, layout: &ReferenceLayout, order: &BTreeMap<ComponentId, usize>) -> OccurrencePreview {
    let start = root.target_text(&o.start);
    let end = o.end.as_ref().and_then(|e| root.target_text(&e.target));
    let mut problem = None;
    let mut excerpt = start.as_ref().map(|s| s.title.chars().take(160).collect()).unwrap_or_default();
    if start.is_none() {
        problem = Some("Start anchor or source passage was removed".into());
    }
    if let Some(endpoint) = &o.end {
        if end.is_none() {
            problem = Some("End anchor or source passage was removed".into());
        }
        if let (Some(a), Some(b)) = (&start, &end) {
            let path = |target: &TextTarget, t: &super::super::numbering::TargetText| {
                (
                    order.get(&target.component).copied().unwrap_or(usize::MAX),
                    root.components.get(&target.component).and_then(|c| c.story_order.iter().position(|id| *id == t.story)).unwrap_or(usize::MAX),
                    t.path.clone(),
                )
            };
            if path(&o.start, a) >= path(&endpoint.target, b) {
                problem = Some("Range endpoints are reversed or collapsed".into());
            }
        }
        if let (Some((a, start)), Some((b, end))) = (anchor_text(root, &o.start), anchor_text(root, &endpoint.target))
            && std::ptr::eq(a, b)
            && start < end
        {
            let text = a.text.get(start.saturating_add(crate::ANCHOR_MARK.len_utf8())..end).unwrap_or("");
            let plain = crate::xref::clean(text);
            if plain.is_empty() {
                problem = Some("The indexed passage was removed between its anchors".into());
            }
            excerpt = plain.chars().take(160).collect();
        }
    }
    let a = layout.locations.get(&o.id).cloned();
    let b = o.end.as_ref().and_then(|e| layout.locations.get(&e.id)).cloned();
    if problem.is_none() && !o.suppress_pages && (a.is_none() || o.end.is_some() && b.is_none()) {
        problem = Some("Destination is unplaced or needs an update".into());
    }
    if problem.is_none() && matches!((&a,&b),(Some(a),Some(b)) if a.global_page>b.global_page) {
        problem = Some("Range endpoints print in reverse order in this edition".into());
    }
    OccurrencePreview { id: o.id, topic: o.topic, start: a, end: b, excerpt, problem, suppress_pages: o.suppress_pages }
}

pub fn preview(root: &Publication, index: &Definition, layout: &ReferenceLayout) -> Result<Preview> {
    let order = root.ordered_components()?.into_iter().enumerate().map(|(i, c)| (c, i)).collect();
    let occurrences: Vec<_> = index.occurrences.values().map(|o| occurrence(root, o, layout, &order)).collect();
    let rows = rows(root, index, &occurrences)?;
    Ok(Preview {
        name: index.name.clone(),
        topics: index.ordered_topics()?.into_iter().filter_map(|(id, _)| index.topics.get(&id).cloned()).collect(),
        occurrences,
        rows,
        stale: layout.source_revision != Some(root.revision),
    })
}

pub fn render_entries(root: &Publication, index: &Definition, layout: &ReferenceLayout) -> Result<Vec<Row>> {
    Ok(preview(root, index, layout)?.rows)
}

fn path(index: &Definition, mut id: u64) -> String {
    let mut parts = Vec::new();
    for _ in 0..16 {
        let Some(topic) = index.topics.get(&id) else { break };
        parts.push(topic.display.as_str());
        match topic.parent {
            Some(parent) => id = parent,
            None => break,
        }
    }
    parts.reverse();
    parts.join(" › ")
}

fn run(text: impl Into<String>, format: &CharFormat, target: Option<u64>) -> RichRun {
    RichRun { text: text.into(), format: format.clone(), target }
}

fn entry(root: &Publication, index: &Definition, topic: &Topic, occurrences: &[&OccurrencePreview]) -> FieldText {
    let f = &index.format;
    let mut out = FieldText { runs: vec![run(&topic.display, &f.topic_format, None)], unresolved: vec![] };
    let mut spans: Vec<_> = occurrences
        .iter()
        .filter(|o| !o.suppress_pages && o.problem.is_none())
        .filter_map(|o| {
            o.start.clone().map(|a| {
                (a.clone(), o.end.clone().unwrap_or(a), o.id, index.occurrences.get(&o.id).and_then(|o| o.end.as_ref()).map_or(o.id, |e| e.id))
            })
        })
        .collect();
    spans.sort_by_key(|(a, b, _, _)| (a.global_page, b.global_page));
    let mut merged: Vec<(Location, Location, u64, u64)> = Vec::new();
    for (a, b, id, end_id) in spans {
        if let Some((_, end, _, link)) = merged.last_mut()
            && a.component == end.component
            && a.global_page <= end.global_page.saturating_add(1)
        {
            if b.global_page > end.global_page {
                *end = b;
                *link = end_id;
            }
        } else {
            merged.push((a, b, id, end_id));
        }
    }
    let mut has_pages = false;
    for (a, b, id, end_id) in merged {
        out.runs.push(run(if has_pages { &f.between_pages } else { &f.following_topic }, &f.page_format, None));
        out.runs.push(run(page_label(root, f, &a), &f.page_format, Some(id)));
        if a.global_page != b.global_page {
            out.runs.push(run(&f.page_range, &f.page_format, None));
            out.runs.push(run(page_label(root, f, &b), &f.page_format, Some(end_id)));
        }
        has_pages = true;
    }
    for occurrence in occurrences.iter().filter(|o| o.problem.is_some()) {
        out.runs.push(run(if has_pages { &f.between_pages } else { &f.following_topic }, &f.page_format, None));
        out.runs.push(run(format!("[Unresolved {}]", occurrence.id), &f.page_format, None));
        out.unresolved.push(format!("Index {} occurrence {}: {}", index.name, occurrence.id, occurrence.problem.as_deref().unwrap_or("unresolved")));
        has_pages = true;
    }
    for (label, targets) in [("See", &topic.see), ("See also", &topic.see_also)] {
        if !targets.is_empty() {
            out.runs.push(run(format!("{}{} ", if has_pages { &f.before_cross_ref } else { &f.following_topic }, label), &f.topic_format, None));
            for (i, target) in targets.iter().enumerate() {
                if i > 0 {
                    out.runs.push(run(&f.between_entries, &f.topic_format, None));
                }
                out.runs.push(run(path(index, *target), &f.topic_format, Some(*target)));
            }
            has_pages = true;
        }
    }
    out
}

fn rows(root: &Publication, index: &Definition, occurrences: &[OccurrencePreview]) -> Result<Vec<Row>> {
    let mut by_topic: BTreeMap<u64, Vec<_>> = BTreeMap::new();
    for o in occurrences {
        by_topic.entry(o.topic).or_default().push(o);
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut heading = String::new();
    for (id, level) in index.ordered_topics()? {
        let topic = index.topics.get(&id).ok_or_else(|| invalid("missing index topic"))?;
        let text = entry(root, index, topic, by_topic.get(&id).map_or(&[], Vec::as_slice));
        if index.format.run_in
            && level > 1
            && let Some(parent) = rows.last_mut()
        {
            parent.text.runs.push(run(
                format!("{}{}", index.format.between_entries, if level > 2 { "› " } else { "" }),
                &index.format.topic_format,
                None,
            ));
            parent.text.runs.extend(text.runs);
            parent.text.unresolved.extend(text.unresolved);
        } else {
            let heading_text = match &topic.sort {
                SortKey::Alphabetical { key } if !key.is_empty() => key.as_str(),
                _ => topic.display.as_str(),
            };
            let current = heading_text
                .chars()
                .next()
                .map(|c| if c.is_alphabetic() { c.to_uppercase().to_string() } else { "Symbols".into() })
                .unwrap_or_default();
            let section = if index.sorting == SortMode::Alphabetical && index.format.section_headings && level == 1 && current != heading {
                heading = current.clone();
                Some(current)
            } else {
                None
            };
            rows.push(Row { topic: id, level, heading: section, text });
        }
    }
    if rows.iter().any(|r| r.text.runs.iter().map(|x| x.text.len()).sum::<usize>() > 1024 * 1024) {
        return Err(invalid("generated index entry exceeds 1 MiB; disable run-in or split its hierarchy"));
    }
    Ok(rows)
}
