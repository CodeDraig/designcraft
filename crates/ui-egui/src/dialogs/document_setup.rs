//! Document setup dialog state and controls.

use super::{DesignApp, Dialog, Unit, Value, check, format_measure, json, semibold, text_field};

/// File › Document Setup, filled from the document.
pub fn open_document_setup(app: &mut DesignApp) {
    let Ok(cur) = app.session.execute("layout.documentSetup", &json!({})) else { return };
    let units = app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let fm = |v: &Value| json!(format_measure(v.as_f64().unwrap_or(0.0), units));
    let mut f = json!({"intent": cur["intent"], "pages": cur["pages"], "startPage": cur["startPage"], "facingPages": cur["facingPages"],
        "binding": cur["binding"], "width": fm(&cur["width"]), "height": fm(&cur["height"])});
    for k in ["bleed", "slug"] {
        for (i, e) in ["Top", "Bottom", "Inside", "Outside"].iter().enumerate() {
            f[format!("{k}{e}")] = fm(&cur[k][i]);
        }
    }
    app.ui.dialog = Some(Dialog::new("documentSetup", f));
}

pub(super) fn document_setup(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    egui::Grid::new("ds").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Intent:"));
        let cur = d.s("intent");
        egui::ComboBox::from_id_salt("ds_intent")
            .selected_text(match cur.as_str() {
                "web" => crate::i18n::tr(&app.ui.language, "Web"),
                "mobile" => crate::i18n::tr(&app.ui.language, "Mobile"),
                _ => crate::i18n::tr(&app.ui.language, "Print"),
            })
            .width(110.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("print", "Print"), ("web", "Web"), ("mobile", "Mobile")] {
                    if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        d.fields.insert("intent".into(), json!(v));
                    }
                }
            });
        ui.label("");
        ui.label("");
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Number of Pages:"));
        text_field(ui, d, "pages", 60.0);
        check(ui, d, "facingPages", crate::i18n::tr(&app.ui.language, "Facing Pages"));
        ui.label("");
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Start Page #:"));
        text_field(ui, d, "startPage", 60.0);
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Binding:"));
        let rtl = d.s("binding") == "rightToLeft";
        egui::ComboBox::from_id_salt("ds_binding")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if rtl { "Right to Left" } else { "Left to Right" })))
            .width(110.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("leftToRight", "Left to Right"), ("rightToLeft", "Right to Left")] {
                    if ui.selectable_label((v == "rightToLeft") == rtl, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        d.fields.insert("binding".into(), json!(v));
                    }
                }
            });
        ui.end_row();
    });
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page Size")).font(semibold(12.0)));
    let (w, h) = (d.m("width").unwrap_or(612.0), d.m("height").unwrap_or(792.0));
    let preset =
        designcraft_doc::build::PRESETS.iter().find(|p| (p.width - w).abs() < 0.5 && (p.height - h).abs() < 0.5).map_or("Custom", |p| p.name);
    egui::Grid::new("ds_size").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Size:"));
        egui::ComboBox::from_id_salt("ds_preset").selected_text(preset).width(140.0).show_ui(ui, |ui| {
            for p in designcraft_doc::build::PRESETS {
                if ui.selectable_label(p.name == preset, p.name).clicked() {
                    d.fields.insert("width".into(), json!(format_measure(p.width, p.units)));
                    d.fields.insert("height".into(), json!(format_measure(p.height, p.units)));
                }
            }
        });
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Orientation:"));
        ui.horizontal(|ui| {
            for (portrait, l) in [(true, "Portrait"), (false, "Landscape")] {
                if ui.selectable_label((h >= w) == portrait, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked()
                    && (h >= w) != portrait
                {
                    let (fw, fh) = (d.s("width"), d.s("height"));
                    d.fields.insert("width".into(), json!(fh));
                    d.fields.insert("height".into(), json!(fw));
                }
            }
        });
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Width:"));
        text_field(ui, d, "width", 80.0);
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Height:"));
        text_field(ui, d, "height", 80.0);
        ui.end_row();
    });
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Bleed and Slug")).font(semibold(12.0)));
    egui::Grid::new("ds_bleed").num_columns(5).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label("");
        for e in ["Top", "Bottom", "Inside", "Outside"] {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, e));
        }
        ui.end_row();
        for (k, l) in [("bleed", "Bleed:"), ("slug", "Slug:")] {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
            for e in ["Top", "Bottom", "Inside", "Outside"] {
                text_field(ui, d, &format!("{k}{e}"), 60.0);
            }
            ui.end_row();
        }
    });
    check(ui, d, "adjustLayout", crate::i18n::tr(&app.ui.language, "Adjust Layout (objects follow the new page size)"));
}
