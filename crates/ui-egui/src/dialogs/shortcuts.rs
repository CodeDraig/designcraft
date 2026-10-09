//! Keyboard shortcut editing form.

use super::{DesignApp, Dialog, json};

pub(super) fn keyboard_shortcuts(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let mut q = d.s("query");
    ui.add(
        egui::TextEdit::singleline(&mut q)
            .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search commands")))
            .desired_width(f32::INFINITY),
    );
    d.fields.insert("query".into(), json!(q));
    let recording = d.s("recording");
    // Capture the next key press for the command being recorded.
    if !recording.is_empty() {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, m)) = pressed {
            let sc = match key {
                egui::Key::Escape => None,
                egui::Key::Backspace if !m.any() => Some(String::new()),
                _ => Some(crate::menus::shortcut_string(m, key)),
            };
            if let Some(sc) = sc {
                let msg = match app.run("window.setShortcut", json!({"id": recording, "shortcut": sc})) {
                    Ok(v) => match v["conflicts"].as_array().filter(|c| !c.is_empty()) {
                        Some(c) => format!("Also used by: {}", c.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")),
                        None => String::new(),
                    },
                    Err(e) => e,
                };
                d.fields.insert("message".into(), json!(msg));
            }
            d.fields.insert("recording".into(), json!(""));
        }
    }
    let ql = q.to_lowercase();
    let mut rows: Vec<(String, String, String)> = designcraft_engine::command_specs()
        .iter()
        .filter(|c| !c.menu.is_empty() || c.shortcut.is_some())
        .map(|c| (c.id.to_string(), c.label.to_string(), c.menu.join(" › ")))
        .chain(crate::menus::UI_COMMANDS.iter().map(|c| (c.0.to_string(), c.1.to_string(), String::new())))
        .filter(|(id, l, _)| ql.is_empty() || l.to_lowercase().contains(&ql) || id.to_lowercase().contains(&ql))
        .collect();
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        egui::Grid::new("shortcuts").num_columns(3).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
            for (id, label, menu) in &rows {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label.trim_end_matches('…')))
                    .on_hover_text(format!("{id}{}", if menu.is_empty() { String::new() } else { format!("  ({menu})") }));
                let cur = crate::menus::shortcut_of(app, id).map(|s| crate::menus::shortcut_text(&s)).unwrap_or_else(|| "—".into());
                let text = if recording == *id { "Press keys…".to_string() } else { cur };
                if ui.add(egui::Button::new(text).min_size(egui::vec2(110.0, 0.0))).clicked() {
                    d.fields.insert("recording".into(), json!(id));
                    d.fields.insert("message".into(), json!(""));
                }
                if app.ui.shortcuts.contains_key(id) {
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Default"))).clicked() {
                        let _ = app.run("window.setShortcut", json!({"id": id, "shortcut": null}));
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    });
    let msg = d.s("message");
    if !msg.is_empty() {
        ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(240, 180, 60)));
    }
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reset All to Defaults"))).clicked() {
        let _ = app.run("window.resetShortcuts", json!({}));
    }
}
