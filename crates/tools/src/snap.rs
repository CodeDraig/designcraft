//! Snapping and smart guides.
//!
//! [`snap`] tries each pass on a free axis and keeps the first hit inside the snap zone.
//! Ruler guides, margins, and columns draw nothing. Alignment draws [`Overlay::Guide`] in
//! canvas coordinates. Equal spacing draws [`Overlay::Gap`] the same way, through one
//! spread-to-canvas transform. A rotation match draws [`Overlay::Measure`] at the pointer
//! the same way.

mod rotation;
use rotation::rotation_snap;
pub(crate) use rotation::{folded_rotation, reference_rotation, rotation_command_delta};
mod grids;
use grids::{pass_baseline, pass_grid};
mod spacing;
use spacing::pass_spacing;
mod dimensions;
pub(crate) use dimensions::{lay_dimension_guides, lay_line_dimension};
use dimensions::{pass_dimensions, point_finite, segment_same, xf_finite};
mod alignment;
pub(crate) use alignment::moving_bounds;
use alignment::{ALIGN_GROUP_DEPTH, align_skips_item, item_target_rect, layout_edits_parents, pass_align};
mod guides;
use guides::pass_guides;

use std::collections::HashSet;

use designcraft_doc::{Item, ItemId, Orientation, PageSide, Selection, SpreadId, SpreadRef, StrokeAlign};
use designcraft_geom::snap::snap_to_grid;
use designcraft_geom::{Affine, Point, Rect, Vec2, format_measure};

pub use crate::{Gesture, Snap, SnapRequest, SnapView};

use crate::{Overlay, ToolContext};

/// Snap a single point (drawing tools).
///
/// Zero-size rect, and every edge flag on, so the point can meet a guide or an alignment target.
pub fn snap_point(cx: &ToolContext, sr: SpreadRef, p: Point) -> (Point, Vec<Overlay>) {
    let s = snap(
        cx,
        SnapRequest {
            spread: sr,
            gesture: Gesture::Point,
            rect: Rect::from_points(p, p),
            x_edges: [true, true, true],
            y_edges: [true, true, true],
            exclude: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: p,
        },
    );
    (p + s.delta, s.guides)
}

fn rect_finite(r: Rect) -> bool {
    r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
}

/// First hit on each free axis.
///
/// A non-finite rect, or no snap category, returns no correction. An axis whose three edge
/// flags are all false stays at 0 and draws nothing. The zone is [`SnapView::zone_px`] screen
/// pixels, converted with [`ToolContext::tol`]. A zone of 0 or less is no hit.
/// Rotate does not run the position passes, so a locked axis still leaves room for [`Snap::angle`].
pub fn snap(cx: &ToolContext, req: SnapRequest<'_>) -> Snap {
    if !rect_finite(req.rect) || !cx.snap.any() {
        return Snap::default();
    }
    let Some(tol) = zone_tol(cx) else {
        return Snap::default();
    };
    // Position passes would ignore a rotate whose edge flags are all false, and a stray
    // edge flag must not move the object. Only the angle match runs.
    if matches!(req.gesture, Gesture::Rotate) {
        return rotation_snap(cx, &req, tol);
    }
    // Three false flags lock the axis: do not move it and do not draw a guide for it.
    let x_free = req.x_edges != [false; 3];
    let y_free = req.y_edges != [false; 3];
    let x_hit = x_free.then(|| first_on_axis(cx, &req, Axis::X, tol)).flatten();
    let y_hit = y_free.then(|| first_on_axis(cx, &req, Axis::Y, tol)).flatten();
    let mut out = Snap::default();
    if let Some(hit) = &x_hit {
        out.delta.x = hit.delta;
        out.length_delta[0] = hit.length_delta;
    }
    if let Some(hit) = &y_hit {
        out.delta.y = hit.delta;
        out.length_delta[1] = hit.length_delta;
    }
    let placed = Rect::new(req.rect.x0 + out.delta.x, req.rect.y0 + out.delta.y, req.rect.x1 + out.delta.x, req.rect.y1 + out.delta.y);
    if let Some(hit) = &x_hit {
        push_hit_guides(&mut out.guides, cx, &req, Axis::X, hit, placed);
    }
    if let Some(hit) = &y_hit {
        push_hit_guides(&mut out.guides, cx, &req, Axis::Y, hit, placed);
    }
    out
}

/// Move the flagged edges by `delta`.
///
/// One edge takes the whole delta. Both edges of an axis split it, low edge one way and high
/// edge the other, so a length match keeps the centre. A non-finite component is ignored.
pub(crate) fn nudge_edges(mut rect: Rect, x_edges: [bool; 3], y_edges: [bool; 3], delta: Vec2) -> Rect {
    nudge_axis(&mut rect.x0, &mut rect.x1, x_edges, delta.x);
    nudge_axis(&mut rect.y0, &mut rect.y1, y_edges, delta.y);
    rect
}

fn nudge_axis(lo: &mut f64, hi: &mut f64, flags: [bool; 3], delta: f64) {
    if !delta.is_finite() {
        return;
    }
    let [low_edge, _, high_edge] = flags;
    if low_edge && high_edge {
        *lo -= delta;
        *hi += delta;
    } else if low_edge {
        *lo += delta;
    } else if high_edge {
        *hi += delta;
    }
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

#[derive(Clone)]
struct AxisHit {
    delta: f64,
    /// Position of the winning line on this axis.
    at: f64,
    /// Target box for an alignment guide. None when the hit draws nothing.
    span: Option<Rect>,
    /// Matched length minus the proposed length, when this hit is a dimension match.
    length_delta: Option<f64>,
    /// Spread-space segment along the matched side, plus the measure text.
    dimension: Option<DimMark>,
    /// Spacing segments in spread space. Empty unless this hit is a spacing match.
    gaps: Vec<GapMark>,
}

/// Spacing segment in spread space. Drawn after one spread-to-canvas transform.
#[derive(Clone)]
struct GapMark {
    a: Point,
    b: Point,
    label: String,
}

#[derive(Clone)]
struct DimMark {
    a: Point,
    b: Point,
    text: String,
}

struct Best {
    dist: f64,
    hit: AxisHit,
}

/// Screen pixels to spread points. A zone of 0 or less, or a non-finite zone, is no hit.
fn zone_tol(cx: &ToolContext) -> Option<f64> {
    let zone = cx.snap.zone_px;
    if !zone.is_finite() || zone <= 0.0 {
        return None;
    }
    let tol = cx.tol(zone);
    if tol.is_finite() && tol > 0.0 { Some(tol) } else { None }
}

fn first_on_axis(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    pass_grid(cx, req, axis, tol)
        .or_else(|| pass_baseline(cx, req, axis, tol))
        .or_else(|| pass_guides(cx, req, axis, tol))
        .or_else(|| pass_align(cx, req, axis, tol))
        .or_else(|| pass_spacing(cx, req, axis, tol))
        .or_else(|| pass_dimensions(cx, req, axis, tol))
}

/// Each edge whose flag is set, against its own target. Nothing is drawn.
fn offer_enabled(best: &mut Option<Best>, moving: Rect, flags: [bool; 3], axis: Axis, tol: f64, mut target_at: impl FnMut(f64) -> Option<f64>) {
    let (left, mid, right) = axis_triple(moving, axis);
    let [use_left, use_mid, use_right] = flags;
    for (on, v) in [(use_left, left), (use_mid, mid), (use_right, right)] {
        if !on {
            continue;
        }
        let Some(target) = target_at(v) else { continue };
        offer(best, v, target, tol, None);
    }
}

/// Left page of a facing parent, otherwise the last page. Same choice as `Document::parent_page_for`.
fn shown_parent_page_index(page_count: usize, side: PageSide) -> Option<usize> {
    if page_count >= 2 && side == PageSide::Left { Some(0) } else { page_count.checked_sub(1) }
}

fn shift_x(r: Rect, dx: f64) -> Rect {
    Rect::new(r.x0 + dx, r.y0, r.x1 + dx, r.y1)
}

fn axis_flags(req: &SnapRequest<'_>, axis: Axis) -> [bool; 3] {
    match axis {
        Axis::X => req.x_edges,
        Axis::Y => req.y_edges,
    }
}

fn axis_orientation(axis: Axis) -> Orientation {
    match axis {
        Axis::X => Orientation::Vertical,
        Axis::Y => Orientation::Horizontal,
    }
}

/// Left/center/right, or top/center/bottom.
fn axis_triple(r: Rect, axis: Axis) -> (f64, f64, f64) {
    match axis {
        Axis::X => (r.x0, r.center().x, r.x1),
        Axis::Y => (r.y0, r.center().y, r.y1),
    }
}

/// The rect's extent on the axis perpendicular to `axis`.
fn perp_ends(r: Rect, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (r.y0, r.y1),
        Axis::Y => (r.x0, r.x1),
    }
}

fn ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    if !a0.is_finite() || !a1.is_finite() || !b0.is_finite() || !b1.is_finite() {
        return false;
    }
    let (a_lo, a_hi) = (a0.min(a1), a0.max(a1));
    let (b_lo, b_hi) = (b0.min(b1), b0.max(b1));
    a_lo <= b_hi && a_hi >= b_lo
}

/// Any requested edge, including the center, against one guide line. No span, so nothing is drawn.
fn offer_line(best: &mut Option<Best>, moving: Rect, flags: [bool; 3], axis: Axis, target: f64, tol: f64) {
    let (left, mid, right) = axis_triple(moving, axis);
    let [use_left, use_mid, use_right] = flags;
    if use_left {
        offer(best, left, target, tol, None);
    }
    if use_mid {
        offer(best, mid, target, tol, None);
    }
    if use_right {
        offer(best, right, target, tol, None);
    }
}

fn offer_box(best: &mut Option<Best>, moving: Rect, flags: [bool; 3], axis: Axis, target: Rect, tol: f64, edges: bool, centers: bool) {
    let (m0, m_mid, m1) = axis_triple(moving, axis);
    let (t0, t_mid, t1) = axis_triple(target, axis);
    let [use_left, use_mid, use_right] = flags;
    if edges {
        if use_left {
            offer(best, m0, t0, tol, Some(target));
            offer(best, m0, t1, tol, Some(target));
        }
        if use_right {
            offer(best, m1, t0, tol, Some(target));
            offer(best, m1, t1, tol, Some(target));
        }
    }
    if centers && use_mid {
        offer(best, m_mid, t_mid, tol, Some(target));
    }
}

fn offer(best: &mut Option<Best>, moving: f64, target: f64, tol: f64, span: Option<Rect>) {
    if !moving.is_finite() || !target.is_finite() {
        return;
    }
    let delta = target - moving;
    let dist = delta.abs();
    if dist <= tol && best.as_ref().is_none_or(|b| dist < b.dist) {
        *best = Some(Best { dist, hit: AxisHit { delta, at: target, span, length_delta: None, dimension: None, gaps: Vec::new() } });
    }
}

fn push_hit_guides(out: &mut Vec<Overlay>, cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, hit: &AxisHit, moving: Rect) {
    push_alignment_guide(out, cx, req.spread, axis, hit, moving);
    push_gap_guides(out, cx, req.spread, hit);
    let Some(mark) = &hit.dimension else { return };
    let xf = cx.layout.xf(req.spread);
    if mark.a.x.is_finite() && mark.a.y.is_finite() && mark.b.x.is_finite() && mark.b.y.is_finite() {
        out.push(Overlay::Guide { a: xf * mark.a, b: xf * mark.b });
    }
    let p = xf * req.pointer;
    if p.x.is_finite() && p.y.is_finite() {
        out.push(Overlay::Measure { p, text: mark.text.clone() });
    }
}

fn push_gap_guides(out: &mut Vec<Overlay>, cx: &ToolContext, spread: SpreadRef, hit: &AxisHit) {
    if hit.gaps.is_empty() {
        return;
    }
    let xf = cx.layout.xf(spread);
    for g in &hit.gaps {
        let a = xf * g.a;
        let b = xf * g.b;
        if point_finite(a) && point_finite(b) {
            out.push(Overlay::Gap { a, b, label: g.label.clone() });
        }
    }
}

fn push_alignment_guide(out: &mut Vec<Overlay>, cx: &ToolContext, spread: SpreadRef, axis: Axis, hit: &AxisHit, moving: Rect) {
    let Some(target) = hit.span else { return };
    let xf = cx.layout.xf(spread);
    let (a, b) = match axis {
        Axis::X => {
            let lo = moving.y0.min(moving.y1).min(target.y0.min(target.y1)) - 6.0;
            let hi = moving.y0.max(moving.y1).max(target.y0.max(target.y1)) + 6.0;
            (xf * Point::new(hit.at, lo), xf * Point::new(hit.at, hi))
        }
        Axis::Y => {
            let lo = moving.x0.min(moving.x1).min(target.x0.min(target.x1)) - 6.0;
            let hi = moving.x0.max(moving.x1).max(target.x0.max(target.x1)) + 6.0;
            (xf * Point::new(lo, hit.at), xf * Point::new(hi, hit.at))
        }
    };
    out.push(Overlay::Guide { a, b });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use designcraft_compose::Cache;
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::{Document, Guide, Item, Margins, Selection, Shape, Stroke};
    use designcraft_geom::shapes;
    use designcraft_geom::{Affine, Unit};

    use crate::layout::CanvasLayout;

    use super::*;

    // The factory flags are const. Pin them anyway.
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn factory_zone_is_four_pixels() {
        assert!(SnapView::FACTORY.snap_to_guides);
        assert!(!SnapView::FACTORY.snap_to_document_grid);
        assert!(SnapView::FACTORY.smart_guides);
        assert_eq!(SnapView::FACTORY.zone_px, 4.0);
        assert!(!SnapView::OFF.any());
        assert!(SnapView::FACTORY.any());
    }

    fn doc_with_items(moving: Rect, other: Rect, guide_x: f64) -> (Document, ItemId, ItemId) {
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let make = |doc: &mut Document, rect: Rect| {
            let id = ItemId(doc.alloc());
            let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
            doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
            id
        };
        let moving_id = make(&mut doc, moving);
        let other_id = make(&mut doc, other);
        let page = &mut Arc::make_mut(&mut doc.spreads[0]).pages[0];
        page.guides.push(Guide { orientation: Orientation::Vertical, position: guide_x, spread: false, locked: false, layer: None, liquid: false });
        (doc, moving_id, other_id)
    }

    fn ctx_on<'a>(doc: &'a Document, sel: &'a Selection, cache: &'a Cache, layout: &'a CanvasLayout) -> ToolContext<'a> {
        ToolContext { doc, selection: sel, cache, layout, zoom: 1.0, layer: doc.default_layer(), snap: SnapView::FACTORY, unit: Unit::Points }
    }

    fn y_req(rect: Rect) -> SnapRequest<'static> {
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect,
            x_edges: [false, false, false],
            y_edges: [true, true, true],
            exclude: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        }
    }

    fn move_req<'a>(rect: Rect, exclude: &'a [ItemId]) -> SnapRequest<'a> {
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect,
            x_edges: [true, true, true],
            y_edges: [false, false, false],
            exclude,
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        }
    }

    #[test]
    fn guide_beats_a_closer_object_edge() {
        // Right edge at 100.5. Object edge at 100 (0.5 away). Guide at 104 (3.5 away). Zone is 4.
        let (doc, moving, _other) = doc_with_items(Rect::new(80.0, 100.0, 100.5, 140.0), Rect::new(90.0, 100.0, 100.0, 160.0), 104.0);
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.zone_px = 4.0;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Move,
                rect: Rect::new(80.0, 100.0, 100.5, 140.0),
                x_edges: [true, true, true],
                y_edges: [false, false, false],
                exclude: &[moving],
                copying: false,
                lengths: [None, None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert!((hit.delta.x - 3.5).abs() < 1e-6);
        assert!(hit.guides.is_empty());
    }

    #[test]
    fn parent_centered_stroke_attracts_at_the_outer_edge() {
        let mut doc = Document::new(&NewDocument::default());
        let pid = doc.add_parent("A", "A-Parent", 1, 12.0, Margins::uniform(36.0));
        doc.apply_parent(&[0], Some(pid)).unwrap();
        let layer = doc.default_layer();
        let id = ItemId(doc.alloc());
        let (ppi, ppg) = doc.parent_page_for(0).unwrap();
        let parent_x = doc.parents[ppi].pages[ppg].x;
        let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(parent_x + 72.0, 140.0, parent_x + 252.0, 300.0)));
        item.stroke = Stroke::default(); // 1 pt, center
        doc.insert_item(SpreadRef::Parent(ppi), item, None).unwrap();
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        // Moving left edge at 253. Visible parent right edge is 252.5.
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Move,
                rect: Rect::new(253.0, 140.0, 353.0, 240.0),
                x_edges: [true, false, false],
                y_edges: [false, false, false],
                exclude: &[],
                copying: false,
                lengths: [None, None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert!((hit.delta.x - -0.5).abs() < 1e-6, "delta {}", hit.delta.x);
    }

    #[test]
    fn object_edge_snaps_when_guide_snap_is_off() {
        let (doc, moving, _other) = doc_with_items(Rect::new(80.0, 100.0, 100.5, 140.0), Rect::new(90.0, 100.0, 100.0, 160.0), 104.0);
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Move,
                rect: Rect::new(80.0, 100.0, 100.5, 140.0),
                x_edges: [true, true, true],
                y_edges: [false, false, false],
                exclude: &[moving],
                copying: false,
                lengths: [None, None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert!((hit.delta.x - -0.5).abs() < 1e-6);
        assert!(matches!(hit.guides.first(), Some(Overlay::Guide { .. })));
    }

    #[test]
    fn hidden_layer_guide_is_not_a_target() {
        let (mut doc, moving, _) = doc_with_items(Rect::new(80.0, 100.0, 102.0, 140.0), Rect::new(300.0, 100.0, 340.0, 140.0), 104.0);
        let layer = doc.default_layer();
        doc.layers.iter_mut().find(|l| l.id == layer).unwrap().visible = false;
        Arc::make_mut(&mut doc.spreads[0]).pages[0].guides[0].layer = Some(layer);
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let cx = ctx_on(&doc, &sel, &cache, &layout);
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Move,
                rect: Rect::new(80.0, 100.0, 102.0, 140.0),
                x_edges: [false, false, true],
                y_edges: [false, false, false],
                exclude: &[moving],
                copying: false,
                lengths: [None, None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert_eq!(hit.delta.x, 0.0);
    }

    #[test]
    fn alt_copy_snaps_to_the_original() {
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let id = ItemId(doc.alloc());
        let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 40.0, 160.0, 80.0)));
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Move,
                rect: Rect::new(162.0, 40.0, 222.0, 80.0),
                x_edges: [true, false, false],
                y_edges: [false, false, false],
                exclude: &[id],
                copying: true,
                lengths: [None, None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert!((hit.delta.x - -2.0).abs() < 1e-6);
    }

    #[test]
    fn alt_copy_does_not_stick_to_the_preview() {
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let make = |doc: &mut Document, rect: Rect| {
            let id = ItemId(doc.alloc());
            let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
            doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
            id
        };
        let original = make(&mut doc, Rect::new(100.0, 40.0, 160.0, 80.0));
        let ghost = make(&mut doc, Rect::new(161.0, 40.0, 221.0, 80.0));
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::items(vec![ghost]);
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Move,
                rect: Rect::new(161.0, 40.0, 221.0, 80.0),
                x_edges: [true, false, false],
                y_edges: [false, false, false],
                exclude: &[original],
                copying: true,
                lengths: [None, None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert!((hit.delta.x - -1.0).abs() < 1e-6, "delta.x = {}", hit.delta.x);
    }

    #[test]
    fn grid_beats_a_closer_guide() {
        let mut doc = Document::new(&NewDocument::default());
        doc.settings.grid.horizontal = 72.0;
        doc.settings.grid.vertical = 72.0;
        doc.settings.grid.subdivisions = 8; // 9 pt
        let layer = doc.default_layer();
        let id = ItemId(doc.alloc());
        let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(10.0, 10.0, 40.0, 40.0)));
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        // Guide 1 pt away at x = 11. Subdivision line at 9 is 1 pt the other way. Grid must win.
        Arc::make_mut(&mut doc.spreads[0]).pages[0].guides.push(Guide {
            orientation: Orientation::Vertical,
            position: 11.0,
            spread: true,
            locked: false,
            layer: None,
            liquid: false,
        });
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_document_grid = true;
        let hit = snap(&cx, move_req(Rect::new(10.0, 10.0, 40.0, 40.0), &[id]));
        assert!((hit.delta.x - -1.0).abs() < 1e-6);
        assert!(hit.guides.is_empty());
    }

    #[test]
    fn hidden_baseline_still_snaps_and_zero_increment_does_not() {
        let mut doc = Document::new(&NewDocument::default());
        doc.settings.baseline_grid.start = 36.0;
        doc.settings.baseline_grid.increment = 12.0;
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let cx = ctx_on(&doc, &sel, &cache, &layout);
        // Top at 49 wants the line at 48 (start 36, increment 12). The margin at 36 is 13 pt away.
        let hit = snap(&cx, y_req(Rect::new(100.0, 49.0, 140.0, 80.0)));
        assert!((hit.delta.y - -1.0).abs() < 1e-6);
        let mut dead = doc.clone();
        dead.settings.baseline_grid.increment = 0.0;
        let layout = CanvasLayout::new(&dead, false);
        let cx = ctx_on(&dead, &sel, &cache, &layout);
        let hit = snap(&cx, y_req(Rect::new(100.0, 49.0, 140.0, 80.0)));
        assert_eq!(hit.delta.y, 0.0);
    }

    #[test]
    fn rotated_item_matches_its_own_side() {
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let id = ItemId(doc.alloc());
        let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 200.0, 40.0)));
        item.xf = Affine::rotate((-30.0_f64).to_radians());
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        cx.snap.snap_to_document_grid = false;
        // Width 193.2 is the axis-aligned box. 200 is the side. Zone 4 reaches 200 from 197, not from 193.
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Resize,
                rect: Rect::new(80.0, 200.0, 277.0, 280.0),
                x_edges: [false, false, true],
                y_edges: [false, false, false],
                exclude: &[],
                copying: false,
                lengths: [Some(197.0), None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(277.0, 240.0),
            },
        );
        assert!((hit.delta.x - 3.0).abs() < 1e-6);
        assert!(matches!(hit.guides.last(), Some(Overlay::Measure { text, .. }) if text.contains("200")));
    }

    #[test]
    fn grouped_child_matches_spread_side() {
        // Group scale 2 on X and 1 on Y. Child rotated -90 degrees (decompose reports +90).
        // Spread-space sides are local width times 1 (100) and local height times 2 (80).
        // 82 is 2 pt from 80 and far from 200, which is what per-node scales would offer.
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let child_id = ItemId(doc.alloc());
        let mut child = Item::new(child_id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 40.0)));
        child.xf = Affine::rotate((-90.0_f64).to_radians());
        let group_id = ItemId(doc.alloc());
        let mut group = Item::new(group_id, layer, Shape::Group, designcraft_geom::PathData::default());
        group.xf = Affine::scale_non_uniform(2.0, 1.0);
        group.content = designcraft_doc::Content::Group { items: vec![std::sync::Arc::new(child)] };
        doc.insert_item(SpreadRef::Doc(0), group, None).unwrap();
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        cx.snap.snap_to_document_grid = false;
        cx.snap.align_edges = false;
        cx.snap.align_centers = false;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Resize,
                rect: Rect::new(200.0, 200.0, 282.0, 240.0),
                x_edges: [false, false, true],
                y_edges: [false, false, false],
                exclude: &[],
                copying: false,
                lengths: [Some(82.0), None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(282.0, 220.0),
            },
        );
        assert!((hit.delta.x - -2.0).abs() < 1e-6, "delta.x = {}, want -2 toward local height times 2", hit.delta.x);
    }

    #[test]
    fn move_matches_a_stationary_gap() {
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        // Stationary gap of 20 between [0,40] and [60,90]. Moving box at [112,150], 22 away from 90.
        for r in [Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(60.0, 0.0, 90.0, 30.0), Rect::new(112.0, 0.0, 150.0, 30.0)] {
            let id = ItemId(doc.alloc());
            let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(r));
            doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        }
        let moving = doc.spreads[0].items[2].id;
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Move,
                rect: Rect::new(112.0, 0.0, 150.0, 30.0),
                x_edges: [true, true, true],
                y_edges: [false, false, false],
                exclude: &[moving],
                copying: false,
                lengths: [None, None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert!((hit.delta.x - -2.0).abs() < 1e-6);
        assert!(hit.guides.iter().any(|g| matches!(g, Overlay::Gap { .. })));
    }

    #[test]
    fn create_does_not_match_spacing() {
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        for r in [Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(60.0, 0.0, 90.0, 30.0)] {
            let id = ItemId(doc.alloc());
            let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(r));
            doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        }
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        cx.snap.smart_dimensions = false;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Create,
                rect: Rect::new(112.0, 0.0, 150.0, 30.0),
                x_edges: [true, true, true],
                y_edges: [false, false, false],
                exclude: &[],
                copying: false,
                lengths: [Some(38.0), None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert!((hit.delta.x).abs() < 1e-6);
    }

    #[test]
    fn rotation_matches_another_items_angle() {
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let id = ItemId(doc.alloc());
        let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 80.0, 40.0)));
        item.xf = Affine::rotate((-30.0_f64).to_radians());
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Rotate,
                rect: Rect::new(100.0, 100.0, 160.0, 140.0),
                x_edges: [false, false, false],
                y_edges: [false, false, false],
                exclude: &[],
                copying: false,
                lengths: [None, None],
                angle: Some(28.0),
                radius: 80.0,
                pointer: Point::new(180.0, 120.0),
            },
        );
        let angle = hit.angle.expect("rotation hit");
        assert!((angle - 30.0).abs() < 1e-6);
        assert!(matches!(hit.guides.first(), Some(Overlay::Measure { .. })));
    }

    #[test]
    fn parent_item_angle_matches() {
        // Document::new already has A-Parent. Apply a second parent and insert on the
        // page parent_page_for shows, not Parent(0). A facing right page sits near x 612.
        let mut doc = Document::new(&NewDocument::default());
        let pid = doc.add_parent("B", "B-Parent", 1, 12.0, Margins::uniform(36.0));
        doc.apply_parent(&[0], Some(pid)).unwrap();
        std::sync::Arc::make_mut(&mut doc.spreads[0]).pages[0].show_parent_items = true;
        let (ppi, ppg) = doc.parent_page_for(0).unwrap();
        let parent_x = doc.parents[ppi].pages[ppg].x;
        let rect = Rect::new(parent_x + 72.0, 140.0, parent_x + 152.0, 180.0);
        let center = rect.center();
        let layer = doc.default_layer();
        let id = ItemId(doc.alloc());
        let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
        item.xf = Affine::translate((center.x, center.y)) * Affine::rotate((-30.0_f64).to_radians()) * Affine::translate((-center.x, -center.y));
        doc.insert_item(SpreadRef::Parent(ppi), item, None).unwrap();
        assert!(doc.spreads[0].items.iter().all(|it| (designcraft_geom::decompose::decompose(it.xf).rotation - 30.0).abs() > 1.0));
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Rotate,
                rect: Rect::new(100.0, 100.0, 160.0, 140.0),
                x_edges: [false, false, false],
                y_edges: [false, false, false],
                exclude: &[],
                copying: false,
                lengths: [None, None],
                angle: Some(28.0),
                radius: 80.0,
                pointer: Point::new(180.0, 120.0),
            },
        );
        let angle = hit.angle.expect("parent rotation hit");
        assert!((angle - 30.0).abs() < 1e-6);
    }

    #[test]
    fn rotated_group_uses_the_per_child_union() {
        // Two children do not fill their local union. Rotating that union as one rectangle
        // swings the empty corners out. The snap target is each child's visible box, transformed, then united.
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let mut child_a = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
        child_a.stroke = Stroke { weight: 2.0, ..Stroke::default() };
        let child_b = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(40.0, 30.0, 50.0, 40.0)));
        let mut group = Item::new(ItemId(doc.alloc()), layer, Shape::Group, designcraft_geom::PathData::default());
        let xf = Affine::translate((300.0, 400.0)) * Affine::rotate((-45.0_f64).to_radians());
        group.xf = xf;
        group.content = designcraft_doc::Content::Group { items: vec![Arc::new(child_a), Arc::new(child_b)] };
        doc.insert_item(SpreadRef::Doc(0), group, None).unwrap();
        let a_vis = Rect::new(-1.0, -1.0, 11.0, 11.0);
        let b_vis = Rect::new(40.0, 30.0, 50.0, 40.0);
        let per_child = xf.transform_rect_bbox(a_vis).union(xf.transform_rect_bbox(b_vis));
        let rotated_union = xf.transform_rect_bbox(a_vis.union(b_vis));
        // At 45 degrees the empty corners move y, not x. The two boxes share x0.
        assert!((per_child.y0 - rotated_union.y0).abs() > 1.0, "fixture does not separate the two boxes");
        let gap = 2.0;
        // A short rect just above the per-child top. Its other edges stay outside the zone of the rotated union.
        let moving = Rect::new(per_child.x0, per_child.y0 - gap - 4.0, per_child.x0 + 20.0, per_child.y0 - gap);
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        cx.snap.snap_to_document_grid = false;
        let hit = snap(&cx, y_req(moving));
        assert!((hit.delta.y - gap).abs() < 1e-4, "delta.y {}, want {gap} onto {}", hit.delta.y, per_child.y0);
        let wrong = rotated_union.y0 - moving.y1;
        assert!((hit.delta.y - wrong).abs() > 1.0, "delta matched the rotated local union");
    }

    #[test]
    fn rotated_group_child_does_not_offer_local_zero() {
        // The group is at 30 degrees. Its child stays at local 0, so the absolute angle is 30.
        // 2 degrees is inside the zone of 0 and far from 30. Local 0 must not win.
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let child = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 40.0, 20.0)));
        let mut group = Item::new(ItemId(doc.alloc()), layer, Shape::Group, designcraft_geom::PathData::default());
        group.xf = Affine::rotate((-30.0_f64).to_radians());
        group.content = designcraft_doc::Content::Group { items: vec![Arc::new(child)] };
        doc.insert_item(SpreadRef::Doc(0), group, None).unwrap();
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.snap_to_guides = false;
        let ask = |angle: f64| {
            snap(
                &cx,
                SnapRequest {
                    spread: SpreadRef::Doc(0),
                    gesture: Gesture::Rotate,
                    rect: Rect::new(100.0, 100.0, 160.0, 140.0),
                    x_edges: [false, false, false],
                    y_edges: [false, false, false],
                    exclude: &[],
                    copying: false,
                    lengths: [None, None],
                    angle: Some(angle),
                    radius: 80.0,
                    pointer: Point::new(180.0, 120.0),
                },
            )
        };
        let near_zero = ask(2.0);
        assert!(near_zero.angle.is_none(), "local 0 was a target: {:?}", near_zero.angle);
        let near_group = ask(28.0);
        let angle = near_group.angle.expect("absolute 30");
        assert!((angle - 30.0).abs() < 1e-6, "angle {angle}");
    }
}
