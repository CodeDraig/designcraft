//! Rotation matching and angle normalization.

use super::{
    ALIGN_GROUP_DEPTH, Affine, Gesture, HashSet, Item, ItemId, Overlay, PageSide, Selection, Snap, SnapRequest, SpreadId, SpreadRef, ToolContext,
    align_skips_item, layout_edits_parents, point_finite, shown_parent_page_index, xf_finite,
};

/// Matched rotation, in the same degrees `transform.rotate` receives.
///
/// Runs only for [`Gesture::Rotate`], and only when Smart Guides and Smart Dimensions are on.
/// `angle` is compared with `decompose` of the absolute transform in the same sign. That
/// transform is the ancestor chain composed with `item.xf`. A top-level item has no ancestor,
/// so it stays `decompose(item.xf)`. The tolerance is the zone length subtended at `radius`
/// (`atan(zone / radius)` in degrees). A radius below 1 pt, or a non-finite angle, radius, or
/// zone, does not hit. Candidates are visible items on the requested spread, including children
/// of a group, plus shown parent items. A child inside a rotated group uses the composed angle,
/// not its local 0. Hidden items and the shared exclude list are skipped. An unrotated item
/// contributes 0. The closest angle wins, folding whole turns so a few degrees across 0 still
/// match. The measure is the angle in degrees and there is no guide line.
pub(super) fn rotation_snap(cx: &ToolContext, req: &SnapRequest<'_>, zone_length: f64) -> Snap {
    let Some(angle) = match_rotation(cx, req, zone_length) else {
        return Snap::default();
    };
    let mut guides = Vec::new();
    let p = cx.layout.xf(req.spread) * req.pointer;
    if point_finite(p) {
        guides.push(Overlay::Measure { p, text: format_angle(angle) });
    }
    Snap { angle: Some(angle), guides, ..Snap::default() }
}

fn match_rotation(cx: &ToolContext, req: &SnapRequest<'_>, zone_length: f64) -> Option<f64> {
    if !matches!(req.gesture, Gesture::Rotate) || !cx.snap.smart_guides || !cx.snap.smart_dimensions {
        return None;
    }
    let proposed = req.angle?;
    if !proposed.is_finite() || !req.radius.is_finite() || req.radius < 1.0 || !zone_length.is_finite() || zone_length <= 0.0 {
        return None;
    }
    let tol = (zone_length / req.radius).atan().to_degrees();
    if !tol.is_finite() {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    let mut seen = HashSet::new();
    if let Some(spread) = cx.doc.spread(req.spread) {
        for item in &spread.items {
            push_rotation(&mut best, cx.selection, req, item, Affine::IDENTITY, false, 0, &mut seen, proposed, tol);
        }
    }
    // Parent editing already lists those items on the spread. Do not add them again.
    if !layout_edits_parents(cx) {
        push_parent_rotations(&mut best, cx, req, &mut seen, proposed, tol);
    }
    best.map(|(_, angle)| angle)
}

/// Shown parent items, same page and `based_on` walk as parent lengths.
/// Parent items are only translated onto the page, so the angle is still `item.xf`.
fn push_parent_rotations(
    best: &mut Option<(f64, f64)>,
    cx: &ToolContext,
    req: &SnapRequest<'_>,
    seen_items: &mut HashSet<ItemId>,
    proposed: f64,
    tol: f64,
) {
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
        push_parent_rotation_chain(best, cx, req, doc_side, &overridden, ppi, seen_items, &mut seen_spreads, proposed, tol);
    }
}

fn push_parent_rotation_chain(
    best: &mut Option<(f64, f64)>,
    cx: &ToolContext,
    req: &SnapRequest<'_>,
    doc_side: PageSide,
    overridden: &[ItemId],
    index: usize,
    seen_items: &mut HashSet<ItemId>,
    seen_spreads: &mut HashSet<SpreadId>,
    proposed: f64,
    tol: f64,
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
            // A facing parent only shows items whose center sits on this document page's parent page.
            if parent.pages.len() > 1 && parent.page_at_x(item.bounds().center().x) != Some(page_idx) {
                continue;
            }
            push_rotation(best, cx.selection, req, item, Affine::IDENTITY, false, 0, seen_items, proposed, tol);
        }
    }
    let Some(next_id) = next else { return };
    let Some(next_index) = cx.doc.parent_index(next_id) else { return };
    push_parent_rotation_chain(best, cx, req, doc_side, overridden, next_index, seen_items, seen_spreads, proposed, tol);
}

fn push_rotation(
    best: &mut Option<(f64, f64)>,
    selection: &Selection,
    req: &SnapRequest<'_>,
    item: &Item,
    ancestor: Affine,
    skipped: bool,
    depth: usize,
    seen: &mut HashSet<ItemId>,
    proposed: f64,
    tol: f64,
) {
    if depth >= ALIGN_GROUP_DEPTH || !seen.insert(item.id) || item.hidden {
        return;
    }
    let abs = ancestor * item.xf;
    let skip = skipped || align_skips_item(selection, req, item.id);
    if !skip && let Some(angle) = rotation_of(abs) {
        let dist = angular_distance(proposed, angle);
        if dist.is_finite() && dist <= tol && best.is_none_or(|(have, _)| dist < have) {
            *best = Some((dist, angle));
        }
    }
    if !xf_finite(abs) {
        return;
    }
    for child in item.children() {
        push_rotation(best, selection, req, child, abs, skip, depth + 1, seen, proposed, tol);
    }
}

/// Decompose rotation of an absolute transform. A non-finite matrix contributes nothing.
fn rotation_of(xf: Affine) -> Option<f64> {
    if !xf_finite(xf) {
        return None;
    }
    let rotation = designcraft_geom::decompose::decompose(xf).rotation;
    rotation.is_finite().then_some(rotation)
}

/// Decompose rotation of the first selected item's own `xf`, captured before a preview changes it.
/// None when nothing is selected. The command composes onto that `xf`, so ancestors stay out.
pub(crate) fn reference_rotation(cx: &ToolContext) -> Option<f64> {
    let id = *cx.selection.items.first()?;
    rotation_of(cx.doc.item(id)?.xf)
}

/// Degrees folded into the range `decompose` reports, `(-180, 180]`.
pub(crate) fn folded_rotation(deg: f64) -> Option<f64> {
    if !deg.is_finite() {
        return None;
    }
    let mut d = deg.rem_euclid(360.0);
    if !d.is_finite() {
        return None;
    }
    if d > 180.0 {
        d -= 360.0;
    }
    if d <= -180.0 + 1e-9 {
        d = 180.0;
    }
    d.is_finite().then_some(d)
}

/// Delta `transform.rotate` should apply so the reference lands on `snapped`.
///
/// `transform.rotate` composes, so the command is `snapped - start`, not `snapped`.
/// Whole turns are added so the result stays nearest the raw command delta.
pub(crate) fn rotation_command_delta(start: f64, snapped: f64, raw: f64) -> Option<f64> {
    if !start.is_finite() || !snapped.is_finite() || !raw.is_finite() {
        return None;
    }
    let base = snapped - start;
    if !base.is_finite() {
        return None;
    }
    let turns = ((raw - base) / 360.0).round();
    if !turns.is_finite() {
        return None;
    }
    let delta = base + turns * 360.0;
    delta.is_finite().then_some(delta)
}

/// Smallest absolute difference in degrees, folded into `0..=180`.
fn angular_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if !d.is_finite() {
        return f64::NAN;
    }
    d.min(360.0 - d)
}

/// Whole degrees have no decimal. Anything else prints one decimal place.
fn format_angle(deg: f64) -> String {
    let nearest = deg.round();
    if nearest.is_finite() && (deg - nearest).abs() < 1e-6 { format!("{nearest:.0}") } else { format!("{deg:.1}") }
}
