use super::{Dialog, json};
pub(super) fn show(ui: &mut egui::Ui, d: &mut Dialog) {
    ui.heading("Publication layout work");
    let r = d.fields.get("report").cloned().unwrap_or_default();
    egui::Grid::new("layout_work").num_columns(2).show(ui, |ui| {
        for (key, label) in [
            ("paragraphs", "Paragraphs recomposed"),
            ("frames", "Frames recomposed"),
            ("incremental", "Checkpoint compositions"),
            ("full", "Full compositions"),
            ("semanticHits", "Unchanged chapter/story hits"),
            ("exactHits", "Exact cache hits"),
            ("referenceEvaluations", "References evaluated"),
            ("referenceHits", "Reference cache hits"),
            ("referenceChanges", "Reference values changed"),
        ] {
            ui.label(label);
            ui.label(r[key].as_u64().unwrap_or_default().to_string());
            ui.end_row();
        }
    });
    if let Some(seconds) = r["elapsedSeconds"].as_f64() {
        ui.label(format!("Composition time: {seconds:.3} seconds"));
    }
    ui.label(format!("Estimated cached payload: {:.1} MiB", r["estimatedRetainedBytes"].as_u64().unwrap_or_default() as f64 / 1048576.));
    ui.small("Counters cover this session. Memory excludes allocator overhead and font storage.");
    ui.separator();
    let mut verify = d.fields.get("verify").and_then(|v| v.as_bool()).unwrap_or(false);
    ui.checkbox(&mut verify, "Compare with full composition when confirming (may take time)");
    d.fields.insert("verify".into(), json!(verify));
    if let Some(v) = d.fields.get("verification").filter(|v| v["equivalent"] == true) {
        ui.label(format!("Verified: {} stories, {} frames match full composition.", v["stories"], v["frames"]));
    }
    ui.label("Confirm refreshes this report.");
    egui::ScrollArea::vertical().max_height(210.).show(ui, |ui| {
        if let Some(events) = r["events"].as_array() {
            for e in events.iter().rev().take(20) {
                let chapter = d
                    .fields
                    .get("components")
                    .and_then(|c| c.as_array())
                    .and_then(|rows| rows.iter().find(|c| c["id"] == e["component"]))
                    .and_then(|c| c["title"].as_str())
                    .unwrap_or("Chapter");
                let mode = match e["mode"].as_str() {
                    Some("semanticHit") => "unchanged",
                    Some("incremental") => "resumed",
                    _ => "full composition",
                };
                ui.label(format!("{chapter} · {mode} · {} paragraphs, {} frames", e["paragraphs"], e["frames"]));
                if let Some(reason) = e["reason"].as_str() {
                    ui.small(reason);
                }
            }
        }
    });
}
