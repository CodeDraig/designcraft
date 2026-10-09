//! Gap discovery and gap movement.

use super::{Affine, Content, Document, ItemId, Point, Rect, Result, Session, Value, bad, bake, json, spread_param};

/// The gap at `p` on spread `sr`: (vertical gap?, edge a, edge b, ids ending at a, ids starting at b).
pub(crate) fn find_gap(d: &Document, sr: designcraft_doc::SpreadRef, p: Point) -> Option<(bool, f64, f64, Vec<ItemId>, Vec<ItemId>)> {
    let sp = d.spread(sr)?;
    let page = sp.page_at_x(p.x).and_then(|i| sp.pages.get(i)).map(|pg| pg.bounds())?;
    let items: Vec<(ItemId, Rect)> = sp
        .items
        .iter()
        .filter(|it| !it.hidden && !it.locked && d.layer(it.layer).is_none_or(|l| !l.locked && l.visible))
        .map(|it| (it.id, it.bounds()))
        .collect();
    if items.iter().any(|(_, b)| b.contains(p)) {
        return None;
    }
    // Between left and right neighbours (a vertical gap), or above and below (horizontal).
    let side = |vertical: bool| {
        let (pos, lo, hi) = if vertical { (p.x, page.x0, page.x1) } else { (p.y, page.y0, page.y1) };
        let across = |b: &Rect| if vertical { b.y0 <= p.y && p.y <= b.y1 } else { b.x0 <= p.x && p.x <= b.x1 };
        let end = |b: &Rect| if vertical { b.x1 } else { b.y1 };
        let start = |b: &Rect| if vertical { b.x0 } else { b.y0 };
        let a = items.iter().filter(|(_, b)| across(b) && end(b) <= pos).map(|(_, b)| end(b)).fold(lo, f64::max);
        let z = items.iter().filter(|(_, b)| across(b) && start(b) >= pos).map(|(_, b)| start(b)).fold(hi, f64::min);
        let before: Vec<ItemId> = items.iter().filter(|(_, b)| across(b) && (end(b) - a).abs() < 0.5).map(|(i, _)| *i).collect();
        let after: Vec<ItemId> = items.iter().filter(|(_, b)| across(b) && (start(b) - z).abs() < 0.5).map(|(i, _)| *i).collect();
        (!before.is_empty() || !after.is_empty()).then_some((vertical, a, z, before, after))
    };
    match (side(true), side(false)) {
        (Some(v), Some(h)) => Some(if v.2 - v.1 <= h.2 - h.1 { v } else { h }),
        (v, h) => v.or(h),
    }
}

pub(super) fn gap_move(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "gap.move";
    let at = p
        .get("at")
        .and_then(Value::as_array)
        .and_then(|a| Some(Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
        .ok_or_else(|| bad(ID, "missing at"))?;
    let sr = spread_param(p, "spread");
    let (vertical, a, z, before, after) = find_gap(&s.doc()?.doc, sr, at).ok_or_else(|| bad(ID, "no gap there"))?;
    // `delta` along the gap's axis, or a drag (dx, dy) of which the axis part counts.
    let delta =
        p.get("delta").and_then(Value::as_f64).unwrap_or_else(|| p.get(if vertical { "dx" } else { "dy" }).and_then(Value::as_f64).unwrap_or(0.0));
    let _ = (a, z);
    s.edit(|d, _| {
        let mut n = 0;
        for (ids, is_before) in [(&before, true), (&after, false)] {
            for id in ids {
                let Some(it) = d.item_mut(*id) else { continue };
                let b = it.bounds();
                let mut to = b;
                match (vertical, is_before) {
                    (true, true) => to.x1 = (b.x1 + delta).max(b.x0 + 1.0),
                    (true, false) => to.x0 = (b.x0 + delta).min(b.x1 - 1.0),
                    (false, true) => to.y1 = (b.y1 + delta).max(b.y0 + 1.0),
                    (false, false) => to.y0 = (b.y0 + delta).min(b.y1 - 1.0),
                }
                let m = Affine::translate((to.x0, to.y0))
                    * Affine::scale_non_uniform(to.width() / b.width().max(1e-9), to.height() / b.height().max(1e-9))
                    * Affine::translate((-b.x0, -b.y0));
                if matches!(it.content, Content::Group { .. }) {
                    it.xf = m * it.xf;
                } else {
                    let inner = it.xf.inverse() * m * it.xf;
                    bake(it, inner);
                }
                n += 1;
            }
        }
        Ok(json!({"axis": if vertical { "x" } else { "y" }, "changed": n}))
    })
}
