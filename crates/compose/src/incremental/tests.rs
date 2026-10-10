#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use designcraft_doc::{ListType, ParaAttrs, SpreadRef, StartParagraph, VerticalJustification, build::NewDocument};

fn book(pages: usize, hard_breaks: bool) -> (Document, StoryId) {
    let mut d = Document::new(&NewDocument::default());
    let text = (0..pages)
        .map(|i| {
            format!(
                "Paragraph {i}: Affinity in typography, a Welsh river and a clear sky. {}",
                "The river follows the orchard. ".repeat(if hard_breaks { 2 } else { 8 })
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let layer = d.default_layer();
    let (mut last, sid) = d
        .add_text_frame(
            SpreadRef::Doc(0),
            Rect::new(0., 0., 260., 300.),
            layer,
            &text,
            ParaFormat {
                para: ParaAttrs { start_paragraph: hard_breaks.then_some(StartParagraph::NextFrame), ..Default::default() },
                ..Default::default()
            },
        )
        .unwrap();
    for _ in 1..pages {
        let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0., 0., 260., 300.), layer, "", ParaFormat::default()).unwrap();
        d.thread(last, fid).unwrap();
        last = fid;
    }
    (d, sid)
}
fn checked(cache: &Cache, d: &Document, sid: StoryId) {
    cache.verify(d, sid).unwrap();
}
#[test]
fn local_edit_reuses_prefix_and_converged_suffix_with_utf8_shift() {
    let (mut d, sid) = book(80, true);
    let cache = Cache::new();
    checked(&cache, &d, sid);
    cache.metrics(true);
    let at = d.story(sid).unwrap().para_ranges()[40].start + 14;
    d.story_mut(sid).unwrap().replace(at..at + 8, "Llŷn Valley");
    checked(&cache, &d, sid);
    let m = cache.metrics(false);
    assert_eq!(m.incremental, 1);
    assert!(m.paragraphs < 8, "{m:?}");
    let w = &m.events[0].work;
    assert!(w.reused_prefix_paragraphs >= 39);
    assert!(w.reused_suffix_paragraphs > 30);
    assert!(w.old_bytes.is_some());
    // Undo, redo and another changed paragraph must keep the next checkpoint chain valid.
    let edited = d.clone();
    d.story_mut(sid).unwrap().replace(at..at + "Llŷn Valley".len(), "typograp");
    checked(&cache, &d, sid);
    checked(&cache, &edited, sid);
}
#[test]
fn paragraph_insert_delete_and_true_cascade_match_full_composition() {
    let (mut d, sid) = book(60, false);
    let cache = Cache::new();
    checked(&cache, &d, sid);
    for (step, text) in
        ["Inserted paragraph.\n", "long content ", "\n\n", "é Welsh ŵ", "", "a much longer discussion "].into_iter().cycle().take(25).enumerate()
    {
        let ranges = d.story(sid).unwrap().para_ranges();
        let r = ranges[8 + step % 15].clone();
        let end = d.story(sid).unwrap().text[r.clone()].char_indices().nth(3).map_or(r.start, |(i, _)| r.start + i);
        d.story_mut(sid).unwrap().replace(r.start..end, text);
        checked(&cache, &d, sid);
    }
    cache.metrics(true);
    let at = d.story(sid).unwrap().para_ranges()[20].start;
    d.story_mut(sid).unwrap().replace(at..at, &"Cascade changes line and page boundaries. ".repeat(150));
    checked(&cache, &d, sid);
    assert!(cache.metrics(false).paragraphs > 10);
}
#[test]
fn keeps_lists_columns_justification_and_wrap_geometry_match_full() {
    let (mut d, sid) = book(32, false);
    for (i, p) in d.story_mut(sid).unwrap().paras.iter_mut().enumerate() {
        p.para.keep_with_next = Some(if i % 5 == 0 { 2 } else { 0 });
        p.para.keep_lines_together = Some(true);
        p.para.keep_all_lines = Some(false);
        p.para.list_type = Some(ListType::Numbers);
    }
    let frames = d.story(sid).unwrap().frames.clone();
    for f in &frames {
        let tf = d.item_mut(*f).unwrap().text_frame_mut().unwrap();
        tf.options.columns = 2;
        tf.options.vertical_justification = VerticalJustification::Center;
    }
    let cache = Cache::new();
    checked(&cache, &d, sid);
    for index in [10, 20, 5, 1, 25] {
        let r = d.story(sid).unwrap().para_ranges()[index].clone();
        d.story_mut(sid).unwrap().replace(r.start..r.start + 9, "A changed");
        checked(&cache, &d, sid);
    }
    d.item_mut(frames[8]).unwrap().text_frame_mut().unwrap().options.columns = 1;
    checked(&cache, &d, sid);
    d.settings.baseline_grid.increment = 17.;
    checked(&cache, &d, sid);
    let layer = d.default_layer();
    let id = ItemId(d.alloc());
    let mut item =
        designcraft_doc::Item::new(id, layer, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0., 0., 100., 100.)));
    item.wrap.mode = WrapMode::BoundingBox;
    d.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    checked(&cache, &d, sid);
    d.layers[0].visible = false;
    checked(&cache, &d, sid);
}
#[test]
fn overset_checkpoint_resumes_when_frames_are_appended() {
    let (mut d, sid) = book(30, true);
    let ids = d.story(sid).unwrap().frames.clone();
    d.story_mut(sid).unwrap().frames.truncate(10);
    let cache = Cache::new();
    assert!(cache.get(&d, sid, None).is_overset());
    d.story_mut(sid).unwrap().frames = ids;
    checked(&cache, &d, sid);
    assert!(!cache.get(&d, sid, None).is_overset());
    assert!(cache.metrics(false).incremental > 0);
}
#[test]
fn complex_note_continuation_is_an_explicit_full_fallback() {
    let (mut d, sid) = book(8, false);
    d.story_mut(sid).unwrap().insert_note(8, "Rich note text with enough words to compose.", ParaFormat::default());
    let cache = Cache::new();
    checked(&cache, &d, sid);
    d.story_mut(sid).unwrap().replace(0..3, "NEW");
    checked(&cache, &d, sid);
    assert!(cache.metrics(false).events.iter().all(|e| e.work.reason.as_deref() == Some("note continuation and numbering")));
}
#[test]
fn semantic_projection_hits_and_global_style_invalidation() {
    let (mut d, sid) = book(30, true);
    let cache = Cache::new();
    checked(&cache, &d, sid);
    cache.metrics(true);
    let story = d.story(sid).unwrap().clone();
    d.stories.insert(sid, Arc::new(story));
    checked(&cache, &d, sid);
    assert_eq!(cache.metrics(false).semantic_hits, 1);
    Arc::make_mut(&mut d.styles).paragraph[0].chars.size = Some(15.);
    checked(&cache, &d, sid);
    assert!(cache.metrics(false).events.iter().any(|e| e.work.reason.as_deref() == Some("styles or composition inputs changed")));
}

#[test]
fn repeated_boundary_edits_and_deleting_an_entire_paragraph_match_oracle() {
    let (mut d, sid) = book(36, false);
    let cache = Cache::new();
    checked(&cache, &d, sid);
    for step in 0..70 {
        let ranges = d.story(sid).unwrap().para_ranges();
        let index = (step * 13) % ranges.len();
        let r = ranges[index].clone();
        let (range, text) = match step % 5 {
            0 => (r.end..r.end, " ŵ"),
            1 => (r.start..r.start, "\nInserted paragraph\n"),
            2 if index + 1 < ranges.len() => (r.start..ranges[index + 1].start, ""),
            3 => (0..0, "First words. "),
            _ => {
                let end = d.story(sid).unwrap().len();
                (end..end, "\nFinal text")
            }
        };
        d.story_mut(sid).unwrap().replace(range, text);
        checked(&cache, &d, sid);
    }
}
