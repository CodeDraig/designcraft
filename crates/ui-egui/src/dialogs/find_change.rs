//! findChange dialog controls.

use super::{DesignApp, Dialog, Value, check, json, text_field};

pub(super) fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for (label, grep) in [("Text", false), ("GREP", true)] {
            if ui
                .selectable_label(!d.b("objectMode") && d.b("grep") == grep, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label)))
                .clicked()
            {
                d.fields.insert("grep".into(), json!(grep));
                d.fields.insert("objectMode".into(), json!(false));
            }
        }
        if ui.selectable_label(d.b("objectMode"), crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Object"))).clicked() {
            d.fields.insert("objectMode".into(), json!(true));
        }
    });
    if d.b("objectMode") {
        // Find/Change › Object: objects by fill and kind; change their fill or opacity.
        let swatches: Vec<String> =
            app.session.active().map(|st| st.doc.swatches.iter().filter(|w| !w.hidden).map(|w| w.name.clone()).collect()).unwrap_or_default();
        egui::Grid::new("fco").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            for (key, label, any) in [("objFill", "Find fill:", "(any)"), ("objChangeFill", "Change fill to:", "(unchanged)")] {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                let cur = d.s(key);
                egui::ComboBox::from_id_salt(key).selected_text(if cur.is_empty() { any } else { cur.as_str() }).width(180.0).show_ui(ui, |ui| {
                    if ui.selectable_label(cur.is_empty(), crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, any))).clicked() {
                        d.fields.insert(key.into(), json!(""));
                    }
                    for w in &swatches {
                        if ui.selectable_label(*w == cur, w).clicked() {
                            d.fields.insert(key.into(), json!(w));
                        }
                    }
                });
                ui.end_row();
            }
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Kind:"));
            let cur = d.s("objKind");
            egui::ComboBox::from_id_salt("objKind").selected_text(if cur.is_empty() { "(any)" } else { cur.as_str() }).show_ui(ui, |ui| {
                for v in ["", "text", "graphic", "shape", "line", "group"] {
                    if ui
                        .selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if v.is_empty() { "(any)" } else { v })))
                        .clicked()
                    {
                        d.fields.insert("objKind".into(), json!(v));
                    }
                }
            });
            ui.end_row();
        });
        let mut crit = json!({});
        if !d.s("objFill").is_empty() {
            crit["fill"] = json!(d.s("objFill"));
        }
        if !d.s("objKind").is_empty() {
            crit["kind"] = json!(d.s("objKind"));
        }
        ui.horizontal(|ui| {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Find All"))).clicked() {
                let n = app.run("find.objects", crit.clone()).ok().and_then(|r| r["ids"].as_array().map(Vec::len)).unwrap_or(0);
                d.fields.insert("status".into(), json!(format!("{n} found")));
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Change All"))).clicked() && !d.s("objChangeFill").is_empty() {
                let mut p = crit.clone();
                p["change"] = json!({"fill": d.s("objChangeFill")});
                let n = app.run("find.changeObjects", p).ok().and_then(|r| r["changed"].as_u64()).unwrap_or(0);
                d.fields.insert("status".into(), json!(format!("{n} changed")));
            }
        });
        ui.label(d.s("status"));
    } else {
        egui::Grid::new("fc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Find what:"));
            text_field(ui, d, "find", 260.0);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Change to:"));
            text_field(ui, d, "change", 260.0);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Search:"));
            let cur = d.s("scope");
            egui::ComboBox::from_id_salt("fcscope").selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &cur))).show_ui(
                ui,
                |ui| {
                    for v in ["document", "story", "selection"] {
                        if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, v))).clicked() {
                            d.fields.insert("scope".into(), json!(v));
                        }
                    }
                },
            );
            ui.end_row();
        });
        ui.horizontal(|ui| {
            check(ui, d, "caseSensitive", crate::i18n::tr(&app.ui.language, "Case sensitive"));
            check(ui, d, "wholeWord", crate::i18n::tr(&app.ui.language, "Whole word"));
        });
        ui.horizontal(|ui| {
        let params0 = json!({"find": d.s("find"), "change": d.s("change"), "grep": d.b("grep"), "caseSensitive": d.b("caseSensitive"), "wholeWord": d.b("wholeWord"), "scope": d.s("scope")});
        let params = || params0.clone();
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Find Next"))).clicked() {
            let r = app.run("find.next", params());
            let msg = match r {
                Ok(Value::Null) => "No matches".to_string(),
                Ok(_) => "Found".to_string(),
                Err(e) => e,
            };
            d.fields.insert("status".into(), json!(msg));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Change All"))).clicked() {
            let msg = match app.run("find.change", params()) {
                Ok(v) => format!("{} replacement(s) made", v["count"]),
                Err(e) => e,
            };
            d.fields.insert("status".into(), json!(msg));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Count"))).clicked() {
            let msg = match app.run("find.find", params()) {
                Ok(v) => format!("{} match(es)", v.as_array().map(|a| a.len()).unwrap_or(0)),
                Err(e) => e,
            };
            d.fields.insert("status".into(), json!(msg));
        }
    });
        if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
            ui.label(egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
        }
    }
}
