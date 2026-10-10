#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::cmd::publication::tests::publication;
use designcraft_doc::{StoryId, Table};

fn setup(s: &mut Session, index: &str) -> u64 {
    s.execute("publication.index.define", &json!({"name":index,"format":{"sectionHeadings":false}})).unwrap();
    s.execute("publication.index.topic", &json!({"index":index,"display":"Cymru"})).unwrap()["topic"].as_u64().unwrap()
}
#[test]
fn multiple_indexes_persist_independent_destinations_and_preview_has_no_side_effects() {
    let (mut s, sid, _, _) = publication();
    for index in ["Subjects", "Names"] {
        let topic = setup(&mut s, index);
        s.execute("publication.index.mark", &json!({"index":index,"topic":topic,"story":sid,"start":0})).unwrap();
        s.execute("publication.index.generate", &json!({"index":index})).unwrap();
    }
    let before = s.doc().unwrap().clone();
    let p = s.execute("publication.index.preview", &json!({"index":"Subjects"})).unwrap();
    assert_eq!(p["rows"][0]["text"]["runs"][0]["text"], "Cymru");
    assert!(p["occurrences"][0]["problem"].is_null());
    assert!(Arc::ptr_eq(&before.doc, &s.doc().unwrap().doc));
    assert_eq!(before.history.undo.len(), s.doc().unwrap().history.undo.len());
    let root = view(&s).unwrap().root;
    let a = root.indexes["Subjects"].destination.as_ref().unwrap();
    let b = root.indexes["Names"].destination.as_ref().unwrap();
    assert_ne!(a.story, b.story);
    let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let reopened = designcraft_format::load(&bytes).unwrap();
    assert_eq!(reopened.publication.as_ref().unwrap().root.indexes, root.indexes);
    assert_eq!(reopened.publication_field(*root.indexes["Subjects"].fields.values().next().unwrap(), None).plain(), "Cymru  1");
}
#[test]
fn indexed_range_shifts_with_edits_reports_deletion_and_undo_restores_everything() {
    let (mut s, sid, _, _) = publication();
    let topic = setup(&mut s, "Subjects");
    let id =
        s.execute("publication.index.mark", &json!({"index":"Subjects","topic":topic,"story":sid,"start":0,"end":8})).unwrap()["occurrence"].clone();
    s.execute("publication.index.generate", &json!({"index":"Subjects"})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    s.execute("text.insert", &json!({"text":"Before "})).unwrap();
    let p = s.execute("publication.index.preview", &json!({"index":"Subjects"})).unwrap();
    assert_eq!(p["occurrences"][0]["id"], id);
    assert_eq!(p["occurrences"][0]["excerpt"], "Original");
    s.execute("text.select", &json!({"story":sid,"anchor":10,"focus":18})).unwrap();
    let before = s.doc().unwrap().clone();
    s.execute("text.insert", &json!({"text":""})).unwrap();
    let p = s.execute("publication.index.preview", &json!({"index":"Subjects"})).unwrap();
    assert!(p["occurrences"][0]["problem"].as_str().unwrap().contains("removed"));
    assert!(p["rows"][0]["text"]["unresolved"].as_array().unwrap().len() == 1);
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before.doc);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.execute("publication.index.preview", &json!({"index":"Subjects"})).unwrap(), p);
}
#[test]
fn nested_table_and_notes_have_distinct_anchors_and_resolve_to_their_printed_pages() {
    let (mut s, sid, _, _) = publication();
    let topic = setup(&mut s, "Sources");
    let (table, note1, note2) = s
        .edit(|d, _| {
            let st = d.story_mut(sid).unwrap();
            let a = st.insert_note(st.len(), "A first footnote", Default::default());
            let b = st.insert_note(st.len(), "A second footnote", Default::default());
            let id = 800;
            let mut t = Table::new(id, 1, 1, 0, 0, 300.);
            t.cell_mut(0, 0).unwrap().text = Story::with_text(StoryId(0), "A table passage", Default::default());
            st.insert_table(st.len(), t);
            Ok((id, a, b))
        })
        .unwrap();
    for path in
        [json!([{"type":"cell","table":table,"row":0,"col":0}]), json!([{"type":"footnote","id":note1}]), json!([{"type":"footnote","id":note2}])]
    {
        s.execute("publication.index.mark", &json!({"index":"Sources","topic":topic,"story":sid,"path":path,"start":0,"end":7})).unwrap();
    }
    let v = view(&s).unwrap();
    let anchors: std::collections::BTreeSet<_> = v.root.indexes["Sources"].targets().map(|(_, t)| t.anchor).collect();
    assert_eq!(anchors.len(), 6);
    let p = s.execute("publication.index.preview", &json!({"index":"Sources"})).unwrap();
    assert_eq!(p["occurrences"].as_array().unwrap().len(), 3);
    for o in p["occurrences"].as_array().unwrap() {
        assert!(o["problem"].is_null(), "{o}");
        assert_eq!(o["start"]["page"], 0);
    }
}
#[test]
fn conversion_migrates_legacy_ranges_aliases_and_generated_destination_once() {
    let (mut s, sid, _, _) = publication();
    s.execute("file.activate", &json!({"index":0})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    s.execute("index.addReference", &json!({"topics":["Wales","Language"],"range":"toEndOfStory"})).unwrap();
    s.execute("index.addReference", &json!({"topics":["Cymru"],"range":"see","target":"Wales"})).unwrap();
    s.execute("index.generate", &json!({"sectionHeadings":false})).unwrap();
    let original = s.doc().unwrap().doc.clone();
    let destination = original.index.as_ref().unwrap().story;
    s.execute("publication.convert", &json!({})).unwrap();
    let root = view(&s).unwrap().root;
    let index = &root.indexes["Index"];
    assert_eq!(index.occurrences.len(), 1);
    assert!(index.occurrences.values().next().unwrap().end.is_some());
    assert_eq!(index.destination.as_ref().unwrap().story, destination);
    assert!(index.topics.values().any(|t| t.display == "Cymru" && t.see.len() == 1));
    assert!(!s.doc().unwrap().doc.story(sid).unwrap().text.contains(designcraft_doc::INDEX_MARK));
    assert_eq!(s.documents()[0].doc, original);
    assert_eq!(s.execute("publication.index.migrate", &json!({})).unwrap()["migrated"], 0);
}
#[test]
fn invalid_sort_and_alias_cycle_preserve_selection_history_and_root() {
    let (mut s, _, _, _) = publication();
    let a = setup(&mut s, "Subjects");
    let b = s.execute("publication.index.topic", &json!({"index":"Subjects","display":"Alias","see":[a]})).unwrap()["topic"].clone();
    let before = s.doc().unwrap().clone();
    for (command, p) in [
        ("publication.index.topic", json!({"index":"Subjects","id":a,"see":[b]})),
        ("publication.index.define", json!({"name":"Subjects","sorting":"biblical","bookOrder":["John"]})),
    ] {
        assert!(s.execute(command, &p).is_err());
        assert!(Arc::ptr_eq(&before.doc, &s.doc().unwrap().doc));
        assert_eq!(before.selection, s.doc().unwrap().selection);
        assert_eq!(before.history.undo.len(), s.doc().unwrap().history.undo.len());
    }
}
#[test]
fn index_detach_keeps_visible_rich_text_and_edition_created_first_gains_frames() {
    let (mut s, sid, _, print) = publication();
    let reading = s.execute("publication.edition.create", &json!({"name":"Reading"})).unwrap()["edition"].clone();
    s.execute("publication.activate", &json!({"edition":print})).unwrap();
    let topic = setup(&mut s, "Subjects");
    s.execute("publication.index.mark", &json!({"index":"Subjects","topic":topic,"story":sid,"start":0})).unwrap();
    let result = s.execute("publication.index.generate", &json!({"index":"Subjects"})).unwrap();
    let sid = StoryId(result["story"].as_u64().unwrap());
    s.execute("publication.activate", &json!({"edition":reading})).unwrap();
    s.execute("publication.update", &json!({})).unwrap();
    assert!(!s.doc().unwrap().doc.story(sid).unwrap().frames.is_empty());
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    assert!(s.execute("text.insert", &json!({"text":"Unexpected"})).is_err());
    s.execute("publication.index.detach", &json!({"index":"Subjects"})).unwrap();
    let st = s.doc().unwrap().doc.story(sid).unwrap();
    assert!(st.text.contains("Cymru  1"));
    assert!(st.xrefs.is_empty());
    s.execute("text.insert", &json!({"text":"Editable "})).unwrap();
}

#[test]
fn cut_and_paste_between_components_moves_index_and_heading_identity_once() {
    let (mut s, sid, first, _) = publication();
    let topic = setup(&mut s, "Subjects");
    let heading = s.execute("publication.numbering.mark", &json!({"story":sid,"pos":0,"kind":"heading"})).unwrap()["object"].clone();
    let len = s.doc().unwrap().doc.story(sid).unwrap().len();
    let occurrence =
        s.execute("publication.index.mark", &json!({"index":"Subjects","topic":topic,"story":sid,"start":0,"end":len})).unwrap()["occurrence"]
            .as_u64()
            .unwrap();
    let len = s.doc().unwrap().doc.story(sid).unwrap().len();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":len})).unwrap();
    s.execute("edit.cut", &json!({})).unwrap();
    let second = s.execute("publication.addComponent", &json!({"title":"Moved destination"})).unwrap()["component"].clone();
    let frame = s.execute("frame.create", &json!({"content":"text","text":"","rect":[36,36,400,600]})).unwrap();
    s.execute("text.select", &json!({"story":frame["story"],"anchor":0,"focus":0})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let query = s.execute("publication.query", &json!({"query":{"ids":[heading]}})).unwrap();
    assert_eq!(query[0]["component"], second);
    assert_eq!(query[0]["resolved"], true);
    assert_eq!(query[0]["title"], "Original text");
    let p = s.execute("publication.index.preview", &json!({"index":"Subjects"})).unwrap();
    assert!(p["occurrences"][0]["problem"].is_null());
    assert_eq!(p["occurrences"][0]["start"]["component"], second);
    let moved = view(&s).unwrap().root.indexes["Subjects"].occurrences[&occurrence].clone();
    assert_ne!(moved.start.component, first);
    // Reusing the clipboard makes an independent copy, without moving the bound original again.
    s.execute("edit.paste", &json!({})).unwrap();
    assert_eq!(view(&s).unwrap().root.indexes["Subjects"].occurrences[&occurrence], moved);
    let st = s.doc().unwrap().doc.story(StoryId(frame["story"].as_u64().unwrap())).unwrap();
    let ids: std::collections::BTreeSet<_> = st.anchors.iter().map(|a| a.id).collect();
    assert_eq!(ids.len(), st.anchors.len());
}

#[test]
fn endnote_occurrences_keep_canonical_ownership_and_resolve_to_the_note_page() {
    let (mut s, sid, _, _) = publication();
    s.execute("text.select", &json!({"story":sid,"anchor":8,"focus":8})).unwrap();
    let note = s.execute("endnote.insert", &json!({"text":"Rich endnote passage"})).unwrap()["id"].clone();
    let topic = setup(&mut s, "Sources");
    s.execute(
        "publication.index.mark",
        &json!({"index":"Sources","topic":topic,"story":sid,"path":[{"type":"endnote","id":note}],"start":0,"end":12}),
    )
    .unwrap();
    let root = view(&s).unwrap().root;
    let expected = *root.components.values().find_map(|c| c.stories.get(&sid)).unwrap();
    assert_eq!(root.indexes["Sources"].occurrences.values().next().unwrap().start.content, expected);
    let p = s.execute("publication.index.preview", &json!({"index":"Sources"})).unwrap();
    assert!(p["occurrences"][0]["problem"].is_null(), "{p}");
    assert_eq!(p["occurrences"][0]["start"]["page"], 1);
}

#[test]
fn exclusive_range_endpoint_at_a_page_break_uses_the_preceding_page() {
    let (mut s, sid, _, _) = publication();
    let text = format!("First passage{}Second page", designcraft_doc::PAGE_BREAK);
    s.edit(|d, _| {
        let st = d.story_mut(sid).unwrap();
        st.replace(0..st.len(), &text);
        d.settings.primary_story = Some(sid);
        Ok(())
    })
    .unwrap();
    let topic = setup(&mut s, "Subjects");
    let end = "First passage".len() + designcraft_doc::PAGE_BREAK.len_utf8();
    s.execute("publication.index.mark", &json!({"index":"Subjects","topic":topic,"story":sid,"start":0,"end":end})).unwrap();
    let p = s.execute("publication.index.preview", &json!({"index":"Subjects"})).unwrap();
    assert_eq!(p["occurrences"][0]["start"]["page"], 0);
    assert_eq!(p["occurrences"][0]["end"]["page"], 0, "{p}");
    assert_eq!(s.doc().unwrap().doc.page_count(), 2);
}

#[test]
fn existing_index_commands_use_named_publication_state_and_one_undo_step() {
    let (mut s, sid, _, _) = publication();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":8})).unwrap();
    let old = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    s.execute("index.addReference", &json!({"index":"Subjects","topics":["Wales","Language"],"range":"toEndOfStory"})).unwrap();
    assert_eq!(view(&s).unwrap().root.indexes["Subjects"].topics.len(), 2);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    assert!(s.doc().unwrap().doc.story(sid).unwrap().index_refs.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, old);
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("index.generate", &json!({"index":"Subjects","runIn":true,"sectionHeadings":false})).unwrap();
    assert!(view(&s).unwrap().root.indexes["Subjects"].destination.is_some());
    let list = s.execute("index.list", &json!({"index":"Subjects"})).unwrap();
    assert_eq!(list[0]["index"], "Subjects");
    s.execute("index.update", &json!({})).unwrap();
    s.execute("publication.index.define", &json!({"name":"Subjects","rename":"Places"})).unwrap();
    assert!(view(&s).unwrap().root.indexes.contains_key("Places"));
    assert!(!view(&s).unwrap().root.indexes.contains_key("Subjects"));
}
