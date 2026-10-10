use super::*;
const BOOK: &[u8] = include_bytes!("../../../../textimport/tests/fixtures/tei-book.xml");
const IMAGE: &[u8] = include_bytes!("../../../../textimport/tests/fixtures/orchard.svg");
fn params() -> Value {
    json!({"name":"book.xml","base64":super::super::file::base64_encode(BOOK),"resources":{"orchard.svg":{"base64":super::super::file::base64_encode(IMAGE)}}})
}
fn simple(text: &str) -> Value {
    json!({"name":"book.tei","base64":super::super::file::base64_encode(format!(r#"<TEI xmlns="http://www.tei-c.org/ns/1.0"><text><body>{text}</body></text></TEI>"#).as_bytes())})
}

#[test]
fn open_resources_reports_roundtrip_and_pdf() {
    let mut s = Session::new();
    let r = s.execute("file.openTei", &params()).unwrap();
    assert_eq!(r["overset"], false);
    let d = &s.doc().unwrap().doc;
    d.check().unwrap();
    assert_eq!(d.title, "The Glass Orchard");
    assert!(s.doc().unwrap().path.is_none());
    assert!(s.doc().unwrap().is_dirty());
    assert_eq!(d.assets.len(), 1);
    assert_eq!(*d.tei_sources[0].bytes, BOOK);
    assert!(d.tei_sources[0].resources.iter().all(|r| r.asset.is_some()));
    let es = d.story(d.endnote_story.unwrap()).unwrap();
    assert!(es.text.contains("Its second paragraph survives."));
    assert!(es.paras.len() >= 5, "rich note paragraphs must survive");
    for h in &d.hyperlinks {
        assert!(h.source.text_range(d).is_some());
        if let HyperlinkDest::Anchor(a) = h.dest {
            assert!(s.cache.xref_index(d).page_index(a).is_some());
        }
    }
    let native = designcraft_format::save(d).unwrap();
    let loaded = designcraft_format::load(&native).unwrap();
    assert_eq!(**d, loaded);
    let report = s.execute("tei.report", &json!({})).unwrap();
    assert_eq!(report[0]["name"], "book.xml");
    assert_eq!(super::super::file::base64_decode(s.execute("tei.source", &json!({})).unwrap()["base64"].as_str().unwrap()), BOOK);
    let pdf = s.execute("file.exportPdf", &json!({})).unwrap();
    assert!(pdf["base64"].as_str().is_some_and(|s| !s.is_empty()));
}
#[test]
fn native_path_and_generic_open_dispatch() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../textimport/tests/fixtures/tei-book.xml");
    let mut s = Session::new();
    s.execute("file.open", &json!({"path":path})).unwrap();
    assert_eq!(s.doc().unwrap().doc.assets.len(), 1);
    assert!(s.doc().unwrap().path.is_none());
    s.execute("file.openBytes", &params()).unwrap();
    assert_eq!(s.documents().len(), 2);
    assert_eq!(s.doc().unwrap().doc.title, "The Glass Orchard");
}
#[test]
fn failed_open_and_place_preserve_document_selection_and_history() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    let revision = s.doc().unwrap().revision;
    let undo = s.doc().unwrap().history.undo.len();
    let bad = json!({"name":"bad.tei","base64":super::super::file::base64_encode(b"<TEI>")});
    for cmd in ["file.openTei", "file.openBytes", "file.place"] {
        assert!(s.execute(cmd, &bad).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().selection, selection);
        assert_eq!(s.doc().unwrap().revision, revision);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        assert_eq!(s.documents().len(), 1);
    }
    let mut p = params();
    p["resources"]["orchard.svg"] = json!({"base64":"bad!"});
    assert!(s.execute("file.place", &p).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    let huge = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="10000"><rect width="100000" height="10000"/></svg>"#;
    p["resources"]["orchard.svg"] = json!({"base64":super::super::file::base64_encode(huge)});
    let error = s.execute("file.place", &p).unwrap_err().to_string();
    assert!(error.contains("100 million pixels"), "{error}");
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}
#[test]
fn place_undo_redo_and_repeated_imports() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let mut p = params();
    p["autoflow"] = json!(true);
    let r = s.execute("file.place", &p).unwrap();
    assert_eq!(r["overset"], false);
    let first = s.doc().unwrap().doc.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *first);
    s.doc_mut().unwrap().selection = doc::Selection::default();
    s.execute("file.place", &p).unwrap();
    let d = &s.doc().unwrap().doc;
    d.check().unwrap();
    assert_eq!(d.tei_sources.len(), 2);
    let mut ids = std::collections::HashSet::new();
    for src in &d.tei_sources {
        assert!(ids.insert(src.id));
        for n in &src.navigation {
            assert!(ids.insert(n.anchor));
            assert!(d.find_anchor(n.anchor).is_some());
        }
    }
    let sid = StoryId(r["story"].as_u64().unwrap());
    s.doc_mut().unwrap().selection = doc::Selection::text(doc::TextSel { story: sid, anchor: 0, focus: 0, frame: None, cell: None });
    s.execute("file.place", &simple("<p>Inserted at the caret.<note place=\"end\">New note.</note></p>")).unwrap();
    let d = &s.doc().unwrap().doc;
    d.check().unwrap();
    assert!(d.story(sid).unwrap().endnotes.iter().any(|n| n.text.text.contains("New note.")));
}
#[test]
fn long_book_flows_through_final_body_and_endnote_text() {
    let mut body = String::new();
    for i in 0..400 {
        body.push_str(&format!(
            "<p>Paragraph {i}. {}<note place=\"end\">Note {i}: a complete record.</note></p>",
            "The orchard keeper records each leaf and every word. ".repeat(10)
        ));
    }
    body.push_str("<p>FINAL BODY SENTINEL.</p>");
    let mut s = Session::new();
    let r = s.execute("file.openTei", &simple(&body)).unwrap();
    assert_eq!(r["overset"], false);
    let d = &s.doc().unwrap().doc;
    assert!(d.page_count() > 20);
    for (sid, story) in &d.stories {
        let cs = s.cache.get(d, *sid, None);
        assert!(cs.overset_at.is_none());
        assert!(cs.frames.iter().map(|f| f.range.end).max().unwrap_or(0) >= story.len());
        assert!(cs.frames.last().is_some_and(|f| !f.lines.is_empty() || !f.notes.is_empty()), "unused trailing frame in story {sid:?}");
    }
    assert!(d.stories.values().any(|s| s.text.contains("FINAL BODY SENTINEL.")));
    assert!(d.story(d.endnote_story.unwrap()).unwrap().text.contains("Note 399:"));
}
#[test]
fn unplaceable_content_rolls_back() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let mut p = simple("<p rend=\"font-size: 1000pt\">Unplaceable</p>");
    p["autoflow"] = json!(true);
    assert!(s.execute("file.place", &p).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}

#[test]
fn table_notes_rich_endnotes_and_nested_links_survive_reflow() {
    let mut s = Session::new();
    let p = simple(
        r##"<p><ref target="#cell">literal cell label</ref><note place="end"><p>First note.</p><p>Second paragraph <hi>italic</hi>.</p></note></p><table><row><cell xml:id="cell">Cell text<note>Table footnote converted.</note><app><lem>chosen</lem><rdg wit="#B">alternative</rdg></app><ref target="#tail">nested link label</ref></cell></row></table><p xml:id="tail">Tail<note place="end">Final note.</note></p>"##,
    );
    let r = s.execute("file.openTei", &p).unwrap();
    assert_eq!(r["overset"], false);
    let d = &s.doc().unwrap().doc;
    d.check().unwrap();
    let end = d.story(d.endnote_story.unwrap()).unwrap();
    assert!(end.text.contains("Table footnote converted."));
    assert!(end.text.contains("alternative"));
    assert!(end.text.contains("Second paragraph"));
    assert_eq!(d.ordered_endnotes().len(), 4);
    for h in &d.hyperlinks {
        let HyperlinkDest::Anchor(id) = h.dest else { panic!("internal link required") };
        assert!(s.cache.xref_index(d).page_index(id).is_some());
        assert!(h.source.text_range(d).is_some());
    }
    let sid = StoryId(r["story"].as_u64().unwrap());
    let numbers = d.endnote_numbers(sid);
    assert_eq!(numbers, vec![1, 4]);
    let list = s.execute("endnote.list", &json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 4);
    let original = s.doc().unwrap().doc.clone();
    s.edit(|d, _| {
        d.story_mut(sid).unwrap().insert(0, &"A reflow paragraph. ".repeat(1000));
        Ok(())
    })
    .unwrap();
    s.edit(|d, _| {
        super::super::place_text::autoflow(d, sid, 2000)?;
        Ok(())
    })
    .unwrap();
    let d = &s.doc().unwrap().doc;
    let target = match d.hyperlinks[0].dest {
        HyperlinkDest::Anchor(a) => a,
        _ => panic!("expected an anchor destination"),
    };
    assert!(s.cache.xref_index(d).page_index(target).unwrap() > s.cache.xref_index(&original).page_index(target).unwrap());
    assert_eq!(d.hyperlinks, original.hyperlinks);
}
