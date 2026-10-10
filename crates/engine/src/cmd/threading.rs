//! An atomic command for connecting existing frames or drawing a new threaded frame.
use super::{CommandSpec, bad, cmd, has_doc, id_param, rect_param};
use crate::{Result, Session};
use designcraft_doc::{ItemId, ParaFormat, Selection};
use serde_json::{Value, json};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "frame.thread",
        "Thread Text Frames",
        [],
        None,
        "{from: id, to?: id, spread?, rect?: [x0,y0,x1,y1]} — connect story endpoints; specify either to, or spread and rect to create a destination; returns {story, frame}",
        has_doc,
        thread
    )]
}

fn thread(s: &mut Session, p: &Value) -> Result<Value> {
    let from = id_param(p, "from").ok_or_else(|| bad("frame.thread", "missing source frame"))?;
    let to = id_param(p, "to");
    let rect = rect_param(p, "rect");
    if p.get("to").is_some() != to.is_some()
        || p.get("rect").is_some() != rect.is_some()
        || to.is_some() == rect.is_some()
        || (to.is_some() && p.get("spread").is_some())
        || (rect.is_some() && p.get("spread").is_none())
    {
        return Err(bad("frame.thread", "specify either to, or spread and rect"));
    }
    if let Some(r) = rect
        && (![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) || r.width() < 0.5 || r.height() < 0.5)
    {
        return Err(bad("frame.thread", "new frame must have finite coordinates and dimensions of at least 0.5 pt"));
    }
    let spread = match p.get("spread") {
        Some(Value::Number(n)) => designcraft_doc::SpreadRef::Doc(
            n.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("frame.thread", "invalid spread index"))?,
        ),
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad("frame.thread", "invalid spread"))?,
        None => designcraft_doc::SpreadRef::Doc(0),
    };
    let layer = s.doc()?.active_layer;
    s.edit(|d, selection| {
        d.validate_thread(from, to)?;
        let source = d.item(from).and_then(|i| i.text_frame()).ok_or(designcraft_doc::DocError::NoItem(from))?.story;
        let to: ItemId = if let Some(to) = to {
            to
        } else {
            let r = rect.ok_or_else(|| bad("frame.thread", "missing new frame rectangle"))?;
            if !matches!(spread, designcraft_doc::SpreadRef::Doc(_)) || d.spread(spread).is_none() {
                return Err(bad("frame.thread", "new frame requires a document spread"));
            }
            if !d.layer(layer).is_some_and(|l| l.visible && !l.locked) {
                return Err(bad("frame.thread", "active layer is locked or hidden"));
            }
            let (id, sid) = d.add_text_frame(spread, r, layer, "", ParaFormat::default())?;
            let (vertical, direction) = d.story(source).map(|st| (st.vertical, st.direction)).ok_or(designcraft_doc::DocError::NoStory(source))?;
            let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
            st.vertical = vertical;
            st.direction = direction;
            id
        };
        d.thread(from, to)?;
        *selection = Selection::items(vec![to]);
        Ok(json!({"story": source.0, "frame": to.0}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::{SpreadRef, StoryId};
    use designcraft_geom::Point;
    use designcraft_tools::{Cursor, PointerEvent, PointerKind};

    fn setup() -> (Session, ItemId, ItemId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let a = s
            .execute(
                "frame.create",
                &json!({"rect": [36,36,136,76], "content":"text", "text": "Alpha beta gamma delta. ".repeat(20), "caret": false}),
            )
            .unwrap();
        let b = s.execute("frame.create", &json!({"rect": [200,36,500,600], "content":"text", "text":"END", "caret":false})).unwrap();
        (s, ItemId(a["id"].as_u64().unwrap()), ItemId(b["id"].as_u64().unwrap()))
    }
    fn sid(s: &Session, id: ItemId) -> StoryId {
        s.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().story
    }
    fn point(s: &Session, id: ItemId) -> Point {
        let d = &s.doc().unwrap().doc;
        let loc = d.find(id).unwrap();
        s.layout().xf(loc.spread) * d.parent_xf(&loc) * d.item(id).unwrap().xf * d.item(id).unwrap().inner_bounds().center()
    }
    fn pointer(s: &mut Session, kind: PointerKind, p: Point) {
        s.pointer(&PointerEvent::new(kind, p.x, p.y), crate::ViewInfo::default()).unwrap();
    }
    fn arm(s: &mut Session, id: ItemId) {
        s.set_tool("selection");
        s.execute("selection.set", &json!({"ids":[id.0]})).unwrap();
        let d = &s.doc().unwrap().doc;
        let loc = d.find(id).unwrap();
        let (_, p) = designcraft_tools::threading::port_positions(d.item(id).unwrap(), s.layout().xf(loc.spread) * d.parent_xf(&loc), 1.0);
        pointer(s, PointerKind::Down, p);
        pointer(s, PointerKind::Up, p);
        assert!(s.tool_busy());
    }
    #[test]
    fn thread_command_flows_text_and_roundtrips_with_one_undo_step() {
        let (mut s, a, b) = setup();
        let source = sid(&s, a);
        let expected = format!("{}\nEND", s.doc().unwrap().doc.story(source).unwrap().text);
        let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
        let history = s.doc().unwrap().history.undo.len();
        let r = s.execute("frame.thread", &json!({"from":a.0,"to":b.0})).unwrap();
        assert_eq!(r, json!({"story":source.0,"frame":b.0}));
        let d = &s.doc().unwrap().doc;
        d.check().unwrap();
        assert_eq!(d.story(source).unwrap().text, expected);
        let cs = s.cache.get(d, source, None);
        assert!(!cs.is_overset());
        assert_eq!(cs.frames.len(), 2);
        assert_eq!(cs.frames[0].range.end, cs.frames[1].range.start);
        assert!(!cs.frames[1].lines.is_empty());
        let native = crate::cmd::file_from(&crate::cmd::file_bytes(d)).unwrap();
        assert_eq!(native.story(source).unwrap().text, expected);
        let idml = designcraft_idml::import_idml(&designcraft_idml::export_idml(d)).unwrap();
        idml.check().unwrap();
        assert!(idml.stories.values().any(|st| st.frames.len() == 2 && st.text == expected));
        assert_eq!(s.doc().unwrap().history.undo.len(), history + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(source).unwrap().text, expected);
    }
    #[test]
    fn thread_empty_destinations_and_journal_replay() {
        for unassigned in [false, true] {
            let (mut s, a, b) = setup();
            let source = sid(&s, a);
            let target = sid(&s, b);
            s.edit(|d, _| {
                if unassigned {
                    d.item_mut(b).unwrap().content = designcraft_doc::Content::Unassigned;
                    d.stories.remove(&target);
                } else {
                    d.set_story_text(target, "")?;
                }
                Ok(())
            })
            .unwrap();
            let expected = s.doc().unwrap().doc.story(source).unwrap().text.clone();
            s.execute("frame.thread", &json!({"from":a.0,"to":b.0})).unwrap();
            assert_eq!(s.doc().unwrap().doc.story(source).unwrap().text, expected);
            s.doc().unwrap().doc.check().unwrap();
        }
        let (mut original, a, _) = setup();
        original.execute("frame.thread", &json!({"from":a.0,"spread":0,"rect":[36,300,300,500]})).unwrap();
        let mut replay = Session::new();
        for (command, params) in &original.journal {
            replay.execute(command, params).unwrap();
        }
        assert_eq!(serde_json::to_value(&*original.doc().unwrap().doc).unwrap(), serde_json::to_value(&*replay.doc().unwrap().doc).unwrap());
    }
    #[test]
    fn thread_rich_payloads_survive_native_and_idml_roundtrip() {
        use designcraft_doc::*;
        let (mut s, a, b) = setup();
        let source = sid(&s, a);
        let target = sid(&s, b);
        s.edit(|d, _| {
            for (sid, label) in [(source, "source"), (target, "target")] {
                let obj = Item::new(
                    ItemId(d.alloc()),
                    d.default_layer(),
                    Shape::Rectangle,
                    designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, 10.0, 10.0)),
                );
                let st = d.story_mut(sid).unwrap();
                st.insert_note(st.len(), label, Default::default());
                st.insert_endnote(st.len(), label, Default::default());
                st.insert(st.len(), &designcraft_doc::endnotes::NOTE_MARK.to_string());
                let anchor = st.insert_anchor(st.len(), label);
                st.insert_xref(st.len(), CrossRef { target: anchor, format: "Paragraph Text".into(), field: None });
                st.insert_index_ref(st.len(), IndexRef { topics: vec![label.into()], ..Default::default() });
                st.insert_object(st.len(), AnchoredObject::new(obj, Default::default()));
                let mut table = Table::new(1, 1, 1, 0, 0, 80.0);
                table.cells[0].text.insert(0, label);
                st.insert_table(st.len(), table);
            }
            Ok(())
        })
        .unwrap();
        s.execute("frame.thread", &json!({"from":a.0,"to":b.0})).unwrap();
        let d = &s.doc().unwrap().doc;
        let native = crate::cmd::file_from(&crate::cmd::file_bytes(d)).unwrap();
        assert_eq!(serde_json::to_value(native.story(source).unwrap()).unwrap(), serde_json::to_value(d.story(source).unwrap()).unwrap());
        let reopened = designcraft_idml::import_idml(&designcraft_idml::export_idml(d)).unwrap();
        reopened.check().unwrap();
        let st = reopened.stories.values().find(|st| st.frames.len() == 2).unwrap();
        // IDML currently omits endnotes and editorial notes even without threading.
        // Native serialization above must preserve everything; test the IDML-supported payloads here.
        let expected: String = d.story(source).unwrap().text.chars().filter(|ch| !matches!(*ch, ENDNOTE_REF | NOTE_MARK)).collect();
        assert_eq!(st.text, expected);
        assert_eq!(st.tables.len(), 2);
        assert_eq!(st.notes.len(), 2);
        assert!(st.endnotes.is_empty());
        assert!(st.editorial.is_empty());
        assert_eq!(st.anchors.len(), 2);
        assert_eq!(st.xrefs.len(), 2);
        assert_eq!(st.index_refs.len(), 2);
        assert_eq!(st.objects.len(), 2);
    }
    #[test]
    fn thread_gesture_connects_existing_and_new_frames_atomically() {
        let (mut s, a, b) = setup();
        arm(&mut s, a);
        let p = point(&s, b);
        pointer(&mut s, PointerKind::Down, p);
        pointer(&mut s, PointerKind::Up, p);
        assert_eq!(sid(&s, a), sid(&s, b));
        assert!(!s.tool_busy());
        arm(&mut s, b);
        let blank = Point::new(36.0, 300.0);
        pointer(&mut s, PointerKind::Down, blank);
        pointer(&mut s, PointerKind::Up, blank);
        assert!(s.tool_busy(), "blank clicks keep text loaded");
        let history = s.doc().unwrap().history.undo.len();
        pointer(&mut s, PointerKind::Down, blank);
        pointer(&mut s, PointerKind::Drag, Point::new(150.0, 500.0));
        assert!(!s.overlays(Default::default()).is_empty());
        pointer(&mut s, PointerKind::Up, Point::new(150.0, 500.0));
        let selected = s.doc().unwrap().selection.items[0];
        assert_ne!(selected, b);
        assert_eq!(sid(&s, selected), sid(&s, a));
        assert_eq!(s.doc().unwrap().history.undo.len(), history + 1);
        assert_eq!(s.journal.last().unwrap().0, "frame.thread");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.item(selected).is_none());
    }
    #[test]
    fn thread_command_can_create_on_another_page_and_inherit_direction() {
        let (mut s, a, _) = setup();
        let source = sid(&s, a);
        s.edit(|d, _| {
            d.story_mut(source).unwrap().vertical = true;
            Ok(())
        })
        .unwrap();
        let r = s.execute("frame.thread", &json!({"from":a.0,"spread":1,"rect":[36,36,300,500]})).unwrap();
        let id = ItemId(r["frame"].as_u64().unwrap());
        assert_eq!(s.doc().unwrap().doc.find(id).unwrap().spread, SpreadRef::Doc(1));
        assert!(s.doc().unwrap().doc.frame_vertical(s.doc().unwrap().doc.item(id).unwrap()));
    }
    #[test]
    fn thread_invalid_targets_and_parameters_are_atomic() {
        let (mut s, a, b) = setup();
        for p in [
            json!({"from":a.0,"to":a.0}),
            json!({"from":a.0,"to":999999}),
            json!({"from":a.0}),
            json!({"from":a.0,"to":b.0,"rect":[0,0,10,10]}),
            json!({"from":a.0,"spread":"bad","rect":[0,0,10,10]}),
            json!({"from":a.0,"spread":0,"rect":[0,0,-10,10]}),
            json!({"from":a.0,"spread":999,"rect":[0,0,10,10]}),
        ] {
            let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
            assert!(s.execute("frame.thread", &p).is_err(), "{p}");
            assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before);
        }
        for mode in 0..5 {
            let (mut s, a, b) = setup();
            let sb = sid(&s, b);
            s.edit(|d, _| {
                match mode {
                    0 => d.item_mut(b).unwrap().locked = true,
                    1 => d.story_mut(sb).unwrap().vertical = true,
                    2 => d.endnote_story = Some(sb),
                    3 => d.story_mut(sb).unwrap().link = Some((sid_from(d, a), 0)),
                    _ => d.item_mut(b).unwrap().content = designcraft_doc::Content::Group { items: vec![] },
                }
                Ok(())
            })
            .unwrap();
            let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
            assert!(s.execute("frame.thread", &json!({"from":a.0,"to":b.0})).is_err());
            assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before);
        }
        fn sid_from(d: &designcraft_doc::Document, a: ItemId) -> StoryId {
            d.item(a).unwrap().text_frame().unwrap().story
        }
    }
    #[test]
    fn thread_rejects_cycles_interior_endpoints_and_locked_layers() {
        let (mut s, a, b) = setup();
        let c = s.execute("frame.create", &json!({"rect":[36,200,136,300],"content":"text","caret":false})).unwrap();
        let c = ItemId(c["id"].as_u64().unwrap());
        s.execute("frame.thread", &json!({"from":a.0,"to":b.0})).unwrap();
        for (from, to) in [(b, a), (a, c), (c, b)] {
            let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
            assert!(s.execute("frame.thread", &json!({"from":from.0,"to":to.0})).is_err());
            assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before);
        }
        s.edit(|d, _| {
            let layer = d.item(c).unwrap().layer;
            d.layer_mut(layer).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
        assert!(s.execute("frame.thread", &json!({"from":b.0,"to":c.0})).is_err());
        assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before);
    }
    #[test]
    fn thread_locked_layer_is_an_invalid_gesture_target_not_blank_canvas() {
        let (mut s, a, b) = setup();
        arm(&mut s, a);
        let p = point(&s, b);
        s.edit(|d, _| {
            let layer = d.item(b).unwrap().layer;
            d.layer_mut(layer).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
        assert_eq!(s.cursor(p, Default::default(), Default::default()), Cursor::NotAllowed);
        assert!(s.pointer(&PointerEvent::new(PointerKind::Down, p.x, p.y), Default::default()).is_err());
        assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before);
        assert!(s.tool_busy());
    }
    #[test]
    fn thread_switch_and_open_cancel_loaded_text() {
        let (mut s, a, _) = setup();
        let bytes = crate::cmd::file_bytes(&s.doc().unwrap().doc);
        s.execute("file.new", &json!({})).unwrap();
        s.set_active(0);
        arm(&mut s, a);
        s.set_active(1);
        assert!(!s.tool_busy());
        s.set_active(0);
        arm(&mut s, a);
        s.execute("file.openBytes", &json!({"name":"thread.designcraft","base64":crate::cmd::file::base64_encode(&bytes)})).unwrap();
        assert!(!s.tool_busy());
    }
    #[test]
    fn thread_pending_gestures_cancel_on_lifecycle_changes() {
        for mode in 0..6 {
            let (mut s, a, _) = setup();
            if mode == 5 {
                s.execute("edit.undo", &json!({})).unwrap();
            }
            arm(&mut s, a);
            match mode {
                0 => {
                    s.tool_key(designcraft_tools::ToolKey::Escape, Default::default(), Default::default()).unwrap();
                }
                1 => s.set_tool("type"),
                2 => {
                    s.execute("file.new", &json!({})).unwrap();
                }
                3 => {
                    s.execute("file.close", &json!({"discard":true})).unwrap();
                }
                4 => {
                    s.execute("edit.undo", &json!({})).unwrap();
                }
                _ => {
                    s.execute("edit.redo", &json!({})).unwrap();
                }
            }
            assert!(!s.tool_busy(), "mode {mode}");
        }
    }
    #[test]
    fn thread_port_hit_test_uses_transformed_frame_geometry() {
        let (mut s, a, _) = setup();
        s.edit(|d, _| {
            d.item_mut(a).unwrap().xf = designcraft_geom::Affine::translate((100.0, 80.0)) * designcraft_geom::Affine::rotate(0.3);
            Ok(())
        })
        .unwrap();
        arm(&mut s, a);
        assert_eq!(s.cursor(Point::new(500.0, 500.0), Default::default(), Default::default()), Cursor::LoadedText);
    }
}
