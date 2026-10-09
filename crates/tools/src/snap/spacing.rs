//! Equal-gap matching and gap measure overlays.

use super::{
    Axis, AxisHit, GapMark, Gesture, Point, Rect, SnapRequest, ToolContext, format_measure, item_target_rect, point_finite, ranges_overlap,
    rect_finite, segment_same,
};

/// Equal gaps on a move, when smart guides and smart spacing are on.
///
/// Boxes are the top-level visible bounds on this spread. A group contributes the union of
/// its children. Parent items are not targets. `req.rect` is the moving box and is not also a
/// stationary box. A box counts only when its range overlaps on the other axis. The nearest
/// box on a side that overlaps the mover, or a zero gap, produces no gap, and a farther box
/// is not used instead. A candidate shifts the mover so one of its gaps equals one stationary
/// nearest-neighbor gap. The shift has to fall inside the zone. The closest shift wins.
/// One gap overlay is drawn for every gap of that length, including the moving gap after the
/// shift. Create, resize, rotate, and point do not match.
pub(super) fn pass_spacing(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !matches!(req.gesture, Gesture::Move) || !cx.snap.smart_guides || !cx.snap.smart_spacing {
        return None;
    }
    if !tol.is_finite() || tol <= 0.0 {
        return None;
    }
    let mover = interval_on(req.rect, axis)?;
    let boxes = spacing_intervals(cx, req, axis, &mover);
    let stationary = stationary_gaps(&boxes);
    if stationary.is_empty() {
        return None;
    }
    let mut best: Option<SpacingChoice> = None;
    if let Some(n) = nearest_side(&mover, &boxes, None, true) {
        for g in &stationary {
            offer_spacing(&mut best, g.gap - n.gap, g.gap, tol);
        }
    }
    if let Some(n) = nearest_side(&mover, &boxes, None, false) {
        for g in &stationary {
            offer_spacing(&mut best, n.gap - g.gap, g.gap, tol);
        }
    }
    let choice = best?;
    let shifted = Interval { lo: mover.lo + choice.delta, hi: mover.hi + choice.delta, perp_lo: mover.perp_lo, perp_hi: mover.perp_hi };
    if !shifted.lo.is_finite() || !shifted.hi.is_finite() {
        return None;
    }
    let label = format_measure(choice.distance, cx.unit);
    let mut gaps = Vec::new();
    push_moving_gaps(&mut gaps, &shifted, &boxes, axis, choice.distance, &label);
    for g in &stationary {
        if gap_matches(g.gap, choice.distance) {
            push_gap_mark(&mut gaps, axis, g.left, g.right, g.perp_lo, g.perp_hi, &label);
        }
    }
    Some(AxisHit { delta: choice.delta, at: 0.0, span: None, length_delta: None, dimension: None, gaps })
}

/// Top-level visible boxes that overlap `mover` on the other axis.
///
/// The moving rect is not one of them. Parents are not walked.
fn spacing_intervals(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, mover: &Interval) -> Vec<Interval> {
    let mut out = Vec::new();
    let Some(sp) = cx.doc.spread(req.spread) else { return out };
    for item in &sp.items {
        let Some(rect) = item_target_rect(cx.selection, req, item) else { continue };
        let Some(span) = interval_on(rect, axis) else { continue };
        if ranges_overlap(mover.perp_lo, mover.perp_hi, span.perp_lo, span.perp_hi) {
            out.push(span);
        }
    }
    out
}

struct Interval {
    lo: f64,
    hi: f64,
    perp_lo: f64,
    perp_hi: f64,
}

struct SideGap {
    gap: f64,
    edge: f64,
    perp_lo: f64,
    perp_hi: f64,
}

struct StoredGap {
    gap: f64,
    left: f64,
    right: f64,
    perp_lo: f64,
    perp_hi: f64,
}

struct SpacingChoice {
    dist: f64,
    delta: f64,
    distance: f64,
}

fn interval_on(rect: Rect, axis: Axis) -> Option<Interval> {
    if !rect_finite(rect) {
        return None;
    }
    let (a0, a1, p0, p1) = match axis {
        Axis::X => (rect.x0, rect.x1, rect.y0, rect.y1),
        Axis::Y => (rect.y0, rect.y1, rect.x0, rect.x1),
    };
    Some(Interval { lo: a0.min(a1), hi: a0.max(a1), perp_lo: p0.min(p1), perp_hi: p0.max(p1) })
}

/// Nearest positive gap on one side of `mover`.
///
/// `low` looks toward decreasing coordinates. A box that overlaps that edge, or a zero gap,
/// is nearer than any open gap, so the side produces nothing and a farther box is not used.
/// `skip` drops a box that would otherwise be compared with itself.
fn nearest_side(mover: &Interval, boxes: &[Interval], skip: Option<usize>, low: bool) -> Option<SideGap> {
    let mut blocked = false;
    let mut best: Option<SideGap> = None;
    for (i, b) in boxes.iter().enumerate() {
        if skip == Some(i) || !ranges_overlap(mover.perp_lo, mover.perp_hi, b.perp_lo, b.perp_hi) {
            continue;
        }
        match side_gap(mover, b, low) {
            SideKind::Block => blocked = true,
            SideKind::Ignore => {}
            SideKind::Gap { gap, edge } => {
                if best.as_ref().is_none_or(|have| gap < have.gap) {
                    best = Some(SideGap { gap, edge, perp_lo: b.perp_lo, perp_hi: b.perp_hi });
                }
            }
        }
    }
    if blocked { None } else { best }
}

enum SideKind {
    Block,
    Ignore,
    Gap { gap: f64, edge: f64 },
}

fn side_gap(mover: &Interval, b: &Interval, low: bool) -> SideKind {
    let (gap, edge, crosses, past) = if low {
        (mover.lo - b.hi, b.hi, b.lo < mover.lo && b.hi > mover.lo, b.hi > mover.lo)
    } else {
        (b.lo - mover.hi, b.lo, b.lo < mover.hi && b.hi > mover.hi, b.lo < mover.hi)
    };
    if crosses {
        return SideKind::Block;
    }
    if past {
        return SideKind::Ignore;
    }
    if !gap.is_finite() {
        return SideKind::Ignore;
    }
    if gap <= 0.0 { SideKind::Block } else { SideKind::Gap { gap, edge } }
}

/// Nearest-neighbor gaps between stationary boxes that overlap on the other axis.
///
/// Only the high side of each box is recorded, so a pair is one gap. An overlap or a zero
/// gap on that side contributes nothing.
fn stationary_gaps(boxes: &[Interval]) -> Vec<StoredGap> {
    let mut out = Vec::new();
    for (i, a) in boxes.iter().enumerate() {
        let Some(n) = nearest_side(a, boxes, Some(i), false) else { continue };
        let Some((perp_lo, perp_hi)) = perp_span(a.perp_lo, a.perp_hi, n.perp_lo, n.perp_hi) else { continue };
        out.push(StoredGap { gap: n.gap, left: a.hi, right: n.edge, perp_lo, perp_hi });
    }
    out
}

fn perp_span(a0: f64, a1: f64, b0: f64, b1: f64) -> Option<(f64, f64)> {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    (lo.is_finite() && hi.is_finite() && lo <= hi).then_some((lo, hi))
}

fn offer_spacing(best: &mut Option<SpacingChoice>, delta: f64, distance: f64, tol: f64) {
    if !delta.is_finite() || !distance.is_finite() || distance <= 0.0 {
        return;
    }
    let dist = delta.abs();
    if dist <= tol && best.as_ref().is_none_or(|have| dist < have.dist) {
        *best = Some(SpacingChoice { dist, delta, distance });
    }
}

fn gap_matches(gap: f64, distance: f64) -> bool {
    gap.is_finite() && distance.is_finite() && (gap - distance).abs() <= 1e-6
}

/// Moving gaps after the shift. A side is drawn when its new nearest gap equals `distance`.
fn push_moving_gaps(out: &mut Vec<GapMark>, shifted: &Interval, boxes: &[Interval], axis: Axis, distance: f64, label: &str) {
    if let Some(n) = nearest_side(shifted, boxes, None, true)
        && gap_matches(n.gap, distance)
        && let Some((perp_lo, perp_hi)) = perp_span(shifted.perp_lo, shifted.perp_hi, n.perp_lo, n.perp_hi)
    {
        push_gap_mark(out, axis, n.edge, shifted.lo, perp_lo, perp_hi, label);
    }
    if let Some(n) = nearest_side(shifted, boxes, None, false)
        && gap_matches(n.gap, distance)
        && let Some((perp_lo, perp_hi)) = perp_span(shifted.perp_lo, shifted.perp_hi, n.perp_lo, n.perp_hi)
    {
        push_gap_mark(out, axis, shifted.hi, n.edge, perp_lo, perp_hi, label);
    }
}

fn push_gap_mark(out: &mut Vec<GapMark>, axis: Axis, left: f64, right: f64, perp_lo: f64, perp_hi: f64, label: &str) {
    let Some((a, b)) = gap_ends(axis, left, right, perp_lo, perp_hi) else { return };
    if out.iter().any(|g| segment_same(g.a, g.b, a, b)) {
        return;
    }
    out.push(GapMark { a, b, label: label.to_string() });
}

/// Facing edges. The line sits on the midpoint of the perpendicular overlap.
fn gap_ends(axis: Axis, left: f64, right: f64, perp_lo: f64, perp_hi: f64) -> Option<(Point, Point)> {
    if !left.is_finite() || !right.is_finite() || !perp_lo.is_finite() || !perp_hi.is_finite() || right <= left {
        return None;
    }
    let mid = (perp_lo + perp_hi) * 0.5;
    if !mid.is_finite() {
        return None;
    }
    let (a, b) = match axis {
        Axis::X => (Point::new(left, mid), Point::new(right, mid)),
        Axis::Y => (Point::new(mid, left), Point::new(mid, right)),
    };
    (point_finite(a) && point_finite(b)).then_some((a, b))
}
