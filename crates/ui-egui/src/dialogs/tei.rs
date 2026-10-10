//! TEI import options, explicit image mappings, and the retained import receipt.
use super::{DesignApp, Dialog, Value, json};

pub(crate) fn open(app: &mut DesignApp, params: Value, purpose: &str) -> Result<Value, String> {
    let info = app.run("tei.inspect", params.clone())?;
    let mut fields = params.as_object().cloned().unwrap_or_default();
    fields.insert("purpose".into(), json!(purpose));
    if matches!(purpose, "publicationAdd" | "manuscript") {
        fields.insert("publicationUid".into(), json!(app.session.doc().map_err(|e| e.to_string())?.uid));
    }
    if purpose == "manuscript" {
        let v = app.session.doc().map_err(|e| e.to_string())?.doc.publication.clone().ok_or("no publication")?;
        fields.insert("edition".into(), json!(v.edition));
        fields.insert("component".into(), json!(v.component));
        fields.insert("publicationRevision".into(), json!(v.root.revision));
    }
    fields.insert("info".into(), info.clone());
    fields.insert("styles".into(), info["styles"].clone());
    fields.insert("witness".into(), params.pointer("/tei/witness").cloned().unwrap_or(json!("")));
    fields.entry("resources").or_insert(json!({}));
    fields.entry("map").or_insert_with(|| params.get("styleMap").cloned().unwrap_or(json!({})));
    let conflicts: Vec<_> = info["styles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|name| matches!(purpose, "place" | "manuscript") && app.session.active().is_some_and(|st| st.doc.styles.para(name).is_some()))
        .collect();
    fields.insert("conflicts".into(), json!(conflicts));
    fields.entry("styleConflicts").or_insert(json!("useExisting"));
    fields.entry("autoflow").or_insert(json!(true));
    app.ui.dialog = Some(Dialog::new("teiImport", Value::Object(fields)));
    Ok(json!({"dialog":"teiImport"}))
}
pub(super) fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.label(d.fields.get("info").and_then(|i| i.get("name")).and_then(Value::as_str).unwrap_or("TEI book"));
    ui.label("Reading edition · reflowed pages · original XML preserved");
    ui.horizontal(|ui| {
        ui.label("Witness:");
        let current = d.s("witness");
        egui::ComboBox::from_id_salt("tei-witness").selected_text(if current.is_empty() { "Lemma (default)" } else { &current }).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_empty(), "Lemma (default)").clicked() {
                d.fields.insert("witness".into(), json!(""));
            }
            let witnesses = d.fields.get("info").and_then(|i| i.get("witnesses")).and_then(Value::as_object).cloned().unwrap_or_default();
            for (id, label) in witnesses {
                if ui.selectable_label(current == id, format!("{id} — {}", label.as_str().unwrap_or(""))).clicked() {
                    d.fields.insert("witness".into(), json!(id));
                }
            }
        });
    });
    if matches!(d.s("purpose").as_str(), "place" | "manuscript") {
        super::check(ui, d, "autoflow", "Add pages until the text and endnotes fit");
    }
    if matches!(d.s("purpose").as_str(), "place" | "manuscript") {
        egui::CollapsingHeader::new("Styles and mapping").show(ui, |ui| super::import_options::show(app, ui, d));
    } else {
        super::check(ui, d, "removeStyles", "Remove styles and formatting");
    }
    let resources = d.fields.get("info").and_then(|i| i.get("resources")).and_then(Value::as_array).cloned().unwrap_or_default();
    if !resources.is_empty() {
        ui.separator();
        ui.label("Illustrations");
        ui.label("Desktop paths resolve beside the TEI file. Choose files here to override them or supply browser images.");
        egui::ScrollArea::vertical().id_salt("tei-resources").max_height(160.0).show(ui, |ui| {
            for resource in resources {
                let url = resource["url"].as_str().unwrap_or("").to_string();
                ui.horizontal(|ui| {
                    ui.label(&url);
                    if d.fields.get("resources").and_then(|r| r.get(&url)).is_some() {
                        ui.label("Assigned");
                    }
                    if ui.button("Choose file…").clicked() {
                        if let Some(pick) = app.services.pick_open.as_mut() {
                            if let Some(path) = pick("teiResource")
                                && let Some(m) = d.fields.get_mut("resources").and_then(Value::as_object_mut)
                            {
                                m.insert(url.clone(), json!({"path":path}));
                            }
                        } else if let Some(open) = app.services.open_async.as_mut() {
                            open(&format!("teiResource:{url}"));
                        }
                    }
                });
            }
        });
    }
    let diagnostics = d.fields.get("info").and_then(|i| i.get("diagnostics")).and_then(Value::as_array).cloned().unwrap_or_default();
    if !diagnostics.is_empty() {
        egui::CollapsingHeader::new(format!("{} source notices", diagnostics.len())).show(ui, |ui| {
            diagnostic_list(ui, &diagnostics);
        });
    }
}
pub(super) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let mut p = Value::Object(d.fields.clone());
    p["tei"] = if d.s("witness").is_empty() { json!({}) } else { json!({"witness":d.s("witness")}) };
    p["styleMap"] = d.fields.get("map").cloned().unwrap_or(json!({}));
    if let Some(uid) = d.fields.get("publicationUid").and_then(Value::as_u64) {
        let i = app.session.documents().iter().position(|s| s.uid == uid).ok_or("the publication is no longer open")?;
        app.run("file.activate", json!({"index":i}))?;
    }
    if d.s("purpose") == "manuscript" {
        let v = app.session.doc().map_err(|e| e.to_string())?.doc.publication.as_ref().ok_or("no publication")?;
        if p["publicationRevision"] != json!(v.root.revision) {
            return Err("The publication changed. Compare the manuscript again.".into());
        }
        app.run("publication.activate", json!({"edition":p["edition"],"component":p["component"]}))?;
        p["mode"] = json!("manuscript");
        return super::publication::open(app, &p);
    }
    let command = match d.s("purpose").as_str() {
        "place" => "file.place",
        "publicationAdd" => "publication.addComponent",
        _ => "file.openTei",
    };
    let result = app.run(command, p);
    match result {
        Ok(r) => {
            let report = app.run("tei.report", json!({"source":r["source"]}))?;
            app.ui.dialog = Some(Dialog::new("teiReport", json!({"reports":report})));
            Ok(r)
        }
        Err(e) => {
            app.ui.dialog = Some(d.clone());
            app.status(e.clone());
            Err(e)
        }
    }
}
fn diagnostic_list(ui: &mut egui::Ui, list: &[Value]) {
    egui::ScrollArea::vertical().id_salt("tei-diagnostics").max_height(170.0).show(ui, |ui| {
        for d in list {
            ui.label(format!("{}:{} · {} — {}", d["line"], d["column"], d["element"].as_str().unwrap_or(""), d["message"].as_str().unwrap_or("")));
        }
    });
}
pub(super) fn report(app: &mut DesignApp, ui: &mut egui::Ui, d: &Dialog) {
    let reports = d.fields.get("reports").and_then(Value::as_array).cloned().unwrap_or_default();
    if reports.is_empty() {
        ui.label("This document has no imported TEI sources.");
    }
    egui::ScrollArea::vertical().id_salt("tei-report").max_height(460.0).show(ui, |ui| {
        for r in reports {
            ui.heading(r["name"].as_str().unwrap_or("TEI source"));
            ui.label(format!("{} source bytes preserved", r["byteLength"]));
            if ui.button("Save original TEI…").clicked() {
                let _ = app.run("app.teiSource", json!({"source":r["id"]}));
            }
            if let Some(metadata) = r["metadata"].as_array() {
                for m in metadata {
                    for key in ["titles", "contributors", "edition", "publication", "sources", "identifiers", "languages", "rights"] {
                        if let Some(values) = m[key].as_array()
                            && !values.is_empty()
                        {
                            ui.label(format!("{key}: {}", values.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")));
                        }
                    }
                }
            }
            if let Some(list) = r["diagnostics"].as_array() {
                egui::CollapsingHeader::new(format!("{} import notices", list.len())).default_open(true).show(ui, |ui| diagnostic_list(ui, list));
            }
            if let Some(nav) = r["navigation"].as_array() {
                egui::CollapsingHeader::new("Contents and source pages").default_open(true).show(ui, |ui| {
                    for n in nav {
                        let label = format!(
                            "{}{}{}",
                            if n["kind"] == "sourcePage" { "Source page " } else { "" },
                            n["label"].as_str().unwrap_or(""),
                            n["page"].as_u64().map(|p| format!(" · page {p}")).unwrap_or_else(|| " · removed or overset".into())
                        );
                        if ui.link(label).clicked()
                            && let Ok(v) = app.run("tei.goTo", json!({"anchor":n["anchor"]}))
                            && let Some(page) = v["page"].as_u64()
                        {
                            let _ = app.run("view.goToPage", json!({"page":page}));
                        }
                    }
                });
            }
            ui.separator();
        }
    });
}
pub(crate) fn save_source(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let v = app.run("tei.source", p.clone())?;
    let name = v["name"].as_str().unwrap_or("source.tei");
    let bytes = designcraft_engine::cmd::base64_decode(v["base64"].as_str().unwrap_or(""));
    if let Some(download) = app.services.download.as_mut() {
        download(name, &bytes);
        return Ok(json!({"name":name,"bytes":bytes.len()}));
    }
    let path = p["path"].as_str().map(str::to_string).or_else(|| app.services.pick_save.as_mut().and_then(|pick| pick(name)));
    if let Some(path) = path {
        let write = app.services.write.as_mut().ok_or("host has no file writer")?;
        write(&path, &bytes)?;
        return Ok(json!({"path":path,"bytes":bytes.len()}));
    }
    Ok(Value::Null)
}
