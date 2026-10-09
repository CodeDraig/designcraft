//! Dimension candidates and measurement guides.

use super::{
    ALIGN_GROUP_DEPTH, Affine, Axis, AxisHit, DimMark, Gesture, HashSet, Item, ItemId, Overlay, PageSide, Point, Rect, Selection, SnapRequest,
    SpreadId, SpreadRef, StrokeAlign, ToolContext, align_skips_item, axis_flags, format_measure, layout_edits_parents, rect_finite,
    shown_parent_page_index,
};

/// Other items' own side lengths, on resize and create.
///
/// Own size is the path box in item space, inflated the way [`Item::visible_bounds`] inflates,
/// then scaled by the absolute transform. That transform is the parent chain composed with the
/// item and decomposed once, so a rotated child inside a non-uniform group keeps the spread-space
/// sides. Rotation is dropped. Width and height are both candidates for whichever length is set.
/// The closest length inside the zone wins. Delta is the length difference applied from the moving
/// edge. A rotated rectangle contributes its side lengths, not the axis-aligned box.
pub(super) fn pass_dimensions(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !matches!(req.gesture, Gesture::Resize | Gesture::Create) || !cx.snap.smart_guides || !cx.snap.smart_dimensions {
        return None;
    }
    let proposed = match axis {
        Axis::X => req.lengths[0],
        Axis::Y => req.lengths[1],
    }?;
    if !proposed.is_finite() {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    for len in dimension_lengths(cx, req) {
        if !len.is_finite() || len < 0.0 {
            continue;
        }
        let dist = (len - proposed).abs();
        if dist <= tol && best.is_none_or(|(have, _)| dist < have) {
            best = Some((dist, len));
        }
    }
    let (_, length) = best?;
    let diff = length - proposed;
    if !diff.is_finite() {
        return None;
    }
    let flags = axis_flags(req, axis);
    let text = format_measure(length, cx.unit);
    Some(AxisHit {
        delta: length_edge_delta(flags, diff),
        at: 0.0,
        span: None,
        length_delta: Some(diff),
        dimension: Some(dimension_mark(req, axis, diff, text)),
        gaps: Vec::new(),
    })
}

/// Edge shift so adding it to the moving edge grows the length by `diff`.
fn length_edge_delta(flags: [bool; 3], diff: f64) -> f64 {
    let [low_edge, mid, high_edge] = flags;
    if low_edge && high_edge && !mid {
        diff / 2.0
    } else if high_edge && !low_edge {
        diff
    } else if low_edge && !high_edge {
        -diff
    } else {
        diff
    }
}

/// Guide along the side whose length matched, after the edge has moved.
fn dimension_mark(req: &SnapRequest<'_>, axis: Axis, diff: f64, text: String) -> DimMark {
    let (a, b) = dimension_segment(req.rect, axis, axis_flags(req, axis), diff).unwrap_or((Point::ORIGIN, Point::ORIGIN));
    DimMark { a, b, text }
}

/// Spread-space segment along the matched side. `diff` 0 is the side of `rect` itself.
fn dimension_segment(rect: Rect, axis: Axis, flags: [bool; 3], diff: f64) -> Option<(Point, Point)> {
    if !diff.is_finite() || !rect_finite(rect) {
        return None;
    }
    let (a, b) = match axis {
        Axis::X => {
            let (x0, x1) = grown_span(rect.x0, rect.x1, flags, diff);
            let y = rect.y0.max(rect.y1);
            (Point::new(x0, y), Point::new(x1, y))
        }
        Axis::Y => {
            let (y0, y1) = grown_span(rect.y0, rect.y1, flags, diff);
            let x = rect.x0.max(rect.x1);
            (Point::new(x, y0), Point::new(x, y1))
        }
    };
    (point_finite(a) && point_finite(b)).then_some((a, b))
}

pub(super) fn point_finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Move dimension guides onto the rect the gesture commits.
///
/// Snap draws the guide on `pre` plus the length difference along the flagged edge. Aspect re-lock
/// and a centered scale move that side afterwards. The guide then follows the committed side.
pub(crate) fn lay_dimension_guides(
    guides: &mut Vec<Overlay>,
    xf: Affine,
    pre: Rect,
    committed: Rect,
    x_edges: [bool; 3],
    y_edges: [bool; 3],
    length_delta: [Option<f64>; 2],
) {
    if !rect_finite(committed) {
        return;
    }
    let axes = [(Axis::X, x_edges, length_delta[0]), (Axis::Y, y_edges, length_delta[1])];
    for (axis, edges, diff) in axes {
        let Some(diff) = diff else { continue };
        let Some((old_a, old_b)) = dimension_segment(pre, axis, edges, diff) else { continue };
        let Some((new_a, new_b)) = dimension_segment(committed, axis, edges, 0.0) else { continue };
        let (oa, ob) = (xf * old_a, xf * old_b);
        let (na, nb) = (xf * new_a, xf * new_b);
        if !point_finite(na) || !point_finite(nb) {
            continue;
        }
        let mut found = false;
        for g in guides.iter_mut() {
            if let Overlay::Guide { a, b } = g
                && segment_same(*a, *b, oa, ob)
            {
                *a = na;
                *b = nb;
                found = true;
                break;
            }
        }
        if !found {
            guides.push(Overlay::Guide { a: na, b: nb });
        }
    }
}

/// Move a line's dimension tick onto the committed segment.
///
/// The dimensions pass draws a short horizontal tick at the proposed endpoint because the
/// length is carried on x. The guide that remains is the segment from the anchor to the
/// endpoint the gesture commits.
pub(crate) fn lay_line_dimension(guides: &mut Vec<Overlay>, xf: Affine, anchor: Point, proposed: Point, committed: Point, diff: f64) {
    if !diff.is_finite() {
        return;
    }
    let rect = Rect::from_points(proposed, proposed);
    let Some((old_a, old_b)) = dimension_segment(rect, Axis::X, [true, true, true], diff) else { return };
    let (oa, ob) = (xf * old_a, xf * old_b);
    let (na, nb) = (xf * anchor, xf * committed);
    if !point_finite(na) || !point_finite(nb) {
        return;
    }
    let mut found = false;
    for g in guides.iter_mut() {
        if let Overlay::Guide { a, b } = g
            && segment_same(*a, *b, oa, ob)
        {
            *a = na;
            *b = nb;
            found = true;
            break;
        }
    }
    if !found {
        guides.push(Overlay::Guide { a: na, b: nb });
    }
}

pub(super) fn segment_same(a0: Point, b0: Point, a1: Point, b1: Point) -> bool {
    let close = |p: Point, q: Point| (p.x - q.x).abs() <= 1e-6 && (p.y - q.y).abs() <= 1e-6;
    (close(a0, a1) && close(b0, b1)) || (close(a0, b1) && close(b0, a1))
}

fn grown_span(a0: f64, a1: f64, flags: [bool; 3], diff: f64) -> (f64, f64) {
    let (lo, hi) = if a0 <= a1 { (a0, a1) } else { (a1, a0) };
    let [low_edge, mid, high_edge] = flags;
    if low_edge && high_edge && !mid {
        let mid_v = (lo + hi) / 2.0;
        let half = (hi - lo) / 2.0 + diff / 2.0;
        (mid_v - half, mid_v + half)
    } else if low_edge && !high_edge {
        (lo - diff, hi)
    } else {
        (lo, hi + diff)
    }
}

fn dimension_lengths(cx: &ToolContext, req: &SnapRequest<'_>) -> Vec<f64> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(sp) = cx.doc.spread(req.spread) {
        for item in &sp.items {
            push_own_lengths(&mut out, cx.selection, req, item, Affine::IDENTITY, false, 0, &mut seen);
        }
    }
    if !layout_edits_parents(cx) {
        push_parent_lengths(&mut out, cx, req, &mut seen);
    }
    out
}

fn push_own_lengths(
    out: &mut Vec<f64>,
    selection: &Selection,
    req: &SnapRequest<'_>,
    item: &Item,
    ancestor: Affine,
    skipped: bool,
    depth: usize,
    seen: &mut HashSet<ItemId>,
) {
    if depth >= ALIGN_GROUP_DEPTH || !seen.insert(item.id) || item.hidden {
        return;
    }
    let abs = ancestor * item.xf;
    if !xf_finite(abs) {
        return;
    }
    let skip = skipped || align_skips_item(selection, req, item.id);
    if item.is_group() && !item.children().is_empty() {
        for child in item.children() {
            push_own_lengths(out, selection, req, child, abs, skip, depth + 1, seen);
        }
        return;
    }
    if skip {
        return;
    }
    let Some((w, h)) = leaf_own_size(item, abs) else { return };
    out.push(w);
    out.push(h);
}

pub(super) fn xf_finite(m: Affine) -> bool {
    m.as_coeffs().iter().all(|c| c.is_finite())
}

/// Path bounds in item space, stroke outset, then one decompose of the absolute transform.
/// Rotation is not applied to the side lengths.
fn leaf_own_size(item: &Item, abs: Affine) -> Option<(f64, f64)> {
    let local = item.path.bounds()?;
    if !rect_finite(local) {
        return None;
    }
    let outset = stroke_outset(item);
    if !outset.is_finite() {
        return None;
    }
    let local = if outset > 0.0 { local.inflate(outset, outset) } else { local };
    if !rect_finite(local) {
        return None;
    }
    let decomposed = designcraft_geom::decompose::decompose(abs);
    let sx = decomposed.scale_x.abs();
    let sy = decomposed.scale_y.abs();
    if !sx.is_finite() || !sy.is_finite() {
        return None;
    }
    let w = (local.x1 - local.x0).abs() * sx;
    let h = (local.y1 - local.y0).abs() * sy;
    (w.is_finite() && h.is_finite()).then_some((w, h))
}

/// Same outset as [`Item::visible_bounds`]: half the weight when centered, the whole weight when
/// outside, nothing when inside or when the stroke is none. Open paths also count arrowheads.
fn stroke_outset(item: &Item) -> f64 {
    if item.stroke.is_none() {
        return 0.0;
    }
    let w = match item.stroke.align {
        StrokeAlign::Center => item.stroke.weight / 2.0,
        StrokeAlign::Inside => 0.0,
        StrokeAlign::Outside => item.stroke.weight,
    };
    if item.path.is_closed() { w } else { w.max(item.stroke.extent() - item.stroke.weight) }
}

fn push_parent_lengths(out: &mut Vec<f64>, cx: &ToolContext, req: &SnapRequest<'_>, seen_items: &mut HashSet<ItemId>) {
    let SpreadRef::Doc(si) = req.spread else { return };
    let page_count = cx.doc.spreads.get(si).map(|sp| sp.pages.len()).unwrap_or(0);
    let first = cx.doc.first_page_of_spread(si);
    for pi in 0..page_count {
        let Some(abs) = first.checked_add(pi) else { continue };
        let Some(page) = cx.doc.page(abs) else { continue };
        if !page.show_parent_items {
            continue;
        }
        let doc_side = page.side;
        let overridden = page.overridden.clone();
        let Some((ppi, _)) = cx.doc.parent_page_for(abs) else { continue };
        let mut seen_spreads = HashSet::new();
        push_parent_length_chain(out, cx, req, doc_side, &overridden, ppi, seen_items, &mut seen_spreads);
    }
}

fn push_parent_length_chain(
    out: &mut Vec<f64>,
    cx: &ToolContext,
    req: &SnapRequest<'_>,
    doc_side: PageSide,
    overridden: &[ItemId],
    index: usize,
    seen_items: &mut HashSet<ItemId>,
    seen_spreads: &mut HashSet<SpreadId>,
) {
    let Some(parent) = cx.doc.parents.get(index) else { return };
    if !seen_spreads.insert(parent.id) {
        return;
    }
    let next = parent.parent.as_ref().and_then(|info| info.based_on);
    if let Some(page_idx) = shown_parent_page_index(parent.pages.len(), doc_side) {
        for item in &parent.items {
            if overridden.contains(&item.id) {
                continue;
            }
            if parent.pages.len() > 1 && parent.page_at_x(item.bounds().center().x) != Some(page_idx) {
                continue;
            }
            push_own_lengths(out, cx.selection, req, item, Affine::IDENTITY, false, 0, seen_items);
        }
    }
    let Some(next_id) = next else { return };
    let Some(next_index) = cx.doc.parent_index(next_id) else { return };
    push_parent_length_chain(out, cx, req, doc_side, overridden, next_index, seen_items, seen_spreads);
}
