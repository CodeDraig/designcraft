#![allow(clippy::unwrap_used, clippy::expect_used)]
use designcraft_doc::{
    build::NewDocument,
    publication::{
        ComponentRole, Publication,
        numbering::{ObjectKind, SemanticObject, TextTarget},
        references::*,
    },
    *,
};
use std::sync::Arc;

/// Authored deterministic fixture: each numbered section starts a page, but its body and fields
/// flow normally and may cross pages. Long insertions therefore cause real pagination cascades.
pub fn book(chapters: usize, pages: usize) -> Document {
    let mut v = Publication::from_document(&chapter(0, pages), "The River Atlas — generated layout fixture").unwrap();
    let root = Arc::make_mut(&mut v.root);
    for c in 1..chapters {
        root.add_document(&chapter(c, pages), ComponentRole::Chapter, None).unwrap();
    }
    let components = root.ordered_components().unwrap();
    let mut objects = Vec::new();
    for (ci, &component) in components.iter().enumerate() {
        let mut d = root.project(v.edition, component).unwrap();
        let sid = d.settings.primary_story.unwrap();
        let content = root.components[&component].stories[&sid];
        let ranges = d.story(sid).unwrap().para_ranges();
        let mut chapter_objects = Vec::new();
        for pi in (0..pages).rev() {
            let anchor = d.story_mut(sid).unwrap().insert_anchor(ranges[pi].start, &format!("section-{ci}-{pi}"));
            let id = SemanticId(root.alloc().unwrap());
            root.semantic.insert(
                id,
                SemanticObject {
                    id,
                    kind: ObjectKind::Heading,
                    target: TextTarget { component, content, anchor },
                    level: 1,
                    title: Some(format!("Section {}.{}", ci + 1, pi + 1)),
                },
            );
            chapter_objects.push(id.0);
        }
        chapter_objects.reverse();
        objects.extend(chapter_objects);
        root.capture(v.edition, component, &d).unwrap();
    }
    root.references.templates.insert(
        "Atlas reference".into(),
        Template {
            name: "Atlas reference".into(),
            parts: vec![
                TemplatePart { token: Token::Title, format: Default::default() },
                TemplatePart { token: Token::Literal { text: " (p. ".into() }, format: Default::default() },
                TemplatePart { token: Token::GlobalPage, format: Default::default() },
                TemplatePart { token: Token::Literal { text: ")".into() }, format: Default::default() },
            ],
        },
    );
    for (ci, &component) in components.iter().enumerate() {
        let mut d = root.project(v.edition, component).unwrap();
        let sid = d.settings.primary_story.unwrap();
        let ranges = d.story(sid).unwrap().para_ranges();
        for pi in (0..pages).rev() {
            for offset in [0, 1, pages, objects.len() / 2] {
                let target = objects[(ci * pages + pi + offset) % objects.len()];
                let id = root.alloc().unwrap();
                root.references.fields.insert(
                    id,
                    Field {
                        id,
                        query: Query { ids: vec![target], ..Default::default() },
                        template: "Atlas reference".into(),
                        use_: FieldUse::First,
                        separator: String::new(),
                        suppress_opening: false,
                    },
                );
                d.story_mut(sid).unwrap().insert_xref(ranges[pi].end, CrossRef { target: 0, format: String::new(), field: Some(id) });
                d.story_mut(sid).unwrap().replace(ranges[pi].end..ranges[pi].end, "; ");
            }
        }
        root.capture(v.edition, component, &d).unwrap();
    }
    root.check().unwrap();
    v.project().unwrap()
}
pub fn chapter(index: usize, pages: usize) -> Document {
    let mut d = Document::new(&NewDocument {
        title: format!("River chapter {}", index + 1),
        pages,
        primary_text_frame: true,
        width: 432.,
        height: 648.,
        ..Default::default()
    });
    let sid = d.settings.primary_story.unwrap();
    let text = (0..pages)
        .map(|p| {
            format!(
                "Section {}.{} — the river and the orchard. {} Related sections: ",
                index + 1,
                p + 1,
                "The river bends below the orchard, where the wind moves through the trees and the old bridge joins the two banks. ".repeat(10)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    d.story_mut(sid).unwrap().replace(0..0, &text);
    for p in &mut d.story_mut(sid).unwrap().paras {
        p.para.start_paragraph = Some(StartParagraph::NextFrame);
    }
    d
}
pub fn page_count(d: &Document) -> usize {
    let v = d.publication.as_ref().unwrap();
    v.root.editions[&v.edition].components.values().map(|c| c.page_count()).sum()
}
pub fn session(d: Document) -> designcraft_engine::Session {
    let mut s = designcraft_engine::Session::new();
    s.add_document(designcraft_engine::DocState::new(d, None));
    s
}
