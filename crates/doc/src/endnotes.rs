//! Endnotes (Type › Insert Endnote).
//!
//! An endnote reference is an [`ENDNOTE_REF`] character; the story's [`Story::endnotes`] holds one
//! entry per reference in text order (like footnotes). Numbering runs through the document in
//! page order. The endnote frame shows a story that is generated from every endnote: a heading
//! and one numbered paragraph per endnote ([`crate::Document::endnote_text`]); its text isn't typed
//! into, endnotes are edited through their references.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attrs::NumberStyle;
use crate::ids::StoryId;
use crate::notes::Footnote;
use crate::story::{BASIC_PARAGRAPH, CharFormat, ParaFormat, Story};

/// Endnote reference character (the number shown in the text).
pub const ENDNOTE_REF: char = '\u{E00F}';

/// Editorial note anchor (Type › Notes): never printed; the note's text is in
/// [`Story::editorial`], one per anchor in text order.
pub const NOTE_MARK: char = '\u{E010}';

/// An editorial note.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorialNote {
    pub id: u64,
    pub author: String,
    pub text: String,
}

/// Document Endnote Options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EndnoteOptions {
    pub style: NumberStyle,
    pub start_at: u32,
    pub prefix: String,
    pub suffix: String,
    /// Endnote frame heading (empty: none) and its paragraph style.
    pub heading: String,
    pub heading_style: String,
    /// Endnote entries' paragraph style.
    pub para_style: String,
    /// Between the number and the endnote text.
    pub separator: String,
}

impl Default for EndnoteOptions {
    fn default() -> Self {
        EndnoteOptions {
            style: NumberStyle::Arabic,
            start_at: 1,
            prefix: String::new(),
            suffix: String::new(),
            heading: "Endnotes".into(),
            heading_style: BASIC_PARAGRAPH.into(),
            para_style: BASIC_PARAGRAPH.into(),
            separator: "\t".into(),
        }
    }
}

impl EndnoteOptions {
    pub fn label(&self, n: u32) -> String {
        format!("{}{}{}", self.prefix, self.style.format(n), self.suffix)
    }
}

impl Story {
    /// Index (in [`Story::endnotes`]) of the endnote whose reference is the k-th before `pos`.
    pub fn endnotes_before(&self, pos: usize) -> usize {
        let pos = crate::story::floor_char_boundary(&self.text, pos);
        self.text[..pos].matches(ENDNOTE_REF).count()
    }

    pub fn endnote(&self, id: u64) -> Option<&Footnote> {
        self.endnotes.iter().find(|n| n.id == id).map(|n| &**n)
    }

    /// Mutable endnote (copy-on-write); bumps the story revision.
    pub fn endnote_mut(&mut self, id: u64) -> Option<&mut Footnote> {
        let n = Arc::make_mut(&mut self.content).endnotes.iter_mut().find(|n| n.id == id)?;
        self.rev += 1;
        Some(Arc::make_mut(n))
    }

    /// Insert an endnote reference at `pos` with the given text. Returns the endnote id.
    pub fn insert_endnote(&mut self, pos: usize, text: &str, para: ParaFormat) -> u64 {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        let fmt = self.char_format_at(pos).clone();
        self.insert_with(pos, &ENDNOTE_REF.to_string(), fmt);
        let k = self.endnotes_before(pos);
        let note = Arc::make_mut(&mut self.endnotes[k]);
        note.text = Story::with_text(StoryId(0), text, para);
        note.id
    }

    /// Insert empty endnotes for `n` new reference characters before which `k` references sit.
    pub(crate) fn endnotes_inserted(&mut self, k: usize, n: usize) {
        for i in 0..n {
            let id = self.endnotes.iter().map(|x| x.id).max().unwrap_or(0) + 1;
            let mut text = Story::new(StoryId(0));
            text.chars[0].format = CharFormat::default();
            let at = (k + i).min(self.endnotes.len());
            self.endnotes.insert(at, Arc::new(Footnote { id, text, annotation: None, apparatus: None }));
        }
    }

    pub(crate) fn check_endnotes(&self) -> Result<(), String> {
        let n = self.text.matches(ENDNOTE_REF).count();
        if n != self.endnotes.len() {
            return Err(format!("story {}: {n} endnote references, {} endnotes", self.id.0, self.endnotes.len()));
        }
        let mut ids = std::collections::BTreeSet::new();
        for note in &self.endnotes {
            if !ids.insert(note.id) {
                return Err(format!("story {}: duplicate endnote {}", self.id.0, note.id));
            }
            note.text.check()?;
            if let Some(apparatus) = &note.apparatus {
                apparatus.check()?;
            }
        }
        Ok(())
    }
}

impl crate::Document {
    /// Append a newly created, empty frame to the generated endnote story. Ordinary threading
    /// must keep rejecting merges into generated text, whose source is its note records.
    pub fn extend_endnote_frame(&mut self, frame: crate::ItemId) -> crate::Result<()> {
        let sid = self.endnote_story.ok_or_else(|| crate::DocError::Invalid("no generated endnote story".into()))?;
        self.story(sid).ok_or(crate::DocError::NoStory(sid))?;
        let target = self.item(frame).and_then(|i| i.text_frame()).map(|t| t.story).ok_or(crate::DocError::NoItem(frame))?;
        let st = self.story(target).ok_or(crate::DocError::NoStory(target))?;
        if target == sid || !st.text.is_empty() || st.frames != [frame] || st.link.is_some() {
            return Err(crate::DocError::Invalid("endnote extension requires one new empty text frame".into()));
        }
        crate::threading::allowed(self, target, None)?;
        if let Some(item) = self.item_mut(frame)
            && let crate::Content::Text(text) = &mut item.content
        {
            text.story = sid;
        }
        self.stories.remove(&target);
        if let Some(st) = self.story_mut(sid) {
            st.frames.push(frame);
            st.rev = st.rev.saturating_add(1);
        }
        Ok(())
    }

    /// Endnotes in reading order, including references within tables and footnotes.
    pub fn ordered_endnotes(&self) -> Vec<(StoryId, usize, &Footnote)> {
        fn collect<'a>(st: &'a Story, out: &mut Vec<(StoryId, usize, &'a Footnote)>) {
            if st.notes.is_empty() && st.tables.is_empty() {
                out.extend(st.endnotes.iter().enumerate().map(|(i, n)| (st.id, i, &**n)));
                return;
            }
            let (mut endnote, mut footnote) = (0, 0);
            for (pos, ch) in st.text.char_indices() {
                match ch {
                    ENDNOTE_REF => {
                        if let Some(n) = st.endnotes.get(endnote) {
                            out.push((st.id, endnote, n));
                        }
                        endnote += 1;
                    }
                    crate::FOOTNOTE_REF => {
                        if let Some(n) = st.notes.get(footnote) {
                            collect(&n.text, out);
                        }
                        footnote += 1;
                    }
                    crate::TABLE_ANCHOR => {
                        if let Some(t) = st.table_at(pos).and_then(|id| st.tables.get(&id)) {
                            for cell in &t.cells {
                                collect(&cell.text, out);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut roots: Vec<_> = self.stories.values().filter(|s| Some(s.id) != self.endnote_story).collect();
        roots.sort_by_key(|s| (s.frames.first().and_then(|f| self.page_of_item(*f)).unwrap_or(usize::MAX), s.id.0));
        let mut out = Vec::new();
        for st in roots {
            collect(st, &mut out);
        }
        out
    }

    /// Display numbers for a story's references. Nested table notes may interrupt the sequence.
    pub fn endnote_numbers(&self, sid: StoryId) -> Vec<u32> {
        self.ordered_endnotes()
            .iter()
            .enumerate()
            .filter(|(_, (s, _, _))| *s == sid)
            .map(|(i, _)| self.endnote_options.start_at.saturating_add(i as u32))
            .collect()
    }
    pub fn endnote_start(&self, sid: StoryId) -> u32 {
        self.endnote_numbers(sid).first().copied().unwrap_or(self.endnote_options.start_at)
    }

    /// The endnote frame's text: the heading, then "number separator text" per endnote, in order.
    pub fn endnote_text(&self, id: StoryId) -> Story {
        let o = &self.endnote_options;
        let para = |style: &str| ParaFormat { style: style.to_string(), ..Default::default() };
        let mut out = Story::with_text(id, &o.heading, para(&o.heading_style));
        for (k, (_, _, note)) in self.ordered_endnotes().into_iter().enumerate() {
            let n = o.start_at.saturating_add(k as u32);
            let at = out.len();
            let first = at == 0 && o.heading.is_empty();
            if !first {
                out.insert(at, "\n");
            }
            let pi = out.paras.len() - 1;
            out.paras[pi] = para(&o.para_style);
            let at = out.len();
            out.insert_with(at, &format!("{}{}", o.label(n), o.separator), CharFormat::default());
            let at = out.len();
            out.insert_story_preserving_anchors(at, &note.text);
        }
        out
    }

    /// Bring the endnote frame's story up to date with the endnotes (after any edit).
    pub fn sync_endnote_story(&mut self) {
        let Some(sid) = self.endnote_story else { return };
        let Some(cur) = self.stories.get(&sid) else {
            self.endnote_story = None;
            return;
        };
        let fresh = self.endnote_text(sid);
        let mut comparable = (**cur).clone();
        comparable.frames.clear();
        comparable.rev = fresh.rev;
        if comparable == fresh {
            return;
        }
        let Some(st) = self.stories.get_mut(&sid) else { return };
        let st = Arc::make_mut(st);
        let (frames, rev) = (std::mem::take(&mut st.frames), st.rev);
        *st = Story { frames, rev: rev + 1, ..fresh };
    }
}
