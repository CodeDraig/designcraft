//! colorSettings dialog controls.

use super::{DesignApp, Dialog, Value, check, json};

pub(super) fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let profiles = d.fields.get("profiles").and_then(Value::as_array).cloned().unwrap_or_default();
    egui::Grid::new("color_settings").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (key, label, kind) in [("rgb", "RGB:", "rgb"), ("cmyk", "CMYK:", "cmyk")] {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
            let cur = d.s(key);
            egui::ComboBox::from_id_salt(("ws", key)).selected_text(&cur).width(260.0).show_ui(ui, |ui| {
                for pr in profiles.iter().filter(|p| p["kind"] == kind) {
                    let name = pr["name"].as_str().unwrap_or("");
                    if ui.selectable_label(name == cur, name).clicked() {
                        d.fields.insert(key.into(), json!(name));
                    }
                }
            });
            ui.end_row();
        }
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Intent:"));
        let cur = d.s("intent");
        egui::ComboBox::from_id_salt("ws_intent")
            .selected_text(crate::rtl::widget(
                ui,
                crate::i18n::tr(&app.ui.language, designcraft_color::cms::Intent::parse(&cur).map_or("", |i| i.label())),
            ))
            .show_ui(ui, |ui| {
                for i in designcraft_color::cms::Intent::ALL {
                    if ui.selectable_label(i.id() == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, i.label()))).clicked() {
                        d.fields.insert("intent".into(), json!(i.id()));
                    }
                }
            });
        ui.end_row();
    });
    check(ui, d, "bpc", crate::i18n::tr(&app.ui.language, "Use Black Point Compensation"));
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load Profile…"))).clicked()
        && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("icc"))
    {
        match app.session.execute("color.loadProfile", &json!({"path": path})) {
            Ok(_) => {
                let f = app.session.execute("color.settings", &json!({})).unwrap_or_default();
                d.fields.insert("profiles".into(), f["profiles"].clone());
            }
            Err(e) => app.status(format!("Color Settings: {e}")),
        }
    }
}
