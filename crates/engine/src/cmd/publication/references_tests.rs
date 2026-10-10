#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::tests::publication;
use super::*;

#[test]
fn fields_follow_titles_and_reflow_and_undo_in_one_step() {
    let (mut s, sid, component, _) = publication();
    let heading = s.execute("publication.numbering.mark", &json!({"story":sid,"pos":0,"kind":"heading"})).unwrap()["object"].clone();
    let len = s.doc().unwrap().doc.story(sid).unwrap().len();
    s.execute("text.select", &json!({"story":sid,"anchor":len,"focus":len})).unwrap();
    s.execute("text.insert", &json!({"text":"\nSee "})).unwrap();
    let f = s.execute("publication.reference.insert", &json!({"query":{"ids":[heading]},"template":"Title and page"})).unwrap()["field"]
        .as_u64()
        .unwrap();
    assert_eq!(s.doc().unwrap().doc.publication_field(f, None).plain(), "Original text, p. 1");
    let old = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    s.execute("text.select", &json!({"story":sid,"anchor":3,"focus":11})).unwrap();
    s.execute("text.insert", &json!({"text":"Changed"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.publication_field(f, None).plain(), "Changed text, p. 1");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    let state = s.execute("publication.references", &json!({})).unwrap();
    assert_eq!(state["stale"], false);
    assert!(state["layout"]["passes"].as_u64().unwrap() <= 8);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, old);
    let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let d = designcraft_format::load(&bytes).unwrap();
    assert_eq!(d.publication_field(f, None).plain(), "Original text, p. 1");
    assert_eq!(d.publication.unwrap().component, component);
}

#[test]
fn deferred_mode_is_visible_and_export_forces_current_fields() {
    let (mut s, sid, component, _) = publication();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    let f = s.execute("publication.reference.insert", &json!({"query":{"ids":[component]},"template":"Title"})).unwrap()["field"].as_u64().unwrap();
    s.execute("publication.updateMode", &json!({"mode":"deferred"})).unwrap();
    s.execute("publication.component", &json!({"title":"New chapter"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.publication_field(f, None).plain(), "Original");
    assert_eq!(s.execute("publication.references", &json!({})).unwrap()["stale"], true);
    s.execute("file.exportPdf", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.publication_field(f, None).plain(), "New chapter");
    assert_eq!(s.execute("publication.references", &json!({})).unwrap()["stale"], false);
}

#[test]
fn sections_are_independent_bound_stories_and_reject_accidental_text_replacement() {
    let (mut s, sid, _, _) = publication();
    s.execute("publication.numbering.mark", &json!({"story":sid,"pos":0,"kind":"heading"})).unwrap();
    for name in ["Contents", "Another list"] {
        s.execute("publication.generated.define", &json!({"name":name,"query":{"kind":"heading"}})).unwrap();
    }
    let v = view(&s).unwrap();
    assert_eq!(v.root.references.sections.len(), 2);
    let a = v.root.references.sections["Contents"].clone();
    let b = v.root.references.sections["Another list"].clone();
    assert_ne!(a.story, b.story);
    assert_eq!(s.doc().unwrap().doc.story(a.story).unwrap().xrefs.len(), 1);
    s.execute("text.select", &json!({"story":a.story,"anchor":0,"focus":0})).unwrap();
    let before = s.doc().unwrap().clone();
    assert!(s.execute("text.insert", &json!({"text":"Oops"})).unwrap_err().to_string().contains("generated section"));
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before.doc));
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len());
    s.execute("publication.generated.detach", &json!({"name":"Contents"})).unwrap();
    s.execute("text.insert", &json!({"text":"Editable "})).unwrap();
    assert!(s.doc().unwrap().doc.story(a.story).unwrap().text.starts_with("Editable "));
}

#[test]
fn failed_fixed_point_edit_restores_document_selection_history_and_revision() {
    let (mut s, sid, component, _) = publication();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    s.execute("publication.reference.insert", &json!({"query":{"ids":[component]},"template":"Title"})).unwrap();
    let before = s.doc().unwrap().clone();
    let long = "long ".repeat(5000);
    let error = s.execute("publication.component", &json!({"title":long})).unwrap_err().to_string();
    assert!(error.contains("unplaced text"), "{error}");
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before.doc));
    assert_eq!(s.doc().unwrap().selection, before.selection);
    assert_eq!(s.doc().unwrap().revision, before.revision);
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len());
}

#[test]
fn sections_created_after_an_edition_get_independent_frames_and_reflowed_destinations() {
    let (mut s, sid, component, print) = publication();
    let reading = s.execute("publication.edition.create", &json!({"name":"Reading"})).unwrap()["edition"].clone();
    s.execute("publication.activate", &json!({"edition":print})).unwrap();
    s.execute("publication.numbering.mark", &json!({"story":sid,"pos":0,"kind":"heading"})).unwrap();
    let g = s.execute("publication.generated.define", &json!({"name":"Contents","query":{"kind":"heading"}})).unwrap();
    let generated = designcraft_doc::StoryId(g["story"].as_u64().unwrap());
    s.execute("publication.activate", &json!({"edition":reading})).unwrap();
    assert!(s.doc().unwrap().doc.story(generated).unwrap().frames.is_empty());
    s.execute("publication.update", &json!({})).unwrap();
    assert!(!s.doc().unwrap().doc.story(generated).unwrap().frames.is_empty());
    let v = view(&s).unwrap();
    assert_eq!(v.root.project(print, component).unwrap().page_count(), 2);
    assert_eq!(s.doc().unwrap().doc.page_count(), 2);
    assert!(s.doc().unwrap().doc.check().is_ok());
}

#[test]
fn registering_styles_is_idempotent_and_nested_targets_resolve() {
    let (mut s, sid, _, _) = publication();
    let rule = json!({"styles":[{"style":"[Basic Paragraph]","kind":"heading"}],"tei":false});
    assert_eq!(s.execute("publication.semantic.scan", &rule).unwrap()["added"], 1);
    assert_eq!(s.execute("publication.semantic.scan", &rule).unwrap()["added"], 0);
    let object = s.execute("publication.query", &json!({"query":{"kind":"heading"}})).unwrap()[0].clone();
    assert_eq!(object["title"], "Original text");
    let count = s.doc().unwrap().doc.story(sid).unwrap().anchors.len();
    assert_eq!(count, 1);
    // Invalid queries do not trigger layout or change the session.
    let old = s.doc().unwrap().doc.clone();
    assert!(s.execute("publication.query", &json!({"query":{"minLevel":7,"maxLevel":2}})).is_err());
    assert!(Arc::ptr_eq(&old, &s.doc().unwrap().doc));
}

#[test]
fn copying_fields_preserves_bindings_in_the_book_and_rejects_cross_book_aliasing() {
    let (mut s, sid, component, _) = publication();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    let f = s.execute("publication.reference.insert", &json!({"query":{"ids":[component]},"template":"Title"})).unwrap()["field"].as_u64().unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":3})).unwrap();
    assert_eq!(s.execute("edit.copy", &json!({})).unwrap()["text"], "Original");
    s.execute("text.select", &json!({"story":sid,"anchor":3,"focus":3})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().xrefs.iter().filter(|x| x.field == Some(f)).count(), 2);
    s.execute("file.activate", &json!({"index":0})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    assert!(s.execute("edit.paste", &json!({})).unwrap_err().to_string().contains("original book"));
    s.execute("edit.pasteWithoutFormatting", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.story(sid).unwrap().text.starts_with("OriginalOriginal"));
}

#[test]
fn threading_retains_the_semantic_destination_when_its_content_owner_changes() {
    let (mut s, sid, _, _) = publication();
    let first_frame = s.doc().unwrap().doc.story(sid).unwrap().frames[0];
    let next = s.execute("frame.create", &json!({"rect":[40,40,400,600],"content":"text","text":"A joined destination"})).unwrap();
    let second_story = next["story"].clone();
    let object = s.execute("publication.numbering.mark", &json!({"story":second_story,"pos":0,"kind":"heading"})).unwrap()["object"].clone();
    let old = s.execute("publication.query", &json!({"query":{"ids":[object]}})).unwrap();
    // Use the model's transaction through the same session edit boundary as threading commands.
    let next_frame = designcraft_doc::ItemId(next["id"].as_u64().unwrap());
    s.edit(|doc, _| {
        doc.thread(first_frame, next_frame)?;
        Ok(())
    })
    .unwrap();
    let now = s.execute("publication.query", &json!({"query":{"ids":[object]}})).unwrap();
    assert_eq!(now[0]["resolved"], true);
    assert_eq!(now[0]["title"], "A joined destination");
    assert_eq!(now[0]["id"], old[0]["id"]);
    assert_ne!(now[0]["target"]["content"], old[0]["target"]["content"]);
}
