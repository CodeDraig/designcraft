//! Edition configuration and output choices. Commands own validation and document changes.
use super::{Dialog, Value, check, combo, json, text_field};
use crate::DesignApp;

pub(super) fn open(app: &mut DesignApp, p: &Value, fields: &mut Value) -> Result<(), String> {
    let v = app.session.doc().map_err(|e| e.to_string())?.doc.publication.clone().ok_or("no publication open")?;
    match fields["mode"].as_str().unwrap_or("") {
        "editionSettings" => {
            let settings = app.run("publication.edition.get", p.clone())?;
            fields["edition"] = settings["edition"].clone();
            fields["name"] = settings["name"].clone();
            fields["witness"] = json!(settings["witness"].as_str().unwrap_or(""));
            fields["sections"] = settings["sections"].clone();
            for key in ["title", "author", "language", "identifier"] {
                fields[key] = json!(settings["output"][key].as_str().unwrap_or(""));
            }
            let id = designcraft_doc::EditionId(settings["edition"].as_u64().ok_or("missing edition")?);
            fields["streams"] = json!(
                v.root.note_streams.keys().map(|name| json!({"name":name,"placement":v.root.note_policy(id,name).placement})).collect::<Vec<_>>()
            );
        }
        "export" => {
            fields["edition"] = p.get("edition").cloned().unwrap_or(json!(v.edition));
            fields["editions"] = json!(v.root.editions.values().map(|e| json!({"id":e.id,"name":e.name})).collect::<Vec<_>>());
            fields["format"] = p.get("format").cloned().unwrap_or(json!("pdf"));
            fields["scope"] = p.get("scope").cloned().unwrap_or(json!("publication"));
            fields["strict"] = json!(false);
            fields["bleed"] = json!(true);
            fields["tagged"] = json!(true);
            fields["cover"] = json!(true);
            fields["pages"] = json!("");
            fields["path"] = p.get("path").cloned().unwrap_or(json!(""));
        }
        _ => return Err("unknown edition dialog".into()),
    }
    Ok(())
}

pub(super) fn show(ui: &mut egui::Ui, d: &mut Dialog) {
    egui::ScrollArea::vertical().max_height(490.0).show(ui, |ui| match d.s("mode").as_str() {
        "editionSettings" => {
            ui.strong("Edition settings");
            ui.label("Name");
            text_field(ui, d, "name", 370.0);
            ui.label("Output metadata — leave blank to use publication metadata");
            for (key, label) in [("title", "Title"), ("author", "Author"), ("language", "Language tag"), ("identifier", "Identifier")] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    text_field(ui, d, key, 320.0);
                });
            }
            ui.separator();
            ui.label("Witness for the main reading (blank uses the imported reading)");
            text_field(ui, d, "witness", 180.0);
            ui.strong("Annotation streams");
            if let Some(streams) = d.fields.get_mut("streams").and_then(Value::as_array_mut) {
                for row in streams {
                    let name = row["name"].as_str().unwrap_or("").to_string();
                    ui.push_id(&name, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(&name);
                            let current = row["placement"].as_str().unwrap_or("footnote").to_string();
                            egui::ComboBox::from_id_salt("placement").selected_text(placement_label(&current)).show_ui(ui, |ui| {
                                for (value, label) in PLACEMENTS {
                                    if ui.selectable_label(current == *value, *label).clicked() {
                                        row["placement"] = json!(value);
                                    }
                                }
                            });
                        });
                    });
                }
            }
            ui.label("Use each stream's Placement dialog in the Publication panel for columns, continuation and spacing.");
            ui.separator();
            ui.strong("Included generated sections");
            if let Some(rows) = d.fields.get_mut("sections").and_then(Value::as_array_mut) {
                if rows.is_empty() {
                    ui.label("No generated sections yet.");
                }
                for row in rows {
                    let mut included = row["included"].as_bool().unwrap_or(true);
                    ui.checkbox(&mut included, row["name"].as_str().unwrap_or("Section"));
                    row["included"] = json!(included);
                }
            }
            ui.label("Typography and page geometry are edited in the selected edition. Shared text stays the same in every edition.");
        }
        "export" => {
            ui.strong("Export publication");
            let editions = d.fields.get("editions").and_then(Value::as_array).cloned().unwrap_or_default();
            let selected = d.fields.get("edition").cloned().unwrap_or(Value::Null);
            let name = editions.iter().find(|e| e["id"] == selected).and_then(|e| e["name"].as_str()).unwrap_or("Choose edition");
            egui::ComboBox::from_id_salt("output_edition").selected_text(name).show_ui(ui, |ui| {
                for e in &editions {
                    if ui.selectable_label(e["id"] == selected, e["name"].as_str().unwrap_or("Edition")).clicked() {
                        d.fields.insert("edition".into(), e["id"].clone());
                    }
                }
            });
            combo(ui, d, "scope", &[("publication", "Whole publication"), ("component", "Current chapter")]);
            combo(
                ui,
                d,
                "format",
                &[("pdf", "PDF"), ("epub", "EPUB — reflowable"), ("fixedEpub", "EPUB — fixed layout"), ("html", "HTML — self-contained")],
            );
            match d.s("format").as_str() {
                "pdf" => {
                    ui.label("Page positions (blank means all pages)");
                    text_field(ui, d, "pages", 240.0);
                    check(ui, d, "bleed", "Include bleed");
                    check(ui, d, "tagged", "Include PDF structure tags");
                }
                "epub" => check(ui, d, "cover", "Render first page as cover"),
                _ => {}
            }
            check(ui, d, "strict", "Stop if export reports unresolved or unsupported content");
            ui.label("Output path (blank opens the save picker or downloads in the browser)");
            text_field(ui, d, "path", 430.0);
            ui.label("Export resolves the chosen edition's references without changing the chapter or edition you are editing.");
        }
        "exportReport" => {
            ui.strong("Export complete");
            ui.label(d.s("path"));
            if let Some(pages) = d.fields.get("pages").and_then(Value::as_u64) {
                ui.label(format!("{pages} pages"));
            }
            if let Some(warnings) = d.fields.get("warnings").and_then(Value::as_array) {
                if warnings.is_empty() {
                    ui.label("No export diagnostics.");
                }
                for warning in warnings {
                    if let Some(w) = warning.as_str() {
                        ui.label(w);
                    }
                }
            }
        }
        _ => {}
    });
}
const PLACEMENTS: &[(&str, &str)] = &[
    ("footnote", "Footnotes"),
    ("componentEnd", "Chapter endnotes"),
    ("publicationEnd", "Publication endnotes"),
    ("sidenote", "Sidenotes"),
    ("omit", "Omit from this edition"),
];
fn placement_label(value: &str) -> &str {
    PLACEMENTS.iter().find(|(id, _)| *id == value).map_or(value, |(_, label)| *label)
}

pub(super) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let result = match d.s("mode").as_str() {
        "editionSettings" => {
            let mut output = json!({});
            for key in ["title", "author", "language", "identifier"] {
                let text = d.s(key);
                output[key] = if text.trim().is_empty() { Value::Null } else { json!(text) };
            }
            output["excludedSections"] = json!(
                d.fields
                    .get("sections")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|r| r["included"] == false)
                    .map(|r| r["content"].clone())
                    .collect::<Vec<_>>()
            );
            let mut policies = json!({});
            for row in d.fields.get("streams").and_then(Value::as_array).into_iter().flatten() {
                if let Some(name) = row["name"].as_str() {
                    policies[name] = json!({"placement":row["placement"]});
                }
            }
            let witness = d.s("witness");
            app.run("publication.edition.configure",json!({"edition":d.fields.get("edition"),"name":d.s("name"),"output":output,"notePolicies":policies,"witness":if witness.trim().is_empty(){Value::Null}else{json!(witness)}}))
        }
        "export" => {
            let cmd = match d.s("format").as_str() {
                "pdf" => "app.exportPdf",
                "epub" => "app.exportEpub",
                "fixedEpub" => "app.exportFixedEpub",
                "html" => "app.exportHtml",
                _ => return Err("choose an output format".into()),
            };
            let mut params = json!({"edition":d.fields.get("edition"),"scope":d.s("scope"),"strict":d.b("strict"),"bleed":d.b("bleed"),"tagged":d.b("tagged"),"cover":d.b("cover")});
            if !d.s("pages").trim().is_empty() && d.s("format") == "pdf" {
                params["pages"] = json!(d.s("pages"));
            }
            if !d.s("path").trim().is_empty() {
                params["path"] = json!(d.s("path"));
            }
            app.run(cmd, params)
        }
        "exportReport" => Ok(Value::Null),
        _ => Err("unknown edition dialog".into()),
    };
    if let Err(e) = &result {
        app.status(e.clone());
        app.ui.dialog = Some(d.clone());
    }
    result
}
