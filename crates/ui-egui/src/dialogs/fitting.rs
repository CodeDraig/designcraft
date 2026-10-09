//! fittingOptions dialog controls.

use super::{DesignApp, Dialog, check, json, semibold, text_field};

pub(super) fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    check(ui, d, "autoFit", crate::i18n::tr(&app.ui.language, "Auto-Fit"));
    ui.add_space(4.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Content Fitting")).font(semibold(12.0)));
    let cur = d.s("fitting");
    let label = |v: &str| match v {
        "fillProportionally" => crate::i18n::tr(&app.ui.language, "Fill Frame Proportionally"),
        "fitProportionally" => crate::i18n::tr(&app.ui.language, "Fit Content Proportionally"),
        "fitContentToFrame" => crate::i18n::tr(&app.ui.language, "Fit Content to Frame"),
        "centerContent" => crate::i18n::tr(&app.ui.language, "Center Content"),
        _ => crate::i18n::tr(&app.ui.language, "None"),
    };
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Fitting:"));
        egui::ComboBox::from_id_salt("ff_fit").selected_text(crate::rtl::widget(ui, label(&cur))).width(200.0).show_ui(ui, |ui| {
            for v in ["none", "fillProportionally", "fitProportionally", "fitContentToFrame", "centerContent"] {
                if ui.selectable_label(cur == v, crate::rtl::widget(ui, label(v))).clicked() {
                    d.fields.insert("fitting".into(), json!(v));
                }
            }
        });
    });
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Align From:"));
        let a = d.n("align").unwrap_or(4.0) as u8;
        egui::Grid::new("ff_align").spacing([2.0, 2.0]).show(ui, |ui| {
            for row in 0..3u8 {
                for col in 0..3u8 {
                    let i = row * 3 + col;
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                    let c = ui.visuals().text_color();
                    if a == i {
                        ui.painter().rect_filled(r.shrink(2.0), 1.0, c);
                    } else {
                        ui.painter().rect_stroke(r.shrink(3.0), 1.0, egui::Stroke::new(1.0, c), egui::StrokeKind::Inside);
                    }
                    if resp.clicked() {
                        d.fields.insert("align".into(), json!(i));
                    }
                }
                ui.end_row();
            }
        });
    });
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Crop Amount")).font(semibold(12.0)));
    egui::Grid::new("ff_crop").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        for (k, l) in [("cropTop", "Top:"), ("cropLeft", "Left:")] {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
            text_field(ui, d, k, 70.0);
        }
        ui.end_row();
        for (k, l) in [("cropBottom", "Bottom:"), ("cropRight", "Right:")] {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
            text_field(ui, d, k, 70.0);
        }
        ui.end_row();
    });
}
