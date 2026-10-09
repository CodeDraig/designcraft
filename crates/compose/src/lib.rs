//! DesignCraft text composition.
//!
//! [`compose_story`] lays a story out through its chain of threaded frames:
//! style resolution → shaping ([`shape`]) → line breaking ([`breaker`]: the paragraph composer is
//! Knuth–Plass total fit, the single-line composer is greedy) → placement in columns and frames
//! (first-baseline offset, leading, space before/after, baseline grid, text wrap, column/frame
//! breaks, vertical justification) → overset detection.
//!
//! Output coordinates are each frame's *inner* space (apply the frame item's `xf` for spread space).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod output;
pub use output::{ComposedStory, Deco, FrameText, Line, PlacedGlyph, PlacedNote, PlacedObject, Rule, RunStyle, upright_in_vertical};
mod frames;
pub use frames::{Exclusion, FrameSpec, frame_specs};
mod caret;
pub use caret::{caret, caret_x, hit, line_rtl, visual_step};
mod path_text;
pub use path_text::path_glyphs;
mod hyphenation;
pub use hyphenation::is_english;
use hyphenation::{foreign_ranges, hyphenation_points};
mod line;
pub use line::hj_severity;
use line::{apply_desired_spacing, cjk_line_reference, layout_line, line_metrics, spacing_for};
mod wrap;
use wrap::{estimate_slots, free_slot};
mod decoration;
use decoration::para_box_decos;

mod bidi;
pub mod breaker;
mod cache;
pub mod hyphen;
mod notes;
mod overlay;
mod ruby;
pub mod shape;
pub mod table;
pub mod vars;
pub mod xref;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use designcraft_doc::{
    Align, Composer, Document, FirstBaseline, GridAlign, ItemId, ParaProps, SpanColumns, StartParagraph, Story, StoryId, TabAlign, TextFrameOptions,
    VerticalJustification, WrapMode, story,
};
use designcraft_fonts::{FontDb, ScopedFonts};
use designcraft_geom::{Point, Rect};

use crate::breaker::{Break, Spacing};
pub use crate::cache::Cache;
use crate::notes::Notes;
pub use crate::notes::{find_note, hit_note, note_caret};
use crate::shape::{Glyph, StyleTable, SubstCtx};
pub use crate::table::{PlacedCell, StrokeSeg, TableFrag, cell_caret, find_cell, hit_cell};

/// Composition inputs that are not in the story itself.
#[derive(Clone, Debug, Default)]
pub struct ComposeOptions {
    /// Page name for page-number markers when the story is composed for a specific page (parent items).
    pub page_name: Option<String>,
    /// Absolute page for text variables when composed for a specific page (parent items).
    pub page: Option<usize>,
    /// Running-header index (needed when the story contains running-header variables).
    pub running: Option<std::sync::Arc<vars::RunningIndex>>,
    /// Generated text before the first paragraph (a footnote's number and separator).
    pub label: Option<String>,
    /// Anchor pages for cross-references (needed when the story contains cross-references).
    pub xrefs: Option<Arc<xref::XrefIndex>>,
}

/// Compose a story of `doc` through its frames.
pub fn compose_story(doc: &Document, sid: StoryId, opts: &ComposeOptions) -> ComposedStory {
    let specs = frame_specs(doc, sid);
    match doc.story(sid) {
        Some(st) => compose(doc, st, &specs, opts),
        None => ComposedStory { story: sid, ..Default::default() },
    }
}

const DEFAULT_TAB: f64 = 36.0;

/// Compose `story` into `frames`.
pub fn compose(doc: &Document, story: &Story, frames: &[FrameSpec], opts: &ComposeOptions) -> ComposedStory {
    compose_with_db(doc, story, frames, opts, FontDb::global())
}

fn compose_with_db(doc: &Document, story: &Story, frames: &[FrameSpec], opts: &ComposeOptions, db: &FontDb) -> ComposedStory {
    let db = &db.scoped(doc.font_scope);
    let mut out = ComposedStory { story: story.id, rev: story.rev, text_len: story.text.len(), ..Default::default() };
    let mut styles_tab: Vec<RunStyle> = Vec::new();
    let mut missing_fonts: HashMap<String, bool> = HashMap::new();
    let cols: Vec<Vec<Rect>> = frames
        .iter()
        .map(|f| {
            let mut columns = f.columns();
            if story.direction == designcraft_doc::TextDirection::RightToLeft && !f.vertical {
                columns.reverse();
            }
            columns
        })
        .collect();
    out.frames = frames
        .iter()
        .zip(&cols)
        .map(|(f, columns)| FrameText { frame: f.id, vertical: f.vertical, columns: columns.clone(), ..Default::default() })
        .collect();
    let mut cur = Cursor { fi: 0, col: 0, last_baseline: None, last_descent: 0.0, last_reference: 0.0, pending: 0.0 };
    let para_ranges = story.para_ranges();
    let np = para_ranges.len();
    let hyph_exceptions = doc.hyphenation_exception_map();
    let mut list_counter: u32 = 0;
    // Named lists number independently of layout: one pass up front.
    let named_numbers: Vec<Option<u32>> = {
        let mut counters: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        story
            .paras
            .iter()
            .map(|p| {
                let (pp, _) = doc.styles.resolve_para(p);
                if pp.list_type != designcraft_doc::ListType::Numbers || pp.list_name.is_empty() {
                    return None;
                }
                let c = counters.entry(pp.list_name.clone()).or_insert_with(|| doc.list_start(story.id, &pp.list_name));
                *c = pp.start_at.map_or(*c + 1, |s| s.max(1));
                Some(*c)
            })
            .collect()
    };
    // Keep options: paragraphs are re-laid from a snapshot when a keep is violated, either forced
    // into the next column or with a cap on the lines set before moving on (widow control).
    let mut snaps: Vec<Snapshot> = Vec::with_capacity(np);
    let mut info: Vec<ParaInfo> = Vec::with_capacity(np);
    let mut force_col = vec![false; np];
    let mut line_cap: Vec<Option<usize>> = vec![None; np];
    let mut restores = 0usize;
    let mut pi = 0;
    let var_story = vars::has_vars(story);
    let mut notes = Notes::new(doc, story);
    let mut note_snaps: Vec<(u32, Option<usize>, usize)> = Vec::with_capacity(np);
    let mut var_cache: std::collections::HashMap<Option<usize>, std::sync::Arc<Vec<String>>> = Default::default();
    'paras: while pi < np {
        let prange = para_ranges[pi].clone();
        let snap = Snapshot::take(&out, &cur, list_counter);
        snaps.truncate(pi);
        snaps.push(snap);
        note_snaps.truncate(pi);
        note_snaps.push((notes.num, notes.key, notes.placed.len()));
        info.truncate(pi);
        let pf = &story.paras[pi];
        let (pp, base_chars) = doc.styles.resolve_para(pf);
        if let Some(t) = story.para_table(pi) {
            if cur.last_baseline.is_some() {
                match pp.start_paragraph {
                    StartParagraph::NextColumn => cur.next_column(&cols),
                    StartParagraph::NextFrame | StartParagraph::NextPage | StartParagraph::NextOddPage | StartParagraph::NextEvenPage => {
                        cur.next_frame()
                    }
                    StartParagraph::Anywhere => {}
                }
            }
            let at_top = cur.last_baseline.is_none();
            let start_at = (cur.fi, cur.col);
            let anchor = prange.start + story.text[prange.clone()].find(story::TABLE_ANCHOR).unwrap_or(0);
            if !table::place_table(doc, t, anchor, pi, &pp, frames, &cols, &mut cur, &mut out, opts) {
                out.overset_at = Some(prange.start);
                break 'paras;
            }
            info.push(ParaInfo {
                start: start_at,
                at_top,
                end: (cur.fi, cur.col),
                lines: 1,
                lines_in_end_col: 1,
                keep_with_next: 0,
                keep_together: false,
                keep_all: false,
                keep_first: 1,
                keep_last: 1,
            });
            list_counter = 0;
            cur.pending += pp.space_after;
            pi += 1;
            continue;
        }
        // Bullets & numbering: generated prefix (shaped as its own glyphs, mapped to the paragraph start).
        let cur_frame = frames.get(cur.fi.min(frames.len().saturating_sub(1)));
        let var_values = if var_story {
            let page = opts.page.or_else(|| cur_frame.and_then(|f| f.page));
            var_cache.entry(page).or_insert_with(|| std::sync::Arc::new(vars::values(doc, page, opts.running.as_deref()))).clone()
        } else {
            Default::default()
        };
        let mut sub = SubstCtx {
            page_name: opts.page_name.clone().or_else(|| cur_frame.and_then(|f| f.page_name.clone())),
            section_marker: None,
            vars: var_values,
            hidden_conditions: doc.conditions.iter().filter(|c| !c.visible).map(|c| c.name.clone()).collect(),
            vertical: cur_frame.is_some_and(|f| f.vertical),
            ..Default::default()
        };
        if !story.endnotes.is_empty() {
            // Endnote reference numbers continue through the document.
            let first = doc.endnote_start(story.id) + story.text[..prange.start].matches(designcraft_doc::ENDNOTE_REF).count() as u32;
            for (k, (i, _)) in story.text[prange.clone()].match_indices(designcraft_doc::ENDNOTE_REF).enumerate() {
                sub.notes.insert(prange.start + i, doc.endnote_options.label(first + k as u32));
            }
        }
        if !story.objects.is_empty() {
            let before = story.text[..prange.start].matches(designcraft_doc::OBJECT_MARK).count();
            for (k, (i, _)) in story.text[prange.clone()].match_indices(designcraft_doc::OBJECT_MARK).enumerate() {
                let Some(o) = story.objects.get(before + k) else { continue };
                let (w, h) = o.size();
                let (y_offset, space, custom) = match &o.position {
                    designcraft_doc::AnchorPosition::Inline { y_offset } => (Some(*y_offset), 0.0, false),
                    designcraft_doc::AnchorPosition::AboveLine { space_before, space_after, .. } => (None, space_before + space_after, false),
                    designcraft_doc::AnchorPosition::Custom { .. } => (Some(0.0), 0.0, true),
                };
                sub.objects.insert(prange.start + i, shape::ObjectSpec { index: before + k, w, h, y_offset, space, custom });
            }
        }
        if !story.xrefs.is_empty() {
            sub.xrefs = xref::texts_in(doc, story, prange.clone(), opts.xrefs.as_deref());
        }
        if notes.active() {
            notes.number_refs(doc, prange.clone(), cur_frame.and_then(|f| f.page), &mut sub);
        }
        let sub_objects = sub.objects.clone();
        let mut table = StyleTable { styles: &mut styles_tab, missing: &mut missing_fonts };
        let env = shape::TypeEnv { auto_leading: pp.auto_leading, adv: doc.settings.advanced_type, glyph_fallback: doc.settings.glyph_fallback };
        let mut sp = shape::shape_para(
            db,
            &doc.styles,
            story,
            pi,
            prange.clone(),
            &base_chars,
            env,
            &sub,
            &mut table,
            &pp.nested_styles,
            &pp.grep_styles,
            &[],
        );
        if !pp.nested_line_styles.is_empty() && cur.fi < frames.len() {
            // Nested line styles: find where the first lines end in this column, restyle them, and
            // look again (the style changes the widths) until the lines settle.
            let cols_here = &cols[cur.fi];
            let col = cols_here[cur.col.min(cols_here.len() - 1)];
            let spacing = spacing_for(&pp, base_chars.size);
            let mut lines: Vec<(std::ops::Range<usize>, String)> = Vec::new();
            for _ in 0..3 {
                let width = |j: usize| (col.width() - pp.left_indent - pp.right_indent - if j == 0 { pp.first_line_indent } else { 0.0 }).max(1.0);
                // The same spacing, hyphenation and breaker as the layout below.
                let mut gl = sp.glyphs.clone();
                apply_desired_spacing(&mut gl, &pp);
                let hy = hyphenation_points(&story.text, &gl, &pp, &hyph_exceptions, &foreign_ranges(doc, story, prange.clone(), &base_chars));
                let breaks = if pp.composer == Composer::SingleLine || gl.iter().any(|g| g.ch == '\t') || gl.len() > 4000 {
                    breaker::greedy(&gl, &hy, &spacing, &width)
                } else if pp.balance_ragged && !spacing.justify {
                    breaker::balanced(&gl, &hy, &spacing, &width)
                } else {
                    breaker::knuth_plass(&gl, &hy, &spacing, &width)
                };
                // Byte where line `k` starts (the paragraph end past the last line).
                let byte_at =
                    |k: usize| if k == 0 { prange.start } else { breaks.get(k - 1).and_then(|b| gl.get(b.next)).map_or(prange.end, |g| g.byte) };
                let mut want = Vec::new();
                let mut line = 0usize;
                for nl in &pp.nested_line_styles {
                    if line >= breaks.len() || nl.lines == 0 {
                        break;
                    }
                    let end = line + nl.lines as usize;
                    if !nl.style.is_empty() && nl.style != designcraft_doc::NO_CHAR_STYLE {
                        want.push((byte_at(line)..byte_at(end).max(byte_at(line)), nl.style.clone()));
                    }
                    line = end;
                }
                if want == lines {
                    break;
                }
                lines = want;
                sp = shape::shape_para(
                    db,
                    &doc.styles,
                    story,
                    pi,
                    prange.clone(),
                    &base_chars,
                    env,
                    &sub,
                    &mut table,
                    &pp.nested_styles,
                    &pp.grep_styles,
                    &lines,
                );
            }
        }
        // List labels take the default super/subscript settings.
        let label_env = shape::TypeEnv { adv: Default::default(), ..env };
        match pp.list_type {
            designcraft_doc::ListType::Numbers if !pp.list_name.is_empty() => {
                // A named list: carries on past other paragraphs (and from earlier stories).
                let n = named_numbers.get(pi).copied().flatten().unwrap_or(1);
                let label = format!("{}.{}", pp.number_style.format(n), pp.list_separator);
                prepend_label(db, &mut sp.glyphs, &label, prange.start, &base_chars, label_env, &mut table);
            }
            designcraft_doc::ListType::Numbers => {
                list_counter = pp.start_at.map_or(list_counter + 1, |s| s.max(1));
                let label = format!("{}.{}", pp.number_style.format(list_counter), pp.list_separator);
                prepend_label(db, &mut sp.glyphs, &label, prange.start, &base_chars, label_env, &mut table);
            }
            designcraft_doc::ListType::Bullets => {
                let label = format!("{}{}", pp.bullet_char, pp.list_separator);
                prepend_label(db, &mut sp.glyphs, &label, prange.start, &base_chars, label_env, &mut table);
            }
            designcraft_doc::ListType::None => list_counter = 0,
        }
        if pi == 0
            && let Some(label) = &opts.label
        {
            prepend_label(db, &mut sp.glyphs, label, prange.start, &base_chars, label_env, &mut table);
        }
        let mut glyphs = sp.glyphs;
        let bidi_text = bidi::paragraph_text(&mut glyphs, &story.text);
        let bidi_info = unicode_bidi::BidiInfo::new(
            &bidi_text,
            Some(if pp.direction == designcraft_doc::TextDirection::RightToLeft { unicode_bidi::Level::rtl() } else { unicode_bidi::Level::ltr() }),
        );
        bidi::resolve_mirroring(&mut glyphs, &bidi_info);
        apply_desired_spacing(&mut glyphs, &pp);
        let hyph_after = hyphenation_points(&story.text, &glyphs, &pp, &hyph_exceptions, &foreign_ranges(doc, story, prange.clone(), &base_chars));
        let base_size = base_chars.size;
        let base_leading = match base_chars.leading {
            designcraft_doc::Leading::Auto => base_size * pp.auto_leading,
            designcraft_doc::Leading::Points(v) => v,
        };
        let spacing = spacing_for(&pp, base_size);
        // Paragraph start options.
        if force_col[pi] {
            cur.next_column(&cols);
        }
        if cur.last_baseline.is_some() {
            match pp.start_paragraph {
                StartParagraph::NextColumn => cur.next_column(&cols),
                StartParagraph::NextFrame | StartParagraph::NextPage | StartParagraph::NextOddPage | StartParagraph::NextEvenPage => cur.next_frame(),
                StartParagraph::Anywhere => {}
            }
        }
        if cur.last_baseline.is_some() {
            cur.pending += pp.space_before;
        }
        let at_top = cur.last_baseline.is_none();
        let start_at = (cur.fi, cur.col);
        // Rule above / shading track the paragraph's first line.
        let mut g0 = 0usize;
        let mut line_no = 0usize;
        // Paragraph line number of the first line in the current column.
        let mut col_first_line = 0usize;
        let has_tabs = glyphs.iter().any(|g| g.ch == '\t');
        let mut first_line_rect: Option<(usize, f64, f64, f64)> = None; // frame, baseline, ascent, x-span
        loop {
            if cur.fi >= frames.len() {
                out.overset_at = Some(glyphs.get(g0).map(|g| g.byte).unwrap_or(prange.start));
                // The part of the paragraph that fits keeps its shading and border.
                para_box_decos(&mut out, pi, &pp);
                break 'paras;
            }
            let f = &frames[cur.fi];
            let col = cols[cur.fi][cur.col.min(cols[cur.fi].len() - 1)];
            let col = match pp.span_columns {
                SpanColumns::Span(n) if cur.col == 0 => {
                    let n = if n == 0 { cols[cur.fi].len() } else { (n as usize).min(cols[cur.fi].len()) };
                    let other = cols[cur.fi][n - 1];
                    Rect::new(col.x0.min(other.x0), col.y0, col.x1.max(other.x1), col.y1)
                }
                _ => col,
            };
            // Estimate slots for the breaker with the paragraph's base leading.
            let est_first = cur.next_baseline(f, col, base_leading, base_chars.size * 0.75, &pp);
            let slots = estimate_slots(f, col, est_first, base_leading, base_chars.size, &glyphs[g0..], &pp, line_no);
            let width = |j: usize| -> f64 {
                let (x0, x1) = slots.get(j).copied().unwrap_or((col.x0, col.x1));
                let ind = pp.left_indent + pp.right_indent + if line_no + j == 0 { pp.first_line_indent } else { 0.0 };
                (x1 - x0 - ind).max(1.0)
            };
            let rest = &glyphs[g0..];
            let rest_h = &hyph_after[g0..];
            let breaks: Vec<Break> = if pp.composer == Composer::SingleLine || has_tabs || rest.len() > 4000 {
                breaker::greedy(rest, rest_h, &spacing, &width)
            } else if pp.balance_ragged && !spacing.justify {
                breaker::balanced(rest, rest_h, &spacing, &width)
            } else {
                breaker::knuth_plass(rest, rest_h, &spacing, &width)
            };
            let mut moved = false;
            for (k, b) in breaks.iter().enumerate() {
                let (s, e) = (g0 + b.start, g0 + b.end);
                let line_glyphs = &glyphs[s..e.max(s)];
                let (asc, desc, lead) = line_metrics(line_glyphs, &glyphs, s, base_leading, base_chars.size, db, &base_chars);
                let reference = cjk_line_reference(line_glyphs);
                let mut baseline = cur.next_baseline(f, col, lead, asc, &pp);
                if cur.last_baseline.is_some() {
                    baseline += cur.last_reference - reference;
                }
                // Baseline grid.
                if let Some((g_start, inc)) = f.grid
                    && (pp.grid_align == GridAlign::AllLines || (pp.grid_align == GridAlign::FirstLineOnly && line_no == 0))
                    && inc > 0.0
                {
                    let n = ((baseline - g_start) / inc - 1e-6).ceil();
                    baseline = g_start + n * inc;
                }
                // Wrap: push the line down until a slot exists.
                let (mut x0, mut x1) = (col.x0, col.x1);
                if !f.exclusions.is_empty() {
                    let mut tries = 0;
                    loop {
                        match free_slot(f, col, baseline - asc, baseline + desc, base_size) {
                            Some((a, b)) => {
                                x0 = a;
                                x1 = b;
                                break;
                            }
                            None => {
                                baseline += 1.0;
                                tries += 1;
                                if baseline > col.y1 || tries > 4000 {
                                    break;
                                }
                            }
                        }
                    }
                }
                let capped = line_cap[pi] == Some(line_no) && line_no > col_first_line;
                // Footnotes referenced on this line need room at the bottom of the column too
                // (a line at the top of a column is set anyway).
                let line_notes = if notes.active() { notes.refs_in(line_glyphs) } else { Vec::new() };
                let col_w = cols[cur.fi][cur.col.min(cols[cur.fi].len() - 1)].width();
                let reserve = notes.reserve(doc, cur.fi, cur.col, &line_notes, col_w, f, opts);
                let reserve = if cur.last_baseline.is_none() { notes.reserve(doc, cur.fi, cur.col, &[], col_w, f, opts) } else { reserve };
                let fits = baseline + desc <= col.y1 - reserve + 0.01 && !capped;
                if !fits {
                    if !capped {
                        let ctx = KeepCtx {
                            pi,
                            line_no,
                            in_col: line_no - col_first_line,
                            started_here: col_first_line == 0,
                            at_top,
                            remaining: breaks.len() - k,
                            here: (cur.fi, cur.col),
                        };
                        let action = if restores < 4 * np + 64 { keep_violation(&ctx, &pp, &info, &force_col, &line_cap) } else { None };
                        if let Some(action) = action {
                            restores += 1;
                            let j = match action {
                                KeepAction::Force(j) => {
                                    force_col[j] = true;
                                    j
                                }
                                KeepAction::Cap(j, n) => {
                                    line_cap[j] = Some(n);
                                    j
                                }
                            };
                            for k in j + 1..np {
                                force_col[k] = false;
                                line_cap[k] = None;
                            }
                            snaps[j].restore(&mut out, &mut cur, &mut list_counter);
                            (notes.num, notes.key) = (note_snaps[j].0, note_snaps[j].1);
                            notes.placed.truncate(note_snaps[j].2);
                            pi = j;
                            continue 'paras;
                        }
                    }
                    // Next column / frame; re-break the rest of the paragraph there.
                    g0 = s;
                    cur.next_column(&cols);
                    col_first_line = line_no;
                    moved = true;
                    break;
                }
                let ind_l = pp.left_indent + if line_no == 0 { pp.first_line_indent } else { 0.0 };
                let lx0 = x0 + ind_l;
                let lx1 = x1 - pp.right_indent;
                let last = k + 1 == breaks.len();
                let (mut placed, end_x, ratio) =
                    layout_line(&glyphs, s, e, b.hyphen, lx0, lx1, col.x0, &pp, &spacing, last, b.forced && !last, f.left_page, &bidi_info);
                ruby::annotate(db, &styles_tab, &mut placed, doc.settings.glyph_fallback);
                let range_end = if last { prange.end } else { glyphs.get(g0 + b.next).map(|g| g.byte).unwrap_or(prange.end) };
                let range_start = glyphs.get(s).map(|g| g.byte).unwrap_or(prange.start).min(range_end);
                let range_start = if line_no == 0 { prange.start } else { range_start };
                let ft = &mut out.frames[cur.fi];
                if line_no == 0 {
                    first_line_rect = Some((cur.fi, baseline, asc, x0));
                }
                ft.lines.push(Line {
                    column: cur.col as u32,
                    baseline,
                    x0,
                    x1,
                    ascent: asc,
                    descent: desc,
                    leading: lead,
                    range: range_start..range_end,
                    para: pi,
                    glyphs: placed,
                    hyphenated: b.hyphen,
                    first_in_para: line_no == 0,
                    last_in_para: last,
                    end_x,
                    spacing: ratio,
                    hj: hj_severity(ratio, pp.word_space_min, pp.word_space_max),
                    keep_violation: false,
                });
                for k in line_notes {
                    notes.place(doc, k, cur.fi, cur.col, col_w, f, opts);
                }
                if !sub_objects.is_empty() {
                    let ft = &mut out.frames[cur.fi];
                    let li = ft.lines.len() - 1;
                    let text_ascent = line_glyphs.iter().filter(|g| g.ch != designcraft_doc::OBJECT_MARK).map(|g| g.ascent).fold(0.0, f64::max);
                    let text_ascent = if text_ascent > 0.0 { text_ascent } else { base_chars.size * 0.75 };
                    for g in ft.lines[li].glyphs.iter().filter(|g| g.len > 0) {
                        if let Some(o) = sub_objects.get(&g.byte) {
                            ft.objects.push(PlacedObject { index: o.index, origin: Point::ZERO, size: (o.w, o.h), line: li, x: g.x, text_ascent });
                        }
                    }
                }
                cur.last_baseline = Some(baseline);
                cur.last_reference = reference;
                cur.last_descent = desc;
                cur.pending = 0.0;
                line_no += 1;
                // Column / frame / page break characters.
                if b.forced && e < glyphs.len() + 1 {
                    let brk = glyphs.get(g0 + b.next.saturating_sub(1)).map(|g| g.ch);
                    let jumped = match brk {
                        Some(story::COLUMN_BREAK) => {
                            cur.next_column(&cols);
                            true
                        }
                        Some(story::FRAME_BREAK) | Some(story::PAGE_BREAK) => {
                            cur.next_frame();
                            true
                        }
                        _ => false,
                    };
                    if jumped {
                        // The rest of the paragraph continues in the new column/frame: re-break it there.
                        col_first_line = line_no;
                        if k + 1 < breaks.len() {
                            g0 += b.next;
                            moved = true;
                            break;
                        }
                    }
                }
            }
            if !moved {
                break;
            }
        }
        info.push(ParaInfo {
            start: start_at,
            at_top,
            end: (cur.fi, cur.col),
            lines: line_no,
            lines_in_end_col: line_no - col_first_line,
            keep_with_next: pp.keep_with_next,
            keep_together: pp.keep_lines_together,
            keep_all: pp.keep_all_lines,
            keep_first: pp.keep_first as usize,
            keep_last: pp.keep_last as usize,
        });
        // Rules and shading for the paragraph.
        if let Some((fi, bl, asc, _)) = first_line_rect {
            let ft = &mut out.frames[fi];
            if pp.rule_above.on {
                let r = &pp.rule_above;
                let y = bl - asc - r.offset;
                let col = ft.lines.iter().rev().find(|l| l.para == pi).map(|l| (l.x0, l.x1)).unwrap_or((0.0, 0.0));
                ft.decos.push(Deco {
                    rect: Rect::new(col.0 + r.left_indent, y - r.weight, col.1 - r.right_indent, y),
                    color: r.color.clone(),
                    tint: r.tint,
                });
            }
        }
        para_box_decos(&mut out, pi, &pp);
        if pp.rule_below.on
            && let Some(ft) = out.frames.get_mut(cur.fi.min(frames.len().saturating_sub(1)))
            && let Some(l) = ft.lines.iter().rev().find(|l| l.para == pi)
        {
            let r = &pp.rule_below;
            let y = l.baseline + r.offset;
            let (x0, x1) = if r.column_width { (l.x0, l.x1) } else { (l.x0, l.end_x) };
            ft.decos.push(Deco { rect: Rect::new(x0 + r.left_indent, y, x1 - r.right_indent, y + r.weight), color: r.color.clone(), tint: r.tint });
        }
        cur.pending += pp.space_after;
        pi += 1;
    }
    notes.finish(doc, story, frames, &cols, &mut out);
    // Ranges, content heights and vertical justification.
    for (fi, ft) in out.frames.iter_mut().enumerate() {
        let f = &frames[fi];
        if let (Some(a), Some(z)) = (ft.lines.first(), ft.lines.last()) {
            ft.range = a.range.start..z.range.end;
            ft.content_height = z.baseline + z.descent - f.area.y0;
        } else {
            let p = ft_prev_end(&out.overset_at, story.text.len());
            ft.range = p..p;
        }
        let before: Vec<f64> = ft.tables.iter().map(|t| ft.lines.get(t.line).map_or(0.0, |l| l.baseline)).collect();
        vertical_justify(ft, f);
        for (t, b) in ft.tables.iter_mut().zip(before) {
            let now = ft.lines.get(t.line).map_or(b, |l| l.baseline);
            t.shift(now - b);
        }
        // Anchored objects follow their (finally placed) lines.
        if !ft.objects.is_empty() {
            for o in &mut ft.objects {
                let Some(l) = ft.lines.get(o.line) else { continue };
                let Some(obj) = story.objects.get(o.index) else { continue };
                let (w, h) = o.size;
                o.origin = match &obj.position {
                    designcraft_doc::AnchorPosition::Inline { y_offset } => Point::new(o.x, l.baseline - y_offset - h),
                    designcraft_doc::AnchorPosition::AboveLine { align, space_after, .. } => {
                        let x = match align {
                            designcraft_doc::anchored::AnchorAlign::Left => l.x0,
                            designcraft_doc::anchored::AnchorAlign::Center => (l.x0 + l.x1 - w) / 2.0,
                            designcraft_doc::anchored::AnchorAlign::Right => l.x1 - w,
                        };
                        Point::new(x, l.baseline - o.text_ascent - space_after - h)
                    }
                    designcraft_doc::AnchorPosition::Custom {
                        x_relative,
                        y_relative,
                        x_offset,
                        y_offset,
                        object_point,
                        ref_point,
                        keep_within_column,
                    } => {
                        use designcraft_doc::anchored::AnchorRelative as R;
                        let col = ft.columns.get(l.column as usize).copied().unwrap_or(f.area);
                        let area = |r: R| match r {
                            R::Anchor => None,
                            R::TextFrame => Some(f.area),
                            R::ColumnEdge => Some(col),
                            R::PageMargin => Some(f.page_rect.map_or(f.area, |p| p.1)),
                            R::PageEdge => Some(f.page_rect.map_or(f.area, |p| p.0)),
                        };
                        let frac = |i: u8| ([0.0, 0.5, 1.0][(i % 3) as usize], [0.0, 0.5, 1.0][(i / 3).min(2) as usize]);
                        let (rfx, rfy) = frac(*ref_point);
                        let (ofx, ofy) = frac(*object_point);
                        let rx = area(*x_relative).map_or(o.x, |a| a.x0 + a.width() * rfx);
                        let ry = area(*y_relative).map_or(l.baseline, |a| a.y0 + a.height() * rfy);
                        let mut y = ry + y_offset - h * ofy;
                        if *keep_within_column {
                            y = y.clamp(col.y0, (col.y1 - h).max(col.y0));
                        }
                        Point::new(rx + x_offset - w * ofx, y)
                    }
                };
            }
        }
    }
    // Frame ranges for empty frames after the text: start at the end of the text shown.
    let mut last_end = 0;
    for ft in &mut out.frames {
        if ft.lines.is_empty() {
            let p = out.overset_at.unwrap_or(last_end).max(last_end);
            ft.range = p..p;
        } else {
            last_end = ft.range.end;
        }
    }
    out.styles = styles_tab;
    mark_keep_violations(doc, story, &mut out);
    out
}

/// Flag the lines of paragraphs whose Keep Options the layout couldn't honour: lines kept
/// together that split, too few lines at the bottom (orphans) or top (widows) of a column, or a
/// keep-with-next paragraph separated from the next one.
fn mark_keep_violations(doc: &Document, story: &Story, out: &mut ComposedStory) {
    // Per paragraph, the columns its lines sit in: ((frame, column), line count) in order.
    let mut cols: HashMap<usize, Vec<((usize, u32), usize)>> = HashMap::new();
    for (fi, ft) in out.frames.iter().enumerate() {
        for l in &ft.lines {
            let v = cols.entry(l.para).or_default();
            match v.last_mut() {
                Some((c, n)) if *c == (fi, l.column) => *n += 1,
                _ => v.push(((fi, l.column), 1)),
            }
        }
    }
    let mut bad: Vec<usize> = Vec::new();
    for (&pi, v) in &cols {
        let Some(pf) = story.paras.get(pi) else { continue };
        let (pp, _) = doc.styles.resolve_para(pf);
        if pp.keep_lines_together && v.len() > 1 {
            let (first, last) = (v[0].1, v[v.len() - 1].1);
            if pp.keep_all_lines || first < pp.keep_first.max(1) as usize || last < pp.keep_last.max(1) as usize {
                bad.push(pi);
                continue;
            }
        }
        if pp.keep_with_next > 0
            && let (Some(end), Some(next)) = (v.last(), cols.get(&(pi + 1)))
            && next.first().is_some_and(|(c, n)| *c != end.0 || (*n < pp.keep_with_next as usize && next.len() > 1))
        {
            bad.push(pi);
        }
    }
    if bad.is_empty() {
        return;
    }
    for ft in &mut out.frames {
        for l in &mut ft.lines {
            if bad.contains(&l.para) {
                l.keep_violation = true;
            }
        }
    }
}

/// Lengths of the composed output and the cursor before a paragraph (keep resolution re-lays
/// paragraphs from here). Only the cursor's frame and later ones can change afterwards.
struct Snapshot {
    cur: Cursor,
    list_counter: u32,
    lines: usize,
    decos: usize,
    tables: usize,
}

impl Snapshot {
    fn take(out: &ComposedStory, cur: &Cursor, list_counter: u32) -> Snapshot {
        let (lines, decos, tables) = out.frames.get(cur.fi).map_or((0, 0, 0), |f| (f.lines.len(), f.decos.len(), f.tables.len()));
        Snapshot { cur: cur.clone(), list_counter, lines, decos, tables }
    }
    fn restore(&self, out: &mut ComposedStory, cur: &mut Cursor, list_counter: &mut u32) {
        let fi = self.cur.fi;
        for (i, f) in out.frames.iter_mut().enumerate().skip(fi) {
            let (l, d, t) = if i == fi { (self.lines, self.decos, self.tables) } else { (0, 0, 0) };
            f.lines.truncate(l);
            f.decos.truncate(d);
            f.tables.truncate(t);
            f.objects.retain(|o| o.line < l);
        }
        *cur = self.cur.clone();
        *list_counter = self.list_counter;
        out.overset_at = None;
    }
}

/// Where a placed paragraph sits (for keep resolution).
#[derive(Clone, Debug)]
struct ParaInfo {
    start: (usize, usize),
    /// Started at the top of a column (moving it can't help).
    at_top: bool,
    end: (usize, usize),
    lines: usize,
    lines_in_end_col: usize,
    keep_with_next: u32,
    keep_together: bool,
    keep_all: bool,
    keep_first: usize,
    keep_last: usize,
}

/// The paragraph being placed when a line doesn't fit its column.
struct KeepCtx {
    pi: usize,
    /// Paragraph line number of the line that doesn't fit.
    line_no: usize,
    /// Lines of this paragraph already in the column.
    in_col: usize,
    /// The paragraph started in this column.
    started_here: bool,
    at_top: bool,
    /// Lines left to set, including the one that doesn't fit.
    remaining: usize,
    here: (usize, usize),
}

enum KeepAction {
    /// Re-lay from paragraph `j`, starting it in the next column.
    Force(usize),
    /// Re-lay from paragraph `j`, moving to the next column after `n` of its lines.
    Cap(usize, usize),
}

/// Keep Options (plan: typography §8): keep with next, keep all lines together, keep first/last
/// lines. Returns how to re-lay, or None to just continue in the next column (also when a keep
/// can't be satisfied, e.g. a paragraph already at the top of a column).
fn keep_violation(ctx: &KeepCtx, pp: &ParaProps, info: &[ParaInfo], force_col: &[bool], line_cap: &[Option<usize>]) -> Option<KeepAction> {
    // The first paragraph of the chain of keep-with-next paragraphs ending in `pi` that sit in this column.
    let chain_start = |pi: usize| {
        let mut j = pi;
        while j > 0 {
            let p = &info[j - 1];
            let starts_here = if j == ctx.pi { ctx.started_here } else { info[j].start == ctx.here };
            if p.keep_with_next > 0 && p.end == ctx.here && starts_here {
                j -= 1;
            } else {
                break;
            }
        }
        j
    };
    let force = |j: usize| -> Option<KeepAction> {
        let top = if j == ctx.pi { ctx.at_top } else { info[j].at_top };
        let starts_here = if j == ctx.pi { ctx.started_here } else { info[j].start == ctx.here };
        (!(force_col[j] || top && starts_here)).then_some(KeepAction::Force(j))
    };
    let total_lines = ctx.line_no + ctx.remaining;
    // Keep with next: the previous paragraph's last line must share a column with our first lines.
    if ctx.started_here && ctx.pi > 0 {
        let prev = &info[ctx.pi - 1];
        let need = (prev.keep_with_next as usize).min(total_lines);
        if prev.keep_with_next > 0 && prev.end == ctx.here && ctx.line_no < need {
            let j = chain_start(ctx.pi);
            if j == ctx.pi - 1 && prev.lines_in_end_col > 1 && !(prev.keep_together && prev.keep_all) && line_cap[j].is_none() {
                // Move only the previous paragraph's last lines (respecting its widow control).
                let keep_last = if prev.keep_together { prev.keep_last.max(1) } else { 1 };
                let stay = prev.lines_in_end_col.saturating_sub(keep_last);
                let min_stay = if prev.start == ctx.here { if prev.keep_together { prev.keep_first.max(1) } else { 1 } } else { 1 };
                if stay >= min_stay {
                    return Some(KeepAction::Cap(j, prev.lines - keep_last));
                }
            }
            if let Some(a) = force(j) {
                return Some(a);
            }
        }
    }
    if !pp.keep_lines_together || ctx.in_col == 0 {
        return None;
    }
    if pp.keep_all_lines {
        return if ctx.started_here { force(chain_start(ctx.pi)) } else { None };
    }
    // Orphan control: at least `keep_first` lines at the bottom of the column.
    if ctx.started_here && ctx.in_col < (pp.keep_first as usize).max(1) {
        return force(chain_start(ctx.pi));
    }
    // Widow control: at least `keep_last` lines at the top of the next column.
    let keep_last = (pp.keep_last as usize).max(1);
    if ctx.remaining < keep_last {
        let need = keep_last - ctx.remaining;
        let min_stay = if ctx.started_here { (pp.keep_first as usize).max(1) } else { 1 };
        if ctx.in_col >= need + min_stay && line_cap[ctx.pi].is_none() {
            return Some(KeepAction::Cap(ctx.pi, ctx.line_no - need));
        }
        if ctx.started_here {
            return force(chain_start(ctx.pi));
        }
    }
    None
}

fn ft_prev_end(overset: &Option<usize>, len: usize) -> usize {
    overset.unwrap_or(len)
}

#[derive(Clone)]
struct Cursor {
    fi: usize,
    col: usize,
    last_baseline: Option<f64>,
    last_descent: f64,
    last_reference: f64,
    /// Space before/after waiting to be added to the next line.
    pending: f64,
}

impl Cursor {
    fn next_column(&mut self, cols: &[Vec<Rect>]) {
        self.col += 1;
        if self.fi < cols.len() && self.col >= cols[self.fi].len() {
            self.col = 0;
            self.fi += 1;
        }
        self.last_baseline = None;
        self.pending = 0.0;
    }
    fn next_frame(&mut self) {
        self.fi += 1;
        self.col = 0;
        self.last_baseline = None;
        self.pending = 0.0;
    }
    /// Baseline for the next line with leading `lead` and ascent `asc`.
    fn next_baseline(&self, f: &FrameSpec, col: Rect, lead: f64, asc: f64, _pp: &ParaProps) -> f64 {
        match self.last_baseline {
            Some(b) => b + lead + self.pending,
            None => {
                let off = match f.opts.first_baseline {
                    FirstBaseline::Ascent => asc,
                    FirstBaseline::CapHeight => asc * 0.72,
                    FirstBaseline::XHeight => asc * 0.5,
                    FirstBaseline::Leading => lead,
                    FirstBaseline::Fixed => 0.0,
                };
                col.y0 + off.max(f.opts.first_baseline_min)
            }
        }
    }
}

fn prepend_label(
    db: &ScopedFonts<'_>,
    glyphs: &mut Vec<Glyph>,
    label: &str,
    at: usize,
    base: &designcraft_doc::CharProps,
    env: shape::TypeEnv,
    table: &mut StyleTable<'_>,
) {
    // Shape the label as a tiny standalone story so it uses the paragraph's base character style.
    let mut tmp = Story::new(StoryId(0));
    tmp.insert(0, label);
    let styles = designcraft_doc::Styles::default();
    let shaped = shape::shape_para(db, &styles, &tmp, 0, 0..label.len(), base, env, &SubstCtx::default(), table, &[], &[], &[]);
    let mut pre: Vec<Glyph> = shaped
        .glyphs
        .into_iter()
        .map(|mut g| {
            g.byte = at;
            g.len = 0;
            g
        })
        .collect();
    pre.append(glyphs);
    *glyphs = pre;
}

fn vertical_justify(ft: &mut FrameText, f: &FrameSpec) {
    if ft.lines.is_empty() || f.opts.vertical_justification == VerticalJustification::Top {
        return;
    }
    // Per column.
    let ncols = ft.columns.len().max(1) as u32;
    for c in 0..ncols {
        let idx: Vec<usize> = ft.lines.iter().enumerate().filter(|(_, l)| l.column == c).map(|(i, _)| i).collect();
        let (Some(&a), Some(&z)) = (idx.first(), idx.last()) else { continue };
        let bottom = ft.lines[z].baseline + ft.lines[z].descent;
        let space = f.area.y1 - bottom;
        if space <= 0.0 {
            continue;
        }
        let _ = a;
        match f.opts.vertical_justification {
            VerticalJustification::Center => idx.iter().for_each(|&i| ft.lines[i].baseline += space / 2.0),
            VerticalJustification::Bottom => idx.iter().for_each(|&i| ft.lines[i].baseline += space),
            VerticalJustification::Justify if idx.len() > 1 => {
                let per = space / (idx.len() - 1) as f64;
                for (k, &i) in idx.iter().enumerate() {
                    ft.lines[i].baseline += per * k as f64;
                }
            }
            _ => {}
        }
    }
}

// ---------- caret and hit testing (Type tool) ----------

#[cfg(test)]
mod tests;
