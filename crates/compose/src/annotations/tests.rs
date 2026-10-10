use super::*;
use designcraft_doc::{
    ParaFormat, SpreadRef, StoryId,
    publication::{Publication, annotations::Identity},
};

fn doc(text: &str, note: &str, pages: usize) -> (Document, StoryId, u64) {
    let mut d = Document::new(&designcraft_doc::build::NewDocument { pages, ..Default::default() });
    let layer = d.default_layer();
    let (first, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 336.0), layer, text, ParaFormat::default()).unwrap();
    let id = d.story_mut(sid).unwrap().insert_note(7, note, Default::default());
    let mut prev = first;
    for page in 1..pages {
        let (si, pi) = d.page_loc(page).unwrap();
        let pg = &d.spreads[si].pages[pi];
        let rect = Rect::new(pg.bounds().x0 + 36.0, 36.0, pg.bounds().x0 + 336.0, 336.0);
        let (next, _) = d.add_text_frame(SpreadRef::Doc(si), rect, layer, "", Default::default()).unwrap();
        d.thread(prev, next).unwrap();
        prev = next;
    }
    let v = Publication::from_document(&d, "Notes").unwrap();
    let id = v.root.annotation_entries().unwrap().into_iter().find(|e| e.note.id == id).unwrap().identity.id;
    (v.project().unwrap(), sid, id)
}

#[test]
fn long_note_continues_with_callout_and_never_overlaps_body() {
    let text = format!("Callout {}", "The body has enough ordinary text to flow through its frames. ".repeat(25));
    let note = "A continuing rich note whose reading order must be preserved. ".repeat(25);
    let (d, sid, id) = doc(&text, &note, 8);
    let cs = crate::compose_story(&d, sid, &Default::default());
    assert!(cs.diagnostics.is_empty(), "{:?}", cs.diagnostics);
    assert!(cs.annotation_overflow.is_empty());
    let fragments: Vec<_> =
        cs.frames.iter().enumerate().flat_map(|(fi, f)| f.notes.iter().filter(move |n| n.annotation == Some(id)).map(move |n| (fi, n))).collect();
    assert!(fragments.len() > 1);
    assert_eq!(fragments[0].0, 0);
    assert_eq!(fragments.last().unwrap().1.source_range.end, note.len());
    for (fi, n) in fragments {
        for l in &cs.frames[fi].lines {
            assert!(l.baseline + l.descent <= n.rect.y0 + 0.01, "body overlaps note: {fi} {} {}", l.baseline + l.descent, n.rect.y0);
        }
    }
}

#[test]
fn streams_number_independently_and_run_in_uses_separate_columns() {
    let (mut d, sid, _) = doc("Callout body and a second point and a third point.", "First.", 2);
    let st = d.story_mut(sid).unwrap();
    let b = st.insert_note(st.len(), "Second.", Default::default());
    st.note_mut(b).unwrap().annotation = Some(Identity { id: 0, callout: 0, stream: "Footnotes".into() });
    let c = st.insert_note(st.len(), "Textual source.", Default::default());
    st.note_mut(c).unwrap().annotation = Some(Identity { id: 0, callout: 0, stream: "Sources".into() });
    d.sync_publication().unwrap();
    let mut v = d.publication.clone().unwrap();
    let r = Arc::make_mut(&mut v.root);
    let edition = Arc::make_mut(r.editions.get_mut(&v.edition).unwrap());
    edition.note_policies.insert("Footnotes".into(), Policy { columns: 2, run_in: true, ..Default::default() });
    d = v.project().unwrap();
    let cs = crate::compose_story(&d, sid, &Default::default());
    assert!(cs.diagnostics.is_empty(), "{:?}", cs.diagnostics);
    let ns: Vec<_> = cs.frames.iter().flat_map(|f| &f.notes).collect();
    assert_eq!(ns.len(), 3);
    assert_eq!(ns.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(), ["1", "2", "1"]);
    assert_eq!(ns[0].text.frames[0].columns.len(), 2);
    assert_eq!(ns[0].text.frames[0].lines[0].baseline, ns[1].text.frames[0].lines[0].baseline);
}

#[test]
fn sidenotes_avoid_collisions_and_report_overflow() {
    let (mut d, sid, _) = doc("Callout body with notes.", &"A long marginal comment. ".repeat(200), 1);
    let mut v = d.publication.clone().unwrap();
    let r = Arc::make_mut(&mut v.root);
    Arc::make_mut(r.editions.get_mut(&v.edition).unwrap())
        .note_policies
        .insert("Footnotes".into(), Policy { placement: Placement::Sidenote, side_width: 70.0, ..Default::default() });
    d = v.project().unwrap();
    let cs = crate::compose_story(&d, sid, &Default::default());
    assert!(cs.diagnostics.iter().any(|s| s.contains("Sidenote") && s.contains("overflows")), "{:?}", cs.diagnostics);
    assert!(cs.frames[0].notes[0].rect.x0 >= 230.0);
}
