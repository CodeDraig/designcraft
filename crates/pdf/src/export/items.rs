//! Layer, group, and item traversal for PDF output.

use super::{
    Affine, Artifact, ArtifactType, Content, ContentTag, DcBlend, Exporter, Fill, FillRule, Item, LinearGradient, NormalizedF32, RadialGradient,
    Rect, SpreadMethod, Stop, Surface, Transform, Vec2, corners, luma, norm, solid_fill, tf, to_path,
};
use designcraft_geom::kurbo::Shape as _;

impl Exporter<'_> {
    pub(super) fn spread(&mut self, s: &mut Surface, si: usize) {
        let doc = self.doc.clone();
        let Some(sp) = doc.spreads.get(si) else { return };
        let first = doc.first_page_of_spread(si);
        for layer in doc.layers.iter().rev() {
            if !layer.visible || !layer.printable {
                continue;
            }
            for (pi, page) in sp.pages.iter().enumerate() {
                if !page.show_parent_items {
                    continue;
                }
                let Some((ppi, ppage)) = doc.parent_page_for(first + pi) else { continue };
                let parent = &doc.parents[ppi];
                let dx = page.x - parent.pages[ppage].x;
                let page_name = doc.page_name(first + pi);
                for it in &parent.items {
                    if it.layer != layer.id || page.overridden.contains(&it.id) {
                        continue;
                    }
                    // Parent items belong to the parent page they sit on.
                    if parent.page_at_x(it.bounds().center().x) != Some(ppage) && parent.pages.len() > 1 {
                        continue;
                    }
                    self.top_item(s, it, Affine::translate((dx, 0.0)), Some(&page_name), true);
                }
            }
            for it in &sp.items {
                if it.layer == layer.id {
                    self.top_item(s, it, Affine::IDENTITY, None, false);
                }
            }
        }
    }

    /// Push blend/opacity for an item drawn as a group. Returns the number of pushes.
    pub(super) fn push_group(s: &mut Surface, opacity: f32, mode: DcBlend) -> usize {
        let mut n = 0;
        if mode != DcBlend::Normal {
            s.push_blend_mode(blend(mode));
            n += 1;
        }
        if opacity < 0.999 {
            s.push_opacity(norm(opacity));
            n += 1;
        } else if mode != DcBlend::Normal {
            s.push_isolated();
            n += 1;
        }
        n
    }

    /// Isolate Blending / Knockout groups: an isolated transparency group (knockout is drawn
    /// isolated too).
    pub(super) fn push_isolation(s: &mut Surface, it: &Item) -> usize {
        if it.isolate || it.knockout {
            s.push_isolated();
            1
        } else {
            0
        }
    }

    pub(crate) fn item(&mut self, s: &mut Surface, it: &Item, parent: Affine, page_name: Option<&str>) {
        let gf = &it.effects.gradient_feather;
        if !gf.on || it.hidden || it.nonprinting {
            return self.item_inner(s, it, parent, page_name);
        }
        // Gradient feather: a luminosity mask (white = opaque) over the object.
        let xf = parent * it.xf;
        let (p0, p1) = gf.points(it.inner_bounds());
        let gray = |a: f32| -> krilla::color::Color { luma::Color::new((a.clamp(0.0, 1.0) * 255.0).round() as u8).into() };
        let stops = vec![
            Stop { offset: NormalizedF32::ZERO, color: gray(gf.start), opacity: NormalizedF32::ONE },
            Stop { offset: NormalizedF32::ONE, color: gray(gf.end), opacity: NormalizedF32::ONE },
        ];
        let paint: krilla::paint::Paint = if gf.radial {
            let r = (p1 - p0).hypot().max(1e-3) as f32;
            RadialGradient {
                fx: p0.x as f32,
                fy: p0.y as f32,
                fr: 0.0,
                cx: p0.x as f32,
                cy: p0.y as f32,
                cr: r,
                transform: Transform::identity(),
                spread_method: SpreadMethod::Pad,
                stops,
                anti_alias: false,
            }
            .into()
        } else {
            LinearGradient {
                x1: p0.x as f32,
                y1: p0.y as f32,
                x2: p1.x as f32,
                y2: p1.y as f32,
                transform: Transform::identity(),
                spread_method: SpreadMethod::Pad,
                stops,
                anti_alias: false,
            }
            .into()
        };
        let area = it.inner_bounds().inflate(it.stroke.extent() + 4.0, it.stroke.extent() + 4.0);
        let mut sb = s.stream_builder();
        let mut ms = sb.surface();
        ms.push_transform(&tf(xf));
        if let Some(p) = to_path(&area.to_path(0.1)) {
            ms.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
            ms.draw_path(&p);
        }
        ms.pop();
        ms.finish();
        let stream = sb.finish();
        s.push_mask(krilla::mask::Mask::new(stream, krilla::mask::MaskType::Luminosity));
        self.item_inner(s, it, parent, page_name);
        s.pop();
    }

    pub(super) fn item_inner(&mut self, s: &mut Surface, it: &Item, parent: Affine, page_name: Option<&str>) {
        if it.hidden || it.nonprinting {
            return;
        }
        let ds = &it.effects.drop_shadow;
        let grow = it.stroke.extent() + 2.0 + if ds.on { ds.distance.abs() + ds.size.abs() } else { 0.0 };
        let vb = parent.transform_rect_bbox(it.bounds()).inflate(grow, grow);
        if !rects_overlap(vb, self.clip) {
            return;
        }
        let xf = parent * it.xf;
        if it.has_nested_items() {
            // A frame with items pasted into it: fill, the items clipped to the frame, stroke.
            let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
            let path = to_path(&bp);
            let pushes = Self::push_group(s, it.opacity, it.blend);
            s.push_transform(&tf(xf));
            if !it.fill.is_none()
                && let Some(p) = &path
                && let Some(paint) = self.fill_paint(&it.fill, bp.bounding_box())
            {
                s.set_stroke(None);
                s.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                s.draw_path(p);
                s.set_fill(None);
            }
            s.pop();
            // The clip in page space, so the children keep their absolute transforms.
            let mut page_bp = bp.clone();
            page_bp.apply_affine(xf);
            let clip = to_path(&page_bp);
            if let Some(c) = &clip {
                s.push_clip_path(c, &FillRule::NonZero);
            }
            for c in it.children() {
                self.item(s, c, xf, page_name);
            }
            if clip.is_some() {
                s.pop();
            }
            if !it.stroke.is_none()
                && let Some(p) = &path
            {
                s.push_transform(&tf(xf));
                self.stroke(s, it, &bp, p);
                s.pop();
            }
            for _ in 0..pushes {
                s.pop();
            }
            return;
        }
        if !it.children().is_empty() {
            let pushes = Self::push_group(s, it.opacity, it.blend) + Self::push_isolation(s, it);
            for c in it.shown_children() {
                self.item(s, c, xf, page_name);
            }
            for _ in 0..pushes {
                s.pop();
            }
            return;
        }
        let doc = self.doc.clone();
        let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
        let path = to_path(&bp);
        let pushes = Self::push_group(s, it.opacity, it.blend);
        let frame_art = self.tag_story.is_some();
        if frame_art {
            s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Other, None)));
        }
        // Drop shadow (simple offset silhouette, like the renderer).
        if ds.on
            && let Some(p) = &path
            && let Some(c) = self.swatch_color(&ds.color, 1.0)
        {
            let a = self.doc.light_angle(ds.angle, ds.global_light).to_radians();
            let off = Vec2::new(-a.cos() * ds.distance, a.sin() * ds.distance);
            s.push_transform(&tf(Affine::translate(off) * xf));
            s.set_stroke(None);
            s.set_fill(Some(solid_fill(c, ds.opacity)));
            s.draw_path(p);
            s.set_fill(None);
            s.pop();
        }
        s.push_transform(&tf(xf));
        // Fill.
        if !it.fill.is_none()
            && it.path.is_closed()
            && let Some(p) = &path
            && let Some(paint) = self.fill_paint(&it.fill, bp.bounding_box())
        {
            if it.fill.overprint {
                self.warn("overprint is not exported yet");
            }
            s.set_stroke(None);
            s.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
            s.draw_path(p);
            s.set_fill(None);
        }
        if frame_art {
            s.end_tagged();
        }
        // Content.
        match &it.content {
            Content::Graphic(g) => {
                let g = &if it.media.is_some() { it.drawn_graphic(&self.doc.assets).unwrap_or_else(|| g.clone()) } else { g.clone() };
                if let Some(p) = &path {
                    s.push_clip_path(p, &FillRule::NonZero);
                    self.graphic(s, g);
                    s.pop();
                }
            }
            Content::Text(tfr) => {
                let cs = self.cache.get(&doc, tfr.story, page_name);
                if let (Some(ft), Some(pt)) = (cs.frame(it.id), &tfr.options.path) {
                    // Type on a path: the glyphs as outlines along the path.
                    for (si, gp) in designcraft_compose::path_glyphs(&cs, ft, &bp, pt) {
                        let st = &cs.styles[si as usize];
                        if let (Some(c), Some(p)) = (self.swatch_color(&st.fill, st.fill_tint), to_path(&gp)) {
                            s.set_fill(Some(Fill { paint: c.into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                            s.draw_path(&p);
                            s.set_fill(None);
                        }
                    }
                } else if let Some(ft) = cs.frame(it.id) {
                    let text = doc.story(tfr.story).map(|st| st.text.as_str()).unwrap_or("");
                    let local = doc.text_local(it);
                    if local != Affine::IDENTITY {
                        s.push_transform(&tf(local));
                    }
                    self.frame_text(s, &cs, ft, text);
                    if local != Affine::IDENTITY {
                        s.pop();
                    }
                }
            }
            _ => {}
        }
        // Stroke.
        if !it.stroke.is_none()
            && let Some(p) = &path
        {
            if frame_art {
                s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Other, None)));
            }
            self.stroke(s, it, &bp, p);
            if frame_art {
                s.end_tagged();
            }
        }
        s.pop();
        for _ in 0..pushes {
            s.pop();
        }
        // Anchored objects in the frame's text.
        if let Content::Text(tfr) = &it.content
            && let Some(st) = doc.story(tfr.story).filter(|st| !st.objects.is_empty())
        {
            let cs = self.cache.get(&doc, tfr.story, page_name);
            if let Some(ft) = cs.frame(it.id) {
                for o in &ft.objects {
                    if let Some(obj) = st.objects.get(o.index) {
                        self.item(s, &obj.item, xf * Affine::translate(o.origin.to_vec2()), page_name);
                    }
                }
            }
        }
    }
}

fn blend(b: DcBlend) -> krilla::blend::BlendMode {
    use krilla::blend::BlendMode as K;
    match b {
        DcBlend::Normal => K::Normal,
        DcBlend::Darken => K::Darken,
        DcBlend::Multiply => K::Multiply,
        DcBlend::ColorBurn => K::ColorBurn,
        DcBlend::Lighten => K::Lighten,
        DcBlend::Screen => K::Screen,
        DcBlend::ColorDodge => K::ColorDodge,
        DcBlend::Overlay => K::Overlay,
        DcBlend::SoftLight => K::SoftLight,
        DcBlend::HardLight => K::HardLight,
        DcBlend::Difference => K::Difference,
        DcBlend::Exclusion => K::Exclusion,
        DcBlend::Hue => K::Hue,
        DcBlend::Saturation => K::Saturation,
        DcBlend::Color => K::Color,
        DcBlend::Luminosity => K::Luminosity,
    }
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}
