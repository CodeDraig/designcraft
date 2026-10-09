//! Graphic clipping paths and frame fitting.

use super::{Content, ItemId, Point, Rect, Result, Session, Shape, Value, bad, json, ok, shapes, str_param, targets};

/// Clipping Path: trace the graphic (its alpha channel, or edges against a white background) and
/// give the frame that outline.
pub(super) fn clipping_path(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.clippingPath";
    let ids = targets(s, p)?;
    let edges = match str_param(p, "type").unwrap_or("alpha") {
        "alpha" => false,
        "edges" => true,
        t => return Err(bad(ID, format!("unknown type `{t}` (alpha, edges)"))),
    };
    let threshold = p.get("threshold").and_then(Value::as_f64).unwrap_or(25.0).clamp(0.0, 255.0);
    let tol = p.get("tolerance").and_then(Value::as_f64).unwrap_or(2.0).max(0.1);
    // Trace each graphic (outside the edit: decoding is the slow part).
    let mut paths: Vec<(ItemId, designcraft_geom::PathData)> = Vec::new();
    {
        let d = &s.doc()?.doc;
        for id in &ids {
            let Some(it) = d.item(*id) else { continue };
            let Some(g) = it.graphic() else { continue };
            let Some(asset) = d.assets.get(&g.asset) else { continue };
            let Some(px) = designcraft_render::decode_pixmap_page(&asset.data, asset.page) else { continue };
            let (pw, ph) = (px.width() as usize, px.height() as usize);
            // A grid of at most 400 cells on the long side.
            let step = (pw.max(ph) as f64 / 400.0).max(1.0);
            let (w, h) = (((pw as f64) / step).ceil() as usize, ((ph as f64) / step).ceil() as usize);
            let mask: Vec<bool> = (0..w * h)
                .map(|i| {
                    let (x, y) = (((i % w) as f64 * step) as u16, ((i / w) as f64 * step) as u16);
                    let c = px.sample(x.min(pw as u16 - 1), y.min(ph as u16 - 1));
                    if edges {
                        // Un-premultiply over white: anything darker than the threshold is inside.
                        let a = c.a as f64 / 255.0;
                        let lum = (0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64) + 255.0 * (1.0 - a);
                        lum < 255.0 - threshold
                    } else {
                        c.a as f64 > threshold
                    }
                })
                .collect();
            // The tolerance is in image pixels; the grid cells are `step` pixels.
            let rings = designcraft_geom::trace::trace(&mask, w, h, tol / step, 4.0);
            if rings.is_empty() {
                return Err(bad(ID, "nothing to trace (the image is empty at this threshold)"));
            }
            let (sx, sy) = (g.size.0 / w as f64, g.size.1 / h as f64);
            let subpaths = rings
                .into_iter()
                .map(|r| {
                    let pts: Vec<Point> = r.into_iter().map(|q| g.xf * Point::new(q.x * sx, q.y * sy)).collect();
                    designcraft_geom::path::SubPath::polyline(&pts, true)
                })
                .collect();
            paths.push((*id, designcraft_geom::PathData::new(subpaths)));
        }
    }
    if paths.is_empty() {
        return Err(bad(ID, "select a frame with a placed graphic"));
    }
    s.edit(|d, _| {
        let n = paths.len();
        for (id, path) in paths {
            if let Some(it) = d.item_mut(id) {
                it.path = path;
                it.shape = Shape::Path;
                it.corners = Default::default();
                if let Content::Graphic(g) = &mut it.content {
                    g.auto_fit = designcraft_doc::Fitting::None;
                }
            }
        }
        Ok(json!({"changed": n}))
    })
}

fn fitting_mode(s: &str) -> Option<designcraft_doc::Fitting> {
    use designcraft_doc::Fitting as F;
    Some(match s {
        "none" => F::None,
        "fillProportionally" => F::FillProportionally,
        "fitProportionally" => F::FitProportionally,
        "fitContentToFrame" => F::FitContentToFrame,
        "centerContent" => F::CenterContent,
        _ => return None,
    })
}

/// Frame Fitting Options: auto-fit, fitting, align from, crop amounts.
pub(super) fn fitting_options(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.fittingOptions";
    let ids = targets(s, p)?;
    let fitting = match str_param(p, "fitting") {
        Some(f) => Some(fitting_mode(f).ok_or_else(|| bad(ID, format!("unknown fitting `{f}`")))?),
        None => None,
    };
    let auto = p.get("autoFit").and_then(Value::as_bool);
    let align = p.get("align").and_then(Value::as_u64).map(|v| v.min(8) as u8);
    let crop = match p.get("crop") {
        Some(Value::Array(a)) if a.len() == 4 => Some([0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(0.0))),
        Some(Value::Number(n)) => Some([n.as_f64().unwrap_or(0.0); 4]),
        Some(v) => return Err(bad(ID, format!("bad `crop`: {v} (n or [t, l, b, r])"))),
        None => None,
    };
    s.edit(|d, _| {
        let mut n = 0;
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            let r = it.inner_bounds();
            let Content::Graphic(g) = &mut it.content else { continue };
            if let Some(a) = align {
                g.fit_align = a;
            }
            if let Some(c) = crop {
                g.crop = c;
            }
            let mode = fitting.unwrap_or(g.auto_fit);
            if let Some(a) = auto {
                g.auto_fit = if a {
                    if mode == designcraft_doc::Fitting::None { designcraft_doc::Fitting::FillProportionally } else { mode }
                } else {
                    designcraft_doc::Fitting::None
                };
            } else if fitting.is_some() && g.auto_fit != designcraft_doc::Fitting::None {
                g.auto_fit = mode;
            }
            if let Some(xf) = g.fitted(r, mode) {
                g.xf = xf;
            }
            n += 1;
        }
        Ok(json!({"changed": n}))
    })
}

pub(super) fn fit(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let mode = str_param(p, "mode").unwrap_or("fitProportionally").to_string();
    s.edit(|d, _| {
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            let r = it.inner_bounds();
            let Content::Graphic(g) = &mut it.content else { continue };
            let (nw, nh) = g.size;
            let cur = g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, nw, nh));
            if mode == "fitFrameToContent" {
                it.path = shapes::rectangle(cur);
                continue;
            }
            let m = fitting_mode(&mode)
                .filter(|m| *m != designcraft_doc::Fitting::None)
                .ok_or_else(|| bad("object.fit", format!("unknown mode `{mode}`")))?;
            if let Some(xf) = g.fitted(r, m) {
                g.xf = xf;
            }
        }
        ok()
    })
}
