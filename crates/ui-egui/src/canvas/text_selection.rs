//! Caret, text, and table-cell selection overlays.

use super::{Affine, CanvasLayout, Color32, DesignApp, Document, Point, Rect, Stroke, Xf, compose, item_canvas_xf, layer_color, path_screen, pos2};

pub(super) fn draw_text_selection(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    ts: designcraft_doc::TextSel,
    cells: Option<designcraft_doc::TableSel>,
) {
    // Arbitrarily nested review text is opened in the editable review/story window.
    // Its address is not a top-level table cell and must not be drawn as one.
    if ts.cell.and_then(|c| c.review_id()).is_some() {
        return;
    }
    if let Some(id) = ts.cell.and_then(|c| c.annotation_id()) {
        draw_annotation_selection(app, painter, xf, doc, layout, ts, id);
        return;
    }
    let cs = app.session.cache.get(doc, ts.story, None);
    let range = ts.range();
    // Selected table cells: tint every selected cell.
    if let Some(sel) = cells.filter(|c| c.story == ts.story) {
        for ft in &cs.frames {
            let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { continue };
            let m = a * doc.text_xf(it);
            for t in ft.tables.iter().filter(|t| t.table == sel.table) {
                for c in t.cells.iter().filter(|c| sel.range.contains(c.row, c.col)) {
                    let q = [
                        xf.to_screen(m * Point::new(c.rect.x0, c.rect.y0)),
                        xf.to_screen(m * Point::new(c.rect.x1, c.rect.y0)),
                        xf.to_screen(m * Point::new(c.rect.x1, c.rect.y1)),
                        xf.to_screen(m * Point::new(c.rect.x0, c.rect.y1)),
                    ];
                    painter.add(egui::Shape::convex_polygon(q.to_vec(), Color32::from_rgba_unmultiplied(80, 140, 255, 90), Stroke::NONE));
                }
            }
        }
        return;
    }
    if let Some(cell) = ts.cell {
        draw_cell_selection(app, painter, xf, doc, layout, &cs, ts, cell);
        return;
    }
    let mut runs: Vec<(usize, Affine, Vec<[Point; 4]>, Vec<(usize, usize)>)> = Vec::new();
    for (fi, ft) in cs.frames.iter().enumerate() {
        let Some((a, _)) = item_canvas_xf(doc, layout, ft.frame) else { continue };
        let Some(it) = doc.item(ft.frame) else { continue };
        let m = a * doc.text_xf(it);
        // Show the frame edge while typing.
        for poly in path_screen(it, xf, a) {
            painter.add(egui::Shape::line(poly, Stroke::new(1.0, layer_color(doc, it).gamma_multiply(0.6))));
        }
        if range.is_empty() {
            continue;
        }
        let mut quads = Vec::new();
        let mut glyphs = Vec::new();
        for (li, l) in ft.lines.iter().enumerate() {
            let s = range.start.max(l.range.start);
            let e = range.end.min(l.range.end);
            if s > e || (s == e && !(l.range.end < range.end && e == l.range.end)) {
                continue;
            }
            let quad = |x0: f64, x1: f64| {
                [
                    Point::new(x0, l.baseline - l.ascent),
                    Point::new(x1, l.baseline - l.ascent),
                    Point::new(x1, l.baseline + l.descent),
                    Point::new(x0, l.baseline + l.descent),
                ]
            };
            if l.glyphs.iter().any(|g| g.rtl) {
                // Mixed directions: the selected glyphs' boxes, joined where they touch.
                let mut spans: Vec<(f64, f64)> =
                    l.glyphs.iter().filter(|g| g.len > 0 && g.byte >= s && g.byte < e).map(|g| (g.x, g.x + g.adv)).collect();
                spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut merged: Vec<(f64, f64)> = Vec::new();
                for (a, b) in spans {
                    match merged.last_mut() {
                        Some(m) if a <= m.1 + 0.5 => m.1 = m.1.max(b),
                        _ => merged.push((a, b)),
                    }
                }
                quads.extend(merged.into_iter().map(|(a, b)| quad(a, b)));
            } else {
                let x0 = compose::caret_x(l, s);
                let x1 = if e == l.range.end && range.end > l.range.end { l.end_x.max(x0 + 3.0) } else { compose::caret_x(l, e) };
                quads.push(quad(x0, x1));
            }
            for (gi, g) in l.glyphs.iter().enumerate() {
                if g.visible && g.len > 0 && g.byte >= s && g.byte < e {
                    glyphs.push((li, gi));
                }
            }
        }
        if !quads.is_empty() {
            runs.push((fi, m, quads, glyphs));
        }
    }
    if !runs.is_empty() {
        draw_inverse_highlight(app, painter, xf, doc, &cs, ts.story, &runs);
    }
    if ts.is_caret()
        && let Some((fi, x, bl, asc, desc)) = compose::caret(&cs, ts.focus)
        && let Some(ft) = cs.frames.get(fi)
        && let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame))
    {
        let m = a * doc.text_xf(it);
        let blink = (painter.ctx().input(|i| i.time) * 1.6) as i64 % 2 == 0;
        if blink {
            let p0 = xf.to_screen(m * Point::new(x, bl - asc));
            let p1 = xf.to_screen(m * Point::new(x, bl + desc));
            painter.line_segment([p0, p1], Stroke::new(1.0, Color32::BLACK));
        }
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
    }
}

fn draw_annotation_selection(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    ts: designcraft_doc::TextSel,
    id: u64,
) {
    let range = ts.range();
    let mut caret_drawn = false;
    let tokens = crate::theme::Tokens::get(painter.ctx());
    for sid in doc.stories.keys() {
        let cs = app.session.cache.get(doc, *sid, None);
        for (fi, origin, text) in compose::annotation_fragments(&cs, id) {
            let Some(ft) = cs.frames.get(fi) else { continue };
            let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { continue };
            let m = a * doc.text_xf(it) * Affine::translate(origin.to_vec2());
            for f in &text.frames {
                for l in &f.lines {
                    let start = range.start.max(l.range.start);
                    let end = range.end.min(l.range.end);
                    if !range.is_empty() && start < end {
                        let x0 = compose::caret_x(l, start);
                        let x1 = compose::caret_x(l, end);
                        let points = [
                            Point::new(x0, l.baseline - l.ascent),
                            Point::new(x1, l.baseline - l.ascent),
                            Point::new(x1, l.baseline + l.descent),
                            Point::new(x0, l.baseline + l.descent),
                        ]
                        .map(|p| xf.to_screen(m * p));
                        painter.add(egui::Shape::convex_polygon(points.to_vec(), tokens.accent.gamma_multiply(0.3), Stroke::NONE));
                    }
                    if ts.is_caret() && !caret_drawn && l.range.start <= ts.focus && ts.focus <= l.range.end {
                        caret_drawn = true;
                        if (painter.ctx().input(|i| i.time) * 1.6) as i64 % 2 == 0 {
                            let x = compose::caret_x(l, ts.focus);
                            painter.line_segment(
                                [xf.to_screen(m * Point::new(x, l.baseline - l.ascent)), xf.to_screen(m * Point::new(x, l.baseline + l.descent))],
                                Stroke::new(1.0, tokens.text),
                            );
                        }
                        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
                    }
                }
            }
        }
    }
}

/// Caret / text highlight inside a table cell (the cell's lines are offset by the cell origin).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_cell_selection(
    _app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    cs: &compose::ComposedStory,
    ts: designcraft_doc::TextSel,
    cell: designcraft_doc::CellAddr,
) {
    // Footnote text or a cell: its composed story and where it sits in the frame.
    let found = match cell.footnote_id() {
        Some(id) => compose::find_note(cs, id).map(|(fi, n)| (fi, n.origin, n.text.clone())),
        None => compose::find_cell(cs, cell.table, cell.row, cell.col).map(|(fi, _, pc)| (fi, pc.origin, pc.text.clone())),
    };
    let Some((fi, origin, text)) = found else { return };
    let Some(ft) = cs.frames.get(fi) else { return };
    let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { return };
    let m = a * doc.text_xf(it) * designcraft_geom::Affine::translate(origin.to_vec2());
    let range = ts.range();
    if !range.is_empty()
        && let Some(cft) = text.frames.first()
    {
        for l in &cft.lines {
            let s = range.start.max(l.range.start);
            let e = range.end.min(l.range.end);
            if s > e || (s == e && !(l.range.end < range.end && e == l.range.end)) {
                continue;
            }
            let x0 = compose::caret_x(l, s);
            let x1 = if e == l.range.end && range.end > l.range.end { l.end_x.max(x0 + 3.0) } else { compose::caret_x(l, e) };
            let q = [
                xf.to_screen(m * Point::new(x0, l.baseline - l.ascent)),
                xf.to_screen(m * Point::new(x1, l.baseline - l.ascent)),
                xf.to_screen(m * Point::new(x1, l.baseline + l.descent)),
                xf.to_screen(m * Point::new(x0, l.baseline + l.descent)),
            ];
            painter.add(egui::Shape::convex_polygon(q.to_vec(), Color32::from_rgba_unmultiplied(80, 140, 255, 110), Stroke::NONE));
        }
    }
    if ts.is_caret()
        && let Some((_, x, bl, asc, desc)) = match cell.footnote_id() {
            Some(id) => compose::note_caret(cs, id, ts.focus),
            None => compose::cell_caret(cs, cell.table, cell.row, cell.col, ts.focus),
        }
    {
        let m = a * doc.text_xf(it);
        let blink = (painter.ctx().input(|i| i.time) * 1.6) as i64 % 2 == 0;
        if blink {
            painter.line_segment(
                [xf.to_screen(m * Point::new(x, bl - asc)), xf.to_screen(m * Point::new(x, bl + desc))],
                Stroke::new(1.0, Color32::BLACK),
            );
        }
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
    }
}

/// InDesign draws selected text inverted: a black highlight with the selected glyphs in the
/// inverse of their colour (white for black text). The highlight and glyph outlines are
/// rasterised with `vello_cpu` into a small screen-space texture over the selection.
fn draw_inverse_highlight(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    cs: &compose::ComposedStory,
    story: designcraft_doc::StoryId,
    runs: &[(usize, Affine, Vec<[Point; 4]>, Vec<(usize, usize)>)],
) {
    use designcraft_render::vello_cpu::{self, kurbo, peniko};
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let to_screen = |m: Affine, p: Point| xf.to_screen(m * p);
    let mut bbox = Rect::NOTHING;
    for (_, m, quads, _) in runs {
        for q in quads {
            for p in q {
                bbox.extend_with(to_screen(*m, *p));
            }
        }
    }
    let bbox = bbox.intersect(painter.clip_rect());
    if !bbox.is_positive() {
        return;
    }
    // Snap to device pixels so the texture maps 1:1.
    let bbox = Rect::from_min_max(
        pos2((bbox.min.x * ppp).floor() / ppp, (bbox.min.y * ppp).floor() / ppp),
        pos2((bbox.max.x * ppp).ceil() / ppp, (bbox.max.y * ppp).ceil() / ppp),
    );
    let (w, h) = (((bbox.width() * ppp).round() as u32).clamp(1, 8192), ((bbox.height() * ppp).round() as u32).clamp(1, 8192));
    let Some(st) = app.session.active() else { return };
    let key = egui::Id::new(("inverse_sel", st.uid, st.revision, std::sync::Arc::as_ptr(&st.doc) as usize, story.0))
        .with((runs.iter().map(|r| r.2.len() + r.3.len() * 7919).sum::<usize>(), w, h))
        .with((bbox.min.x.to_bits(), bbox.min.y.to_bits(), xf.zoom.to_bits(), st.selection.text.map(|t| (t.anchor, t.focus))));
    let cache_id = egui::Id::new("inverse_sel_tex");
    let cached: Option<(egui::Id, egui::TextureHandle)> = ctx.data(|d| d.get_temp(cache_id));
    let tex = match cached {
        Some((k, t)) if k == key => t,
        _ => {
            let view = xf.affine(ppp as f64, bbox.min);
            let mut rc = vello_cpu::RenderContext::new_with(w as u16, h as u16, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
            rc.set_paint(peniko::Color::from_rgba8(0, 0, 0, 255));
            for (_, m, quads, _) in runs {
                rc.set_transform(view * *m);
                for q in quads {
                    let mut bp = kurbo::BezPath::new();
                    bp.move_to(q[0]);
                    for p in &q[1..] {
                        bp.line_to(*p);
                    }
                    bp.close_path();
                    rc.fill_path(&bp);
                }
            }
            let db = designcraft_fonts::FontDb::global();
            for (fi, m, _, glyphs) in runs {
                let Some(ft) = cs.frames.get(*fi) else { continue };
                for &(li, gi) in glyphs {
                    let Some(l) = ft.lines.get(li) else { continue };
                    let Some(g) = l.glyphs.get(gi) else { continue };
                    let Some(style) = cs.styles.get(g.style as usize) else { continue };
                    let Some(c) = doc.resolve_color(&style.fill, style.fill_tint) else { continue };
                    let [r, gg, b, _] = c.to_rgba8(1.0);
                    rc.set_paint(peniko::Color::from_rgba8(255 - r, 255 - gg, 255 - b, 255));
                    let outline = db.outline(&g.face, g.gid);
                    let skew =
                        if style.skew != 0.0 { Affine::new([1.0, 0.0, -style.skew.to_radians().tan(), 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
                    let ga = Affine::translate((g.x, l.baseline + g.y)) * skew * Affine::scale_non_uniform(g.sx, g.sy);
                    rc.set_transform(view * *m * ga);
                    rc.fill_path(&outline);
                }
            }
            rc.flush();
            let mut pm = vello_cpu::Pixmap::new(w as u16, h as u16);
            let mut res = vello_cpu::Resources::new();
            rc.render(&mut pm, &mut res);
            let ci = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pm.data_as_u8_slice());
            let t = ctx.load_texture("inverse_selection", ci, egui::TextureOptions::NEAREST);
            ctx.data_mut(|d| d.insert_temp(cache_id, (key, t.clone())));
            t
        }
    };
    painter.image(tex.id(), bbox, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}
