//! Structure panel controls.

use super::{DesignApp, Tokens, json};

/// Tags panel and structure: tags (click to tag the selection), the structure in reading order,
/// XML export/import.
pub fn tags(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(info) = app.session.execute("xml.tags", &json!({})) else { return };
    let has_sel = app.session.active().is_some_and(|d| !d.selection.items.is_empty());
    // Selected text is tagged inline instead of its frame.
    let text_sel = app.session.active().and_then(|d| d.selection.text).is_some_and(|t| !t.range().is_empty());
    let id = egui::Id::new("new_tag_name");
    let mut name: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New tag")))
                .desired_width(140.0),
        );
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New"))).clicked() && !name.trim().is_empty() {
            match app.run("xml.newTag", json!({"name": name.trim()})) {
                Ok(_) => name.clear(),
                Err(e) => app.status(format!("Tags: {e}")),
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, name));
    for tag in info["tags"].as_array().cloned().unwrap_or_default() {
        let n = tag["name"].as_str().unwrap_or("").to_string();
        let c = tag["color"].as_array().map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0) as u8).collect::<Vec<_>>()).unwrap_or_default();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            if c.len() == 3 {
                ui.painter().rect_filled(r, 2.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
            }
            if ui
                .add_enabled(has_sel || text_sel, egui::Button::new(&n).frame(false))
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Tag the selection"));
                })
                .clicked()
            {
                let _ = if text_sel { app.run("xml.tagText", json!({"tag": n})) } else { app.run("xml.tag", json!({"tag": n})) };
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))).clicked() {
                    let _ = app.run("xml.deleteTag", json!({"name": n}));
                }
            });
        });
    }
    if (has_sel || text_sel) && ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Untag Selection"))).clicked() {
        let _ = if text_sel { app.run("xml.tagText", json!({"tag": null})) } else { app.run("xml.tag", json!({"tag": null})) };
    }
    let mut markers = app.ui.tag_markers;
    if ui.checkbox(&mut markers, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Show Tag Markers"))).changed() {
        let _ = app.run("view.tagMarkers", json!({"on": markers}));
    }
    ui.separator();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Structure")).strong());
    let st = app.session.execute("xml.structure", &json!({})).unwrap_or_default();
    crate::rtl::label(ui, egui::RichText::new(st["root"].as_str().unwrap_or("Root")).size(11.0).color(t.text_dim));
    for el in st["elements"].as_array().cloned().unwrap_or_default() {
        let text = el["text"].as_str().unwrap_or("").chars().take(40).collect::<String>();
        let label = format!("  <{}> {}", el["tag"].as_str().unwrap_or(""), text);
        if ui.selectable_label(false, egui::RichText::new(label).size(11.0)).clicked() {
            let _ = app.run("selection.set", json!({"ids": [el["id"]]}));
        }
    }
    // DTD: load, validate (problems listed until the next validation), delete.
    let vid = egui::Id::new("dtd_problems");
    let mut problems: Option<Vec<String>> = ui.data(|d| d.get_temp(vid));
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load DTD…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("dtd"))
        {
            match app.run("xml.loadDtd", json!({"path": path})) {
                Ok(_) => problems = None,
                Err(e) => app.status(format!("Load DTD: {e}")),
            }
        }
        let has_dtd = info["dtd"].as_bool().unwrap_or(false);
        if ui.add_enabled(has_dtd, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Validate")))).clicked()
            && let Ok(v) = app.session.execute("xml.validate", &json!({}))
        {
            problems = Some(
                v["problems"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| format!("{}: {}", p["path"].as_str().unwrap_or(""), p["message"].as_str().unwrap_or("")))
                    .collect(),
            );
        }
        if has_dtd && ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete DTD"))).clicked() {
            let _ = app.run("xml.deleteDtd", json!({}));
            problems = None;
        }
    });
    match &problems {
        Some(list) if list.is_empty() => {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "The structure is valid.")).size(11.0).color(t.text_dim));
        }
        Some(list) => {
            for p in list {
                crate::rtl::label(ui, egui::RichText::new(format!("⚠ {p}")).size(11.0));
            }
        }
        None => {}
    }
    ui.data_mut(|d| match problems {
        Some(p) => {
            d.insert_temp(vid, p);
        }
        None => {
            d.remove::<Vec<String>>(vid);
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Export XML…"))).clicked() {
            let _ = app.run("app.exportXml", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Import XML…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("xml"))
            && let Err(e) = app.run("file.importXml", json!({"path": path}))
        {
            app.status(format!("Import XML: {e}"));
        }
    });
}
