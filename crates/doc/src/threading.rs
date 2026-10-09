//! Moving story content during threading, preserving marker identity and external ranges.
use crate::{DocError, Document, HyperlinkSource, Result, Story, StoryId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

fn invalid(message: &str) -> DocError {
    DocError::Invalid(message.into())
}

fn unique(used: &mut BTreeSet<u64>, id: u64) -> Result<u64> {
    if used.insert(id) {
        return Ok(id);
    }
    let mut next = 1u64;
    while used.contains(&next) {
        next = next.checked_add(1).ok_or_else(|| invalid("story marker IDs exhausted"))?;
    }
    used.insert(next);
    Ok(next)
}

pub(crate) fn allowed(d: &Document, a: StoryId, b: Option<StoryId>) -> Result<()> {
    let involved = |id| id == a || Some(id) == b;
    if d.endnote_story.is_some_and(involved)
        || d.toc.as_ref().is_some_and(|t| involved(t.story))
        || d.index.as_ref().is_some_and(|i| involved(i.story))
    {
        return Err(invalid("cannot thread a generated story"));
    }
    if d.stories.values().any(|s| s.link.is_some_and(|(parent, _)| involved(s.id) || involved(parent))) {
        return Err(invalid("cannot thread stories participating in Place and Link"));
    }
    Ok(())
}

/// Move all payloads, not a clipboard copy: document-wide anchors and object IDs stay stable.
pub(crate) fn append(dst: &mut Story, src: &Story) -> Result<usize> {
    dst.check().map_err(DocError::Invalid)?;
    src.check().map_err(DocError::Invalid)?;
    let separator = usize::from(!dst.text.is_empty() && !src.text.is_empty());
    let offset = dst.len().checked_add(separator).ok_or_else(|| invalid("threaded story is too large"))?;
    offset.checked_add(src.len()).ok_or_else(|| invalid("threaded story is too large"))?;
    let mut tables = BTreeMap::new();
    let mut used: BTreeSet<_> = dst.tables.keys().copied().collect();
    for (&id, table) in &src.tables {
        let new = unique(&mut used, id)?;
        tables.insert(id, new);
        let mut table = table.clone();
        Arc::make_mut(&mut table).id = new;
        dst.tables.insert(new, table);
    }
    if !src.text.is_empty() {
        let empty = dst.text.is_empty();
        if empty {
            dst.paras.clear();
            dst.chars.clear();
        } else {
            dst.text.push('\n');
            let last = dst.chars.last_mut().ok_or_else(|| invalid("source has no character formatting"))?;
            last.len = last.len.checked_add(1).ok_or_else(|| invalid("character run is too large"))?;
        }
        dst.text.push_str(&src.text);
        for para in &src.paras {
            let mut para = para.clone();
            if let Some(id) = para.table {
                para.table = Some(*tables.get(&id).ok_or_else(|| invalid("missing table while threading"))?);
            }
            dst.paras.push(para);
        }
        dst.chars.extend(src.chars.iter().cloned());
    }
    for (destination, source) in [(&mut dst.notes, &src.notes), (&mut dst.endnotes, &src.endnotes)] {
        let mut used = destination.iter().map(|n| n.id).collect();
        for n in source {
            let mut n = n.clone();
            Arc::make_mut(&mut n).id = unique(&mut used, n.id)?;
            destination.push(n);
        }
    }
    let mut used = dst.editorial.iter().map(|n| n.id).collect();
    for n in &src.editorial {
        let mut n = n.clone();
        Arc::make_mut(&mut n).id = unique(&mut used, n.id)?;
        dst.editorial.push(n);
    }
    let anchors: BTreeSet<_> = dst.anchors.iter().map(|a| a.id).collect();
    if src.anchors.iter().any(|a| anchors.contains(&a.id)) {
        return Err(invalid("duplicate document text anchor while threading"));
    }
    dst.anchors.extend(src.anchors.iter().cloned());
    dst.xrefs.extend(src.xrefs.iter().cloned());
    dst.index_refs.extend(src.index_refs.iter().cloned());
    dst.objects.extend(src.objects.iter().cloned());
    dst.check().map_err(DocError::Invalid)?;
    Ok(offset)
}

pub(crate) fn retarget(d: &mut Document, src: &Story, to: StoryId, offset: usize) -> Result<()> {
    let shift = |story: &mut StoryId, start: &mut usize, end: &mut usize| -> Result<()> {
        if *story == src.id {
            if *start > *end || !src.text.is_char_boundary(*start) || !src.text.is_char_boundary(*end) {
                return Err(invalid("invalid text reference in threaded destination"));
            }
            *start = start.checked_add(offset).ok_or_else(|| invalid("text reference offset overflow"))?;
            *end = end.checked_add(offset).ok_or_else(|| invalid("text reference offset overflow"))?;
            *story = to;
        }
        Ok(())
    };
    for link in &mut d.hyperlinks {
        if let HyperlinkSource::Text { story, start, end } = &mut link.source {
            shift(story, start, end)?;
        }
    }
    for placeholder in &mut d.data_merge.placeholders {
        if let crate::datamerge::PlaceholderAnchor::Text { story, start, end } = &mut placeholder.anchor {
            shift(story, start, end)?;
        }
    }
    if d.settings.primary_story == Some(src.id) {
        d.settings.primary_story = Some(to);
    }
    Ok(())
}

impl Document {
    /// Preconditions for the user-facing endpoint-to-endpoint threading operation.
    pub fn validate_thread(&self, from: crate::ItemId, to: Option<crate::ItemId>) -> Result<()> {
        editable(self, from)?;
        let source =
            self.item(from).and_then(|i| i.text_frame()).and_then(|t| self.story(t.story)).ok_or_else(|| invalid("source must be a text frame"))?;
        if source.frames.last() != Some(&from) {
            return Err(invalid("connect from the last frame of the source story"));
        }
        for id in &source.frames {
            editable(self, *id)?;
        }
        allowed(self, source.id, None)?;
        let mut target = None;
        if let Some(to) = to {
            editable(self, to)?;
            if to == from {
                return Err(invalid("cannot thread a frame to itself"));
            }
            match &self.item(to).ok_or(crate::DocError::NoItem(to))?.content {
                crate::Content::Unassigned => {}
                crate::Content::Text(t) => {
                    target = Some(t.story);
                    let other = self.story(t.story).ok_or(crate::DocError::NoStory(t.story))?;
                    if other.id == source.id {
                        return Err(invalid("frames are already in the same story"));
                    }
                    if other.frames.first() != Some(&to) {
                        return Err(invalid("connect to the first frame of the destination story"));
                    }
                    if other.vertical != source.vertical || other.direction != source.direction {
                        return Err(invalid("stories have incompatible text directions"));
                    }
                    for id in &other.frames {
                        editable(self, *id)?;
                    }
                }
                _ => return Err(invalid("destination must be a text or empty unassigned frame")),
            }
        }
        allowed(self, source.id, target)
    }
}

fn editable(d: &Document, id: crate::ItemId) -> Result<()> {
    let loc = d.find(id).ok_or(crate::DocError::NoItem(id))?;
    if !matches!(loc.spread, crate::SpreadRef::Doc(_)) {
        return Err(invalid("thread document frames, not parent-page frames"));
    }
    for n in 1..=loc.path.len() {
        let path = loc.path.get(..n).ok_or_else(|| invalid("invalid frame path"))?.to_vec();
        let ancestor = d.item_at(&crate::ItemLoc { spread: loc.spread, path }).ok_or(crate::DocError::NoItem(id))?;
        let layer = d.layer(ancestor.layer).ok_or_else(|| invalid("frame layer is missing"))?;
        if ancestor.locked || ancestor.hidden || layer.locked || !layer.visible {
            return Err(invalid("cannot thread locked or hidden frames"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use designcraft_geom::{Rect, shapes};

    fn pair(first: &str, second: &str) -> (Document, ItemId, StoryId, ItemId, StoryId) {
        let mut d = Document::new(&build::NewDocument::default());
        let layer = d.default_layer();
        let (a, sa) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 150.0, 80.0), layer, first, ParaFormat::default()).unwrap();
        let (b, sb) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(200.0, 36.0, 400.0, 600.0), layer, second, ParaFormat::default()).unwrap();
        (d, a, sa, b, sb)
    }
    fn enrich(d: &mut Document, sid: StoryId, label: &str) -> u64 {
        let id = ItemId(d.alloc());
        let obj = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
        let st = d.story_mut(sid).unwrap();
        st.format_chars(0..st.len(), |f| f.over.size = Some(19.0));
        st.insert_note(st.len(), &format!("footnote {label}"), ParaFormat::default());
        st.insert_endnote(st.len(), &format!("endnote {label}"), ParaFormat::default());
        st.insert(st.len(), &crate::endnotes::NOTE_MARK.to_string());
        Arc::make_mut(st.editorial.last_mut().unwrap()).text = format!("editorial {label}");
        let anchor = st.insert_anchor(st.len(), label);
        st.insert_xref(st.len(), CrossRef { target: anchor, format: "Paragraph Text".into() });
        st.insert_index_ref(st.len(), IndexRef { topics: vec![label.into()], ..Default::default() });
        st.insert_object(st.len(), AnchoredObject::new(obj, Default::default()));
        let mut table = Table::new(1, 1, 1, 0, 0, 80.0);
        table.cells[0].text.insert(0, label);
        st.insert_table(st.len(), table);
        st.check().unwrap();
        anchor
    }
    #[test]
    fn threading_preserves_all_payloads_and_retargets_ranges() {
        let (mut d, a, sa, b, sb) = pair("first", "目标");
        enrich(&mut d, sa, "source");
        let anchor = enrich(&mut d, sb, "destination");
        let left = d.story(sa).unwrap().clone();
        let right = d.story(sb).unwrap().clone();
        d.hyperlinks.push(Hyperlink {
            id: 1,
            name: "target text".into(),
            source: HyperlinkSource::Text { story: sb, start: 0, end: 6 },
            dest: HyperlinkDest::Url("https://example.com".into()),
        });
        d.data_merge.placeholders.push(crate::datamerge::Placeholder {
            id: 1,
            source_id: 1,
            field: "name".into(),
            role: crate::datamerge::PlaceholderRole::Text,
            anchor: crate::datamerge::PlaceholderAnchor::Text { story: sb, start: 0, end: 6 },
        });
        d.settings.primary_story = Some(sb);
        d.thread(a, b).unwrap();
        d.check().unwrap();
        let merged = d.story(sa).unwrap();
        let offset = left.len() + 1;
        assert_eq!(merged.text, format!("{}\n{}", left.text, right.text));
        assert_eq!(merged.tables.len(), 2);
        assert_ne!(merged.paras.iter().find_map(|p| p.table), merged.paras.iter().rev().find_map(|p| p.table));
        assert_eq!(merged.tables.values().map(|t| t.cells[0].text.text.as_str()).collect::<Vec<_>>(), ["source", "destination"]);
        for (notes, old) in [(&merged.notes, &right.notes), (&merged.endnotes, &right.endnotes)] {
            assert_eq!(notes.len(), 2);
            assert_ne!(notes[0].id, notes[1].id);
            assert_eq!(notes[1].text, old[0].text);
        }
        assert_eq!(merged.editorial.len(), 2);
        assert_eq!(merged.editorial[1].text, "editorial destination");
        assert_ne!(merged.editorial[0].id, merged.editorial[1].id);
        assert_eq!(merged.anchors[1], right.anchors[0]);
        assert_eq!(merged.xrefs[1], right.xrefs[0]);
        assert_eq!(merged.index_refs[1], right.index_refs[0]);
        assert_eq!(merged.objects[1], right.objects[0]);
        assert_eq!(merged.char_format_at(offset + 1).over.size, Some(19.0));
        assert_eq!(d.find_anchor(anchor).unwrap().0, sa);
        assert_eq!(d.settings.primary_story, Some(sa));
        assert_eq!(d.hyperlinks[0].source, HyperlinkSource::Text { story: sa, start: offset, end: offset + 6 });
        assert_eq!(d.data_merge.placeholders[0].anchor, crate::datamerge::PlaceholderAnchor::Text { story: sa, start: offset, end: offset + 6 });
    }
    #[test]
    fn threading_an_empty_source_has_no_leading_paragraph() {
        let (mut d, a, sa, b, sb) = pair("", "destination\nsecond paragraph");
        d.story_mut(sb).unwrap().paras[0].para.space_before = Some(17.0);
        let original = d.story(sb).unwrap().clone();
        d.thread(a, b).unwrap();
        d.check().unwrap();
        let merged = d.story(sa).unwrap();
        assert_eq!(merged.text, original.text);
        assert_eq!(merged.paras, original.paras);
        assert_eq!(merged.chars, original.chars);
    }
    #[test]
    fn threading_reference_failure_keeps_both_original_stories() {
        let (mut d, a, _, b, sb) = pair("first", "目标");
        d.hyperlinks.push(Hyperlink {
            id: 1,
            name: "invalid".into(),
            source: HyperlinkSource::Text { story: sb, start: 1, end: 6 },
            dest: HyperlinkDest::Url("https://example.com".into()),
        });
        let before = serde_json::to_value(&d).unwrap();
        assert!(d.thread(a, b).is_err());
        assert_eq!(serde_json::to_value(&d).unwrap(), before);
    }
}
