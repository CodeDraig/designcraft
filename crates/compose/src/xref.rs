//! Cross-reference resolution: the page each text anchor lands on (from composition) and the
//! generated text of each cross-reference.

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_doc::{Document, Story, StoryId, XREF_MARK, xref};

use crate::ComposedStory;

/// Page name of every text anchor in the document.
#[derive(Debug, Default)]
pub struct XrefIndex {
    pages: HashMap<u64, (usize, String)>,
}

pub fn has_xrefs(st: &Story) -> bool {
    let mut pending = vec![st];
    while let Some(s) = pending.pop() {
        if !s.xrefs.is_empty() {
            return true;
        }
        pending.extend(s.tables.values().flat_map(|t| t.cells.iter()).map(|c| &c.text));
        pending.extend(s.notes.iter().chain(&s.endnotes).map(|n| &n.text));
    }
    false
}

/// Publication fields already carry resolved rich values; only legacy fields need this index.
pub(crate) fn needs_index(st: &Story) -> bool {
    let mut pending = vec![st];
    while let Some(s) = pending.pop() {
        if s.xrefs.iter().any(|x| x.field.is_none()) {
            return true;
        }
        pending.extend(s.tables.values().flat_map(|t| t.cells.iter()).map(|c| &c.text));
        pending.extend(s.notes.iter().chain(&s.endnotes).map(|n| &n.text));
    }
    false
}

impl XrefIndex {
    /// `get` composes a story (normally through the cache). Stories that themselves hold
    /// cross-references or running headers are composed without resolving those dynamic fields.
    /// Resolving a header here can request a cross-referencing heading and enter this build again.
    pub fn build(doc: &Document, get: &dyn Fn(StoryId) -> Arc<ComposedStory>) -> XrefIndex {
        let mut pages = HashMap::new();
        for (sid, st) in &doc.stories {
            if st.anchors.is_empty()
                && st.tables.is_empty()
                && st.notes.is_empty()
                && doc.publication.as_ref().is_none_or(|v| v.root.note_output(v.component, *sid).is_none())
            {
                continue;
            }
            let cs = if has_xrefs(st) || crate::vars::has_running(doc, st) {
                Arc::new(crate::compose_story(doc, *sid, &Default::default()))
            } else {
                get(*sid)
            };
            fn collect(doc: &Document, st: &Story, cs: &ComposedStory, parent_page: Option<usize>, pages: &mut HashMap<u64, (usize, String)>) {
                for ft in &cs.frames {
                    let Some(page) = parent_page.or_else(|| doc.page_of_item(ft.frame)) else { continue };
                    for a in &st.anchors {
                        if let Some(pos) = st.anchor_pos(a.id)
                            && ft.range.contains(&pos)
                        {
                            pages.entry(a.id).or_insert_with(|| (page, doc.page_name(page)));
                        }
                    }
                    for fragment in &ft.tables {
                        if let Some(table) = st.tables.get(&fragment.table) {
                            for cell in &fragment.cells {
                                if let Some(source) = table.cell(cell.row, cell.col) {
                                    collect(doc, &source.text, &cell.text, Some(page), pages);
                                }
                            }
                        }
                    }
                    for note in &ft.notes {
                        if let Some(source) = note.source_story.as_deref().or_else(|| st.notes.get(note.index).map(|n| &n.text)) {
                            collect(doc, source, &note.text, Some(page), pages);
                        }
                    }
                }
            }
            collect(doc, st, &cs, None, &mut pages);
        }
        XrefIndex { pages }
    }

    /// Page name of an anchor.
    pub fn page(&self, anchor: u64) -> Option<&str> {
        self.pages.get(&anchor).map(|p| p.1.as_str())
    }

    /// Absolute page index of an anchor.
    pub fn page_index(&self, anchor: u64) -> Option<usize> {
        self.pages.get(&anchor).map(|p| p.0)
    }
}

/// The text a cross-reference shows ("??" when its destination is gone, like an unresolved
/// reference in typesetting).
pub fn xref_text(doc: &Document, x: &xref::CrossRef, index: Option<&XrefIndex>) -> String {
    if let Some(id) = x.field {
        return doc.publication_field(id, None).plain();
    }
    let Some(mut v) = doc.xref_values(x.target) else { return "??".into() };
    v.page = index.and_then(|i| i.page(x.target)).unwrap_or("?").to_string();
    let def = doc.xref_format(&x.format).map_or("<fullPara />", |f| f.definition.as_str());
    xref::expand(def, &v)
}

/// Generated text of the cross-references in `range` of `story`, by byte of their mark.
pub fn texts_in(doc: &Document, story: &Story, range: std::ops::Range<usize>, index: Option<&XrefIndex>) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    if story.xrefs.is_empty() {
        return out;
    }
    let before = story.text[..range.start].matches(XREF_MARK).count();
    for (k, (i, _)) in story.text[range.clone()].match_indices(XREF_MARK).enumerate() {
        if let Some(x) = story.xrefs.get(before + k) {
            out.insert(range.start + i, xref_text(doc, x, index));
        }
    }
    out
}

/// Rich generated fields retain their binding while shaped through normal line breaking.
pub fn rich_texts_in(
    doc: &Document,
    story: &Story,
    range: std::ops::Range<usize>,
    page: Option<usize>,
) -> HashMap<usize, designcraft_doc::publication::references::FieldText> {
    let before = story.text.get(..range.start).unwrap_or("").matches(XREF_MARK).count();
    story
        .text
        .get(range.clone())
        .unwrap_or("")
        .match_indices(XREF_MARK)
        .enumerate()
        .filter_map(|(k, (i, _))| story.xrefs.get(before + k)?.field.map(|id| (range.start + i, doc.publication_field(id, page))))
        .collect()
}
