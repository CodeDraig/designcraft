use super::*;
use crate::{Document, SpreadRef};
use std::sync::Arc;

fn topic(id: u64, display: &str, sort: SortKey) -> Topic {
    Topic { id, display: display.into(), sort, parent: None, see: vec![], see_also: vec![] }
}
#[test]
fn typed_sorting_preserves_display_and_hierarchy() {
    let mut index = Definition { name: "Numbers".into(), sorting: SortMode::Numeric, ..Default::default() };
    index.topics.insert(1, topic(1, "Ten", SortKey::Numeric { value: 10 }));
    index.topics.insert(2, topic(2, "Two", SortKey::Numeric { value: 2 }));
    assert_eq!(index.ordered_topics().unwrap(), [(2, 1), (1, 1)]);
    index.sorting = SortMode::Archival;
    for (id, shelfmark, folio, side) in [(1, "MS 10", 1, Side::Recto), (2, "MS 2", 2, Side::Verso), (3, "MS 2", 2, Side::Recto)] {
        index.topics.insert(
            id,
            topic(
                id,
                &format!("Original label {id}"),
                SortKey::Archival { repository: "NLW".into(), collection: "Peniarth".into(), shelfmark: shelfmark.into(), folio, side },
            ),
        );
    }
    assert_eq!(index.ordered_topics().unwrap(), [(3, 1), (2, 1), (1, 1)]);
    index.topics.get_mut(&1).unwrap().parent = Some(2);
    assert_eq!(index.ordered_topics().unwrap(), [(3, 1), (2, 1), (1, 2)]);
}
#[test]
fn biblical_order_is_explicit_and_aliases_do_not_change_display() {
    let mut index = Definition {
        name: "Scripture".into(),
        sorting: SortMode::Biblical,
        book_order: vec!["John".into(), "Genesis".into()],
        book_aliases: BTreeMap::from([("Jn".into(), "John".into())]),
        ..Default::default()
    };
    let key = |book: &str, verse| SortKey::Biblical { book: book.into(), chapter: 1, verse, end_chapter: None, end_verse: Some(verse + 1) };
    index.topics.insert(1, topic(1, "Gen. i. 1–2", key("Genesis", 1)));
    index.topics.insert(2, topic(2, "Ioannes i. 2–3", key("Jn", 2)));
    index.topics.insert(3, topic(3, "John 1:1–2", key("John", 1)));
    assert_eq!(index.ordered_topics().unwrap(), [(3, 1), (2, 1), (1, 1)]);
    assert_eq!(index.topics[&2].display, "Ioannes i. 2–3");
    index.topics.insert(4, topic(4, "Unconfigured", key("Tobit", 1)));
    assert!(index.ordered_topics().unwrap_err().to_string().contains("Tobit"));
}
#[test]
fn anchored_range_reports_removed_text_and_merges_overlapping_pages() {
    let mut d = Document::new(&Default::default());
    let layer = d.default_layer();
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), designcraft_geom::Rect::new(36., 36., 400., 600.), layer, "one passage", Default::default()).unwrap();
    let end = d.story_mut(sid).unwrap().insert_anchor(11, "end");
    let start = d.story_mut(sid).unwrap().insert_anchor(0, "start");
    let v = Publication::from_document(&d, "Ranges").unwrap();
    let mut root = (*v.root).clone();
    let content = root.components[&v.component].stories[&sid];
    let topic_id = root.alloc().unwrap();
    let occurrence_id = root.alloc().unwrap();
    let end_id = root.alloc().unwrap();
    let target = |anchor| TextTarget { component: v.component, content, anchor };
    let mut index = Definition { name: "Names".into(), ..Default::default() };
    index.topics.insert(topic_id, topic(topic_id, "A passage", Default::default()));
    index.occurrences.insert(
        occurrence_id,
        Occurrence {
            id: occurrence_id,
            topic: topic_id,
            start: target(start),
            end: Some(Endpoint { id: end_id, target: target(end) }),
            suppress_pages: false,
        },
    );
    let mut layout = super::super::references::ReferenceLayout::default();
    let loc = |page| Location { component: v.component, page, global_page: page, label: (page + 1).to_string(), order: vec![] };
    layout.locations.insert(occurrence_id, loc(2));
    layout.locations.insert(end_id, loc(4));
    let p = preview(&root, &index, &layout).unwrap();
    assert_eq!(p.rows[0].text.plain(), "A passage  3–5");
    assert_eq!(p.occurrences[0].excerpt, "one passage");
    assert!(p.occurrences[0].problem.is_none());
    let c = Arc::make_mut(root.content.get_mut(&content).unwrap());
    let mut st = crate::Story::new(sid);
    st.content = Arc::new(c.clone());
    st.delete(3..14);
    *c = (*st.content).clone();
    let p = preview(&root, &index, &layout).unwrap();
    assert!(p.occurrences[0].problem.as_ref().unwrap().contains("removed"));
    assert!(p.rows[0].text.plain().contains("Unresolved"));
}

#[test]
fn command_json_accepts_biblical_ranges_and_partial_rich_formats() {
    let key: SortKey =
        serde_json::from_value(serde_json::json!({"type":"biblical","book":"Jn","chapter":3,"verse":16,"endChapter":4,"endVerse":2})).unwrap();
    assert!(matches!(key, SortKey::Biblical { end_chapter: Some(4), end_verse: Some(2), .. }));
    let format: Formatting = serde_json::from_value(serde_json::json!({"pageFormat":{"over":{"size":14.0}}})).unwrap();
    assert_eq!(format.page_format.over.size, Some(14.0));
}
