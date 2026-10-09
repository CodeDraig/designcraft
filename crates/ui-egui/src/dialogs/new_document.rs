//! newDocument dialog controls.

use super::{DesignApp, Dialog, check, format_measure, json, text_field};

pub(super) fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        ui.with_layout(
            if crate::i18n::is_rtl(&app.ui.language) {
                egui::Layout::right_to_left(egui::Align::Center)
            } else {
                egui::Layout::left_to_right(egui::Align::Center)
            },
            |ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Preset"));
                let cur = d.s("preset");
                egui::ComboBox::from_id_salt("preset").selected_text(&cur).width(180.0).show_ui(ui, |ui| {
                    for p in designcraft_doc::build::PRESETS {
                        if ui.selectable_label(cur == p.name, p.name).clicked() {
                            d.fields.insert("preset".into(), json!(p.name));
                            d.fields.insert("width".into(), json!(format_measure(p.width, p.units)));
                            d.fields.insert("height".into(), json!(format_measure(p.height, p.units)));
                            d.fields.insert("facingPages".into(), json!(p.intent == designcraft_doc::Intent::Print));
                        }
                    }
                });
            },
        );
    });
    if crate::i18n::is_rtl(&app.ui.language) {
        // egui Grid currently supports LTR only. Use explicit RTL rows;
        // the numeric editors retain LTR alignment and logical values.
        for (first, key, second, other, toggle) in [
            ("Width", "width", "Height", "height", false),
            ("Pages", "pages", "Facing Pages", "facingPages", true),
            ("Columns", "columns", "Gutter", "gutter", false),
            ("Top", "marginTop", "Bottom", "marginBottom", false),
            ("Inside", "marginInside", "Outside", "marginOutside", false),
            ("Bleed", "bleed", "Primary Text Frame", "primaryTextFrame", true),
        ] {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, first));
                    text_field(ui, d, key, 80.0);
                    if toggle {
                        check(ui, d, other, crate::i18n::tr(&app.ui.language, second));
                    } else {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, second));
                        text_field(ui, d, other, 80.0);
                    }
                });
            });
        }
    } else {
        egui::Grid::new("nd").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Width"));
            text_field(ui, d, "width", 80.0);
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Height"));
            text_field(ui, d, "height", 80.0);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Pages"));
            text_field(ui, d, "pages", 80.0);
            ui.label("");
            check(ui, d, "facingPages", crate::i18n::tr(&app.ui.language, "Facing Pages"));
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Columns"));
            text_field(ui, d, "columns", 80.0);
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Gutter"));
            text_field(ui, d, "gutter", 80.0);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Top"));
            text_field(ui, d, "marginTop", 80.0);
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Bottom"));
            text_field(ui, d, "marginBottom", 80.0);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Inside"));
            text_field(ui, d, "marginInside", 80.0);
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Outside"));
            text_field(ui, d, "marginOutside", 80.0);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Bleed"));
            text_field(ui, d, "bleed", 80.0);
            ui.label("");
            check(ui, d, "primaryTextFrame", crate::i18n::tr(&app.ui.language, "Primary Text Frame"));
            ui.end_row();
        });
    }
}
