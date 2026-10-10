use super::*;
use designcraft_doc::{ParaFormat, StoryId};

pub(super) struct Block {
    pub entry: model::Entry,
    pub start: usize,
    pub body: usize,
    pub end: usize,
    objects: usize,
    tables: BTreeMap<u64, u64>,
    source: Arc<Story>,
    document: Option<Arc<Document>>,
    label: String,
}
pub(super) struct Sequence {
    pub story: Story,
    pub blocks: Vec<Block>,
    pub releases: BTreeMap<usize, usize>,
    pub heading_end: usize,
}
impl Sequence {
    pub fn build(doc: &Document, name: &str, entries: &[model::Entry], policy: &Policy, heading: bool, calls: &BTreeMap<u64, (usize, f64)>) -> Self {
        let mut story = Story::new(StoryId(0));
        let mut blocks = Vec::new();
        let mut releases = BTreeMap::new();
        let view = doc.publication.as_ref();
        let stream = view.and_then(|v| v.root.note_streams.get(name)).cloned().unwrap_or_default();
        if heading && !stream.heading.is_empty() {
            story.insert(0, &stream.heading);
            story.paras[0].style = stream.heading_style.clone();
            story.insert(story.len(), "\n");
        }
        let heading_end = story.len();
        let mut previous_run_in = false;
        let mut owners = BTreeMap::new();
        for entry in entries {
            let single = !entry.note.text.text.contains('\n') && entry.note.text.tables.is_empty();
            if !blocks.is_empty() {
                story.insert(story.len(), if policy.run_in && single && previous_run_in { &policy.between_notes } else { "\n" });
            }
            previous_run_in = single;
            let start = story.len();
            let label = view.and_then(|v| v.root.note_text_label(v.edition, entry.identity.id)).unwrap_or_else(|| "?".into());
            story.insert(start, &format!("{label}{}", stream.separator));
            let body = story.len();
            let p0 = story.para_at(body);
            let objects = story.objects.len();
            let mut source = entry.note.text.clone();
            let owner = owners
                .entry(entry.component)
                .or_insert_with(|| {
                    view.and_then(|v| designcraft_doc::publication::PublicationView { component: entry.component, ..v.clone() }.project().ok())
                        .map(Arc::new)
                })
                .clone();
            let styles = owner.as_ref().map_or(doc.styles.as_ref(), |d| d.styles.as_ref());
            if !stream.preserve_paragraph_styles
                && let Some(p) = source.paras.first_mut()
            {
                p.style = stream.para_style.clone();
            }
            if let Some(p) = source.paras.last_mut() {
                p.para.space_after = Some(policy.space_between);
            }
            // Resolve named styles against the owning component, so a publication-end note
            // keeps its edition typography when another component uses the same style name.
            materialize(&mut source, styles);
            let end = story.insert_story_preserving_anchors(body, &source);
            if let (Some(target), Some(para)) = (story.paras.get_mut(p0), source.paras.first()) {
                *target = ParaFormat { table: target.table, ..para.clone() };
            }
            let tables = source.paras.iter().enumerate().filter_map(|(i, p)| Some((story.paras.get(p0 + i)?.table?, p.table?))).collect();
            if let Some((fi, _)) = calls.get(&entry.identity.id) {
                releases.insert(start, *fi);
            }
            blocks.push(Block { entry: entry.clone(), start, body, end, objects, tables, source: Arc::new(source), document: owner, label });
        }
        Self { story, blocks, releases, heading_end }
    }
}

pub(super) fn height(ft: &FrameText) -> f64 {
    ft.lines
        .iter()
        .map(|l| l.baseline + l.descent)
        .chain(ft.tables.iter().map(|t| t.rect.y1))
        .chain(ft.objects.iter().map(|o| o.origin.y + o.size.1))
        .fold(0.0, f64::max)
}

/// Split generated area geometry back into canonical notes, retaining one-to-one byte
/// addresses for rich text. Several run-in notes may share a line without sharing identity.
pub(super) fn append_fragments(seq: &Sequence, cs: &ComposedStory, offsets: &[Point], out: &mut ComposedStory) -> BTreeMap<u64, usize> {
    let mut first = BTreeMap::new();
    for block in &seq.blocks {
        for (fi, ft) in cs.frames.iter().enumerate() {
            let Some(target) = out.frames.get_mut(fi) else { continue };
            let shift = |at: usize| at.saturating_sub(block.body).min(block.source.len());
            let mut frag = FrameText { frame: ItemId(0), vertical: ft.vertical, columns: ft.columns.clone(), ..Default::default() };
            let mut line_map = BTreeMap::new();
            for (li, l) in ft.lines.iter().enumerate() {
                if l.range.end <= block.start || l.range.start >= block.end {
                    continue;
                }
                let mut line = l.clone();
                line.glyphs.retain(|g| g.byte >= block.start && g.byte < block.end);
                for g in &mut line.glyphs {
                    if g.byte < block.body {
                        let text = seq.story.text.get(g.byte..g.byte.saturating_add(g.len).min(block.body)).unwrap_or("");
                        g.generated = Some(crate::GeneratedGlyph { text: Some(Arc::from(text)), target: Some(block.entry.identity.callout) });
                        g.byte = 0;
                        g.len = 0;
                    } else {
                        g.byte = shift(g.byte);
                    }
                }
                line.range = shift(l.range.start)..shift(l.range.end);
                line.para = block.source.para_at(line.range.start);
                line_map.insert(li, frag.lines.len());
                frag.lines.push(line);
            }
            for table in &ft.tables {
                if let Some(original) = block.tables.get(&table.table) {
                    let mut t = table.clone();
                    t.table = *original;
                    t.anchor = shift(t.anchor);
                    t.line = line_map.get(&t.line).copied().unwrap_or(0);
                    frag.tables.push(t);
                }
            }
            for object in &ft.objects {
                if object.index >= block.objects && object.index < block.objects + block.source.objects.len() {
                    let mut o = object.clone();
                    o.index -= block.objects;
                    o.line = line_map.get(&o.line).copied().unwrap_or(0);
                    frag.objects.push(o);
                }
            }
            if frag.lines.is_empty() && frag.tables.is_empty() && frag.objects.is_empty() {
                continue;
            }
            first.entry(block.entry.identity.id).or_insert(fi);
            let x0 = frag
                .lines
                .iter()
                .flat_map(|l| l.glyphs.iter().map(|g| g.x))
                .chain(frag.tables.iter().map(|t| t.rect.x0))
                .fold(f64::INFINITY, f64::min);
            let x1 =
                frag.lines.iter().flat_map(|l| l.glyphs.iter().map(|g| g.x + g.adv)).chain(frag.tables.iter().map(|t| t.rect.x1)).fold(0.0, f64::max);
            let y0 = frag.lines.iter().map(|l| l.baseline - l.ascent).chain(frag.tables.iter().map(|t| t.rect.y0)).fold(f64::INFINITY, f64::min);
            let y1 = height(&frag);
            let x0 = if x0.is_finite() { x0 } else { 0.0 };
            let y0 = if y0.is_finite() { y0 } else { 0.0 };
            frag.range = frag.lines.iter().map(|l| l.range.start).min().unwrap_or(0)..frag.lines.iter().map(|l| l.range.end).max().unwrap_or(0);
            frag.content_height = y1;
            for d in &ft.decos {
                let mut d = d.clone();
                d.rect = d.rect.intersect(Rect::new(x0, y0, x1, y1));
                if d.rect.area() > 0.0 {
                    frag.decos.push(d);
                }
            }
            let source_range = frag.range.clone();
            out.annotation_progress = out.annotation_progress.saturating_add(source_range.len());
            let origin = offsets.get(fi).copied().unwrap_or(Point::ZERO);
            let composed = ComposedStory {
                story: block.source.id,
                rev: block.source.rev,
                styles: cs.styles.clone(),
                text_len: block.source.len(),
                frames: vec![frag],
                source: Some(block.source.clone()),
                render_document: block.document.clone(),
                ..Default::default()
            };
            target.notes.push(PlacedNote {
                id: block.entry.note.id,
                index: 0,
                annotation: Some(block.entry.identity.id),
                source_story: Some(block.source.clone()),
                source_range,
                column: 0,
                label: block.label.clone(),
                origin,
                rect: Rect::new(x0 + origin.x, y0 + origin.y, x1 + origin.x, y1 + origin.y),
                text: Arc::new(composed),
                source: Arc::from(block.source.text.as_str()),
            });
        }
    }
    first
}

fn materialize(story: &mut Story, styles: &designcraft_doc::Styles) {
    story.for_each_text_mut(&mut |s| {
        let ranges = s.para_ranges();
        let runs: Vec<_> = s.runs().map(|(r, f)| (r, f.clone())).collect();
        let mut chars = Vec::new();
        for (mut range, p) in ranges.into_iter().zip(s.paras.clone()) {
            // Paragraph ranges omit their newline, but character runs cover every byte.
            // Keeping that byte also prevents the next run boundary splitting a UTF-8 marker.
            if range.end < s.len() {
                range.end += 1;
            }
            let (_, base) = styles.resolve_para(&p);
            for (r, f) in &runs {
                let a = r.start.max(range.start);
                let b = r.end.min(range.end);
                if a >= b {
                    continue;
                }
                let props = styles.resolve_char(&base, f);
                chars.push(designcraft_doc::CharRun {
                    len: b - a,
                    format: designcraft_doc::CharFormat {
                        style: designcraft_doc::NO_CHAR_STYLE.into(),
                        over: designcraft_doc::CharProps::common([&props]),
                    },
                });
            }
        }
        for p in &mut s.paras {
            let (props, base) = styles.resolve_para(p);
            p.style = designcraft_doc::BASIC_PARAGRAPH.into();
            p.para = designcraft_doc::ParaProps::common([&props]);
            p.chars = designcraft_doc::CharProps::common([&base]);
        }
        if !chars.is_empty() {
            s.chars = chars;
        }
    });
}

#[cfg(test)]
#[test]
fn materializing_note_styles_keeps_newlines_and_utf8_boundaries() {
    let d = Document::new(&Default::default());
    let mut s = Story::with_text(StoryId(0), "First paragraph.\n\nCymraeg: ŵŷ\nFinal paragraph.", Default::default());
    s.format_chars(0..5, |f| f.over.font_style = Some("Italic".into()));
    let original = s.text.clone();
    materialize(&mut s, &d.styles);
    assert_eq!(s.chars.iter().map(|r| r.len).sum::<usize>(), s.len());
    assert!(s.runs().all(|(r, _)| s.text.get(r).is_some()));
    let mut target = Story::new(StoryId(1));
    target.insert_story(0, &s);
    assert_eq!(target.text, original);
}
