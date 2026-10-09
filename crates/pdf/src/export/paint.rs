//! PDF colours, fill paints, and strokes.

use super::{
    BezPath, Color, ColorType, Exporter, Fill, FillRule, GradientKind, Item, LinearGradient, NormalizedF32, Path, RadialGradient, Rect, SpreadMethod,
    Stop, Stroke, StrokeAlign, StrokeDash, StrokeType, Surface, SwatchValue, Transform, Vec2, cmyk, luma, norm, rgb, to_path,
};
use designcraft_geom::kurbo::Shape as _;

impl Exporter<'_> {
    /// A swatch + tint as a PDF colour: spot swatches (and tints of them) become `/Separation`
    /// with the swatch's values as the alternate space; process colours stay in their device space.
    pub(crate) fn swatch_color(&self, name: &str, tint: f32) -> Option<krilla::color::Color> {
        use krilla::color::separation::{Color as SepColor, SeparationColorant, SeparationSpace};
        let mut n = name;
        let mut t = tint;
        for _ in 0..8 {
            let sw = self.doc.swatch(n)?;
            match &sw.value {
                SwatchValue::None => return None,
                // [Paper] prints no ink (it knocks out what lies below); its colour is only what
                // the screen shows. Written as RGB white (`1 1 1 rg`) it would put DeviceRGB in a
                // CMYK print file.
                SwatchValue::Paper { .. } if self.for_print() => return Some(device(&Color::cmyk(0.0, 0.0, 0.0, 0.0), self.rgb_only)),
                SwatchValue::Tint { base, tint: bt } => {
                    t *= bt;
                    n = base;
                }
                SwatchValue::Registration => return Some(registration(t, self.rgb_only)),
                SwatchValue::Color { color: _, color_type: ColorType::Spot } => {
                    // Ink Manager: an alias prints on another ink; converted inks go to process.
                    let (ink, process) = self.doc.inks.resolve(&sw.name);
                    let target =
                        self.doc.swatch(ink).filter(|w| matches!(w.value, SwatchValue::Color { color_type: ColorType::Spot, .. })).unwrap_or(sw);
                    let SwatchValue::Color { color, .. } = &target.value else { break };
                    if process {
                        return Some(device(&designcraft_color::swatch::apply_tint(*color, t), self.rgb_only));
                    }
                    let space = SeparationSpace::new(SeparationColorant::Custom(target.name.clone()), regular(color, self.rgb_only));
                    return Some(SepColor::new(q(t), space).into());
                }
                _ => break,
            }
        }
        self.doc.resolve_color(name, tint).map(|c| device(&c, self.rgb_only))
    }

    /// A swatch fill (solid or gradient) in the item's inner space. `None` for [None]/unknown.
    pub(super) fn fill_paint(&self, fill: &designcraft_doc::Fill, bounds: Rect) -> Option<krilla::paint::Paint> {
        let (swatch, tint, angle) = (fill.swatch.as_str(), fill.tint, fill.gradient_angle);
        if let Some(g) = designcraft_color::swatch::resolve_gradient(&self.doc.swatches, swatch) {
            let mut stops: Vec<Stop> = Vec::new();
            let mut last = 0.0f32;
            for (o, c, a) in g.expanded_stops() {
                let o = o.clamp(last, 1.0);
                last = o;
                stops.push(Stop { offset: norm(o), color: device(&c, self.rgb_only), opacity: norm(a) });
            }
            if stops.is_empty() {
                return None;
            }
            let c = bounds.center();
            let v = fill.gradient_vector;
            return Some(match g.kind {
                GradientKind::Radial => {
                    let (c, r) = match v {
                        Some([x0, y0, x1, y1]) => (designcraft_geom::Point::new(x0, y0), Vec2::new(x1 - x0, y1 - y0).hypot()),
                        None => (c, bounds.width().max(bounds.height()) / 2.0),
                    };
                    let r = r.max(1e-3) as f32;
                    let (cx, cy) = (c.x as f32, c.y as f32);
                    RadialGradient {
                        fx: cx,
                        fy: cy,
                        fr: 0.0,
                        cx,
                        cy,
                        cr: r,
                        transform: Transform::identity(),
                        spread_method: SpreadMethod::Pad,
                        stops,
                        anti_alias: false,
                    }
                    .into()
                }
                _ => {
                    let a = angle.unwrap_or(0.0).to_radians();
                    let half = ((bounds.width() * a.cos().abs() + bounds.height() * a.sin().abs()) / 2.0).max(1e-3);
                    let d = Vec2::new(a.cos(), -a.sin()) * half;
                    let (p0, p1) = match v {
                        Some([x0, y0, x1, y1]) => (designcraft_geom::Point::new(x0, y0), designcraft_geom::Point::new(x1, y1)),
                        None => (c - d, c + d),
                    };
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
                }
            });
        }
        self.swatch_color(swatch, tint).map(Into::into)
    }

    pub(super) fn stroke(&mut self, s: &mut Surface, it: &Item, bp: &BezPath, path: &Path) {
        let st = &it.stroke;
        let Some(c) = self.swatch_color(&st.swatch, st.tint) else { return };
        let closed = it.path.is_closed();
        let mut cap = match st.cap {
            designcraft_doc::Cap::Butt => krilla::paint::LineCap::Butt,
            designcraft_doc::Cap::Round => krilla::paint::LineCap::Round,
            designcraft_doc::Cap::Projecting => krilla::paint::LineCap::Square,
        };
        let kind = self.doc.stroke_kind(&st.kind);
        // Stripes, wavy and hash strokes: a filled outline (gap colour under it).
        if let Some(o) = kind.outline(bp, st.weight, 0.05)
            && let Some(op) = to_path(&o)
        {
            s.set_stroke(None);
            if let Some(g) = self.swatch_color(&st.gap_swatch, st.gap_tint)
                && st.gap_swatch != designcraft_color::swatch::NONE
            {
                s.set_stroke(Some(Stroke { paint: g.into(), width: st.weight as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
                s.set_fill(None);
                s.draw_path(path);
                s.set_stroke(None);
            }
            s.set_fill(Some(Fill { paint: c.into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
            s.draw_path(&op);
            s.set_fill(None);
            return;
        }
        let dash = match &kind {
            StrokeType::Dashed { pattern } if pattern.iter().any(|v| *v > 0.0) => {
                let mut pat: Vec<f32> = pattern.iter().map(|v| v.max(0.0) as f32).collect();
                if pat.len() % 2 == 1 {
                    pat.extend(pat.clone());
                }
                Some(StrokeDash { array: pat, offset: 0.0 })
            }
            StrokeType::Dotted => {
                cap = krilla::paint::LineCap::Round;
                Some(StrokeDash { array: vec![0.0, (st.weight * 2.0) as f32], offset: 0.0 })
            }
            _ => None,
        };
        let arrows = designcraft_doc::arrow::apply(bp, st, closed);
        let trimmed = arrows.as_ref().and_then(|a| to_path(&a.0));
        let path = trimmed.as_ref().unwrap_or(path);
        let aligned = closed && st.align != StrokeAlign::Center;
        let width = if aligned { st.weight * 2.0 } else { st.weight };
        let mut pushes = 0;
        match st.align {
            StrokeAlign::Inside if closed => {
                s.push_clip_path(path, &FillRule::NonZero);
                pushes += 1;
            }
            StrokeAlign::Outside if closed => {
                // Clip to everything outside the path: a big frame plus the path, even-odd.
                let b = bp.bounding_box().inflate(width * 2.0 + 10.0, width * 2.0 + 10.0);
                let mut outside = b.to_path(0.1);
                outside.extend(bp.iter());
                if let Some(p) = to_path(&outside) {
                    s.push_clip_path(&p, &FillRule::EvenOdd);
                    pushes += 1;
                }
            }
            _ => {}
        }
        s.set_fill(None);
        if dash.is_some()
            && pushes == 0
            && let Some(g) = self.swatch_color(&st.gap_swatch, st.gap_tint)
        {
            // Gap colour under dashes and dots.
            s.set_stroke(Some(Stroke { paint: g.into(), width: width as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
            s.draw_path(path);
        }
        s.set_stroke(Some(Stroke {
            paint: c.clone().into(),
            width: width as f32,
            miter_limit: st.miter_limit.max(1.0) as f32,
            line_cap: cap,
            line_join: match st.join {
                designcraft_doc::Join::Miter => krilla::paint::LineJoin::Miter,
                designcraft_doc::Join::Round => krilla::paint::LineJoin::Round,
                designcraft_doc::Join::Bevel => krilla::paint::LineJoin::Bevel,
            },
            opacity: NormalizedF32::ONE,
            dash,
        }));
        s.draw_path(path);
        s.set_stroke(None);
        for _ in 0..pushes {
            s.pop();
        }
        for h in arrows.iter().flat_map(|a| &a.1) {
            let Some(p) = to_path(&h.path) else { continue };
            match h.outline {
                Some(w) => {
                    s.set_stroke(Some(Stroke { paint: c.clone().into(), width: w as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
                    s.draw_path(&p);
                    s.set_stroke(None);
                }
                None => {
                    s.set_fill(Some(krilla::paint::Fill { paint: c.clone().into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                    s.draw_path(&p);
                    s.set_fill(None);
                }
            }
        }
    }
}

fn q(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn regular(c: &Color, rgb_only: bool) -> krilla::color::RegularColor {
    use krilla::color::RegularColor as R;
    if rgb_only && matches!(c, Color::Cmyk { .. }) {
        let [r, g, b] = c.to_rgb();
        return R::Rgb(rgb::Color::new(q(r), q(g), q(b)));
    }
    match *c {
        Color::Rgb { r, g, b } => R::Rgb(rgb::Color::new(q(r), q(g), q(b))),
        Color::Cmyk { c, m, y, k } => R::Cmyk(cmyk::Color::new(q(c), q(m), q(y), q(k))),
        // DesignCraft grey is ink coverage (0 = white); PDF DeviceGray is lightness.
        Color::Gray { k } => R::Luma(luma::Color::new(q(1.0 - k))),
    }
}

/// A process colour in its own device space (CMYK → DeviceCMYK, RGB → DeviceRGB, Gray → DeviceGray).
/// `rgb_only` converts CMYK to RGB (PDF/A without a CMYK output profile).
pub(crate) fn device(c: &Color, rgb_only: bool) -> krilla::color::Color {
    match regular(c, rgb_only) {
        krilla::color::RegularColor::Rgb(v) => v.into(),
        krilla::color::RegularColor::Cmyk(v) => v.into(),
        krilla::color::RegularColor::Luma(v) => v.into(),
    }
}

/// `[Registration]` prints on every plate: `/Separation /All`.
pub(crate) fn registration(tint: f32, rgb_only: bool) -> krilla::color::Color {
    use krilla::color::separation::{Color as SepColor, SeparationColorant, SeparationSpace};
    let alt = regular(&Color::cmyk(1.0, 1.0, 1.0, 1.0), rgb_only);
    SepColor::new(q(tint), SeparationSpace::new(SeparationColorant::AllColorants, alt)).into()
}

pub(crate) fn solid_fill(c: krilla::color::Color, opacity: f32) -> Fill {
    Fill { paint: c.into(), opacity: norm(opacity), rule: FillRule::NonZero }
}
