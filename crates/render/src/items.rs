//! Draw page-item bodies, strokes, graphics and nested groups.

use super::{
    Affine, Arc, BezPath, BlendMode, Compose, ComposedStory, Content, DcBlend, DisplayQuality, Document, Frame, Item, Mix, Rect, RenderContext,
    Renderer, StoryId, StrokeAlign, StrokeType, Vec2, corners, fx, images, is_pdf, kurbo, pdf_layers, peniko, plate_pixmap, rect_overlaps, vello_cpu,
};
use super::{HIDDEN_LAYERS, PLATE, RICH_BLACK};
use designcraft_geom::kurbo::Shape as _;

impl Renderer {
    pub(super) fn draw_item(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, parent: Affine, page_name: Option<&str>) {
        if it.hidden || f.opts.hidden.contains(&it.id) || f.opts.printing_only && it.nonprinting {
            return;
        }
        let xf = parent * it.xf;
        let vb = xf.transform_rect_bbox(it.inner_bounds()).inflate(it.stroke.extent() + 2.0, it.stroke.extent() + 2.0);
        if it.has_nested_items() {
            // A frame with items pasted into it: fill, the items clipped to the frame, stroke.
            if !rect_overlaps(vb, f.visible) {
                return;
            }
            let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
            let layered = it.opacity < 0.999 || it.blend != DcBlend::Normal;
            if layered {
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_layer(None, Some(blend_mode(it.blend)), Some(it.opacity), None, None);
            }
            if !it.fill.is_none() {
                ctx.set_transform(f.view * xf);
                if set_fill_paint(ctx, f.doc, &it.fill, bp.bounding_box()) {
                    ctx.fill_path(&bp);
                }
                ctx.reset_paint_transform();
            }
            ctx.set_transform(f.view * xf);
            ctx.push_clip_layer(&bp);
            for c in it.children() {
                self.draw_item(ctx, f, c, xf, page_name);
            }
            ctx.pop_layer();
            if !it.stroke.is_none() {
                self.draw_stroke(ctx, f, it, &bp, xf);
            }
            if layered {
                ctx.pop_layer();
            }
            return;
        }
        if !it.children().is_empty() {
            // Groups: children carry their own transforms relative to the group.
            // Isolate Blending / Knockout: the group is its own layer (blend modes stop at it).
            let layered = it.opacity < 0.999 || it.blend != DcBlend::Normal || it.isolate || it.knockout;
            if layered {
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_layer(None, Some(blend_mode(it.blend)), Some(it.opacity), None, None);
            }
            if it.knockout {
                // Knockout: each object first clears its area of those behind it, so it composites
                // with the group's backdrop rather than with its siblings.
                for c in it.shown_children() {
                    if !c.hidden {
                        ctx.set_transform(Affine::IDENTITY);
                        ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
                        ctx.set_transform(f.view * xf * c.xf);
                        ctx.set_paint(peniko::Color::BLACK);
                        ctx.fill_path(&c.path.to_bezpath());
                        ctx.pop_layer();
                    }
                    self.draw_item(ctx, f, c, xf, page_name);
                }
            } else {
                for c in it.shown_children() {
                    self.draw_item(ctx, f, c, xf, page_name);
                }
            }
            if layered {
                ctx.pop_layer();
            }
            return;
        }
        // Composed text stays inside its frame apart from glyph overhang (italics, swashes, big
        // initials), so text frames get a margin rather than being exempt from culling.
        let margin = match it.content {
            Content::Text(_) => 36.0 + 0.25 * vb.width().max(vb.height()),
            _ => 0.0,
        };
        let effects = it.effects.any() && f.opts.quality != DisplayQuality::Fast;
        let fx_outset = if effects { fx::effect_outset(it) } else { 0.0 };
        if !rect_overlaps(vb.inflate(margin + fx_outset, margin + fx_outset), f.visible) {
            return;
        }
        self.stats.items += 1;
        let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
        let layered = it.opacity < 0.999 || it.blend != DcBlend::Normal;
        if layered {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(blend_mode(it.blend)), Some(it.opacity), None, None);
        }
        if effects {
            self.draw_item_fx(ctx, f, it, &bp, xf, page_name);
        } else {
            self.draw_body(ctx, f, it, &bp, xf, page_name);
        }
        if layered {
            ctx.pop_layer();
        }
    }

    /// The composed story for a frame (memoised for the current render: the cache lookup
    /// validates the whole thread each time).
    pub(super) fn story(&mut self, f: &Frame, sid: StoryId, page_name: Option<&str>) -> Arc<ComposedStory> {
        let key = (sid, page_name.map(str::to_string));
        if let Some(cs) = self.stories.get(&key) {
            return cs.clone();
        }
        let cs = f.cache.get(f.doc, sid, page_name);
        self.stories.insert(key, cs.clone());
        cs
    }

    /// Fill, content and stroke of a leaf item (no opacity, blend or effects).
    pub(crate) fn draw_body(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine, page_name: Option<&str>) {
        let doc = f.doc;
        // Fill.
        if !it.fill.is_none() {
            ctx.set_transform(f.view * xf);
            let op = overprints(f, &it.fill.swatch, it.fill.tint, it.fill.overprint);
            if op {
                ctx.push_layer(None, Some(BlendMode::new(Mix::Multiply, Compose::SrcOver)), None, None, None);
            }
            if set_fill_paint(ctx, doc, &it.fill, bp.bounding_box()) && it.path.is_closed() {
                ctx.fill_path(bp);
            }
            ctx.reset_paint_transform();
            if op {
                ctx.pop_layer();
            }
        }
        // Content.
        match &it.content {
            Content::Graphic(g) => {
                let g = &if it.media.is_some() { it.drawn_graphic(&f.doc.assets).unwrap_or_else(|| g.clone()) } else { g.clone() };
                ctx.set_transform(f.view * xf);
                ctx.push_clip_layer(bp);
                if !it.pdf_hidden_layers.is_empty() {
                    HIDDEN_LAYERS.with(|h| *h.borrow_mut() = it.pdf_hidden_layers.clone());
                }
                self.draw_graphic(ctx, f, g, xf);
                if !it.pdf_hidden_layers.is_empty() {
                    HIDDEN_LAYERS.with(|h| h.borrow_mut().clear());
                }
                ctx.pop_layer();
            }
            Content::Text(tf) => {
                let cs = self.story(f, tf.story, page_name);
                if let Some(ft) = cs.frame(it.id) {
                    match &tf.options.path {
                        Some(pt) => self.draw_path_text(ctx, f, &cs, ft, xf, bp, pt),
                        None => self.draw_text(ctx, f, &cs, ft, xf * f.doc.text_local(it)),
                    }
                }
            }
            _ => {}
        }
        // Form fields (screen only; PDF viewers draw their own): a tint, a check mark.
        if let Some(ff) = &it.form_field
            && !f.opts.printing_only
        {
            let r = it.inner_bounds();
            ctx.set_transform(f.view * xf);
            ctx.set_paint(peniko::Color::from_rgba8(80, 140, 255, 40));
            ctx.fill_rect(&r);
            ctx.set_paint(peniko::Color::from_rgba8(60, 110, 220, 200));
            ctx.set_stroke(kurbo::Stroke::new(0.75));
            ctx.stroke_path(&r.to_path(0.1));
            if ff.kind == designcraft_doc::FieldKind::CheckBox && !ff.value.is_empty() && ff.value != "Off" {
                let (w, h) = (r.width(), r.height());
                let mut c = BezPath::new();
                c.move_to((r.x0 + w * 0.22, r.y0 + h * 0.48));
                c.line_to((r.x0 + w * 0.42, r.y0 + h * 0.72));
                c.line_to((r.x0 + w * 0.78, r.y0 + h * 0.24));
                ctx.set_paint(peniko::Color::BLACK);
                ctx.set_stroke(kurbo::Stroke::new((w.min(h) * 0.12).max(0.8)).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round));
                ctx.stroke_path(&c);
            }
        }
        // Stroke.
        if !it.stroke.is_none() {
            let op = overprints(f, &it.stroke.swatch, it.stroke.tint, it.stroke.overprint);
            if op {
                ctx.set_transform(f.view * xf);
                ctx.push_layer(None, Some(BlendMode::new(Mix::Multiply, Compose::SrcOver)), None, None, None);
            }
            self.draw_stroke(ctx, f, it, bp, xf);
            if op {
                ctx.pop_layer();
            }
        }
    }

    pub(super) fn draw_stroke(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine) {
        let st = &it.stroke;
        let Some(c) = f.doc.resolve_color(&st.swatch, st.tint) else { return };
        let mut stroke = kurbo::Stroke::new(st.weight)
            .with_caps(match st.cap {
                designcraft_doc::Cap::Butt => kurbo::Cap::Butt,
                designcraft_doc::Cap::Round => kurbo::Cap::Round,
                designcraft_doc::Cap::Projecting => kurbo::Cap::Square,
            })
            .with_join(match st.join {
                designcraft_doc::Join::Miter => kurbo::Join::Miter,
                designcraft_doc::Join::Round => kurbo::Join::Round,
                designcraft_doc::Join::Bevel => kurbo::Join::Bevel,
            })
            .with_miter_limit(st.miter_limit);
        let kind = f.doc.stroke_kind(&st.kind);
        match &kind {
            StrokeType::Dashed { pattern } if !pattern.is_empty() => stroke = stroke.with_dashes(0.0, pattern.iter().copied()),
            StrokeType::Dotted => stroke = stroke.with_dashes(0.0, [0.0, st.weight * 2.0]).with_caps(kurbo::Cap::Round),
            _ => {}
        }
        ctx.set_transform(f.view * xf);
        ctx.set_paint(color_of(&c, 1.0));
        let closed = it.path.is_closed();
        // Arrowheads: the path is shortened under them; they're drawn after it.
        let arrows = designcraft_doc::arrow::apply(bp, st, closed);
        let bp = arrows.as_ref().map_or(bp, |a| &a.0);
        // Gap colour under dashes and dots: the whole path, undashed.
        let gap = if matches!(kind, StrokeType::Solid) { None } else { f.doc.resolve_color(&st.gap_swatch, st.gap_tint) };
        if let Some(g) = gap
            && !matches!(st.align, StrokeAlign::Inside | StrokeAlign::Outside if closed)
        {
            let op = overprints(f, &st.gap_swatch, st.gap_tint, st.gap_overprint);
            if op {
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_layer(None, Some(BlendMode::new(Mix::Multiply, Compose::SrcOver)), None, None, None);
                ctx.set_transform(f.view * xf);
            }
            ctx.set_paint(color_of(&g, 1.0));
            ctx.set_stroke(kurbo::Stroke {
                dash_pattern: Default::default(),
                start_cap: kurbo::Cap::Butt,
                end_cap: kurbo::Cap::Butt,
                ..stroke.clone()
            });
            ctx.stroke_path(bp);
            if op {
                ctx.pop_layer();
            }
            ctx.set_paint(color_of(&c, 1.0));
        }
        // Stripes, wavy and hash strokes are fills (along the path's centre line).
        if let Some(o) = kind.outline(bp, st.weight, 0.05 * f.px) {
            ctx.fill_path(&o);
            for h in arrows.iter().flat_map(|a| &a.1) {
                match h.outline {
                    Some(w) => {
                        ctx.set_stroke(kurbo::Stroke::new(w).with_join(kurbo::Join::Miter));
                        ctx.stroke_path(&h.path);
                    }
                    None => ctx.fill_path(&h.path),
                }
            }
            return;
        }
        match st.align {
            StrokeAlign::Inside if closed => {
                // Clip a double-width stroke to the path.
                ctx.push_clip_layer(bp);
                ctx.set_stroke(kurbo::Stroke { width: st.weight * 2.0, ..stroke });
                ctx.stroke_path(bp);
                ctx.pop_layer();
            }
            StrokeAlign::Outside if closed => {
                // Subtract the filled path from a double-width stroke. Combining a stroked
                // outline (already hollow) with the path using even-odd would paint its centre.
                // Isolate the stroke so the subtraction leaves the item's fill/content and
                // backdrop intact; the same nonzero fill rule also preserves compound holes.
                ctx.push_layer(None, None, None, None, None);
                ctx.set_stroke(kurbo::Stroke { width: st.weight * 2.0, ..stroke });
                ctx.stroke_path(bp);
                ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
                ctx.fill_path(bp);
                ctx.pop_layer();
                ctx.pop_layer();
            }
            _ => {
                ctx.set_stroke(stroke);
                ctx.stroke_path(bp);
            }
        }
        for h in arrows.iter().flat_map(|a| &a.1) {
            match h.outline {
                Some(w) => {
                    ctx.set_stroke(kurbo::Stroke::new(w).with_join(kurbo::Join::Miter));
                    ctx.stroke_path(&h.path);
                }
                None => ctx.fill_path(&h.path),
            }
        }
    }

    pub(crate) fn draw_graphic(&mut self, ctx: &mut RenderContext, f: &Frame, g: &designcraft_doc::Graphic, xf: Affine) {
        let Some(asset) = f.doc.assets.get(&g.asset) else { return };
        if designcraft_doc::media_kind(&asset.mime).is_some() {
            // Video / sound: a dark frame with a play mark (the poster, when set, is drawn by the item).
            ctx.set_transform(f.view * xf * g.xf);
            ctx.set_paint(peniko::Color::from_rgba8(48, 48, 52, 255));
            ctx.fill_rect(&Rect::new(0.0, 0.0, g.size.0, g.size.1));
            let (cx, cy, r) = (g.size.0 / 2.0, g.size.1 / 2.0, g.size.0.min(g.size.1) * 0.18);
            let mut tri = BezPath::new();
            tri.move_to((cx - r * 0.6, cy - r));
            tri.line_to((cx + r, cy));
            tri.line_to((cx - r * 0.6, cy + r));
            tri.close_path();
            ctx.set_paint(peniko::Color::from_rgba8(235, 235, 235, 255));
            ctx.fill_path(&tri);
            return;
        }
        if f.opts.quality == DisplayQuality::Fast {
            ctx.set_transform(f.view * xf * g.xf);
            ctx.set_paint(peniko::Color::from_rgba8(178, 178, 178, 255));
            ctx.fill_rect(&Rect::new(0.0, 0.0, g.size.0, g.size.1));
            return;
        }
        // Pick a mip level close to the on-screen size (Typical: a 72 ppi proxy).
        let on_screen = (f.view * xf * g.xf).determinant().abs().sqrt();
        let on_screen = if f.opts.quality == DisplayQuality::Typical { on_screen.min(1.0) } else { on_screen };
        let hidden = HIDDEN_LAYERS.with(|h| h.borrow().clone());
        let data = if !hidden.is_empty() && is_pdf(&asset.data) { pdf_layers::layered(&asset.data, &hidden) } else { asset.data.clone() };
        let Some(pm) = images::mip(&data, asset.page, g.size.0, on_screen) else { return };
        let pm = match PLATE.with(|p| p.get()) {
            Some(plate) => plate_pixmap(&pm, plate),
            None => pm,
        };
        let rect = Rect::new(0.0, 0.0, g.size.0, g.size.1);
        ctx.set_transform(f.view * xf * g.xf);
        let sx = g.size.0 / pm.width().max(1) as f64;
        let sy = g.size.1 / pm.height().max(1) as f64;
        ctx.set_paint(vello_cpu::Image {
            image: vello_cpu::ImageSource::Pixmap(pm),
            sampler: peniko::ImageSampler::default().with_quality(peniko::ImageQuality::Medium),
        });
        ctx.set_paint_transform(Affine::scale_non_uniform(sx, sy));
        ctx.fill_rect(&rect);
        ctx.reset_paint_transform();
    }
}

pub fn color_of(c: &designcraft_color::Color, alpha: f32) -> peniko::Color {
    if let Some(p) = PLATE.with(|p| p.get()) {
        let cmyk = match *c {
            designcraft_color::Color::Cmyk { c, m, y, k } => [c, m, y, k],
            _ => c.to_cmyk_managed(designcraft_color::cms::Intent::RelativeColorimetric),
        };
        let v = ((1.0 - cmyk[p.min(3) as usize].clamp(0.0, 1.0)) * 255.0).round() as u8;
        return peniko::Color::from_rgba8(v, v, v, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    if RICH_BLACK.with(|r| r.get())
        && let designcraft_color::Color::Cmyk { c: cc, m, y, k } = *c
        && cc <= 1e-3
        && m <= 1e-3
        && y <= 1e-3
        && k >= 0.999
    {
        return peniko::Color::from_rgba8(0, 0, 0, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    let [r, g, b, a] = c.to_rgba8(alpha);
    peniko::Color::from_rgba8(r, g, b, a)
}

/// Set a swatch fill (solid or gradient) as the paint. Returns false for [None]/unknown.
fn set_fill_paint(ctx: &mut RenderContext, doc: &Document, fill: &designcraft_doc::Fill, bounds: Rect) -> bool {
    let (swatch, tint, angle) = (fill.swatch.as_str(), fill.tint, fill.gradient_angle);
    if let Some(g) = designcraft_color::swatch::resolve_gradient(&doc.swatches, swatch) {
        let stops: Vec<peniko::ColorStop> = g.expanded_stops().iter().map(|(o, c, a)| peniko::ColorStop::from((*o, color_of(c, *a)))).collect();
        if stops.is_empty() {
            return false;
        }
        let c = bounds.center();
        let grad = match (g.kind, fill.gradient_vector) {
            (designcraft_color::GradientKind::Radial, Some([x0, y0, x1, y1])) => {
                let r = Vec2::new(x1 - x0, y1 - y0).hypot().max(1e-3);
                peniko::Gradient::new_radial((x0, y0), r as f32).with_stops(stops.as_slice())
            }
            (_, Some([x0, y0, x1, y1])) => peniko::Gradient::new_linear((x0, y0), (x1, y1)).with_stops(stops.as_slice()),
            (designcraft_color::GradientKind::Radial, None) => {
                let r = bounds.width().max(bounds.height()) / 2.0;
                peniko::Gradient::new_radial(c, r as f32).with_stops(stops.as_slice())
            }
            _ => {
                let a = angle.unwrap_or(0.0).to_radians();
                let half = (bounds.width() * a.cos().abs() + bounds.height() * a.sin().abs()) / 2.0;
                let d = Vec2::new(a.cos(), -a.sin()) * half;
                peniko::Gradient::new_linear(c - d, c + d).with_stops(stops.as_slice())
            }
        };
        ctx.set_paint(grad);
        return true;
    }
    match doc.resolve_color(swatch, tint) {
        Some(c) => {
            ctx.set_paint(color_of(&c, 1.0));
            true
        }
        None => false,
    }
}

/// Overprint Preview: does this paint overprint (its own flag, or 100% [Black], which
/// overprints by default)? It's then multiplied over what's beneath.
pub(crate) fn overprints(f: &Frame, swatch: &str, tint: f32, flag: bool) -> bool {
    f.opts.overprint_preview && (flag || (f.doc.settings.overprint_black && swatch == designcraft_color::swatch::BLACK && tint >= 0.999))
}

pub fn blend_mode(b: DcBlend) -> BlendMode {
    use DcBlend as B;
    let mix = match b {
        B::Normal => Mix::Normal,
        B::Darken => Mix::Darken,
        B::Multiply => Mix::Multiply,
        B::ColorBurn => Mix::ColorBurn,
        B::Lighten => Mix::Lighten,
        B::Screen => Mix::Screen,
        B::ColorDodge => Mix::ColorDodge,
        B::Overlay => Mix::Overlay,
        B::SoftLight => Mix::SoftLight,
        B::HardLight => Mix::HardLight,
        B::Difference => Mix::Difference,
        B::Exclusion => Mix::Exclusion,
        B::Hue => Mix::Hue,
        B::Saturation => Mix::Saturation,
        B::Color => Mix::Color,
        B::Luminosity => Mix::Luminosity,
    };
    BlendMode::new(mix, Compose::SrcOver)
}
