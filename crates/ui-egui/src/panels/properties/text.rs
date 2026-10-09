//! Contextual text and text-frame properties.

use super::{
    Align, DesignApp, FIELD_H, NumField, Rect, Sense, Stroke, StrokeKind, Tokens, Ui, block, full_width, icon_toggle, icons, json, link_label,
    more_options, outline_button, place, pos2, section, sub, swatch_menu, text_attrs, units, vec2, widgets,
};

pub(super) fn text_style_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    section(ui, crate::i18n::tr(&app.ui.language, "Text Style"), true);
    let Some(a) = text_attrs(app) else { return };
    let tab = app.ui.text_style_tab;
    let fw = full_width(ui);
    if let Some(k) = widgets::segmented(
        ui,
        &[crate::i18n::tr(&app.ui.language, "Paragraph Styles"), crate::i18n::tr(&app.ui.language, "Character Styles")],
        tab as usize,
        fw,
    ) {
        app.ui.text_style_tab = k as u8;
    }
    ui.add_space(5.0);
    let (names, cur): (Vec<String>, String) = {
        let Some(st) = app.session.active() else { return };
        if tab == 0 {
            (
                st.doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| n != designcraft_doc::NO_PARA_STYLE).collect(),
                a["paragraphStyle"].as_str().unwrap_or("").to_string(),
            )
        } else {
            (st.doc.styles.character.iter().map(|p| p.name.clone()).collect(), a["characterStyle"].as_str().unwrap_or("[None]").to_string())
        }
    };
    let ov = if tab == 0 { a["paraOverrides"].as_u64().unwrap_or(0) + a["charOverrides"].as_u64().unwrap_or(0) } else { 0 };
    let shown = crate::i18n::style_name(&app.ui.language, &cur);
    let label = if ov > 0 { format!("{shown}+") } else { shown.to_string() };
    let resp = widgets::dropdown_h(ui, "", fw, 34.0);
    // "Ag" swatch + style name.
    let ag = Rect::from_min_size(resp.rect.min + vec2(6.0, 6.0), vec2(22.0, 22.0));
    ui.painter().rect_filled(ag, 1.0, egui::Color32::WHITE);
    ui.painter().text(ag.center(), egui::Align2::CENTER_CENTER, "Ag", egui::FontId::new(13.0, egui::FontFamily::Proportional), egui::Color32::BLACK);
    crate::rtl::paint(
        &ui.painter().with_clip_rect(resp.rect.shrink2(vec2(20.0, 0.0))),
        pos2(ag.max.x + 8.0, resp.rect.center().y),
        egui::Align2::LEFT_CENTER,
        &label,
        egui::FontId::proportional(11.5),
        t.text,
    );
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(fw);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for n in &names {
                if ui.selectable_label(*n == cur, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                    pick = Some(n.clone());
                    ui.close();
                }
            }
        });
    });
    if let Some(n) = pick {
        if tab == 0 {
            let _ = app.run("style.paragraph.apply", json!({"name": n, "clearOverrides": false}));
        } else {
            let _ = app.run("style.character.apply", json!({"name": n}));
        }
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if icon_toggle(
            ui,
            "pilcrow-menu",
            false,
            crate::i18n::tr(&app.ui.language, if tab == 0 { "Paragraph Styles panel" } else { "Character Styles panel" }),
        )
        .clicked()
        {
            app.ui.open_panel = Some(if tab == 0 { "paragraphStyles" } else { "characterStyles" }.into());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.add_space(4.0);
            if icon_toggle(ui, "style-new", false, crate::i18n::tr(&app.ui.language, crate::i18n::tr(&app.ui.language, "Create new style"))).clicked()
            {
                let cmd = if tab == 0 { "style.paragraph.create" } else { "style.character.create" };
                let _ = app.run(cmd, json!({}));
            }
            if icon_toggle(ui, "style-clear", false, crate::i18n::tr(&app.ui.language, crate::i18n::tr(&app.ui.language, "Clear overrides")))
                .clicked()
                && tab == 0
                && !cur.is_empty()
            {
                let _ = app.run("style.paragraph.apply", json!({"name": cur, "clearOverrides": true}));
            }
            if icon_toggle(ui, "style-load", false, crate::i18n::tr(&app.ui.language, "Styles panel")).clicked() {
                app.ui.open_panel = Some(if tab == 0 { "paragraphStyles" } else { "characterStyles" }.into());
            }
        });
    });
}

pub(super) fn text_appearance_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(a) = text_attrs(app) else { return };
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let c = &a["chars"];
    for (k, (key, label)) in [("fill", "Fill"), ("stroke", "Stroke")].into_iter().enumerate() {
        let sw = c[key].as_str().unwrap_or(designcraft_color::swatch::NONE).to_string();
        let b = block(ui, FIELD_H);
        let (col, g) = widgets::swatch_colors(&doc, &sw, 1.0);
        let chip = sub(b, 4.0, 2.0, 17.0, 17.0);
        if k == 0 {
            widgets::paint_chip(ui.painter(), chip, col, g);
        } else {
            widgets::paint_stroke_chip(ui.painter(), chip, col, egui::Color32::from_gray(20));
        }
        ui.painter().rect_stroke(chip, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
        // The "T" marks text formatting.
        let tc = if col.is_some_and(|c| c.r() as u32 + c.g() as u32 + c.b() as u32 > 380) { egui::Color32::BLACK } else { egui::Color32::WHITE };
        if k == 0 {
            ui.painter().text(chip.center(), egui::Align2::CENTER_CENTER, "T", crate::theme::semibold(12.0), tc);
        }
        let resp = ui.interact(chip, ui.id().with(("tchip", k)), Sense::click()).on_hover_text(format!("{label}: {sw}"));
        swatch_menu(app, &resp, &doc, &sw, |app, n| {
            let _ = app.run("type.char", json!({"attrs": {key: n}}));
        });
        if k == 0 {
            crate::rtl::paint(
                ui.painter(),
                b.min + vec2(34.0, FIELD_H / 2.0),
                egui::Align2::LEFT_CENTER,
                crate::i18n::tr(&app.ui.language, label),
                egui::FontId::proportional(11.5),
                t.text,
            );
        } else {
            if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, label), 56.0)).clicked() {
                app.ui.open_panel = Some("stroke".into());
            }
            const WEIGHTS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 2.0, 3.0, 4.0, 5.0, 10.0];
            if let Some(v) = place(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H), |ui| {
                NumField::number("tsw", c["strokeWeight"].as_f64(), " pt", 3).width(69.0).spinner().presets(WEIGHTS).range(0.0, 100.0).show(ui)
            }) {
                let _ = app.run("type.char", json!({"attrs": {"strokeWeight": v}}));
            }
        }
        ui.add_space(4.0);
    }
}

/// Character section: font family (with search), style, size/leading, kerning/tracking, •••.
pub(super) fn character_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(a) = text_attrs(app) else { return };
    let c = a["chars"].clone();
    let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
    let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
    let fw = full_width(ui);
    // Family dropdown with a search segment on the left.
    let fonts = super::super::fonts(app);
    let menu = super::super::font_menu(app);
    let resp = widgets::dropdown(ui, "", fw);
    let r = resp.rect;
    icons::paint(ui.painter(), Rect::from_min_size(r.min + vec2(3.0, 3.0), vec2(14.0, 14.0)), "search", t.icon);
    widgets::chevron_down(ui.painter(), r.min + vec2(22.0, 10.5), t.icon);
    ui.painter().text(
        r.min + vec2(30.0, 10.5),
        egui::Align2::LEFT_CENTER,
        if fam.is_empty() { "—".to_string() } else { super::super::font_label(app, &menu, &fam) },
        egui::FontId::proportional(11.5),
        t.text,
    );
    let mut pick_fam = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(fw);
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            if let Some(f) = super::super::font_menu_rows(app, ui, &menu, &fam) {
                pick_fam = Some(f);
                ui.close();
            }
        });
    });
    if let Some(f) = pick_fam {
        let styles = fonts.styles(&f);
        let style = if styles.iter().any(|s| s == "Regular") { "Regular".to_string() } else { styles.first().cloned().unwrap_or_default() };
        let _ = app.run("type.char", json!({"attrs": {"fontFamily": f, "fontStyle": style}}));
    }
    ui.add_space(1.0);
    let styles = fonts.styles(&fam);
    let cur = styles.iter().position(|s| *s == sty);
    if let Some(k) = widgets::dropdown_list(ui, if sty.is_empty() { "—" } else { &sty }, fw, &styles, cur) {
        let _ = app.run("type.char", json!({"attrs": {"fontStyle": styles[k]}}));
    }
    ui.add_space(1.0);
    const SIZES: &[f64] = &[6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 21.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0];
    let half = (fw - 10.0) / 2.0;
    let b = block(ui, 2.0 * FIELD_H + 5.0);
    let size = c["size"].as_f64();
    let auto = c["leading"]["kind"].as_str() != Some("points");
    let lv = if auto { size.map(|s| s * a["para"]["autoLeading"].as_f64().unwrap_or(1.2)) } else { c["leading"]["value"].as_f64() };
    icons::paint(ui.painter(), sub(b, 0.0, 1.0, 19.0, 19.0), "font-size", t.icon);
    if let Some(v) = place(ui, sub(b, 24.0, 0.0, half - 24.0, FIELD_H), |ui| {
        NumField::number("csize", size, " pt", 2).width(half - 24.0).spinner().presets(SIZES).range(0.1, 1296.0).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"size": v}}));
    }
    icons::paint(ui.painter(), sub(b, half + 10.0, 1.0, 19.0, 19.0), "leading", t.icon);
    let lw = half - 24.0;
    let lshow = lv.map(|v| (v * 100.0).round() / 100.0);
    if let Some(v) = place(ui, sub(b, half + 34.0, 0.0, lw, FIELD_H), |ui| {
        let key = if auto { "cleadauto" } else { "clead" };
        // Auto leading is shown in parentheses, like "(14.4 pt)".
        NumField::number(key, lshow, " pt", 2).width(lw).spinner().presets(SIZES).range(0.0, 5000.0).parens(auto).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
    }
    // Kerning / tracking.
    icons::paint(ui.painter(), sub(b, 0.0, 27.0, 19.0, 19.0), "kerning", t.icon);
    let kern = c["kerning"]["kind"].as_str().unwrap_or("metrics").to_string();
    let kern_label = match kern.as_str() {
        "optical" => "Optical".to_string(),
        "none" => "0".to_string(),
        "manual" => format!("{}", c["kerning"]["value"].as_f64().unwrap_or(0.0)),
        _ => "Metrics".to_string(),
    };
    place(ui, sub(b, 24.0, 26.0, half - 24.0, FIELD_H), |ui| {
        let resp = widgets::dropdown(ui, crate::i18n::tr(&app.ui.language, &kern_label), half - 24.0);
        egui::Popup::menu(&resp).show(|ui| {
            for (label, v) in [("Metrics", json!({"kind": "metrics"})), ("Optical", json!({"kind": "optical"})), ("0", json!({"kind": "none"}))] {
                if ui.selectable_label(kern_label == label, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("type.char", json!({"attrs": {"kerning": v}}));
                    ui.close();
                }
            }
        });
    });
    icons::paint(ui.painter(), sub(b, half + 10.0, 27.0, 19.0, 19.0), "tracking", t.icon);
    const TRACK: &[f64] = &[-100.0, -75.0, -50.0, -25.0, -10.0, -5.0, 0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 200.0];
    if let Some(v) = place(ui, sub(b, half + 34.0, 26.0, lw, FIELD_H), |ui| {
        NumField::number("ctrack", c["tracking"].as_f64(), "", 0).width(lw).spinner().presets(TRACK).step(5.0).range(-1000.0, 10000.0).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
    }
    if more_options(ui, &app.ui.language).clicked() {
        app.ui.open_panel = Some("character".into());
    }
}

/// Paragraph section: the 9 alignment buttons and •••.
pub(super) fn paragraph_section(app: &mut DesignApp, ui: &mut Ui) {
    let Some(a) = text_attrs(app) else { return };
    let cur: Align = serde_json::from_value(a["para"]["align"].clone()).unwrap_or_default();
    let fw = full_width(ui);
    let pitch = fw / 9.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (al, icon) in [
            (Align::Left, "palign-left"),
            (Align::Center, "palign-center"),
            (Align::Right, "palign-right"),
            (Align::LeftJustified, "palign-justify-left"),
            (Align::CenterJustified, "palign-justify-center"),
            (Align::RightJustified, "palign-justify-right"),
            (Align::FullyJustified, "palign-justify-all"),
            (Align::TowardsSpine, "palign-spine-towards"),
            (Align::AwayFromSpine, "palign-spine-away"),
        ] {
            if widgets::icon_toggle_sized(ui, icon, cur == al, al.label(), vec2(pitch.min(23.0), 21.0)).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"align": al}}));
            }
            if pitch > 23.0 {
                ui.add_space(pitch - 23.0);
            }
        }
    });
    if more_options(ui, &app.ui.language).clicked() {
        app.ui.open_panel = Some("paragraph".into());
    }
}

pub(super) fn bullets_section(app: &mut DesignApp, ui: &mut Ui) {
    let Some(a) = text_attrs(app) else { return };
    let lt = a["para"]["listType"].as_str().unwrap_or("none").to_string();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (icon, kind, tip) in [("bullets", "bullets", "Bulleted List"), ("numbering", "numbers", "Numbered List")] {
            let on = lt == kind;
            if icon_toggle(ui, icon, on, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"listType": if on { "none" } else { kind }}}));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if outline_button(ui, crate::i18n::tr(&app.ui.language, "Options"), 66.0).clicked() {
                app.ui.open_panel = Some("paragraph".into());
            }
        });
    });
}

pub(super) fn quick_actions_text(app: &mut DesignApp, ui: &mut Ui) {
    let fw = full_width(ui);
    let hw = (fw - 10.0) / 2.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let r = outline_button(ui, crate::i18n::tr(&app.ui.language, "Change Case"), hw);
        egui::Popup::menu(&r).show(|ui| {
            for (label, v) in [("UPPERCASE", "allCaps"), ("Small Caps", "smallCaps"), ("Normal", "normal")] {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("type.char", json!({"attrs": {"capitalization": v}}));
                    ui.close();
                }
            }
        });
        if outline_button(ui, crate::i18n::tr(&app.ui.language, "Story Editor"), hw).clicked() {
            let _ = app.run("app.storyEditor", json!({}));
        }
    });
    ui.add_space(4.0);
    if outline_button(ui, crate::i18n::tr(&app.ui.language, "Fill with Placeholder Text"), fw).clicked() {
        let _ = app.run("type.fillWithPlaceholder", json!({}));
    }
}

/// Text Frame section: columns, gutter, [Options].
pub(super) fn text_frame_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let u = units(app);
    let Some(st) = app.session.active() else { return };
    let fid = st.selection.items.first().copied().or_else(|| st.selection.text.and_then(|t| t.frame));
    let Some(opts) = fid.and_then(|f| st.doc.item(f)).and_then(|i| i.text_frame()).map(|t| t.options.clone()) else { return };
    let b = block(ui, FIELD_H);
    icons::paint(ui.painter(), sub(b, 0.0, 1.0, 19.0, 19.0), "text-columns", t.icon);
    if let Some(v) = place(ui, sub(b, 22.0, 0.0, 54.0, FIELD_H), |ui| {
        NumField::number("tfcols", Some(opts.columns as f64), "", 0).width(54.0).spinner().range(1.0, 40.0).show(ui)
    }) {
        let _ = app.run("object.textFrameOptions", json!({"columns": v.max(1.0) as u64}));
    }
    icons::paint(ui.painter(), sub(b, 83.0, 1.0, 19.0, 19.0), "text-gutter", t.icon);
    if let Some(v) = place(ui, sub(b, 105.0, 0.0, 54.0, FIELD_H), |ui| {
        NumField::measure("tfgut", Some(opts.gutter), u).width(54.0).spinner().range(0.0, 1440.0).show(ui)
    }) {
        let _ = app.run("object.textFrameOptions", json!({"gutter": v}));
    }
    let ow = (b.max.x - 8.0) - (b.min.x + 164.0);
    if place(ui, sub(b, 164.0, 0.5, ow, 20.0), |ui| outline_button(ui, crate::i18n::tr(&app.ui.language, "Options"), ow)).clicked() {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
    }
}

/// Languages offered for text (InDesign-style names).
pub(super) const LANGUAGES: &[&str] = &[
    "[No Language]",
    "English: USA",
    "English: UK",
    "English: Canadian",
    "German: 2006 Reform",
    "German: Swiss 2006 Reform",
    "French",
    "French: Canadian",
    "Spanish",
    "Italian",
    "Portuguese",
    "Portuguese: Brazilian",
    "Dutch: 2005 Reform",
    "Danish",
    "Swedish",
    "Norwegian: Bokmål",
    "Finnish",
    "Polish",
    "Czech",
    "Hungarian",
    "Greek",
    "Russian",
    "Ukrainian",
    "Turkish",
    "Japanese",
    "Chinese",
    "Chinese: Simplified",
    "Chinese: Traditional",
    "Korean",
    "Arabic",
    "Hebrew",
];
