//! Pointer selection, text dragging, and caret navigation.

use super::{
    CellAddr, Content, ItemId, Point, Result, Selection, Session, StoryId, TextSel, Value, bad, bool_or, compose, id_param, json, next_char,
    next_word, ok, point_param, prev_char, prev_word, str_param,
};

/// Story byte at spread point `pt` in text frame `frame` (in a table cell's story when the point is in a cell).
fn hit_byte(s: &Session, frame: ItemId, pt: Point) -> Option<(StoryId, usize, Option<CellAddr>)> {
    let st = s.active()?;
    let loc = st.doc.find(frame)?;
    let it = st.doc.item_at(&loc)?;
    let sid = it.text_frame()?.story;
    let xf = st.doc.parent_xf(&loc) * st.doc.text_xf(it);
    let inner = xf.inverse() * pt;
    let cs = s.cache.get(&st.doc, sid, None);
    let fi = cs.frames.iter().position(|f| f.frame == frame)?;
    if let Some((id, b)) = compose::hit_annotation(&cs, fi, inner) {
        let entry = st.doc.publication.as_ref()?.root.annotation_entries().ok()?.into_iter().find(|e| e.identity.id == id)?;
        return Some((entry.story, b, Some(CellAddr::annotation(id))));
    }
    if let Some((table, row, col, b)) = compose::hit_cell(&cs, fi, inner) {
        return Some((sid, b, Some(CellAddr { table, row, col })));
    }
    if let Some((id, b)) = compose::hit_note(&cs, fi, inner) {
        return Some((sid, b, Some(CellAddr::footnote(id))));
    }
    let b = compose::hit(&cs, fi, inner).unwrap_or(0);
    Some((sid, b, None))
}

pub(super) fn place(s: &mut Session, p: &Value, extend: bool) -> Result<Value> {
    let frame = id_param(p, "frame").ok_or_else(|| bad("text.placeCaret", "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    // Empty frames become text frames.
    let needs_convert = s.doc()?.doc.item(frame).is_some_and(|i| matches!(i.content, Content::Unassigned));
    if needs_convert {
        s.execute("object.content", &json!({"ids": [frame.0], "type": "text"}))?;
    }
    let (sid, b, cell) = hit_byte(s, frame, pt).ok_or_else(|| bad("text.placeCaret", "not a text frame"))?;
    if let Some(id) = cell.and_then(|c| c.annotation_id()) {
        let v = s.doc()?.doc.publication.as_ref().ok_or_else(|| bad("text.placeCaret", "publication missing"))?;
        if v.root.annotation_entries()?.iter().any(|e| e.identity.id == id && e.component != v.component) {
            let mut result = s.execute("publication.note.select", &json!({"id":id,"anchor":b,"focus":b}))?;
            result["noteOwnerActivated"] = json!(true);
            return Ok(result);
        }
    }
    // Drag and drop: a press inside the selected text keeps it (to be dragged).
    if !extend {
        s.text_drag = s.doc()?.selection.text.is_some_and(|t| t.story == sid && t.cell == cell && !t.range().is_empty() && t.range().contains(&b));
        if s.text_drag {
            return Ok(json!({"story": sid.0, "dragging": true}));
        }
    } else if s.text_drag {
        return Ok(json!({"story": sid.0, "dragging": true}));
    }
    let st = s.doc_mut()?;
    // Dragging from one cell into another selects cells.
    if extend
        && let (Some(t), Some(c)) = (st.selection.text, cell)
        && let Some(a) = t.cell
        && t.story == sid
        && a.table == c.table
        && a.annotation_id().is_none()
        && a.footnote_id().is_none()
        && (a.row, a.col) != (c.row, c.col)
    {
        let range = designcraft_doc::CellRange::new(a.row, a.col, c.row, c.col);
        st.selection.cells = Some(designcraft_doc::TableSel { story: sid, table: c.table, range });
        st.revision += 1;
        return Ok(json!({"story": sid.0, "cells": range}));
    }
    let t = match (extend, st.selection.text) {
        (true, Some(t)) if t.story == sid && t.cell == cell => TextSel { focus: b, frame: Some(frame), ..t },
        _ => TextSel { story: sid, anchor: b, focus: b, frame: Some(frame), cell },
    };
    st.selection = Selection::text(t);
    st.revision += 1;
    Ok(json!({"story": sid.0, "pos": b, "cell": cell}))
}

pub(super) fn release(s: &mut Session, p: &Value) -> Result<Value> {
    if !std::mem::take(&mut s.text_drag) {
        return ok();
    }
    let frame = id_param(p, "frame").ok_or_else(|| bad("text.release", "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    if !p.get("moved").and_then(Value::as_bool).unwrap_or(false) {
        // A click in the selection: the caret goes there.
        s.doc_mut()?.selection.text = None;
        return place(s, &json!({"frame": frame.0, "point": [pt.x, pt.y]}), false);
    }
    let copy = p.get("copy").and_then(Value::as_bool).unwrap_or(false);
    let Some(t) = s.doc()?.selection.text else { return ok() };
    let Some((sid, b, cell)) = hit_byte(s, frame, pt) else { return ok() };
    let r = t.range();
    if sid != t.story || cell != t.cell || (r.contains(&b) && !copy) {
        return ok();
    }
    s.edit(|d, sel| {
        let st = d.text_story_mut(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let piece = st.extract(r.clone());
        let mut at = b;
        if !copy {
            st.delete(r.clone());
            if at > r.start {
                at -= r.len().min(at - r.start);
            }
        }
        let end = st.insert_story(at, &piece);
        sel.text = Some(TextSel { story: sid, anchor: at, focus: end, frame: Some(frame), cell });
        Ok(json!({"moved": piece.text.len(), "at": at}))
    })
}

pub(super) fn move_caret(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = str_param(p, "dir").unwrap_or("right").to_string();
    let extend = bool_or(p, "extend", false);
    let word = bool_or(p, "word", false);
    let st = s.doc()?;
    let t = st.selection.text.ok_or_else(|| bad("text.move", "no caret"))?;
    let story = st.doc.text_story(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
    let text = story.text.clone();
    let cs = s.cache.get(&st.doc, t.story, None);
    // In a cell, lines come from the cell's own composition.
    let cs = match t.cell {
        Some(c) if c.annotation_id().is_some() => {
            let mut joined = compose::ComposedStory::default();
            for sid in st.doc.stories.keys() {
                let carrier = s.cache.get(&st.doc, *sid, None);
                for (_, _, fragment) in compose::annotation_fragments(&carrier, c.annotation_id().unwrap_or(0)) {
                    joined.frames.extend(fragment.frames.iter().cloned());
                }
            }
            std::sync::Arc::new(joined)
        }
        Some(c) if c.footnote_id().is_some() => compose::find_note(&cs, c.row as u64).map(|(_, n)| n.text.clone()).unwrap_or(cs),
        Some(c) => compose::find_cell(&cs, c.table, c.row, c.col).map(|(_, _, pc)| pc.text.clone()).unwrap_or(cs),
        None => cs,
    };
    let pos = t.focus.min(text.len());
    // Vertical lines run top to bottom and follow each other right to left: Down and Up move along
    // the line (Right and Left of horizontal text), Left and Right to the next and previous line.
    let vertical_text = compose::caret(&cs, pos).and_then(|(fi, ..)| cs.frames.get(fi)).is_some_and(|f| f.vertical);
    let dir = match (vertical_text, dir.as_str()) {
        (true, "down") => "right",
        (true, "up") => "left",
        (true, "left") => "down",
        (true, "right") => "up",
        (_, d) => d,
    };
    let collapse_to = |left: bool| if left { t.range().start } else { t.range().end };
    let new = match dir {
        "left" if !extend && !t.is_caret() => collapse_to(true),
        "right" if !extend && !t.is_caret() => collapse_to(false),
        "left" | "right" => {
            // Lines with right-to-left text move as drawn; at a line's edge, or by words, a
            // right-to-left line's "left" is forward in the text.
            let left = dir == "left";
            let visual = if word { None } else { compose::visual_step(&cs, pos, left) };
            match visual {
                Some(p) => p,
                None => {
                    let forward = left == compose::line_rtl(&cs, pos);
                    match (forward, word) {
                        (true, true) => next_word(&text, pos),
                        (true, false) => next_char(&text, pos),
                        (false, true) => prev_word(&text, pos),
                        (false, false) => prev_char(&text, pos),
                    }
                }
            }
        }
        "up" | "down" => vertical(&cs, pos, dir == "up").unwrap_or(pos),
        "lineStart" => line_of(&cs, pos).map(|l| l.0).unwrap_or(0),
        "lineEnd" => line_of(&cs, pos).map(|l| l.1).unwrap_or(text.len()),
        "storyStart" => 0,
        "storyEnd" => text.len(),
        other => return Err(bad("text.move", format!("unknown dir `{other}`"))),
    };
    let stm = s.doc_mut()?;
    stm.selection.text = Some(if extend { TextSel { focus: new, ..t } } else { TextSel { anchor: new, focus: new, ..t } });
    stm.revision += 1;
    Ok(json!({"pos": new}))
}

fn line_of(cs: &compose::ComposedStory, pos: usize) -> Option<(usize, usize)> {
    cs.frames.iter().flat_map(|f| f.lines.iter()).find(|l| pos >= l.range.start && pos <= l.range.end).map(|l| (l.range.start, l.range.end))
}

fn vertical(cs: &compose::ComposedStory, pos: usize, up: bool) -> Option<usize> {
    let (fi, x, baseline, _, _) = compose::caret(cs, pos)?;
    let lines: Vec<(usize, &compose::Line)> = cs.frames.iter().enumerate().flat_map(|(i, f)| f.lines.iter().map(move |l| (i, l))).collect();
    let cur = lines.iter().position(|(i, l)| *i == fi && (l.baseline - baseline).abs() < 0.01 && pos >= l.range.start && pos <= l.range.end)?;
    let target = if up { cur.checked_sub(1)? } else { cur + 1 };
    let (tf, tl) = lines.get(target)?;
    compose::hit(cs, *tf, Point::new(x, tl.baseline - 1.0))
}
