use super::{Dialog, Value, combo, json, text_field};
use crate::DesignApp;

pub(crate) fn open(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let mode = p.get("mode").and_then(Value::as_str).unwrap_or("new");
    let mut fields = json!({"mode":mode});
    match mode {
        "layout" => {
            fields["report"] = app.run("publication.layout.metrics", json!({}))?;
            fields["components"] = app.run("publication.list", json!({}))?["components"].clone();
            fields["verify"] = json!(false);
        }
        "new" => {
            fields["title"] = json!("Untitled Publication");
            fields["componentTitle"] = json!("Chapter 1");
        }
        "edition" => {
            fields["name"] = json!("Reading edition");
        }
        "component" => {
            let v = app.session.active().and_then(|s| s.doc.publication.as_ref()).ok_or("no publication open")?;
            let id = designcraft_doc::ComponentId(p.get("component").and_then(Value::as_u64).unwrap_or(v.component.0));
            let c = v.root.components.get(&id).ok_or("no such chapter")?;
            fields["component"] = json!(id);
            fields["title"] = json!(c.title);
            fields["role"] = json!(c.role);
        }
        "editionSettings" | "export" => super::publication_editions::open(app, p, &mut fields)?,
        "review" | "comment" | "proof" | "proofCompare" | "manuscript" => super::publication_review::open(app, p, &mut fields)?,
        "settings" | "styles" | "numbering" | "mark" => super::publication_settings::open(app, p, &mut fields)?,
        "index" | "indexTopic" | "indexMark" | "indexPreview" => super::publication_indexes::open(app, p, &mut fields)?,
        "noteStream" | "notePolicy" | "noteInsert" | "notes" | "apparatus" | "apparatusRecover" => {
            super::publication_notes::open(app, p, &mut fields)?
        }
        "reference" | "generated" | "template" | "scan" => super::publication_references::open(app, p, &mut fields)?,
        _ => return Err("choose new, component, edition, settings, styles, numbering, mark, reference, generated, template or scan".into()),
    }
    if mode != "new" {
        fields["uid"] = json!(app.session.doc().map_err(|e| e.to_string())?.uid);
    }
    app.ui.dialog = Some(Dialog::new("publication", fields));
    Ok(Value::Null)
}

pub(super) fn show(ui: &mut egui::Ui, d: &mut Dialog) {
    match d.s("mode").as_str() {
        "layout" => super::publication_layout::show(ui, d),
        "review" | "comment" | "proof" | "proofCompare" | "proofDiff" | "manuscript" => super::publication_review::show(ui, d),
        "editionSettings" | "export" | "exportReport" => super::publication_editions::show(ui, d),
        "settings" | "styles" | "numbering" | "mark" => super::publication_settings::show(ui, d),
        "index" | "indexTopic" | "indexMark" | "indexPreview" => super::publication_indexes::show(ui, d),
        "noteStream" | "notePolicy" | "noteInsert" | "notes" | "apparatus" | "apparatusRecover" => super::publication_notes::show(ui, d),
        "reference" | "generated" | "template" | "scan" => super::publication_references::show(ui, d),
        "edition" => {
            ui.label("Edition name");
            text_field(ui, d, "name", 300.0);
            ui.label("Shares the publication's text and starts with the current edition's layout.");
        }
        "component" => {
            ui.label("Chapter title");
            text_field(ui, d, "title", 300.0);
            ui.label("Part of the book");
            combo(ui, d, "role", &[("frontMatter", "Front matter"), ("chapter", "Chapters"), ("backMatter", "Back matter")]);
        }
        _ => {
            ui.label("Publication title");
            text_field(ui, d, "title", 300.0);
            ui.label("First chapter title");
            text_field(ui, d, "componentTitle", 300.0);
        }
    }
}

pub(super) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    if let Some(uid) = d.fields.get("uid").and_then(Value::as_u64) {
        let i = app.session.documents().iter().position(|s| s.uid == uid).ok_or("the publication is no longer open")?;
        app.run("file.activate", json!({"index":i}))?;
    }
    if matches!(d.s("mode").as_str(), "editionSettings" | "export" | "exportReport") {
        return super::publication_editions::confirm(app, d);
    }
    if matches!(d.s("mode").as_str(), "review" | "comment" | "proof" | "proofCompare" | "proofDiff" | "manuscript") {
        return super::publication_review::confirm(app, d);
    }
    if matches!(d.s("mode").as_str(), "settings" | "styles" | "numbering" | "mark") {
        return super::publication_settings::confirm(app, d);
    }
    if matches!(d.s("mode").as_str(), "index" | "indexTopic" | "indexMark" | "indexPreview") {
        return super::publication_indexes::confirm(app, d);
    }
    if matches!(d.s("mode").as_str(), "reference" | "generated" | "template" | "scan") {
        return super::publication_references::confirm(app, d);
    }
    if matches!(d.s("mode").as_str(), "noteStream" | "notePolicy" | "noteInsert" | "notes" | "apparatus" | "apparatusRecover") {
        return super::publication_notes::confirm(app, d);
    }
    if d.s("mode") == "layout" {
        let result = if d.fields.get("verify").and_then(Value::as_bool).unwrap_or(false) {
            Some(app.run("publication.layout.verify", json!({}))?)
        } else {
            None
        };
        let mut fields = json!({"mode":"layout","report":app.run("publication.layout.metrics",json!({}))?,"verification":result});
        fields["uid"] = json!(app.session.doc().map_err(|e| e.to_string())?.uid);
        fields["components"] = app.run("publication.list", json!({}))?["components"].clone();
        app.ui.dialog = Some(Dialog::new("publication", fields));
        return Ok(Value::Null);
    }
    let cmd = match d.s("mode").as_str() {
        "component" => "publication.component",
        "edition" => "publication.edition.create",
        _ => "publication.new",
    };
    let result = app.run(cmd, Value::Object(d.fields.clone()));
    if result.is_ok() {
        app.run("window.panel", json!({"panel":"publication"}))?;
    }
    result
}
