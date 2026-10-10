//! Browser file picker and download lifetimes.

use super::{DOC_EXTS, IMAGE_EXTS};
use designcraft_ui_egui::{DeliveredFiles, Inbox, Services};
use wasm_bindgen::JsCast as _;

pub(super) fn services(inbox: Inbox, ctx: egui::Context) -> Services {
    let open_inbox = inbox.clone();
    let status = designcraft_ui_egui::StatusInbox::default();
    let open_status = status.clone();
    let prefix = format!("{}-{:x}/", js_sys::Date::now(), js_sys::Math::random().to_bits());
    let save_recovery = super::recovery::writer(prefix.clone(), status.clone(), ctx.clone());
    Services {
        open_async: Some(Box::new(move |purpose: &str| {
            let inbox = open_inbox.clone();
            let ctx = ctx.clone();
            if purpose == "browserRecovery" {
                super::recovery::recover(prefix.clone(), inbox, open_status.clone(), ctx);
                return;
            }
            let purpose = purpose.to_string();
            let dialog = if purpose == "swatches" {
                rfd::AsyncFileDialog::new().add_filter("Swatch Exchange (ASE)", &["ase"])
            } else if purpose == "publicationBook" {
                rfd::AsyncFileDialog::new().add_filter("Book and chapters", &["dcbook", "designcraft", "idml", "tei", "xml"])
            } else if purpose.starts_with("manuscript:") {
                rfd::AsyncFileDialog::new().add_filter("Revised manuscript", &["tei", "xml", "docx", "rtf", "txt", "md"])
            } else if purpose == "place" || purpose.starts_with("teiResource:") {
                rfd::AsyncFileDialog::new().add_filter("Graphics", IMAGE_EXTS)
            } else {
                rfd::AsyncFileDialog::new().add_filter("DesignCraft", DOC_EXTS)
            };
            wasm_bindgen_futures::spawn_local(async move {
                if purpose == "publicationBook" {
                    let Some(files) = dialog.pick_files().await else {
                        return;
                    };
                    let mut delivered = Vec::new();
                    for file in files {
                        delivered.push((file.file_name(), file.read().await));
                    }
                    inbox.lock().unwrap_or_else(|e| e.into_inner()).push(DeliveredFiles { purpose, files: delivered });
                    ctx.request_repaint();
                    return;
                }
                let Some(file) = dialog.pick_file().await else {
                    return;
                };
                let bytes = file.read().await;
                inbox.lock().unwrap_or_else(|e| e.into_inner()).push(DeliveredFiles { purpose, files: vec![(file.file_name(), bytes)] });
                ctx.request_repaint();
            });
        })),
        // Exports ask for a name; the browser decides where the download goes.
        pick_save: Some(Box::new(|name: &str| Some(name.to_string()))),
        write: Some(Box::new(|name: &str, bytes: &[u8]| download(name, bytes))),
        download: Some(Box::new(|name: &str, bytes: &[u8]| {
            if let Err(e) = download(name, bytes) {
                log::error!("download of {name} failed: {e}");
            }
        })),
        inbox: Some(inbox),
        save_recovery: Some(save_recovery),
        status_inbox: Some(status),
        ..Default::default()
    }
}

/// Trigger a browser download of `bytes` named after the last component of `path`.
fn download(path: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "designcraft".into());
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime_for(&name));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let a: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.dyn_into().map_err(|_| "not an anchor")?;
    a.set_href(&url);
    a.set_download(&name);
    a.style().set_property("display", "none").map_err(js)?;
    let body = document.body().ok_or("no body")?;
    body.append_child(&a).map_err(js)?;
    a.click();
    a.remove();
    // Revoke after the click has been dispatched; the download keeps its own reference.
    let revoke = wasm_bindgen::closure::Closure::once_into_js(move || {
        web_sys::Url::revoke_object_url(&url).ok();
    });
    window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000).map_err(js)?;
    Ok(())
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("pdf") => "application/pdf",
        Some("designcraft") => "application/vnd.designcraft+zip",
        Some("tei" | "xml") => "application/tei+xml",
        Some("idml") => "application/vnd.adobe.indesign-idml-package",
        _ => "application/octet-stream",
    }
}
