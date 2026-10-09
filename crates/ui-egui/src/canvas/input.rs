//! Pointer, keyboard, clipboard, and tool input.

use super::{DVec2, DesignApp, Mods, Point, PointerEvent, PointerKind, Pos2, Rect, ToolKey, Xf, guide_drag, json, mods, zoom_at};

pub(super) fn handle_input(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect) {
    let Some(v) = app.view().copied() else { return };
    let xf = Xf::new(rect, &v);
    let wants_text = app.session.wants_text();
    let space = ui.input(|i| i.key_down(egui::Key::Space)) && !wants_text && !ui.ctx().text_edit_focused();
    // Scroll / zoom.
    if resp.hovered() {
        let (scroll, zoom_delta, m, hover) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.hover_pos()));
        if let Some(hp) = hover {
            if zoom_delta != 1.0 {
                zoom_at(app, hp, zoom_delta as f64);
            } else if m.command && scroll.y != 0.0 {
                zoom_at(app, hp, (1.0 + scroll.y as f64 * 0.003).clamp(0.5, 2.0));
            } else if scroll != egui::Vec2::ZERO
                && let Some(v) = app.view_mut()
            {
                let scroll = xf.unrotate_delta(scroll);
                v.origin = Point::new(v.origin.x - scroll.x as f64 / v.zoom, v.origin.y - scroll.y as f64 / v.zoom);
            }
        }
    }
    // Space-drag = hand (unless a tool drag is under way: then Space is a modifier, e.g. Live
    // Distribute while resizing). Middle-drag pans too, with any tool; tools only
    // see the primary button.
    let tool_drag: bool = ui.data(|d| d.get_temp(egui::Id::new(("canvas_pointer_down", app.pane)))).unwrap_or(false);
    if !tool_drag && ((space && resp.dragged()) || resp.dragged_by(egui::PointerButton::Middle)) {
        let d = xf.unrotate_delta(resp.drag_delta());
        if let Some(v) = app.view_mut() {
            v.origin = Point::new(v.origin.x - d.x as f64 / v.zoom, v.origin.y - d.y as f64 / v.zoom);
        }
        return;
    }
    // Ruler guides need the rulers (hidden while the view is rotated).
    if !space && xf.rot == 0 && guide_drag(app, ui, resp, rect, &xf) {
        return;
    }
    let m = ui.input(|i| mods(i, space));
    let vi = app.view_info();
    let pos = |p: Pos2| xf.to_canvas(p);
    let mut events: Vec<PointerEvent> = Vec::new();
    let down_id = egui::Id::new(("canvas_pointer_down", app.pane));
    let mut down: bool = ui.data(|d| d.get_temp(down_id)).unwrap_or(false);
    let (pressed, released, origin, latest, dbl) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
        )
    });
    if pressed
        && !space
        && let Some(o) = origin.filter(|o| (if xf.rot == 0 { rect.contains(*o) } else { resp.rect.contains(*o) }) && resp.hovered())
    {
        events.push(PointerEvent { kind: if dbl { PointerKind::DoubleClick } else { PointerKind::Down }, pos: pos(o), mods: m });
        down = true;
    }
    if down
        && resp.dragged()
        && resp.drag_delta() != egui::Vec2::ZERO
        && let Some(p) = latest
    {
        events.push(PointerEvent { kind: PointerKind::Drag, pos: pos(p), mods: m });
    }
    // Power Zoom: holding the Hand tool still for half a second turns the press into one.
    let hold_id = egui::Id::new(("canvas_hold", app.pane));
    if down && !released && app.session.tool_id() == "hand" && !m.alt {
        let (now, origin_pos) = ui.input(|i| (i.time, i.pointer.press_origin()));
        let (start, fired): (f64, bool) = ui.data(|d| d.get_temp(hold_id)).unwrap_or((now, false));
        let still = origin_pos.zip(latest).is_some_and(|(o, l)| (o - l).length() < 3.0);
        if !fired && still && now - start >= 0.5 {
            let p = pos(latest.unwrap_or(rect.center()));
            events.push(PointerEvent { kind: PointerKind::Up, pos: p, mods: m });
            events.push(PointerEvent { kind: PointerKind::Down, pos: p, mods: Mods { alt: true, ..m } });
            ui.data_mut(|d| d.insert_temp(hold_id, (start, true)));
        } else {
            ui.data_mut(|d| d.insert_temp(hold_id, (start, fired)));
            if !fired {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    } else {
        ui.data_mut(|d| d.remove::<(f64, bool)>(hold_id));
    }
    if down && released {
        let p = latest.unwrap_or(rect.center());
        events.push(PointerEvent { kind: PointerKind::Up, pos: pos(p), mods: m });
        down = false;
    } else if !down && let Some(p) = resp.hover_pos() {
        events.push(PointerEvent { kind: PointerKind::Move, pos: pos(p), mods: m });
    }
    ui.data_mut(|d| d.insert_temp(down_id, down));
    for e in events {
        if let Err(err) = app.session.pointer(&e, vi) {
            app.status(err.to_string());
        }
    }
    app.after_engine();
    if resp.clicked() || resp.drag_started() {
        resp.request_focus();
    }
    // Keyboard for the active tool.
    if ui.ctx().text_edit_focused() && !resp.has_focus() {
        return;
    }
    let evs = ui.input(|i| i.events.clone());
    for e in evs {
        match e {
            egui::Event::Text(t) if wants_text => {
                let _ = app.run("text.insert", json!({"text": t}));
            }
            egui::Event::Paste(t) if wants_text => {
                // Formatted when the system clipboard still holds what was copied here;
                // Shift (⇧⌘V) pastes without formatting.
                let plain = ui.input(|i| i.modifiers.shift);
                let id = if plain { "edit.pasteWithoutFormatting" } else { "edit.paste" };
                let _ = app.run(id, json!({"text": t}));
            }
            egui::Event::Copy | egui::Event::Cut if wants_text => {
                let id = if matches!(e, egui::Event::Cut) { "edit.cut" } else { "edit.copy" };
                if let Ok(r) = app.run(id, json!({}))
                    && let Some(t) = r.get("text").and_then(serde_json::Value::as_str)
                {
                    ui.ctx().copy_text(t.to_string());
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let k = match key {
                    egui::Key::ArrowLeft => Some(ToolKey::Left),
                    egui::Key::ArrowRight => Some(ToolKey::Right),
                    egui::Key::ArrowUp => Some(ToolKey::Up),
                    egui::Key::ArrowDown => Some(ToolKey::Down),
                    egui::Key::Enter => Some(ToolKey::Enter),
                    egui::Key::Escape => Some(ToolKey::Escape),
                    egui::Key::Backspace => Some(ToolKey::Backspace),
                    egui::Key::Delete => Some(ToolKey::Delete),
                    egui::Key::Tab => Some(ToolKey::Tab),
                    egui::Key::Home => Some(ToolKey::Home),
                    egui::Key::End => Some(ToolKey::End),
                    _ => None,
                };
                if let Some(k) = k {
                    let m = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command, ctrl: modifiers.ctrl, space: false };
                    let handled = app.session.tool_key(k, m, vi).unwrap_or(false);
                    if !handled && k == ToolKey::Escape {
                        let _ = app.run("edit.deselectAll", json!({}));
                    }
                    app.after_engine();
                }
            }
            _ => {}
        }
    }
    let _ = DVec2::ZERO;
}

/// Plain text of the current text selection (story markers resolved to readable characters).
pub fn selected_text(app: &DesignApp) -> Option<String> {
    let st = app.session.active()?;
    let t = st.selection.text?;
    let story = st.doc.text_story(t.story, t.cell)?;
    let s = story.slice(t.range());
    if s.is_empty() {
        return None;
    }
    Some(s.replace(designcraft_doc::story::FORCED_LINE_BREAK, "\n"))
}
