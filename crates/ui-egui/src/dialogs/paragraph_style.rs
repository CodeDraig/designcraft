//! Paragraph style option controls.

use super::{DesignApp, Dialog, Value, json, semibold, text_field};

/// Paragraph Style Options: sections in a left list (like InDesign), fields on the right.
/// Edited values are stored as `p.<attr>` / `c.<attr>` fields and applied on OK.
/// Is `p` a pattern the GREP styles can use? (Checked as the user types.)
pub(super) fn regex_ok(p: &str) -> Result<(), ()> {
    if p.is_empty() {
        return Ok(());
    }
    regex::Regex::new(p).map(|_| ()).map_err(|_| ())
}

pub(super) fn paragraph_style_options(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let name = d.s("name");
    let Some(st) = app.session.active() else { return };
    let Some(style) = st.doc.styles.para(&name).cloned() else {
        ui.label(format!("No style named {name}"));
        return;
    };
    let names: Vec<String> = st.doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| *n != name).collect();
    let (pp, cp) = st.doc.styles.resolve_para_style(&name);
    let units = st.doc.settings.horizontal_units;
    let pv = serde_json::to_value(&pp).unwrap_or_default();
    let cv = serde_json::to_value(&cp).unwrap_or_default();
    let cur = |d: &Dialog, k: &str, base: &Value| d.fields.get(k).cloned().unwrap_or_else(|| base.clone());
    ui.set_min_width(560.0);
    ui.horizontal_top(|ui| {
        // A fixed height: the separator would otherwise stretch the dialog to the window.
        ui.set_min_height(380.0);
        ui.set_max_height(380.0);
        ui.vertical(|ui| {
            ui.set_width(170.0);
            for (id, label) in [
                ("general", "General"),
                ("chars", "Basic Character Formats"),
                ("indents", "Indents and Spacing"),
                ("hyph", "Hyphenation"),
                ("justify", "Justification"),
                ("nested", "Drop Caps and Nested Styles"),
                ("grep", "GREP Style"),
                ("color", "Character Color"),
                ("export", "Export Tagging"),
            ] {
                if ui.selectable_label(d.s("section") == id, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    d.fields.insert("section".into(), json!(id));
                }
            }
        });
        ui.separator();
        let cnames: Vec<String> = app.session.active().map(|s| s.doc.styles.character.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
        ui.vertical(|ui| match d.s("section").as_str() {
            "export" => {
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "EPUB and HTML")).font(semibold(12.0)));
                let et = app.session.active().and_then(|s| s.doc.styles.export_tag(&name, false).cloned()).unwrap_or_default();
                if !d.fields.contains_key("x.tag") {
                    d.fields.insert("x.tag".into(), json!(if et.tag.is_empty() { "[Automatic]" } else { et.tag.as_str() }));
                    d.fields.insert("x.class".into(), json!(et.class));
                }
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Tag:"));
                    let cur = d.s("x.tag");
                    egui::ComboBox::from_id_salt("export_tag")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &cur)))
                        .show_ui(ui, |ui| {
                            for t in ["[Automatic]", "p", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "li", "figcaption", "aside", "div"]
                            {
                                if ui.selectable_label(cur == t, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, t))).clicked() {
                                    d.fields.insert("x.tag".into(), json!(t));
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Class:"));
                    text_field(ui, d, "x.class", 160.0);
                });
            }
            "nested" => {
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Nested Styles")).font(semibold(12.0)));
                let mut list: Vec<Value> = cur(d, "p.nestedStyles", &pv["nestedStyles"]).as_array().cloned().unwrap_or_default();
                let mut changed = false;
                let mut remove = None;
                for (i, ns) in list.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let style = ns["style"].as_str().unwrap_or("[None]").to_string();
                        egui::ComboBox::from_id_salt(("ns_style", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &style)))
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                for c in &cnames {
                                    if ui
                                        .selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, c)))
                                        .clicked()
                                    {
                                        ns["style"] = json!(c);
                                        changed = true;
                                    }
                                }
                            });
                        let through = ns["through"].as_bool().unwrap_or(true);
                        egui::ComboBox::from_id_salt(("ns_thr", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if through { "through" } else { "up to" })))
                            .width(70.0)
                            .show_ui(ui, |ui| {
                                for (v, l) in [(true, "through"), (false, "up to")] {
                                    if ui.selectable_label(v == through, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                                        ns["through"] = json!(v);
                                        changed = true;
                                    }
                                }
                            });
                        let mut n = ns["count"].as_u64().unwrap_or(1) as u32;
                        if ui.add(egui::DragValue::new(&mut n).range(1..=999)).changed() {
                            ns["count"] = json!(n);
                            changed = true;
                        }
                        let kind = ns["until"]["kind"].as_str().unwrap_or("words").to_string();
                        egui::ComboBox::from_id_salt(("ns_until", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &kind)))
                            .width(100.0)
                            .show_ui(ui, |ui| {
                                for k in
                                    ["sentences", "words", "characters", "letters", "digits", "tab", "forcedLineBreak", "emSpace", "enSpace", "chars"]
                                {
                                    if ui.selectable_label(k == kind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, k))).clicked() {
                                        ns["until"] = if k == "chars" { json!({"kind": "chars", "chars": ":"}) } else { json!({"kind": k}) };
                                        changed = true;
                                    }
                                }
                            });
                        if kind == "chars" {
                            let mut c = ns["until"]["chars"].as_str().unwrap_or("").to_string();
                            if ui.add(egui::TextEdit::singleline(&mut c).desired_width(40.0)).changed() {
                                ns["until"]["chars"] = json!(c);
                                changed = true;
                            }
                        }
                        if ui
                            .small_button("×")
                            .on_hover_ui(|ui| {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Delete"));
                            })
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    list.remove(i);
                    changed = true;
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Nested Style"))).clicked() {
                    list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "through": true, "count": 1, "until": {"kind": "words"}}));
                    changed = true;
                }
                if changed {
                    d.fields.insert("p.nestedStyles".into(), Value::Array(list));
                }
                ui.add_space(8.0);
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Nested Line Styles")).font(semibold(12.0)));
                let mut lines: Vec<Value> = cur(d, "p.nestedLineStyles", &pv["nestedLineStyles"]).as_array().cloned().unwrap_or_default();
                let mut changed = false;
                let mut remove = None;
                for (i, l) in lines.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let style = l["style"].as_str().unwrap_or("").to_string();
                        egui::ComboBox::from_id_salt(("nls_style", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &style)))
                            .width(130.0)
                            .show_ui(ui, |ui| {
                                for c in &cnames {
                                    if ui
                                        .selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, c)))
                                        .clicked()
                                    {
                                        l["style"] = json!(c);
                                        changed = true;
                                    }
                                }
                            });
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "for"));
                        let mut n = l["lines"].as_u64().unwrap_or(1) as u32;
                        if ui.add(egui::DragValue::new(&mut n).range(1..=999)).changed() {
                            l["lines"] = json!(n);
                            changed = true;
                        }
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, if n == 1 { "line" } else { "lines" }));
                        if ui
                            .small_button("×")
                            .on_hover_ui(|ui| {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Delete"));
                            })
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    lines.remove(i);
                    changed = true;
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Line Style"))).clicked() {
                    lines.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "lines": 1}));
                    changed = true;
                }
                if changed {
                    d.fields.insert("p.nestedLineStyles".into(), Value::Array(lines));
                }
            }
            "grep" => {
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "GREP Styles")).font(semibold(12.0)));
                let mut list: Vec<Value> = cur(d, "p.grepStyles", &pv["grepStyles"]).as_array().cloned().unwrap_or_default();
                let mut changed = false;
                let mut remove = None;
                for (i, g) in list.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Apply Style:"));
                        let style = g["style"].as_str().unwrap_or("").to_string();
                        egui::ComboBox::from_id_salt(("gs_style", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &style)))
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                for c in &cnames {
                                    if ui
                                        .selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, c)))
                                        .clicked()
                                    {
                                        g["style"] = json!(c);
                                        changed = true;
                                    }
                                }
                            });
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "To Text:"));
                        let mut pat = g["pattern"].as_str().unwrap_or("").to_string();
                        let bad = regex_ok(&pat).is_err();
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut pat)
                                .desired_width(150.0)
                                .text_color_opt(bad.then_some(egui::Color32::from_rgb(240, 90, 90))),
                        );
                        if r.changed() {
                            g["pattern"] = json!(pat);
                            changed = true;
                        }
                        if ui
                            .small_button("×")
                            .on_hover_ui(|ui| {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Delete"));
                            })
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    list.remove(i);
                    changed = true;
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New GREP Style"))).clicked() {
                    list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "pattern": "\\d+"}));
                    changed = true;
                }
                if changed {
                    d.fields.insert("p.grepStyles".into(), Value::Array(list));
                }
            }
            "chars" => {
                let fonts = crate::panels::fonts(app);
                let menu = crate::panels::font_menu(app);
                egui::Grid::new("psc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Family:"));
                    let fam = cur(d, "c.fontFamily", &cv["fontFamily"]).as_str().unwrap_or("").to_string();
                    egui::ComboBox::from_id_salt("psfam").selected_text(crate::panels::font_label(app, &menu, &fam)).width(200.0).show_ui(ui, |ui| {
                        if let Some(f) = crate::panels::font_menu_rows(app, ui, &menu, &fam) {
                            d.fields.insert("c.fontFamily".into(), json!(f));
                        }
                    });
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Style:"));
                    let sty = cur(d, "c.fontStyle", &cv["fontStyle"]).as_str().unwrap_or("").to_string();
                    egui::ComboBox::from_id_salt("pssty").selected_text(&sty).width(200.0).show_ui(ui, |ui| {
                        for s in fonts.styles(&fam) {
                            if ui.selectable_label(s == sty, &s).clicked() {
                                d.fields.insert("c.fontStyle".into(), json!(s));
                            }
                        }
                    });
                    ui.end_row();
                    for (label, key, suffix) in [("Size:", "size", " pt"), ("Tracking:", "tracking", "")] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        let v = cur(d, &format!("c.{key}"), &cv[key]).as_f64();
                        if let Some(n) = crate::widgets::number(ui, &format!("ps{key}"), v, suffix, 80.0, 2) {
                            d.fields.insert(format!("c.{key}"), json!(n));
                        }
                        ui.end_row();
                    }
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Leading:"));
                    let lv = match cur(d, "c.leading", &cv["leading"]) {
                        v if v["kind"] == "points" => v["value"].as_f64(),
                        _ => None,
                    };
                    if let Some(n) = crate::widgets::number(ui, "pslead", lv, " pt", 80.0, 2) {
                        d.fields.insert("c.leading".into(), json!({"kind": "points", "value": n}));
                    }
                    ui.end_row();
                });
            }
            "indents" => {
                egui::Grid::new("psi").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Alignment:"));
                    let a: designcraft_doc::Align = serde_json::from_value(cur(d, "p.align", &pv["align"])).unwrap_or_default();
                    egui::ComboBox::from_id_salt("psalign")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, a.label())))
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for al in designcraft_doc::Align::ALL {
                                if ui.selectable_label(al == a, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, al.label()))).clicked() {
                                    d.fields.insert("p.align".into(), serde_json::to_value(al).unwrap_or_default());
                                }
                            }
                        });
                    ui.end_row();
                    for (label, key) in [
                        ("Left Indent:", "leftIndent"),
                        ("First Line Indent:", "firstLineIndent"),
                        ("Right Indent:", "rightIndent"),
                        ("Space Before:", "spaceBefore"),
                        ("Space After:", "spaceAfter"),
                    ] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        let v = cur(d, &format!("p.{key}"), &pv[key]).as_f64();
                        if let Some(n) = crate::widgets::measure(ui, &format!("ps{key}"), v, units, 80.0) {
                            d.fields.insert(format!("p.{key}"), json!(n));
                        }
                        ui.end_row();
                    }
                });
            }
            "hyph" => {
                let mut h = cur(d, "p.hyphenate", &pv["hyphenate"]).as_bool().unwrap_or(true);
                if ui.checkbox(&mut h, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Hyphenate"))).changed() {
                    d.fields.insert("p.hyphenate".into(), json!(h));
                }
                egui::Grid::new("psh").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    for (label, key) in [
                        ("Words with at Least:", "hyphMinWord"),
                        ("After First:", "hyphAfterFirst"),
                        ("Before Last:", "hyphBeforeLast"),
                        ("Hyphen Limit:", "hyphLimit"),
                    ] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        let v = cur(d, &format!("p.{key}"), &pv[key]).as_f64();
                        if let Some(n) = crate::widgets::number(ui, &format!("ps{key}"), v, "", 60.0, 0) {
                            d.fields.insert(format!("p.{key}"), json!(n.max(0.0) as u64));
                        }
                        ui.end_row();
                    }
                });
            }
            "justify" => {
                egui::Grid::new("psj").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("");
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Minimum"));
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Desired"));
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Maximum"));
                    ui.end_row();
                    for (label, base) in [("Word Spacing:", "wordSpace"), ("Letter Spacing:", "letterSpace"), ("Glyph Scaling:", "glyphScale")] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        for suffix in ["Min", "Desired", "Max"] {
                            let key = format!("{base}{suffix}");
                            let v = cur(d, &format!("p.{key}"), &pv[key.as_str()]).as_f64().map(|x| x * 100.0);
                            if let Some(n) = ui.scope(|ui| crate::widgets::number(ui, &format!("ps{key}"), v, "%", 60.0, 0)).inner {
                                d.fields.insert(format!("p.{key}"), json!(n / 100.0));
                            }
                        }
                        ui.end_row();
                    }
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Composer:"));
                    let single = cur(d, "p.composer", &pv["composer"]).as_str() == Some("singleLine");
                    if ui.selectable_label(!single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Paragraph Composer"))).clicked() {
                        d.fields.insert("p.composer".into(), json!("paragraph"));
                    }
                    if ui.selectable_label(single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Single-line Composer"))).clicked() {
                        d.fields.insert("p.composer".into(), json!("singleLine"));
                    }
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Insert Kashidas:"));
                    let mut k = cur(d, "p.kashidas", &pv["kashidas"]).as_bool().unwrap_or(true);
                    if ui.checkbox(&mut k, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "In justified Arabic text"))).changed() {
                        d.fields.insert("p.kashidas".into(), json!(k));
                    }
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Korean Line Breaks:"));
                    let mut kb = cur(d, "p.koreanCharBreaks", &pv["koreanCharBreaks"]).as_bool().unwrap_or(false);
                    if ui
                        .checkbox(&mut kb, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Between syllables (not only at spaces)")))
                        .changed()
                    {
                        d.fields.insert("p.koreanCharBreaks".into(), json!(kb));
                    }
                    ui.end_row();
                });
            }
            "color" => {
                let cur_fill = cur(d, "c.fill", &cv["fill"]).as_str().unwrap_or("").to_string();
                let swatches: Vec<String> = st.doc.swatches.iter().map(|s| s.name.clone()).collect();
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for sw in swatches {
                        let (c, g) = crate::widgets::swatch_colors(&st.doc, &sw, 1.0);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                            crate::widgets::paint_chip(ui.painter(), r, c, g);
                            if ui.selectable_label(sw == cur_fill, &sw).clicked() {
                                d.fields.insert("c.fill".into(), json!(sw));
                            }
                        });
                    }
                });
            }
            _ => {
                egui::Grid::new("psg").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Style Name:"));
                    if !d.fields.contains_key("rename") {
                        d.fields.insert("rename".into(), json!(name));
                    }
                    let mut rn = d.s("rename");
                    if ui.add(egui::TextEdit::singleline(&mut rn).desired_width(220.0)).changed() {
                        d.fields.insert("rename".into(), json!(rn));
                    }
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Based On:"));
                    let based = d
                        .fields
                        .get("basedOn")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or(style.based_on.clone())
                        .unwrap_or_else(|| "[No Paragraph Style]".into());
                    egui::ComboBox::from_id_salt("psbased")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &based)))
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for n in &names {
                                if ui.selectable_label(*n == based, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                                    d.fields.insert("basedOn".into(), json!(n));
                                }
                            }
                        });
                    ui.end_row();
                });
                ui.add_space(8.0);
                crate::rtl::label(
                    ui,
                    egui::RichText::new(format!(
                        "{} {} {:.1} pt · {}",
                        cp.font_family,
                        cp.font_style,
                        cp.size,
                        crate::i18n::tr(&app.ui.language, pp.align.label())
                    ))
                    .color(crate::theme::Tokens::get(ui.ctx()).text_dim),
                );
            }
        });
    });
}
