//! Place composed glyph outlines along a text frame path.

use super::{ComposedStory, FrameText};

/// Each visible glyph of a type-on-a-path frame placed on the path: its outline in frame space
/// (grouped by run style).
pub fn path_glyphs(
    cs: &ComposedStory,
    ft: &FrameText,
    path: &designcraft_geom::BezPath,
    pt: &designcraft_doc::PathType,
) -> Vec<(u32, designcraft_geom::BezPath)> {
    let db = designcraft_fonts::FontDb::global();
    let warp = designcraft_geom::warp::PathWarp::new(path, pt.flip);
    let mut runs: Vec<(u32, designcraft_geom::BezPath)> = Vec::new();
    for l in &ft.lines {
        // The part of the type that sits on the path.
        let on = match pt.align {
            designcraft_doc::PathAlign::Baseline => l.baseline,
            designcraft_doc::PathAlign::Ascender => l.baseline - l.ascent,
            designcraft_doc::PathAlign::Descender => l.baseline + l.descent,
            designcraft_doc::PathAlign::Center => l.baseline - (l.ascent - l.descent) / 2.0,
        };
        for g in l.glyphs.iter().filter(|g| g.visible) {
            let outline = db.outline(&g.face, g.gid);
            if outline.elements().is_empty() {
                continue;
            }
            let style = &cs.styles[g.style as usize];
            let mid = g.x + g.adv / 2.0;
            let Some((p, a)) = warp.at(pt.start + mid) else { continue };
            let place = designcraft_geom::Affine::translate(p.to_vec2())
                * designcraft_geom::Affine::rotate(a)
                * designcraft_geom::Affine::translate((-mid, -on));
            let skew = if style.skew != 0.0 {
                designcraft_geom::Affine::new([1.0, 0.0, -style.skew.to_radians().tan(), 1.0, 0.0, 0.0])
            } else {
                designcraft_geom::Affine::IDENTITY
            };
            let m =
                place * designcraft_geom::Affine::translate((g.x, l.baseline + g.y)) * skew * designcraft_geom::Affine::scale_non_uniform(g.sx, g.sy);
            let k = match runs.iter().position(|r| r.0 == g.style) {
                Some(k) => k,
                None => {
                    runs.push((g.style, designcraft_geom::BezPath::new()));
                    runs.len() - 1
                }
            };
            let bp = &mut runs[k].1;
            bp.extend((m * outline.as_ref().clone()).elements().iter().copied());
        }
    }
    runs
}
