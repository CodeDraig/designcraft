//! Composition cache shared by the engine, the renderer and exporters.
//!
//! Allocation signatures provide the fast path. Exact semantic inputs permit reuse across
//! publication projections, with paragraph checkpoints for local edits. Complex continuation
//! state uses the full composer and records its fallback reason.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use designcraft_doc::{Document, Item, Story, StoryId, Styles, WrapMode};

use crate::vars::RunningIndex;
use crate::{ComposeOptions, ComposedStory};

type Namespace = Option<(designcraft_doc::EditionId, designcraft_doc::ComponentId)>;
type Key = (Namespace, StoryId, Option<String>);
fn namespace(doc: &Document) -> Namespace {
    doc.publication.as_ref().map(|v| (v.edition, v.component))
}
mod metrics;
use crate::incremental::{self, Inputs, Trace, Work};
pub use metrics::{Metrics, WorkEvent};

struct Entry {
    sig: Vec<usize>,
    _keep: (Arc<Story>, Arc<Styles>, Vec<Arc<Item>>),
    out: Arc<ComposedStory>,
    stamp: u64,
    trace: Option<Arc<Trace>>,
    bytes: usize,
}

#[derive(Default)]
pub struct Cache {
    map: Mutex<(HashMap<Key, Entry>, u64)>,
    /// Running-header index with the document signature it was built for.
    running: Mutex<Option<(u64, Arc<RunningIndex>)>>,
    /// Cross-reference anchor pages with the document signature they were built for.
    xrefs: Mutex<Option<(u64, Arc<crate::xref::XrefIndex>)>>,
    /// The composition each story (page-independent key) had before its latest recomposition,
    /// with the story revision it was composed from (repaint only the frames an edit changed).
    previous: Mutex<HashMap<(Namespace, StoryId), (Vec<usize>, Arc<ComposedStory>)>>,
    metrics: Mutex<Metrics>,
    fields: Mutex<metrics::FieldCache>,
}

/// Identity of everything a running header can depend on (stories, spreads, styles, variables).
fn doc_signature(doc: &Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (id, st) in &doc.stories {
        (id.0, Arc::as_ptr(st) as usize, st.rev).hash(&mut h);
    }
    for sp in &doc.spreads {
        (Arc::as_ptr(sp) as usize).hash(&mut h);
    }
    (Arc::as_ptr(&doc.styles) as usize).hash(&mut h);
    format!("{:?}", doc.text_variables).hash(&mut h);
    doc.sections.len().hash(&mut h);
    if let Some(v) = &doc.publication {
        (v.root.revision, v.edition, v.component).hash(&mut h);
        v.root.editions.get(&v.edition).map(|e| e.references.generation).hash(&mut h);
    }
    let grid = &doc.settings.baseline_grid;
    (grid.start.to_bits(), grid.increment.to_bits()).hash(&mut h);
    h.finish()
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The composed story, from cache or freshly composed.
    pub fn get(&self, doc: &Document, sid: StoryId, page_name: Option<&str>) -> Arc<ComposedStory> {
        let Some(story) = doc.stories.get(&sid) else { return Arc::new(ComposedStory { story: sid, ..Default::default() }) };
        let (mut sig, keep_items) = signature(doc, story);
        let key = (namespace(doc), sid, page_name.map(str::to_string));
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();
        let with_vars = crate::vars::has_vars(story);
        let running = with_vars && crate::vars::has_running(doc, story);
        if with_vars {
            // Variables depend on the page count, sections and definitions (and running headers on
            // the whole document).
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", doc.text_variables).hash(&mut h);
            // Shared publication metadata and numbering can change without editing this story.
            for i in 0..doc.text_variables.len() {
                doc.variable_value(i, None).hash(&mut h);
            }
            sig.extend([doc.page_count(), h.finish() as usize]);
            if running {
                sig.push(doc_signature(doc) as usize);
            }
        }
        let with_xrefs = crate::xref::has_xrefs(story);
        if with_xrefs {
            // Cross-references show other stories' text and pages.
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", doc.xref_formats).hash(&mut h);
            sig.extend([doc_signature(doc) as usize, h.finish() as usize]);
        }
        if !story.notes.is_empty() {
            // Footnote numbers continue from earlier stories; layout follows the options.
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", doc.footnote_options).hash(&mut h);
            sig.extend([doc.footnote_start(sid) as usize, h.finish() as usize]);
        }
        // Endnote numbers continue from stories on earlier pages.
        if !story.endnotes.is_empty() {
            sig.extend(doc.endnote_numbers(sid).into_iter().map(|n| n as usize));
        }
        // User hyphenation exceptions.
        if !doc.hyphenation_exceptions.is_empty() {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            doc.hyphenation_exceptions.hash(&mut h);
            sig.push(h.finish() as usize);
        }
        // Hidden conditions take their text out of the layout.
        for (i, c) in doc.conditions.iter().enumerate() {
            if !c.visible {
                sig.push(usize::MAX - i);
            }
        }
        // Advanced Type sizes super/subscripts.
        let a = doc.settings.advanced_type;
        sig.extend([a.superscript_size, a.superscript_position, a.subscript_size, a.subscript_position].map(|v| v.to_bits() as usize));
        // Missing glyphs: fallback fonts or the font's box.
        sig.push(usize::from(doc.settings.glyph_fallback));
        // Named lists continue from earlier stories.
        for l in &doc.settings.lists {
            sig.push(doc.list_start(sid, &l.name) as usize);
        }
        let previous = {
            let mut g = self.map.lock().unwrap_or_else(|e| e.into_inner());
            g.1 += 1;
            let stamp = g.1;
            if let Some(e) = g.0.get_mut(&key)
                && e.sig == sig
            {
                e.stamp = stamp;
                self.record_hit();
                return e.out.clone();
            }
            g.0.get(&key).and_then(|e| e.trace.clone().map(|t| (t, e.out.clone())))
        };
        // Parent-page items are composed per page; variables need that page's index.
        let page = match page_name {
            Some(n) if with_vars => (0..doc.page_count()).find(|&i| doc.page_name(i) == n),
            _ => None,
        };
        let running = running.then(|| self.running_index(doc));
        let opts = ComposeOptions {
            page_name: page_name.map(str::to_string),
            page,
            running,
            label: None,
            xrefs: crate::xref::needs_index(story).then(|| self.xref_index(doc)),
            ..Default::default()
        };
        let fallback =
            if !doc.publication_story_included(sid) { Some("story excluded from edition") } else { incremental::fallback(doc, story, &opts) };
        let (out, trace, mut report) = if let Some(reason) = fallback {
            let out = Arc::new(crate::compose_story(doc, sid, &opts));
            let report = crate::WorkReport {
                mode: "full".into(),
                reason: Some(reason.into()),
                paragraphs: if doc.publication_story_included(sid) { story.paras.len() } else { 0 },
                total_paragraphs: story.paras.len(),
                frames: out.frames.iter().filter(|f| !f.lines.is_empty() || !f.notes.is_empty()).count(),
                total_frames: out.frames.len(),
                ..Default::default()
            };
            (out, None, report)
        } else {
            let inputs = Arc::new(Inputs::new(doc, story, &opts));
            if let Some((prior, cached)) = previous.as_ref().filter(|(p, _)| p.inputs.equal(&inputs)) {
                let current = if cached.rev == story.rev {
                    cached.clone()
                } else {
                    let mut current = (**cached).clone();
                    current.rev = story.rev;
                    Arc::new(current)
                };
                let report = crate::WorkReport {
                    mode: "semanticHit".into(),
                    total_paragraphs: story.paras.len(),
                    total_frames: inputs.frames.len(),
                    retained_bytes: prior.bytes(),
                    ..Default::default()
                };
                (current, Some(prior.clone()), report)
            } else {
                self.compose_incremental(doc, story, &opts, inputs, previous.map(|(trace, _)| trace))
            }
        };
        report.elapsed_seconds = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                Some(started.elapsed().as_secs_f64())
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        };
        let bytes = incremental::frame_bytes(&out.frames) + trace.as_ref().map_or(0, |t| t.bytes());
        let mut g = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let stamp = g.1;
        let replaced =
            g.0.insert(key.clone(), Entry { sig, _keep: (story.clone(), doc.styles.clone(), keep_items), out: out.clone(), stamp, trace, bytes });
        // Retain at most 512 MiB of estimated composition payload. Estimates omit allocator
        // overhead and font storage; the metric is deliberately not labelled process RSS.
        let mut retained = g.0.values().map(|e| e.bytes).sum::<usize>();
        while g.0.len() > 4096 || retained > 512 * 1024 * 1024 {
            let oldest = g.0.iter().min_by_key(|(_, e)| e.stamp).map(|(k, _)| k.clone());
            let Some(oldest) = oldest else { break };
            if let Some(e) = g.0.remove(&oldest) {
                retained = retained.saturating_sub(e.bytes);
            }
        }
        drop(g);
        if key.2.is_none()
            && let Some(old) = replaced
        {
            let mut previous = self.previous.lock().unwrap_or_else(|e| e.into_inner());
            if previous.len() >= 64 {
                previous.clear();
            }
            previous.insert((key.0, sid), (old.sig, old.out));
            if previous.values().map(|(_, c)| incremental::frame_bytes(&c.frames)).sum::<usize>() > 64 * 1024 * 1024 {
                previous.clear();
            }
        }
        self.record(doc, sid, report, retained);
        out
    }

    fn compose_incremental(
        &self,
        doc: &Document,
        story: &Story,
        opts: &ComposeOptions,
        inputs: Arc<Inputs>,
        previous: Option<Arc<Trace>>,
    ) -> (Arc<ComposedStory>, Option<Arc<Trace>>, crate::WorkReport) {
        let mut work = Work::new(inputs.clone(), previous, doc);
        let out = crate::compose_traced(doc, story, &inputs.frames, opts, designcraft_fonts::FontDb::global(), Some(&mut work));
        (Arc::new(out), work.trace, work.report)
    }

    /// The cached composition of `sid` made from exactly this version of `doc` (the current
    /// entry or the one it replaced), without composing anything.
    pub fn composed_for(&self, doc: &Document, sid: StoryId) -> Option<Arc<ComposedStory>> {
        let story = doc.stories.get(&sid)?;
        let (sig, _) = signature(doc, story);
        // Only the geometry/story part of the signature (variables etc. add to it).
        let matches = |s: &[usize]| s.len() >= sig.len() && s[..sig.len()] == sig[..];
        {
            let g = self.map.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(e) = g.0.get(&(namespace(doc), sid, None))
                && matches(&e.sig)
            {
                return Some(e.out.clone());
            }
        }
        let g = self.previous.lock().unwrap_or_else(|e| e.into_inner());
        g.get(&(namespace(doc), sid)).filter(|(s, _)| matches(s)).map(|(_, c)| c.clone())
    }

    /// Frames whose composed content differs between two compositions of a story.
    pub fn changed_frames(a: &ComposedStory, b: &ComposedStory) -> Vec<designcraft_doc::ItemId> {
        let same_line = |x: &crate::Line, y: &crate::Line| {
            x.range == y.range
                && (x.baseline - y.baseline).abs() < 1e-9
                && (x.x0 - y.x0).abs() < 1e-9
                && (x.end_x - y.end_x).abs() < 1e-9
                && x.glyphs.len() == y.glyphs.len()
                && x.glyphs
                    .iter()
                    .zip(&y.glyphs)
                    .all(|(g, h)| g.gid == h.gid && g.style == h.style && (g.x - h.x).abs() < 1e-9 && (g.y - h.y).abs() < 1e-9)
        };
        let mut out = Vec::new();
        for fb in &b.frames {
            let same = a.frame(fb.frame).is_some_and(|fa| {
                fa.lines.len() == fb.lines.len()
                    && fa.lines.iter().zip(&fb.lines).all(|(x, y)| same_line(x, y))
                    && fa.decos == fb.decos
                    && fa.tables.is_empty()
                    && fb.tables.is_empty()
                    && fa.notes.is_empty()
                    && fb.notes.is_empty()
                    && fa.objects.is_empty()
                    && fb.objects.is_empty()
            }) && a.styles == b.styles;
            if !same {
                out.push(fb.frame);
            }
        }
        for fa in &a.frames {
            if b.frame(fa.frame).is_none() {
                out.push(fa.frame);
            }
        }
        out
    }

    pub fn clear(&self) {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).0.clear();
        self.previous.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.fields.lock().unwrap_or_else(|e| e.into_inner()).clear();
        *self.running.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.xrefs.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Anchor pages for cross-references (rebuilt when the document changes).
    pub fn xref_index(&self, doc: &Document) -> Arc<crate::xref::XrefIndex> {
        let sig = doc_signature(doc);
        if let Some((s, r)) = &*self.xrefs.lock().unwrap_or_else(|e| e.into_inner())
            && *s == sig
        {
            return r.clone();
        }
        let r = Arc::new(crate::xref::XrefIndex::build(doc, &|sid| self.get(doc, sid, None)));
        *self.xrefs.lock().unwrap_or_else(|e| e.into_inner()) = Some((sig, r.clone()));
        r
    }

    /// The running-header index for `doc` (rebuilt when the document changes).
    pub fn running_index(&self, doc: &Document) -> Arc<RunningIndex> {
        let sig = doc_signature(doc);
        if let Some((s, r)) = &*self.running.lock().unwrap_or_else(|e| e.into_inner())
            && *s == sig
        {
            return r.clone();
        }
        let r = Arc::new(RunningIndex::build(doc, &|sid| self.get(doc, sid, None)));
        *self.running.lock().unwrap_or_else(|e| e.into_inner()) = Some((sig, r.clone()));
        r
    }
}

fn signature(doc: &Document, story: &Arc<Story>) -> (Vec<usize>, Vec<Arc<Item>>) {
    // The document's font scope: another document's fonts of the same name set it differently.
    let mut sig =
        vec![Arc::as_ptr(story) as usize, story.rev as usize, Arc::as_ptr(&doc.styles) as usize, doc.sections.len(), doc.font_scope as usize];
    if let Some(v) = &doc.publication {
        sig.extend([v.root.revision as usize, v.edition.0 as usize, v.component.0 as usize]);
        sig.push(v.root.editions.get(&v.edition).map_or(0, |e| e.references.generation) as usize);
        sig.push(usize::from(doc.publication_story_included(story.id)));
    }
    use std::hash::{Hash, Hasher};
    let mut geometry = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", (&doc.sections, &doc.layers)).hash(&mut geometry);
    for sp in doc.spreads.iter().chain(&doc.parents) {
        format!("{:?}", &sp.pages).hash(&mut geometry);
    }
    let bits = geometry.finish();
    sig.extend([bits as u32 as usize, (bits >> 32) as usize]);
    // Preferences can change the grid without replacing any story, style or frame Arc.
    // Keep both halves of each f64 on 32-bit WASM as well as native targets.
    for v in [doc.settings.baseline_grid.start, doc.settings.baseline_grid.increment] {
        let bits = v.to_bits();
        sig.extend([bits as u32 as usize, (bits >> 32) as usize]);
    }
    let mut keep = Vec::new();
    let mut spreads = Vec::new();
    for f in &story.frames {
        let Some(loc) = doc.find(*f) else {
            sig.push(0);
            continue;
        };
        if let Some(sp) = doc.spread(loc.spread) {
            let top = &sp.items[loc.top()];
            sig.push(Arc::as_ptr(top) as usize);
            keep.push(top.clone());
            // Page position matters for page-number markers.
            sig.push(loc.top());
            if !spreads.contains(&loc.spread) {
                spreads.push(loc.spread);
                for it in &sp.items {
                    if it.wrap.mode != WrapMode::None {
                        sig.push(Arc::as_ptr(it) as usize);
                        keep.push(it.clone());
                    }
                }
                sig.push(usize::MAX);
            }
        }
    }
    (sig, keep)
}
