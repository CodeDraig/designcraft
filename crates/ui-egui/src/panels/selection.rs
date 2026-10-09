//! Projection of document selection into panel values.

use super::{Content, DesignApp, Rect, StrokeAlign, Value, WrapMode, json};

/// Summary of the current object selection (for the Control panel and Properties).
#[derive(Clone, Debug)]
pub struct SelInfo {
    /// Bounds relative to the page the selection is on.
    pub page_rect: Rect,
    pub count: usize,
    pub fill: String,
    pub stroke: String,
    pub stroke_weight: f64,
    pub stroke_align: StrokeAlign,
    pub opacity: f64,
    pub shadow: bool,
    pub wrap: &'static str,
    pub columns: Option<u32>,
    pub kind: &'static str,
    pub is_graphic: bool,
    pub is_text: bool,
    /// Stroke type id (`solid`, `dashed`, `thickThin`, …).
    pub stroke_kind: String,
    /// Uniform corner (shape, size) — the first corner.
    pub corner: (designcraft_geom::corners::CornerShape, f64),
    pub wrap_invert: bool,
    pub locked: bool,
}

/// Fill (or stroke) tint of the first selected item (1 = 100%).
pub fn sel_tint(app: &DesignApp, stroke: bool) -> f32 {
    let Some(st) = app.session.active() else { return 1.0 };
    st.selection.items.first().and_then(|id| st.doc.item(*id)).map_or(1.0, |it| if stroke { it.stroke.tint } else { it.fill.tint })
}

pub fn sel_info(app: &DesignApp) -> Option<SelInfo> {
    let st = app.session.active()?;
    if st.selection.items.is_empty() {
        return None;
    }
    let d = &st.doc;
    let with_stroke = app.session.prefs.dimensions_include_stroke;
    let mut b: Option<Rect> = None;
    for id in &st.selection.items {
        let it = d.item(*id)?;
        let ib = if with_stroke { it.visible_bounds() } else { it.bounds() };
        b = Some(b.map_or(ib, |r| r.union(ib)));
    }
    let b = b?;
    let first = d.item(st.selection.items[0])?;
    let loc = d.find(first.id)?;
    let sp = d.spread(loc.spread)?;
    let pi = sp.page_at_x(b.center().x).unwrap_or(0);
    let px = sp.pages.get(pi).map(|p| p.x).unwrap_or(0.0);
    let wrap = match first.wrap.mode {
        WrapMode::None => "none",
        WrapMode::BoundingBox => "boundingBox",
        WrapMode::Contour => "contour",
        WrapMode::JumpObject => "jumpObject",
        WrapMode::JumpToNextColumn => "jumpToNextColumn",
    };
    Some(SelInfo {
        page_rect: Rect::new(b.x0 - px, b.y0, b.x1 - px, b.y1),
        count: st.selection.items.len(),
        fill: first.fill.swatch.clone(),
        stroke: first.stroke.swatch.clone(),
        stroke_weight: first.stroke.weight,
        stroke_align: first.stroke.align,
        opacity: first.opacity as f64,
        shadow: first.effects.drop_shadow.on,
        wrap,
        columns: first.text_frame().map(|t| t.options.columns),
        kind: first.default_label(),
        is_graphic: matches!(first.content, Content::Graphic(_)),
        is_text: first.is_text_frame(),
        stroke_kind: serde_json::to_value(&first.stroke.kind)
            .ok()
            .and_then(|v| v.get("kind").and_then(Value::as_str).map(str::to_string).or_else(|| v.as_str().map(str::to_string)))
            .unwrap_or_else(|| "solid".into()),
        corner: (first.corners.corners[0].shape, first.corners.corners[0].size),
        wrap_invert: first.wrap.invert,
        locked: first.locked,
    })
}

pub fn text_attrs(app: &mut DesignApp) -> Option<Value> {
    let v = app.session.execute("type.selectionAttrs", &json!({})).ok()?;
    if v.is_null() { None } else { Some(serde_json::to_value(v).ok()?) }
}

/// Count preflight errors: overset stories (more checks land with the Preflight panel).
pub fn preflight_errors(app: &DesignApp) -> usize {
    // Memoized per document snapshot (the status bar asks every frame).
    static MEMO: std::sync::Mutex<Option<(usize, usize)>> = std::sync::Mutex::new(None);
    let Some(st) = app.session.active() else { return 0 };
    let key = std::sync::Arc::as_ptr(&st.doc) as usize;
    let mut g = MEMO.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, n)) = *g
        && k == key
    {
        return n;
    }
    let n = designcraft_engine::cmd::preflight::check(&app.session, 150.0).iter().filter(|i| i.severity == "error").count();
    *g = Some((key, n));
    n
}
