//! Host downloads and export file delivery.

use super::{DesignApp, Value, json};

/// Save by handing the serialized document to the host as a download (web).
pub(super) fn download_document(app: &mut DesignApp) -> Result<Value, String> {
    let name = app
        .session
        .active()
        .map(|d| format!("{}.designcraft", d.doc.publication.as_ref().map_or(d.doc.title.as_str(), |v| &v.root.title)))
        .ok_or("no document")?;
    let revision = app.session.doc().map_err(|e| e.to_string())?.revision;
    download_copy(app, &name)?;
    app.run("file.markDownloaded", json!({"name":name,"revision":revision}))
}

/// A browser copy delivers bytes without changing the document's path or saved state.
pub(super) fn download_copy(app: &mut DesignApp, name: &str) -> Result<Value, String> {
    let ser = app.run("file.serialize", json!({}))?;
    let bytes = designcraft_engine::cmd::base64_decode(ser["base64"].as_str().unwrap_or_default());
    if let Some(write) = app.services.write.as_mut() {
        write(name, &bytes)?;
    } else if let Some(download) = app.services.download.as_mut() {
        download(name, &bytes);
    } else {
        return Err("no download service available".into());
    }
    Ok(json!({"path": name, "bytes": bytes.len()}))
}

pub(super) fn export_png(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let page = p.get("page").and_then(Value::as_u64).map(|v| v as usize).or_else(|| crate::canvas::current_page(app)).unwrap_or(0);
    let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(2.0);
    let mut r = designcraft_render::Renderer::new();
    let img = r
        .render_page(
            &st.doc,
            &app.session.cache,
            page,
            scale,
            false,
            &designcraft_render::RenderOptions { printing_only: true, rich_black: app.session.prefs.rich_black_output, ..Default::default() },
        )
        .ok_or("no such page")?;
    let png = img.to_png();
    let path = match p.get("path").and_then(Value::as_str) {
        Some(s) => Some(s.to_string()),
        None => {
            let name = format!("{}-p{}.png", st.doc.title, page + 1);
            app.services.pick_save.as_mut().and_then(|f| f(&name))
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    match app.services.write.as_mut() {
        Some(w) => w(&path, &png).map(|_| json!({"path": path, "width": img.width, "height": img.height})),
        None => Err("no writer".into()),
    }
}

/// File › Export IDML…: ask for a path, export through `file.exportIdml` and write the bytes with
/// the platform writer (a download on the web).
pub(super) fn export_idml(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let path = match p.get("path").and_then(Value::as_str) {
        Some(s) => Some(s.to_string()),
        None => {
            let name = format!("{}.idml", st.doc.title);
            app.services.pick_save.as_mut().and_then(|f| f(&name))
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let r = app.run("file.exportIdml", json!({}))?;
    let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
    match app.services.write.as_mut() {
        Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len()})),
        None => Err("no writer".into()),
    }
}

fn output_title(app: &DesignApp, p: &Value) -> String {
    app.session
        .active()
        .map(|s| {
            if let Some(v) = &s.doc.publication {
                let id = designcraft_doc::EditionId(p.get("edition").and_then(Value::as_u64).unwrap_or(v.edition.0));
                v.root.editions.get(&id).and_then(|e| e.output.title.clone()).unwrap_or_else(|| v.root.title.clone())
            } else {
                s.doc.title.clone()
            }
        })
        .unwrap_or_else(|| "Untitled".into())
}
fn report_export(app: &mut DesignApp, result: &Value) {
    if app.session.active().is_some_and(|s| s.doc.publication.is_some()) || result["warnings"].as_array().is_some_and(|w| !w.is_empty()) {
        let mut fields = result.clone();
        fields["mode"] = json!("exportReport");
        app.ui.dialog = Some(crate::dialogs::Dialog::new("publication", fields));
    }
    app.status(format!("Exported {}", result["path"].as_str().unwrap_or("document")));
}

/// Export EPUB / Text: ask for a path (default extension `ext`), run `cmd` without one and write
/// what it returns (`base64` or `text`) with the platform writer.
pub(super) fn export_bytes(app: &mut DesignApp, p: &Value, ext: &str, cmd: &str) -> Result<Value, String> {
    app.session.active().ok_or("no document")?;
    let path = match p.get("path").and_then(Value::as_str) {
        Some(s) => Some(s.to_string()),
        None => {
            let name = format!("{}.{ext}", output_title(app, p));
            app.services.pick_save.as_mut().and_then(|f| f(&name))
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let mut params = if p.is_object() { p.clone() } else { json!({}) };
    if let Some(parameters) = params.as_object_mut() {
        parameters.remove("path");
    }
    if cmd == "file.exportText" {
        let txt = path.to_ascii_lowercase().ends_with(".txt");
        params["format"] = json!(if txt { "txt" } else { "rtf" });
    }
    let r = app.run(cmd, params)?;
    let bytes = match r["text"].as_str() {
        Some(t) => t.as_bytes().to_vec(),
        None => designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default()),
    };
    app.services.write.as_mut().ok_or("no writer")?(&path, &bytes)?;
    let result = json!({"path":path,"bytes":bytes.len(),"warnings":r["warnings"],"pages":r["pages"]});
    if cmd != "file.exportText" {
        report_export(app, &result);
    }
    Ok(result)
}

/// File › Export PDF…: ask for a path, export through `file.exportPdf` and write the bytes with the
/// platform writer (a download on the web).
pub(super) fn export_pdf(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    app.session.active().ok_or("no document")?;
    let path = match p.get("path").and_then(Value::as_str) {
        Some(s) => Some(s.to_string()),
        None => {
            let name = format!("{}.pdf", output_title(app, p));
            app.services.pick_save.as_mut().and_then(|f| f(&name))
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let mut params = if p.is_object() { p.clone() } else { json!({}) };
    if let Some(o) = params.as_object_mut() {
        o.remove("path");
        o.entry("bleed").or_insert(json!(true));
        o.entry("tagged").or_insert(json!(true));
        if !app.ui.flattener.is_empty() {
            o.entry("flatten").or_insert(json!(app.ui.flattener));
        }
    }
    let r = app.run("file.exportPdf", params)?;
    let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
    app.services.write.as_mut().ok_or("no writer")?(&path, &bytes)?;
    let result = json!({"path":path,"bytes":bytes.len(),"warnings":r["warnings"],"pages":r["pages"]});
    report_export(app, &result);
    Ok(result)
}
