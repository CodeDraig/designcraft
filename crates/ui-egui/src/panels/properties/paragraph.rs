//! Paragraph formatting panel.

use super::{Align, DesignApp, caption, icons, json, measure, number, text_attrs, units, vec2};

pub fn paragraph_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(a) = text_attrs(app) else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
        return;
    };
    let u = units(app);
    let p = a["para"].clone();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Paragraph")).strong());
    let cur: Align = serde_json::from_value(p["align"].clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        for (al, icon) in [
            (Align::Left, "align-left"),
            (Align::Center, "align-center"),
            (Align::Right, "align-right"),
            (Align::LeftJustified, "align-justify"),
            (Align::FullyJustified, "align-justify-all"),
        ] {
            if icons::button(ui, icon, 24.0, cur == al, al.label()).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"align": al}}));
            }
        }
    });
    // Paragraph Direction (World-Ready): switching also mirrors left/right alignment.
    let rtl = p["direction"] == "rightToLeft";
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Direction"));
        for (on, label, dir) in [(!rtl, "Left to Right", "leftToRight"), (rtl, "Right to Left", "rightToLeft")] {
            if ui.selectable_label(on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() && !on {
                let align = match cur {
                    Align::Left => Align::Right,
                    Align::Right => Align::Left,
                    a => a,
                };
                let _ = app.run("type.para", json!({"attrs": {"direction": dir, "align": align}}));
            }
        }
    });
    egui::Grid::new("paragrid").num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        for (row, ((la, ka), (lb, kb))) in [
            (("Left", "leftIndent"), ("Right", "rightIndent")),
            (("First", "firstLineIndent"), ("Last", "lastLineIndent")),
            (("Before", "spaceBefore"), ("After", "spaceAfter")),
        ]
        .into_iter()
        .enumerate()
        {
            caption(ui, crate::i18n::tr(&app.ui.language, la));
            if let Some(v) = measure(ui, &format!("pp{row}a"), p[ka].as_f64(), u, 64.0) {
                let _ = app.run("type.para", json!({"attrs": {ka: v}}));
            }
            caption(ui, crate::i18n::tr(&app.ui.language, lb));
            if let Some(v) = measure(ui, &format!("pp{row}b"), p[kb].as_f64(), u, 64.0) {
                let _ = app.run("type.para", json!({"attrs": {kb: v}}));
            }
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        let mut h = p["hyphenate"].as_bool().unwrap_or(true);
        if ui.checkbox(&mut h, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Hyphenate"))).changed() {
            let _ = app.run("type.para", json!({"attrs": {"hyphenate": h}}));
        }
        let mut grid = p["gridAlign"].as_str() == Some("allLines");
        if ui.checkbox(&mut grid, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Align to baseline grid"))).changed() {
            let _ = app.run("type.para", json!({"attrs": {"gridAlign": if grid { "allLines" } else { "none" }}}));
        }
    });
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Composer"));
        let single = p["composer"].as_str() == Some("singleLine");
        if ui.selectable_label(!single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Paragraph"))).clicked() {
            let _ = app.run("type.para", json!({"attrs": {"composer": "paragraph"}}));
        }
        if ui.selectable_label(single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Single-line"))).clicked() {
            let _ = app.run("type.para", json!({"attrs": {"composer": "singleLine"}}));
        }
    });
    // Paragraph Border and Shading.
    let swatches: Vec<String> =
        app.session.active().map(|d| d.doc.swatches.iter().filter(|w| !w.hidden).map(|w| w.name.clone()).collect()).unwrap_or_default();
    for (on_key, color_key, label) in [("shadingOn", "shadingColor", "Shading"), ("borderOn", "borderColor", "Border")] {
        ui.horizontal(|ui| {
            let mut on = p[on_key].as_bool().unwrap_or(false);
            if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).changed() {
                let _ = app.run("type.para", json!({ on_key: on }));
            }
            if on {
                let cur = p[color_key].as_str().unwrap_or("[Black]").to_string();
                egui::ComboBox::from_id_salt(("pbs", label)).selected_text(&cur).width(110.0).show_ui(ui, |ui| {
                    for w in &swatches {
                        if ui.selectable_label(*w == cur, w).clicked() {
                            let _ = app.run("type.para", json!({ color_key: w }));
                        }
                    }
                });
                if label == "Shading" {
                    if let Some(v) = number(ui, "pshtint", p["shadingTint"].as_f64().map(|t| t * 100.0), "%", 46.0, 0) {
                        let _ = app.run("type.para", json!({"shadingTint": (v / 100.0).clamp(0.0, 1.0)}));
                    }
                } else if let Some(v) = number(ui, "pbw", p["borderWeights"][0].as_f64(), " pt", 50.0, 2) {
                    let w = [v.max(0.0); 4];
                    let _ = app.run("type.para", json!({ "borderWeights": w }));
                }
            }
        });
    }
    if p["shadingOn"].as_bool().unwrap_or(false) || p["borderOn"].as_bool().unwrap_or(false) {
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Offsets"));
            let o = p["borderOffsets"][0].as_f64().or(p["shadingOffsets"][0].as_f64());
            if let Some(v) = number(ui, "pbo", o, " pt", 50.0, 2) {
                let o = [v; 4];
                let _ = app.run("type.para", json!({ "borderOffsets": o, "shadingOffsets": o }));
            }
        });
    }
    let ps = a["paragraphStyle"].as_str().unwrap_or("").to_string();
    let ov = a["paraOverrides"].as_u64().unwrap_or(0) + a["charOverrides"].as_u64().unwrap_or(0);
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Style"));
        super::super::para_style_picker(app, ui, &ps, ov > 0, 190.0);
    });
}
