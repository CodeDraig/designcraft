//! Canvas context actions use the same commands as the menu bar.
use designcraft_geom::Point;
use designcraft_tools::ToolContext;
use serde_json::{Value, json};

use crate::{DesignApp, canvas::Xf, menus};

pub fn sync_document(app: &mut DesignApp, ctx: &egui::Context) {
    let key = egui::Id::new("canvas_context_document");
    let current = app.session.active().map(|st| st.uid);
    let previous: Option<Option<u64>> = ctx.data(|d| d.get_temp(key));
    if previous.is_some_and(|old| old != current) {
        egui::Popup::close_all(ctx);
        app.session.cancel_tool_gesture();
    }
    ctx.data_mut(|d| d.insert_temp(key, current));
}

/// Select the context target without starting a primary-button gesture.
fn prepare(app: &mut DesignApp, point: Point) {
    app.session.cancel_tool_gesture();
    let target = (|| {
        let st = app.session.active()?;
        let layout = app.session.layout();
        let view = app.view_info();
        let cx = ToolContext {
            doc: &st.doc,
            selection: &st.selection,
            cache: &app.session.cache,
            layout: &layout,
            zoom: view.zoom,
            layer: st.active_layer,
            snap: view.snap,
            unit: view.unit,
        };
        let (spread, hit) = cx.hit(point)?;
        let text = st.selection.text.is_some() && st.doc.item(hit).is_some_and(|it| it.is_text_frame());
        let id = if st.selection.contains(hit) || app.session.tool_id() == "directSelection" { hit } else { st.doc.top_level_of(hit).unwrap_or(hit) };
        Some((id, text, layout.to_spread(spread, point), st.selection.contains(id)))
    })();
    match target {
        Some((id, true, p, _)) => {
            let _ = app.run("text.placeCaret", json!({"frame": id.0, "point": [p.x, p.y]}));
            // placeCaret preserves selected text, but normally also arms drag-and-drop.
            app.session.cancel_tool_gesture();
        }
        Some((_, false, _, true)) => {}
        Some((id, false, _, false)) => {
            let _ = app.run("selection.set", json!({"ids": [id.0]}));
        }
        None => {
            let _ = app.run("selection.set", json!({"ids": []}));
        }
    }
}

fn entry(app: &mut DesignApp, ui: &mut egui::Ui, id: &str) {
    let Some(spec) = designcraft_engine::find_command(id) else { return };
    let label = crate::i18n::tr(&app.ui.language, spec.label);
    let mut button = egui::Button::new(crate::rtl::widget(ui, label));
    if let Some(shortcut) = menus::shortcut_of(app, id) {
        button = button.shortcut_text(menus::shortcut_text(&shortcut));
    }
    if ui.add_enabled(menus::menu_enabled(app, id), button).clicked() {
        menus::activate(app, id, &Value::Null);
        ui.close();
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui, response: &egui::Response, xf: &Xf, screen: egui::Rect) {
    sync_document(app, ui.ctx());
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos().filter(|p| screen.contains(*p))
    {
        prepare(app, xf.to_canvas(p));
    }
    response.context_menu(|ui| {
        for id in ["edit.cut", "edit.copy", "edit.paste", "edit.pasteInPlace", "edit.clear", "edit.duplicate", "edit.selectAll"] {
            entry(app, ui, id);
        }
        let objects = app.session.active().is_some_and(|st| st.selection.text.is_none() && !st.selection.items.is_empty());
        if objects {
            ui.separator();
            let label = crate::i18n::tr(&app.ui.language, "Arrange").to_string();
            ui.menu_button(crate::rtl::widget(ui, label), |ui| {
                for id in ["object.bringToFront", "object.bringForward", "object.sendBackward", "object.sendToBack"] {
                    entry(app, ui, id);
                }
            });
            for id in ["object.group", "object.ungroup", "object.lock"] {
                entry(app, ui, id);
            }
        }
        if app.session.active().is_some_and(|st| st.selection.items.iter().any(|id| st.doc.item(*id).is_some_and(|it| it.is_text_frame()))) {
            ui.separator();
            entry(app, ui, "object.textFrameOptions");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::{ItemId, TextSel};
    use designcraft_tools::CanvasLayout;

    fn setup() -> (DesignApp, ItemId, ItemId) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 236, 136], "content": "text", "text": "one two three four"})).unwrap();
        let b = app.run("frame.create", json!({"rect": [280, 36, 480, 136], "content": "text", "text": "second"})).unwrap();
        (app, ItemId(a["id"].as_u64().unwrap()), ItemId(b["id"].as_u64().unwrap()))
    }
    fn at(app: &DesignApp, id: ItemId) -> Point {
        let st = app.session.doc().unwrap();
        let loc = st.doc.find(id).unwrap();
        let layout = CanvasLayout::new(&st.doc, false);
        layout.xf(loc.spread) * st.doc.item(id).unwrap().bounds().center()
    }
    #[test]
    fn context_target_preserves_multiselection_and_selects_other_or_blank() {
        let (mut app, a, b) = setup();
        app.run("selection.set", json!({"ids": [a.0, b.0]})).unwrap();
        let p = at(&app, a);
        prepare(&mut app, p);
        assert_eq!(app.session.doc().unwrap().selection.items, vec![a, b]);
        app.run("selection.set", json!({"ids": [a.0]})).unwrap();
        let p = at(&app, b);
        prepare(&mut app, p);
        assert_eq!(app.session.doc().unwrap().selection.items, vec![b]);
        prepare(&mut app, Point::new(-10000.0, -10000.0));
        assert!(app.session.doc().unwrap().selection.items.is_empty());
    }
    #[test]
    fn context_inside_text_keeps_selection_without_arming_drag() {
        let (mut app, a, _) = setup();
        let st = app.session.doc().unwrap();
        let sid = st.doc.item(a).unwrap().text_frame().unwrap().story;
        app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 18, "frame": a.0})).unwrap();
        let st = app.session.doc().unwrap();
        let cs = app.session.cache.get(&st.doc, sid, None);
        let line = &cs.frames[0].lines[0];
        let it = st.doc.item(a).unwrap();
        let loc = st.doc.find(a).unwrap();
        let p = app.session.layout().xf(loc.spread) * st.doc.text_xf(it) * Point::new(line.x0 + 2.0, line.baseline - 2.0);
        let before: TextSel = st.selection.text.unwrap();
        prepare(&mut app, p);
        assert_eq!(app.session.doc().unwrap().selection.text, Some(before));
        assert!(!app.session.tool_busy());
        app.run("text.release", json!({"frame": a.0, "point": [100, 100], "moved": true})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "one two three four");
    }
    #[test]
    fn document_switch_closes_existing_popups() {
        let (mut app, _, _) = setup();
        let ctx = egui::Context::default();
        sync_document(&mut app, &ctx);
        egui::Popup::open_id(&ctx, egui::Id::new("stale_context"));
        app.run("file.new", json!({})).unwrap();
        sync_document(&mut app, &ctx);
        assert!(!egui::Popup::is_any_open(&ctx));
    }
}
