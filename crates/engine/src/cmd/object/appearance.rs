//! Appearance command implementations.

use super::{Result, Session, Stroke, Value, bad, f64_or, json, set_flag, str_param, targets};

pub(super) fn object_match_attributes(s: &mut Session, p: &Value) -> Result<Value> {
    let from = super::super::id_param(p, "from").ok_or_else(|| bad("object.matchAttributes", "missing from"))?;
    let src = s.doc()?.doc.item(from).cloned().ok_or(designcraft_doc::DocError::NoItem(from))?;
    set_flag(
        s,
        p,
        move |i| {
            if i.id == src.id {
                return;
            }
            i.fill = src.fill.clone();
            i.stroke = src.stroke.clone();
            i.corners = src.corners;
            i.opacity = src.opacity;
            i.blend = src.blend;
            i.effects = src.effects.clone();
        },
        false,
    )
}

pub(super) fn object_stroke(s: &mut Session, p: &Value) -> Result<Value> {
    let p2 = p.clone();
    set_flag(
        s,
        p,
        move |i| {
            let mut st = if i.stroke.is_none() && p2.get("swatch").is_none() { Stroke::default() } else { i.stroke.clone() };
            if let Some(v) = p2.get("swatch").and_then(Value::as_str) {
                st.swatch = v.into();
            }
            if let Some(v) = p2.get("tint").and_then(Value::as_f64) {
                st.tint = v as f32;
            }
            if let Some(v) = p2.get("weight").and_then(Value::as_f64) {
                st.weight = v.max(0.0);
            }
            if let Some(v) = p2.get("miterLimit").and_then(Value::as_f64) {
                st.miter_limit = v.clamp(1.0, 500.0);
            }
            if let Some(v) = p2.get("gapSwatch").and_then(Value::as_str) {
                st.gap_swatch = v.into();
            }
            if let Some(v) = p2.get("gapTint").and_then(Value::as_f64) {
                st.gap_tint = v as f32;
            }
            if let Some(v) = p2.get("gapOverprint").and_then(Value::as_bool) {
                st.gap_overprint = v;
            }
            for (k, apply) in [("align", 0), ("type", 1), ("cap", 2), ("join", 3), ("start", 4), ("end", 5)] {
                if let Some(v) = p2.get(k).cloned() {
                    let _ = match apply {
                        0 => serde_json::from_value(v).map(|x| st.align = x),
                        1 => serde_json::from_value(v).map(|x| st.kind = x),
                        2 => serde_json::from_value(v).map(|x| st.cap = x),
                        3 => serde_json::from_value(v).map(|x| st.join = x),
                        4 => serde_json::from_value(v).map(|x| st.start = x),
                        _ => serde_json::from_value(v).map(|x| st.end = x),
                    };
                }
            }
            i.stroke = st;
        },
        false,
    )
}

pub(super) fn object_opacity(s: &mut Session, p: &Value) -> Result<Value> {
    let o = f64_or(p, "opacity", 1.0).clamp(0.0, 1.0) as f32;
    let blend = p.get("blend").and_then(|b| serde_json::from_value(b.clone()).ok());
    set_flag(
        s,
        p,
        move |i| {
            i.opacity = o;
            if let Some(b) = blend {
                i.blend = b;
            }
        },
        false,
    )
}

pub(super) fn object_drop_shadow(s: &mut Session, p: &Value) -> Result<Value> {
    let p2 = p.clone();
    set_flag(
        s,
        p,
        move |i| {
            let ds = &mut i.effects.drop_shadow;
            ds.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!ds.on);
            ds.distance = f64_or(&p2, "distance", ds.distance);
            ds.angle = f64_or(&p2, "angle", ds.angle);
            ds.global_light = p2.get("globalLight").and_then(Value::as_bool).unwrap_or(ds.global_light);
            ds.opacity = f64_or(&p2, "opacity", ds.opacity as f64) as f32;
            ds.size = f64_or(&p2, "size", ds.size);
            ds.spread = f64_or(&p2, "spread", ds.spread);
            if let Some(c) = str_param(&p2, "color") {
                ds.color = c.into();
            }
        },
        false,
    )
}

pub(super) fn object_inner_shadow(s: &mut Session, p: &Value) -> Result<Value> {
    let p2 = p.clone();
    set_flag(
        s,
        p,
        move |i| {
            let e = &mut i.effects.inner_shadow;
            e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
            e.distance = f64_or(&p2, "distance", e.distance);
            e.angle = f64_or(&p2, "angle", e.angle);
            e.global_light = p2.get("globalLight").and_then(Value::as_bool).unwrap_or(e.global_light);
            e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
            e.size = f64_or(&p2, "size", e.size);
            e.choke = f64_or(&p2, "choke", e.choke);
            if let Some(c) = str_param(&p2, "color") {
                e.color = c.into();
            }
        },
        false,
    )
}

pub(super) fn object_outer_glow(s: &mut Session, p: &Value) -> Result<Value> {
    let p2 = p.clone();
    set_flag(
        s,
        p,
        move |i| {
            let e = &mut i.effects.outer_glow;
            e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
            e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
            e.size = f64_or(&p2, "size", e.size);
            e.spread = f64_or(&p2, "spread", e.spread);
            if let Some(c) = str_param(&p2, "color") {
                e.color = c.into();
            }
        },
        false,
    )
}

pub(super) fn object_export_options(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let p = p.clone();
    s.edit(|d, _| {
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            let e = &mut it.export_options;
            if let Some(v) = p.get("artifact").and_then(Value::as_bool) {
                e.artifact = v;
            }
            if let Some(v) = p.get("rasterize").and_then(Value::as_bool) {
                e.rasterize = v;
            }
            if let Some(v) = p.get("align").and_then(Value::as_str) {
                e.align = v.to_string();
            }
            if let Some(v) = p.get("pageBreakBefore").and_then(Value::as_bool) {
                e.page_break_before = v;
            }
            if let Some(v) = p.get("altText").and_then(Value::as_str) {
                it.alt_text = v.to_string();
            }
        }
        Ok(json!({"objects": ids.len()}))
    })
}

pub(super) fn object_inner_glow(s: &mut Session, p: &Value) -> Result<Value> {
    let p2 = p.clone();
    set_flag(
        s,
        p,
        move |i| {
            let e = &mut i.effects.inner_glow;
            e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
            e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
            e.size = f64_or(&p2, "size", e.size);
            e.choke = f64_or(&p2, "choke", e.choke);
            e.center = p2.get("center").and_then(Value::as_bool).unwrap_or(e.center);
            if let Some(c) = str_param(&p2, "color") {
                e.color = c.into();
            }
        },
        false,
    )
}

pub(super) fn object_bevel(s: &mut Session, p: &Value) -> Result<Value> {
    let p2 = p.clone();
    set_flag(
        s,
        p,
        move |i| {
            let e = &mut i.effects.bevel;
            e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
            e.size = f64_or(&p2, "size", e.size);
            e.depth = f64_or(&p2, "depth", e.depth);
            e.angle = f64_or(&p2, "angle", e.angle);
            e.global_light = p2.get("globalLight").and_then(Value::as_bool).unwrap_or(e.global_light);
            e.highlight_opacity = f64_or(&p2, "highlightOpacity", e.highlight_opacity as f64) as f32;
            e.shadow_opacity = f64_or(&p2, "shadowOpacity", e.shadow_opacity as f64) as f32;
            if let Some(c) = str_param(&p2, "highlight") {
                e.highlight = c.into();
            }
            if let Some(c) = str_param(&p2, "shadow") {
                e.shadow = c.into();
            }
        },
        false,
    )
}

pub(super) fn object_satin(s: &mut Session, p: &Value) -> Result<Value> {
    let p2 = p.clone();
    set_flag(
        s,
        p,
        move |i| {
            let e = &mut i.effects.satin;
            e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
            e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
            e.angle = f64_or(&p2, "angle", e.angle);
            e.distance = f64_or(&p2, "distance", e.distance);
            e.size = f64_or(&p2, "size", e.size);
            e.invert = p2.get("invert").and_then(Value::as_bool).unwrap_or(e.invert);
            if let Some(c) = str_param(&p2, "color") {
                e.color = c.into();
            }
        },
        false,
    )
}

pub(super) fn object_directional_feather(s: &mut Session, p: &Value) -> Result<Value> {
    let p2 = p.clone();
    set_flag(
        s,
        p,
        move |i| {
            let e = &mut i.effects.directional_feather;
            e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
            match p2.get("widths") {
                Some(Value::Number(n)) => e.widths = [n.as_f64().unwrap_or(0.0).max(0.0); 4],
                Some(v @ Value::Array(_)) => {
                    if let Ok(w) = serde_json::from_value::<[f64; 4]>(v.clone()) {
                        e.widths = w.map(|x| x.max(0.0));
                    }
                }
                _ => {}
            }
            if e.on && e.widths.iter().all(|w| *w <= 0.0) {
                e.widths = [9.0; 4];
            }
        },
        false,
    )
}

pub(super) fn object_text_wrap(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = p.get("mode").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(designcraft_doc::WrapMode::BoundingBox);
    let off = match p.get("offset") {
        Some(Value::Number(n)) => Some([n.as_f64().unwrap_or(0.0); 4]),
        Some(v) => serde_json::from_value(v.clone()).ok(),
        None => None,
    };
    let invert = p.get("invert").and_then(Value::as_bool);
    set_flag(
        s,
        p,
        move |i| {
            i.wrap.mode = mode;
            if let Some(o) = off {
                i.wrap.offsets = o;
            }
            if let Some(v) = invert {
                i.wrap.invert = v;
            }
        },
        false,
    )
}

pub(super) fn object_transparency_group(s: &mut Session, p: &Value) -> Result<Value> {
    let iso = p.get("isolate").and_then(Value::as_bool);
    let ko = p.get("knockout").and_then(Value::as_bool);
    set_flag(
        s,
        p,
        move |i| {
            if let Some(v) = iso {
                i.isolate = v;
            }
            if let Some(v) = ko {
                i.knockout = v;
            }
        },
        false,
    )
}
