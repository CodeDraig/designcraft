//! Text wrap, pathfinder, and alignment panels.

use super::{DesignApp, caption, icons, json, measure, sel_info, units};

pub fn wrap_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let info = sel_info(app);
    ui.horizontal(|ui| {
        for (icon, mode, tip) in [
            ("wrap-none", "none", "No Text Wrap"),
            ("wrap-bbox", "boundingBox", "Wrap Around Bounding Box"),
            ("wrap-contour", "contour", "Wrap Around Object Shape"),
            ("wrap-jump", "jumpObject", "Jump Object"),
            ("wrap-next", "jumpToNextColumn", "Jump to Next Column"),
        ] {
            let on = info.as_ref().is_some_and(|i| i.wrap == mode);
            if icons::button(ui, icon, 26.0, on, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.textWrap", json!({"mode": mode}));
            }
        }
    });
    let u = units(app);
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Offset"));
        if let Some(v) = measure(ui, "wo", Some(0.0), u, 60.0) {
            let _ = app.run("object.textWrap", json!({"mode": info.as_ref().map(|i| i.wrap).unwrap_or("boundingBox"), "offset": v}));
        }
    });
}

/// Pathfinder panel: combine the selected shapes; make or release compound paths.
pub fn pathfinder_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Pathfinder")).strong());
    ui.horizontal_wrapped(|ui| {
        for (label, op) in
            [("Add", "add"), ("Subtract", "subtract"), ("Intersect", "intersect"), ("Exclude Overlap", "exclude"), ("Minus Back", "minusBack")]
        {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = app.run("object.pathfinder", json!({"op": op}));
            }
        }
    });
    ui.add_space(4.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Paths")).strong());
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Make Compound"))).clicked() {
            let _ = app.run("object.makeCompoundPath", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Release"))).clicked() {
            let _ = app.run("object.releaseCompoundPath", json!({}));
        }
    });
}

pub fn align_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for (edge, tip) in [
            ("left", "Align left edges"),
            ("hcenter", "Align horizontal centers"),
            ("right", "Align right edges"),
            ("top", "Align top edges"),
            ("vcenter", "Align vertical centers"),
            ("bottom", "Align bottom edges"),
        ] {
            if ui
                .small_button(match edge {
                    "left" => "⇤",
                    "hcenter" => "↔",
                    "right" => "⇥",
                    "top" => "⤒",
                    "vcenter" => "↕",
                    _ => "⤓",
                })
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, tip));
                })
                .clicked()
            {
                let _ = app.run("object.align", json!({"edge": edge}));
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Distribute ↔"))).clicked() {
            let _ = app.run("object.distribute", json!({"axis": "horizontal"}));
        }
        if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Distribute ↕"))).clicked() {
            let _ = app.run("object.distribute", json!({"axis": "vertical"}));
        }
    });
}
