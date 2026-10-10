use super::*;
use designcraft_doc::publication::{Publication, references::*};
use designcraft_doc::{Document, ParaAttrs, ParaFormat, SpreadRef, StoryId};
use designcraft_geom::Rect;

fn field_doc(text: &str) -> (Document, StoryId) {
    let mut d = Document::new(&Default::default());
    let layer = d.default_layer();
    let (a, sid) = d
        .add_text_frame(
            SpreadRef::Doc(0),
            Rect::new(0., 0., 150., 45.),
            layer,
            "",
            ParaFormat { para: ParaAttrs { hyphenate: Some(false), ..Default::default() }, ..Default::default() },
        )
        .unwrap();
    let (b, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(200., 0., 350., 600.), layer, "", ParaFormat::default()).unwrap();
    d.thread(a, b).unwrap();
    let v = Publication::from_document(&d, "Fields").unwrap();
    let mut root = (*v.root).clone();
    let id = root.alloc().unwrap();
    root.references.fields.insert(
        id,
        Field {
            id,
            query: Query { ids: vec![v.component.0], ..Default::default() },
            template: "Title".into(),
            use_: FieldUse::First,
            separator: ", ".into(),
            suppress_opening: false,
        },
    );
    let e = Arc::make_mut(root.editions.get_mut(&v.edition).unwrap());
    e.references.fields.insert(
        id,
        FieldValues {
            text: FieldText {
                runs: vec![RichRun { text: text.into(), format: Default::default(), target: Some(v.component.0) }],
                unresolved: vec![],
            },
            ..Default::default()
        },
    );
    d.publication = Some(designcraft_doc::publication::PublicationView { root: Arc::new(root), ..v });
    d.story_mut(sid).unwrap().insert_xref(0, designcraft_doc::xref::CrossRef { target: 0, format: String::new(), field: Some(id) });
    (d, sid)
}

#[test]
fn generated_title_wraps_and_retains_source_coverage_across_frames() {
    let value = "A long title with ordinary wrapping and Unicode: Cymru café œuvre fiancée. More words to carry this title onto another line and another frame.";
    let (d, sid) = field_doc(value);
    let cs = compose_story(&d, sid, &Default::default());
    assert!(!cs.is_overset());
    assert!(cs.frames.iter().all(|f| !f.lines.is_empty()));
    assert!(cs.line_count() > 3);
    for f in &cs.frames {
        assert_eq!(f.range, 0..designcraft_doc::XREF_MARK.len_utf8());
    }
    let mut actual = String::new();
    for l in cs.frames.iter().flat_map(|f| &f.lines) {
        assert_eq!(l.range, 0..designcraft_doc::XREF_MARK.len_utf8());
        assert!(l.end_x <= l.x1 + 1., "{} > {}", l.end_x, l.x1);
        for g in &l.glyphs {
            if let Some(t) = g.generated.as_ref().and_then(|g| g.text.as_ref()) {
                actual.push_str(t);
            }
            assert_eq!(g.byte, 0);
        }
        actual.push(' ');
    }
    // Trimmed break spaces are absent from placed geometry; words and Unicode remain exact.
    assert_eq!(actual.split_whitespace().collect::<Vec<_>>(), value.split_whitespace().collect::<Vec<_>>());
}

#[test]
fn generated_bidi_uses_the_replacement_text_and_preserves_cluster_unicode() {
    let value = "Cymru (שלום) office café";
    let (d, sid) = field_doc(value);
    let cs = compose_story(&d, sid, &Default::default());
    let glyphs: Vec<_> = cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).collect();
    assert!(glyphs.iter().any(|g| g.rtl));
    assert!(glyphs.iter().any(|g| g.generated.as_ref().and_then(|g| g.text.as_ref()).is_some_and(|s| s.contains('é'))));
    assert!(glyphs.iter().all(|g| g.generated.as_ref().is_some_and(|g| g.target.is_some())));
}
