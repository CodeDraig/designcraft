#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use designcraft_doc::{StoryId, TextSel};

pub(super) fn publication() -> (Session, StoryId, ComponentId, EditionId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"title":"Original"})).unwrap();
    let result = s.execute("frame.create", &json!({"rect":[40,40,400,600],"content":"text","text":"Original text"})).unwrap();
    let sid = StoryId(result["story"].as_u64().unwrap());
    let result = s.execute("publication.convert", &json!({"title":"Book"})).unwrap();
    (s, sid, ComponentId(result["component"].as_u64().unwrap()), EditionId(result["edition"].as_u64().unwrap()))
}

#[test]
fn edits_are_shared_between_editions_and_undo_restores_the_entire_root() {
    let (mut s, sid, component, print) = publication();
    let result = s.execute("publication.edition.create", &json!({"name":"Reading"})).unwrap();
    let reading = EditionId(result["edition"].as_u64().unwrap());
    let original = s.doc().unwrap().doc.clone();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":8})).unwrap();
    s.execute("text.insert", &json!({"text":"Edited"})).unwrap();
    s.execute("publication.activate", &json!({"edition":print})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Edited text");
    assert_eq!(s.documents()[0].doc.story(sid).unwrap().text, "Original text");
    s.execute("edit.undo", &json!({})).unwrap();
    let v = view(&s).unwrap();
    assert_eq!(v.edition, reading);
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Original text");
    assert_eq!(v.root, original.publication.as_ref().unwrap().root);
    assert_eq!(s.doc().unwrap().selection.text.unwrap().range(), 0..8);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(view(&s).unwrap().root.project(print, component).unwrap().story(sid).unwrap().text, "Edited text");
}

#[test]
fn failed_import_and_cyclic_move_preserve_document_selection_and_history() {
    let (mut s, sid, component, _) = publication();
    s.doc_mut().unwrap().selection = Selection::text(TextSel { story: sid, anchor: 2, focus: 4, frame: None, cell: None });
    let before = s.doc().unwrap().clone();
    let journal = s.journal.clone();
    for (command, params) in [
        ("publication.addComponent", json!({"name":"broken.designcraft","base64":"Z2FyYmFnZQ=="})),
        ("publication.moveComponent", json!({"component":component,"parent":component})),
        ("publication.activate", json!({"component":99999})),
    ] {
        assert!(s.execute(command, &params).is_err());
        let after = s.doc().unwrap();
        assert!(Arc::ptr_eq(&after.doc, &before.doc));
        assert_eq!(after.selection, before.selection);
        assert_eq!(after.history.undo.len(), before.history.undo.len());
        assert_eq!(after.history.redo.len(), before.history.redo.len());
        assert_eq!(after.revision, before.revision);
        assert_eq!(s.journal, journal);
    }
}

#[test]
fn component_navigation_is_clean_after_save_and_repeated_imports_have_new_ids() {
    let (mut s, _, first, _) = publication();
    let bytes = designcraft_format::save(&s.documents()[0].doc).unwrap();
    let params = json!({"name":"same.designcraft","base64":super::super::base64_encode(&bytes)});
    let second = s.execute("publication.addComponent", &params).unwrap()["component"].clone();
    let third = s.execute("publication.addComponent", &params).unwrap()["component"].clone();
    assert_ne!(second, third);
    let root = view(&s).unwrap().root;
    let all: std::collections::BTreeSet<_> = root.components.values().flat_map(|c| c.stories.values()).collect();
    assert_eq!(all.len(), 3);
    s.doc_mut().unwrap().saved_doc = s.doc().unwrap().doc.clone();
    assert!(!s.doc().unwrap().is_dirty());
    s.execute("publication.activate", &json!({"component":first})).unwrap();
    assert!(!s.doc().unwrap().is_dirty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(view(&s).unwrap().root.components.len(), 2);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(view(&s).unwrap().root.components.len(), 3);
}

#[test]
fn bytes_roundtrip_keeps_the_publication_title_and_inactive_chapters() {
    let (mut s, _, _, _) = publication();
    s.execute("publication.addComponent", &json!({"title":"Chapter two","role":"backMatter"})).unwrap();
    let serialized = s.execute("file.serialize", &json!({})).unwrap();
    let original = view(&s).unwrap();
    s.execute("file.openBytes", &json!({"name":"download.designcraft","base64":serialized["base64"]})).unwrap();
    let reopened = view(&s).unwrap();
    assert_eq!(reopened.root, original.root);
    assert_eq!(s.doc().unwrap().title(), "Book");
    assert_eq!(s.doc().unwrap().doc.title, "Chapter two");
    assert!(s.doc().unwrap().path.is_none());
}

#[test]
fn byte_book_conversion_is_all_or_nothing() {
    let (mut s, _, _, _) = publication();
    let chapter = super::super::base64_encode(&designcraft_format::save(&s.documents()[0].doc).unwrap());
    let book = super::super::base64_encode(br#"{"documents":["one.designcraft","two.designcraft"]}"#);
    let count = s.documents().len();
    let mut params = json!({"base64":book,"name":"book.dcbook","documents":{"one.designcraft":{"name":"one.designcraft","base64":chapter}}});
    assert!(s.execute("publication.importBook", &params).is_err());
    assert_eq!(s.documents().len(), count);
    params["documents"]["two.designcraft"] = params["documents"]["one.designcraft"].clone();
    s.execute("publication.importBook", &params).unwrap();
    assert_eq!(s.documents().len(), count + 1);
    assert_eq!(view(&s).unwrap().root.components.len(), 2);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn save_recovery_and_failed_save_preserve_complete_publications() {
    let (mut s, _, first, _) = publication();
    s.execute("publication.addComponent", &json!({"title":"Second"})).unwrap();
    let dir = std::env::temp_dir().join(format!("dc-publication-{}-{}", std::process::id(), s.doc().unwrap().uid));
    std::fs::create_dir(&dir).unwrap();
    assert_eq!(crate::recovery::save(&s, &dir).unwrap(), 2); // edited standalone original and publication
    let mut recovered = Session::new();
    crate::recovery::open(&mut recovered, &dir).unwrap();
    assert_eq!(view(&recovered).unwrap().root.components.len(), 2);
    assert!(recovered.doc().unwrap().is_dirty());
    let file = dir.join("book.designcraft");
    s.execute("file.save", &json!({"path":file})).unwrap();
    let original_bytes = std::fs::read(&file).unwrap();
    s.execute("publication.activate", &json!({"component":first})).unwrap();
    assert!(!s.doc().unwrap().is_dirty());
    s.execute("publication.component", &json!({"title":"Changed"})).unwrap();
    let before = s.doc().unwrap().clone();
    assert!(s.execute("file.save", &json!({"path":dir})).is_err());
    assert_eq!(s.doc().unwrap().path, before.path);
    assert!(Arc::ptr_eq(&s.doc().unwrap().saved_doc, &before.saved_doc));
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before.doc));
    assert_eq!(std::fs::read(&file).unwrap(), original_bytes);
    s.execute("file.open", &json!({"path":file})).unwrap();
    assert_eq!(view(&s).unwrap().root.components.len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}
