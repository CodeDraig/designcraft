//! Stacking, grouping, alignment, and distribution.

use super::{Affine, Arc, Content, Document, Item, ItemId, Rect, Result, Selection, Session, Shape, Value, Vec2, bad, json, ok, str_param, targets};

pub(super) fn arrange(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let to = str_param(p, "to").unwrap_or("front").to_string();
    s.edit(|d, _| {
        for id in &ids {
            let Some(loc) = d.find(*id) else { continue };
            if loc.path.len() != 1 {
                continue;
            }
            let Some(sp) = d.spread_mut(loc.spread) else { continue };
            let i = loc.path[0];
            let n = sp.items.len();
            let it = sp.items.remove(i);
            let j = match to.as_str() {
                "front" => n - 1,
                "back" => 0,
                "forward" => (i + 1).min(n - 1),
                _ => i.saturating_sub(1),
            };
            sp.items.insert(j, it);
        }
        ok()
    })
}

pub(super) fn group(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    if ids.len() < 2 {
        return Err(bad("object.group", "select at least two objects"));
    }
    s.edit(|d, sel| {
        let sr = d.find(ids[0]).map(|l| l.spread).ok_or(designcraft_doc::DocError::NoItem(ids[0]))?;
        // Keep stacking order: collect in z-order.
        let order: Vec<ItemId> = d.spread(sr).map(|sp| sp.items.iter().map(|i| i.id).filter(|i| ids.contains(i)).collect()).unwrap_or_default();
        let front = d.spread(sr).and_then(|sp| sp.items.iter().position(|i| Some(&i.id) == order.last())).unwrap_or(0);
        let layer = d.item(order[0]).map(|i| i.layer).unwrap_or_default();
        let mut kids = Vec::new();
        for id in &order {
            kids.push(d.remove_item_keep_story(*id)?);
        }
        let gid = ItemId(d.alloc());
        let mut g = Item::new(gid, layer, Shape::Group, designcraft_geom::PathData::default());
        g.content = Content::Group { items: kids.into_iter().map(Arc::new).collect() };
        let at = front + 1 - order.len();
        d.insert_item(sr, g, Some(at))?;
        *sel = Selection::items(vec![gid]);
        Ok(json!({"id": gid.0}))
    })
}

pub(super) fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, sel| {
        let mut out = Vec::new();
        for id in &ids {
            let Some(loc) = d.find(*id) else { continue };
            let Some(g) = d.item(*id).cloned() else { continue };
            let Content::Group { items } = &g.content else { continue };
            if loc.path.len() != 1 {
                continue;
            }
            let Some(sp) = d.spread_mut(loc.spread) else { continue };
            sp.items.remove(loc.path[0]);
            for (k, c) in items.iter().enumerate() {
                let mut c = (**c).clone();
                c.xf = g.xf * c.xf;
                out.push(c.id);
                sp.items.insert(loc.path[0] + k, Arc::new(c));
            }
        }
        *sel = Selection::items(out);
        ok()
    })
}

pub(super) fn set_flag(s: &mut Session, p: &Value, f: impl Fn(&mut Item), deselect: bool) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, sel| {
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                f(it);
            }
        }
        if deselect {
            *sel = Selection::default();
        }
        Ok(json!({"changed": ids.len()}))
    })
}

pub(super) fn all_flag(s: &mut Session, f: impl Fn(&mut Item)) -> Result<Value> {
    s.edit(|d, _| {
        for i in 0..d.spreads.len() {
            let sp = Arc::make_mut(&mut d.spreads[i]);
            for it in &mut sp.items {
                f(Arc::make_mut(it));
            }
        }
        ok()
    })
}

pub(crate) trait RemoveKeep {
    fn remove_item_keep_story(&mut self, id: ItemId) -> Result<Item>;
}

/// Detach an item without touching story threads (moving between spreads, grouping).
impl RemoveKeep for Document {
    fn remove_item_keep_story(&mut self, id: ItemId) -> Result<Item> {
        let loc = self.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        if loc.path.len() != 1 {
            return Err(bad("object", "nested items can't be detached"));
        }
        let sp = self.spread_mut(loc.spread).ok_or(designcraft_doc::DocError::NoItem(id))?;
        Ok(Arc::unwrap_or_clone(sp.items.remove(loc.path[0])))
    }
}

/// Bounds of each target item (spread space) and the alignment reference rect.
fn align_reference(d: &Document, ids: &[ItemId], to: &str, key: Option<ItemId>) -> Option<Rect> {
    let bounds: Vec<Rect> = ids.iter().filter_map(|i| d.item(*i).map(Item::bounds)).collect();
    let union = bounds.iter().copied().reduce(|a, b| a.union(b))?;
    let first = ids.first().and_then(|i| d.find(*i))?;
    let sp = d.spread(first.spread)?;
    let page = sp.page_at_x(union.center().x).and_then(|pi| sp.pages.get(pi));
    Some(match to {
        "keyObject" => key.and_then(|k| d.item(k)).map(Item::bounds).unwrap_or(union),
        "margins" => page.map(|p| p.margin_rect()).unwrap_or(union),
        "page" => page.map(|p| p.bounds()).unwrap_or(union),
        "spread" => sp.bounds(),
        _ => union,
    })
}

pub(super) fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let edge = str_param(p, "edge").unwrap_or("left").to_string();
    let to = str_param(p, "to").map(str::to_string).unwrap_or_else(|| if ids.len() > 1 { "selection".into() } else { "page".into() });
    let key = s.doc()?.selection.key;
    s.edit(|d, _| {
        let r = align_reference(d, &ids, &to, key).ok_or_else(|| bad("object.align", "nothing to align"))?;
        for id in &ids {
            if Some(*id) == key && to == "keyObject" {
                continue;
            }
            let Some(it) = d.item_mut(*id) else { continue };
            if it.locked {
                continue;
            }
            let b = it.bounds();
            let (dx, dy) = match edge.as_str() {
                "left" => (r.x0 - b.x0, 0.0),
                "hcenter" => (r.center().x - b.center().x, 0.0),
                "right" => (r.x1 - b.x1, 0.0),
                "top" => (0.0, r.y0 - b.y0),
                "vcenter" => (0.0, r.center().y - b.center().y),
                "bottom" => (0.0, r.y1 - b.y1),
                other => return Err(bad("object.align", format!("unknown edge `{other}`"))),
            };
            it.xf = Affine::translate((dx, dy)) * it.xf;
        }
        ok()
    })
}

pub(super) fn distribute(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    if ids.len() < 3 && p.get("spacing").is_none() {
        return Err(bad("object.distribute", "select at least three objects"));
    }
    let horizontal = str_param(p, "axis") != Some("vertical");
    let by_spacing = str_param(p, "by") == Some("spacing") || p.get("spacing").is_some();
    let fixed = p.get("spacing").and_then(Value::as_f64);
    s.edit(|d, _| {
        let mut items: Vec<(ItemId, Rect)> = ids.iter().filter_map(|i| d.item(*i).map(|it| (*i, it.bounds()))).collect();
        let key = |r: &Rect| if horizontal { r.x0 } else { r.y0 };
        items.sort_by(|a, b| key(&a.1).total_cmp(&key(&b.1)));
        let n = items.len();
        if n < 2 {
            return ok();
        }
        let (first, last) = (items[0].1, items[n - 1].1);
        let lo = |r: &Rect| if horizontal { r.x0 } else { r.y0 };
        let hi = |r: &Rect| if horizontal { r.x1 } else { r.y1 };
        let size = |r: &Rect| hi(r) - lo(r);
        let mut targets: Vec<f64> = Vec::with_capacity(n);
        if by_spacing {
            let total: f64 = items.iter().map(|(_, r)| size(r)).sum();
            let gap = fixed.unwrap_or_else(|| (hi(&last) - lo(&first) - total) / (n - 1) as f64);
            let mut pos = lo(&first);
            for (_, r) in &items {
                targets.push(pos);
                pos += size(r) + gap;
            }
        } else {
            let c0 = (lo(&first) + hi(&first)) / 2.0;
            let c1 = (lo(&last) + hi(&last)) / 2.0;
            for (k, (_, r)) in items.iter().enumerate() {
                targets.push(c0 + (c1 - c0) * k as f64 / (n - 1) as f64 - size(r) / 2.0);
            }
        }
        for ((id, r), t) in items.iter().zip(targets) {
            let delta = t - lo(r);
            if let Some(it) = d.item_mut(*id) {
                let v = if horizontal { Vec2::new(delta, 0.0) } else { Vec2::new(0.0, delta) };
                it.xf = Affine::translate(v) * it.xf;
            }
        }
        ok()
    })
}

pub(super) fn object_set_flags(s: &mut Session, p: &Value) -> Result<Value> {
    let hidden = p.get("hidden").and_then(Value::as_bool);
    let locked = p.get("locked").and_then(Value::as_bool);
    set_flag(
        s,
        p,
        move |i| {
            if let Some(h) = hidden {
                i.hidden = h;
            }
            if let Some(l) = locked {
                i.locked = l;
            }
        },
        hidden == Some(true) || locked == Some(true),
    )
}
