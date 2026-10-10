#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::cmd::publication::tests::publication;

#[test]
fn rich_tei_fixture_composes_tables_figures_and_multiline_notes() {
    let mut s = Session::new();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../textimport/tests/fixtures/tei-notes-apparatus.xml");
    s.execute("file.openTei", &json!({"path":path})).unwrap();
    s.execute("publication.convert", &json!({})).unwrap();
    s.execute("publication.update", &json!({})).unwrap();
    s.execute("publication.notes.policy", &json!({"stream":"Footnotes","columns":2,"maxFraction":0.6})).unwrap();
    s.execute("publication.apparatus.witness", &json!({"witness":"B"})).unwrap();
    let entries = view(&s).unwrap().root.annotation_entries().unwrap();
    assert_eq!(entries.len(), 6);
    let d = &s.doc().unwrap().doc;
    let all: Vec<_> = d.stories.keys().map(|sid| designcraft_compose::compose_story(d, *sid, &Default::default())).collect();
    assert!(all.iter().all(|c| c.overset_at.is_none() && c.annotation_overflow.is_empty()));
    assert!(all.iter().flat_map(|c| &c.frames).flat_map(|f| &f.notes).any(|n| n.text.frames.iter().any(|f| !f.tables.is_empty())));
    assert!(all.iter().flat_map(|c| &c.frames).flat_map(|f| &f.notes).any(|n| n.text.frames.iter().any(|f| !f.objects.is_empty())));
}

#[test]
fn independent_streams_rich_edit_placement_editions_and_persistence() {
    let (mut s, sid, _, _) = publication();
    s.execute("publication.notes.stream", &json!({"name":"Sources","style":"lowerRoman"})).unwrap();
    s.execute("publication.notes.stream", &json!({"name":"Comments","style":"upperLetters"})).unwrap();
    let a = s.execute("publication.note.insert", &json!({"stream":"Sources","story":sid,"at":2,"text":"First source.\nSecond paragraph."})).unwrap()
        ["id"]
        .clone();
    let b = s.execute("publication.note.insert", &json!({"stream":"Comments","story":sid,"at":1,"text":"Comment."})).unwrap()["id"].clone();
    let entries = s.execute("publication.notes.list", &json!({})).unwrap();
    assert_eq!(entries["notes"][0]["label"], "A");
    assert_eq!(entries["notes"][1]["label"], "i");
    s.execute("publication.note.select", &json!({"id":a,"anchor":0,"focus":5})).unwrap();
    s.execute("text.insert", &json!({"text":"Edited"})).unwrap();
    let before = s.doc().unwrap().clone();
    s.execute("publication.notes.policy", &json!({"stream":"Sources","placement":"componentEnd","columns":2})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    let v = view(&s).unwrap();
    let id = a.as_u64().unwrap();
    let note = v.root.annotation_entries().unwrap().into_iter().find(|e| e.identity.id == id).unwrap();
    assert_eq!(note.note.text.text, "Edited source.\nSecond paragraph.");
    let loc = &v.root.editions[&v.edition].references.locations[&id];
    assert!(loc.page > 0);
    let carrier = v.root.note_outputs.values().find(|o| o.stream == "Sources").unwrap();
    let cs = designcraft_compose::compose_story(&s.doc().unwrap().doc, carrier.story, &Default::default());
    assert!(!cs.frames.iter().flat_map(|f| &f.notes).collect::<Vec<_>>().is_empty());
    assert!(cs.annotation_overflow.is_empty());
    let reading = s.execute("publication.edition.create", &json!({"name":"Reading"})).unwrap()["edition"].clone();
    s.execute("publication.activate", &json!({"edition":reading})).unwrap();
    s.execute("publication.notes.policy", &json!({"stream":"Sources","placement":"footnote"})).unwrap();
    let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let restored = designcraft_format::load(&bytes).unwrap();
    let r = restored.publication.as_ref().unwrap();
    assert_eq!(r.root.annotation_entries().unwrap().len(), 2);
    assert_eq!(r.root.note_policy(v.edition, "Sources").placement, Placement::ComponentEnd);
    assert_eq!(r.root.note_policy(r.edition, "Sources").placement, Placement::Footnote);
    assert_eq!(s.execute("publication.note.goTo", &json!({"id":b,"target":"callout"})).unwrap()["page"], 0);
}

#[test]
fn long_notes_autoflow_and_failed_policy_preserves_document_and_history() {
    let (mut s, sid, _, _) = publication();
    s.execute("publication.notes.stream", &json!({"name":"Long"})).unwrap();
    let r = s
        .execute(
            "publication.note.insert",
            &json!({"stream":"Long","story":sid,"at":1,"text":"A long source note that continues with its rich text intact. ".repeat(180)}),
        )
        .unwrap();
    let d = &s.doc().unwrap().doc;
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    assert!(d.page_count() > 1);
    assert!(cs.annotation_overflow.is_empty());
    assert!(cs.frames.iter().filter(|f| !f.notes.is_empty()).count() > 1);
    let before = s.doc().unwrap().clone();
    assert!(s.execute("publication.notes.policy", &json!({"stream":"Long","placement":"sidenote","sideWidth":30})).is_err());
    let after = s.doc().unwrap();
    assert_eq!(before.doc, after.doc);
    assert_eq!(before.selection, after.selection);
    assert_eq!(before.history.undo.len(), after.history.undo.len());
    assert!(r["id"].is_number());
}

const XML: &str = r##"<TEI xmlns="http://www.tei-c.org/ns/1.0"><teiHeader><fileDesc><titleStmt><title>Witnesses</title></titleStmt><publicationStmt><p>Test</p></publicationStmt><sourceDesc><p>Authored fixture</p></sourceDesc></fileDesc><encodingDesc><listWit><witness xml:id="A">First</witness><witness xml:id="B">Second</witness></listWit></encodingDesc></teiHeader><text><body><p>The <app xml:id="reading"><lem wit="#A">red</lem><rdg wit="#B"><hi rend="italic">blue</hi></rdg></app> book.</p></body></text></TEI>"##;
fn tei() -> Session {
    let mut s = Session::new();
    s.execute("file.openBytes", &json!({"name":"apparatus.tei","base64":crate::cmd::file::base64_encode(XML.as_bytes())})).unwrap();
    s.execute("publication.convert", &json!({})).unwrap();
    s
}
#[test]
fn witness_changes_rendering_without_overwriting_imported_or_edited_text() {
    let mut s = tei();
    let before = view(&s).unwrap();
    let e = before.root.annotation_entries().unwrap().into_iter().find(|e| e.note.apparatus.is_some()).unwrap();
    assert_eq!(e.note.apparatus.as_ref().unwrap().readings.len(), 2);
    let canonical = before.root.content[&e.content].text.clone();
    s.execute("publication.apparatus.witness", &json!({"witness":"B"})).unwrap();
    let d = &s.doc().unwrap().doc;
    let cs = designcraft_compose::compose_story(d, e.story, &Default::default());
    let generated: String =
        cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).filter_map(|g| g.generated.as_ref()?.text.as_deref()).collect();
    assert!(generated.contains("blue"), "{generated}");
    assert_eq!(view(&s).unwrap().root.content[&e.content].text, canonical);
    let range = e.note.apparatus.as_ref().unwrap().range(d.story(e.story).unwrap()).unwrap();
    s.execute("text.select", &json!({"story":e.story,"anchor":range.start,"focus":range.end})).unwrap();
    s.execute("text.insert", &json!({"text":"edited green"})).unwrap();
    let list = s.execute("publication.apparatus.list", &json!({})).unwrap();
    assert_eq!(list["entries"][0]["edited"], true);
    let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let d = designcraft_format::load(&bytes).unwrap();
    assert_eq!(d.tei_sources[0].bytes.as_slice(), XML.as_bytes());
    assert!(d.story(e.story).unwrap().text.contains("edited green"));
    assert_eq!(d.publication.as_ref().unwrap().root.annotation_entries().unwrap()[0].note.apparatus.as_ref().unwrap().readings.len(), 2);
}

#[test]
fn recovering_old_source_structure_keeps_later_edits() {
    let mut s = tei();
    let e = view(&s).unwrap().root.annotation_entries().unwrap()[0].clone();
    s.edit(|d, _| {
        let st = d.story_mut(e.story).unwrap();
        let a = st.endnote_mut(e.note.id).unwrap().apparatus.take().unwrap();
        let range = a.range(st).unwrap();
        st.replace(range, "green");
        Ok(())
    })
    .unwrap();
    let report = s.execute("publication.apparatus.recover", &json!({})).unwrap();
    assert_eq!(report["candidates"].as_array().unwrap().len(), 1);
    let before = s.doc().unwrap().doc.story(e.story).unwrap().text.clone();
    let r = s.execute("publication.apparatus.recover", &json!({"apply":true})).unwrap();
    assert_eq!(r["applied"], 1);
    assert_eq!(designcraft_doc::xref::clean(&s.doc().unwrap().doc.story(e.story).unwrap().text), designcraft_doc::xref::clean(&before));
    assert_eq!(s.execute("publication.apparatus.list", &json!({})).unwrap()["entries"][0]["edited"], true);
}

#[test]
fn publication_endnotes_keep_owner_assets_and_allow_canonical_edits() {
    use designcraft_doc::{AnchoredObject, Asset, AssetId, Content, Document, Graphic, Item, ItemId, Shape, SpreadRef};
    use designcraft_geom::{Affine, Rect, shapes};
    fn chapter(color: &str) -> Document {
        let mut d = Document::new(&Default::default());
        d.title = color.into();
        let layer = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36., 36., 436., 650.), layer, "A source callout.", Default::default()).unwrap();
        let asset = AssetId(d.alloc());
        let bytes = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="{color}"/></svg>"#)
            .into_bytes();
        d.assets.insert(
            asset,
            Arc::new(Asset {
                id: asset,
                name: format!("{color}.svg"),
                mime: "image/svg+xml".into(),
                link: None,
                data: Arc::new(bytes),
                pixels: Some((40, 20)),
                page: 0,
            }),
        );
        let mut item = Item::new(ItemId(d.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(0., 0., 40., 20.)));
        item.content =
            Content::Graphic(Graphic { asset, size: (40., 20.), xf: Affine::IDENTITY, auto_fit: Default::default(), fit_align: 4, crop: [0.; 4] });
        let st = d.story_mut(sid).unwrap();
        let id = st.insert_note(1, &format!("{color} note with a picture."), Default::default());
        let n = st.note_mut(id).unwrap();
        n.text.insert_object(n.text.len(), AnchoredObject::new(item, Default::default()));
        n.text.format_chars(0..color.len(), |f| f.over.font_style = Some("Italic".into()));
        d
    }
    let mut s = Session::new();
    s.add_document(crate::DocState::new(chapter("red"), None));
    s.execute("publication.convert", &json!({})).unwrap();
    let first = view(&s).unwrap().component;
    let other = chapter("blue");
    let bytes = designcraft_format::save(&other).unwrap();
    s.execute("publication.addComponent", &json!({"name":"second.designcraft","base64":crate::cmd::file::base64_encode(&bytes)})).unwrap();
    s.execute("publication.notes.policy", &json!({"stream":"Footnotes","placement":"publicationEnd"})).unwrap();
    let v = view(&s).unwrap();
    let carrier = v.root.note_outputs.values().find(|o| o.source.is_none()).unwrap();
    let id = carrier.story;
    assert_ne!(carrier.component, first);
    let cs = designcraft_compose::compose_story(&s.doc().unwrap().doc, id, &Default::default());
    let notes: Vec<_> = cs.frames.iter().flat_map(|f| &f.notes).collect();
    assert_eq!(notes.len(), 2);
    for (n, color) in notes.iter().zip(["red", "blue"]) {
        let owner = n.text.render_document.as_ref().unwrap();
        let source = n.source_story.as_ref().unwrap();
        let Content::Graphic(g) = &source.objects[0].item.content else { panic!("missing illustration") };
        assert_eq!(owner.assets[&g.asset].name, format!("{color}.svg"));
        assert!(!n.text.frames[0].objects.is_empty());
        assert_eq!(source.char_format_at(0).over.font_style.as_deref(), Some("Italic"));
    }
    let original = notes[0].annotation.unwrap();
    let fi = cs.frames.iter().position(|f| f.notes.iter().any(|n| n.annotation == Some(original))).unwrap();
    let frame = cs.frames[fi].frame;
    let local = notes[0].rect.center();
    let doc = &s.doc().unwrap().doc;
    let loc = doc.find(frame).unwrap();
    let it = doc.item(frame).unwrap();
    let p = doc.parent_xf(&loc) * doc.text_xf(it) * local;
    let hit = s.execute("text.placeCaret", &json!({"frame":frame,"point":[p.x,p.y]})).unwrap();
    assert_eq!(hit["noteOwnerActivated"], true);
    assert_eq!(view(&s).unwrap().component, first);
    s.execute("publication.note.edit", &json!({"id":original,"text":"red revised note with a picture.\u{e00e}"})).unwrap();
    let e = view(&s).unwrap().root.annotation_entries().unwrap().into_iter().find(|e| e.identity.id == original).unwrap();
    assert_eq!(e.note.text.objects.len(), 1);
    assert_eq!(e.note.text.char_format_at(0).over.font_style.as_deref(), Some("Italic"));
}

#[test]
fn copying_apparatus_allocates_distinct_boundaries_and_standalone_v3() {
    let mut s = tei();
    let e = view(&s).unwrap().root.annotation_entries().unwrap()[0].clone();
    let len = s.doc().unwrap().doc.story(e.story).unwrap().len();
    s.execute("text.select", &json!({"story":e.story,"anchor":0,"focus":len})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("text.select", &json!({"story":e.story,"anchor":len,"focus":len})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let entries = view(&s).unwrap().root.annotation_entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].identity.id, entries[1].identity.id);
    assert_ne!(entries[0].note.apparatus.as_ref().unwrap().start, entries[1].note.apparatus.as_ref().unwrap().start);
    for e in entries {
        assert!(!e.note.apparatus.as_ref().unwrap().edited(s.doc().unwrap().doc.story(e.story).unwrap()));
    }
    let mut standalone = s.doc().unwrap().doc.as_ref().clone();
    standalone.publication = None;
    let bytes = designcraft_format::save(&standalone).unwrap();
    let d = designcraft_format::load(&bytes).unwrap();
    assert_eq!(d.story(e.story).unwrap().endnotes.len(), 2);
    assert_eq!(d.tei_sources[0].bytes.as_slice(), XML.as_bytes());
}

#[test]
fn recovering_duplicate_source_names_never_selects_the_first_arbitrarily() {
    let mut s = tei();
    let e = view(&s).unwrap().root.annotation_entries().unwrap().remove(0);
    s.edit(|d, _| {
        let st = d.story_mut(e.story).unwrap();
        let a = st.endnote_mut(e.note.id).unwrap().apparatus.take().unwrap();
        st.insert_anchor(0, &a.source_key);
        st.insert_anchor(0, &a.source_key);
        Ok(())
    })
    .unwrap();
    let before = s.doc().unwrap().doc.clone();
    let report = s.execute("publication.apparatus.recover", &json!({"apply":true})).unwrap();
    assert_eq!(report["applied"], 0);
    assert!(report["candidates"][0]["problem"].as_str().unwrap().contains("Repeated source identity"));
    assert_eq!(s.doc().unwrap().doc.story(e.story), before.story(e.story));
}
