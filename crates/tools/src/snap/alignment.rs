//! Page, item, parent, and group alignment candidates.

use super::{
    Axis, AxisHit, Best, HashSet, Item, ItemId, PageSide, Rect, Selection, SnapRequest, SpreadId, SpreadRef, ToolContext, axis_flags, offer_box,
    rect_finite, shift_x, shown_parent_page_index,
};

/// How deep a group may nest before alignment stops walking it.
pub(super) const ALIGN_GROUP_DEPTH: usize = 32;

/// Page and item edges and centers. Draws one [`Overlay::Guide`] per winning axis.
///
/// Edges match edges only while `align_edges` is on. Centers match centers only while
/// `align_centers` is on, and only when the request's middle flag is set. Hidden items are
/// skipped. Without copying, excluded items are skipped. With copying, that list is the
/// drag-start set and stays a target. Live selection items outside that set are preview
/// copies and are skipped. A group with children uses the union of each child's visible bounds
/// after that child's transform, then the group's transform. Transform each child, then union.
/// It does not rotate the local union as one rectangle, and it does not use the group's own stroke.
/// Parent items are added for a document page that shows them, in document spread space.
/// A layout built for parent editing already has those items on the spread, so they are not
/// added again.
pub(super) fn pass_align(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !cx.snap.smart_guides || (!cx.snap.align_edges && !cx.snap.align_centers) {
        return None;
    }
    let flags = axis_flags(req, axis);
    let mut best: Option<Best> = None;
    if let Some(sp) = cx.doc.spread(req.spread) {
        for page in &sp.pages {
            offer_box(&mut best, req.rect, flags, axis, page.bounds(), tol, cx.snap.align_edges, cx.snap.align_centers);
        }
        for item in &sp.items {
            let Some(rect) = item_target_rect(cx.selection, req, item) else { continue };
            offer_box(&mut best, req.rect, flags, axis, rect, tol, cx.snap.align_edges, cx.snap.align_centers);
        }
    }
    if !layout_edits_parents(cx) {
        offer_parent_items(&mut best, cx, req, flags, axis, tol);
    }
    best.map(|b| b.hit)
}

/// Drag-start ids stay targets while copying. Preview copies are the live selection minus that set.
pub(super) fn align_skips_item(selection: &Selection, req: &SnapRequest<'_>, id: ItemId) -> bool {
    if req.copying { selection.items.contains(&id) && !req.exclude.contains(&id) } else { req.exclude.contains(&id) }
}

/// Parent spreads are the canvas while a parent is being edited. Their items are ordinary targets.
pub(super) fn layout_edits_parents(cx: &ToolContext) -> bool {
    cx.layout.slots.iter().any(|slot| matches!(slot.spread, SpreadRef::Parent(_)))
}

/// Visible box used for alignment. Groups with children contribute the union of the children.
pub(super) fn item_target_rect(selection: &Selection, req: &SnapRequest<'_>, item: &Item) -> Option<Rect> {
    if item.hidden || align_skips_item(selection, req, item.id) {
        return None;
    }
    if item.is_group() && !item.children().is_empty() {
        let mut seen = HashSet::new();
        return group_target_rect(selection, req, item, &mut seen, 0);
    }
    let bounds = item.visible_bounds();
    rect_finite(bounds).then_some(bounds)
}

/// Union of each child's visible bounds transformed by `item.xf`.
///
/// Transform each child, then union. Rotating the local union as one rectangle is larger when
/// a rotated group's children do not fill that rectangle. Hidden and excluded children stay out.
/// Nested groups use the same rule. A child's stroke is part of that child's visible bounds.
/// The group's own stroke is not added.
fn group_target_rect(selection: &Selection, req: &SnapRequest<'_>, item: &Item, seen: &mut HashSet<ItemId>, depth: usize) -> Option<Rect> {
    group_box(item, seen, depth, Some((selection, req)))
}

/// Visible box of an item being moved. Groups use the same box as alignment, with no exclude list.
pub(crate) fn moving_bounds(item: &Item) -> Option<Rect> {
    if item.is_group() && !item.children().is_empty() {
        let mut seen = HashSet::new();
        return group_box(item, &mut seen, 0, None);
    }
    let bounds = item.visible_bounds();
    rect_finite(bounds).then_some(bounds)
}

fn group_box(item: &Item, seen: &mut HashSet<ItemId>, depth: usize, filter: Option<(&Selection, &SnapRequest<'_>)>) -> Option<Rect> {
    if depth >= ALIGN_GROUP_DEPTH || !seen.insert(item.id) {
        return None;
    }
    let mut acc: Option<Rect> = None;
    for child in item.children() {
        if child.hidden {
            continue;
        }
        if let Some((selection, req)) = filter
            && align_skips_item(selection, req, child.id)
        {
            continue;
        }
        let local = if child.is_group() && !child.children().is_empty() {
            group_box(child, seen, depth + 1, filter)
        } else {
            let bounds = child.visible_bounds();
            rect_finite(bounds).then_some(bounds)
        };
        let Some(local) = local else { continue };
        if !rect_finite(local) {
            continue;
        }
        let placed = item.xf.transform_rect_bbox(local);
        if !rect_finite(placed) {
            continue;
        }
        acc = Some(match acc {
            Some(have) => have.union(placed),
            None => placed,
        });
    }
    acc.filter(|r| rect_finite(*r))
}

/// Parent items shown on the document spread, shifted into that spread's space.
fn offer_parent_items(best: &mut Option<Best>, cx: &ToolContext, req: &SnapRequest<'_>, flags: [bool; 3], axis: Axis, tol: f64) {
    let SpreadRef::Doc(si) = req.spread else { return };
    let page_count = cx.doc.spreads.get(si).map(|sp| sp.pages.len()).unwrap_or(0);
    let first = cx.doc.first_page_of_spread(si);
    for pi in 0..page_count {
        let Some(abs) = first.checked_add(pi) else { continue };
        let Some(page) = cx.doc.page(abs) else { continue };
        if !page.show_parent_items {
            continue;
        }
        let doc_x = page.x;
        let doc_side = page.side;
        let overridden = page.overridden.clone();
        let Some((ppi, _)) = cx.doc.parent_page_for(abs) else { continue };
        let mut seen = HashSet::new();
        offer_parent_chain(best, cx, req, flags, axis, tol, doc_x, doc_side, &overridden, ppi, &mut seen);
    }
}

/// Walk `based_on`. A repeated spread id ends the walk.
fn offer_parent_chain(
    best: &mut Option<Best>,
    cx: &ToolContext,
    req: &SnapRequest<'_>,
    flags: [bool; 3],
    axis: Axis,
    tol: f64,
    doc_x: f64,
    doc_side: PageSide,
    overridden: &[ItemId],
    index: usize,
    seen: &mut HashSet<SpreadId>,
) {
    let Some(parent) = cx.doc.parents.get(index) else { return };
    if !seen.insert(parent.id) {
        return;
    }
    let next = parent.parent.as_ref().and_then(|info| info.based_on);
    if let Some(page_idx) = shown_parent_page_index(parent.pages.len(), doc_side)
        && let Some(parent_page) = parent.pages.get(page_idx)
    {
        let dx = doc_x - parent_page.x;
        if dx.is_finite() {
            for item in &parent.items {
                if overridden.contains(&item.id) {
                    continue;
                }
                // Same other-page skip as the renderer: a facing parent only shows items
                // whose center sits on the parent page this document page uses.
                if parent.pages.len() > 1 && parent.page_at_x(item.bounds().center().x) != Some(page_idx) {
                    continue;
                }
                let Some(rect) = item_target_rect(cx.selection, req, item) else { continue };
                offer_box(best, req.rect, flags, axis, shift_x(rect, dx), tol, cx.snap.align_edges, cx.snap.align_centers);
            }
        }
    }
    let Some(next_id) = next else { return };
    let Some(next_index) = cx.doc.parent_index(next_id) else { return };
    offer_parent_chain(best, cx, req, flags, axis, tol, doc_x, doc_side, overridden, next_index, seen);
}
