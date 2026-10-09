//! Document grids, ruler guides, and guide dragging.

use super::{CanvasLayout, Color32, DRect, DesignApp, Document, PageSide, Point, Pos2, Rect, Stroke, StrokeKind, Tokens, Xf, c32, hair, json};

pub(super) fn draw_guides(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout, t: &Tokens) {
    let s = &doc.settings;
    for slot in &layout.slots {
        let Some(sp) = doc.spread(slot.spread) else { continue };
        // Everything below is in spread coordinates, placed through the slot (which may be turned).
        let m = slot.xf;
        let pt = |p: Point| xf.to_screen(m * p);
        let rect = |r: DRect| xf.rect(m.transform_rect_bbox(r));
        // Bleed: one rectangle around the spread (inside bleed only applies at the spread's outer edges).
        let b = s.bleed;
        if app.ui.guides && b.iter().any(|v| *v > 0.0) && !sp.pages.is_empty() {
            let first = &sp.pages[0];
            let last = &sp.pages[sp.pages.len() - 1];
            let l = if first.side == PageSide::Left { b[3] } else { b[2] };
            let r = if last.side == PageSide::Right || last.side == PageSide::Single { b[3] } else { b[2] };
            let sb = sp.bounds();
            let br = DRect::new(sb.x0 - l, sb.y0 - b[0], sb.x1 + r, sb.y1 + b[1]);
            painter.rect_stroke(rect(br), 0.0, Stroke::new(hair(painter), c32(s.bleed_color)), StrokeKind::Middle);
        }
        let pasteboard = m.inverse().transform_rect_bbox(layout.pasteboard);
        for p in &sp.pages {
            let pr = p.bounds();
            if !app.ui.guides {
                continue;
            }
            // Baseline grid.
            if app.ui.baseline_grid && xf.zoom >= s.baseline_grid.view_threshold {
                let g = &s.baseline_grid;
                let mut y = g.start;
                while y < p.height {
                    painter.line_segment(
                        [pt(Point::new(pr.x0, pr.y0 + y)), pt(Point::new(pr.x1, pr.y0 + y))],
                        Stroke::new(hair(painter), c32(g.color).gamma_multiply(0.8)),
                    );
                    y += g.increment.max(1.0);
                }
            }
            // Margins (magenta) and columns (violet).
            painter.rect_stroke(rect(p.margin_rect()), 0.0, Stroke::new(hair(painter), c32(s.margin_color)), StrokeKind::Middle);
            let cols = p.column_rects();
            if cols.len() > 1 {
                for (i, c) in cols.iter().enumerate() {
                    let col = Stroke::new(hair(painter), c32(s.column_color));
                    if i > 0 {
                        painter.line_segment([pt(Point::new(c.x0, c.y0)), pt(Point::new(c.x0, c.y1))], col);
                    }
                    if i + 1 < cols.len() {
                        painter.line_segment([pt(Point::new(c.x1, c.y0)), pt(Point::new(c.x1, c.y1))], col);
                    }
                }
            }
            for g in p.guides.iter().filter(|g| g.visible_in(doc)) {
                // Spread guides cross the pasteboard; page guides their page.
                let span = if g.spread { pasteboard } else { pr };
                let (a, bb) = match g.orientation {
                    designcraft_doc::Orientation::Horizontal => (Point::new(span.x0, g.position), Point::new(span.x1, g.position)),
                    designcraft_doc::Orientation::Vertical => {
                        let span = if g.spread { sp.bounds().inflate(0.0, 36.0) } else { pr };
                        (Point::new(g.position, span.y0), Point::new(g.position, span.y1))
                    }
                };
                let stroke = Stroke::new(hair(painter), Color32::from_rgb(74, 227, 255));
                if g.liquid {
                    // Liquid guides are dashed.
                    painter.extend(egui::Shape::dashed_line(&[pt(a), pt(bb)], stroke, 6.0, 3.0));
                } else {
                    painter.line_segment([pt(a), pt(bb)], stroke);
                }
            }
        }
    }
    let _ = t;
}

pub(super) const GUIDE_DRAG: &str = "canvas_guide_drag";

/// A ruler guide being dragged: out of a ruler (`from` None) or an existing one.
#[derive(Clone, Debug)]
pub(super) struct GuideDrag {
    pub(super) orientation: designcraft_doc::Orientation,
    from: Option<(designcraft_doc::SpreadRef, usize, usize)>,
    /// Pointer position (screen).
    pub(super) at: Option<Pos2>,
}

/// The guide under a screen point (within 3 px), as (spread, page, index, orientation).
fn guide_at(app: &DesignApp, xf: &Xf, p: Pos2) -> Option<(designcraft_doc::SpreadRef, usize, usize, designcraft_doc::Orientation)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let cp = xf.to_canvas(p);
    let tol = 3.0 / xf.zoom;
    for slot in &layout.slots {
        let sp = st.doc.spread(slot.spread)?;
        let local = slot.to_spread(cp);
        for (pi, pg) in sp.pages.iter().enumerate() {
            let pr = if pg.guides.iter().any(|g| g.spread) { slot.xf.inverse().transform_rect_bbox(layout.pasteboard) } else { pg.bounds() };
            for (gi, g) in pg.guides.iter().enumerate() {
                if !g.editable_in(&st.doc) {
                    continue;
                }
                let r = if g.spread { pr } else { pg.bounds() };
                let hit = match g.orientation {
                    designcraft_doc::Orientation::Horizontal => (local.y - g.position).abs() <= tol && local.x >= r.x0 && local.x <= r.x1,
                    designcraft_doc::Orientation::Vertical => (local.x - g.position).abs() <= tol && local.y >= r.y0 - tol && local.y <= r.y1 + tol,
                };
                if hit && !g.locked {
                    return Some((slot.spread, pi, gi, g.orientation));
                }
            }
        }
    }
    None
}

/// Ruler guides: drag one out of a ruler, drag an existing one (Selection tools, not over an
/// object) to move it, drop it back on a ruler to delete it. Returns true while it owns the pointer.
pub(super) fn guide_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf) -> bool {
    let id = egui::Id::new(GUIDE_DRAG);
    let (pressed, released, origin, latest) =
        ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin(), i.pointer.latest_pos()));
    let mut drag: Option<GuideDrag> = ui.data(|d| d.get_temp(id));
    if drag.is_none() && pressed && resp.hovered() && app.ui.guides && !app.ui.guides_locked {
        let Some(o) = origin else { return false };
        let full = resp.rect;
        if full.contains(o) && !rect.contains(o) {
            // In a ruler (not the corner).
            let orientation = if o.y < rect.min.y && o.x >= rect.min.x {
                Some(designcraft_doc::Orientation::Horizontal)
            } else if o.x < rect.min.x && o.y >= rect.min.y {
                Some(designcraft_doc::Orientation::Vertical)
            } else {
                None
            };
            if let Some(orientation) = orientation {
                drag = Some(GuideDrag { orientation, from: None, at: Some(o) });
            }
        } else if rect.contains(o)
            && matches!(app.session.tool_id(), "selection" | "directSelection")
            && let Some((r, pi, gi, orientation)) = guide_at(app, xf, o)
        {
            // Objects take precedence over the guides behind them.
            let over_item = app.session.active().is_some_and(|st| {
                let layout = CanvasLayout::new(&st.doc, st.editing_parents);
                layout.spread_at(xf.to_canvas(o)).is_some_and(|(sr, p)| match sr {
                    designcraft_doc::SpreadRef::Doc(si) => st.doc.hit_item(si, p, 3.0 / xf.zoom).is_some(),
                    _ => false,
                })
            });
            if !over_item {
                drag = Some(GuideDrag { orientation, from: Some((r, pi, gi)), at: Some(o) });
            }
        }
    }
    let Some(mut g) = drag else { return false };
    if let Some(p) = latest {
        g.at = Some(p);
    }
    if !released {
        ui.ctx().set_cursor_icon(match g.orientation {
            designcraft_doc::Orientation::Horizontal => egui::CursorIcon::ResizeVertical,
            designcraft_doc::Orientation::Vertical => egui::CursorIcon::ResizeHorizontal,
        });
        ui.data_mut(|d| d.insert_temp(id, g));
        return true;
    }
    ui.data_mut(|d| d.remove::<GuideDrag>(id));
    let Some(p) = g.at else { return true };
    let on_canvas = rect.contains(p);
    let target = app.session.active().and_then(|st| {
        let layout = CanvasLayout::new(&st.doc, st.editing_parents);
        let (r, sp) = layout.spread_at(xf.to_canvas(p))?;
        let spread = st.doc.spread(r)?;
        let on_page = spread.pages.iter().position(|pg| pg.bounds().contains(sp));
        Some((r, sp, on_page))
    });
    let pos_of = |sp: Point| match g.orientation {
        designcraft_doc::Orientation::Horizontal => sp.y,
        designcraft_doc::Orientation::Vertical => sp.x,
    };
    let orient = match g.orientation {
        designcraft_doc::Orientation::Horizontal => "horizontal",
        designcraft_doc::Orientation::Vertical => "vertical",
    };
    match (g.from, on_canvas, target) {
        // Dropped back on a ruler: delete.
        (Some((r, pi, gi)), false, _) => {
            let _ = app.run("guide.delete", json!({"spread": r, "page": pi, "index": gi}));
        }
        (Some((r, pi, gi)), true, Some((tr, sp, _))) if tr == r => {
            let _ = app.run("guide.move", json!({"spread": r, "page": pi, "index": gi, "position": pos_of(sp)}));
        }
        (None, true, Some((r, sp, on_page))) => {
            let mut params = json!({"spread": r, "orientation": orient, "position": pos_of(sp), "at": sp.x, "spreadGuide": on_page.is_none()});
            if let Some(pi) = on_page {
                params["page"] = json!(pi);
            }
            let _ = app.run("guide.add", params);
        }
        _ => {}
    }
    true
}
