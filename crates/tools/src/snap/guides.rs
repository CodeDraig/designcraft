//! Guide, margin, and column snapping.

use super::{Axis, AxisHit, Best, SnapRequest, ToolContext, axis_flags, axis_orientation, offer_line, perp_ends, ranges_overlap};

/// Ruler guides, margins, and column sides. No overlay.
///
/// A page guide attracts when the moving rect overlaps that page on the other axis. A spread
/// guide uses the spread bounds. Locked guides are included. A guide is a target only when
/// `Guide::visible_in` accepts it.
pub(super) fn pass_guides(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !cx.snap.snap_to_guides || !cx.snap.show_guides {
        return None;
    }
    let sp = cx.doc.spread(req.spread)?;
    let spread_bounds = sp.bounds();
    let flags = axis_flags(req, axis);
    let (moving_lo, moving_hi) = perp_ends(req.rect, axis);
    let mut best: Option<Best> = None;
    for page in &sp.pages {
        let page_bounds = page.bounds();
        for guide in &page.guides {
            if !guide.visible_in(cx.doc) || !guide.position.is_finite() || guide.orientation != axis_orientation(axis) {
                continue;
            }
            let range = if guide.spread { spread_bounds } else { page_bounds };
            let (range_lo, range_hi) = perp_ends(range, axis);
            if !ranges_overlap(moving_lo, moving_hi, range_lo, range_hi) {
                continue;
            }
            offer_line(&mut best, req.rect, flags, axis, guide.position, tol);
        }
        let margin = page.margin_rect();
        let (a, b) = match axis {
            Axis::X => (margin.x0, margin.x1),
            Axis::Y => (margin.y0, margin.y1),
        };
        offer_line(&mut best, req.rect, flags, axis, a, tol);
        offer_line(&mut best, req.rect, flags, axis, b, tol);
        if matches!(axis, Axis::X) {
            for column in page.column_rects() {
                offer_line(&mut best, req.rect, flags, axis, column.x0, tol);
                offer_line(&mut best, req.rect, flags, axis, column.x1, tol);
            }
        }
    }
    best.map(|b| b.hit)
}
