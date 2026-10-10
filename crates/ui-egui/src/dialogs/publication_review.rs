//! Review UI calls the same publication commands as CLI/control/MCP clients.
use super::{Dialog, Value, check, combo, json, text_field};
use crate::DesignApp;

pub(super) fn open(app: &mut DesignApp, p: &Value, f: &mut Value) -> Result<(), String> {
    let report = app.run("publication.review.list", json!({}))?;
    f["author"] = report["author"].clone();
    f["report"] = report;
    match f["mode"].as_str().unwrap_or("") {
        "review" => {
            f["on"] = f["report"]["tracking"].clone();
            f["action"] = json!("goTo");
            f["selected"] = json!([]);
        }
        "comment" => {
            f["text"] = json!("");
            f["selection"] = json!(app.session.doc().map_err(|e| e.to_string())?.selection);
            f["revision"] = json!(app.session.doc().map_err(|e| e.to_string())?.revision);
        }
        "proof" => {
            f["name"] = json!(format!("Proof {}", f["report"]["proofs"].as_array().map_or(1, |p| p.len() + 1)));
            f["action"] = json!("create");
            f["note"] = json!("");
        }
        "proofCompare" => {
            f["before"] = f["report"]["proofs"].as_array().and_then(|v| v.first()).map(|p| p["id"].clone()).unwrap_or(Value::Null);
            f["after"] = Value::Null;
        }
        "manuscript" => {
            f["report"] = match p.get("report") {
                Some(r) => r.clone(),
                None => app.run("publication.manuscript.preview", p.clone())?,
            };
            f["selected"] = json!([]);
            f["autoflow"] = json!(true);
        }
        _ => return Err("Unknown review dialog".into()),
    }
    Ok(())
}
fn choose(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str, proofs: &[Value], current: bool) {
    let mut selected = d.fields.get(key).and_then(Value::as_u64);
    let name = proofs.iter().find(|p| p["id"].as_u64() == selected).and_then(|p| p["name"].as_str()).unwrap_or("Current edition");
    egui::ComboBox::from_id_salt(key).selected_text(format!("{label}: {name}")).show_ui(ui, |ui| {
        if current {
            ui.selectable_value(&mut selected, None, "Current edition");
        }
        for p in proofs {
            ui.selectable_value(&mut selected, p["id"].as_u64(), p["name"].as_str().unwrap_or("Proof"));
        }
    });
    d.fields.insert(key.into(), json!(selected));
}
fn selected(ui: &mut egui::Ui, id: u64, label: impl Into<egui::WidgetText>, values: &mut std::collections::BTreeSet<u64>) {
    let mut on = values.contains(&id);
    if ui.checkbox(&mut on, label).changed() {
        if on {
            values.insert(id);
        } else {
            values.remove(&id);
        }
    }
}
pub(super) fn show(ui: &mut egui::Ui, d: &mut Dialog) {
    let report = d.fields.get("report").cloned().unwrap_or(Value::Null);
    let proofs = report["proofs"].as_array().cloned().unwrap_or_default();
    egui::ScrollArea::vertical().id_salt("publication-review").max_height(530.).show(ui, |ui| match d.s("mode").as_str() {
        "review" => {
            ui.heading("Publication review");
            ui.horizontal(|ui| {
                ui.label("Author");
                text_field(ui, d, "author", 300.);
            });
            check(ui, d, "on", "Track edits throughout this publication");
            combo(
                ui,
                d,
                "action",
                &[
                    ("goTo", "Go to selected passage"),
                    ("accept", "Accept selected changes"),
                    ("reject", "Reject selected changes"),
                    ("resolveComment", "Resolve selected comments"),
                    ("tracking", "Apply tracking settings"),
                ],
            );
            let mut values = d.fields.get("selected").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
            ui.separator();
            ui.strong("Pending changes");
            for r in report["changes"].as_array().into_iter().flatten() {
                if let Some(id) = r["id"].as_u64() {
                    selected(
                        ui,
                        id,
                        format!("{} · {} · {}", r["author"].as_str().unwrap_or(""), r["kind"].as_str().unwrap_or(""), snippet(&r["text"])),
                        &mut values,
                    );
                }
            }
            if report["changes"].as_array().is_some_and(|r| r.is_empty()) {
                ui.label("No pending changes.");
            }
            ui.separator();
            ui.strong("Comments");
            for c in report["comments"].as_array().into_iter().flatten() {
                let r = &c["comment"];
                if let Some(id) = r["id"].as_u64() {
                    selected(
                        ui,
                        id,
                        format!(
                            "{} · {}{}{}",
                            r["author"].as_str().unwrap_or(""),
                            snippet(&r["text"]),
                            if r["resolved"] == true { " · resolved" } else { "" },
                            if c["unresolved"] == true { " · passage removed" } else { "" }
                        ),
                        &mut values,
                    );
                    ui.label(format!("“{}”", snippet(&r["quote"])));
                }
            }
            d.fields.insert("selected".into(), json!(values));
        }
        "comment" => {
            ui.heading("Comment on selected passage");
            ui.label("Author");
            text_field(ui, d, "author", 350.);
            ui.label("Comment");
            let mut t = d.s("text");
            ui.add(egui::TextEdit::multiline(&mut t).desired_width(510.).desired_rows(7));
            d.fields.insert("text".into(), json!(t));
        }
        "proof" => {
            ui.heading("Proof baselines");
            combo(ui, d, "action", &[("create", "Capture a new proof"), ("approve", "Approve an existing proof")]);
            ui.label("Author");
            text_field(ui, d, "author", 340.);
            if d.s("action") == "create" {
                ui.label("Unique proof name");
                text_field(ui, d, "name", 400.);
                ui.label("Captures content, edition settings, pagination and fonts. This snapshot stays unchanged as editing continues.");
            } else {
                choose(ui, d, "id", "Proof", &proofs, false);
                ui.label("Approval note");
                text_field(ui, d, "note", 450.);
            }
            for p in &proofs {
                ui.label(format!(
                    "{} · {}{}",
                    p["name"].as_str().unwrap_or(""),
                    p["author"].as_str().unwrap_or(""),
                    if !p["approval"].is_null() { " · approved" } else { "" }
                ));
            }
        }
        "proofCompare" => {
            ui.heading("Compare proofs");
            choose(ui, d, "before", "From", &proofs, false);
            choose(ui, d, "after", "To", &proofs, true);
            ui.label("Compare content, typography, figure positions, pagination, references and removed index passages.");
        }
        "proofDiff" => {
            ui.heading("Proof comparison");
            let rows = report["rows"].as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                ui.label("No differences found.");
            }
            let mut row = d.fields.get("row").and_then(Value::as_u64);
            for r in rows {
                let component = r["componentTitle"]
                    .as_str()
                    .map(|s| format!("{s} · "))
                    .or_else(|| r["component"].as_u64().map(|n| format!("Chapter {n} · ")))
                    .unwrap_or_default();
                ui.selectable_value(&mut row, r["row"].as_u64(), format!("{component}{}", r["kind"].as_str().unwrap_or("Difference")));
                if r["kind"] == "content" {
                    ui.label(format!("Before: {}", snippet(&r["before"])));
                    ui.label(format!("After: {}", snippet(&r["after"])));
                }
                if let Some(detail) = r["detail"].as_str() {
                    ui.label(detail);
                }
            }
            d.fields.insert("row".into(), json!(row));
            ui.label("Select a chapter difference and confirm to go to the current passage.");
        }
        "manuscript" => {
            ui.heading("Revised manuscript");
            ui.label("Select revisions to apply. Selecting a text or positional match explicitly approves that proposed pairing.");
            check(ui, d, "autoflow", "Add pages when revised text needs them");
            let mut values = d.fields.get("selected").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
            for r in report["rows"].as_array().into_iter().flatten() {
                if let Some(row) = r["row"].as_u64() {
                    ui.group(|ui| {
                        selected(
                            ui,
                            row,
                            format!(
                                "{} · {}{}",
                                r["kind"].as_str().unwrap_or("Revision"),
                                r["confidence"].as_str().unwrap_or(""),
                                if r["requiresReview"] == true { " · review this match" } else { "" }
                            ),
                            &mut values,
                        );
                        ui.label(format!("Before: {}", snippet(&r["beforeText"])));
                        ui.label(format!("After: {}", snippet(&r["afterText"])));
                    });
                }
            }
            d.fields.insert("selected".into(), json!(values));
            for w in report["warnings"].as_array().into_iter().flatten() {
                if let Some(w) = w.as_str() {
                    ui.label(w);
                }
            }
        }
        _ => {}
    });
}
fn snippet(v: &Value) -> String {
    designcraft_doc::xref::clean(v.as_str().unwrap_or("")).chars().take(160).collect()
}
pub(super) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let p = Value::Object(d.fields.clone());
    match d.s("mode").as_str() {
        "review" => {
            let ids = p["selected"].as_array().cloned().unwrap_or_default();
            match d.s("action").as_str() {
                "tracking" => app.run("changes.track", json!({"on":p["on"],"author":p["author"]})),
                "accept" | "reject" => app.run("publication.review.resolve", json!({"ids":ids,"accept":d.s("action")=="accept"})),
                "resolveComment" => {
                    for id in ids {
                        app.run("publication.comment.edit", json!({"id":id,"resolved":true}))?;
                    }
                    Ok(Value::Null)
                }
                _ => {
                    if ids.len() != 1 {
                        return Err("Select one passage to open.".into());
                    }
                    app.run("publication.review.goTo", json!({"id":ids.first()}))
                }
            }
        }
        "comment" => {
            let state = app.session.doc().map_err(|e| e.to_string())?;
            if p["revision"] != json!(state.revision) || p["selection"] != json!(state.selection) {
                return Err("The selection changed. Select the passage and open a new comment.".into());
            }
            app.run("publication.comment.add", json!({"text":p["text"],"author":p["author"]}))
        }
        "proof" => app.run(if d.s("action") == "approve" { "publication.proof.approve" } else { "publication.proof.create" }, p),
        "proofCompare" => {
            let report = app.run("publication.proof.compare", json!({"before":p["before"],"after":p["after"]}))?;
            app.ui.dialog = Some(Dialog::new("publication", json!({"mode":"proofDiff","report":report,"uid":p["uid"]})));
            Ok(Value::Null)
        }
        "proofDiff" => {
            let row = p["row"].as_u64().and_then(|i| p["report"]["rows"].get(i as usize)).ok_or("Select a difference with a chapter location.")?;
            app.run("publication.proof.goTo", row.clone())
        }
        "manuscript" => app.run(
            "publication.manuscript.apply",
            json!({"preview":p["report"]["preview"],"rows":p["selected"],"reviewed":p["selected"],"autoflow":p["autoflow"]}),
        ),
        _ => Err("Unknown review dialog".into()),
    }
}
