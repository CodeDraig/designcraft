//! Frame and line creation, including polygon geometry.

use super::{
    Affine, Item, ItemId, ParaFormat, Point, Rect, Result, Selection, Session, Shape, Stroke, TextSel, Value, bad, bool_or, json, point_param,
    rect_param, shapes, spread_param, str_param,
};

pub(super) fn frame_create(s: &mut Session, p: &Value) -> Result<Value> {
    let rect = rect_param(p, "rect").ok_or_else(|| bad("frame.create", "missing rect"))?;
    let shape = str_param(p, "shape").unwrap_or("rectangle");
    let content = str_param(p, "content").unwrap_or("graphic");
    let sr = spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    let text = super::super::text_param(p, "text");
    let caret = bool_or(p, "caret", content == "text");
    let vertical = bool_or(p, "vertical", false);
    let sides = p.get("sides").and_then(Value::as_u64).map_or(s.prefs.polygon_sides, |v| v as u32).clamp(3, 100);
    let inset = p.get("starInset").and_then(Value::as_f64).map_or(s.prefs.star_inset, |v| v / 100.0).clamp(0.0, 1.0);
    let rect = Rect::new(rect.x0, rect.y0, rect.x1.max(rect.x0 + 0.5), rect.y1.max(rect.y0 + 0.5));
    s.edit(|d, sel| {
        if d.spread(sr).is_none() {
            return Err(bad("frame.create", "no such spread"));
        }
        let (path, sh) = match shape {
            "ellipse" | "oval" => (shapes::ellipse(rect), Shape::Oval),
            "polygon" => (polygon_in(rect, sides, inset), Shape::Polygon),
            _ => (shapes::rectangle(rect), Shape::Rectangle),
        };
        if content == "text" {
            let (id, sid) = d.add_text_frame(sr, rect, lid, &text, ParaFormat { style: d.styles.default_paragraph.clone(), ..Default::default() })?;
            if let Some(it) = d.item_mut(id) {
                it.path = path;
                it.shape = sh;
            }
            super::super::text::set_story_direction(d, &[sid], vertical);
            *sel = if caret {
                Selection::text(TextSel { story: sid, anchor: text.len(), focus: text.len(), frame: Some(id), cell: None })
            } else {
                Selection::items(vec![id])
            };
            return Ok(json!({"id": id.0, "story": sid.0}));
        }
        let id = ItemId(d.alloc());
        let mut it = Item::new(id, lid, sh, path);
        if content == "graphic" {
            it.object_style = d.styles.default_graphic_frame.clone();
        } else {
            // Shapes drawn with Rectangle/Ellipse/Polygon get the default 1 pt black stroke.
            it.stroke = Stroke::default();
        }
        d.insert_item(sr, it, None)?;
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

/// A polygon (or, with a star inset, a star with `sides` points) fitted to `r`.
pub(crate) fn polygon_in(r: Rect, sides: u32, inset: f64) -> designcraft_geom::PathData {
    let c = r.center();
    let mut p =
        if inset > 0.0 { shapes::star(Point::ZERO, 1.0, (1.0 - inset).max(0.0), sides, 0.0) } else { shapes::polygon(Point::ZERO, 1.0, sides, 0.0) };
    // Fit the unit polygon's bounds to the rect.
    if let Some(b) = p.bounds() {
        let a = Affine::translate(c.to_vec2())
            * Affine::scale_non_uniform(r.width() / b.width().max(1e-9), r.height() / b.height().max(1e-9))
            * Affine::translate(-b.center().to_vec2());
        p.transform(a);
    }
    p
}

pub(super) fn line_create(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_param(p, "a").ok_or_else(|| bad("line.create", "missing a"))?;
    let b = point_param(p, "b").ok_or_else(|| bad("line.create", "missing b"))?;
    let sr = spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let id = ItemId(d.alloc());
        let mut it = Item::new(id, lid, Shape::GraphicLine, shapes::line(a, b));
        it.stroke = Stroke::default();
        d.insert_item(sr, it, None)?;
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

pub(super) fn tool_polygon_settings(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(n) = p.get("sides").and_then(Value::as_u64) {
        s.prefs.polygon_sides = (n as u32).clamp(3, 100);
    }
    if let Some(v) = p.get("starInset").and_then(Value::as_f64) {
        s.prefs.star_inset = (v / 100.0).clamp(0.0, 1.0);
    }
    Ok(json!({"sides": s.prefs.polygon_sides, "starInset": (s.prefs.star_inset * 100.0).round()}))
}
