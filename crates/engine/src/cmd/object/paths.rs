//! Path creation and anchor editing.

use super::{Item, ItemId, Point, Result, Selection, Session, Shape, Stroke, Value, Vec2, bad, bool_or, f64_or, json, ok, spread_param, str_param};

fn anchor_from(v: &Value) -> Option<designcraft_geom::Anchor> {
    let pt = |k: &str| -> Option<Point> {
        let a = v.get(k)?.as_array()?;
        Some(Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
    };
    let p = pt("p")?;
    let h_in = pt("in").unwrap_or(p);
    let h_out = pt("out").unwrap_or(p);
    let smooth = (h_in - p).hypot() > 1e-9 || (h_out - p).hypot() > 1e-9;
    Some(designcraft_geom::Anchor {
        p,
        h_in,
        h_out,
        kind: if smooth { designcraft_geom::AnchorKind::Smooth } else { designcraft_geom::AnchorKind::Corner },
    })
}

pub(super) fn path_create(s: &mut Session, p: &Value) -> Result<Value> {
    let anchors: Vec<designcraft_geom::Anchor> =
        p.get("anchors").and_then(Value::as_array).map(|a| a.iter().filter_map(anchor_from).collect()).unwrap_or_default();
    if anchors.len() < 2 {
        return Err(bad("path.create", "a path needs at least two anchors"));
    }
    let closed = bool_or(p, "closed", false);
    let sr = spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let id = ItemId(d.alloc());
        let shape = if anchors.len() == 2 && !closed { Shape::GraphicLine } else { Shape::Path };
        let mut it = Item::new(id, lid, shape, designcraft_geom::PathData::single(designcraft_geom::SubPath::new(anchors, closed)));
        it.stroke = Stroke::default();
        d.insert_item(sr, it, None)?;
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

pub(super) fn path_append(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::super::id_param(p, "id").ok_or_else(|| bad("path.appendAnchor", "missing id"))?;
    let a = p.get("anchor").and_then(anchor_from).ok_or_else(|| bad("path.appendAnchor", "missing anchor"))?;
    s.edit(|d, _| {
        let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let inv = (d.parent_xf(&loc) * d.item_at(&loc).map(|i| i.xf).unwrap_or_default()).inverse();
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let sp = it.path.subpaths.last_mut().ok_or_else(|| bad("path.appendAnchor", "empty path"))?;
        sp.anchors.push(a.transform(inv));
        if it.shape == Shape::GraphicLine {
            it.shape = Shape::Path;
        }
        ok()
    })
}

pub(super) fn path_close(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::super::id_param(p, "id").ok_or_else(|| bad("path.close", "missing id"))?;
    s.edit(|d, _| {
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        for sp in &mut it.path.subpaths {
            sp.closed = true;
        }
        if it.shape == Shape::GraphicLine {
            it.shape = Shape::Path;
        }
        ok()
    })
}

pub(super) fn path_move_anchors(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::super::id_param(p, "id").ok_or_else(|| bad("path.moveAnchors", "missing id"))?;
    let which: Vec<(usize, usize)> = p.get("anchors").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let d0 = Vec2::new(f64_or(p, "dx", 0.0), f64_or(p, "dy", 0.0));
    let handle = str_param(p, "handle").map(str::to_string);
    s.edit(|d, _| {
        let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let m = d.parent_xf(&loc) * d.item_at(&loc).map(|i| i.xf).unwrap_or_default();
        // Spread-space delta → inner-space delta (linear part only).
        let inv = m.inverse();
        let dv = (inv * Point::new(d0.x, d0.y)) - (inv * Point::ZERO);
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        for (si, ai) in &which {
            let Some(a) = it.path.anchor_mut(*si, *ai) else { continue };
            match handle.as_deref() {
                Some("in") => {
                    a.h_in += dv;
                    if a.kind == designcraft_geom::AnchorKind::Smooth {
                        a.h_out = a.p - (a.h_in - a.p);
                    }
                }
                Some("out") => {
                    a.h_out += dv;
                    if a.kind == designcraft_geom::AnchorKind::Smooth {
                        a.h_in = a.p - (a.h_out - a.p);
                    }
                }
                _ => a.translate(dv),
            }
        }
        if it.shape == Shape::Rectangle || it.shape == Shape::Oval || it.shape == Shape::Polygon {
            it.shape = Shape::Path;
        }
        ok()
    })
}
