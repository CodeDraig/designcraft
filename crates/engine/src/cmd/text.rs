//! Text editing (Type tool) and character/paragraph formatting.

mod caret;
use caret::{move_caret, place, release};
mod editing;
pub(crate) use editing::delete_selection;
pub use editing::quote_marks;
use editing::{change_case, delete, insert};
mod boundaries;
use boundaries::word_bounds;
pub use boundaries::{next_char, next_word, prev_char, prev_word};
mod formatting;
use formatting::{break_link, open_type, selection_attrs, step_size};
pub(crate) use formatting::{format_chars, format_paras, format_targets};
mod placeholder;
pub use placeholder::PLACEHOLDER;
use placeholder::fill_placeholder;

use designcraft_compose as compose;
use designcraft_doc::story::floor_char_boundary;
use designcraft_doc::{CellAddr, CharAttrs, Content, ItemId, ParaAttrs, Selection, StoryId, TextSel};
use designcraft_geom::Point;
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_doc, has_text, has_text_or_frames, id_param, ok, point_param, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "text.placeCaret", "Place Caret", [], None, "{frame, point: [x,y] (spread coords)} — converts empty frames to text frames", has_doc, |s, p| place(s, p, false)),
        cmd!(
            "text.release",
            "Release",
            [],
            None,
            "{frame, point, moved: bool, copy?: bool} — ends a press in text: a drag that started in the selected text moves (copy: duplicates) it to the point; a click places the caret",
            has_doc,
            release
        ),
        cmd!(noundo "text.extendTo", "Extend Text Selection", [], None, "{frame, point}", has_doc, |s, p| place(s, p, true)),
        cmd!(noundo "text.selectWord", "Select Word", [], None, "{frame, point}", has_doc, |s, p| {
            place(s, p, false)?;
            let st = s.doc_mut()?;
            let Some(t) = st.selection.text else { return ok() };
            let text = &st.doc.text_story(t.story, t.cell).map(|x| x.text.clone()).unwrap_or_default();
            let (a, b) = word_bounds(text, t.focus);
            st.selection.text = Some(TextSel { anchor: a, focus: b, ..t });
            ok()
        }),
        cmd!(noundo "text.select", "Select Text", [], None, "{story, anchor, focus} (UTF-8 byte offsets into the story text, as find.find reports them: á or — counts 2 or 3)", has_doc, |s, p| {
            let sid = StoryId(p.get("story").and_then(Value::as_u64).unwrap_or(0));
            let st = s.doc_mut()?;
            let text = &st.doc.story(sid).ok_or_else(|| bad("text.select", "no such story"))?.text;
            let a = floor_char_boundary(text, p.get("anchor").and_then(Value::as_u64).unwrap_or(0) as usize);
            let f = floor_char_boundary(text, p.get("focus").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(a));
            st.selection = Selection::text(TextSel { story: sid, anchor: a, focus: f, frame: None, cell: None });
            st.revision += 1;
            ok()
        }),
        cmd!("text.insert", "Type", [], None, "{text, raw?: bool (no typographer's quotes)} — replaces the selected text", has_text, insert),
        cmd!("text.delete", "Delete Text", [], None, "{forward?: bool, word?: bool}", has_text, delete),
        cmd!(noundo "text.move", "Move Caret", [], None, "{dir: left|right|up|down|lineStart|lineEnd|storyStart|storyEnd, extend?, word?}", has_text, move_caret),
        cmd!(noundo "text.exitToFrame", "Select Frame", [], None, "{}", has_doc, |s, _| {
            let st = s.doc_mut()?;
            if let Some(t) = st.selection.text.take() {
                let f = t.frame.or_else(|| st.doc.story(t.story).and_then(|x| x.frames.first().copied()));
                st.selection.items = f.into_iter().collect();
            }
            st.selection.cells = None;
            st.revision += 1;
            ok()
        }),
        cmd!(
            "story.setDirection",
            "Story Column Direction",
            ["Type", "Story Column Direction"],
            None,
            "{story?, direction: leftToRight|rightToLeft} — column progression, independent of paragraph direction",
            has_doc,
            |s, p| {
                let sid = story_of(s, p).ok_or_else(|| bad("story.setDirection", "no story"))?;
                let direction: designcraft_doc::TextDirection =
                    serde_json::from_value(p.get("direction").cloned().ok_or_else(|| bad("story.setDirection", "missing direction"))?)
                        .map_err(|e| bad("story.setDirection", e.to_string()))?;
                s.edit(|d, _| {
                    let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                    st.direction = direction;
                    st.rev = st.rev.wrapping_add(1);
                    ok()
                })
            }
        ),
        cmd!("story.setText", "Set Story Text", [], None, "{story, text} — replace a story's whole text", has_doc, |s, p| {
            let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("story.setText", "missing story"))?);
            let text = str_param(p, "text").unwrap_or("").to_string();
            s.edit(|d, sel| {
                d.set_story_text(sid, &text)?;
                if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid) {
                    t.anchor = floor_char_boundary(&text, t.anchor);
                    t.focus = floor_char_boundary(&text, t.focus);
                }
                ok()
            })
        }),
        cmd!(
            "story.replaceRange",
            "Edit Story",
            [],
            None,
            "{story, start, end, text} — replace a byte range keeping surrounding formatting",
            has_doc,
            |s, p| {
                let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("story.replaceRange", "missing story"))?);
                let a = p.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
                let b = p.get("end").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(a);
                let text = str_param(p, "text").unwrap_or("").to_string();
                let tracking = super::changes::tracking(&s.doc()?.doc);
                s.edit(|d, sel| {
                    let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                    let (a, b) = (a.min(st.len()), b.min(st.len()).max(a.min(st.len())));
                    let a = floor_char_boundary(&st.text, a);
                    let b = floor_char_boundary(&st.text, b);
                    super::changes::replace(st, a..b, &text, tracking);
                    let len = st.len();
                    if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid) {
                        t.anchor = floor_char_boundary(&st.text, t.anchor);
                        t.focus = floor_char_boundary(&st.text, t.focus);
                    }
                    Ok(json!({"length": len}))
                })
            }
        ),
        cmd!(query "story.get", "Get Story", [], None, "{story? | frame?} → text, frames, paragraphs, vertical, overset", has_doc, |s, p| {
            let st = s.doc()?;
            let sid = story_of(s, p).ok_or_else(|| bad("story.get", "no story"))?;
            let cs = s.cache.get(&st.doc, sid, None);
            let mut v = st.doc.story_summary(sid).unwrap_or_default();
            v["overset"] = json!(cs.overset_at);
            v["lines"] = json!(cs.line_count());
            Ok(v)
        }),
        cmd!("type.fillWithPlaceholder", "Fill with Placeholder Text", ["Type"], None, "{frame?}", has_text_or_frames, fill_placeholder),
        cmd!(
            "type.char",
            "Character Formatting",
            [],
            None,
            "{attrs: {fontFamily?, fontStyle?, size?, leading?: {kind:auto}|{kind:points,value}, tracking?, kerning?, hScale?, vScale?, baselineShift?, fill?, capitalization?, underline?, …}}",
            has_text_or_frames,
            |s, p| format_chars(s, p.get("attrs").unwrap_or(p))
        ),
        cmd!(
            "type.para",
            "Paragraph Formatting",
            [],
            None,
            "{attrs: {align?, leftIndent?, firstLineIndent?, spaceBefore?, spaceAfter?, dropCapLines?, hyphenate?, composer?, tabs?, …}}",
            has_text_or_frames,
            |s, p| format_paras(s, p.get("attrs").unwrap_or(p))
        ),
        cmd!("type.align", "Align", [], None, "{align: left|center|right|leftJustified|…}", has_text_or_frames, |s, p| {
            format_paras(s, &json!({"align": p.get("align").cloned().unwrap_or(json!("left"))}))
        }),
        cmd!("type.alignLeft", "Align Left", [], Some("Cmd+Shift+L"), "{}", has_text_or_frames, |s, _| format_paras(s, &json!({"align": "left"}))),
        cmd!("type.alignCenter", "Align Center", [], Some("Cmd+Shift+C"), "{}", has_text_or_frames, |s, _| format_paras(
            s,
            &json!({"align": "center"})
        )),
        cmd!("type.alignRight", "Align Right", [], Some("Cmd+Shift+R"), "{}", has_text_or_frames, |s, _| format_paras(s, &json!({"align": "right"}))),
        cmd!("type.justify", "Justify Left", [], Some("Cmd+Shift+J"), "{}", has_text_or_frames, |s, _| format_paras(
            s,
            &json!({"align": "leftJustified"})
        )),
        cmd!("type.bold", "Bold", [], Some("Cmd+Shift+B"), "{}", has_text_or_frames, |s, _| format_chars(s, &json!({"fontStyle": "Bold"}))),
        cmd!("type.italic", "Italic", [], Some("Cmd+Shift+I"), "{}", has_text_or_frames, |s, _| format_chars(s, &json!({"fontStyle": "Italic"}))),
        cmd!("type.underline", "Underline", [], Some("Cmd+Shift+U"), "{}", has_text_or_frames, |s, _| format_chars(s, &json!({"underline": true}))),
        cmd!("type.allCaps", "All Caps", [], Some("Cmd+Shift+K"), "{}", has_text_or_frames, |s, _| format_chars(
            s,
            &json!({"capitalization": "allCaps"})
        )),
        cmd!(
            "type.tateChuYoko",
            "Tate-Chu-Yoko",
            [],
            None,
            "{on?: bool} — set the selected text horizontally within one em of a vertical line (toggles by default)",
            has_text_or_frames,
            |s, p| {
                let on = match p.get("on").and_then(Value::as_bool) {
                    Some(v) => v,
                    None => !format_targets(s).first().is_some_and(|t| {
                        s.doc().ok().and_then(|d| d.doc.text_story(t.story, t.cell)).is_some_and(|st| {
                            st.char_format_at(if t.range.is_empty() { t.range.start } else { t.range.start + 1 }).over.tate_chu_yoko == Some(true)
                        })
                    }),
                };
                format_chars(s, &json!({"tate_chu_yoko": on}))
            }
        ),
        cmd!(
            "type.ruby",
            "Ruby",
            [],
            None,
            "{text} — the reading set over the selected text (one group); empty removes it",
            has_text_or_frames,
            |s, p| { format_chars(s, &json!({"ruby": str_param(p, "text").unwrap_or("")})) }
        ),
        cmd!(
            "type.kenten",
            "Kenten",
            [],
            None,
            "{on?: bool} — emphasis dots over the selected characters (toggles by default)",
            has_text_or_frames,
            |s, p| {
                let on = match p.get("on").and_then(Value::as_bool) {
                    Some(v) => v,
                    None => !format_targets(s).first().is_some_and(|t| {
                        s.doc().ok().and_then(|d| d.doc.text_story(t.story, t.cell)).is_some_and(|st| {
                            st.char_format_at(if t.range.is_empty() { t.range.start } else { t.range.start + 1 }).over.kenten == Some(true)
                        })
                    }),
                };
                format_chars(s, &json!({"kenten": on}))
            }
        ),
        cmd!(
            "type.storyDirection",
            "Story Direction",
            [],
            None,
            "{vertical: bool} — the stories of the text selection or the selected frames: vertical lines run top to bottom and follow each other right to left, in every frame of the thread",
            has_text_or_frames,
            story_direction
        ),
        cmd!("type.sizeUp", "Increase Point Size", [], Some("Cmd+Shift+."), "{}", has_text_or_frames, |s, _| step_size(s, 2.0)),
        cmd!("type.sizeDown", "Decrease Point Size", [], Some("Cmd+Shift+,"), "{}", has_text_or_frames, |s, _| step_size(s, -2.0)),
        cmd!(
            "type.changeCase",
            "Change Case",
            ["Type"],
            None,
            "{case: upper|lower|title|sentence} — the selected text, or the stories of selected frames (formatting kept)",
            has_text_or_frames,
            change_case
        ),
        cmd!(
            "type.openType",
            "OpenType",
            [],
            None,
            "{feature?: dlig|frac|ordn|swsh|titl|calt|zero|ssNN|any tag, on?: bool (default: toggle), figures?: tabularLining|proportionalOldstyle|proportionalLining|tabularOldstyle|default, stylisticSets?: [1–20]} → {features, figures}",
            has_text_or_frames,
            open_type
        ),
        cmd!(
            "style.breakLink",
            "Break Link to Style",
            [],
            None,
            "{kind: paragraph|character} — the selected text keeps its look as local formatting, with [No Paragraph Style] / [None]",
            has_text_or_frames,
            break_link
        ),
        cmd!(query "type.selectionAttrs", "Selection Attributes", [], None, "{} → resolved character/paragraph attributes at the text selection", has_doc, selection_attrs),
    ]
}

pub(crate) fn story_of(s: &Session, p: &Value) -> Option<StoryId> {
    let st = s.active()?;
    if let Some(v) = p.get("story").and_then(Value::as_u64) {
        return Some(StoryId(v));
    }
    let frame = id_param(p, "frame").or_else(|| st.selection.items.first().copied());
    if let Some(f) = frame {
        return st.doc.item(f).and_then(|i| i.text_frame()).map(|t| t.story);
    }
    st.selection.text.map(|t| t.story)
}

/// Type ▸ Story Direction.
fn story_direction(s: &mut Session, p: &Value) -> Result<Value> {
    let vertical = p.get("vertical").and_then(Value::as_bool).ok_or_else(|| bad("type.storyDirection", "missing vertical"))?;
    let st = s.doc()?;
    let mut stories: Vec<StoryId> = st.selection.text.map(|t| t.story).into_iter().chain(st.selection.cells.map(|c| c.story)).collect();
    for id in &st.selection.items {
        if let Some(tf) = st.doc.item(*id).and_then(|i| i.text_frame())
            && !stories.contains(&tf.story)
        {
            stories.push(tf.story);
        }
    }
    s.edit(|d, _| {
        set_story_direction(d, &stories, vertical);
        Ok(json!({"stories": stories.len()}))
    })
}

/// Set the direction of `stories` (all their frames turn together).
pub(crate) fn set_story_direction(d: &mut designcraft_doc::Document, stories: &[StoryId], vertical: bool) {
    for sid in stories {
        if let Some(st) = d.story_mut(*sid)
            && st.vertical != vertical
        {
            st.vertical = vertical;
            st.rev += 1;
        }
    }
}

#[cfg(test)]
mod open_type_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn text_selection_stays_on_utf8_boundaries() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 200, 100], "content": "text", "text": "é漢🙂x"})).unwrap();
        let sid = r["story"].clone();
        s.execute("text.select", &json!({"story": sid, "anchor": 1, "focus": 4})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (0, 2));
        s.execute("text.select", &json!({"story": sid, "anchor": 4})).unwrap();
        s.execute("text.delete", &json!({"forward": true})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(t.story).unwrap().text, "é🙂x");
        s.execute("text.select", &json!({"story": sid, "anchor": 2})).unwrap();
        s.execute("story.setText", &json!({"story": sid, "text": "漢字"})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (0, 0));
        s.execute("text.move", &json!({"dir": "right"})).unwrap();
        assert_eq!(s.doc().unwrap().selection.text.unwrap().focus, 3);
    }

    #[test]
    fn replacing_a_range_stays_on_utf8_boundaries() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 200, 100], "content": "text", "text": "é漢🙂x"})).unwrap();
        let sid = r["story"].clone();
        // Offsets inside 漢 (2..5) and 🙂 (5..9) snap back to the character starts: 漢 is replaced.
        s.execute("story.replaceRange", &json!({"story": sid, "start": 3, "end": 6, "text": "字"})).unwrap();
        let story = designcraft_doc::StoryId(sid.as_u64().unwrap());
        assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "é字🙂x");
        // A caret before 🙂 (byte 5) would land inside it once é (2 bytes) is gone.
        s.execute("text.select", &json!({"story": sid, "anchor": 5})).unwrap();
        s.execute("story.replaceRange", &json!({"story": sid, "start": 0, "end": 2, "text": ""})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.unwrap();
        let text = &st.doc.story(t.story).unwrap().text;
        assert_eq!(text, "字🙂x");
        assert!(text.is_char_boundary(t.anchor) && text.is_char_boundary(t.focus), "caret {t:?} in {text:?}");
        s.execute("text.move", &json!({"dir": "right"})).unwrap();
    }

    #[test]
    fn open_type_toggles_reach_the_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Office 1/2 0"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 12})).unwrap();
        let r = s.execute("type.openType", &json!({"feature": "dlig"})).unwrap();
        assert_eq!(r["features"], json!(["dlig"]));
        let r = s.execute("type.openType", &json!({"feature": "dlig"})).unwrap();
        assert_eq!(r["features"], json!([]), "toggles off");
        let r = s.execute("type.openType", &json!({"figures": "proportionalOldstyle", "stylisticSets": [1, 2]})).unwrap();
        assert_eq!(r["figures"], "proportionalOldstyle");
        let a = s.execute("type.selectionAttrs", &json!({})).unwrap();
        let f: Vec<&str> = a["chars"]["otfFeatures"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert!(f.contains(&"onum") && f.contains(&"ss02"), "{f:?}");
        assert!(s.execute("type.openType", &json!({"feature": "bad tag!"})).is_err());
    }

    #[test]
    fn break_link_to_paragraph_and_character_styles() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head", "para": {"spaceBefore": 6}, "chars": {"size": 24}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Em", "chars": {"fontStyle": "Italic"}})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Title here"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 10})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 5})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Em"})).unwrap();
        let before = s.execute("type.selectionAttrs", &json!({})).unwrap();
        s.execute("style.breakLink", &json!({"kind": "character"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 10})).unwrap();
        s.execute("style.breakLink", &json!({"kind": "paragraph"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 5})).unwrap();
        let after = s.execute("type.selectionAttrs", &json!({})).unwrap();
        assert_eq!(after["paragraphStyle"], "[No Paragraph Style]");
        assert_eq!(after["characterStyle"], "[None]");
        assert_eq!(after["chars"], before["chars"], "same look");
        assert_eq!(after["para"], before["para"]);
    }

    /// Old-style figures shape to other glyphs than the default lining ones (Source Serif 4).
    #[test]
    fn oldstyle_figures_change_the_glyphs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "2024"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let gids = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, sid, &Default::default());
            cs.frames[0].lines[0].glyphs.iter().map(|g| g.gid).collect::<Vec<_>>()
        };
        let lining = gids(&s);
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.openType", &json!({"figures": "proportionalOldstyle"})).unwrap();
        assert_ne!(gids(&s), lining);
    }
}

#[cfg(test)]
mod nested_style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn nested_and_grep_styles_colour_the_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Lead", "chars": {"fill": "C=100 M=0 Y=0 K=0"}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Num", "chars": {"fill": "C=0 M=100 Y=0 K=0"}})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Opening words then 2026 here"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
        s.execute(
            "type.para",
            &json!({"attrs": {
                "nestedStyles": [{"style": "Lead", "through": true, "count": 2, "until": {"kind": "words"}}],
                "grepStyles": [{"style": "Num", "pattern": "\\d+"}]
            }}),
        )
        .unwrap();
        let d = s.doc().unwrap().doc.clone();
        let cs = designcraft_compose::compose_story(&d, designcraft_doc::StoryId(sid), &Default::default());
        let text = &d.story(designcraft_doc::StoryId(sid)).unwrap().text;
        let fill_at = |byte: usize| {
            let g = cs.frames[0].lines.iter().flat_map(|l| &l.glyphs).find(|g| g.byte == byte).unwrap();
            cs.styles[g.style as usize].fill.clone()
        };
        assert_eq!(fill_at(0), "C=100 M=0 Y=0 K=0", "nested: first two words");
        assert_eq!(fill_at(text.find("words").unwrap()), "C=100 M=0 Y=0 K=0");
        assert_eq!(fill_at(text.find("then").unwrap()), "[Black]");
        assert_eq!(fill_at(text.find("2026").unwrap()), "C=0 M=100 Y=0 K=0", "GREP: digits");
    }
}

#[cfg(test)]
mod drag_text_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn drag_selected_text_to_move_or_copy_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "Hello brave world"})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        // Spread point of the glyph at byte `b`.
        let at = |s: &Session, b: usize| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let l = &cs.frames[0].lines[0];
            let g = l.glyphs.iter().find(|g| g.byte == b).unwrap();
            json!([g.x + g.adv * 0.25, l.baseline - 3.0])
        };
        let end = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let l = &cs.frames[0].lines[0];
            json!([l.end_x + 2.0, l.baseline - 3.0])
        };
        s.execute("text.select", &json!({"story": sid, "anchor": 6, "focus": 12})).unwrap();
        let r = s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 8)})).unwrap();
        assert_eq!(r["dragging"], true);
        s.execute("text.extendTo", &json!({"frame": fid, "point": end(&s)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": end(&s), "moved": true})).unwrap();
        let text = |s: &Session| s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
        assert_eq!(text(&s), "Hello worldbrave ");
        // Alt-drag copies (the moved text is still selected).
        s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 12)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": at(&s, 0), "moved": true, "copy": true})).unwrap();
        assert_eq!(text(&s), "brave Hello worldbrave ");
        // A click inside the selection just places the caret.
        s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 2)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": at(&s, 2), "moved": false})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!(t.anchor, t.focus);
    }
}

#[cfg(test)]
mod nested_line_style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn first_line_takes_the_nested_line_style() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Lead", "chars": {"size": 18}})).unwrap();
        let text = "The opening line of this paragraph reads larger, and the lines after it return to the body size again.";
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 400], "content": "text", "text": text})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.para", &json!({"nestedLineStyles": [{"style": "Lead", "lines": 1}]})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
        let lines = &cs.frames[0].lines;
        assert!(lines.len() >= 3);
        let size_of = |l: usize| cs.styles[lines[l].glyphs.iter().find(|g| g.visible).unwrap().style as usize].size;
        assert_eq!(size_of(0), 18.0, "the first line");
        assert!(
            lines[0].glyphs.iter().filter(|g| g.visible && g.adv > 0.0).all(|g| cs.styles[g.style as usize].size == 18.0),
            "the whole first line"
        );
        assert_eq!(size_of(1), 12.0, "the next lines");
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        let st = back.stories.values().find(|st| st.text.starts_with("The opening")).unwrap();
        let nl = st.paras[0].para.nested_line_styles.clone().unwrap_or_default();
        assert_eq!((nl.len(), nl.first().map(|n| n.lines)), (1, Some(1)), "IDML AllNestedLineStyles");
        assert_eq!(nl[0].style, "Lead");
    }
}

#[cfg(test)]
mod border_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn paragraph_border_and_shading_follow_each_column() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "word ".repeat(80);
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 160], "content": "text", "text": text, "caret": false})).unwrap();
        s.execute("object.textFrameOptions", &json!({"ids": [r["id"]], "columns": 2})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.para", &json!({"borderOn": true, "borderWeights": [2, 1, 2, 1], "borderOffsets": [4, 4, 4, 4], "borderColor": "[Black]", "shadingOn": true, "shadingOffsets": [4, 4, 4, 4]})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
        let decos = &cs.frames[0].decos;
        let shading: Vec<_> = decos.iter().filter(|d| d.tint < 0.5).collect();
        assert_eq!(shading.len(), 2, "one shaded box per column");
        let edges = decos.len() - shading.len();
        assert_eq!(edges, 6, "top on the first part, bottom on the last, sides on both");
        let top = decos.iter().filter(|d| d.tint >= 0.5).map(|d| d.rect).fold(f64::MAX, |a, r| a.min(r.y0));
        let first_line = &cs.frames[0].lines[0];
        assert!((top - (first_line.baseline - first_line.ascent - 4.0 - 2.0)).abs() < 1e-6, "offset 4 + weight 2 above the text");
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        let p = &back.stories.values().find(|st| st.text.starts_with("word")).unwrap().paras[0].para;
        assert_eq!(
            (p.border_on, p.border_weights, p.shading_offsets),
            (Some(true), Some([2.0, 1.0, 2.0, 1.0]), Some([4.0; 4])),
            "IDML paragraph border"
        );
    }
}

#[cfg(test)]
mod autocorrect_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn autocorrect_fixes_the_word_before_a_space() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        let text = |s: &Session| s.doc().unwrap().doc.stories[&sid].text.clone();
        // Off by default.
        for c in ["t", "e", "h", " "] {
            s.execute("text.insert", &json!({"text": c})).unwrap();
        }
        assert_eq!(text(&s), "teh ");
        s.execute("prefs.set", &json!({"autocorrect": true})).unwrap();
        for c in ["T", "e", "h", " ", "c", "a", "t", "."] {
            s.execute("text.insert", &json!({"text": c})).unwrap();
        }
        assert_eq!(text(&s), "teh The cat.");
        // The caret ends after the inserted punctuation.
        assert_eq!(s.doc().unwrap().selection.text.unwrap().focus, "teh The cat.".len());
    }
}

#[cfg(test)]
mod language_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn language_sets_quotes_spelling_and_hyphenation() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("type.char", &json!({"attrs": {"language": "German: 2006 Reform"}})).unwrap();
        s.execute("text.insert", &json!({"text": "\"Wort\" ist's"})).unwrap();
        let text = s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].text.clone();
        assert_eq!(text, "\u{201E}Wort\u{201C} ist\u{2019}s");
        // German words aren't English misspellings.
        let st = s.execute("spelling.check", &json!({"story": sid})).unwrap();
        assert!(st.as_array().unwrap().is_empty(), "{st}");
    }

    #[test]
    fn cjk_languages_have_their_quotes_and_no_english_rules() {
        use super::quote_marks;
        assert_eq!(quote_marks("Chinese: Simplified"), ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}']);
        assert_eq!(quote_marks("Chinese: Traditional"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        assert_eq!(quote_marks("Chinese"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        assert_eq!(quote_marks("Japanese"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        // Other spellings of the language names take the same quotes.
        assert_eq!(quote_marks("Simplified Chinese"), ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}']);
        assert_eq!(quote_marks("zh_CN"), ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}']);
        assert_eq!(quote_marks("Chinese (Traditional)"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        assert_eq!(quote_marks("zh-TW"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        assert_eq!(quote_marks("ja_JP"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        for l in ["Japanese", "Korean", "Chinese", "Chinese: Simplified", "Chinese: Traditional"] {
            assert!(!designcraft_compose::is_english(l), "{l}: no English hyphenation or spelling");
        }
    }

    #[test]
    fn locale_coded_languages_take_their_quotes_and_rules() {
        use super::quote_marks;
        const GERMAN: [char; 4] = ['\u{201E}', '\u{201C}', '\u{201A}', '\u{2018}'];
        const DUTCH: [char; 4] = ['\u{201E}', '\u{201D}', '\u{201A}', '\u{2019}'];
        const GUILLEMETS: [char; 4] = ['\u{00AB}', '\u{00BB}', '\u{2039}', '\u{203A}'];
        const SPANISH: [char; 4] = ['\u{00AB}', '\u{00BB}', '\u{201C}', '\u{201D}'];
        const ENGLISH: [char; 4] = ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}'];
        const CORNERS: [char; 4] = ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}'];
        for (language, quotes, english) in [
            ("de_DE_2006", GERMAN, false),
            ("German: 2006 Reform", GERMAN, false),
            ("de_CH_2006", GUILLEMETS, false),
            ("German: Swiss 2006 Reform", GUILLEMETS, false),
            ("nl_NL_2005", DUTCH, false),
            ("Dutch: 2005 Reform", DUTCH, false),
            ("English: USA", ENGLISH, true),
            ("English: UK", ENGLISH, true),
            ("en_GB", ENGLISH, true),
            ("Spanish: Castilian", SPANISH, false),
            ("es_ES", SPANISH, false),
            ("French", GUILLEMETS, false),
            ("fr_FR", GUILLEMETS, false),
            ("nb_NO", GUILLEMETS, false),
            ("ja_JP", CORNERS, false),
            ("[No Language]", ENGLISH, false),
        ] {
            assert_eq!(quote_marks(language), quotes, "{language}");
            assert_eq!(designcraft_compose::is_english(language), english, "{language}");
        }
    }

    #[test]
    fn idml_language_names_keep_their_spelling_and_pick_the_language() {
        use std::io::Read;
        // A stand-in for Songti SC (first in the Simplified Chinese chain; the real one when installed).
        designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::font_with("Songti SC", &['直']).unwrap());
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "直"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 3})).unwrap();
        s.execute("type.char", &json!({"attrs": {"language": "Simplified Chinese"}})).unwrap();
        let idml = designcraft_idml::export_idml(&s.doc().unwrap().doc);
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(idml.clone())).unwrap();
        let stories: String = (0..z.len())
            .map(|i| {
                let mut f = z.by_index(i).unwrap();
                let mut x = String::new();
                if f.name().starts_with("Stories/") {
                    f.read_to_string(&mut x).unwrap();
                }
                x
            })
            .collect();
        assert!(stories.contains(r#"AppliedLanguage="$ID/Simplified Chinese""#), "{stories}");
        // The name comes back as written and sets the text as Simplified Chinese.
        let mut back = designcraft_idml::import_idml(&idml).unwrap();
        back.settings.glyph_fallback = true;
        let st = back.stories.values().find(|st| st.text.contains('直')).unwrap();
        let (_, base) = back.styles.resolve_para(&st.paras[0]);
        assert_eq!(back.styles.resolve_char(&base, st.char_format_at(0)).language, "Simplified Chinese");
        let cs = designcraft_compose::compose_story(&back, st.id, &Default::default());
        let face = cs.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 0).unwrap().face;
        assert_eq!(face.family, "Songti SC");
    }
}

#[cfg(test)]
mod tcy_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn tate_chu_yoko_toggles_on_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 200, 400], "content": "text", "text": "令和12年", "vertical": true})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        let at = "令和".len();
        s.execute("text.select", &json!({"story": sid, "anchor": at, "focus": at + 2})).unwrap();
        let get = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(at + 1).over.tate_chu_yoko;
        s.execute("type.tateChuYoko", &json!({})).unwrap();
        assert_eq!(get(&s), Some(true));
        s.execute("type.tateChuYoko", &json!({})).unwrap();
        assert_eq!(get(&s), Some(false));
        s.execute("type.tateChuYoko", &json!({"on": true})).unwrap();
        assert_eq!(get(&s), Some(true));
    }

    #[test]
    fn ruby_and_kenten_on_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "漢字です"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": "漢字".len()})).unwrap();
        s.execute("type.ruby", &json!({"text": "かんじ"})).unwrap();
        s.execute("type.kenten", &json!({})).unwrap();
        let f = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(1).over.clone();
        assert_eq!(f(&s).ruby.as_deref(), Some("かんじ"));
        assert_eq!(f(&s).kenten, Some(true));
        s.execute("type.kenten", &json!({})).unwrap();
        s.execute("type.ruby", &json!({"text": ""})).unwrap();
        assert_eq!(f(&s).kenten, Some(false));
        assert_eq!(f(&s).ruby.as_deref(), Some(""));
    }
}

#[cfg(test)]
mod bidi_caret_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn arrow_keys_in_a_right_to_left_paragraph() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "سلام عليكم"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("type.para", &json!({"attrs": {"direction": "rightToLeft"}})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        // Left moves forward through right-to-left text, Right back.
        let p = s.execute("text.move", &json!({"dir": "left"})).unwrap()["pos"].as_u64().unwrap();
        assert_eq!(p, 'س'.len_utf8() as u64);
        let p = s.execute("text.move", &json!({"dir": "right"})).unwrap()["pos"].as_u64().unwrap();
        assert_eq!(p, 0);
        // By words too.
        let p = s.execute("text.move", &json!({"dir": "left", "word": true})).unwrap()["pos"].as_u64().unwrap();
        assert!(p >= "سلام".len() as u64, "{p}");
    }
}

#[cfg(test)]
mod arabic_tests {
    use super::*;
    #[test]
    fn arabic_story_direction_command_validates_and_supports_undo() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "abc"})).unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        let before = s.cache.get(&s.doc().unwrap().doc, sid, None);
        s.execute("story.setDirection", &json!({"story": sid.0, "direction": "rightToLeft"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().direction, designcraft_doc::TextDirection::RightToLeft);
        let after = s.cache.get(&s.doc().unwrap().doc, sid, None);
        assert!(!std::sync::Arc::ptr_eq(&before, &after));
        assert!(s.execute("story.setDirection", &json!({"story": sid.0, "direction": "unknown"})).is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().direction, designcraft_doc::TextDirection::LeftToRight);
    }
}

#[cfg(test)]
mod korean_breaks_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn korean_character_breaks_are_a_paragraph_attribute() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "한국어 문장"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("type.para", &json!({"attrs": {"koreanCharBreaks": true}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().paras[0].para.korean_char_breaks, Some(true));
        s.execute("style.paragraph.create", &json!({"name": "Korean", "para": {"koreanCharBreaks": true}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.para("Korean").unwrap().para.korean_char_breaks, Some(true));
    }
}

#[cfg(test)]
mod story_direction_tests {
    use designcraft_doc::{ItemId, StoryId};
    use serde_json::json;

    use crate::Session;

    fn frame(s: &mut Session, rect: [f64; 4], text: &str, vertical: bool) -> (ItemId, StoryId) {
        let r = s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text, "caret": false, "vertical": vertical})).unwrap();
        (ItemId(r["id"].as_u64().unwrap()), StoryId(r["story"].as_u64().unwrap()))
    }

    /// Which frames of the story are composed vertically, in thread order.
    fn composed(s: &Session, sid: StoryId) -> Vec<bool> {
        s.cache.get(&s.doc().unwrap().doc, sid, None).frames.iter().map(|f| f.vertical).collect()
    }

    #[test]
    fn story_direction_turns_every_frame_of_the_story() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (a, sid) = frame(&mut s, [72.0, 72.0, 200.0, 400.0], "縦書きの文章です。", false);
        let (b, _) = frame(&mut s, [300.0, 72.0, 428.0, 400.0], "", false);
        s.edit(|d, _| Ok(d.thread(a, b)?)).unwrap();
        assert_eq!(composed(&s, sid), [false, false]);
        // From the second frame selected, through Type ▸ Story Direction.
        s.execute("selection.set", &json!({"ids": [b.0]})).unwrap();
        s.execute("type.storyDirection", &json!({"vertical": true})).unwrap();
        assert_eq!(composed(&s, sid), [true, true]);
        // From a caret in the text.
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.storyDirection", &json!({"vertical": false})).unwrap();
        assert_eq!(composed(&s, sid), [false, false]);
        // The frame option sets the frame's story.
        s.execute("object.textFrameOptions", &json!({"ids": [a.0], "vertical": true})).unwrap();
        assert_eq!(composed(&s, sid), [true, true]);
        // Undo puts it back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(composed(&s, sid), [false, false]);
    }

    #[test]
    fn a_frame_threaded_onto_a_vertical_story_turns_vertical() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (a, sid) = frame(&mut s, [72.0, 72.0, 200.0, 400.0], "縦書きの文章です。", true);
        let (b, _) = frame(&mut s, [300.0, 72.0, 428.0, 400.0], "", false);
        let c = s.execute("frame.create", &json!({"rect": [450, 72, 550, 400]})).unwrap();
        let c = ItemId(c["id"].as_u64().unwrap());
        s.edit(|d, _| {
            d.thread(a, b)?;
            Ok(d.thread(b, c)?)
        })
        .unwrap();
        assert_eq!(composed(&s, sid), [true, true, true]);
    }
}

#[cfg(test)]
mod vertical_caret_tests {
    use designcraft_doc::StoryId;
    use serde_json::json;

    use crate::Session;

    /// A story set in three or more lines; returns its id.
    fn story(s: &mut Session, vertical: bool) -> StoryId {
        s.execute("file.new", &json!({})).unwrap();
        let text = "一二三四五六七八九十".repeat(8);
        let rect = if vertical { [72, 72, 200, 400] } else { [72, 72, 400, 200] };
        let r = s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text, "vertical": vertical})).unwrap();
        StoryId(r["story"].as_u64().unwrap())
    }

    fn mv(s: &mut Session, dir: &str, extend: bool, word: bool) -> usize {
        s.execute("text.move", &json!({"dir": dir, "extend": extend, "word": word})).unwrap()["pos"].as_u64().unwrap() as usize
    }

    /// (line index, position along the line) of the caret at `pos`.
    fn place(s: &Session, sid: StoryId, pos: usize) -> (usize, f64) {
        let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
        let (_, x, baseline, _, _) = crate::compose::caret(&cs, pos).unwrap();
        let line = cs.frames[0].lines.iter().position(|l| (l.baseline - baseline).abs() < 0.01).unwrap();
        (line, x)
    }

    #[test]
    fn arrow_keys_follow_vertical_lines() {
        let mut s = Session::new();
        let sid = story(&mut s, true);
        assert!(s.cache.get(&s.doc().unwrap().doc, sid, None).frames[0].lines.len() >= 3);
        let c = '一'.len_utf8();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 3 * c, "focus": 3 * c})).unwrap();
        // Down and Up move along the line, a character at a time.
        assert_eq!(mv(&mut s, "down", false, false), 4 * c);
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
        // Left moves to the next line (lines follow each other right to left), keeping the
        // position along the line; Right comes back.
        let (line, x) = place(&s, sid, 3 * c);
        let next = mv(&mut s, "left", false, false);
        let (nline, nx) = place(&s, sid, next);
        assert_eq!(nline, line + 1);
        assert!((nx - x).abs() < 0.5, "{x} {nx}");
        assert_eq!(mv(&mut s, "right", false, false), 3 * c);
        // Shift extends.
        assert_eq!(mv(&mut s, "down", true, false), 4 * c);
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (3 * c, 4 * c));
        let p = mv(&mut s, "left", true, false);
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (3 * c, p));
        assert_eq!(place(&s, sid, p).0, line + 1);
        // With a selection, Up collapses it to its start.
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
        // By words: Alt+Down is the vertical Alt+Right.
        let text = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
        assert_eq!(mv(&mut s, "down", false, true), super::next_word(&text, 3 * c));
    }

    #[test]
    fn arrow_keys_in_horizontal_lines() {
        let mut s = Session::new();
        let sid = story(&mut s, false);
        let c = '一'.len_utf8();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 3 * c, "focus": 3 * c})).unwrap();
        assert_eq!(mv(&mut s, "right", false, false), 4 * c);
        assert_eq!(mv(&mut s, "left", false, false), 3 * c);
        let (line, x) = place(&s, sid, 3 * c);
        let next = mv(&mut s, "down", false, false);
        let (nline, nx) = place(&s, sid, next);
        assert_eq!(nline, line + 1);
        assert!((nx - x).abs() < 0.5, "{x} {nx}");
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
    }
}
