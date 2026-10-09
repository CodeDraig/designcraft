//! Buttons panel controls.

use super::{DesignApp, Tokens, json};

/// Buttons and Forms panel: the selected objects' On Release action.
pub fn buttons(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(id) = app.session.active().and_then(|d| d.selection.items.first().copied()) else {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select an object to make it a button.")).size(11.0).color(t.text_dim),
            ),
        );
        return;
    };
    let cur = app.session.active().and_then(|d| d.doc.item(id)).and_then(|it| it.button.clone());
    use designcraft_doc::ButtonAction as B;
    let (kind, page, url) = match &cur {
        None => ("none", 0, String::new()),
        Some(B::GoToPage { page }) => ("page", *page, String::new()),
        Some(B::GoToFirstPage) => ("firstPage", 0, String::new()),
        Some(B::GoToLastPage) => ("lastPage", 0, String::new()),
        Some(B::GoToNextPage) => ("nextPage", 0, String::new()),
        Some(B::GoToPreviousPage) => ("previousPage", 0, String::new()),
        Some(B::GoToUrl { url }) => ("url", 0, url.clone()),
    };
    let labels = [
        ("none", "Not a button"),
        ("nextPage", "Go To Next Page"),
        ("previousPage", "Go To Previous Page"),
        ("firstPage", "Go To First Page"),
        ("lastPage", "Go To Last Page"),
        ("page", "Go To Page"),
        ("url", "Go To URL"),
    ];
    let mut pick = None;
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "On Release"));
        egui::ComboBox::from_id_salt("button_action")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, labels.iter().find(|l| l.0 == kind).map_or("", |l| l.1))))
            .show_ui(ui, |ui| {
                for (k, l) in labels {
                    if ui.selectable_label(k == kind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        pick = Some(k);
                    }
                }
            });
    });
    let mut params = None;
    if let Some(k) = pick {
        params = Some(json!({"action": k, "page": page, "url": if url.is_empty() { "https://" } else { &url }}));
    }
    if kind == "page" {
        let total = app.session.active().map_or(1, |d| d.doc.page_count());
        let mut n = page + 1;
        ui.horizontal(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Page"));
            if ui.add(egui::DragValue::new(&mut n).range(1..=total)).changed() {
                params = Some(json!({"action": "page", "page": n - 1}));
            }
        });
    }
    if kind == "url" {
        let key = egui::Id::new(("button_url", id.0));
        let mut u: String = ui.data(|d| d.get_temp(key)).unwrap_or(url.clone());
        let r = ui.add(egui::TextEdit::singleline(&mut u).desired_width(f32::INFINITY));
        if r.lost_focus() && u != url {
            params = Some(json!({"action": "url", "url": u}));
        }
        ui.data_mut(|d| d.insert_temp(key, u));
    }
    if let Some(p) = params
        && let Err(e) = app.run("button.set", p)
    {
        app.status(format!("Buttons: {e}"));
    }
    crate::rtl::label(
        ui,
        crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Buttons act in interactive PDF export.")).size(10.5).color(t.text_dim),
        ),
    );
    // Form field.
    ui.separator();
    let ff = app.session.active().and_then(|d| d.doc.item(id)).and_then(|it| it.form_field.clone());
    use designcraft_doc::FieldKind as K;
    let kinds = [
        (None, "Not a form field"),
        (Some(K::TextField), "Text Field"),
        (Some(K::CheckBox), "Check Box"),
        (Some(K::ComboBox), "Combo Box"),
        (Some(K::ListBox), "List Box"),
        (Some(K::Signature), "Signature Field"),
    ];
    let cur = ff.as_ref().map(|f| f.kind);
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Form Field"));
        egui::ComboBox::from_id_salt("form_kind")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, kinds.iter().find(|k| k.0 == cur).map_or("", |k| k.1))))
            .show_ui(ui, |ui| {
                for (k, l) in kinds {
                    if ui.selectable_label(k == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        let _ = match k {
                            Some(k) => app.run("form.set", json!({"kind": k})),
                            None => app.run("form.clear", json!({})),
                        };
                    }
                }
            });
    });
    if let Some(f) = ff {
        let key = egui::Id::new(("form_edit", id.0));
        let (mut name, mut value, mut opts): (String, String, String) =
            ui.data(|d| d.get_temp(key)).unwrap_or((f.name.clone(), f.value.clone(), f.options.join(", ")));
        let mut commit = false;
        ui.horizontal(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Name"));
            commit |= ui.text_edit_singleline(&mut name).lost_focus();
        });
        if f.kind == K::CheckBox {
            let mut on = !f.value.is_empty() && f.value != "Off";
            if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Checked by default"))).changed() {
                let _ = app.run("form.set", json!({"kind": f.kind, "value": if on { "On" } else { "" }}));
            }
        } else if f.kind != K::Signature {
            ui.horizontal(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Default"));
                commit |= ui.text_edit_singleline(&mut value).lost_focus();
            });
        }
        if matches!(f.kind, K::ComboBox | K::ListBox) {
            ui.horizontal(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Choices"));
                commit |= ui
                    .text_edit_singleline(&mut opts)
                    .on_hover_ui(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Separated by commas"));
                    })
                    .lost_focus();
            });
        }
        let mut req = f.required;
        if ui.checkbox(&mut req, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Required"))).changed() {
            let _ = app.run("form.set", json!({"kind": f.kind, "required": req}));
        }
        if f.kind == K::TextField {
            let mut ml = f.multiline;
            if ui.checkbox(&mut ml, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Multiline"))).changed() {
                let _ = app.run("form.set", json!({"kind": f.kind, "multiline": ml}));
            }
        }
        if commit {
            let options: Vec<String> = opts.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
            let _ = app.run("form.set", json!({"kind": f.kind, "name": name, "value": value, "options": options}));
            ui.data_mut(|d| d.remove::<(String, String, String)>(key));
        } else {
            ui.data_mut(|d| d.insert_temp(key, (name, value, opts)));
        }
    }
}
