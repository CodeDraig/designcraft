//! States panel controls.

use super::{DesignApp, Tokens, json};

/// Object States panel: the selected multi-state object's states (click to show one).
pub fn states(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let n = app.session.active().map_or(0, |d| d.selection.items.len());
    let info = app.session.execute("states.list", &json!({})).ok();
    let list = info.as_ref().and_then(|i| i["states"].as_array().cloned()).unwrap_or_default();
    if list.is_empty() {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select two or more objects (or a group) to make a multi-state object."))
                    .size(11.0)
                    .color(t.text_dim),
            ),
        );
        if ui
            .add_enabled(
                n > 0,
                egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Convert Selection to Multi-State Object"))),
            )
            .clicked()
            && let Err(e) = app.run("states.create", json!({}))
        {
            app.status(format!("Object States: {e}"));
        }
        return;
    }
    let active = info.as_ref().and_then(|i| i["active"].as_u64()).unwrap_or(0) as usize;
    for (i, st) in list.iter().enumerate() {
        if ui.selectable_label(i == active, st.as_str().unwrap_or("")).clicked() {
            let _ = app.run("states.show", json!({"index": i}));
        }
    }
    ui.separator();
    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Release to Objects"))).clicked() {
        let _ = app.run("states.release", json!({}));
    }
}
