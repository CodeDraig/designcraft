//! Character formatting and variable-font controls.

use super::{DesignApp, LANGUAGES, caption, json, number, text_attrs, vec2};

pub fn character_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(a) = text_attrs(app) else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
        return;
    };
    let c = a["chars"].clone();
    let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
    let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Character")).strong());
    super::super::font_family_picker(app, ui, &fam, 220.0);
    super::super::font_style_picker(app, ui, &fam, &sty, 220.0);
    variable_font_axes(app, ui, &fam, &sty);
    // Language: hyphenation, spelling and typographer's quotes follow it.
    let lang = c["language"].as_str().unwrap_or("English: USA").to_string();
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Language"));
        egui::ComboBox::from_id_salt("char_language")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &lang)))
            .width(170.0)
            .show_ui(ui, |ui| {
                for l in LANGUAGES {
                    if ui.selectable_label(*l == lang, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        let _ = app.run("type.char", json!({"attrs": {"language": l}}));
                    }
                }
            });
    });
    // Digits (World-Ready): shown for right-to-left paragraphs or once set.
    let digits = c["digits"].as_str().unwrap_or("default").to_string();
    if digits != "default" || a["para"]["direction"].as_str() == Some("rightToLeft") {
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Digits"));
            let opts = [("default", "Default"), ("arabic", "Arabic"), ("hindi", "Hindi"), ("farsi", "Farsi"), ("native", "Native")];
            let cur = opts.iter().find(|o| o.0 == digits).map_or("Default", |o| o.1);
            egui::ComboBox::from_id_salt("char_digits")
                .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, cur)))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (k, l) in opts {
                        if ui.selectable_label(k == digits, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                            let _ = app.run("type.char", json!({"attrs": {"digits": k}}));
                        }
                    }
                });
        });
    }
    egui::Grid::new("chargrid").num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Size"));
        if let Some(v) = number(ui, "pcs", c["size"].as_f64(), " pt", 60.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"size": v}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "Leading"));
        let auto = c["leading"]["kind"].as_str() != Some("points");
        let lv = if auto { c["size"].as_f64().map(|s| s * a["para"]["autoLeading"].as_f64().unwrap_or(1.2)) } else { c["leading"]["value"].as_f64() };
        if let Some(v) = number(ui, "pcl", lv, if auto { crate::i18n::tr(&app.ui.language, " (auto)") } else { " pt" }, 70.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Tracking"));
        if let Some(v) = number(ui, "pct", c["tracking"].as_f64(), "", 60.0, 0) {
            let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "Baseline"));
        if let Some(v) = number(ui, "pcb", c["baselineShift"].as_f64(), " pt", 70.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"baselineShift": v}}));
        }
        ui.end_row();
    });
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Color"));
        super::super::swatch_picker(app, ui, "tfill", c["fill"].as_str().map(str::to_string), |app, n| {
            let _ = app.run("type.char", json!({"attrs": {"fill": n}}));
        });
        crate::rtl::label(ui, c["fill"].as_str().unwrap_or(""));
    });
    ui.horizontal(|ui| {
        let caps = c["capitalization"].as_str() == Some("allCaps");
        if ui
            .selectable_label(caps, "TT")
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "All Caps"));
            })
            .clicked()
        {
            let _ = app.run("type.char", json!({"attrs": {"capitalization": if caps { "normal" } else { "allCaps" }}}));
        }
        let ul = c["underline"].as_bool().unwrap_or(false);
        if ui
            .selectable_label(ul, "U")
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Underline"));
            })
            .clicked()
        {
            let _ = app.run("type.char", json!({"attrs": {"underline": !ul}}));
        }
        let st = c["strikethrough"].as_bool().unwrap_or(false);
        if ui
            .selectable_label(st, "S")
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Strikethrough"));
            })
            .clicked()
        {
            let _ = app.run("type.char", json!({"attrs": {"strikethrough": !st}}));
        }
        let sup = c["position"].as_str() == Some("superscript");
        if ui
            .selectable_label(sup, "T¹")
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Superscript"));
            })
            .clicked()
        {
            let _ = app.run("type.char", json!({"attrs": {"position": if sup { "normal" } else { "superscript" }}}));
        }
        let features: Vec<String> =
            c["otfFeatures"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        ui.menu_button("OpenType", |ui| open_type_menu(app, ui, &features));
    });
    // Underline / Strikethrough Options.
    for (key, title) in [("underline", "Underline Options"), ("strikethrough", "Strikethrough Options")] {
        if !c[key].as_bool().unwrap_or(false) {
            continue;
        }
        egui::CollapsingHeader::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, title))).id_salt(key).show(ui, |ui| {
            let size = c["size"].as_f64().unwrap_or(12.0);
            let auto_off = if key == "underline" { size * 0.12 } else { size * 0.3 };
            egui::Grid::new((key, "opts")).num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                caption(ui, crate::i18n::tr(&app.ui.language, "Weight"));
                let w = c[format!("{key}Weight")].as_f64().unwrap_or(size / 14.0);
                if let Some(v) = number(ui, &format!("{key}w"), Some(w), " pt", 60.0, 2) {
                    let _ = app.run("type.char", json!({"attrs": {format!("{key}Weight"): v.max(0.0)}}));
                }
                caption(ui, crate::i18n::tr(&app.ui.language, "Offset"));
                let o = c[format!("{key}Offset")].as_f64().unwrap_or(auto_off);
                if let Some(v) = number(ui, &format!("{key}o"), Some(o), " pt", 60.0, 2) {
                    let _ = app.run("type.char", json!({"attrs": {format!("{key}Offset"): v}}));
                }
                ui.end_row();
            });
            ui.horizontal(|ui| {
                caption(ui, crate::i18n::tr(&app.ui.language, "Color"));
                let cur = c[format!("{key}Color")].as_str().filter(|s| !s.is_empty()).map(str::to_string);
                super::super::swatch_picker(app, ui, &format!("{key}c"), cur.clone(), |app, n| {
                    let _ = app.run("type.char", json!({"attrs": {format!("{key}Color"): n}}));
                });
                crate::rtl::label(ui, cur.unwrap_or_else(|| crate::i18n::tr(&app.ui.language, "(Text Color)").into()));
            });
        });
    }
}

/// The Character panel's OpenType menu: feature toggles, figure styles and stylistic sets.
pub fn open_type_menu(app: &mut DesignApp, ui: &mut egui::Ui, features: &[String]) {
    use designcraft_doc::otf;
    ui.set_min_width(220.0);
    for (tag, label, _) in otf::TOGGLES {
        let mut on = otf::is_on(features, tag);
        if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
            let _ = app.run("type.openType", json!({"feature": tag, "on": on}));
            ui.close();
        }
    }
    ui.separator();
    let fig = otf::figures(features);
    for (id, label, ..) in otf::FIGURES {
        if ui.radio(fig == *id, *label).clicked() {
            let _ = app.run("type.openType", json!({"figures": id}));
            ui.close();
        }
    }
    ui.separator();
    ui.menu_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Stylistic Sets")), |ui| {
        let mask = otf::stylistic_sets(features);
        for n in 1..=20u32 {
            let mut on = mask & (1 << (n - 1)) != 0;
            if ui.checkbox(&mut on, crate::rtl::widget(ui, format!("{} {n}", crate::i18n::tr(&app.ui.language, "Set")))).clicked() {
                let sets: Vec<u32> = (1..=20).filter(|k| if *k == n { on } else { mask & (1 << (k - 1)) != 0 }).collect();
                let _ = app.run("type.openType", json!({"stylisticSets": sets}));
            }
        }
    });
}

/// Variable Font sliders: one per axis; the style becomes `Named {tag:value,…}`.
fn variable_font_axes(app: &mut DesignApp, ui: &mut egui::Ui, family: &str, style: &str) {
    if family.is_empty() {
        return;
    }
    let db = super::super::fonts(app);
    let axes = db.axes(family, style);
    if axes.is_empty() {
        return;
    }
    let base = designcraft_fonts::base_style(style).to_string();
    let face = db.face(family, style);
    let cur = |tag: &str, default: f32| face.coords.iter().find(|(t, _)| t == tag.as_bytes()).map_or(default, |c| c.1);
    let mut values: Vec<(String, f32)> = axes.iter().map(|a| (a.0.clone(), cur(&a.0, a.3))).collect();
    let mut changed = false;
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Variable Font")).size(11.0));
    egui::Grid::new("vf_axes").num_columns(2).spacing(vec2(6.0, 2.0)).show(ui, |ui| {
        for (i, (_, name, min, _, max)) in axes.iter().enumerate() {
            caption(ui, name);
            if ui.add(egui::Slider::new(&mut values[i].1, *min..=*max).step_by(1.0)).drag_stopped() {
                changed = true;
            }
            ui.end_row();
        }
    });
    if changed {
        let spec = values.iter().map(|(t, v)| format!("{t}:{}", v.round())).collect::<Vec<_>>().join(",");
        let _ = app.run("type.char", json!({"attrs": {"fontStyle": format!("{base} {{{spec}}}")}}));
    }
}
