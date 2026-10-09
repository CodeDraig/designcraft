//! Document grid and text baseline snapping.

use super::{Axis, AxisHit, Best, SnapRequest, ToolContext, axis_flags, offer_enabled, perp_ends, ranges_overlap, snap_to_grid};

/// Document grid. No overlay.
///
/// The interval is the major spacing divided by the subdivision count. A count below 1 counts
/// as 1. `grid.vertical` spaces the vertical lines (x). `grid.horizontal` spaces the horizontal
/// lines (y). A spacing of 0 or less skips that axis. The origin is the page-bounds origin of
/// the page under the rect center. Whether the grid is shown is not read.
pub(super) fn pass_grid(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !cx.snap.snap_to_document_grid {
        return None;
    }
    let spacing = grid_interval(cx, axis)?;
    let origin = grid_origin(cx, req, axis)?;
    let mut best: Option<Best> = None;
    offer_enabled(&mut best, req.rect, axis_flags(req, axis), axis, tol, |v| Some(snap_to_grid(v, spacing, origin)));
    best.map(|b| b.hit)
}

/// Major spacing divided by the subdivision count. Non-finite or non-positive spacing is no hit.
fn grid_interval(cx: &ToolContext, axis: Axis) -> Option<f64> {
    let grid = &cx.doc.settings.grid;
    let major = match axis {
        Axis::X => grid.vertical,
        Axis::Y => grid.horizontal,
    };
    if !major.is_finite() || major <= 0.0 {
        return None;
    }
    let interval = major / f64::from(grid.subdivisions.max(1));
    (interval.is_finite() && interval > 0.0).then_some(interval)
}

/// Page-bounds origin on `axis` for the page under the rect center.
fn grid_origin(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis) -> Option<f64> {
    let spread = cx.doc.spread(req.spread)?;
    let bounds = spread.pages.get(spread.page_at_x(req.rect.center().x)?)?.bounds();
    let origin = match axis {
        Axis::X => bounds.x0,
        Axis::Y => bounds.y0,
    };
    origin.is_finite().then_some(origin)
}

/// Baseline grid. No overlay.
///
/// Lines are `page.y0 + start + n * increment` for n >= 0 while the line is inside the page
/// (the bottom edge is not a line, matching the canvas). An increment of 0 or less skips the
/// pass. `relative_to` and `view_threshold` are not read, and hiding the baseline grid does not
/// turn this off. Only a page the rect crosses on x contributes.
pub(super) fn pass_baseline(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !matches!(axis, Axis::Y) || !cx.snap.snap_to_guides {
        return None;
    }
    let baseline = &cx.doc.settings.baseline_grid;
    let increment = baseline.increment;
    if !increment.is_finite() || increment <= 0.0 || !baseline.start.is_finite() {
        return None;
    }
    let spread = cx.doc.spread(req.spread)?;
    let flags = axis_flags(req, axis);
    let (moving_lo, moving_hi) = perp_ends(req.rect, axis);
    let mut best: Option<Best> = None;
    for page in &spread.pages {
        let bounds = page.bounds();
        let (lo, hi) = perp_ends(bounds, axis);
        if !ranges_overlap(moving_lo, moving_hi, lo, hi) {
            continue;
        }
        let origin = bounds.y0 + baseline.start;
        offer_enabled(&mut best, req.rect, flags, axis, tol, |v| nearest_baseline(v, origin, increment, bounds.y0, bounds.y1));
    }
    best.map(|b| b.hit)
}

/// Nearest baseline at or after `origin`, strictly inside `[y0, y1)`.
fn nearest_baseline(v: f64, origin: f64, increment: f64, y0: f64, y1: f64) -> Option<f64> {
    let (n_min, n_max) = baseline_n_range(origin, increment, y0, y1)?;
    let snapped = snap_to_grid(v, increment, origin);
    if baseline_line_ok(snapped, origin, increment, y0, y1) {
        return Some(snapped);
    }
    let n = ((snapped - origin) / increment).round();
    if !n.is_finite() {
        return None;
    }
    let n = if n < n_min {
        n_min
    } else if n > n_max {
        n_max
    } else {
        n
    };
    let line = origin + n * increment;
    baseline_line_ok(line, origin, increment, y0, y1).then_some(line)
}

fn baseline_n_range(origin: f64, increment: f64, y0: f64, y1: f64) -> Option<(f64, f64)> {
    if !origin.is_finite() || !increment.is_finite() || increment <= 0.0 || !y0.is_finite() || !y1.is_finite() || y0 >= y1 {
        return None;
    }
    let mut n_min = if origin < y0 { ((y0 - origin) / increment).ceil() } else { 0.0 };
    if !n_min.is_finite() || n_min < 0.0 {
        return None;
    }
    if origin + n_min * increment < y0 {
        n_min += 1.0;
    }
    let mut n_max = ((y1 - origin) / increment).floor();
    if !n_max.is_finite() {
        return None;
    }
    if origin + n_max * increment >= y1 {
        n_max -= 1.0;
    }
    (n_max >= n_min).then_some((n_min, n_max))
}

fn baseline_line_ok(line: f64, origin: f64, increment: f64, y0: f64, y1: f64) -> bool {
    if !line.is_finite() || !increment.is_finite() || increment <= 0.0 || line < y0 || line >= y1 {
        return false;
    }
    let n = ((line - origin) / increment).round();
    n.is_finite() && n >= 0.0
}
