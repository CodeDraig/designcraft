//! Object transforms, measurement, and transform repetition.

use super::RemoveKeep;
use super::{
    Affine, Arc, Content, Document, Item, ItemId, Point, Rect, Result, Selection, Session, Shape, SpreadRef, Value, Vec2, bad, bool_or,
    duplicate_from, f64_or, json, ok, rect_param, targets,
};

pub(super) fn transform_move(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let (dx, dy) = (f64_or(p, "dx", 0.0), f64_or(p, "dy", 0.0));
    let copy = bool_or(p, "copy", false);
    let to: Option<SpreadRef> = p.get("toSpread").and_then(|v| serde_json::from_value(v.clone()).ok());
    s.edit(|d, sel| {
        let ids = if copy {
            let src = d.clone();
            let mut new = Vec::new();
            for id in &ids {
                let sr = d.find(*id).map(|l| l.spread).unwrap_or(SpreadRef::Doc(0));
                new.extend(duplicate_from(d, &src, &[*id], to.unwrap_or(sr), Vec2::ZERO)?);
            }
            *sel = Selection::items(new.clone());
            new
        } else {
            ids
        };
        for id in &ids {
            if let Some(target) = to
                && !copy
                && let Some(loc) = d.find(*id)
                && loc.spread != target
                && loc.path.len() == 1
            {
                let it = d.remove_item_keep_story(*id)?;
                d.insert_item(target, it, None)?;
            }
            let it = d.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            if it.locked {
                continue;
            }
            it.xf = Affine::translate((dx, dy)) * it.xf;
        }
        Ok(json!({"moved": ids.len()}))
    })
}

pub(super) fn transform_resize(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let from = rect_param(p, "from").ok_or_else(|| bad("transform.resize", "missing from"))?;
    let to = rect_param(p, "to").ok_or_else(|| bad("transform.resize", "missing to"))?;
    let scale_content = bool_or(p, "content", false);
    let distribute = bool_or(p, "distribute", false);
    if from.width().abs() < 1e-9 || from.height().abs() < 1e-9 {
        return Err(bad("transform.resize", "degenerate source rect"));
    }
    let sx = to.width() / from.width();
    let sy = to.height() / from.height();
    let sx = if sx.abs() < 1e-3 { 1e-3f64.copysign(sx) } else { sx };
    let sy = if sy.abs() < 1e-3 { 1e-3f64.copysign(sy) } else { sy };
    let m = Affine::translate((to.x0, to.y0)) * Affine::scale_non_uniform(sx, sy) * Affine::translate((-from.x0, -from.y0));
    let strokes = s.prefs.scale_strokes;
    s.edit(|d, _| {
        for id in &ids {
            let it = d.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            if it.locked {
                continue;
            }
            if distribute {
                let c = it.bounds().center();
                it.xf = Affine::translate(m * c - c) * it.xf;
            } else if scale_content {
                scale_item(it, m, (sx * sy).abs().sqrt(), strokes);
            } else if matches!(it.content, Content::Group { .. }) {
                it.xf = m * it.xf;
            } else {
                // Resize the frame's path (in inner space); content keeps its size (InDesign's
                // default) unless the frame auto-fits it.
                let inner = it.xf.inverse() * m * it.xf;
                bake(it, inner);
                let r = it.inner_bounds();
                if let Content::Graphic(g) = &mut it.content
                    && let Some(xf) = g.fitted(r, g.auto_fit)
                {
                    g.xf = xf;
                }
            }
        }
        Ok(json!({"resized": ids.len()}))
    })
}

pub(super) fn union_bounds(d: &Document, ids: &[ItemId]) -> Rect {
    ids.iter().filter_map(|i| d.item(*i)).map(Item::bounds).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO)
}

/// Transform an item's geometry in its own space: the path and the gradient vector with it.
pub(crate) fn bake(it: &mut Item, inner: Affine) {
    it.path.transform(inner);
    if let Some([x0, y0, x1, y1]) = it.fill.gradient_vector {
        let (a, b) = (inner * Point::new(x0, y0), inner * Point::new(x1, y1));
        it.fill.gradient_vector = Some([a.x, a.y, b.x, b.y]);
    }
}

/// Multiply the stroke weights of an item and everything nested in it.
fn scale_weights(it: &mut Item, k: f64) {
    it.stroke.weight *= k;
    if let Content::Group { items } = &mut it.content {
        for c in items.iter_mut() {
            scale_weights(Arc::make_mut(c), k);
        }
    }
}

/// Scale an item and its content by the spread-space transform `m` (InDesign's "Apply to
/// Content"): paths and placed graphics take the scale into their geometry, so the stroke weight
/// is scaled by `k` (the mean scale factor) only with Scale Strokes on; text frames and groups are
/// transformed as a whole, so with Scale Strokes off their weights are divided by `k` to keep
/// their look.
pub(crate) fn scale_item(it: &mut Item, m: Affine, k: f64, strokes: bool) {
    let inner = it.xf.inverse() * m * it.xf;
    match &mut it.content {
        Content::Unassigned => {
            bake(it, inner);
            if strokes {
                it.stroke.weight *= k;
            }
        }
        Content::Graphic(g) => {
            g.xf = inner * g.xf;
            bake(it, inner);
            if strokes {
                it.stroke.weight *= k;
            }
        }
        Content::Text(_) | Content::Group { .. } => {
            it.xf = m * it.xf;
            if !strokes && k > 1e-9 {
                scale_weights(it, 1.0 / k);
            }
        }
    }
}

pub(super) fn transform_again(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let (list, _, _) = &s.transforms;
    if list.is_empty() {
        return Err(bad("transform.again", "no transform to repeat"));
    }
    let steps: Vec<(String, Value)> = if bool_or(p, "sequence", false) { list.clone() } else { list.last().cloned().into_iter().collect() };
    let mut groups: Vec<Vec<ItemId>> = if bool_or(p, "individually", false) { ids.iter().map(|i| vec![*i]).collect() } else { vec![ids.clone()] };
    // Run the steps directly (no recording, one undo step for the whole repeat).
    let saved = s.transforms.clone();
    for (cmd, params) in &steps {
        let spec = super::super::find_command(cmd).ok_or_else(|| bad("transform.again", "unknown transform"))?;
        for g in &groups {
            let q = super::super::with_param(params, "ids", json!(g.iter().map(|i| i.0).collect::<Vec<_>>()));
            (spec.run)(s, &q)?;
        }
        // Moving a copy selects the copies: later steps (and the next repeat) act on them.
        if bool_or(params, "copy", false) {
            groups = vec![s.doc()?.selection.items.clone()];
        }
    }
    s.transforms = saved;
    Ok(json!({"repeated": steps.len()}))
}

pub(super) fn clear_transformations(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            if matches!(it.content, Content::Group { .. }) && it.shape == Shape::Group {
                continue;
            }
            let c = it.bounds().center();
            let inner = it.inner_bounds().center();
            it.xf = Affine::translate(c - inner);
        }
        ok()
    })
}

pub(super) fn apply_about_center(s: &mut Session, p: &Value, a: Affine) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        let mut b: Option<Rect> = None;
        for id in &ids {
            if let Some(it) = d.item(*id) {
                b = Some(b.map_or(it.bounds(), |r| r.union(it.bounds())));
            }
        }
        let c = b.unwrap_or(Rect::ZERO).center();
        let m = Affine::translate(c.to_vec2()) * a * Affine::translate(-c.to_vec2());
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                it.xf = m * it.xf;
            }
        }
        ok()
    })
}

pub(super) fn transform_rotate(s: &mut Session, p: &Value) -> Result<Value> {
    let a = f64_or(p, "angle", 0.0);
    apply_about_center(s, p, Affine::rotate(-a.to_radians()))
}

/// The transform an item's panel values are measured from: its own (relative to its container)
/// or, with Transformations are Totals, the whole chain to the pasteboard.
fn measured_xf(d: &designcraft_doc::Document, id: ItemId, totals: bool) -> Option<(Affine, Affine)> {
    let loc = d.find(id)?;
    let it = d.item_at(&loc)?;
    let parent = if totals { d.parent_xf(&loc) } else { Affine::IDENTITY };
    Some((parent, parent * it.xf))
}

fn values_json(m: Affine) -> Value {
    let v = designcraft_geom::decompose::decompose(m);
    let r = |x: f64| (x * 1e4).round() / 1e4;
    json!({"scaleX": r(v.scale_x * 100.0), "scaleY": r(v.scale_y * 100.0), "rotation": r(v.rotation), "shear": r(v.shear)})
}

pub(super) fn transform_info(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let totals = s.prefs.transformations_are_totals;
    let d = &s.doc()?.doc;
    let id = *ids.first().ok_or_else(|| bad("transform.info", "nothing selected"))?;
    let (_, m) = measured_xf(d, id, totals).ok_or_else(|| bad("transform.info", "no such object"))?;
    let mut out = values_json(m);
    if let Some(g) = d.item(id).and_then(Item::graphic) {
        out["content"] = values_json(if totals { m * g.xf } else { g.xf });
    }
    Ok(out)
}

/// Absolute rotation / shear / scale from the Transform panel, about the reference point.
fn transform_values(s: &mut Session, p: &Value, ids: &[ItemId], rf: usize) -> Result<Value> {
    let totals = s.prefs.transformations_are_totals;
    let strokes = s.prefs.scale_strokes;
    let get = |k: &str| p.get(k).and_then(Value::as_f64);
    let (rot, shear, sx, sy) = (get("rotation"), get("shear").map(|v| v.clamp(-85.0, 85.0)), get("scaleX"), get("scaleY"));
    if [sx, sy].iter().flatten().any(|v| v.abs() < 0.01) {
        return Err(bad("transform.set", "scale too small"));
    }
    s.edit(|d, _| {
        for id in ids {
            let Some((parent, cur)) = measured_xf(d, *id, totals) else { continue };
            let Some(it) = d.item(*id) else { continue };
            // The reference point, on the pasteboard, then in the measuring space.
            let full = d.find(*id).map_or(Affine::IDENTITY, |l| d.parent_xf(&l));
            let spread_a = designcraft_geom::reference_point((full * it.xf).transform_rect_bbox(it.inner_bounds()), rf);
            let a = if totals { spread_a } else { full.inverse() * spread_a };
            let v = designcraft_geom::decompose::decompose(cur);
            let mut t = v;
            if let Some(r) = rot {
                t.rotation = r;
            }
            if let Some(k) = shear {
                t.shear = k;
            }
            let lin = |m: Affine| {
                let c = m.as_coeffs();
                Affine::new([c[0], c[1], c[2], c[3], 0.0, 0.0])
            };
            let delta = designcraft_geom::decompose::compose(&t) * lin(cur).inverse();
            // Δ in measuring space about `a`, brought into the item's container space.
            let m = parent.inverse() * Affine::translate(a.to_vec2()) * delta * Affine::translate(-a.to_vec2()) * parent;
            let it = d.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            it.xf = m * it.xf;
            // Scale: relative to what is shown (frames show 100% — their scale lives in the geometry).
            if sx.is_some() || sy.is_some() {
                let (fx, fy) = (sx.map_or(1.0, |x| x / 100.0 / v.scale_x), sy.map_or(1.0, |y| y / 100.0 / v.scale_y));
                let m = Affine::translate(a.to_vec2()) * Affine::scale_non_uniform(fx, fy) * Affine::translate(-a.to_vec2());
                let m = parent.inverse() * m * parent;
                scale_item(it, m, (fx * fy).abs().sqrt(), strokes);
            }
        }
        ok()
    })
}

pub(super) fn transform_set(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    if ["rotation", "shear", "scaleX", "scaleY"].iter().any(|k| p.get(*k).is_some()) {
        let rf = p.get("ref").and_then(Value::as_u64).unwrap_or(4) as usize;
        if !["x", "y", "width", "height"].iter().any(|k| p.get(*k).is_some()) {
            return transform_values(s, p, &ids, rf);
        }
        transform_values(s, p, &ids, rf)?;
    }
    let rf = p.get("ref").and_then(Value::as_u64).unwrap_or(0) as usize;
    let with_stroke = s.prefs.dimensions_include_stroke;
    s.edit(|d, _| {
        if !ids.iter().any(|i| d.item(*i).is_some()) {
            return ok();
        }
        // The measured box (with Dimensions Include Stroke Weight, the stroke's outer edge) and
        // the geometry it pads.
        let geom = union_bounds(d, &ids);
        let from = if with_stroke {
            ids.iter().filter_map(|i| d.item(*i)).map(Item::visible_bounds).reduce(|a, b| a.union(b)).unwrap_or(geom)
        } else {
            geom
        };
        let pad = (geom.x0 - from.x0, geom.y0 - from.y0, from.x1 - geom.x1, from.y1 - geom.y1);
        let w = f64_or(p, "width", from.width()).max(0.01);
        let h = f64_or(p, "height", from.height()).max(0.01);
        let anchor = designcraft_geom::reference_point(from, rf);
        // New rect keeping the reference point fixed, then moved so the ref point lands on x/y.
        let fx = [0.0, 0.5, 1.0][rf % 3];
        let fy = [0.0, 0.5, 1.0][(rf / 3).min(2)];
        let mut to = Rect::new(anchor.x - fx * w, anchor.y - fy * h, anchor.x - fx * w + w, anchor.y - fy * h + h);
        // x/y are the reference point position relative to the page origin of the first page in the spread.
        let page_x = ids
            .first()
            .and_then(|i| d.find(*i))
            .and_then(|l| {
                d.spread(l.spread).map(|sp| {
                    let pi = sp.page_at_x(from.center().x).unwrap_or(0);
                    sp.pages[pi].x
                })
            })
            .unwrap_or(0.0);
        if let Some(x) = p.get("x").and_then(Value::as_f64) {
            to = to + Vec2::new(x + page_x - anchor.x, 0.0);
        }
        if let Some(y) = p.get("y").and_then(Value::as_f64) {
            to = to + Vec2::new(0.0, y - anchor.y);
        }
        let to = Rect::new(to.x0 + pad.0, to.y0 + pad.1, (to.x1 - pad.2).max(to.x0 + pad.0 + 0.01), (to.y1 - pad.3).max(to.y0 + pad.1 + 0.01));
        let m = Affine::translate((to.x0, to.y0))
            * Affine::scale_non_uniform(to.width() / geom.width().max(1e-9), to.height() / geom.height().max(1e-9))
            * Affine::translate((-geom.x0, -geom.y0));
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                if matches!(it.content, Content::Group { .. }) {
                    it.xf = m * it.xf;
                } else {
                    let inner = it.xf.inverse() * m * it.xf;
                    bake(it, inner);
                }
            }
        }
        ok()
    })
}

pub(super) fn step_and_repeat(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let (dx, dy) = (f64_or(p, "dx", 12.0), f64_or(p, "dy", 12.0));
    let rows = p.get("rows").and_then(Value::as_u64).map(|v| v as usize);
    let cols = p.get("columns").and_then(Value::as_u64).map(|v| v as usize);
    let count = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000) as usize;
    let offsets: Vec<Vec2> = match (rows, cols) {
        (Some(r), Some(c)) if r * c <= 1000 => (0..r)
            .flat_map(|i| (0..c).map(move |j| (i, j)))
            .filter(|&(i, j)| i + j > 0)
            .map(|(i, j)| Vec2::new(j as f64 * dx, i as f64 * dy))
            .collect(),
        _ => (1..=count).map(|k| Vec2::new(k as f64 * dx, k as f64 * dy)).collect(),
    };
    s.edit(|d, sel| {
        let src = d.clone();
        let mut all = ids.clone();
        for off in offsets {
            for id in &ids {
                let sr = src.find(*id).map(|l| l.spread).unwrap_or(SpreadRef::Doc(0));
                all.extend(duplicate_from(d, &src, &[*id], sr, off)?);
            }
        }
        *sel = Selection::items(all.clone());
        Ok(json!({"created": all.len() - ids.len()}))
    })
}

pub(super) fn transform_scale(s: &mut Session, p: &Value) -> Result<Value> {
    let sx = f64_or(p, "sx", 1.0);
    let sy = f64_or(p, "sy", sx);
    if sx.abs() < 1e-4 || sy.abs() < 1e-4 {
        return Err(bad("transform.scale", "scale too small"));
    }
    let strokes = s.prefs.scale_strokes;
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        let c = union_bounds(d, &ids).center();
        let m = Affine::translate(c.to_vec2()) * Affine::scale_non_uniform(sx, sy) * Affine::translate(-c.to_vec2());
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                scale_item(it, m, (sx * sy).abs().sqrt(), strokes);
            }
        }
        ok()
    })
}
