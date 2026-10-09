//! Write story content and inline markers in character-run order.

use super::{CharFormat, El, Ex, Node, Story, anchor_self, bool_s, names, num, st, topic_self, uid, xref_format_self};

impl Ex<'_> {
    /// `<Footnote>`: the footnote's paragraphs, the first starting with the footnote number marker
    /// and the separator (as InDesign stores them).
    pub(super) fn footnote_el(&mut self, note: &Story) -> El {
        let mut el = El::new("Footnote");
        let mut paras = self.story_paras(note);
        let lead = vec![Node::Pi("ACE".into(), "4".into()), Node::Text(self.d.footnote_options.separator.clone())];
        if let Some(Node::El(csr)) =
            paras.first_mut().and_then(|p| p.children.iter_mut().find(|n| matches!(n, Node::El(e) if e.name == "CharacterStyleRange")))
        {
            match csr.children.iter_mut().find_map(|n| match n {
                Node::El(c) if c.name == "Content" => Some(c),
                _ => None,
            }) {
                Some(c) => {
                    c.children.splice(0..0, lead);
                }
                None => {
                    let mut c = El::new("Content");
                    c.children = lead;
                    csr.children.insert(0, Node::El(c));
                }
            }
        }
        for p in paras {
            el.push(p);
        }
        el
    }

    pub(super) fn story_el(&mut self, s: &Story) -> El {
        let mut el = El::new("Story").attr("Self", uid(s.id.0)).attr("AppliedTOCStyle", "n").attr("TrackChanges", "false").attr("StoryTitle", "$ID/");
        el.push(
            El::new("StoryPreference")
                .attr("OpticalMarginAlignment", "false")
                .attr("OpticalMarginSize", "12")
                .attr("FrameType", "TextFrameType")
                .attr("StoryOrientation", if s.vertical { "Vertical" } else { "Horizontal" })
                .attr(
                    "StoryDirection",
                    if s.direction == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" },
                ),
        );
        for psr in self.story_paras(s) {
            el.push(psr);
        }
        el
    }

    /// `ParagraphStyleRange`s of a story (or a table cell's story).
    pub(super) fn story_paras(&mut self, s: &Story) -> Vec<El> {
        let mut paras = Vec::new();
        let ranges = s.para_ranges();
        let n = ranges.len();
        for (pi, r) in ranges.into_iter().enumerate() {
            let pf = s.paras.get(pi).cloned().unwrap_or_default();
            let mut psr = self.psr_el(&pf);
            let last = pi + 1 == n;
            // Character runs intersecting the paragraph.
            let mut segs: Vec<(std::ops::Range<usize>, CharFormat)> = Vec::new();
            for (rr, f) in s.runs() {
                let a = rr.start.max(r.start);
                let b = rr.end.min(r.end);
                if a < b {
                    segs.push((a..b, f.clone()));
                }
            }
            if segs.is_empty() {
                let f = s.char_format_at(r.start).clone();
                segs.push((r.start..r.start, f));
            }
            let nseg = segs.len();
            for (k, (rr, f)) in segs.into_iter().enumerate() {
                let seg_start = rr.start;
                let text = &s.text[rr];
                // Split at break characters.
                let mut cur = String::new();
                let mut pending: Vec<Node> = Vec::new();
                let mut csrs: Vec<El> = Vec::new();
                let flush_text = |cur: &mut String, pending: &mut Vec<Node>| {
                    if !cur.is_empty() {
                        pending.push(Node::Text(std::mem::take(cur)));
                    }
                };
                let flush_content = |pending: &mut Vec<Node>, out: &mut Vec<Node>| {
                    if !pending.is_empty() {
                        let mut c = El::new("Content");
                        c.children = std::mem::take(pending);
                        out.push(Node::El(c));
                    }
                };
                let mut out: Vec<Node> = Vec::new();
                for (ci, ch) in text.char_indices() {
                    let ace = match ch {
                        st::PAGE_NUMBER | st::NEXT_PAGE_NUMBER | st::PREV_PAGE_NUMBER => Some("18"),
                        st::SECTION_MARKER => Some("19"),
                        st::INDENT_HERE => Some("7"),
                        st::RIGHT_INDENT_TAB => Some("8"),
                        _ => None,
                    };
                    let brk = match ch {
                        st::COLUMN_BREAK => Some("NextColumn"),
                        st::FRAME_BREAK => Some("NextFrame"),
                        st::PAGE_BREAK => Some("NextPage"),
                        _ => None,
                    };
                    if let Some(code) = ace {
                        flush_text(&mut cur, &mut pending);
                        pending.push(Node::Pi("ACE".into(), code.into()));
                    } else if ch == st::TABLE_ANCHOR {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        if let Some(t) = s.para_table(pi) {
                            out.push(Node::El(self.table_el(t)));
                        }
                    } else if ch == designcraft_doc::ANCHOR_MARK {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::ANCHOR_MARK).count();
                        if let Some(a) = s.anchors.get(k) {
                            out.push(Node::El(
                                El::new("HyperlinkTextDestination")
                                    .attr("Self", anchor_self(a.id))
                                    .attr("Name", &a.name)
                                    .attr("Hidden", "false")
                                    .attr("DestinationUniqueKey", a.id & 0xFFFF_FFFF),
                            ));
                        }
                    } else if ch == designcraft_doc::OBJECT_MARK {
                        // An anchored object: the item inside the range with its anchoring settings.
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::OBJECT_MARK).count();
                        if let Some(o) = s.objects.get(k) {
                            let mut e = self.item_el(&o.item, o.item.xf);
                            use designcraft_doc::anchored::AnchorAlign as A;
                            let setting = match &o.position {
                                designcraft_doc::AnchorPosition::Inline { y_offset } => {
                                    El::new("AnchoredObjectSetting").attr("AnchoredPosition", "InlinePosition").attr("AnchorYoffset", num(*y_offset))
                                }
                                designcraft_doc::AnchorPosition::AboveLine { align, space_before, space_after } => El::new("AnchoredObjectSetting")
                                    .attr("AnchoredPosition", "AboveLine")
                                    .attr(
                                        "HorizontalAlignment",
                                        match align {
                                            A::Left => "LeftAlign",
                                            A::Center => "CenterAlign",
                                            A::Right => "RightAlign",
                                        },
                                    )
                                    .attr("AnchorSpaceAbove", num(*space_before))
                                    .attr("AnchorYoffset", num(*space_after)),
                                designcraft_doc::AnchorPosition::Custom {
                                    x_relative,
                                    y_relative,
                                    x_offset,
                                    y_offset,
                                    object_point,
                                    ref_point,
                                    keep_within_column,
                                } => El::new("AnchoredObjectSetting")
                                    .attr("AnchoredPosition", "Anchored")
                                    .attr("AnchorPoint", names::anchor_out(*object_point))
                                    .attr("HorizontalReferencePoint", names::anchor_rel_out(*x_relative, false))
                                    .attr("VerticalReferencePoint", names::anchor_rel_out(*y_relative, true))
                                    .attr("HorizontalAlignment", ["LeftAlign", "CenterAlign", "RightAlign"][(*ref_point % 3) as usize])
                                    .attr("VerticalAlignment", ["TopAlign", "CenterAlign", "BottomAlign"][(*ref_point / 3).min(2) as usize])
                                    .attr("AnchorXoffset", num(*x_offset))
                                    .attr("AnchorYoffset", num(*y_offset))
                                    .attr("PinPosition", bool_s(*keep_within_column)),
                            };
                            e.push(setting);
                            out.push(Node::El(e));
                        }
                    } else if ch == designcraft_doc::INDEX_MARK {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::INDEX_MARK).count();
                        if let Some(r) = s.index_refs.get(k).filter(|r| !r.topics.is_empty()) {
                            use designcraft_doc::index::IndexRange as R;
                            for n in 1..=r.topics.len() {
                                self.index_topics.insert(r.topics[..n].to_vec());
                            }
                            let (kind, limit) = match &r.range {
                                R::CurrentPage => (Some("CurrentPage"), None),
                                R::ToEndOfStory => (Some("ToEndOfStory"), None),
                                R::NextParagraphs(n) => (Some("ForNextNParagraphs"), Some(*n)),
                                R::SuppressPageRange => (Some("SuppressPageNumbers"), None),
                                R::See(t) => {
                                    self.index_xrefs.push((r.topics.clone(), "See", t.clone()));
                                    (None, None)
                                }
                                R::SeeAlso(t) => {
                                    self.index_xrefs.push((r.topics.clone(), "SeeAlso", t.clone()));
                                    (Some("CurrentPage"), None)
                                }
                            };
                            if let Some(kind) = kind {
                                let mut el = El::new("PageReference")
                                    .attr("Self", self.fresh())
                                    .attr("PageReferenceType", kind)
                                    .attr("ReferencedTopic", topic_self(&r.topics))
                                    .attr("Id", k + 1);
                                if let Some(n) = limit {
                                    el.set("PageReferenceLimit", n);
                                }
                                out.push(Node::El(el));
                            }
                        }
                    } else if ch == designcraft_doc::XREF_MARK {
                        // A source wrapping its current text (InDesign regenerates it on update).
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        if !out.is_empty() {
                            let mut c = self.csr_el(&f);
                            c.children.append(&mut out);
                            csrs.push(c);
                        }
                        let k = s.text[..seg_start + ci].matches(designcraft_doc::XREF_MARK).count();
                        if let Some(x) = s.xrefs.get(k) {
                            let me = self.fresh();
                            let fmt = self.d.xref_formats.iter().position(|ff| ff.name == x.format).map_or("n".to_string(), xref_format_self);
                            let text = self.d.xref_values(x.target).map_or("??".into(), |mut v| {
                                v.page = "?".into();
                                designcraft_doc::xref::expand(self.d.xref_format(&x.format).map_or("<fullPara />", |ff| ff.definition.as_str()), &v)
                            });
                            let mut c = self.csr_el(&f);
                            let mut content = El::new("Content");
                            content.children.push(Node::Text(text));
                            c.push(content);
                            let mut src = El::new("CrossReferenceSource")
                                .attr("Self", &me)
                                .attr("AppliedFormat", fmt)
                                .attr("Name", format!("Cross-Reference {}", self.xref_sources.len() + 1))
                                .attr("Hidden", "false");
                            src.push(c);
                            csrs.push(src);
                            self.xref_sources.push((me, x.target));
                        }
                    } else if ch == designcraft_doc::NOTE_MARK {
                        // Editorial notes aren't written to IDML yet.
                    } else if ch == designcraft_doc::ENDNOTE_REF {
                        // Endnotes aren't written to IDML yet: the endnote frame keeps their text
                        // (numbers included) and the references are left out.
                    } else if ch == designcraft_doc::FOOTNOTE_REF {
                        // The reference: its own range carrying the reference position.
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        if !out.is_empty() {
                            let mut c = self.csr_el(&f);
                            c.children.append(&mut out);
                            csrs.push(c);
                        }
                        if let Some(n) = s.notes.get(s.notes_before(seg_start + ci)) {
                            let mut c = self.csr_el(&f);
                            let pos = self.d.footnote_options.ref_position;
                            if pos != designcraft_doc::Position::Normal {
                                c.set("Position", names::position_out(pos));
                            }
                            c.push(self.footnote_el(&n.text));
                            csrs.push(c);
                        }
                    } else if let Some(bt) = brk {
                        flush_text(&mut cur, &mut pending);
                        flush_content(&mut pending, &mut out);
                        if !out.is_empty() {
                            let mut c = self.csr_el(&f);
                            c.children.append(&mut out);
                            csrs.push(c);
                        }
                        let mut c = self.csr_el(&f);
                        c.set("ParagraphBreakType", bt);
                        c.push(El::new("Br"));
                        csrs.push(c);
                    } else {
                        cur.push(ch);
                    }
                }
                flush_text(&mut cur, &mut pending);
                flush_content(&mut pending, &mut out);
                let is_last_seg = k + 1 == nseg;
                if is_last_seg && !last {
                    out.push(Node::El(El::new("Br")));
                }
                if !out.is_empty() || csrs.is_empty() {
                    let mut c = self.csr_el(&f);
                    c.children.extend(out);
                    csrs.push(c);
                }
                for c in csrs {
                    psr.push(c);
                }
            }
            paras.push(psr);
        }
        paras
    }
}
