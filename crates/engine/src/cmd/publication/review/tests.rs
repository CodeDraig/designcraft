#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::cmd::publication::tests::publication;
use designcraft_doc::{CellAddr, ComponentId, Table};

#[test]
fn publication_tracking_attributes_nested_and_inactive_chapters_and_resolves_atomically() {
    let (mut s, sid, first, edition) = publication();
    s.execute("changes.track", &json!({"on":true,"author":"Editor A"})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":8})).unwrap();
    s.execute("text.insert", &json!({"text":"Revised"})).unwrap();
    let result = s.execute("publication.addComponent", &json!({"title":"Second"})).unwrap();
    let second = ComponentId(result["component"].as_u64().unwrap());
    let frame = s.execute("frame.create", &json!({"rect":[40,40,400,600],"content":"text","text":"Second chapter"})).unwrap();
    let sid2 = StoryId(frame["story"].as_u64().unwrap());
    s.execute("changes.track", &json!({"on":true,"author":"Editor B"})).unwrap();
    s.execute("text.select", &json!({"story":sid2,"anchor":0,"focus":6})).unwrap();
    s.execute("text.insert", &json!({"text":"Another"})).unwrap();
    let changes = s.execute("changes.list", &json!({})).unwrap();
    let changes = changes.as_array().unwrap();
    assert_eq!(changes.len(), 4, "{changes:?}");
    assert!(changes.iter().any(|c| c["component"] == first.0 && c["author"] == "Editor A"));
    assert!(changes.iter().any(|c| c["component"] == second.0 && c["author"] == "Editor B"));
    let root = view(&s).unwrap().root;
    s.execute("changes.rejectAll", &json!({})).unwrap();
    let v = view(&s).unwrap();
    assert_eq!(v.root.project(edition, first).unwrap().story(sid).unwrap().text, "Original text");
    assert_eq!(v.root.project(edition, second).unwrap().story(sid2).unwrap().text, "Second chapter");
    assert!(v.root.change_ranges().unwrap().is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(view(&s).unwrap().root, root);
    let before = s.doc().unwrap().clone();
    assert!(s.execute("publication.review.resolve", &json!({"ids":[changes[0]["id"],999999],"accept":true})).is_err());
    assert!(Arc::ptr_eq(&before.doc, &s.doc().unwrap().doc));
    assert_eq!(before.history.undo.len(), s.doc().unwrap().history.undo.len());
    s.execute("changes.acceptAll", &json!({})).unwrap();
    assert_eq!(view(&s).unwrap().root.project(edition, first).unwrap().story(sid).unwrap().text, "Revised text");
    let saved = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let loaded = designcraft_format::load(&saved).unwrap();
    assert_eq!(loaded.publication.unwrap().root, view(&s).unwrap().root);
}

#[test]
fn anchored_comments_follow_edits_and_nested_review_selection_edits_the_leaf() {
    let (mut s, sid, _, _) = publication();
    let note = s.edit(|d, _| Ok(d.story_mut(sid).unwrap().insert_note(8, "A note to review", Default::default()))).unwrap();
    let id = s
        .execute(
            "publication.comment.add",
            &json!({"story":sid,"path":[{"type":"footnote","id":note}],"start":2,"end":6,"text":"Check this word","author":"Reviewer"}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let result = s.execute("publication.review.goTo", &json!({"id":id})).unwrap();
    assert_eq!(result["path"][0]["type"], "footnote");
    let t = s.doc().unwrap().selection.text.unwrap();
    assert_eq!(s.doc().unwrap().doc.text_story(t.story, t.cell).unwrap().text[t.range()], *"note");
    s.execute("changes.track", &json!({"on":true,"author":"Note editor"})).unwrap();
    s.execute("text.insert", &json!({"text":"remark"})).unwrap();
    let ranges = view(&s).unwrap().root.change_ranges().unwrap();
    assert_eq!(ranges.len(), 2);
    assert!(ranges.iter().all(|r| r.path == vec![PathStep::Footnote { id: note }] && r.author == "Note editor"));
    let deletion = ranges.iter().find(|r| r.kind == ChangeMark::Deleted).unwrap().id;
    s.execute("publication.review.goTo", &json!({"id":deletion})).unwrap();
    assert_eq!(s.doc().unwrap().selection.text.unwrap().cell, Some(CellAddr::review(deletion)));
    s.execute("changes.rejectAll", &json!({})).unwrap();
    s.execute("publication.review.goTo", &json!({"id":id})).unwrap();
    let t = s.doc().unwrap().selection.text.unwrap();
    assert_eq!(&s.doc().unwrap().doc.text_story(t.story, t.cell).unwrap().text[t.range()], "note");
    s.execute("changes.track", &json!({"on":false})).unwrap();
    s.execute("text.insert", &json!({"text":"new note"})).unwrap();
    s.execute("publication.review.goTo", &json!({"id":id})).unwrap();
    let t = s.doc().unwrap().selection.text.unwrap();
    assert_eq!(&s.doc().unwrap().doc.text_story(t.story, t.cell).unwrap().text[t.range()], "new note");
    let report = s.execute("publication.review.list", &json!({})).unwrap();
    assert_eq!(report["comments"][0]["comment"]["quote"], "note");
    assert_eq!(report["comments"][0]["unresolved"], false);
}

#[test]
fn proof_is_immutable_and_distinguishes_content_formatting_pagination_and_removed_index_passages() {
    let (mut s, sid, component, _) = publication();
    let proof = s.execute("publication.proof.create", &json!({"name":"First proof","author":"Proof reader"})).unwrap()["id"].clone();
    let baseline = view(&s).unwrap().root.review.proofs[&proof.as_u64().unwrap()].clone();
    assert!(!baseline.fonts.is_empty());
    assert!(!baseline.stories[0].frames[0].lines.is_empty());
    s.execute("publication.proof.approve", &json!({"id":proof,"note":"Approved for review"})).unwrap();
    assert!(s.execute("publication.proof.approve", &json!({"id":proof})).is_err());
    assert!(s.execute("publication.proof.create", &json!({"name":"First proof"})).is_err());
    let unchanged = s.execute("publication.proof.compare", &json!({"before":proof})).unwrap();
    assert!(unchanged["rows"].as_array().unwrap().is_empty(), "{unchanged}");
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":8})).unwrap();
    s.execute("text.insert", &json!({"text":"Different"})).unwrap();
    let diff = s.execute("publication.proof.compare", &json!({"before":proof})).unwrap();
    assert!(diff["rows"].as_array().unwrap().iter().any(|r| r["kind"] == "content" && r["component"] == component.0), "{diff}");
    let second = s.execute("publication.proof.create", &json!({"name":"Second proof"})).unwrap()["id"].clone();
    s.edit(|d, _| {
        d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.size = Some(30.));
        Ok(())
    })
    .unwrap();
    let diff = s.execute("publication.proof.compare", &json!({"before":second})).unwrap();
    assert!(diff["rows"].as_array().unwrap().iter().any(|r| r["kind"] == "formatting"), "{diff}");
    assert!(!diff["rows"].as_array().unwrap().iter().any(|r| r["kind"] == "content"), "{diff}");
    assert_eq!(view(&s).unwrap().root.review.proofs[&proof.as_u64().unwrap()], baseline);
    let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let loaded = designcraft_format::load(&bytes).unwrap();
    assert_eq!(loaded.publication.unwrap().root.review.proofs[&proof.as_u64().unwrap()], baseline);
}

fn xml(body: &str) -> String {
    format!(r#"<TEI xmlns="http://www.tei-c.org/ns/1.0"><text><body>{body}</body></text></TEI>"#)
}

#[test]
fn adding_an_anchored_comment_does_not_change_visible_proof_content_or_layout() {
    let (mut s, sid, _, _) = publication();
    let proof = s.execute("publication.proof.create", &json!({"name":"Before comment"})).unwrap()["id"].clone();
    s.execute("publication.comment.add", &json!({"story":sid,"start":2,"end":6,"text":"Check wording"})).unwrap();
    let diff = s.execute("publication.proof.compare", &json!({"before":proof})).unwrap();
    assert!(diff["rows"].as_array().unwrap().is_empty(), "{diff}");
    s.execute("story.replaceRange", &json!({"story":sid,"start":16,"end":17,"text":"X"})).unwrap();
    let diff = s.execute("publication.proof.compare", &json!({"before":proof})).unwrap();
    let row = diff["rows"].as_array().unwrap().iter().find(|r| r["kind"] == "content").unwrap();
    assert_eq!(row["start"], 16);
    s.execute("publication.proof.goTo", row).unwrap();
    assert_eq!(s.doc().unwrap().selection.text.unwrap().anchor, 16);
}

#[test]
fn reference_proof_difference_navigates_to_its_source_in_an_inactive_chapter() {
    let (mut s, _, first, _) = publication();
    let second = s.execute("publication.addComponent", &json!({"title":"Second"})).unwrap()["component"].clone();
    let sid = s.execute("frame.create", &json!({"rect":[40,40,400,600],"content":"text","text":"See "})).unwrap()["story"].clone();
    s.execute("publication.template.define", &json!({"name":"Book title","parts":[{"token":{"type":"publicationTitle"}}]})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":4,"focus":4})).unwrap();
    s.execute("publication.reference.insert", &json!({"query":{"ids":[first]},"template":"Book title"})).unwrap();
    let proof = s.execute("publication.proof.create", &json!({"name":"Before title"})).unwrap()["id"].clone();
    s.execute("publication.activate", &json!({"component":first})).unwrap();
    s.execute("publication.metadata", &json!({"title":"Revised book title"})).unwrap();
    let diff = s.execute("publication.proof.compare", &json!({"before":proof})).unwrap();
    let row = diff["rows"].as_array().unwrap().iter().find(|r| r["kind"] == "reference").unwrap();
    assert_eq!(row["component"], second);
    assert_eq!(row["story"], sid);
    assert_eq!(row["start"], 4);
    s.execute("publication.proof.goTo", row).unwrap();
    assert_eq!(view(&s).unwrap().component.0, second.as_u64().unwrap());
    assert_eq!(s.doc().unwrap().selection.text.unwrap().anchor, 4);
}

#[test]
fn anchored_comments_inside_rich_notes_preserve_printed_proof() {
    let mut s = Session::new();
    s.execute("file.openTei", &input(include_str!("../../../../../textimport/tests/fixtures/tei-notes-apparatus.xml"))).unwrap();
    s.execute("publication.convert", &json!({})).unwrap();
    let proof = s.execute("publication.proof.create", &json!({"name":"Before rich note comment"})).unwrap()["id"].clone();
    let notes = s.execute("publication.notes.list", &json!({})).unwrap();
    let note = notes["notes"].as_array().unwrap().iter().find(|n| n["stream"] == "Footnotes").unwrap();
    s.execute("publication.note.select", &json!({"id":note["id"],"anchor":2,"focus":6})).unwrap();
    s.execute("publication.comment.add", &json!({"text":"Check note"})).unwrap();
    let diff = s.execute("publication.proof.compare", &json!({"before":proof})).unwrap();
    assert!(diff["rows"].as_array().unwrap().is_empty(), "{diff}");
    s.execute("publication.note.select", &json!({"id":note["id"],"anchor":0,"focus":0})).unwrap();
    s.execute("text.insert", &json!({"text":"Updated "})).unwrap();
    let diff = s.execute("publication.proof.compare", &json!({"before":proof})).unwrap();
    let row = diff["rows"].as_array().unwrap().iter().find(|r| r["kind"] == "content").unwrap();
    assert!(row["after"].as_str().unwrap().starts_with("Updated "), "{row}");
    let result = s.execute("publication.proof.goTo", row).unwrap();
    assert_eq!(result["noteOwnerActivated"], true);
    assert_eq!(result["id"], note["id"]);
    assert_eq!(s.doc().unwrap().selection.text.unwrap().cell.unwrap().annotation_id(), note["id"].as_u64());
}
fn input(text: &str) -> Value {
    json!({"name":"revised.tei","base64":crate::cmd::base64_encode(text.as_bytes())})
}

#[test]
fn manuscript_comparison_matches_source_ids_keeps_rich_content_and_original_sources() {
    let mut s = Session::new();
    let first = xml(r#"<p xml:id="p1">First <hi rend="italic">passage</hi>.</p><p xml:id="p2">Second paragraph.</p>"#);
    s.execute("file.openTei", &input(&first)).unwrap();
    s.execute("publication.convert", &json!({})).unwrap();
    let sid = s.doc().unwrap().doc.settings.primary_story.or_else(|| s.doc().unwrap().doc.stories.keys().next().copied()).unwrap();
    let original = s.doc().unwrap().doc.clone();
    let revised = xml(
        r#"<p xml:id="p1">First <hi rend="italic">revised passage</hi>.<note>A rich <hi rend="bold">note</hi>.</note></p><p xml:id="p2">Second paragraph.</p>"#,
    );
    let preview = s.execute("publication.manuscript.preview", &input(&revised)).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &original));
    let rows = preview["rows"].as_array().unwrap();
    assert!(rows.iter().all(|r| r["confidence"] == "sourceId"), "{preview}");
    s.execute("publication.manuscript.apply", &json!({"preview":preview["preview"],"rows":rows.iter().map(|r|r["row"].clone()).collect::<Vec<_>>()}))
        .unwrap();
    let d = &s.doc().unwrap().doc;
    assert!(d.story(sid).unwrap().text.contains("revised passage"));
    assert_eq!(d.story(sid).unwrap().notes.len(), 1);
    assert_eq!(d.tei_sources.len(), 2);
    assert_eq!(d.tei_sources[0].bytes.as_slice(), first.as_bytes());
    assert_eq!(d.tei_sources[1].bytes.as_slice(), revised.as_bytes());
    d.check().unwrap();
    let final_doc = d.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(view(&s).unwrap().root, original.publication.as_ref().unwrap().root);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, final_doc);
}

#[test]
fn uncertain_manuscript_matches_and_stale_previews_preserve_state() {
    let (mut s, _, _, _) = publication();
    let preview =
        s.execute("publication.manuscript.preview", &json!({"name":"revision.txt","base64":crate::cmd::base64_encode(b"Revised text")})).unwrap();
    assert_eq!(preview["rows"][0]["requiresReview"], true);
    let before = s.doc().unwrap().clone();
    assert!(s.execute("publication.manuscript.apply", &json!({"preview":preview["preview"],"rows":[0]})).is_err());
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before.doc));
    assert_eq!(s.doc().unwrap().selection, before.selection);
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len());
    s.execute("publication.manuscript.apply", &json!({"preview":preview["preview"],"rows":[0],"reviewed":[0]})).unwrap();
    let preview =
        s.execute("publication.manuscript.preview", &json!({"name":"revision.txt","base64":crate::cmd::base64_encode(b"Later text")})).unwrap();
    s.execute("publication.component", &json!({"title":"Changed"})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("publication.manuscript.apply", &json!({"preview":preview["preview"],"rows":[0],"reviewed":[0]})).is_err());
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
}

#[test]
fn nested_table_note_review_and_story_editor_share_tracking_behavior() {
    let (mut s, sid, _, _) = publication();
    let note = s
        .edit(|d, _| {
            let mut table = Table::new(77, 1, 1, 0, 0, 200.);
            let leaf = &mut table.cell_mut(0, 0).unwrap().text;
            leaf.insert(0, "Cell text");
            let note = leaf.insert_note(4, "Nested note", Default::default());
            d.story_mut(sid).unwrap().insert_table(0, table);
            Ok(note)
        })
        .unwrap();
    let comment=s.execute("publication.comment.add",&json!({"story":sid,"path":[{"type":"cell","table":77,"row":0,"col":0},{"type":"footnote","id":note}],"start":0,"end":6,"text":"Nested comment"})).unwrap()["id"].clone();
    s.execute("changes.track", &json!({"on":true,"author":"Nested editor"})).unwrap();
    s.execute("publication.review.goTo", &json!({"id":comment})).unwrap();
    let t = s.doc().unwrap().selection.text.unwrap();
    s.execute("publication.review.replace", &json!({"id":comment,"start":t.anchor,"end":t.focus,"text":"Changed"})).unwrap();
    let ranges = view(&s).unwrap().root.change_ranges().unwrap();
    assert_eq!(ranges.len(), 2);
    assert!(ranges.iter().all(|r| r.path.len() == 2 && r.author == "Nested editor"));
    s.execute("changes.rejectAll", &json!({})).unwrap();
    s.execute("publication.review.goTo", &json!({"id":comment})).unwrap();
    let t = s.doc().unwrap().selection.text.unwrap();
    assert_eq!(&s.doc().unwrap().doc.text_story(sid, t.cell).unwrap().text[t.range()], "Nested");
    let pos = s.doc().unwrap().doc.story(sid).unwrap().text.find("Original").unwrap();
    s.execute("story.replaceRange", &json!({"story":sid,"start":pos,"end":pos+8,"text":"Edited"})).unwrap();
    assert_eq!(view(&s).unwrap().root.change_ranges().unwrap().len(), 2);
}

#[test]
fn proof_reports_moved_figures_and_deleted_index_passages_with_navigation() {
    let (mut s, sid, component, _) = publication();
    s.execute("publication.index.define", &json!({"name":"Subjects"})).unwrap();
    let topic = s.execute("publication.index.topic", &json!({"index":"Subjects","display":"Passage"})).unwrap()["topic"].clone();
    let occurrence =
        s.execute("publication.index.mark", &json!({"index":"Subjects","topic":topic,"story":sid,"start":0,"end":8})).unwrap()["occurrence"].clone();
    let figure = s.execute("frame.create", &json!({"rect":[420,50,500,100]})).unwrap()["id"].as_u64().unwrap();
    let proof = s.execute("publication.proof.create", &json!({"name":"Before movement"})).unwrap()["id"].clone();
    s.edit(|d, _| {
        let i = d.item_mut(designcraft_doc::ItemId(figure)).unwrap();
        i.xf = designcraft_geom::Affine::translate((0., 70.)) * i.xf;
        d.story_mut(sid).unwrap().delete(3..11);
        Ok(())
    })
    .unwrap();
    let report = s.execute("publication.proof.compare", &json!({"before":proof})).unwrap();
    let rows = report["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["kind"] == "movedFigure" && r["item"] == figure), "{report}");
    assert!(rows.iter().any(|r| r["kind"] == "indexPassageRemoved" && r["occurrence"] == occurrence), "{report}");
    let nav = s.execute("publication.proof.goTo", &json!({"component":component,"item":figure})).unwrap();
    assert_eq!(nav["page"], 0);
    assert_eq!(s.doc().unwrap().selection.items, [designcraft_doc::ItemId(figure)]);
}

#[test]
fn reordered_manuscript_keeps_paragraph_boundaries_and_source_destinations() {
    let mut s = Session::new();
    s.execute("file.openTei", &input(&xml(r#"<p xml:id="a">First.</p><p xml:id="b">Second.</p><p xml:id="c">Third.</p>"#))).unwrap();
    s.execute("publication.convert", &json!({})).unwrap();
    let before = view(&s).unwrap();
    let story = s.doc().unwrap().doc.stories.values().max_by_key(|s| s.len()).unwrap().clone();
    let a = story.anchors.iter().find(|a| a.name == "a").unwrap().id;
    let preview = s
        .execute(
            "publication.manuscript.preview",
            &input(&xml(r#"<p xml:id="c">Third.</p><p xml:id="a">First edited.</p><p xml:id="b">Second.</p>"#)),
        )
        .unwrap();
    let rows = preview["rows"].as_array().unwrap().iter().map(|r| r["row"].clone()).collect::<Vec<_>>();
    s.execute("publication.manuscript.apply", &json!({"preview":preview["preview"],"rows":rows})).unwrap();
    let d = &s.doc().unwrap().doc;
    let revised = d.story(story.id).unwrap();
    assert_eq!(designcraft_doc::xref::clean(&revised.text).trim(), "Third.\nFirst edited.\nSecond.");
    assert_eq!(revised.anchors.iter().find(|i| i.name == "a").unwrap().id, a);
    assert!(d.publication.as_ref().unwrap().root.revision > before.root.revision);
}
