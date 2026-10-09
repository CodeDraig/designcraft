//! Synchronous and asynchronous host file delivery.

use super::{DesignApp, Value, dialogs, json};

impl DesignApp {
    /// Ask the host for a file to open (`open`) or place (`place`). Synchronous pickers run the
    /// command right away; asynchronous ones (web) deliver the file through the inbox.
    pub fn pick_and_open(&mut self, purpose: &str) -> Result<Value, String> {
        let cmd = if purpose == "place" { "file.place" } else { "file.open" };
        if let Some(pick) = self.services.pick_open.as_mut() {
            return match pick(purpose) {
                // Image Import Options: a multi-page PDF asks which page.
                Some(path) if purpose == "place" && path.to_ascii_lowercase().ends_with(".pdf") => {
                    let pages =
                        self.services.read.as_mut().and_then(|r| r(&path).ok()).and_then(|b| designcraft_render::pdf_page_count(&b)).unwrap_or(1);
                    if pages > 1 {
                        self.ui.dialog = Some(dialogs::Dialog::new("pdfImport", json!({"path": path, "page": "1", "pages": pages})));
                        Ok(Value::Null)
                    } else {
                        self.run(cmd, json!({"path": path}))
                    }
                }
                Some(path) => self.run(cmd, json!({"path": path})),
                None => Ok(Value::Null),
            };
        }
        if let Some(open) = self.services.open_async.as_mut() {
            open(purpose);
        }
        Ok(Value::Null)
    }

    /// Open or place files delivered through the inbox.
    pub(super) fn drain_inbox(&mut self) {
        let Some(inbox) = self.services.inbox.clone() else { return };
        let files = std::mem::take(&mut *inbox.lock().unwrap_or_else(|e| e.into_inner()));
        for (name, bytes) in files {
            let b64 = designcraft_engine::cmd::base64_encode(&bytes);
            let lower = name.to_ascii_lowercase();
            let r = if lower.ends_with(".ase") {
                self.run("swatch.load", json!({"base64": b64}))
            } else if lower.ends_with(".designcraft") || lower.ends_with(".idml") {
                let title = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
                self.run("file.openBytes", json!({"name": title, "base64": b64}))
            } else {
                self.run("file.place", json!({"name": name, "base64": b64}))
            };
            if let Err(e) = r {
                self.status(format!("{name}: {e}"));
            }
        }
    }
}
