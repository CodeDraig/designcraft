//! Convert font outlines to y-down Bézier paths.

use super::{BezPath, DrawSettings, FontFace, GlyphId, OutlinePen, Size};
use skrifa::MetadataProvider;

/// Glyph `gid`'s outline as the font draws it, in font units, y-down (flipped).
pub(super) fn font_outline(face: &FontFace, gid: u32) -> BezPath {
    let mut pen = FlipPen(BezPath::new());
    if let Some(f) = face.skrifa()
        && let Some(g) = f.outline_glyphs().get(GlyphId::new(gid))
    {
        let _ = g.draw(DrawSettings::unhinted(Size::unscaled(), face.location()), &mut pen);
    }
    pen.0
}

struct FlipPen(BezPath);

impl OutlinePen for FlipPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x as f64, -y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x as f64, -y as f64));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((cx0 as f64, -cy0 as f64), (x as f64, -y as f64));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((cx0 as f64, -cy0 as f64), (cx1 as f64, -cy1 as f64), (x as f64, -y as f64));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}
