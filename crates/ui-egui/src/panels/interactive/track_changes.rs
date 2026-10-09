//! Track changes panel controls.

use super::{DesignApp, Tokens, json};

/// Track Changes panel: tracking on/off and each change with Accept / Reject.
pub fn track_changes(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(on) = app.session.active().map(|d| d.doc.settings.track_changes) else { return };
    let mut track = on;
    if ui.checkbox(&mut track, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Track Changes in All Stories"))).changed() {
        let _ = app.run("changes.track", json!({"on": track}));
    }
    let list = app.session.execute("changes.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    ui.separator();
    if list.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No changes.")).size(11.0).color(t.text_dim));
        return;
    }
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for c in &list {
            let (story, start) = (c["story"].clone(), c["start"].clone());
            let kind = c["kind"].as_str().unwrap_or("");
            let text: String = c["text"].as_str().unwrap_or("").chars().take(40).collect();
            ui.horizontal(|ui| {
                let label = egui::RichText::new(format!(
                    "{} “{}”",
                    crate::i18n::tr(&app.ui.language, if kind == "inserted" { "Added" } else { "Deleted" }),
                    text.replace('\n', "¶")
                ))
                .size(11.0);
                let label = if kind == "deleted" { label.strikethrough() } else { label };
                if ui
                    .selectable_label(false, label)
                    .on_hover_ui(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Show in the text"));
                    })
                    .clicked()
                {
                    let end = c["end"].clone();
                    let _ = app.run("text.select", json!({"story": story, "anchor": start, "focus": end}));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reject"))).clicked() {
                        let _ = app.run("changes.reject", json!({"story": story, "start": start}));
                    }
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Accept"))).clicked() {
                        let _ = app.run("changes.accept", json!({"story": story, "start": start}));
                    }
                });
            });
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Accept All"))).clicked() {
            let _ = app.run("changes.acceptAll", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reject All"))).clicked() {
            let _ = app.run("changes.rejectAll", json!({}));
        }
    });
}
