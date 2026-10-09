//! Contextual object and text control bar.

use super::{Align, DesignApp, SelInfo, Sense, Stroke, Tokens, Unit, caption, icons, json, measure, number, panels, semibold, vec2};

fn vsep(ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 46.0), Sense::hover());
    ui.painter().line_segment([r.center_top() + vec2(0.0, 4.0), r.center_bottom() - vec2(0.0, 4.0)], Stroke::new(1.0, t.divider));
}

/// Two-row context-sensitive Control panel.
pub fn control_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("control_bar")
        .exact_size(59.0)
        .frame(
            egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin { left: 8, right: 8, top: 5, bottom: 3 }).stroke(Stroke::new(1.0, t.divider)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let text_mode =
                    matches!(app.session.tool_id(), "type" | "verticalType") || app.session.active().is_some_and(|d| d.selection.text.is_some());
                if text_mode {
                    control_text(app, ui);
                } else {
                    control_object(app, ui);
                }
            });
        });
}

/// A field label drawn as a tool icon (hover for its name).
fn icon_caption(ui: &mut egui::Ui, icon: &str, tip: &str) {
    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    icons::paint(ui.painter(), r, icon, Tokens::get(ui.ctx()).icon);
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    });
}

fn control_object(app: &mut DesignApp, ui: &mut egui::Ui) {
    let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let info = panels::sel_info(app);
    // Reference point proxy.
    let (r, _) = ui.allocate_exact_size(vec2(30.0, 44.0), Sense::hover());
    let t = Tokens::get(ui.ctx());
    icons::paint(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(26.0, 26.0)), "ref-point", t.icon);
    ui.painter().rect_filled(egui::Rect::from_center_size(r.center() + vec2(-7.8, -7.8), vec2(4.0, 4.0)), 0.0, t.text_strong);
    let (x, y, w, h) = match &info {
        Some(SelInfo { page_rect, .. }) => (Some(page_rect.x0), Some(page_rect.y0), Some(page_rect.width()), Some(page_rect.height())),
        None => (None, None, None, None),
    };
    egui::Grid::new("ctl_xywh").num_columns(4).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "X:"));
        if let Some(v) = measure(ui, "cx", x, units, 64.0) {
            let _ = app.run("transform.set", json!({"x": v}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "W:"));
        if let Some(v) = measure(ui, "cw", w, units, 64.0) {
            let _ = app.run("transform.set", json!({"width": v}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Y:"));
        if let Some(v) = measure(ui, "cy", y, units, 64.0) {
            let _ = app.run("transform.set", json!({"y": v}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "H:"));
        if let Some(v) = measure(ui, "ch", h, units, 64.0) {
            let _ = app.run("transform.set", json!({"height": v}));
        }
        ui.end_row();
    });
    vsep(ui);
    // Scale, rotation and shear (absolute; see Transformations are Totals).
    let tv = if info.is_some() { app.session.execute("transform.info", &json!({})).ok() } else { None };
    let tv_get = |k: &str| tv.as_ref().and_then(|v| v[k].as_f64());
    egui::Grid::new("ctl_srs").num_columns(4).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        icon_caption(ui, "tool-scale", crate::i18n::tr(&app.ui.language, "Scale X Percentage"));
        if let Some(v) = number(ui, "csx", tv_get("scaleX"), "%", 56.0, 1) {
            let _ = app.run("transform.set", json!({"scaleX": v}));
        }
        icon_caption(ui, "tool-rotate", crate::i18n::tr(&app.ui.language, "Rotation Angle"));
        if let Some(v) = number(ui, "crot", tv_get("rotation"), "°", 50.0, 1) {
            let _ = app.run("transform.set", json!({"rotation": v}));
        }
        ui.end_row();
        icon_caption(ui, "tool-scale", crate::i18n::tr(&app.ui.language, "Scale Y Percentage"));
        if let Some(v) = number(ui, "csy", tv_get("scaleY"), "%", 56.0, 1) {
            let _ = app.run("transform.set", json!({"scaleY": v}));
        }
        icon_caption(ui, "tool-shear", crate::i18n::tr(&app.ui.language, "Shear X Angle"));
        if let Some(v) = number(ui, "cshr", tv_get("shear"), "°", 50.0, 1) {
            let _ = app.run("transform.set", json!({"shear": v}));
        }
        ui.end_row();
    });
    vsep(ui);
    // Rotate / flip.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if icons::button(ui, "tool-rotate", 20.0, false, crate::i18n::tr(&app.ui.language, "Rotate 90° Counterclockwise")).clicked() {
                let _ = app.run("transform.rotate", json!({"angle": 90}));
            }
            if ui
                .small_button("⇋")
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Flip Horizontal"));
                })
                .clicked()
            {
                let _ = app.run("transform.flip", json!({"axis": "horizontal"}));
            }
        });
        ui.horizontal(|ui| {
            if ui
                .small_button("↻")
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Rotate 90° Clockwise"));
                })
                .clicked()
            {
                let _ = app.run("transform.rotate", json!({"angle": -90}));
            }
            if ui
                .small_button("⇵")
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Flip Vertical"));
                })
                .clicked()
            {
                let _ = app.run("transform.flip", json!({"axis": "vertical"}));
            }
        });
    });
    vsep(ui);
    // Fill / stroke.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Fill"));
            panels::swatch_picker(app, ui, "ctlfill", info.as_ref().map(|i| i.fill.clone()), |app, name| {
                let _ = app.run("object.fill", json!({"swatch": name}));
            });
        });
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Stroke"));
            panels::swatch_picker(app, ui, "ctlstroke", info.as_ref().map(|i| i.stroke.clone()), |app, name| {
                let _ = app.run("object.stroke", json!({"swatch": name}));
            });
            if let Some(v) = number(ui, "csw", info.as_ref().map(|i| i.stroke_weight), " pt", 46.0, 3) {
                let _ = app.run("object.stroke", json!({"weight": v}));
            }
        });
    });
    vsep(ui);
    // Opacity, fx.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Opacity"));
            if let Some(v) = number(ui, "cop", info.as_ref().map(|i| i.opacity * 100.0), "%", 42.0, 0) {
                let _ = app.run("object.opacity", json!({"opacity": v / 100.0}));
            }
        });
        ui.horizontal(|ui| {
            if icons::button(ui, "panel-effects", 20.0, info.as_ref().is_some_and(|i| i.shadow), crate::i18n::tr(&app.ui.language, "Drop Shadow"))
                .clicked()
            {
                let _ = app.run("object.dropShadow", json!({}));
            }
        });
    });
    vsep(ui);
    // Fitting.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, mode, tip) in [
                ("fit-fill", "fillProportionally", "Fill Frame Proportionally"),
                ("fit-prop", "fitProportionally", "Fit Content Proportionally"),
                ("fit-content", "fitContentToFrame", "Fit Content to Frame"),
            ] {
                if icons::button(ui, icon, 20.0, false, tip).clicked() {
                    let _ = app.run("object.fit", json!({"mode": mode}));
                }
            }
        });
        ui.horizontal(|ui| {
            for (icon, mode, tip) in [("fit-frame", "fitFrameToContent", "Fit Frame to Content"), ("fit-center", "centerContent", "Center Content")] {
                if icons::button(ui, icon, 20.0, false, tip).clicked() {
                    let _ = app.run("object.fit", json!({"mode": mode}));
                }
            }
        });
    });
    vsep(ui);
    // Text wrap.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, mode, tip) in [
                ("wrap-none", "none", "No Text Wrap"),
                ("wrap-bbox", "boundingBox", "Wrap Around Bounding Box"),
                ("wrap-contour", "contour", "Wrap Around Object Shape"),
            ] {
                let on = info.as_ref().is_some_and(|i| i.wrap == mode);
                if icons::button(ui, icon, 20.0, on, tip).clicked() {
                    let _ = app.run("object.textWrap", json!({"mode": mode}));
                }
            }
        });
        ui.horizontal(|ui| {
            for (icon, mode, tip) in [("wrap-jump", "jumpObject", "Jump Object"), ("wrap-next", "jumpToNextColumn", "Jump to Next Column")] {
                let on = info.as_ref().is_some_and(|i| i.wrap == mode);
                if icons::button(ui, icon, 20.0, on, tip).clicked() {
                    let _ = app.run("object.textWrap", json!({"mode": mode}));
                }
            }
        });
    });
    if let Some(i) = &info
        && let Some(cols) = i.columns
    {
        vsep(ui);
        ui.vertical(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Columns"));
            if let Some(v) = number(ui, "ccols", Some(cols as f64), "", 36.0, 0) {
                let _ = app.run("object.textFrameOptions", json!({"columns": v.max(1.0) as u64}));
            }
        });
    }
}

fn control_text(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let attrs = panels::text_attrs(app);
    let chars = attrs.as_ref().map(|a| a["chars"].clone()).unwrap_or_default();
    let para = attrs.as_ref().map(|a| a["para"].clone()).unwrap_or_default();
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("A").font(semibold(15.0)).color(t.text_strong));
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("¶").font(semibold(15.0)).color(t.text_dim));
        });
    });
    vsep(ui);
    ui.vertical(|ui| {
        let fam = chars["fontFamily"].as_str().unwrap_or("").to_string();
        let sty = chars["fontStyle"].as_str().unwrap_or("").to_string();
        panels::font_family_picker(app, ui, &fam, 170.0);
        panels::font_style_picker(app, ui, &fam, &sty, 170.0);
    });
    vsep(ui);
    egui::Grid::new("ctl_char").num_columns(4).spacing(vec2(4.0, 3.0)).show(ui, |ui| {
        caption(ui, "𝐓");
        if let Some(v) = number(ui, "csize", chars["size"].as_f64(), " pt", 54.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"size": v}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "VA"));
        if let Some(v) = number(ui, "ctrack", chars["tracking"].as_f64(), "", 48.0, 0) {
            let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
        }
        ui.end_row();
        caption(ui, "Ā");
        let lead = match chars["leading"]["kind"].as_str() {
            Some("points") => chars["leading"]["value"].as_f64(),
            _ => chars["size"].as_f64().map(|s| s * para["autoLeading"].as_f64().unwrap_or(1.2)),
        };
        if let Some(v) = number(ui, "clead", lead, " pt", 54.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "IT"));
        if let Some(v) = number(ui, "chs", chars["hScale"].as_f64().map(|v| v * 100.0), "%", 48.0, 0) {
            let _ = app.run("type.char", json!({"attrs": {"hScale": v / 100.0}}));
        }
        ui.end_row();
    });
    vsep(ui);
    ui.vertical(|ui| {
        let cur: Align = serde_json::from_value(para["align"].clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            for (a, icon) in [(Align::Left, "align-left"), (Align::Center, "align-center"), (Align::Right, "align-right")] {
                if icons::button(ui, icon, 20.0, cur == a, a.label()).clicked() {
                    let _ = app.run("type.para", json!({"attrs": {"align": a}}));
                }
            }
        });
        ui.horizontal(|ui| {
            for (a, icon) in [(Align::LeftJustified, "align-justify"), (Align::FullyJustified, "align-justify-all")] {
                if icons::button(ui, icon, 20.0, cur == a, a.label()).clicked() {
                    let _ = app.run("type.para", json!({"attrs": {"align": a}}));
                }
            }
        });
    });
    vsep(ui);
    let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    egui::Grid::new("ctl_para").num_columns(4).spacing(vec2(4.0, 3.0)).show(ui, |ui| {
        caption(ui, "→|");
        if let Some(v) = measure(ui, "cli", para["leftIndent"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"leftIndent": v}}));
        }
        caption(ui, "↑¶");
        if let Some(v) = measure(ui, "csb", para["spaceBefore"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"spaceBefore": v}}));
        }
        ui.end_row();
        caption(ui, "→¶");
        if let Some(v) = measure(ui, "cfi", para["firstLineIndent"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"firstLineIndent": v}}));
        }
        caption(ui, "↓¶");
        if let Some(v) = measure(ui, "csa", para["spaceAfter"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"spaceAfter": v}}));
        }
        ui.end_row();
    });
    vsep(ui);
    ui.vertical(|ui| {
        let ps = attrs.as_ref().and_then(|a| a["paragraphStyle"].as_str()).unwrap_or("").to_string();
        let ov = attrs.as_ref().and_then(|a| a["paraOverrides"].as_u64()).unwrap_or(0)
            + attrs.as_ref().and_then(|a| a["charOverrides"].as_u64()).unwrap_or(0);
        panels::para_style_picker(app, ui, &ps, ov > 0, 170.0);
        let hy = para["hyphenate"].as_bool().unwrap_or(true);
        let mut h = hy;
        if ui.checkbox(&mut h, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Hyphenate"))).changed() {
            let _ = app.run("type.para", json!({"attrs": {"hyphenate": h}}));
        }
    });
}
