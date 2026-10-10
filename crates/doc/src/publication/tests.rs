#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::{ParaFormat, SpreadRef, build::NewDocument, geom::Rect};

fn chapter(title: &str, text: &str) -> (Document, StoryId) {
    let mut doc = Document::new(&NewDocument { title: title.into(), ..Default::default() });
    let layer = doc.default_layer();
    let (_, story) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 400.0, 600.0), layer, text, ParaFormat::default()).unwrap();
    (doc, story)
}

#[test]
fn independent_editions_share_rich_text_and_keep_original_layouts() {
    let (mut doc, story) = chapter("One", "Shared text");
    doc.story_mut(story).unwrap().insert_note(4, "rich\nnote", ParaFormat::default());
    let view = Publication::from_document(&doc, "Book").unwrap();
    let mut root = (*view.root).clone();
    let reading = root.duplicate_edition(view.edition, "Reading").unwrap();
    let mut edition = root.project(reading, view.component).unwrap();
    edition.settings.page_width = 300.0;
    root.capture(reading, view.component, &edition).unwrap();
    let original = root.project(view.edition, view.component).unwrap();
    assert_eq!(original.spreads, doc.spreads);
    assert_eq!(original.styles, doc.styles);
    assert_eq!(original.settings.page_width, doc.settings.page_width);
    assert!(Arc::ptr_eq(&original.story(story).unwrap().content, &edition.story(story).unwrap().content));
    let baseline = root.clone();
    edition.story_mut(story).unwrap().insert(0, "New ");
    let identity = root.components[&view.component].stories[&story];
    root.capture(reading, view.component, &edition).unwrap();
    assert_eq!(root.components[&view.component].stories[&story], identity);
    let other = root.project(view.edition, view.component).unwrap();
    assert_eq!(other.story(story).unwrap().text, edition.story(story).unwrap().text);
    assert!(other.story(story).unwrap().text.starts_with("New Shar"));
    assert_eq!(other.story(story).unwrap().notes[0].text.text, "rich\nnote");
    assert!(!baseline.content[&identity].text.starts_with("New "));
    root.check().unwrap();
}

#[test]
fn repeated_imports_allocate_distinct_content_and_resource_identities() {
    let (doc, story) = chapter("Chapter", "same local story id");
    let view = Publication::from_document(&doc, "Book").unwrap();
    let mut root = (*view.root).clone();
    let second = root.add_document(&doc, ComponentRole::BackMatter, None).unwrap();
    assert_ne!(root.components[&view.component].stories[&story], root.components[&second].stories[&story]);
    let id = root.components[&second].stories[&story];
    root.roots.reverse();
    assert_eq!(root.ordered_components().unwrap(), vec![second, view.component]);
    assert_eq!(root.components[&second].stories[&story], id);
    assert_eq!(root.project(view.edition, second).unwrap().spreads, doc.spreads);
    root.check().unwrap();
}

#[test]
fn tree_cycles_and_missing_layouts_are_rejected() {
    let (doc, _) = chapter("One", "text");
    let view = Publication::from_document(&doc, "Book").unwrap();
    let mut root = (*view.root).clone();
    Arc::make_mut(root.components.get_mut(&view.component).unwrap()).children.push(view.component);
    assert!(root.check().unwrap_err().to_string().contains("cycle"));
    let mut root = (*view.root).clone();
    Arc::make_mut(root.editions.get_mut(&view.edition).unwrap()).components.clear();
    assert!(root.check().is_err());
}

#[test]
fn changed_projection_does_not_mutate_undo_snapshot() {
    let (doc, story) = chapter("One", "before");
    let view = Publication::from_document(&doc, "Book").unwrap();
    let before = view.project().unwrap();
    let mut edited = before.clone();
    edited.story_mut(story).unwrap().replace(0..6, "after");
    edited.sync_publication().unwrap();
    let current = edited.publication.as_ref().unwrap().project().unwrap();
    assert_eq!(current.story(story).unwrap().text, "after");
    assert_eq!(before.publication.as_ref().unwrap().project().unwrap().story(story).unwrap().text, "before");
}

#[test]
fn deletion_removes_other_editions_frames_and_new_content_is_unplaced() {
    let (doc, story) = chapter("One", "text");
    let view = Publication::from_document(&doc, "Book").unwrap();
    let mut root = (*view.root).clone();
    let second = root.duplicate_edition(view.edition, "Second").unwrap();
    let mut edited = root.project(view.edition, view.component).unwrap();
    edited.remove_item(edited.story(story).unwrap().frames[0]).unwrap();
    let layer = edited.default_layer();
    let (_, new) = edited.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 200.0, 400.0), layer, "new", ParaFormat::default()).unwrap();
    root.capture(view.edition, view.component, &edited).unwrap();
    let other = root.project(second, view.component).unwrap();
    assert!(other.story(story).is_none());
    assert_eq!(other.story(new).unwrap().text, "new");
    assert!(other.story(new).unwrap().frames.is_empty());
    assert_eq!(other.next_id, edited.next_id);
    root.check().unwrap();
}

#[test]
fn edition_typography_survives_disjoint_unicode_edits_and_shared_change_marks() {
    let (doc, sid) = chapter("One", "one ŵ middle three");
    let view = Publication::from_document(&doc, "Book").unwrap();
    let mut root = (*view.root).clone();
    let reading = root.duplicate_edition(view.edition, "Reading").unwrap();
    let mut read = root.project(reading, view.component).unwrap();
    read.story_mut(sid).unwrap().format_chars(7..13, |f| f.over.size = Some(24.0));
    root.capture(reading, view.component, &read).unwrap();
    let mut print = root.project(view.edition, view.component).unwrap();
    assert_eq!(print.story(sid).unwrap().format_after(7).over.size, None);
    print.story_mut(sid).unwrap().insert(0, "NEW ");
    let end = print.story(sid).unwrap().len();
    print.story_mut(sid).unwrap().insert(end, " END");
    print.story_mut(sid).unwrap().format_chars(11..17, |f| f.over.change = Some(crate::ChangeMark::Inserted));
    root.capture(view.edition, view.component, &print).unwrap();
    let read = root.project(reading, view.component).unwrap();
    let st = read.story(sid).unwrap();
    assert_eq!(st.text, "NEW one ŵ middle three END");
    assert_eq!(st.format_after(11).over.size, Some(24.0));
    assert_eq!(st.format_after(11).over.change, Some(crate::ChangeMark::Inserted));
    assert_eq!(st.format_after(0).over.size, None);
    root.check().unwrap();
}

#[test]
fn table_cell_identity_carries_edition_presentation_through_insertions_and_nested_edits() {
    let (mut doc, sid) = chapter("One", "intro");
    let mut table = crate::Table::new(91, 2, 1, 0, 0, 200.0);
    table.cells[0].text.insert(0, "alpha");
    table.cells[1].text.insert_note(0, "note", ParaFormat::default());
    doc.story_mut(sid).unwrap().insert_table(5, table);
    let view = Publication::from_document(&doc, "Book").unwrap();
    let mut root = (*view.root).clone();
    let reading = root.duplicate_edition(view.edition, "Reading").unwrap();
    let mut read = root.project(reading, view.component).unwrap();
    let t = Arc::make_mut(read.story_mut(sid).unwrap().tables.get_mut(&91).unwrap());
    let original_id = t.cells[0].id;
    t.cells[0].insets = [15.0; 4];
    t.columns[0].width = 175.0;
    t.rows[0].height = 45.0;
    Arc::make_mut(&mut t.cells[1].text.notes[0]).text.paras[0].chars.size = Some(19.0);
    root.capture(reading, view.component, &read).unwrap();
    let mut print = root.project(view.edition, view.component).unwrap();
    let t = Arc::make_mut(print.story_mut(sid).unwrap().tables.get_mut(&91).unwrap());
    t.insert_rows(0, 1);
    t.cells[1].text.insert(0, "new ");
    Arc::make_mut(&mut t.cells[2].text.notes[0]).text.insert(0, "new ");
    root.capture(view.edition, view.component, &print).unwrap();
    let read = root.project(reading, view.component).unwrap();
    let t = &read.story(sid).unwrap().tables[&91];
    assert_ne!(t.cells[0].id, original_id);
    assert_eq!(t.cells[1].id, original_id);
    assert_eq!(t.cells[1].insets, [15.0; 4]);
    assert_eq!(t.cells[1].text.text, "new alpha");
    assert_eq!(t.cells[2].text.notes[0].text.text, "new note");
    assert_eq!(t.cells[2].text.notes[0].text.paras[0].chars.size, Some(19.0));
    assert_eq!(root.project(view.edition, view.component).unwrap().story(sid).unwrap().tables[&91].cells[1].insets, [4.0; 4]);
    let mut print = root.project(view.edition, view.component).unwrap();
    let t = Arc::make_mut(print.story_mut(sid).unwrap().tables.get_mut(&91).unwrap());
    t.insert_cols(0, 1, 50.0);
    root.capture(view.edition, view.component, &print).unwrap();
    let read = root.project(reading, view.component).unwrap();
    let t = &read.story(sid).unwrap().tables[&91];
    assert_eq!(t.columns[1].width, 175.0);
    assert_eq!(t.rows[1].height, 45.0);
    assert_eq!(t.cells[3].id, original_id);
    root.check().unwrap();
}

#[test]
fn synchronization_updates_materialized_styles_preserves_local_overrides_and_other_edition() {
    let (mut target, sid) = chapter("Target", "Text");
    let cs = crate::CellStyle { name: "Body Cell".into(), fill: Some("old".into()), insets: Some([5.0; 4]), ..Default::default() };
    let ts = crate::TableStyle { name: "Grid".into(), body: Some("Body Cell".into()), space_before: Some(6.0), ..Default::default() };
    target.styles_mut().cell.push(cs.clone());
    target.styles_mut().table.push(ts.clone());
    target.styles_mut().object.push(crate::ObjectStyle { name: "Frame".into(), fill: Some(crate::Fill::swatch("old")), ..Default::default() });
    let frame = target.story(sid).unwrap().frames[0];
    target.item_mut(frame).unwrap().object_style = "Frame".into();
    target.item_mut(frame).unwrap().fill = crate::Fill::swatch("old");
    let mut table = crate::Table::new(91, 2, 1, 0, 0, 200.0);
    ts.apply_to(&mut table, &[cs]);
    table.cells[1].fill = "local override".into();
    table.cells[1].insets[0] = 40.0;
    target.story_mut(sid).unwrap().insert_table(4, table);
    let view = Publication::from_document(&target, "Book").unwrap();
    let mut root = (*view.root).clone();
    let reading = root.duplicate_edition(view.edition, "Reading").unwrap();
    let mut source = target.clone();
    source.title = "Source".into();
    source.styles_mut().cell[1].fill = Some("new".into());
    source.styles_mut().cell[1].insets = Some([10.0; 4]);
    source.styles_mut().table[1].space_before = Some(20.0);
    source.styles_mut().object.last_mut().unwrap().fill = Some(crate::Fill::swatch("new"));
    let source = root.add_document(&source, ComponentRole::Chapter, None).unwrap();
    let rows: Vec<_> = root.style_preview(view.edition, source, view.edition, &[view.component]).unwrap().into_iter().filter(|r| r.changed).collect();
    root.apply_styles(view.edition, &rows).unwrap();
    let print = root.project(view.edition, view.component).unwrap();
    let t = &print.story(sid).unwrap().tables[&91];
    assert_eq!(t.options.space_before, 20.0);
    assert_eq!(t.cells[0].fill, "new");
    assert_eq!(t.cells[1].fill, "local override");
    assert_eq!(t.cells[1].insets, [40.0, 10.0, 10.0, 10.0]);
    assert_eq!(print.item(frame).unwrap().fill, crate::Fill::swatch("new"));
    let other = root.project(reading, view.component).unwrap();
    assert_eq!(other.story(sid).unwrap().tables[&91].options.space_before, 6.0);
    assert_eq!(other.story(sid).unwrap().tables[&91].cells[0].fill, "old");
    assert_eq!(other.item(frame).unwrap().fill, crate::Fill::swatch("old"));
    let mut edit = print;
    edit.story_mut(sid).unwrap().insert(0, "added ");
    root.capture(view.edition, view.component, &edit).unwrap();
    assert_eq!(root.project(reading, view.component).unwrap().story(sid).unwrap().tables[&91].cells[0].fill, "old");
    root.check().unwrap();
}
