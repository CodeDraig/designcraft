//! Modal dialogs. Each dialog keeps its fields in a JSON map so the control channel can set them
//! (`ui.dialog.set {field, value}`) and confirm them (`ui.dialog.confirm`) like a user would.

mod color_settings;
mod dictionary;
mod find_change;
mod fitting;
mod import_options;
mod new_document;
pub(crate) mod publication;
mod publication_editions;
mod publication_indexes;
mod publication_layout;
mod publication_notes;
mod publication_references;
mod publication_review;
mod publication_settings;
mod qr_code;
pub(crate) mod tei;
use dictionary::user_dictionary;
mod shortcuts;
use shortcuts::keyboard_shortcuts;
mod printing;
pub use printing::open_print;
use printing::print_dialog;
mod document_setup;
use document_setup::document_setup;
pub use document_setup::open_document_setup;
mod preferences;
use preferences::preferences;
mod paragraph_style;
use paragraph_style::paragraph_style_options;
mod footnotes;
use footnotes::footnote_options;
mod command_form;
use command_form::command_form;
pub use command_form::{CommandField, command_fields};
mod references;
use references::insert_xref;
mod color_picker;
use color_picker::color_picker;
mod fonts;
use fonts::find_font;

use designcraft_geom::{Unit, format_measure, units::parse_measure};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::DesignApp;
use crate::theme::semibold;

#[derive(Clone, Debug, Serialize)]
pub struct Dialog {
    pub id: String,
    pub fields: Map<String, Value>,
}

impl Dialog {
    pub fn new(id: &str, params: Value) -> Self {
        let mut fields = match params {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let defaults = match id {
            "newDocument" => {
                json!({"preset": "Letter", "width": "51p0", "height": "66p0", "pages": 1, "facingPages": true, "columns": 1, "gutter": "1p0",
                "marginTop": "3p0", "marginBottom": "3p0", "marginInside": "3p0", "marginOutside": "3p0", "bleed": "0p0", "primaryTextFrame": false})
            }
            "goToPage" => json!({"page": "1"}),
            "qrCode" => json!({"type": "url", "content": "https://", "color": "[Black]"}),
            "insertTable" => json!({"bodyRows": 4, "columns": 4, "headerRows": 0, "footerRows": 0}),
            "insertXref" => json!({"linkTo": "paragraph", "style": "", "target": "", "format": ""}),
            "findChange" => json!({"find": "", "change": "", "grep": false, "caseSensitive": false, "wholeWord": false, "scope": "document"}),
            "textFrameOptions" => json!({"columns": 1, "gutter": "1p0", "inset": "0p0", "verticalJustification": "top"}),
            "documentSetup" => json!({}),
            _ => json!({}),
        };
        if let Value::Object(d) = defaults {
            for (k, v) in d {
                fields.entry(k).or_insert(v);
            }
        }
        if id == "footnoteOptions" {
            // Flatten the rule and show special characters as InDesign metacharacters.
            if let Some(Value::Object(r)) = fields.remove("rule") {
                for (k, v) in r {
                    fields.insert(format!("rule.{k}"), v);
                }
            }
            let sep = fields.get("separator").and_then(Value::as_str).unwrap_or("").to_string();
            fields.insert("separator".into(), json!(sep.replace('\t', "^t").replace('\u{2003}', "^m").replace('\u{2002}', "^>")));
            fields.entry("tab".to_string()).or_insert(json!("numbering"));
        }
        if id == "paragraphStyleOptions" {
            fields.entry("section".to_string()).or_insert(json!("general"));
        }
        if id == "frameSize" {
            for k in ["width", "height"] {
                if let Some(v) = fields.get(k).and_then(Value::as_f64) {
                    fields.insert(k.into(), json!(format_measure(v, Unit::Picas)));
                }
            }
        }
        Dialog { id: id.into(), fields }
    }
    fn s(&self, k: &str) -> String {
        match self.fields.get(k) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        }
    }
    fn m(&self, k: &str) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => parse_measure(s, Unit::Picas).ok(),
            _ => None,
        }
    }
    /// A measure in points ("6", "6 pt", "0p6", "2 mm").
    fn pt(&self, k: &str) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => parse_measure(s, Unit::Points).ok(),
            _ => None,
        }
    }
    fn b(&self, k: &str) -> bool {
        self.fields.get(k).and_then(Value::as_bool).unwrap_or(false)
    }
    fn n(&self, k: &str) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

fn text_field(ui: &mut egui::Ui, d: &mut Dialog, key: &str, w: f32) {
    let mut s = d.s(key);
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(w).horizontal_align(egui::Align::Min)).changed() {
        d.fields.insert(key.into(), Value::String(s));
    }
}

/// An `[r, g, b]` field edited with a colour button.
fn color_field(ui: &mut egui::Ui, d: &mut Dialog, key: &str) {
    let v: [u8; 3] = d.fields.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or([128, 128, 128]);
    let mut c = v;
    if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() {
        d.fields.insert(key.into(), json!(c));
    }
}

fn check(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str) {
    let mut b = d.b(key);
    if ui.checkbox(&mut b, crate::rtl::widget(ui, label)).changed() {
        d.fields.insert(key.into(), Value::Bool(b));
    }
}

pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(mut d) = app.ui.dialog.clone() else { return };
    let mut result: Option<bool> = None;
    let title = match d.id.as_str() {
        "closeDocument" => crate::i18n::tr(&app.ui.language, "Save Changes"),
        "ruby" => crate::i18n::tr(&app.ui.language, "Ruby"),
        "newDocument" => crate::i18n::tr(&app.ui.language, "New Document"),
        "frameSize" => crate::i18n::tr(&app.ui.language, "Rectangle"),
        "goToPage" => crate::i18n::tr(&app.ui.language, "Go to Page"),
        "insertTable" => crate::i18n::tr(&app.ui.language, "Create Table"),
        "textFrameOptions" => crate::i18n::tr(&app.ui.language, "Text Frame Options"),
        "documentSetup" => crate::i18n::tr(&app.ui.language, "Document Setup"),
        "findChange" => crate::i18n::tr(&app.ui.language, "Find/Change"),
        "paragraphStyleOptions" => crate::i18n::tr(&app.ui.language, "Paragraph Style Options"),
        "footnoteOptions" => crate::i18n::tr(&app.ui.language, "Footnote Options"),
        "insertXref" => crate::i18n::tr(&app.ui.language, "New Cross-Reference"),
        "findFont" => crate::i18n::tr(&app.ui.language, "Find/Replace Font"),
        "polygonSettings" => crate::i18n::tr(&app.ui.language, "Polygon Settings"),
        "userDictionary" => crate::i18n::tr(&app.ui.language, "User Dictionary"),
        "newWorkspace" => crate::i18n::tr(&app.ui.language, "New Workspace"),
        "menus" => crate::i18n::tr(&app.ui.language, "Menu Customization"),
        "teiImport" => "Import TEI Book",
        "teiReport" => "TEI Import Report",
        "publication" => "Publication",
        "importOptions" => crate::i18n::tr(&app.ui.language, "Import Options"),
        "fittingOptions" => crate::i18n::tr(&app.ui.language, "Frame Fitting Options"),
        "qrCode" => crate::i18n::tr(&app.ui.language, "Generate QR Code"),
        "preferences" => crate::i18n::tr(&app.ui.language, "Preferences"),
        "print" => crate::i18n::tr(&app.ui.language, "Print"),
        "pdfImport" => crate::i18n::tr(&app.ui.language, "Place PDF"),
        "colorSettings" => crate::i18n::tr(&app.ui.language, "Color Settings"),
        "layerOptions" => crate::i18n::tr(&app.ui.language, "Object Layer Options"),
        "keyboardShortcuts" => crate::i18n::tr(&app.ui.language, "Keyboard Shortcuts"),
        "colorPicker" => crate::i18n::tr(&app.ui.language, "Color Picker"),
        id => match id.strip_prefix("cmd:").and_then(designcraft_engine::find_command) {
            Some(c) => crate::i18n::tr(&app.ui.language, c.label.trim_end_matches('…')),
            None => crate::i18n::tr(&app.ui.language, "Dialog"),
        },
    };
    egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
        ui.set_min_width(if matches!(d.id.as_str(), "teiImport" | "teiReport") { 560.0 } else { 380.0 });
        ui.set_max_width(if d.id == "newDocument" && crate::i18n::is_rtl(&app.ui.language) { 380.0 } else { 640.0 });
        if crate::i18n::is_rtl(&app.ui.language) {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::rtl::label(ui, egui::RichText::new(title).font(semibold(16.0)));
                });
            });
        } else {
            crate::rtl::label(ui, egui::RichText::new(title).font(semibold(16.0)));
        }
        ui.add_space(10.0);
        match d.id.as_str() {
            "publication" => publication::show(ui, &mut d),
            "closeDocument" => {
                crate::rtl::label(ui, format!("{} “{}”?", crate::i18n::tr(&app.ui.language, "Save changes to"), d.s("title")));
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Discarding will lose the unsaved changes."));
            }
            "newDocument" => new_document::show(app, ui, &mut d),
            "frameSize" => {
                egui::Grid::new("fs").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Width"));
                    text_field(ui, &mut d, "width", 90.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Height"));
                    text_field(ui, &mut d, "height", 90.0);
                    ui.end_row();
                });
            }
            "findChange" => find_change::show(app, ui, &mut d),
            "paragraphStyleOptions" => paragraph_style_options(app, ui, &mut d),
            "footnoteOptions" => footnote_options(app, ui, &mut d),
            "insertXref" => insert_xref(app, ui, &mut d),
            "findFont" => find_font(app, ui, &mut d),
            "colorPicker" => color_picker(ui, &mut d),
            "preferences" => preferences(app, ui, &mut d),
            "print" => print_dialog(app, ui, &mut d),
            "pdfImport" => {
                ui.label(egui::RichText::new(d.s("path")).size(11.0));
                ui.horizontal(|ui| {
                    crate::rtl::label(
                        ui,
                        crate::i18n::tr(&app.ui.language, "Page (1–{count}):").replace("{count}", &d.n("pages").unwrap_or(1.0).to_string()),
                    );
                    text_field(ui, &mut d, "page", 60.0);
                });
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Crop to:"));
                    let cur = d.s("crop");
                    let label = |v: &str| match v {
                        "trim" => crate::i18n::tr(&app.ui.language, "Trim"),
                        "bleed" => crate::i18n::tr(&app.ui.language, "Bleed"),
                        "art" => crate::i18n::tr(&app.ui.language, "Art"),
                        "media" => crate::i18n::tr(&app.ui.language, "Media"),
                        _ => crate::i18n::tr(&app.ui.language, "Crop"),
                    };
                    egui::ComboBox::from_id_salt("pdf_crop").selected_text(crate::rtl::widget(ui, label(&cur))).show_ui(ui, |ui| {
                        for v in ["crop", "trim", "bleed", "art", "media"] {
                            if ui.selectable_label(label(&cur) == label(v), label(v)).clicked() {
                                d.fields.insert("crop".into(), json!(v));
                            }
                        }
                    });
                });
            }
            "layerOptions" => {
                let mut layers = d.fields.get("layers").and_then(Value::as_array).cloned().unwrap_or_default();
                if layers.is_empty() {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "This PDF has no layers."));
                }
                for l in &mut layers {
                    let mut on = l["visible"].as_bool().unwrap_or(true);
                    if ui.checkbox(&mut on, l["name"].as_str().unwrap_or("")).changed() {
                        l["visible"] = json!(on);
                    }
                }
                d.fields.insert("layers".into(), json!(layers));
            }
            "colorSettings" => color_settings::show(app, ui, &mut d),
            "keyboardShortcuts" => keyboard_shortcuts(app, ui, &mut d),
            "userDictionary" => user_dictionary(app, ui, &mut d),
            "menus" => {
                text_field(ui, &mut d, "query", 260.0);
                let q = d.s("query").to_lowercase();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (menu, label) in crate::menus::all_menu_items()
                        .into_iter()
                        .filter(|(m, l)| q.is_empty() || m.to_lowercase().contains(&q) || l.to_lowercase().contains(&q))
                    {
                        let key = crate::menus::menu_key(&menu, &label);
                        let mut visible = !app.ui.hidden_menu_items.contains(&key);
                        if ui
                            .checkbox(
                                &mut visible,
                                crate::rtl::widget(
                                    ui,
                                    format!("{} › {}", crate::i18n::tr(&app.ui.language, &menu), crate::i18n::tr(&app.ui.language, &label)),
                                ),
                            )
                            .changed()
                        {
                            let _ = app.run("window.hideMenuItem", json!({"item": key, "hidden": !visible}));
                        }
                    }
                });
            }
            "importOptions" => import_options::show(app, ui, &mut d),
            "teiImport" => tei::show(app, ui, &mut d),
            "teiReport" => tei::report(app, ui, &d),
            "newWorkspace" => {
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Name:"));
                    text_field(ui, &mut d, "name", 220.0);
                });
                crate::rtl::label(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Saves which bars show and where the panels are.")).size(10.5),
                );
            }
            "fittingOptions" => fitting::show(app, ui, &mut d),
            "qrCode" => qr_code::show(app, ui, &mut d),
            "polygonSettings" => {
                egui::Grid::new("poly").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Number of Sides:"));
                    text_field(ui, &mut d, "sides", 60.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Star Inset:"));
                    ui.horizontal(|ui| {
                        text_field(ui, &mut d, "starInset", 60.0);
                        ui.label("%");
                    });
                    ui.end_row();
                });
            }
            id if id.starts_with("cmd:") => command_form(app, ui, &mut d),
            "goToPage" => {
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Page"));
                    text_field(ui, &mut d, "page", 80.0);
                });
            }
            "ruby" => {
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Ruby:"));
                    text_field(ui, &mut d, "text", 200.0);
                });
                crate::rtl::label(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Set over the selected text; empty removes it.")).size(11.0),
                );
            }
            "insertTable" => {
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Table Dimensions")).font(semibold(12.0)));
                egui::Grid::new("ins_table").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Body Rows"));
                    text_field(ui, &mut d, "bodyRows", 60.0);
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Columns"));
                    text_field(ui, &mut d, "columns", 60.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Header Rows"));
                    text_field(ui, &mut d, "headerRows", 60.0);
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Footer Rows"));
                    text_field(ui, &mut d, "footerRows", 60.0);
                    ui.end_row();
                });
            }
            "textFrameOptions" => {
                egui::Grid::new("tfo").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Number of columns"));
                    text_field(ui, &mut d, "columns", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Gutter"));
                    text_field(ui, &mut d, "gutter", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Inset spacing"));
                    text_field(ui, &mut d, "inset", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Vertical justification"));
                    let cur = d.s("verticalJustification");
                    egui::ComboBox::from_id_salt("vj").selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &cur))).show_ui(
                        ui,
                        |ui| {
                            for v in ["top", "center", "bottom", "justify"] {
                                if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, v))).clicked() {
                                    d.fields.insert("verticalJustification".into(), json!(v));
                                }
                            }
                        },
                    );
                    ui.end_row();
                });
            }
            "documentSetup" => document_setup(app, ui, &mut d),
            _ => {}
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, if d.id == "closeDocument" { "Save" } else { "  OK  " }))
                                .color(egui::Color32::WHITE),
                        ))
                        .fill(crate::theme::Tokens::get(ui.ctx()).accent_strong),
                    )
                    .clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    if d.id == "closeDocument" {
                        d.fields.insert("action".into(), json!("save"));
                    }
                    result = Some(true);
                }
                if d.id == "closeDocument" && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Discard"))).clicked() {
                    d.fields.insert("action".into(), json!("discard"));
                    result = Some(true);
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Cancel"))).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    result = Some(false);
                }
            });
        });
    });
    app.ui.dialog = Some(d);
    match result {
        Some(true) => {
            let _ = confirm(app);
        }
        Some(false) => app.ui.dialog = None,
        None => {}
    }
}

/// Apply the open dialog.
pub fn confirm(app: &mut DesignApp) -> Result<Value, String> {
    let Some(d) = app.ui.dialog.take() else { return Err("no dialog open".into()) };
    match d.id.as_str() {
        "publication" => publication::confirm(app, &d),
        "teiImport" => tei::confirm(app, &d),
        "teiReport" => Ok(Value::Null),
        "closeDocument" => {
            let result = (|| {
                let uid = d.fields.get("uid").and_then(Value::as_u64).ok_or("missing document identity")?;
                // Indices can move while a dialog is open; its decision belongs to this document.
                let index = app.session.documents().iter().position(|st| st.uid == uid).ok_or("the document is no longer open")?;
                match d.s("action").as_str() {
                    "cancel" => Ok(Value::Null),
                    "discard" => app.run("file.close", json!({"index": index, "discard": true})),
                    "save" => {
                        app.run("file.activate", json!({"index": index}))?;
                        if app.session.doc().map_err(|e| e.to_string())?.is_dirty() {
                            app.run("app.save", json!({}))?;
                        }
                        // A cancelled picker returns successfully without saving.
                        if app.session.doc().map_err(|e| e.to_string())?.is_dirty() {
                            return Err("Save was cancelled; the document is still open.".into());
                        }
                        app.run("file.close", json!({"index": index}))
                    }
                    _ => Err("choose save, discard or cancel".into()),
                }
            })();
            if let Err(e) = &result {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            result
        }
        "newDocument" => {
            let margins = json!({"top": d.m("marginTop").unwrap_or(36.0), "bottom": d.m("marginBottom").unwrap_or(36.0), "inside": d.m("marginInside").unwrap_or(36.0), "outside": d.m("marginOutside").unwrap_or(36.0)});
            app.run(
                "file.new",
                json!({"width": d.m("width"), "height": d.m("height"), "pages": d.n("pages").unwrap_or(1.0) as u64, "facingPages": d.b("facingPages"),
                    "columns": d.n("columns").unwrap_or(1.0) as u64, "gutter": d.m("gutter").unwrap_or(12.0), "margins": margins, "bleed": d.m("bleed").unwrap_or(0.0),
                    "primaryTextFrame": d.b("primaryTextFrame")}),
            )
        }
        "frameSize" => {
            let x = d.fields.get("x").and_then(Value::as_f64).unwrap_or(0.0);
            let y = d.fields.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            let w = d.m("width").unwrap_or(72.0);
            let h = d.m("height").unwrap_or(72.0);
            app.run(
                "frame.create",
                json!({"spread": d.fields.get("spread").cloned().unwrap_or(json!(0)), "shape": d.s("shape"), "content": d.s("content"), "rect": [x, y, x + w, y + h]}),
            )
        }
        "ruby" => app.run("type.ruby", json!({"text": d.s("text")})),
        "goToPage" => {
            // A page name ("iv", "A-3"), a number, or "+n" for an absolute position.
            let typed = match d.fields.get("page") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => "1".into(),
            };
            match app.session.resolve_page(&typed) {
                Some(abs) => {
                    crate::canvas::go_to_page(app, abs);
                    Ok(Value::Null)
                }
                None => Err(format!("no page \"{typed}\"")),
            }
        }
        "insertTable" => app.run(
            "table.insert",
            json!({"rows": d.n("bodyRows").unwrap_or(4.0).max(1.0) as u64, "cols": d.n("columns").unwrap_or(4.0).max(1.0) as u64,
                "headerRows": d.n("headerRows").unwrap_or(0.0).max(0.0) as u64, "footerRows": d.n("footerRows").unwrap_or(0.0).max(0.0) as u64}),
        ),
        "textFrameOptions" => app.run(
            "object.textFrameOptions",
            json!({"columns": d.n("columns").unwrap_or(1.0) as u64, "gutter": d.m("gutter").unwrap_or(12.0), "inset": d.m("inset").unwrap_or(0.0), "verticalJustification": d.s("verticalJustification")}),
        ),
        "documentSetup" => {
            let edges = |k: &str| json!(["Top", "Bottom", "Inside", "Outside"].map(|e| d.m(&format!("{k}{e}")).unwrap_or(0.0)));
            app.run(
                "layout.documentSetup",
                json!({"intent": d.s("intent"), "pages": d.n("pages").unwrap_or(1.0).max(1.0) as u64, "startPage": d.n("startPage").unwrap_or(1.0).max(1.0) as u64,
                    "facingPages": d.b("facingPages"), "binding": d.s("binding"), "width": d.m("width"), "height": d.m("height"), "bleed": edges("bleed"), "slug": edges("slug"), "adjustLayout": d.b("adjustLayout")}),
            )
        }
        "paragraphStyleOptions" => {
            let name = d.s("name");
            let mut para = serde_json::Map::new();
            let mut chars = serde_json::Map::new();
            for (k, v) in &d.fields {
                if let Some(a) = k.strip_prefix("p.") {
                    para.insert(a.into(), v.clone());
                } else if let Some(a) = k.strip_prefix("c.") {
                    chars.insert(a.into(), v.clone());
                }
            }
            let mut params = json!({"name": name, "para": para, "chars": chars});
            let based = d.s("basedOn");
            if !based.is_empty() {
                params["basedOn"] = if based == "[No Paragraph Style]" { Value::Null } else { json!(based) };
            }
            let rename = d.s("rename");
            if !rename.is_empty() && rename != name {
                params["rename"] = json!(rename);
            }
            let r = app.run("style.paragraph.edit", params)?;
            if d.fields.contains_key("x.tag") {
                let tag = d.s("x.tag");
                let tag = if tag == "[Automatic]" { String::new() } else { tag };
                let style = if !rename.is_empty() && rename != name { rename } else { name };
                app.run("style.exportTag", json!({"style": style, "tag": tag, "class": d.s("x.class")}))?;
            }
            Ok(r)
        }
        "colorPicker" => {
            let hex = d.s("hex");
            app.run("object.color", json!({"color": hex, "target": d.s("target")}))
        }
        "layerOptions" => {
            let hidden: Vec<Value> = d.fields.get("layers").and_then(Value::as_array).map(|a| a.iter().filter(|l| l["visible"] == false).map(|l| l["name"].clone()).collect()).unwrap_or_default();
            app.run("object.layerOptions", json!({"hidden": hidden}))
        }
        "colorSettings" => {
            let r = app.run("color.settings", json!({"rgb": d.s("rgb"), "cmyk": d.s("cmyk"), "intent": d.s("intent"), "bpc": d.b("bpc")}))?;
            app.ui.color_settings = Some(designcraft_color::cms::active_settings());
            app.canvas.shown = None;
            Ok(r)
        }
        "pdfImport" => {
            let crop = d.s("crop");
            app.run("file.place", json!({"path": d.s("path"), "pdfPage": d.n("page").unwrap_or(1.0).max(1.0) as u64, "pdfCrop": if crop.is_empty() { "crop".to_string() } else { crop }}))
        }
        "print" => {
            let pages = if d.s("range") == "all" { Value::Null } else { json!(d.s("pages")) };
            let printer = d.s("printer");
            let r = app.run(
                "file.print",
                json!({"printer": if printer.is_empty() { Value::Null } else { json!(printer) }, "copies": d.n("copies").unwrap_or(1.0).max(1.0) as u64,
                    "pages": pages, "spreads": d.b("spreads"), "marks": d.b("marks"), "bleed": d.b("bleed")}),
            )?;
            app.status(format!("Sent {} page(s) to {}", r["pages"], r["printer"].as_str().unwrap_or("the default printer")));
            Ok(r)
        }
        "preferences" => preferences::apply(app, &d),
        "newWorkspace" => app.run("window.newWorkspace", json!({"name": d.s("name")})),
        "importOptions" => app.run(
            "file.place",
            json!({"path": d.s("path"), "removeStyles": d.b("removeStyles"), "styleConflicts": d.s("styleConflicts"), "styleMap": d.fields.get("map").cloned().unwrap_or(json!({}))}),
        ),
        "fittingOptions" => {
            let m = |k: &str| d.fields.get(k).and_then(Value::as_str).and_then(|s| parse_measure(s, Unit::Points).ok()).unwrap_or(0.0);
            app.run(
                "object.fittingOptions",
                json!({"autoFit": d.b("autoFit"), "fitting": d.s("fitting"), "align": d.n("align").unwrap_or(4.0) as u64,
                    "crop": [m("cropTop"), m("cropLeft"), m("cropBottom"), m("cropRight")]}),
            )
        }
        "qrCode" => {
            let mut p = Value::Object(d.fields.clone());
            // Nothing selected: a 2-inch code at the top left of the page in view.
            if let Some(st) = app.session.active().filter(|s| s.selection.items.is_empty()) {
                let abs = crate::canvas::current_page(app).unwrap_or(0);
                let (si, pi) = st.doc.page_loc(abs).unwrap_or((0, 0));
                let x = st.doc.spreads.get(si).and_then(|sp| sp.pages.get(pi)).map_or(0.0, |pg| pg.x);
                p["rect"] = json!([x + 36.0, 36.0, x + 180.0, 180.0]);
                p["spread"] = json!(si);
            }
            app.run("object.qrCode", p)
        }
        "polygonSettings" => app.run("tool.polygonSettings", json!({"sides": d.n("sides").unwrap_or(6.0) as u64, "starInset": d.n("starInset").unwrap_or(0.0)})),
        "findFont" => {
            let (f, st) = (d.s("family"), d.s("style"));
            if f.is_empty() || d.s("toFamily").is_empty() {
                return Ok(Value::Null);
            }
            app.run("font.replace", json!({"family": f, "style": st, "toFamily": d.s("toFamily"), "toStyle": d.s("toStyle")}))
        }
        "insertXref" => {
            let format = d.s("format");
            let target = d.s("target");
            let params = match target.split_once(':') {
                Some(("a", id)) => json!({"anchor": id.parse::<u64>().unwrap_or(0), "format": format}),
                Some((sid, pi)) => json!({"story": sid.parse::<u64>().unwrap_or(0), "para": pi.parse::<u64>().unwrap_or(0), "format": format}),
                None => {
                    let mut d = d.clone();
                    d.fields.insert("status".into(), json!("Choose a destination paragraph or text anchor."));
                    app.ui.dialog = Some(d);
                    return Err("no destination chosen".into());
                }
            };
            app.run("xref.insert", params)
        }
        "footnoteOptions" => {
            let pt = |k: &str| d.pt(k).map_or(Value::Null, |v| json!(v));
            let mut p = json!({
                "style": d.s("style"), "startAt": d.n("startAt").unwrap_or(1.0).max(0.0) as u32, "restart": d.s("restart"),
                "prefix": d.s("prefix"), "suffix": d.s("suffix"), "affixIn": d.s("affixIn"), "refPosition": d.s("refPosition"),
                "refCharStyle": d.s("refCharStyle"), "paraStyle": d.s("paraStyle"),
                "separator": d.s("separator").replace("^t", "\t").replace("^m", "\u{2003}").replace("^>", "\u{2002}"),
                "spaceBefore": pt("spaceBefore"), "spaceBetween": pt("spaceBetween"), "firstBaseline": d.s("firstBaseline"),
                "firstBaselineMin": pt("firstBaselineMin"), "spanColumns": d.b("spanColumns"),
                "rule": {"on": d.b("rule.on"), "weight": pt("rule.weight"), "color": d.s("rule.color"), "width": pt("rule.width"),
                    "offset": pt("rule.offset"), "leftIndent": pt("rule.leftIndent")},
            });
            // Unparsable measures keep their current values.
            if let Some(o) = p.as_object_mut() {
                o.retain(|_, v| !v.is_null());
            }
            if let Some(r) = p.get_mut("rule").and_then(Value::as_object_mut) {
                r.retain(|_, v| !v.is_null());
            }
            app.run("footnote.options", p)
        }
        "findChange" if d.b("objectMode") => Ok(Value::Null),
        "findChange" => app.run("find.change", json!({"find": d.s("find"), "change": d.s("change"), "grep": d.b("grep"), "caseSensitive": d.b("caseSensitive"), "wholeWord": d.b("wholeWord"), "scope": d.s("scope")})),
        id if id.starts_with("cmd:") => {
            let cid = &id[4..];
            let doc = designcraft_engine::find_command(cid).map_or("", |c| c.params);
            let mut p = Map::new();
            for f in command_fields(doc) {
                match d.fields.get(&f.key) {
                    Some(Value::String(v)) if !v.trim().is_empty() => {
                        let v = v.trim();
                        // Numbers, booleans, arrays and objects as JSON; anything else is a string.
                        let parsed = serde_json::from_str::<Value>(v).ok().filter(|x| !x.is_string());
                        p.insert(f.key, parsed.unwrap_or_else(|| json!(v)));
                    }
                    Some(Value::Bool(b)) => {
                        p.insert(f.key, json!(b));
                    }
                    _ => {}
                }
            }
            let r = app.run(cid, Value::Object(p));
            if let Err(e) = &r {
                // Keep the dialog open with the error.
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        other => Err(format!("unknown dialog {other}")),
    }
}

fn combo(ui: &mut egui::Ui, d: &mut Dialog, key: &str, opts: &[(&str, &str)]) {
    let cur = d.s(key);
    let shown = opts.iter().find(|o| o.0 == cur).map_or(cur.as_str(), |o| o.1).to_string();
    egui::ComboBox::from_id_salt(key).selected_text(crate::rtl::widget(ui, &shown)).width(170.0).show_ui(ui, |ui| {
        for (v, label) in opts {
            if ui.selectable_label(cur == *v, crate::rtl::widget(ui, *label)).clicked() {
                d.fields.insert(key.into(), json!(v));
            }
        }
    });
}

/// Translate enum captions without changing serialized values or user-defined names.
fn translated_combo(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, key: &str, opts: &[(&str, &str)]) {
    let opts: Vec<_> = opts.iter().map(|(value, label)| (*value, crate::i18n::tr(lang, label))).collect();
    combo(ui, d, key, &opts);
}

/// Style name picker (paragraph or character styles of the active document).
fn style_combo(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, key: &str, character: bool) {
    let names: Vec<String> = app
        .session
        .active()
        .map(|st| {
            if character {
                st.doc.styles.character.iter().map(|s| s.name.clone()).collect()
            } else {
                st.doc.styles.paragraph.iter().map(|s| s.name.clone()).collect()
            }
        })
        .unwrap_or_default();
    let mut opts: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), crate::i18n::style_name(&app.ui.language, n))).collect();
    if character && !names.iter().any(|n| n == designcraft_doc::NO_CHAR_STYLE) {
        opts.insert(0, (designcraft_doc::NO_CHAR_STYLE, crate::i18n::style_name(&app.ui.language, designcraft_doc::NO_CHAR_STYLE)));
    }
    combo(ui, d, key, &opts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_params() {
        let f = command_fields("{name, type: custom|lastPageNumber|chapterNumber, text?, rule?: {on, weight}, flag?: bool} — creates");
        let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["name", "type", "text", "rule", "flag"]);
        assert_eq!(f[1].choices, ["custom", "lastPageNumber", "chapterNumber"]);
        assert!(!f[0].optional && f[2].optional);
        assert!(f[4].boolean);
        assert!(command_fields("{}").is_empty());
        assert!(command_fields("no params").is_empty());
        // A parenthetical after the key describes the value.
        let f = command_fields("{angle (degrees, CCW), ids?}");
        let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["angle", "ids"]);
        assert!(!f[0].optional && f[1].optional);
        assert_eq!(f[0].hint, "degrees, CCW");
        let f = command_fields("{scaleX? (%), ref?: 0..8, ids?: parent items (default: all)}");
        let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["scaleX", "ref", "ids"]);
        assert!(f.iter().all(|f| f.optional));
        assert_eq!(f[0].hint, "%");
        assert_eq!(f[2].hint, "parent items (default: all)");
        // Every command with a "…" label parses without panicking.
        for c in designcraft_engine::command_specs() {
            let _ = command_fields(c.params);
        }
    }
}
