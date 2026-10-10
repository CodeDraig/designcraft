//! Bounded runtime instrumentation; never persisted into authored publication data.
use super::*;
use designcraft_doc::{
    EditionId,
    publication::references::{Dependency, FieldValues},
};

#[derive(Clone, Debug, Default)]
pub struct Metrics {
    pub calls: u64,
    pub exact_hits: u64,
    pub semantic_hits: u64,
    pub incremental: u64,
    pub full: u64,
    pub paragraphs: u64,
    pub frames: u64,
    pub reference_evaluations: u64,
    pub reference_hits: u64,
    pub reference_changes: u64,
    pub elapsed_seconds: f64,
    pub estimated_retained_bytes: usize,
    pub estimated_peak_bytes: usize,
    pub events: Vec<WorkEvent>,
}
#[derive(Clone, Debug)]
pub struct WorkEvent {
    pub edition: Option<u64>,
    pub component: Option<u64>,
    pub story: u64,
    pub work: crate::WorkReport,
}
#[derive(Default)]
pub(super) struct FieldCache {
    entries: HashMap<(EditionId, u64), (String, FieldValues, Vec<Dependency>)>,
    bytes: usize,
}
impl FieldCache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}
impl Cache {
    pub fn metrics(&self, reset: bool) -> Metrics {
        let payload = self.map.lock().unwrap_or_else(|e| e.into_inner()).0.values().map(|e| e.bytes).sum::<usize>();
        let previous =
            self.previous.lock().unwrap_or_else(|e| e.into_inner()).values().map(|(_, c)| incremental::frame_bytes(&c.frames)).sum::<usize>();
        let fields = self.fields.lock().unwrap_or_else(|e| e.into_inner()).bytes;
        let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
        m.estimated_retained_bytes = payload + previous + fields;
        m.estimated_peak_bytes = m.estimated_peak_bytes.max(m.estimated_retained_bytes);
        let result = m.clone();
        if reset {
            *m = Metrics {
                estimated_retained_bytes: m.estimated_retained_bytes,
                estimated_peak_bytes: m.estimated_retained_bytes,
                ..Default::default()
            };
        }
        result
    }
    pub(super) fn record_hit(&self) {
        let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
        m.calls += 1;
        m.exact_hits += 1;
    }
    pub(super) fn record(&self, doc: &Document, sid: StoryId, report: crate::WorkReport, bytes: usize) {
        let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
        m.calls += 1;
        match report.mode.as_str() {
            "semanticHit" => m.semantic_hits += 1,
            "incremental" => m.incremental += 1,
            _ => m.full += 1,
        }
        m.paragraphs += report.paragraphs as u64;
        m.frames += report.frames as u64;
        m.elapsed_seconds += report.elapsed_seconds.unwrap_or_default();
        m.estimated_retained_bytes = bytes;
        m.estimated_peak_bytes = m.estimated_peak_bytes.max(bytes);
        if m.events.len() >= 128 {
            m.events.remove(0);
        }
        m.events.push(WorkEvent {
            edition: doc.publication.as_ref().map(|v| v.edition.0),
            component: doc.publication.as_ref().map(|v| v.component.0),
            story: sid.0,
            work: report,
        });
    }
    /// `signature` contains exact dependency values, including query membership. It is not a
    /// digest: even a hash collision cannot return the wrong generated reading.
    pub fn field_cached(&self, edition: EditionId, id: u64, signature: &str) -> Option<(FieldValues, Vec<Dependency>)> {
        self.fields
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .get(&(edition, id))
            .filter(|(key, _, _)| key == signature)
            .map(|(_, v, d)| (v.clone(), d.clone()))
    }
    pub fn previous_field(&self, edition: EditionId, id: u64) -> Option<FieldValues> {
        self.fields.lock().unwrap_or_else(|e| e.into_inner()).entries.get(&(edition, id)).map(|(_, v, _)| v.clone())
    }
    pub fn remember_field(&self, edition: EditionId, id: u64, signature: String, values: FieldValues, deps: Vec<Dependency>) {
        let mut f = self.fields.lock().unwrap_or_else(|e| e.into_inner());
        let size = signature.len() + format!("{values:?}{deps:?}").len();
        if f.entries.len() >= 100_000 || f.bytes.saturating_add(size) > 64 * 1024 * 1024 {
            f.clear();
        }
        if let Some((k, v, d)) = f.entries.remove(&(edition, id)) {
            f.bytes = f.bytes.saturating_sub(k.len() + format!("{v:?}{d:?}").len());
        }
        if size <= 64 * 1024 * 1024 {
            f.entries.insert((edition, id), (signature, values, deps));
            f.bytes += size;
        }
    }
    pub fn record_reference(&self, reused: bool, changed: bool) {
        let mut m = self.metrics.lock().unwrap_or_else(|e| e.into_inner());
        if reused {
            m.reference_hits += 1;
        } else {
            m.reference_evaluations += 1;
        }
        if changed {
            m.reference_changes += 1;
        }
    }
}
