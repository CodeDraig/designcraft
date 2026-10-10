//! Publication note areas. Body and annotation composition converge together; every note is
//! released no earlier than its callout frame, and unfinished notes remain explicit overflow.
use crate::{ComposeOptions, ComposedStory, FrameSpec, FrameText, PlacedNote, shape::SubstCtx};
use designcraft_doc::{
    Document, ENDNOTE_REF, FOOTNOTE_REF, ItemId, Story,
    publication::annotations::{self as model, Placement, Policy},
};
use designcraft_geom::{Point, Rect};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

mod geometry;
mod text;
pub use geometry::{annotation_fragments, hit_annotation};
use text::{Sequence, append_fragments};

pub(crate) fn substitutions(doc: &Document, story: &Story, range: std::ops::Range<usize>, sub: &mut SubstCtx) {
    let Some(v) = &doc.publication else { return };
    for (mark, notes) in [(FOOTNOTE_REF, &story.notes), (ENDNOTE_REF, &story.endnotes)] {
        for ((at, _), note) in story.text.match_indices(mark).zip(notes) {
            if !range.contains(&at) {
                continue;
            }
            let Some(a) = &note.annotation else { continue };
            let Some(stream) = v.root.note_streams.get(&a.stream) else { continue };
            let label = if v.root.note_policy(v.edition, &a.stream).placement == Placement::Omit {
                String::new()
            } else {
                v.root.note_label(v.edition, a.id).unwrap_or_else(|| "?".into())
            };
            let mut format = designcraft_doc::CharFormat { style: stream.ref_char_style.clone(), ..Default::default() };
            format.over.position = Some(stream.ref_position);
            sub.rich.insert(
                at,
                designcraft_doc::publication::references::FieldText {
                    runs: vec![designcraft_doc::publication::references::RichRun { text: label, format, target: Some(a.id) }],
                    unresolved: Vec::new(),
                },
            );
        }
    }
    if let Some(witness) = v.root.editions.get(&v.edition).and_then(|e| e.apparatus_witness.as_deref()) {
        for n in story.notes.iter().chain(&story.endnotes) {
            let Some(a) = &n.apparatus else { continue };
            if a.edited(story) {
                continue;
            }
            let (Some(r), Some(reading)) = (a.range(story), a.choose(witness).and_then(|i| a.readings.get(i))) else { continue };
            if r.start < range.start
                || r.end > range.end
                || reading.text.text.contains('\n')
                || !reading.text.tables.is_empty()
                || !reading.text.objects.is_empty()
            {
                continue;
            }
            let runs = reading
                .text
                .runs()
                .map(|(r, f)| designcraft_doc::publication::references::RichRun {
                    text: designcraft_doc::xref::clean(&reading.text.text[r]),
                    format: f.clone(),
                    target: None,
                })
                .collect();
            sub.replacements.push((r, designcraft_doc::publication::references::FieldText { runs, unresolved: Vec::new() }));
        }
        sub.replacements.sort_by_key(|(r, _)| r.start);
    }
}

fn raw(doc: &Document, story: &Story, frames: &[FrameSpec], opts: &ComposeOptions) -> ComposedStory {
    let opts = ComposeOptions { suppress_annotations: true, ..opts.clone() };
    crate::compose_with_db(doc, story, frames, &opts, designcraft_fonts::FontDb::global())
}

fn entries(doc: &Document, story: &Story) -> Vec<model::Entry> {
    let Some(v) = &doc.publication else { return Vec::new() };
    v.root
        .annotation_entries()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.component == v.component && e.story == story.id)
        .map(|mut e| {
            if let Some(owner) = model::at_path(story, &e.path)
                && let Some(note) = if e.endnote { owner.endnote(e.note.id) } else { owner.note(e.note.id) }
            {
                e.note = Arc::new(note.clone());
            }
            e
        })
        .collect()
}

/// Frame and baseline of callouts, including those inside table cells and note fragments.
fn callouts(cs: &ComposedStory, ids: &BTreeSet<u64>) -> BTreeMap<u64, (usize, f64)> {
    fn walk(ft: &FrameText, fi: usize, y: f64, ids: &BTreeSet<u64>, out: &mut BTreeMap<u64, (usize, f64)>, depth: usize) {
        if depth > 32 {
            return;
        }
        for l in &ft.lines {
            for g in &l.glyphs {
                if let Some(id) = g.generated.as_ref().and_then(|g| g.target).filter(|id| ids.contains(id)) {
                    out.entry(id).or_insert((fi, y + l.baseline + l.descent));
                }
            }
        }
        for t in &ft.tables {
            for c in &t.cells {
                for f in &c.text.frames {
                    walk(f, fi, y + c.origin.y, ids, out, depth + 1);
                }
            }
        }
        for n in &ft.notes {
            for f in &n.text.frames {
                walk(f, fi, y + n.origin.y, ids, out, depth + 1);
            }
        }
    }
    let mut out = BTreeMap::new();
    for (fi, ft) in cs.frames.iter().enumerate() {
        walk(ft, fi, 0.0, ids, &mut out, 0);
    }
    out
}

pub(crate) fn compose(doc: &Document, story: &Story, frames: &[FrameSpec], opts: &ComposeOptions) -> ComposedStory {
    let Some(v) = &doc.publication else { return raw(doc, story, frames, opts) };
    if let Some(output) = v.root.note_output(v.component, story.id) {
        return endnotes(doc, story, output, frames, opts);
    }
    let entries = entries(doc, story);
    if entries.is_empty() {
        return raw(doc, story, frames, opts);
    }
    let ids: BTreeSet<_> = entries.iter().map(|e| e.identity.id).collect();
    let mut streams: BTreeMap<String, (Policy, Vec<model::Entry>)> = BTreeMap::new();
    for e in entries {
        let policy = v.root.note_policy(v.edition, &e.identity.stream);
        if matches!(policy.placement, Placement::Footnote | Placement::Sidenote) {
            streams.entry(e.identity.stream.clone()).or_insert((policy, Vec::new())).1.push(e);
        }
    }
    if streams.is_empty() {
        return raw(doc, story, frames, opts);
    }
    let mut body = frames.to_vec();
    let mut side_areas = BTreeMap::new();
    for (name, (policy, _)) in &streams {
        if policy.placement == Placement::Sidenote {
            let mut areas = Vec::new();
            for f in &mut body {
                let left = matches!(policy.side, model::Side::Left) || (policy.side == model::Side::Outer && f.left_page);
                let area = if left {
                    let r = Rect::new(f.area.x0, f.area.y0, f.area.x0 + policy.side_width, f.area.y1);
                    f.area.x0 += policy.side_width + policy.side_gap;
                    r
                } else {
                    let r = Rect::new(f.area.x1 - policy.side_width, f.area.y0, f.area.x1, f.area.y1);
                    f.area.x1 -= policy.side_width + policy.side_gap;
                    r
                };
                areas.push(area);
            }
            side_areas.insert(name.clone(), areas);
        }
    }
    if body.iter().any(|f| f.area.width() < 24.0) {
        let mut out = raw(doc, story, frames, opts);
        out.diagnostics.push("Sidenote lanes leave less than 24 pt for the body; widen the frame or reduce the note width.".into());
        return out;
    }
    let mut reserves = vec![0.0f64; frames.len()];
    let mut previous = None;
    let mut last = ComposedStory::default();
    for pass in 0..16 {
        let adjusted: Vec<_> = body
            .iter()
            .zip(&reserves)
            .map(|(f, h)| FrameSpec { area: Rect::new(f.area.x0, f.area.y0, f.area.x1, (f.area.y1 - h).max(f.area.y0 + 1.0)), ..f.clone() })
            .collect();
        let mut out = raw(doc, story, &adjusted, opts);
        // Nested callouts become available after their containing note has been placed.
        let mut calls = callouts(&last, &ids);
        calls.extend(callouts(&out, &ids));
        let mut heights = vec![0.0f64; frames.len()];
        let mut placed = BTreeMap::new();
        let mut hard = Vec::new();
        let mut awaiting = BTreeSet::new();
        let foot_count = streams.values().filter(|(p, _)| p.placement == Placement::Footnote).count().max(1) as f64;
        for (name, (policy, notes)) in &streams {
            let active: Vec<_> = notes.iter().filter(|e| calls.contains_key(&e.identity.id)).cloned().collect();
            if active.is_empty() {
                continue;
            }
            if policy.placement == Placement::Sidenote {
                sidenotes(doc, name, policy, &active, &calls, side_areas.get(name).map_or(&[], Vec::as_slice), frames, opts, &mut out);
                continue;
            }
            awaiting.extend(active.iter().map(|e| e.identity.id));
            let seq = Sequence::build(doc, name, &active, policy, false, &calls);
            let specs: Vec<_> = frames
                .iter()
                .map(|f| {
                    let h = (f.area.height() * policy.max_fraction / foot_count - policy.space_before).max(1.0);
                    let mut n = f.clone();
                    n.id = ItemId(0);
                    n.area = Rect::new(0.0, 0.0, f.area.width(), h);
                    n.opts = designcraft_doc::TextFrameOptions {
                        columns: policy.columns,
                        gutter: policy.gutter,
                        first_baseline: designcraft_doc::FirstBaseline::Ascent,
                        ..Default::default()
                    };
                    n.exclusions.clear();
                    n.grid = None;
                    n
                })
                .collect();
            let nopts = ComposeOptions { note_releases: Some(Arc::new(seq.releases.clone())), ..opts.clone() };
            let cs = raw(doc, &seq.story, &specs, &nopts);
            let used: Vec<_> = cs.frames.iter().map(text::height).collect();
            let offsets: Vec<_> = frames
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let h = used.get(i).copied().unwrap_or(0.0);
                    if h > 0.0 {
                        heights[i] += h + policy.space_before;
                    }
                    Point::new(f.area.x0, f.area.y1 - heights[i] + policy.space_before)
                })
                .collect();
            let first = append_fragments(&seq, &cs, &offsets, &mut out);
            placed.extend(first);
            if cs.is_overset() {
                out.annotation_overflow.extend(seq.blocks.iter().filter(|b| cs.overset_at.is_some_and(|at| at < b.end)).map(|b| b.entry.identity.id));
            }
        }
        let mut forced = false;
        for (id, (fi, y)) in &calls {
            if awaiting.contains(id)
                && placed.get(id).is_none_or(|actual| actual > fi)
                && let Some(f) = frames.get(*fi)
            {
                // Force the callout line into a frame where its initial note fragment fits.
                let wanted = (f.area.y1 - y + 1.0).max(heights[*fi]);
                if wanted >= f.area.height() - 8.0 {
                    hard.push(format!("Note {id} cannot place its first fragment beside its callout; enlarge the note area."));
                } else {
                    heights[*fi] = wanted;
                    forced = true;
                }
            }
        }
        out.diagnostics.extend(hard);
        let signature = (calls.clone(), heights.iter().map(|h| h.to_bits()).collect::<Vec<_>>());
        let grew = heights.iter().zip(&reserves).any(|(a, b)| *a > b + 0.01);
        for (r, h) in reserves.iter_mut().zip(heights) {
            *r = r.max(h);
        }
        if (!grew && !forced) || previous.as_ref() == Some(&signature) {
            if forced && out.diagnostics.is_empty() {
                out.diagnostics.push("Note callouts and their initial fragments did not converge.".into());
            }
            return out;
        }
        previous = Some(signature);
        last = out;
        if pass == 15 {
            last.diagnostics.push("Body and note areas did not converge within 16 passes.".into());
        }
    }
    last
}

fn endnotes(doc: &Document, story: &Story, output: &model::Output, frames: &[FrameSpec], opts: &ComposeOptions) -> ComposedStory {
    let Some(v) = &doc.publication else { return raw(doc, story, frames, opts) };
    let policy = v.root.note_policy(v.edition, &output.stream);
    if !matches!((policy.placement, output.source), (Placement::ComponentEnd, Some(_)) | (Placement::PublicationEnd, None)) {
        return raw(doc, story, frames, opts);
    }
    let entries: Vec<_> = v
        .root
        .annotation_entries()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.identity.stream == output.stream && output.source.is_none_or(|c| c == e.component))
        .map(|mut e| {
            // Use this edition's rich note appearance, including styles in another component.
            if let Ok(d) = v.root.project(v.edition, e.component)
                && let Some(st) = d.story(e.story).and_then(|s| model::at_path(s, &e.path))
                && let Some(n) = if e.endnote { st.endnote(e.note.id) } else { st.note(e.note.id) }
            {
                e.note = Arc::new(n.clone());
            }
            e
        })
        .collect();
    let seq = Sequence::build(doc, &output.stream, &entries, &policy, true, &BTreeMap::new());
    let specs: Vec<_> = frames
        .iter()
        .map(|f| {
            let mut n = f.clone();
            n.opts.columns = policy.columns;
            n.opts.gutter = policy.gutter;
            n
        })
        .collect();
    let cs = raw(doc, &seq.story, &specs, opts);
    let mut out = ComposedStory {
        story: story.id,
        rev: story.rev,
        frames: frames.iter().map(|f| FrameText { frame: f.id, columns: f.columns(), ..Default::default() }).collect(),
        ..Default::default()
    };
    // The heading belongs to the generated carrier; note fragments retain their source identity.
    for (source, target) in cs.frames.iter().zip(&mut out.frames) {
        target.lines = source.lines.iter().filter(|l| l.range.start < seq.heading_end).cloned().collect();
        target.decos = source.decos.clone();
    }
    out.source = Some(Arc::new(seq.story.clone()));
    out.styles = cs.styles.clone();
    append_fragments(&seq, &cs, &vec![Point::ZERO; frames.len()], &mut out);
    if let Some(at) = cs.overset_at {
        out.annotation_overflow = seq.blocks.iter().filter(|b| at < b.end).map(|b| b.entry.identity.id).collect();
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn sidenotes(
    doc: &Document,
    name: &str,
    policy: &Policy,
    entries: &[model::Entry],
    calls: &BTreeMap<u64, (usize, f64)>,
    areas: &[Rect],
    frames: &[FrameSpec],
    opts: &ComposeOptions,
    out: &mut ComposedStory,
) {
    let mut bottoms = vec![0.0f64; frames.len()];
    for e in entries {
        let Some(&(fi, y)) = calls.get(&e.identity.id) else { continue };
        let (Some(area), Some(f)) = (areas.get(fi), frames.get(fi)) else { continue };
        let top = y.max(area.y0).max(bottoms[fi] + policy.space_between);
        if top >= area.y1 {
            out.diagnostics.push(format!("Sidenote {} overflows page {}.", e.identity.id, f.page.map_or(fi + 1, |p| p + 1)));
            continue;
        }
        let seq = Sequence::build(doc, name, std::slice::from_ref(e), policy, false, &BTreeMap::new());
        let spec = FrameSpec {
            area: Rect::new(0.0, 0.0, area.width(), area.y1 - top),
            opts: designcraft_doc::TextFrameOptions { first_baseline: designcraft_doc::FirstBaseline::Ascent, ..Default::default() },
            exclusions: Vec::new(),
            grid: None,
            ..f.clone()
        };
        let cs = raw(doc, &seq.story, std::slice::from_ref(&spec), opts);
        let h = cs.frames.first().map_or(0.0, text::height);
        bottoms[fi] = top + h;
        let mut target = ComposedStory { frames: vec![FrameText::default()], ..Default::default() };
        append_fragments(&seq, &cs, &[Point::new(area.x0, top)], &mut target);
        out.annotation_progress += target.annotation_progress;
        if let (Some(dst), Some(src)) = (out.frames.get_mut(fi), target.frames.first_mut()) {
            dst.notes.append(&mut src.notes);
        }
        if cs.is_overset() {
            out.diagnostics.push(format!(
                "Sidenote {} overflows page {}; reduce its size or choose another placement.",
                e.identity.id,
                f.page.map_or(fi + 1, |p| p + 1)
            ));
        }
    }
}

#[cfg(test)]
mod tests;
