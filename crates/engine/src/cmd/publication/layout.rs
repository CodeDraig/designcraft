//! Runtime layout evidence, shared by GUI, CLI, control and MCP.
use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "publication.layout.metrics","Publication Layout Work",[],None,
            "{reset?:bool} → session cache counters and last 128 composition events; memory is an estimated retained payload, native timing is seconds; reset changes runtime counters only",has_publication,|s,p|{
            let m=s.cache.metrics(p.get("reset").and_then(Value::as_bool).unwrap_or(false));
            let events:Vec<_>=m.events.iter().map(|e|{let w=&e.work;json!({"edition":e.edition,"component":e.component,"story":e.story,
                "mode":w.mode,"reason":w.reason,"paragraphs":w.paragraphs,"frames":w.frames,"totalParagraphs":w.total_paragraphs,"totalFrames":w.total_frames,
                "restartParagraph":w.restart_paragraph,"convergedParagraph":w.converged_paragraph,"oldBytes":w.old_bytes.as_ref().map(|r|[r.start,r.end]),"newBytes":w.new_bytes.as_ref().map(|r|[r.start,r.end]),
                "reusedPrefixParagraphs":w.reused_prefix_paragraphs,"reusedSuffixParagraphs":w.reused_suffix_paragraphs,"retainedBytes":w.retained_bytes,"elapsedSeconds":w.elapsed_seconds})}).collect();
            Ok(json!({"calls":m.calls,"exactHits":m.exact_hits,"semanticHits":m.semantic_hits,"incremental":m.incremental,"full":m.full,
                "paragraphs":m.paragraphs,"frames":m.frames,"referenceEvaluations":m.reference_evaluations,"referenceHits":m.reference_hits,"referenceChanges":m.reference_changes,
                "elapsedSeconds":(!cfg!(target_arch="wasm32")).then_some(m.elapsed_seconds),"estimatedRetainedBytes":m.estimated_retained_bytes,"estimatedPeakBytes":m.estimated_peak_bytes,"events":events}))
        }),
        cmd!(query "publication.layout.verify","Verify Layout Against Full Composition",[],None,
        "{} → compare every placed story in the active edition, including notes and table cells, against full composition and freshly evaluated reference fields; expensive, leaves document and undo unchanged",has_publication,verify),
    ]
}
fn verify(s: &mut Session, _: &Value) -> Result<Value> {
    let v = view(s)?;
    let mut stories = 0usize;
    let mut frames = 0usize;
    for component in v.root.ordered_components()? {
        let d = PublicationView { component, ..v.clone() }.project()?;
        for (sid, story) in &d.stories {
            if story.frames.is_empty() || !d.publication_story_included(*sid) {
                continue;
            }
            s.cache.verify(&d, *sid).map_err(|e| bad("publication.layout.verify", format!("component {}: {e}", component.0)))?;
            stories += 1;
            frames += story.frames.len();
        }
    }
    // Force a fresh fixed point with an empty cache, including field rendering, rather than
    // checking only two composers fed the same potentially stale generated values.
    let current = v.root.editions.get(&v.edition).ok_or_else(|| bad("publication.layout.verify", "missing edition"))?;
    let stale = current.references.source_revision != Some(v.root.revision);
    if !stale {
        let mut candidate = v.clone();
        let root = Arc::make_mut(&mut candidate.root);
        Arc::make_mut(root.editions.get_mut(&v.edition).ok_or_else(|| bad("publication.layout.verify", "missing edition"))?)
            .references
            .source_revision = None;
        let mut d = candidate.project()?;
        super::update::refresh(&mut d, true, &designcraft_compose::Cache::new())?;
        let after = d
            .publication
            .as_ref()
            .and_then(|v| v.root.editions.get(&v.edition))
            .ok_or_else(|| bad("publication.layout.verify", "missing updated edition"))?;
        if current.references.fields != after.references.fields
            || current.references.locations != after.references.locations
            || current.references.first_body != after.references.first_body
        {
            return Err(bad("publication.layout.verify", "fresh reference evaluation or pagination differs from the cached edition"));
        }
    }
    Ok(json!({"equivalent":true,"stories":stories,"frames":frames,"referencesChecked":!stale,"stale":stale}))
}
