//! Build stories and preserve inline content in XML order.

use super::{Arc, BTreeMap, CharAttrs, CharFormat, CharRun, El, Importer, Node, ParaFormat, Story, StoryId, Table, names, st};

impl Importer<'_> {
    pub(super) fn story(&mut self, id: StoryId, e: &El) -> Story {
        let mut b = StoryBuilder {
            text: String::new(),
            paras: vec![ParaFormat::default()],
            runs: Vec::new(),
            fresh: true,
            last: CharFormat::default(),
            tables: Vec::new(),
            after_table: None,
            notes: Vec::new(),
            anchors: Vec::new(),
            xrefs: Vec::new(),
            index_refs: Vec::new(),
            objects: Vec::new(),
        };
        self.walk_story(e, &mut b, &ParaFormat::default(), &CharAttrs::default(), &CharFormat::default(), None);
        let StoryBuilder { text, mut paras, runs, last, tables: tbls, notes, anchors, xrefs, index_refs, objects, .. } = b;
        let mut chars: Vec<CharRun> = runs.into_iter().filter(|r| r.len > 0).collect();
        if chars.is_empty() {
            chars.push(CharRun { len: 0, format: last });
        }
        let mut tables = BTreeMap::new();
        for (pi, t) in tbls {
            if let Some(p) = paras.get_mut(pi) {
                p.table = Some(t.id);
                tables.insert(t.id, Arc::new(t));
            }
        }
        let notes = notes.into_iter().enumerate().map(|(i, text)| Arc::new(designcraft_doc::Footnote { id: i as u64 + 1, text })).collect();
        let mut anchor_list = Vec::new();
        for (k, (me, name)) in anchors.into_iter().enumerate() {
            let aid = (id.0 << 24) | (k as u64 + 1);
            self.anchor_ids.insert(me, aid);
            anchor_list.push(Arc::new(designcraft_doc::TextAnchor { id: aid, name }));
        }
        let mut xref_list = Vec::new();
        for (k, (me, fmt)) in xrefs.into_iter().enumerate() {
            self.xref_srcs.insert(me, (id, k));
            let format = self.xref_format_names.get(&fmt).cloned().unwrap_or_else(|| "Full Paragraph & Page Number".into());
            xref_list.push(Arc::new(designcraft_doc::CrossRef { target: 0, format }));
        }
        let mut st = Story {
            id,
            text,
            paras,
            chars,
            frames: vec![],
            direction: if e.find("StoryPreference").and_then(|p| p.get("StoryDirection")) == Some("RightToLeftDirection") {
                designcraft_doc::TextDirection::RightToLeft
            } else {
                designcraft_doc::TextDirection::LeftToRight
            },
            vertical: e.find("StoryPreference").and_then(|p| p.get("StoryOrientation")) == Some("Vertical"),
            rev: 0,
            tables,
            notes,
            endnotes: Vec::new(),
            anchors: anchor_list,
            xrefs: xref_list,
            index_refs: index_refs.into_iter().map(Arc::new).collect(),
            objects: objects.into_iter().map(Arc::new).collect(),
            editorial: Vec::new(),
            link: None,
        };
        st.fix_notes();
        st.fix_marks();
        // Drop anything inconsistent (e.g. a table sharing a paragraph with another).
        if st.check().is_err() {
            st.tables.retain(|_, _| false);
            for p in &mut st.paras {
                p.table = None;
            }
        }
        st
    }

    fn walk_story(&mut self, e: &El, b: &mut StoryBuilder, pf: &ParaFormat, pchars: &CharAttrs, cf: &CharFormat, brk: Option<&str>) {
        for n in &e.children {
            match n {
                Node::El(c) => match c.local() {
                    "ParagraphStyleRange" => {
                        let style = c.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)).unwrap_or_else(|| st::BASIC_PARAGRAPH.into());
                        let para = self.para_attrs(c);
                        let chars = self.char_attrs(c);
                        let npf = ParaFormat { style, para, chars: CharAttrs::default(), table: None };
                        if b.fresh
                            && let Some(last) = b.paras.last_mut()
                        {
                            *last = npf.clone();
                        }
                        self.walk_story(c, b, &npf, &chars, cf, None);
                    }
                    "CharacterStyleRange" => {
                        let style = c.get("AppliedCharacterStyle").map(|r| self.char_style_ref(r)).unwrap_or_else(|| st::NO_CHAR_STYLE.into());
                        let mut over = pchars.clone();
                        over.merge(&self.char_attrs(c));
                        let ncf = CharFormat { style, over };
                        let bt = c.get("ParagraphBreakType").filter(|t| *t != "Anywhere");
                        b.last = ncf.clone();
                        self.walk_story(c, b, pf, pchars, &ncf, bt);
                    }
                    "Content" => {
                        for t in &c.children {
                            match t {
                                Node::Text(s) | Node::CData(s) => {
                                    let s: String = s
                                        .chars()
                                        .filter(|c| *c != '\u{FEFF}')
                                        .map(|c| if c == '\n' || c == '\r' || c == '\u{2029}' { st::FORCED_LINE_BREAK } else { c })
                                        .collect();
                                    b.push(&s, cf);
                                }
                                Node::Pi(t, v) if t == "ACE" => {
                                    if let Some(ch) = ace_char(v) {
                                        b.push(&ch.to_string(), cf);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "Br" => match brk {
                        Some(t) => {
                            let ch = match t {
                                "NextColumn" => st::COLUMN_BREAK,
                                "NextFrame" => st::FRAME_BREAK,
                                _ => st::PAGE_BREAK,
                            };
                            b.push(&ch.to_string(), cf);
                        }
                        None => {
                            b.push("\n", cf);
                            b.paras.push(pf.clone());
                            b.fresh = true;
                        }
                    },
                    "Table" => {
                        let t = self.table(c);
                        b.push_table(t, pf, cf);
                    }
                    "HyperlinkTextDestination" | "ParagraphDestination" => {
                        b.anchors.push((c.get("Self").unwrap_or("").to_string(), c.get("Name").unwrap_or("Anchor").to_string()));
                        b.push(&designcraft_doc::ANCHOR_MARK.to_string(), cf);
                    }
                    "PageReference" => {
                        use designcraft_doc::index::IndexRange as R;
                        let topics = c.get("ReferencedTopic").and_then(|r| self.index_topics.get(r)).cloned().unwrap_or_default();
                        let range = match c.get("PageReferenceType") {
                            Some("ToEndOfStory") | Some("ToEndOfDocument") | Some("ToEndOfSection") => R::ToEndOfStory,
                            Some("ForNextNParagraphs") => R::NextParagraphs(c.num("PageReferenceLimit").unwrap_or(1.0).max(1.0) as u32),
                            Some("SuppressPageNumbers") => R::SuppressPageRange,
                            _ => R::CurrentPage,
                        };
                        b.index_refs.push(designcraft_doc::IndexRef { topics, sort: vec![], range });
                        b.push(&designcraft_doc::INDEX_MARK.to_string(), cf);
                    }
                    "CrossReferenceSource" => {
                        // The generated text is regenerated from the format.
                        b.xrefs.push((c.get("Self").unwrap_or("").to_string(), c.get("AppliedFormat").unwrap_or("").to_string()));
                        let inner = c.find("CharacterStyleRange");
                        let f = match inner.and_then(|r| r.get("AppliedCharacterStyle")) {
                            Some(r) => CharFormat { style: self.char_style_ref(r), over: pchars.clone() },
                            None => cf.clone(),
                        };
                        b.push(&designcraft_doc::XREF_MARK.to_string(), &f);
                    }
                    "Footnote" => {
                        // The text starts with the number marker (ACE 4, dropped) and the separator.
                        let mut note = self.story(StoryId(0), c);
                        let sep = self.footnote_options.separator.clone();
                        if !sep.is_empty() && note.text.starts_with(&sep) {
                            note.delete(0..sep.len());
                        }
                        b.notes.push(note);
                        // The reference's position comes from the footnote options.
                        let mut rcf = cf.clone();
                        rcf.over.position = None;
                        b.push(&designcraft_doc::FOOTNOTE_REF.to_string(), &rcf);
                    }
                    // Not supported yet: skip their content entirely.
                    "Rectangle" | "Oval" | "Polygon" | "GraphicLine" | "Group" => {
                        // An anchored object (text frames inside stories aren't supported yet).
                        let has_text = c.find_all("TextFrame").next().is_some();
                        if !has_text && let Some(item) = self.item(c, designcraft_geom::Affine::IDENTITY) {
                            use designcraft_doc::anchored::AnchorAlign as A;
                            let set = c.find("AnchoredObjectSetting");
                            let get = |k: &str| set.and_then(|s| s.num(k)).unwrap_or(0.0);
                            let position = match set.and_then(|s| s.get("AnchoredPosition")) {
                                Some("AboveLine") => designcraft_doc::AnchorPosition::AboveLine {
                                    align: match set.and_then(|s| s.get("HorizontalAlignment")) {
                                        Some("CenterAlign") => A::Center,
                                        Some("RightAlign") => A::Right,
                                        _ => A::Left,
                                    },
                                    space_before: get("AnchorSpaceAbove"),
                                    space_after: get("AnchorYoffset"),
                                },
                                Some("Anchored") => {
                                    let s = |k: &str| set.and_then(|x| x.get(k)).unwrap_or("");
                                    let col = match s("HorizontalAlignment") {
                                        "CenterAlign" => 1,
                                        "RightAlign" => 2,
                                        _ => 0,
                                    };
                                    let row = match s("VerticalAlignment") {
                                        "CenterAlign" => 1,
                                        "BottomAlign" => 2,
                                        _ => 0,
                                    };
                                    designcraft_doc::AnchorPosition::Custom {
                                        x_relative: names::anchor_rel_in(s("HorizontalReferencePoint")),
                                        y_relative: names::anchor_rel_in(s("VerticalReferencePoint")),
                                        x_offset: get("AnchorXoffset"),
                                        y_offset: get("AnchorYoffset"),
                                        object_point: names::anchor_in(s("AnchorPoint")),
                                        ref_point: row * 3 + col,
                                        keep_within_column: s("PinPosition") == "true",
                                    }
                                }
                                _ => designcraft_doc::AnchorPosition::Inline { y_offset: get("AnchorYoffset") },
                            };
                            b.objects.push(designcraft_doc::AnchoredObject::new(item, position));
                            b.push(&designcraft_doc::OBJECT_MARK.to_string(), cf);
                        }
                    }
                    "Properties" | "Note" | "TextFrame" | "StoryPreference" | "InCopyExportOption" | "TextVariableInstance" => {}
                    _ => self.walk_story(c, b, pf, pchars, cf, brk),
                },
                Node::Pi(t, v) if t == "ACE" => {
                    if let Some(ch) = ace_char(v) {
                        b.push(&ch.to_string(), cf);
                    }
                }
                _ => {}
            }
        }
    }
}

struct StoryBuilder {
    text: String,
    paras: Vec<ParaFormat>,
    runs: Vec<CharRun>,
    /// The current paragraph has no content yet (its format can still be set by a range).
    fresh: bool,
    last: CharFormat,
    /// Tables by anchor paragraph index.
    tables: Vec<(usize, Table)>,
    /// Content after a table anchor starts a new paragraph (with this format).
    after_table: Option<ParaFormat>,
    /// Footnote texts in reference order.
    notes: Vec<Story>,
    /// Text destinations (Self, name) and cross-reference sources (Self, format Self) in order.
    anchors: Vec<(String, String)>,
    index_refs: Vec<designcraft_doc::IndexRef>,
    xrefs: Vec<(String, String)>,
    objects: Vec<designcraft_doc::AnchoredObject>,
}

impl StoryBuilder {
    /// A table gets an anchor paragraph of its own.
    fn push_table(&mut self, t: Table, pf: &ParaFormat, cf: &CharFormat) {
        let para_empty = self.text.rsplit('\n').next().is_none_or(str::is_empty);
        if !para_empty {
            self.after_table = None;
            self.push("\n", cf);
            self.paras.push(pf.clone());
        }
        self.after_table = None;
        self.push(&st::TABLE_ANCHOR.to_string(), cf);
        self.tables.push((self.paras.len() - 1, t));
        self.after_table = Some(pf.clone());
        self.fresh = false;
    }

    fn push(&mut self, s: &str, f: &CharFormat) {
        if s.is_empty() {
            return;
        }
        if let Some(pf) = self.after_table.take()
            && !s.starts_with('\n')
        {
            self.push("\n", f);
            self.paras.push(pf);
        }
        self.text.push_str(s);
        if !s.contains('\n') || s.len() > 1 {
            self.fresh = false;
        }
        match self.runs.last_mut() {
            Some(r) if r.format == *f => r.len += s.len(),
            _ => self.runs.push(CharRun { len: s.len(), format: f.clone() }),
        }
    }
}

fn ace_char(v: &str) -> Option<char> {
    match v.trim() {
        "18" => Some(st::PAGE_NUMBER),
        "19" => Some(st::SECTION_MARKER),
        "7" => Some(st::INDENT_HERE),
        "8" => Some(st::RIGHT_INDENT_TAB),
        _ => None,
    }
}
