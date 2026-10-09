//! Canvas view and proof display commands.

use super::{DesignApp, ScreenMode, Value, json};

pub(super) fn run(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let rect = app.canvas_rect;
    let flag = |b: &mut bool| {
        *b = !*b;
        Ok(json!(*b))
    };
    Some(match id {
        "view.zoomIn" | "view.zoomOut" => {
            if let Some(r) = rect {
                crate::canvas::zoom_at(app, r.center(), if id == "view.zoomIn" { 1.5 } else { 1.0 / 1.5 });
            }
            Ok(Value::Null)
        }
        "view.fitSelection" => {
            if let Some(r) = rect {
                crate::canvas::fit(app, r, "selection");
            }
            Ok(Value::Null)
        }
        "view.fitPage" | "view.fitSpread" | "view.entirePasteboard" => {
            if let Some(r) = rect {
                crate::canvas::fit(
                    app,
                    r,
                    if id == "view.fitPage" {
                        "page"
                    } else if id == "view.fitSpread" {
                        "spread"
                    } else {
                        "all"
                    },
                );
            }
            Ok(Value::Null)
        }
        "view.actualSize" => {
            crate::canvas::set_zoom(app, 1.0);
            Ok(Value::Null)
        }
        "view.zoom" => {
            crate::canvas::set_zoom(app, p.get("zoom").and_then(Value::as_f64).unwrap_or(1.0));
            Ok(Value::Null)
        }
        "view.screenMode" => {
            app.ui.screen_mode = match p.get("mode").and_then(Value::as_str).unwrap_or("normal") {
                "preview" => ScreenMode::Preview,
                "bleed" => ScreenMode::Bleed,
                "slug" => ScreenMode::Slug,
                "presentation" => ScreenMode::Presentation,
                _ => ScreenMode::Normal,
            };
            Ok(Value::Null)
        }
        "view.togglePreview" => {
            app.ui.screen_mode = if app.ui.screen_mode == ScreenMode::Normal { ScreenMode::Preview } else { ScreenMode::Normal };
            Ok(Value::Null)
        }
        "view.frameEdges" => flag(&mut app.ui.frame_edges),
        "view.overprintPreview" => {
            app.canvas.shown = None;
            flag(&mut app.ui.overprint_preview)
        }
        "view.fastDisplay" | "view.typicalDisplay" | "view.highQualityDisplay" => {
            use designcraft_render::DisplayQuality as Q;
            app.ui.display_quality = match id {
                "view.fastDisplay" => Q::Fast,
                "view.typicalDisplay" => Q::Typical,
                _ => Q::High,
            };
            // Redraw everything at the new quality.
            app.canvas.shown = None;
            Ok(Value::Null)
        }
        "view.rulers" => flag(&mut app.ui.rulers),
        "view.guides" => flag(&mut app.ui.guides),
        "view.snapToGuides" => flag(&mut app.ui.snap_to_guides),
        "view.snapToDocumentGrid" => flag(&mut app.ui.snap_to_document_grid),
        "view.smartGuides" => flag(&mut app.ui.smart_guides),
        "view.baselineGrid" => flag(&mut app.ui.baseline_grid),
        "view.textThreads" => flag(&mut app.ui.text_threads),
        "view.hiddenCharacters" => flag(&mut app.ui.hidden_characters),
        "view.taggedFrames" => flag(&mut app.ui.tagged_frames),
        "view.rotateSpread" => {
            // Turns the spread in the middle of the view (InDesign's Rotate Spread).
            let angle = p.get("angle").and_then(Value::as_i64).unwrap_or(90);
            let Some(st) = app.session.active() else { return Some(Err("no document".into())) };
            let layout = designcraft_tools::CanvasLayout::new(&st.doc, st.editing_parents);
            let Some(slot) = crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i).copied()) else {
                return Some(Err("no spread".into()));
            };
            let designcraft_doc::SpreadRef::Doc(si) = slot.spread else { return Some(Err("parent spreads don't turn".into())) };
            let r = app.run("layout.rotateSpreadView", json!({"spread": si, "angle": angle}));
            if let Some(rect) = app.canvas_rect {
                crate::canvas::fit(app, rect, "spread");
            }
            app.canvas.shown = None;
            r.map(|v| v["rotation"].clone())
        }
        "view.proofColors" => {
            app.ui.proof_colors = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.proof_colors);
            app.canvas.shown = None;
            Ok(json!(app.ui.proof_colors))
        }
        "view.proofSetup" => {
            if let Some(t) = p.get("target").and_then(Value::as_str) {
                let Some(t) = designcraft_color::cms::ProofTarget::parse(t) else { return Some(Err(format!("unknown proof target `{t}`"))) };
                app.ui.proof_setup.target = t;
                app.ui.proof_colors = true;
            }
            if let Some(i) = p.get("intent").and_then(Value::as_str) {
                let Some(i) = designcraft_color::cms::Intent::parse(i) else { return Some(Err(format!("unknown intent `{i}`"))) };
                app.ui.proof_setup.intent = i;
            }
            match p.get("simulatePaper") {
                Some(Value::Bool(b)) => app.ui.proof_setup.simulate_paper = *b,
                Some(Value::String(s)) if s == "toggle" => app.ui.proof_setup.simulate_paper = !app.ui.proof_setup.simulate_paper,
                _ => {}
            }
            app.canvas.shown = None;
            Ok(json!({"target": app.ui.proof_setup.target.id(), "simulatePaper": app.ui.proof_setup.simulate_paper, "on": app.ui.proof_colors}))
        }
        "view.separations" => {
            if let Some(v) = p.get("plate") {
                app.ui.separation = v.as_str().and_then(|s| ["cyan", "magenta", "yellow", "black"].iter().position(|x| *x == s)).map(|i| i as u8);
            }
            if let Some(v) = p.get("inkLimit") {
                app.ui.ink_limit = v.as_f64().map(|l| (l / 100.0) as f32);
            }
            app.canvas.shown = None;
            Ok(Value::Null)
        }
        "view.goToPage" => {
            match p.get("page") {
                Some(Value::Number(n)) => crate::canvas::go_to_page(app, (n.as_u64().unwrap_or(1) as usize).saturating_sub(1)),
                Some(Value::String(s)) => match app.session.resolve_page(s) {
                    Some(abs) => crate::canvas::go_to_page(app, abs),
                    None => return Some(Err(format!("no page \"{s}\""))),
                },
                _ => app.ui.dialog = Some(crate::dialogs::Dialog::new("goToPage", json!({}))),
            }
            Ok(Value::Null)
        }
        "view.snapPreferences" => {
            let b = |k: &str| p.get(k).and_then(Value::as_bool);
            let ui = &mut app.ui;
            ui.align_edges = b("alignEdges").unwrap_or(ui.align_edges);
            ui.align_centers = b("alignCenters").unwrap_or(ui.align_centers);
            ui.smart_dimensions = b("smartDimensions").unwrap_or(ui.smart_dimensions);
            ui.smart_spacing = b("smartSpacing").unwrap_or(ui.smart_spacing);
            if let Some(z) = p.get("zone").and_then(Value::as_f64).filter(|z| z.is_finite()) {
                ui.snap_zone = z.max(0.0);
            }
            Ok(json!({
                "alignEdges": ui.align_edges,
                "alignCenters": ui.align_centers,
                "smartDimensions": ui.smart_dimensions,
                "smartSpacing": ui.smart_spacing,
                "zone": ui.snap_zone,
            }))
        }
        "view.flattenerPreview" => flag(&mut app.ui.flattener_preview),
        "view.tagMarkers" => {
            app.canvas.shown = None;
            flag(&mut app.ui.tag_markers)
        }
        _ => return None,
    })
}
