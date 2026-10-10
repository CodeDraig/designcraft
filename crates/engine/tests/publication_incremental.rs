#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "support/publication_fixture.rs"]
mod fixture;
use serde_json::json;
use std::sync::Arc;
#[test]
fn publication_incremental_dependencies_namespaces_and_oracle_are_transactional() {
    let mut s = fixture::session(fixture::book(3, 14));
    s.execute("publication.update", &json!({})).unwrap();
    assert_eq!(fixture::page_count(&s.doc().unwrap().doc), 42);
    s.cache.clear(); // Reopening a resolved archive starts without rendered-field memo entries.
    s.execute("publication.layout.verify", &json!({})).unwrap();
    let saved = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    let sid = s.doc().unwrap().doc.settings.primary_story.unwrap();
    let at = s.doc().unwrap().doc.story(sid).unwrap().para_ranges()[7].start + designcraft_doc::ANCHOR_MARK.len_utf8();
    s.execute("publication.layout.metrics", &json!({"reset":true})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":at,"focus":at+7})).unwrap();
    s.execute("text.insert", &json!({"text":"Updated"})).unwrap();
    let m = s.execute("publication.layout.metrics", &json!({})).unwrap();
    assert!(m["incremental"].as_u64().unwrap() > 0, "{m}");
    assert!(m["semanticHits"].as_u64().unwrap() > 0, "{m}");
    assert!(m["referenceHits"].as_u64().unwrap() > 0, "{m}");
    assert_eq!(m["referenceChanges"], 0, "unchanged persisted values are not changes: {m}");
    assert!(m["paragraphs"].as_u64().unwrap() < 15, "{m}");
    let before = s.doc().unwrap().clone();
    assert_eq!(s.execute("publication.layout.verify", &json!({})).unwrap()["equivalent"], true);
    assert!(Arc::ptr_eq(&before.doc, &s.doc().unwrap().doc));
    assert_eq!(before.history.undo.len(), s.doc().unwrap().history.undo.len());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, saved);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    s.execute("publication.layout.verify", &json!({})).unwrap();
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("publication.layout.verify", &json!({})).unwrap();
    let chapter = fixture::chapter(99, 2);
    let bytes = designcraft_format::save(&chapter).unwrap();
    let path = std::env::temp_dir().join(format!("designcraft-r8-chapter-{}.designcraft", s.doc().unwrap().uid));
    std::fs::write(&path, bytes).unwrap();
    let id = s.execute("publication.addComponent", &json!({"path":path})).unwrap()["component"].clone();
    s.execute("publication.moveComponent", &json!({"component":id,"at":0})).unwrap();
    assert_eq!(fixture::page_count(&s.doc().unwrap().doc), 44);
    s.execute("publication.layout.verify", &json!({})).unwrap();
    let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let d = designcraft_format::load(&bytes).unwrap();
    let v = d.publication.as_ref().unwrap();
    let projected = v.project().unwrap();
    assert_eq!(projected.title, d.title);
}

#[test]
fn rich_note_table_and_apparatus_fallbacks_match_full_and_report_their_work() {
    let mut s = designcraft_engine::Session::new();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../textimport/tests/fixtures/tei-notes-apparatus.xml");
    s.execute("file.openTei", &json!({"path":fixture})).unwrap();
    s.execute("publication.convert", &json!({})).unwrap();
    s.execute("publication.update", &json!({})).unwrap();
    let report = s.execute("publication.layout.verify", &json!({})).unwrap();
    assert_eq!(report["equivalent"], true);
    let m = s.execute("publication.layout.metrics", &json!({})).unwrap();
    assert!(m["events"].as_array().unwrap().iter().any(|e| e["reason"] == "note continuation and numbering"));
    let before = s.doc().unwrap().doc.clone();
    s.execute("publication.layout.metrics", &json!({"reset":true})).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}
