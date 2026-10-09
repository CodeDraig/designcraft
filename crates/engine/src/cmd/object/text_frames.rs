//! Frame content conversion and text-frame options.

use super::{
    Arc, Content, Item, ItemId, Result, Session, Story, StoryId, TextFrame, TextFrameOptions, Value, bad, frame_create, json, ok, rect_param,
    str_param, targets,
};

pub(super) fn content_type(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let t = str_param(p, "type").unwrap_or("unassigned").to_string();
    s.edit(|d, _| {
        for id in &ids {
            let cur = d.item(*id).map(|i| i.content.clone());
            match (t.as_str(), cur) {
                ("text", Some(Content::Unassigned | Content::Graphic(_))) => {
                    let sid = StoryId(d.alloc());
                    let mut st = Story::new(sid);
                    st.frames.push(*id);
                    d.stories.insert(sid, Arc::new(st));
                    if let Some(it) = d.item_mut(*id) {
                        it.content = Content::Text(TextFrame { story: sid, options: TextFrameOptions::default() });
                    }
                }
                ("graphic" | "unassigned", Some(Content::Text(tf))) => {
                    let empty = d.story(tf.story).map(|s| s.frames.len() == 1).unwrap_or(true);
                    if let Some(st) = d.story_mut(tf.story) {
                        st.frames.retain(|f| f != id);
                    }
                    if empty {
                        d.stories.remove(&tf.story);
                    }
                    if let Some(it) = d.item_mut(*id) {
                        it.content = Content::Unassigned;
                    }
                }
                ("graphic" | "unassigned", Some(Content::Graphic(_))) if t == "unassigned" => {
                    if let Some(it) = d.item_mut(*id) {
                        it.content = Content::Unassigned;
                    }
                }
                _ => {}
            }
        }
        ok()
    })
}

pub(super) fn text_frame_options(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let mut ids = targets(s, p)?;
    if ids.is_empty()
        && let Some(t) = st.selection.text
    {
        ids = st.doc.story(t.story).map(|s| s.frames.clone()).unwrap_or_default();
    }
    let p = p.clone();
    s.edit(|d, _| {
        for id in &ids {
            let Some(tf) = d.item_mut(*id).and_then(Item::text_frame_mut) else { continue };
            let o = &mut tf.options;
            if let Some(v) = p.get("columns").and_then(Value::as_u64) {
                o.columns = (v as u32).clamp(1, 40);
            }
            if let Some(v) = p.get("gutter").and_then(Value::as_f64) {
                o.gutter = v.max(0.0);
            }
            match p.get("inset") {
                Some(Value::Number(n)) => o.inset = [n.as_f64().unwrap_or(0.0); 4],
                Some(v @ Value::Array(_)) => {
                    if let Ok(a) = serde_json::from_value(v.clone()) {
                        o.inset = a;
                    }
                }
                _ => {}
            }
            for (k, which) in [("verticalJustification", 0), ("firstBaseline", 1), ("autoSize", 2), ("columnsKind", 3)] {
                if let Some(v) = p.get(k).cloned() {
                    let _ = match which {
                        0 => serde_json::from_value(v).map(|x| o.vertical_justification = x),
                        1 => serde_json::from_value(v).map(|x| o.first_baseline = x),
                        2 => serde_json::from_value(v).map(|x| o.auto_size = x),
                        _ => serde_json::from_value(v).map(|x| o.columns_kind = x),
                    };
                }
            }
            if let Some(v) = p.get("ignoreWrap").and_then(Value::as_bool) {
                o.ignore_wrap = v;
            }
            if let Some(v) = p.get("balanceColumns").and_then(Value::as_bool) {
                o.balance_columns = v;
            }
            if let Some(v) = p.get("columnRule").and_then(Value::as_bool) {
                o.column_rule = v;
            }
        }
        // Story direction belongs to the story: every frame of the thread turns.
        if let Some(v) = p.get("vertical").and_then(Value::as_bool) {
            let stories: Vec<StoryId> = ids.iter().filter_map(|id| d.item(*id).and_then(Item::text_frame).map(|t| t.story)).collect();
            super::super::text::set_story_direction(d, &stories, v);
        }
        Ok(json!({"changed": ids.len()}))
    })
}

pub(super) fn frame_grid(s: &mut Session, p: &Value) -> Result<Value> {
    let rect = rect_param(p, "rect").ok_or_else(|| bad("frame.grid", "missing rect"))?;
    let cols = p.get("cols").and_then(Value::as_u64).unwrap_or(1).clamp(1, 100) as usize;
    let rows = p.get("rows").and_then(Value::as_u64).unwrap_or(1).clamp(1, 100) as usize;
    let gutter = p.get("gutter").and_then(Value::as_f64).unwrap_or(12.0).max(0.0);
    let cw = ((rect.width() - gutter * (cols - 1) as f64) / cols as f64).max(0.5);
    let ch = ((rect.height() - gutter * (rows - 1) as f64) / rows as f64).max(0.5);
    let mut ids = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let x0 = rect.x0 + c as f64 * (cw + gutter);
            let y0 = rect.y0 + r as f64 * (ch + gutter);
            let q = super::super::with_param(&super::super::with_param(p, "rect", json!([x0, y0, x0 + cw, y0 + ch])), "caret", json!(false));
            ids.push(frame_create(s, &q)?["id"].clone());
        }
    }
    let sel: Vec<ItemId> = ids.iter().filter_map(Value::as_u64).map(ItemId).collect();
    s.doc_mut()?.selection = designcraft_doc::Selection::items(sel);
    Ok(json!({"ids": ids}))
}

pub(super) fn object_primary_text_frame(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let sid = ids
        .iter()
        .find_map(|id| s.doc().ok()?.doc.item(*id)?.text_frame().map(|t| t.story))
        .ok_or_else(|| bad("object.primaryTextFrame", "select a text frame"))?;
    let cur = s.doc()?.doc.settings.primary_story;
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(cur != Some(sid));
    s.edit(|d, _| {
        d.settings.primary_story = if on {
            Some(sid)
        } else if cur == Some(sid) {
            None
        } else {
            cur
        };
        Ok(json!({"primary": on, "story": sid.0}))
    })
}
