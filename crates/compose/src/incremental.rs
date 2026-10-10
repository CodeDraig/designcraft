//! Paragraph checkpoints used by the composition cache. Full composition remains the oracle.
//! A checkpoint is taken before vertical justification, so resumed frames are finalized once.
use super::*;
use designcraft_doc::{CharFormat, ParaFormat, publication::references::FieldValues};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct WorkReport {
    pub mode: String,
    pub reason: Option<String>,
    pub paragraphs: usize,
    pub frames: usize,
    pub total_paragraphs: usize,
    pub total_frames: usize,
    pub restart_paragraph: usize,
    pub converged_paragraph: Option<usize>,
    pub old_bytes: Option<Range<usize>>,
    pub new_bytes: Option<Range<usize>>,
    pub reused_prefix_paragraphs: usize,
    pub reused_suffix_paragraphs: usize,
    pub retained_bytes: usize,
    pub elapsed_seconds: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct Paragraph {
    text: String,
    format: ParaFormat,
    chars: Vec<(usize, usize, CharFormat)>,
    caret: CharFormat,
    fields: Vec<(designcraft_doc::CrossRef, Option<Arc<FieldValues>>)>,
    number: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct Inputs {
    common: String,
    pub frames: Vec<FrameSpec>,
    paragraphs: Vec<Paragraph>,
    ranges: Vec<Range<usize>>,
    text_len: usize,
}

/// These cases deliberately retain their complete continuation algorithm. Returning a named
/// fallback is preferable to resuming with an incomplete note/table/running-header state.
pub(crate) fn fallback(doc: &Document, story: &Story, opts: &ComposeOptions) -> Option<&'static str> {
    if story.len() > 8 * 1024 * 1024 {
        return Some("checkpoint source exceeds 8 MiB");
    }
    if !story.notes.is_empty()
        || !story.endnotes.is_empty()
        || doc.publication.as_ref().is_some_and(|v| v.root.note_output(v.component, story.id).is_some())
        || opts.note_releases.is_some()
    {
        return Some("note continuation and numbering");
    }
    if !story.tables.is_empty() {
        return Some("table continuation");
    }
    if !story.objects.is_empty() {
        return Some("anchored object continuation");
    }
    if crate::vars::has_vars(story) {
        return Some("text variable or running-head dependency");
    }
    if story.xrefs.iter().any(|x| x.field.is_none()) {
        return Some("legacy cross-reference dependency");
    }
    None
}

impl Inputs {
    pub(crate) fn new(doc: &Document, story: &Story, opts: &ComposeOptions) -> Self {
        let ranges = story.para_ranges();
        let runs: Vec<_> = story.runs().collect();
        let references: Vec<_> = story.text.match_indices(designcraft_doc::XREF_MARK).zip(&story.xrefs).collect();
        let layout = doc.publication.as_ref().and_then(|v| v.root.editions.get(&v.edition)).map(|e| &e.references);
        let mut counters = HashMap::new();
        let mut field_values: HashMap<u64, Arc<FieldValues>> = HashMap::new();
        let paragraphs = ranges
            .iter()
            .zip(&story.paras)
            .map(|(range, format)| {
                let start = runs.partition_point(|(r, _)| r.end <= range.start);
                let chars = runs[start..]
                    .iter()
                    .take_while(|(r, _)| r.start < range.end)
                    .map(|(r, f)| (r.start.max(range.start) - range.start, r.end.min(range.end) - range.start, (*f).clone()))
                    .collect();
                let first = references.partition_point(|((at, _), _)| *at < range.start);
                let fields = references[first..]
                    .iter()
                    .take_while(|((at, _), _)| *at < range.end)
                    .map(|(_, x)| {
                        let value = x.field.and_then(|id| {
                            layout.and_then(|l| l.fields.get(&id)).map(|v| field_values.entry(id).or_insert_with(|| Arc::new(v.clone())).clone())
                        });
                        ((***x).clone(), value)
                    })
                    .collect();
                let (pp, _) = doc.styles.resolve_para(format);
                let number = if pp.list_type == designcraft_doc::ListType::Numbers && !pp.list_name.is_empty() {
                    let counter = counters.entry(pp.list_name.clone()).or_insert_with(|| doc.list_start(story.id, &pp.list_name));
                    *counter = pp.start_at.map_or(counter.saturating_add(1), |n| n.max(1));
                    Some(*counter)
                } else {
                    None
                };
                Paragraph {
                    text: story.text.get(range.clone()).unwrap_or_default().into(),
                    format: format.clone(),
                    chars,
                    caret: story.char_format_at(range.start).clone(),
                    fields,
                    number,
                }
            })
            .collect();
        // Compare actual values, not only a hash or publication revision: a projection allocates
        // fresh Story Arcs even when this component has not changed.
        let common = format!(
            "{:?}",
            (
                &doc.styles,
                doc.font_scope,
                &doc.conditions,
                &doc.hyphenation_exceptions,
                doc.settings.advanced_type,
                doc.settings.glyph_fallback,
                story.direction,
                story.vertical,
                &opts.page_name,
                opts.page,
                &opts.label,
                opts.suppress_annotations
            )
        );
        Self { common, frames: frame_specs(doc, story.id), paragraphs, ranges, text_len: story.len() }
    }
    pub(crate) fn equal(&self, other: &Self) -> bool {
        self.common == other.common && self.frames == other.frames && self.paragraphs == other.paragraphs
    }
    fn bytes(&self) -> usize {
        let mut fields = BTreeSet::new();
        let field_bytes = self
            .paragraphs
            .iter()
            .flat_map(|p| &p.fields)
            .filter_map(|(xref, v)| v.as_ref().filter(|_| fields.insert(xref.field)))
            .map(|v| {
                std::mem::size_of::<FieldValues>()
                    + std::iter::once(&v.text)
                        .chain(v.pages.values().flat_map(|p| p.values()))
                        .map(|t| {
                            t.runs
                                .iter()
                                .map(|r| r.text.len() + std::mem::size_of::<designcraft_doc::publication::references::RichRun>())
                                .sum::<usize>()
                        })
                        .sum::<usize>()
            })
            .sum::<usize>();
        field_bytes
            + self.common.len()
            + self.paragraphs.iter().map(|p| p.text.len() + p.chars.len() * std::mem::size_of::<(usize, usize, CharFormat)>()).sum::<usize>()
            + self.frames.len() * std::mem::size_of::<FrameSpec>()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Trace {
    pub inputs: Arc<Inputs>,
    raw: Vec<FrameText>,
    snaps: Vec<Snapshot>,
    info: Vec<ParaInfo>,
    force_col: Vec<bool>,
    line_cap: Vec<Option<usize>>,
    styles: Vec<RunStyle>,
    overset: Option<usize>,
}
impl Trace {
    pub(crate) fn bytes(&self) -> usize {
        self.inputs.bytes()
            + frame_bytes(&self.raw)
            + self.snaps.len() * std::mem::size_of::<Snapshot>()
            + self.info.len() * std::mem::size_of::<ParaInfo>()
            + self.styles.len() * std::mem::size_of::<RunStyle>()
    }
}

pub(crate) struct Work {
    pub inputs: Arc<Inputs>,
    old: Option<Arc<Trace>>,
    start: usize,
    old_suffix: usize,
    new_suffix: usize,
    visited_frames: BTreeSet<usize>,
    pub report: WorkReport,
    pub trace: Option<Arc<Trace>>,
}

impl Work {
    pub(crate) fn new(inputs: Arc<Inputs>, previous: Option<Arc<Trace>>, doc: &Document) -> Self {
        let n = inputs.paragraphs.len();
        let mut report = WorkReport {
            mode: "full".into(),
            reason: Some("cold cache".into()),
            total_paragraphs: n,
            total_frames: inputs.frames.len(),
            ..Default::default()
        };
        let mut start = 0;
        let mut old_suffix = 0;
        let mut new_suffix = 0;
        let mut old = None;
        if let Some(prior) = previous {
            if inputs.common != prior.inputs.common {
                report.reason = Some("styles or composition inputs changed".into());
            } else {
                let old_n = prior.inputs.paragraphs.len();
                let prefix = inputs.paragraphs.iter().zip(&prior.inputs.paragraphs).take_while(|(a, b)| a == b).count();
                let suffix = inputs
                    .paragraphs
                    .iter()
                    .rev()
                    .zip(prior.inputs.paragraphs.iter().rev())
                    .take(n.min(old_n) - prefix)
                    .take_while(|(a, b)| a == b)
                    .count();
                old_suffix = old_n - suffix;
                new_suffix = n - suffix;
                report.old_bytes = Some(
                    prior.inputs.ranges.get(prefix).map_or(prior.inputs.text_len, |r| r.start)
                        ..prior.inputs.ranges.get(old_suffix).map_or(prior.inputs.text_len, |r| r.start),
                );
                report.new_bytes = Some(
                    inputs.ranges.get(prefix).map_or(inputs.text_len, |r| r.start)
                        ..inputs.ranges.get(new_suffix).map_or(inputs.text_len, |r| r.start),
                );
                start = prefix.min(prior.snaps.len().saturating_sub(1)).min(n.saturating_sub(1));
                // Appended frames resume from the paragraph which previously overset. Changed
                // geometry restarts before the first paragraph touching that frame.
                if let Some(fi) = inputs
                    .frames
                    .iter()
                    .zip(&prior.inputs.frames)
                    .position(|(a, b)| a != b)
                    .or_else(|| (inputs.frames.len() != prior.inputs.frames.len()).then_some(inputs.frames.len().min(prior.inputs.frames.len())))
                {
                    let affected = prior.info.iter().position(|p| p.end.0 >= fi).unwrap_or(prior.info.len());
                    start = start.min(affected);
                }
                start = start.saturating_sub(1);
                while start > 0 && inputs.paragraphs.get(start - 1).is_some_and(|p| doc.styles.resolve_para(&p.format).0.keep_with_next > 0) {
                    start -= 1;
                }
                if prior.snaps.get(start).is_some_and(|s| s.cur.fi < inputs.frames.len()) {
                    report.mode = "incremental".into();
                    report.reason = None;
                    report.restart_paragraph = start;
                    report.reused_prefix_paragraphs = start;
                    old = Some(prior);
                } else {
                    start = 0;
                    report.reason = Some("no usable continuation checkpoint".into());
                }
            }
        }
        Self { inputs, old, start, old_suffix, new_suffix, visited_frames: BTreeSet::new(), report, trace: None }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resume(
        &self,
        out: &mut ComposedStory,
        cur: &mut Cursor,
        list: &mut u32,
        styles: &mut Vec<RunStyle>,
        snaps: &mut Vec<Snapshot>,
        info: &mut Vec<ParaInfo>,
        force: &mut [bool],
        caps: &mut [Option<usize>],
        note_snaps: &mut Vec<(u32, Option<usize>, usize)>,
    ) -> usize {
        let Some(old) = &self.old else { return 0 };
        let Some(snap) = old.snaps.get(self.start) else { return 0 };
        for (i, f) in out.frames.iter_mut().enumerate().take(snap.cur.fi + 1) {
            if let Some(previous) = old.raw.get(i) {
                let columns = f.columns.clone();
                let vertical = f.vertical;
                *f = previous.clone();
                f.columns = columns;
                f.vertical = vertical;
            }
        }
        snap.restore(out, cur, list);
        *styles = old.styles.clone();
        snaps.extend(old.snaps.iter().take(self.start).cloned());
        info.extend(old.info.iter().take(self.start).cloned());
        for (i, (f, c)) in force.iter_mut().zip(caps).enumerate().take(self.start) {
            *f = old.force_col.get(i).copied().unwrap_or(false);
            *c = old.line_cap.get(i).copied().flatten();
        }
        note_snaps.resize(self.start, (1, None, 0));
        self.start
    }
    pub(crate) fn frame(&mut self, frame: usize) {
        self.visited_frames.insert(frame);
    }
    fn byte(&self, old: usize) -> usize {
        let a = self
            .old
            .as_ref()
            .and_then(|t| t.inputs.ranges.get(self.old_suffix))
            .map_or_else(|| self.old.as_ref().map_or(0, |t| t.inputs.text_len), |r| r.start);
        let b = self.inputs.ranges.get(self.new_suffix).map_or(self.inputs.text_len, |r| r.start);
        b.saturating_add(old.saturating_sub(a))
    }
    fn para(&self, old: usize) -> usize {
        self.new_suffix.saturating_add(old.saturating_sub(self.old_suffix))
    }
    fn shift(&self, frame: &mut FrameText) {
        frame.range = self.byte(frame.range.start)..self.byte(frame.range.end);
        for line in &mut frame.lines {
            line.range = self.byte(line.range.start)..self.byte(line.range.end);
            line.para = self.para(line.para);
            for g in &mut line.glyphs {
                g.byte = self.byte(g.byte);
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reuse_tail(
        &mut self,
        styles: &[RunStyle],
        out: &mut ComposedStory,
        cur: &mut Cursor,
        list: &mut u32,
        pi: usize,
        snaps: &mut Vec<Snapshot>,
        info: &mut Vec<ParaInfo>,
        force: &mut [bool],
        caps: &mut [Option<usize>],
    ) -> bool {
        let Some(old) = &self.old else { return false };
        if pi <= self.start || pi < self.new_suffix || old.overset.is_some() || self.inputs.frames != old.inputs.frames {
            return false;
        }
        let oi = self.old_suffix + pi - self.new_suffix;
        let Some(boundary) = old.snaps.get(oi) else { return false };
        if cur != &boundary.cur || *list != boundary.list_counter || info.last().is_some_and(|p| p.keep_with_next > 0) {
            return false;
        }
        let Some(current) = out.frames.get(cur.fi) else { return false };
        let Some(previous) = old.raw.get(cur.fi) else { return false };
        if current.lines.iter().any(|l| l.para < self.new_suffix) || previous.lines.iter().take(boundary.lines).any(|l| l.para < self.old_suffix) {
            return false;
        }
        let mut prefix = previous.clone();
        prefix.lines.truncate(boundary.lines);
        prefix.decos.truncate(boundary.decos);
        self.shift(&mut prefix);
        if current.lines.len() != prefix.lines.len()
            || current.decos != prefix.decos
            || !current.lines.iter().zip(&prefix.lines).all(|(a, b)| same_line(a, b, styles, &old.styles))
        {
            return false;
        }
        // Both the complete output of this frame prefix and the continuation cursor match.
        // No keep dependency reaches back across the boundary; later output can be reused.
        for (i, f) in out.frames.iter_mut().enumerate().skip(cur.fi) {
            if let Some(old_frame) = old.raw.get(i) {
                let mut copied = old_frame.clone();
                self.shift(&mut copied);
                if i == cur.fi {
                    f.lines.extend(copied.lines.into_iter().skip(boundary.lines));
                    f.decos.extend(copied.decos.into_iter().skip(boundary.decos));
                } else {
                    *f = copied;
                }
            }
        }
        snaps.truncate(pi);
        snaps.extend(old.snaps.iter().skip(oi).cloned());
        info.truncate(pi);
        info.extend(old.info.iter().skip(oi).cloned());
        for i in pi..force.len() {
            let previous = oi + i - pi;
            force[i] = old.force_col.get(previous).copied().unwrap_or(false);
            caps[i] = old.line_cap.get(previous).copied().flatten();
        }
        if let Some(last) = old.snaps.last() {
            *cur = last.cur.clone();
            *list = last.list_counter;
        }
        self.report.converged_paragraph = Some(pi);
        self.report.reused_suffix_paragraphs = self.inputs.paragraphs.len() - pi;
        true
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish(
        &mut self,
        out: &ComposedStory,
        cur: &Cursor,
        list: u32,
        pi: usize,
        snaps: &[Snapshot],
        info: &[ParaInfo],
        force: &[bool],
        caps: &[Option<usize>],
        styles: &[RunStyle],
    ) {
        let mut checkpoints = snaps.to_vec();
        if pi == self.inputs.paragraphs.len() && checkpoints.len() == pi {
            checkpoints.push(Snapshot::take(out, cur, list));
        }
        let trace = Arc::new(Trace {
            inputs: self.inputs.clone(),
            raw: out.frames.clone(),
            snaps: checkpoints,
            info: info.to_vec(),
            force_col: force.to_vec(),
            line_cap: caps.to_vec(),
            styles: styles.to_vec(),
            overset: out.overset_at,
        });
        self.report.frames = self.visited_frames.len();
        self.report.retained_bytes = trace.bytes();
        self.trace = Some(trace);
    }
}

pub(crate) fn frame_bytes(frames: &[FrameText]) -> usize {
    let mut pending: Vec<_> = frames.iter().map(|f| (f, 0usize)).collect();
    let mut bytes = 0usize;
    while let Some((f, depth)) = pending.pop() {
        bytes = bytes.saturating_add(
            std::mem::size_of::<FrameText>()
                + f.columns.len() * std::mem::size_of::<Rect>()
                + f.lines
                    .iter()
                    .map(|l| {
                        std::mem::size_of::<Line>()
                            + l.glyphs.len() * std::mem::size_of::<PlacedGlyph>()
                            + l.glyphs.iter().filter_map(|g| g.generated.as_ref().and_then(|g| g.text.as_ref())).map(|t| t.len()).sum::<usize>()
                    })
                    .sum::<usize>()
                + f.decos.len() * std::mem::size_of::<Deco>()
                + f.objects.len() * std::mem::size_of::<PlacedObject>(),
        );
        if depth >= 64 {
            continue;
        }
        for note in &f.notes {
            bytes = bytes.saturating_add(std::mem::size_of::<PlacedNote>() + note.source.len());
            pending.extend(note.text.frames.iter().map(|f| (f, depth + 1)));
        }
        for table in &f.tables {
            bytes = bytes.saturating_add(std::mem::size_of::<TableFrag>() + table.strokes.len() * std::mem::size_of::<StrokeSeg>());
            for cell in &table.cells {
                bytes = bytes.saturating_add(std::mem::size_of::<PlacedCell>() + cell.source.len());
                pending.extend(cell.text.frames.iter().map(|f| (f, depth + 1)));
            }
        }
    }
    bytes
}

/// Semantic equality compares glyphs with their resolved styles, so an unused style-table
/// entry retained by a checkpoint is not confused with a different printed result.
pub(crate) fn same_line(a: &Line, b: &Line, sa: &[RunStyle], sb: &[RunStyle]) -> bool {
    a.column == b.column
        && a.baseline == b.baseline
        && a.x0 == b.x0
        && a.x1 == b.x1
        && a.ascent == b.ascent
        && a.descent == b.descent
        && a.leading == b.leading
        && a.range == b.range
        && a.para == b.para
        && a.hyphenated == b.hyphenated
        && a.first_in_para == b.first_in_para
        && a.last_in_para == b.last_in_para
        && a.end_x == b.end_x
        && a.spacing == b.spacing
        && a.hj == b.hj
        && a.keep_violation == b.keep_violation
        && a.glyphs.len() == b.glyphs.len()
        && a.glyphs.iter().zip(&b.glyphs).all(|(a, b)| {
            a.face.id() == b.face.id()
                && a.gid == b.gid
                && a.x == b.x
                && a.y == b.y
                && a.adv == b.adv
                && a.sx == b.sx
                && a.sy == b.sy
                && sa.get(a.style as usize) == sb.get(b.style as usize)
                && a.byte == b.byte
                && a.len == b.len
                && a.visible == b.visible
                && a.upright == b.upright
                && a.tcy == b.tcy
                && a.rtl == b.rtl
                && a.generated.as_ref().map(|g| (&g.text, g.target)) == b.generated.as_ref().map(|g| (&g.text, g.target))
        })
}

#[cfg(test)]
mod tests;
