use super::*;
use crate::publication::numbering::SemanticObject;
use crate::{Document, ParaFormat, SemanticId, SpreadRef};

#[test]
fn running_head_rule_uses_source_order_strictly_before_first_body_and_suppresses_opening() {
    let mut d = Document::new(&Default::default());
    let layer = d.default_layer();
    let (_, sid) = d
        .add_text_frame(
            SpreadRef::Doc(0),
            designcraft_geom::Rect::new(0., 0., 200., 600.),
            layer,
            "First heading\nText\nSecond heading\nMore",
            ParaFormat::default(),
        )
        .unwrap();
    let a = d.story_mut(sid).unwrap().insert_anchor(0, "First");
    let at = d.story(sid).unwrap().text.find("Second").unwrap();
    let b = d.story_mut(sid).unwrap().insert_anchor(at, "Second");
    let v = Publication::from_document(&d, "Book").unwrap();
    let mut r = (*v.root).clone();
    let content = r.components[&v.component].stories[&sid];
    let mut ids = Vec::new();
    for anchor in [a, b] {
        let id = SemanticId(r.alloc().unwrap());
        r.semantic.insert(
            id,
            SemanticObject { id, kind: ObjectKind::Heading, target: TextTarget { component: v.component, content, anchor }, level: 1, title: None },
        );
        ids.push(id.0);
    }
    let index = SemanticIndex::build(&r).unwrap();
    let mut layout = ReferenceLayout::default();
    for (i, id) in ids.iter().enumerate() {
        layout.locations.insert(
            *id,
            Location { component: v.component, page: i, global_page: i, label: (i + 1).to_string(), order: vec![0, if i == 0 { 0 } else { at }] },
        );
    }
    layout.first_body.insert(v.component, [(0, vec![0, 0]), (1, vec![0, at]), (2, vec![0, at + 10])].into());
    let context = RenderContext::new(&r, &r.references, &index, &layout).unwrap();
    let f = Field {
        id: 99,
        query: Query { kind: Some(ObjectKind::Heading), ..Default::default() },
        template: "Title".into(),
        use_: FieldUse::RunningHead,
        separator: String::new(),
        suppress_opening: true,
    };
    assert_eq!(context.field(&f, Some((v.component, 0))).unwrap().0.plain(), "");
    assert_eq!(context.field(&f, Some((v.component, 1))).unwrap().0.plain(), "First heading");
    assert_eq!(context.field(&f, Some((v.component, 2))).unwrap().0.plain(), "Second heading");
}

#[test]
fn template_character_format_allows_partial_overrides() {
    let value: Template = serde_json::from_str(
        r#"{"name":"Rich","parts":[{"token":{"type":"title"},"format":{"over":{"fontStyle":"Italic"}}},{"token":{"type":"page"},"format":{}}]}"#,
    )
    .unwrap();
    assert_eq!(value.parts[0].format.over.font_style.as_deref(), Some("Italic"));
    assert!(value.parts[1].format.over.is_empty());
}
