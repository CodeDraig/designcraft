//! Clipboard anchor allocation and publication passage moves. Every pasted nested destination
//! gets a fresh component-local identity; cut/paste moves retain their publication identities.
use crate::Result;
use designcraft_doc::{ComponentId, Document, Story, StoryId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) fn prepare(doc: &mut Document, slice: &mut Story) -> Result<BTreeMap<u64, u64>> {
    let mut ids = BTreeSet::new();
    let mut duplicate = false;
    slice.for_each_encoding_mut(&mut |s| {
        for a in &s.anchors {
            duplicate |= !ids.insert(a.id);
        }
    });
    if duplicate {
        return Err(super::super::bad("edit.paste", "clipboard contains duplicate text anchor identities"));
    }
    let mut mapping = BTreeMap::new();
    for id in ids {
        let next = doc.next_id.checked_add(1).ok_or_else(|| super::super::bad("edit.paste", "anchor identity exhausted"))?;
        let anchor =
            next.checked_mul(1 << 24).and_then(|n| n.checked_add(1)).ok_or_else(|| super::super::bad("edit.paste", "anchor identity exhausted"))?;
        doc.next_id = next;
        mapping.insert(id, anchor);
    }
    slice.for_each_encoding_mut(&mut |s| {
        for anchor in &mut s.anchors {
            if let Some(id) = mapping.get(&anchor.id) {
                Arc::make_mut(anchor).id = *id;
            }
        }
        for x in &mut s.xrefs {
            if let Some(id) = mapping.get(&x.target) {
                Arc::make_mut(x).target = *id;
            }
        }
        let content = Arc::make_mut(&mut s.content);
        for note in content.notes.iter_mut().chain(content.endnotes.iter_mut()) {
            if let Some(a) = &mut Arc::make_mut(note).apparatus {
                if let Some(id) = mapping.get(&a.start) {
                    a.start = *id;
                }
                if let Some(id) = mapping.get(&a.end) {
                    a.end = *id;
                }
            }
        }
    });
    Ok(mapping)
}

pub(super) fn transfer(doc: &mut Document, from: ComponentId, story: StoryId, mapping: &BTreeMap<u64, u64>) -> Result<()> {
    let Some(mut view) = doc.publication.clone() else { return Ok(()) };
    let mut present = BTreeSet::new();
    if let Some(component) = view.root.components.get(&from) {
        let mut pending: Vec<_> = component.stories.values().filter_map(|id| view.root.content.get(id).map(|s| s.as_ref())).collect();
        while let Some(s) = pending.pop() {
            present.extend(s.anchors.iter().map(|a| a.id));
            pending.extend(s.tables.values().flat_map(|t| &t.cells).map(|c| c.text.content.as_ref()));
            pending.extend(s.notes.iter().chain(&s.endnotes).map(|n| n.text.content.as_ref()));
        }
    }
    let content = *view
        .root
        .components
        .get(&view.component)
        .and_then(|c| c.stories.get(&story))
        .ok_or_else(|| super::super::bad("edit.paste", "missing publication destination"))?;
    let mut root = (*view.root).clone();
    let mut changed = false;
    let mut relocate = |target: &mut designcraft_doc::publication::numbering::TextTarget| {
        if target.component == from
            && !present.contains(&target.anchor)
            && let Some(anchor) = mapping.get(&target.anchor)
        {
            target.component = view.component;
            target.content = content;
            target.anchor = *anchor;
            changed = true;
        }
    };
    for object in root.semantic.values_mut() {
        relocate(&mut object.target);
    }
    for index in root.indexes.values_mut() {
        for o in index.occurrences.values_mut() {
            relocate(&mut o.start);
            if let Some(end) = &mut o.end {
                relocate(&mut end.target);
            }
        }
    }
    if changed {
        root.revision = root.revision.saturating_add(1);
        view.root = Arc::new(root);
        doc.publication = Some(view);
    }
    Ok(())
}
