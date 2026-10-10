//! Synchronous and asynchronous host file delivery.

use super::{DesignApp, Value, dialogs, json};

impl DesignApp {
    /// Ask the host for a file to open (`open`) or place (`place`). Synchronous pickers run the
    /// command right away; asynchronous ones (web) deliver the file through the inbox.
    pub fn pick_and_open(&mut self, purpose: &str) -> Result<Value, String> {
        if matches!(purpose, "publicationAdd" | "manuscript") && self.session.active().is_none_or(|s| s.doc.publication.is_none()) {
            return Err("open a publication before importing a chapter".into());
        }
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
                Some(path) => self.receive_path(purpose, &path),
                None => Ok(Value::Null),
            };
        }
        if let Some(open) = self.services.open_async.as_mut() {
            let purpose = if purpose == "manuscript" {
                let st = self.session.doc().map_err(|e| e.to_string())?;
                let v = st.doc.publication.as_ref().ok_or("no publication")?;
                format!(
                    "manuscript:{}:{}:{}:{}:{}",
                    st.uid,
                    v.edition.0,
                    v.component.0,
                    st.selection.text.map(|t| t.story.0).or(st.doc.settings.primary_story.map(|s| s.0)).unwrap_or(0),
                    v.root.revision
                )
            } else if purpose == "publicationAdd" {
                format!("publicationAdd:{}", self.session.doc().map_err(|e| e.to_string())?.uid)
            } else {
                purpose.to_string()
            };
            open(&purpose);
        }
        Ok(Value::Null)
    }

    pub(crate) fn receive_path(&mut self, purpose: &str, path: &str) -> Result<Value, String> {
        let lower = path.to_ascii_lowercase();
        if purpose == "publicationBook" {
            return self.run("publication.importBook", json!({"path":path}));
        }
        if purpose == "tei" || lower.ends_with(".tei") || lower.ends_with(".xml") {
            return dialogs::tei::open(
                self,
                json!({"path":path}),
                if matches!(purpose, "place" | "publicationAdd" | "manuscript") { purpose } else { "open" },
            );
        }
        if purpose == "manuscript" {
            return dialogs::publication::open(self, &json!({"mode":"manuscript","path":path}));
        }
        if purpose == "publicationAdd" {
            return self.run("publication.addComponent", json!({"path":path}));
        }
        self.run(if purpose == "place" { "file.place" } else { "file.open" }, json!({"path":path}))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn receive_dropped_paths(&mut self, paths: Vec<String>) {
        let tei: Vec<_> = paths
            .iter()
            .filter(|p| {
                let p = p.to_ascii_lowercase();
                p.ends_with(".tei") || p.ends_with(".xml")
            })
            .collect();
        if !tei.is_empty() {
            if tei.len() != 1 {
                self.status("Import one TEI book at a time.");
                return;
            }
            if let Err(e) = self.receive_path("open", tei[0]) {
                self.status(e);
                return;
            }
            if let Some(d) = self.ui.dialog.as_mut() {
                let requests = d.fields["info"]["resources"].as_array().cloned().unwrap_or_default();
                for request in requests {
                    let Some(url) = request["url"].as_str() else { continue };
                    let matches: Vec<_> =
                        paths.iter().filter(|p| std::path::Path::new(p).file_name().and_then(|s| s.to_str()) == url.rsplit('/').next()).collect();
                    if matches.len() == 1
                        && let Some(resources) = d.fields.get_mut("resources").and_then(Value::as_object_mut)
                    {
                        resources.insert(url.into(), json!({"path":matches[0]}));
                    }
                }
            }
            return;
        }
        for path in paths.iter().filter(|p| !p.is_empty()) {
            let lower = path.to_ascii_lowercase();
            let cmd = if lower.ends_with(".designcraft") || lower.ends_with(".idml") { "file.open" } else { "file.place" };
            if let Err(e) = self.run(cmd, json!({"path":path})) {
                self.status(e);
            }
        }
    }

    /// Open/Place intent survives asynchronous delivery; dropped images can accompany TEI.
    pub(super) fn drain_inbox(&mut self) {
        let Some(inbox) = self.services.inbox.clone() else { return };
        let batches = std::mem::take(&mut *inbox.lock().unwrap_or_else(|e| e.into_inner()));
        for mut batch in batches {
            let mut manuscript_story = None;
            if let Some(context) = batch.purpose.strip_prefix("manuscript:") {
                let parts: Vec<_> = context.split(':').map(str::parse::<u64>).collect();
                let context = parts.into_iter().collect::<Result<Vec<_>, _>>();
                let Ok(context) = context else {
                    self.status("Invalid manuscript import context.");
                    continue;
                };
                let [uid, edition, component, story, revision] = context.as_slice() else {
                    self.status("Invalid manuscript import context.");
                    continue;
                };
                let index = self
                    .session
                    .documents()
                    .iter()
                    .position(|s| s.uid == *uid && s.doc.publication.as_ref().is_some_and(|v| v.root.revision == *revision));
                let Some(index) = index else {
                    self.status("The publication changed or closed while choosing the manuscript. Choose the revised file again.");
                    continue;
                };
                if let Err(e) = self
                    .run("file.activate", json!({"index":index}))
                    .and_then(|_| self.run("publication.activate", json!({"edition":edition,"component":component})))
                {
                    self.status(e);
                    continue;
                }
                manuscript_story = (*story > 0).then_some(*story);
                batch.purpose = "manuscript".into();
            }
            if batch.purpose == "recovery" {
                let mut count = 0;
                for (name, bytes) in batch.files {
                    match self.run("file.recoverBytes", json!({"base64":designcraft_engine::cmd::base64_encode(&bytes)})) {
                        Ok(_) => count += 1,
                        Err(e) => self.status(format!("Could not recover {name}: {e}")),
                    }
                }
                if count > 0 {
                    self.status(format!("Recovered {count} unsaved document(s). Older recovery copies are retained in browser storage."));
                }
                continue;
            }
            if let Some(uid) = batch.purpose.strip_prefix("publicationAdd:").and_then(|s| s.parse::<u64>().ok()) {
                let index = self.session.documents().iter().position(|s| s.uid == uid);
                let Some(index) = index else {
                    self.status("The publication chosen for this import is no longer open.");
                    continue;
                };
                if let Err(e) = self.run("file.activate", json!({"index":index})) {
                    self.status(e);
                    continue;
                }
                batch.purpose = "publicationAdd".into();
            }
            if batch.purpose == "publicationBook" {
                if let Err(e) = self.receive_book_bytes(&batch.files) {
                    self.status(e);
                }
                continue;
            }
            if let Some(url) = batch.purpose.strip_prefix("teiResource:") {
                if let Some((name, bytes)) = batch.files.first()
                    && let Some(d) = self.ui.dialog.as_mut().filter(|d| d.id == "teiImport")
                    && let Some(resources) = d.fields.get_mut("resources").and_then(Value::as_object_mut)
                {
                    resources.insert(url.into(), json!({"name":name,"base64":designcraft_engine::cmd::base64_encode(bytes)}));
                }
                continue;
            }
            let tei_files: Vec<_> = batch
                .files
                .iter()
                .filter(|(name, bytes)| {
                    let name = name.to_ascii_lowercase();
                    name.ends_with(".tei") || name.ends_with(".xml") || designcraft_textimport::tei::is_tei(bytes)
                })
                .collect();
            if !tei_files.is_empty() {
                if tei_files.len() != 1 {
                    self.status("Import one TEI book at a time; its illustrations may be dropped with it.");
                    continue;
                }
                let (name, bytes) = tei_files[0];
                let mut params = json!({"name":name,"base64":designcraft_engine::cmd::base64_encode(bytes)});
                if let Some(story) = manuscript_story {
                    params["story"] = json!(story);
                }
                let result = dialogs::tei::open(
                    self,
                    params,
                    if matches!(batch.purpose.as_str(), "place" | "publicationAdd" | "manuscript") { &batch.purpose } else { "open" },
                );
                if let Err(e) = result {
                    self.status(e);
                    continue;
                }
                if let Some(d) = self.ui.dialog.as_mut() {
                    let requests = d.fields.get("info").and_then(|i| i.get("resources")).and_then(Value::as_array).cloned().unwrap_or_default();
                    for request in requests {
                        let Some(url) = request["url"].as_str() else { continue };
                        let matches: Vec<_> = batch.files.iter().filter(|(n, _)| n == url || n == url.rsplit('/').next().unwrap_or(url)).collect();
                        if matches.len() == 1
                            && let Some(resources) = d.fields.get_mut("resources").and_then(Value::as_object_mut)
                        {
                            let (name, bytes) = matches[0];
                            resources.insert(url.into(), json!({"name":name,"base64":designcraft_engine::cmd::base64_encode(bytes)}));
                        }
                    }
                }
                continue;
            }
            for (name, bytes) in batch.files {
                let b64 = designcraft_engine::cmd::base64_encode(&bytes);
                let lower = name.to_ascii_lowercase();
                let r = if batch.purpose == "manuscript" {
                    let mut p = json!({"mode":"manuscript","name":name,"base64":b64});
                    if let Some(story) = manuscript_story {
                        p["story"] = json!(story);
                    }
                    dialogs::publication::open(self, &p)
                } else if batch.purpose == "publicationAdd" {
                    self.run("publication.addComponent", json!({"name":name,"base64":b64}))
                } else if batch.purpose == "swatches" || lower.ends_with(".ase") {
                    self.run("swatch.load", json!({"base64":b64}))
                } else if batch.purpose == "open"
                    || batch.purpose == "tei"
                    || batch.purpose == "drop" && (lower.ends_with(".designcraft") || lower.ends_with(".idml"))
                {
                    self.run("file.openBytes", json!({"name":name,"base64":b64}))
                } else {
                    self.run("file.place", json!({"name":name,"base64":b64}))
                };
                if let Err(e) = r {
                    self.status(format!("{name}: {e}"));
                }
            }
        }
    }

    fn receive_book_bytes(&mut self, files: &[(String, Vec<u8>)]) -> Result<Value, String> {
        let books: Vec<_> = files.iter().filter(|(n, _)| n.to_ascii_lowercase().ends_with(".dcbook")).collect();
        if books.len() != 1 {
            return Err("Choose one .dcbook file together with its chapter files.".into());
        }
        let (name, bytes) = books[0];
        let book: designcraft_engine::cmd::book::Book = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let mut documents = serde_json::Map::new();
        for path in &book.documents {
            let matches: Vec<_> = files.iter().filter(|(n, _)| n == path || Some(n.as_str()) == path.rsplit('/').next()).collect();
            if matches.is_empty() {
                return Err(format!("Missing chapter {path}. Choose the Book file and all its chapters together."));
            }
            if matches.len() > 1 {
                return Err(format!("More than one selected file matches {path}. Choose a single copy of each chapter."));
            }
            let (name, bytes) = matches[0];
            documents.insert(path.clone(), json!({"name":name,"base64":designcraft_engine::cmd::base64_encode(bytes)}));
        }
        self.run("publication.importBook", json!({"name":name,"base64":designcraft_engine::cmd::base64_encode(bytes),"documents":documents}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeliveredFiles, Inbox, Services};
    const XML: &[u8] =
        br#"<TEI xmlns="http://www.tei-c.org/ns/1.0"><text><body><p>Browser book.<note place="end">A browser note.</note></p></body></text></TEI>"#;
    #[test]
    fn browser_delivery_keeps_open_and_place_intent() {
        for purpose in ["open", "place"] {
            let inbox = Inbox::default();
            let mut app = DesignApp::new(designcraft_engine::Session::new(), Services { inbox: Some(inbox.clone()), ..Default::default() });
            app.run("file.new", json!({})).unwrap();
            inbox.lock().unwrap().push(DeliveredFiles { purpose: purpose.into(), files: vec![("book.tei".into(), XML.to_vec())] });
            app.drain_inbox();
            assert_eq!(app.ui.dialog.as_ref().unwrap().id, "teiImport");
            assert_eq!(app.ui.dialog.as_ref().unwrap().fields["purpose"], purpose);
            dialogs::confirm(&mut app).unwrap();
            assert_eq!(app.session.documents().len(), if purpose == "open" { 2 } else { 1 });
            assert_eq!(app.ui.dialog.as_ref().unwrap().id, "teiReport");
            assert_eq!(app.session.doc().unwrap().doc.tei_sources.len(), 1);
        }
    }
    #[test]
    fn ambiguous_dropped_illustrations_require_explicit_mapping() {
        let xml = br#"<TEI xmlns="http://www.tei-c.org/ns/1.0"><text><body><figure><graphic url="plates/image.svg"/></figure></body></text></TEI>"#;
        let inbox = Inbox::default();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services { inbox: Some(inbox.clone()), ..Default::default() });
        inbox.lock().unwrap().push(DeliveredFiles {
            purpose: "drop".into(),
            files: vec![("book.xml".into(), xml.to_vec()), ("image.svg".into(), b"first".to_vec()), ("image.svg".into(), b"second".to_vec())],
        });
        app.drain_inbox();
        assert!(app.ui.dialog.as_ref().unwrap().fields["resources"].get("plates/image.svg").is_none());
        assert!(app.session.documents().is_empty());
    }

    #[test]
    fn browser_resource_delivery_updates_only_explicit_uri() {
        let inbox = Inbox::default();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services { inbox: Some(inbox.clone()), ..Default::default() });
        dialogs::tei::open(&mut app, json!({"name":"book.tei","base64":designcraft_engine::cmd::base64_encode(XML)}), "open").unwrap();
        inbox
            .lock()
            .unwrap()
            .push(DeliveredFiles { purpose: "teiResource:images/plate.svg".into(), files: vec![("different.svg".into(), b"resource".to_vec())] });
        app.drain_inbox();
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(d.fields["resources"].get("images/plate.svg").is_some());
        assert!(app.session.documents().is_empty());
    }

    #[test]
    fn browser_publication_import_returns_to_its_original_target_and_downloads_all_chapters() {
        let inbox = Inbox::default();
        let downloads = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<u8>)>::new()));
        let output = downloads.clone();
        let mut app = DesignApp::new(
            designcraft_engine::Session::new(),
            Services {
                inbox: Some(inbox.clone()),
                download: Some(Box::new(move |name, bytes| output.lock().unwrap().push((name.into(), bytes.to_vec())))),
                ..Default::default()
            },
        );
        app.run("publication.new", json!({"title":"Browser Publication"})).unwrap();
        let uid = app.session.doc().unwrap().uid;
        app.run("file.new", json!({"title":"Unrelated"})).unwrap();
        inbox.lock().unwrap().push(DeliveredFiles { purpose: format!("publicationAdd:{uid}"), files: vec![("chapter.tei".into(), XML.to_vec())] });
        app.drain_inbox();
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields["purpose"], "publicationAdd");
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.doc().unwrap().uid, uid);
        assert_eq!(app.session.doc().unwrap().doc.publication.as_ref().unwrap().root.components.len(), 2);
        app.run("app.save", json!({})).unwrap();
        assert!(!app.session.doc().unwrap().is_dirty());
        let files = downloads.lock().unwrap();
        assert_eq!(files[0].0, "Browser Publication.designcraft");
        let decoded = designcraft_engine::cmd::file_from(&files[0].1).unwrap();
        assert_eq!(decoded.publication.as_ref().unwrap().root.components.len(), 2);
        assert_eq!(decoded.tei_sources[0].bytes.as_ref(), XML);
        assert_eq!(app.session.documents()[1].doc.title, "Unrelated");
    }

    #[test]
    fn browser_book_conversion_reads_named_chapters_as_one_transaction() {
        let inbox = Inbox::default();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services { inbox: Some(inbox.clone()), ..Default::default() });
        app.run("file.new", json!({"title":"Chapter"})).unwrap();
        let bytes = designcraft_engine::cmd::file_bytes(&app.session.doc().unwrap().doc);
        inbox.lock().unwrap().push(DeliveredFiles {
            purpose: "publicationBook".into(),
            files: vec![
                ("book.dcbook".into(), br#"{"documents":["one.designcraft","two.designcraft"]}"#.to_vec()),
                ("one.designcraft".into(), bytes.clone()),
                ("two.designcraft".into(), bytes),
            ],
        });
        app.drain_inbox();
        assert_eq!(app.session.doc().unwrap().doc.publication.as_ref().unwrap().root.components.len(), 2);
        assert_eq!(app.session.documents().len(), 2);
    }

    #[test]
    fn delayed_manuscript_delivery_keeps_chapter_and_rejects_changed_publications() {
        for changed in [false, true] {
            let inbox = Inbox::default();
            let mut app = DesignApp::new(designcraft_engine::Session::new(), Services { inbox: Some(inbox.clone()), ..Default::default() });
            app.run("file.openTei", json!({"name":"original.tei","base64":designcraft_engine::cmd::base64_encode(XML)})).unwrap();
            app.run("publication.convert", json!({})).unwrap();
            let st = app.session.doc().unwrap();
            let v = st.doc.publication.clone().unwrap();
            let sid = st.doc.stories.keys().next().unwrap().0;
            let purpose = format!("manuscript:{}:{}:{}:{}:{}", st.uid, v.edition.0, v.component.0, sid, v.root.revision);
            if changed {
                app.run("publication.component", json!({"title":"Changed"})).unwrap();
            }
            app.run("file.new", json!({"title":"Other document"})).unwrap();
            let original = app.session.documents()[1].doc.clone();
            inbox.lock().unwrap().push(DeliveredFiles { purpose, files: vec![("revision.tei".into(), XML.to_vec())] });
            app.drain_inbox();
            assert_eq!(app.session.documents().len(), 3);
            if changed {
                assert!(app.ui.dialog.is_none());
                assert!(app.ui.status.contains("changed or closed"));
            } else {
                let d = app.ui.dialog.as_ref().unwrap();
                assert_eq!(d.fields["purpose"], "manuscript");
                assert_eq!(d.fields["story"], sid);
                dialogs::confirm(&mut app).unwrap();
                let d = app.ui.dialog.as_ref().unwrap();
                assert_eq!(d.fields["mode"], "manuscript");
                assert_eq!(d.fields["report"]["component"], v.component.0);
                assert_eq!(app.session.doc().unwrap().doc, original);
            }
        }
    }
}
