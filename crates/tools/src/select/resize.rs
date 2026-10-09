//! Selection-handle resize geometry and modifier/snap constraints.

use super::{Mods, Point, Rect, ToolContext};

/// New rect when dragging `handle` of `from` to `p`.
pub fn resize_rect(from: Rect, handle: usize, p: Point, m: Mods) -> Rect {
    finish_resize(from, handle, resize_edges_rect(from, handle, p), m)
}

/// Edges the pointer sets, before Shift or Alt.
pub(super) fn resize_edges_rect(from: Rect, handle: usize, p: Point) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (from.x0, from.y0, from.x1, from.y1);
    match handle {
        0 => {
            x0 = p.x;
            y0 = p.y;
        }
        1 => y0 = p.y,
        2 => {
            x1 = p.x;
            y0 = p.y;
        }
        3 => x1 = p.x,
        4 => {
            x1 = p.x;
            y1 = p.y;
        }
        5 => y1 = p.y,
        6 => {
            x0 = p.x;
            y1 = p.y;
        }
        _ => x0 = p.x,
    }
    Rect::new(x0, y0, x1, y1)
}

fn finish_resize(from: Rect, handle: usize, rect: Rect, m: Mods) -> Rect {
    let rect = if m.shift && from.width() > 1e-9 && from.height() > 1e-9 {
        apply_aspect(from, handle, rect, aspect_scale(from, handle, rect))
    } else {
        rect
    };
    if m.alt { apply_alt(from, handle, rect) } else { rect }
}

fn aspect_scale(from: Rect, handle: usize, rect: Rect) -> f64 {
    let sx = (rect.x1 - rect.x0) / from.width();
    let sy = (rect.y1 - rect.y0) / from.height();
    if matches!(handle, 1 | 5) {
        sy
    } else if matches!(handle, 3 | 7) || sx.abs() > sy.abs() {
        sx
    } else {
        sy
    }
}

/// True when the width, not the height, drives a Shift constraint.
pub(super) fn drives_x(handle: usize, from: Rect, rect: Rect) -> bool {
    if matches!(handle, 1 | 5) {
        return false;
    }
    if matches!(handle, 3 | 7) {
        return true;
    }
    if from.width() <= 1e-9 || from.height() <= 1e-9 {
        return true;
    }
    let sx = ((rect.x1 - rect.x0) / from.width()).abs();
    let sy = ((rect.y1 - rect.y0) / from.height()).abs();
    sx > sy
}

fn apply_aspect(from: Rect, handle: usize, rect: Rect, s: f64) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (rect.x0, rect.y0, rect.x1, rect.y1);
    let (w, h) = (from.width() * s, from.height() * s);
    match handle {
        0 => {
            x0 = x1 - w;
            y0 = y1 - h;
        }
        2 => {
            x1 = x0 + w;
            y0 = y1 - h;
        }
        3..=5 => {
            x1 = x0 + w;
            y1 = y0 + h;
        }
        6 | 7 => {
            x0 = x1 - w;
            y1 = y0 + h;
        }
        _ => {
            x0 = x1 - w;
            y0 = y1 - h;
        }
    }
    Rect::new(x0, y0, x1, y1)
}

/// Snap changes the size. Shift then restores the aspect, and Alt places that size on the original center.
///
/// Nudging one edge of an already centered rect walks the center. Mirroring that nudge doubles a length match.
pub(super) fn finish_snapped_resize(
    from: Rect,
    handle: usize,
    proposed: Rect,
    mods: Mods,
    driver_x: bool,
    x_edges: [bool; 3],
    y_edges: [bool; 3],
    hit: &crate::Snap,
) -> Rect {
    if !mods.alt {
        let mut to = crate::snap::nudge_edges(proposed, x_edges, y_edges, hit.delta);
        if mods.shift && from.width() > 1e-9 && from.height() > 1e-9 {
            let s = if driver_x { (to.x1 - to.x0) / from.width() } else { (to.y1 - to.y0) / from.height() };
            if s.is_finite() {
                to = apply_aspect(from, handle, to, s);
            }
        }
        return to;
    }
    let mut w = proposed.x1 - proposed.x0;
    let mut h = proposed.y1 - proposed.y0;
    w += centered_size_delta(x_edges, hit.delta.x, hit.length_delta[0]);
    h += centered_size_delta(y_edges, hit.delta.y, hit.length_delta[1]);
    if mods.shift && from.width() > 1e-9 && from.height() > 1e-9 {
        if driver_x {
            let s = w / from.width();
            if s.is_finite() {
                h = from.height() * s;
            }
        } else {
            let s = h / from.height();
            if s.is_finite() {
                w = from.width() * s;
            }
        }
    }
    if !(w.is_finite() && h.is_finite()) {
        return proposed;
    }
    let c = from.center();
    Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0)
}

/// Length match: add the difference once. Position match: the flagged edge moves and the other side mirrors.
fn centered_size_delta(edges: [bool; 3], delta: f64, length_delta: Option<f64>) -> f64 {
    if let Some(diff) = length_delta.filter(|d| d.is_finite()) {
        return diff;
    }
    let edge = axis_size_delta(edges, delta);
    if edge.is_finite() { edge * 2.0 } else { 0.0 }
}

/// Signed change in `hi - lo` when one flagged edge moves by `delta`.
fn axis_size_delta(edges: [bool; 3], delta: f64) -> f64 {
    if !delta.is_finite() {
        return 0.0;
    }
    let [low_edge, _, high_edge] = edges;
    if high_edge && !low_edge {
        delta
    } else if low_edge && !high_edge {
        -delta
    } else {
        0.0
    }
}

fn apply_alt(from: Rect, handle: usize, rect: Rect) -> Rect {
    let (x0, y0, x1, y1) = (rect.x0, rect.y0, rect.x1, rect.y1);
    let c = from.center();
    let (hw, hh) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let (hw, hh) = match handle {
        1 | 5 => (from.width() / 2.0, (if handle == 1 { c.y - y0 } else { y1 - c.y })),
        3 | 7 => ((if handle == 7 { c.x - x0 } else { x1 - c.x }), from.height() / 2.0),
        _ => (hw.abs().max((c.x - x0).abs()).max((x1 - c.x).abs()), hh.abs().max((c.y - y0).abs()).max((y1 - c.y).abs())),
    };
    Rect::new(c.x - hw, c.y - hh, c.x + hw, c.y + hh)
}

/// Left/center/right and top/center/bottom for a handle. Centers stay off.
pub(super) fn resize_edge_flags(handle: usize) -> ([bool; 3], [bool; 3]) {
    let x = match handle {
        0 | 6 | 7 => [true, false, false],
        2..=4 => [false, false, true],
        _ => [false, false, false],
    };
    let y = match handle {
        0..=2 => [true, false, false],
        4..=6 => [false, false, true],
        _ => [false, false, false],
    };
    (x, y)
}

pub(super) fn axis_length(edges: [bool; 3], size: f64, pad: f64) -> Option<f64> {
    if edges == [false, false, false] {
        return None;
    }
    let len = size.abs() + pad;
    len.is_finite().then_some(len)
}

/// Visible size minus path size, for one selected item. Centered stroke then matches the outer edge.
pub(super) fn single_stroke_pad(cx: &ToolContext) -> (f64, f64) {
    if cx.selection.items.len() != 1 {
        return (0.0, 0.0);
    }
    let Some(id) = cx.selection.items.first() else { return (0.0, 0.0) };
    let Some(it) = cx.doc.item(*id) else { return (0.0, 0.0) };
    let path = it.bounds();
    let vis = it.visible_bounds();
    (positive_pad(vis.width() - path.width()), positive_pad(vis.height() - path.height()))
}

fn positive_pad(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}
