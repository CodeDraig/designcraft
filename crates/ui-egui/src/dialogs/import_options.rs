//! importOptions dialog controls.

use super::{DesignApp, Dialog, Value, check, json, semibold};

pub(super) fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.label(egui::RichText::new(d.s("path")).size(10.5));
    check(ui, d, "removeStyles", crate::i18n::tr(&app.ui.language, "Remove Styles and Formatting from Text and Tables"));
    if !d.b("removeStyles") {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Style Name Conflicts")).font(semibold(12.0)));
        let conflicts: Vec<String> = d
            .fields
            .get("conflicts")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        ui.label(if conflicts.is_empty() {
            crate::i18n::tr(&app.ui.language, "No conflicts").to_string()
        } else {
            format!("{} conflict(s): {}", conflicts.len(), conflicts.join(", "))
        });
        let cur = d.s("styleConflicts");
        ui.horizontal(|ui| {
            for (v, l) in [("useExisting", "Use Document Style Definition"), ("redefine", "Redefine Document Style"), ("autoRename", "Auto Rename")] {
                if ui.radio(cur == v, l).clicked() {
                    d.fields.insert("styleConflicts".into(), json!(v));
                }
            }
        });
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Style Mapping")).font(semibold(12.0)));
        let names: Vec<String> = app.session.active().map(|s| s.doc.styles.paragraph.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
        let imported: Vec<String> = d
            .fields
            .get("styles")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            egui::Grid::new("imp_map").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                for w in &imported {
                    ui.label(w);
                    let mapped = d.fields.get("map").and_then(|m| m.get(w)).and_then(Value::as_str).unwrap_or("").to_string();
                    let shown = if mapped.is_empty() { "(import)".to_string() } else { mapped.clone() };
                    egui::ComboBox::from_id_salt(("imp_map", w)).selected_text(crate::rtl::widget(ui, &shown)).width(170.0).show_ui(ui, |ui| {
                        if ui.selectable_label(mapped.is_empty(), crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "(import)"))).clicked()
                            && let Some(m) = d.fields.get_mut("map").and_then(Value::as_object_mut)
                        {
                            m.remove(w);
                        }
                        for n in &names {
                            if ui.selectable_label(*n == mapped, n).clicked()
                                && let Some(m) = d.fields.get_mut("map").and_then(Value::as_object_mut)
                            {
                                m.insert(w.clone(), json!(n));
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
    }
}
