use designcraft_engine::Session;
use serde_json::json;

fn frame(s: &mut Session, text: &str) -> serde_json::Value {
    s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap()
}

#[test]
fn regression_failed_script_keeps_partial_edits_undoable() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let f = frame(&mut s, "Keep this text");
    s.doc_mut().unwrap().history.undo.clear();
    let script = json!([
        {"command": "edit.clear", "params": {"ids": [f["id"]]}},
        {"command": "no.such.command"}
    ]);
    assert!(s.execute("script.run", &json!({"text": script.to_string()})).is_err());
    let id = designcraft_doc::ItemId(f["id"].as_u64().unwrap());
    assert!(s.doc().unwrap().doc.item(id).is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.item(id).is_some());
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.item(id).is_none());
}

#[test]
fn regression_dirty_close_keeps_document_and_recovery_until_discard() {
    let dir = std::env::temp_dir().join(format!("dc-regression-close-{}", std::process::id()));
    let mut s = Session::new();
    s.recovery_dir = Some(dir.clone());
    s.execute("file.new", &json!({})).unwrap();
    frame(&mut s, "Unsaved text");
    s.execute("file.recovery.save", &json!({})).unwrap();
    assert_eq!(s.execute("file.recovery.list", &json!({})).unwrap().as_array().unwrap().len(), 1);
    assert!(s.execute("file.close", &json!({})).is_err());
    assert_eq!(s.documents().len(), 1);
    assert_eq!(s.execute("file.recovery.list", &json!({})).unwrap().as_array().unwrap().len(), 1);
    assert!(!s.doc().unwrap().history.undo.is_empty());
    s.execute("file.close", &json!({"discard": true})).unwrap();
    assert!(s.documents().is_empty());
    assert!(s.execute("file.recovery.list", &json!({})).unwrap().as_array().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn regression_book_checks_all_write_targets_before_overwriting_idml() {
    let dir = std::env::temp_dir().join(format!("dc-regression-book-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let native = dir.join("first.designcraft");
    let idml = dir.join("last.IDML");
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    frame(&mut s, "Chapter text");
    s.execute("file.saveAs", &json!({"path": native})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Only in IDML"})).unwrap();
    s.execute("file.exportIdml", &json!({"path": idml})).unwrap();
    let originals = [std::fs::read(&native).unwrap(), std::fs::read(&idml).unwrap()];
    s.execute("book.new", &json!({"path": dir.join("book.dcbook")})).unwrap();
    s.execute("book.add", &json!({"path": native})).unwrap();
    s.execute("book.add", &json!({"path": idml})).unwrap();
    for command in ["book.paginate", "book.syncStyles"] {
        assert!(s.execute(command, &json!({})).is_err(), "{command} must preserve IDML originals");
        assert_eq!(std::fs::read(&native).unwrap(), originals[0]);
        assert_eq!(std::fs::read(&idml).unwrap(), originals[1]);
        assert_eq!(s.execute("book.list", &json!({})).unwrap()["documents"][1]["pages"], 1);
    }
    // An IDML chapter can still supply styles when only native chapters are written.
    s.execute("book.styleSource", &json!({"index": 1})).unwrap();
    assert_eq!(s.execute("book.syncStyles", &json!({})).unwrap()["synced"], 1);
    assert_eq!(std::fs::read(&idml).unwrap(), originals[1]);
    s.execute("file.open", &json!({"path": native})).unwrap();
    assert!(s.doc().unwrap().doc.styles.para("Only in IDML").is_some());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn regression_baseline_grid_invalidates_composition() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let f = frame(&mut s, "First line\nSecond line");
    s.execute("text.select", &json!({"story": f["story"], "anchor": 0, "focus": 1})).unwrap();
    s.execute("type.para", &json!({"gridAlign": "allLines"})).unwrap();
    let sid = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
    let first_y = |cs: &designcraft_compose::ComposedStory| cs.frames[0].lines[0].baseline;
    let old = s.cache.get(&s.doc().unwrap().doc, sid, None);
    s.execute("document.preferences", &json!({"baselineGrid": {"start": 7, "increment": 37}})).unwrap();
    let fresh = designcraft_compose::Cache::new().get(&s.doc().unwrap().doc, sid, None);
    let updated = s.cache.get(&s.doc().unwrap().doc, sid, None);
    assert_ne!(first_y(&old), first_y(&fresh), "fixture must change the text position");
    assert_eq!(first_y(&updated), first_y(&fresh));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(first_y(&s.cache.get(&s.doc().unwrap().doc, sid, None)), first_y(&old));
}

#[test]
fn regression_mixed_running_headers_and_xrefs_terminate() {
    const CHILD: &str = "DESIGNCRAFT_MIXED_INDEX_REGRESSION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "regression_mixed_running_headers_and_xrefs_terminate", "--nocapture"])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if child.try_wait().unwrap().is_some() {
                let output = child.wait_with_output().unwrap();
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                return;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("composition did not terminate: {}", String::from_utf8_lossy(&output.stderr));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Heading"})).unwrap();
    let a = frame(&mut s, "Chapter");
    s.execute("text.select", &json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
    s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
    s.execute("variables.define", &json!({"name": "Header", "type": "runningHeader", "style": "Heading"})).unwrap();
    let b = frame(&mut s, "");
    s.execute("text.select", &json!({"story": b["story"], "anchor": 0, "focus": 0})).unwrap();
    s.execute("variables.insert", &json!({"name": "Header"})).unwrap();
    let anchor = s.execute("anchor.create", &json!({"name": "header-anchor"})).unwrap();
    s.execute("text.select", &json!({"story": a["story"], "anchor": 7, "focus": 7})).unwrap();
    s.execute("xref.insert", &json!({"anchor": anchor["id"]})).unwrap();
    for first in [a["story"].as_u64().unwrap(), b["story"].as_u64().unwrap()] {
        let cache = designcraft_compose::Cache::new();
        let doc = &s.doc().unwrap().doc;
        assert!(!cache.get(doc, designcraft_doc::StoryId(first), None).frames.is_empty());
        assert_eq!(cache.xref_index(doc).page(anchor["id"].as_u64().unwrap()), Some("1"));
        assert_eq!(cache.running_index(doc).value("Heading", false, Default::default(), 0), "Chapter");
    }
    assert!(s.execute("document.inspect", &json!({})).is_ok());
}
