//! Find usable line slots around text-wrap exclusions.

use super::{FrameSpec, Glyph, ParaProps, Rect, WrapMode};

/// Widest free horizontal interval of `col` in the band, or None if blocked.
pub(super) fn free_slot(f: &FrameSpec, col: Rect, y0: f64, y1: f64, size: f64) -> Option<(f64, f64)> {
    let mut free = vec![(col.x0, col.x1)];
    for ex in &f.exclusions {
        let r = ex.rect;
        let overlaps_band = r.y0 < y1 && r.y1 > y0;
        match ex.mode {
            WrapMode::JumpToNextColumn if y1 > r.y0 && r.x0 < col.x1 && r.x1 > col.x0 => return None,
            WrapMode::JumpObject if overlaps_band && r.x0 < col.x1 && r.x1 > col.x0 => return None,
            WrapMode::BoundingBox | WrapMode::Contour if overlaps_band => {
                let mut next = Vec::new();
                for (a, b) in free {
                    if r.x1 <= a || r.x0 >= b {
                        next.push((a, b));
                        continue;
                    }
                    if r.x0 > a {
                        next.push((a, r.x0));
                    }
                    if r.x1 < b {
                        next.push((r.x1, b));
                    }
                }
                free = next;
            }
            _ => {}
        }
    }
    free.into_iter().filter(|(a, b)| b - a >= size * 1.5).max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn estimate_slots(
    f: &FrameSpec,
    col: Rect,
    first: f64,
    lead: f64,
    size: f64,
    glyphs: &[Glyph],
    pp: &ParaProps,
    line_no: usize,
) -> Vec<(f64, f64)> {
    let _ = (pp, line_no);
    if f.exclusions.is_empty() {
        return vec![];
    }
    // Rough upper bound on lines: total advance / column width × 2.
    let total: f64 = glyphs.iter().map(|g| g.adv).sum();
    let n = ((total / (col.width().max(10.0) * 0.5)).ceil() as usize + 2).min(2000);
    let mut v = Vec::with_capacity(n);
    let mut b = first;
    for _ in 0..n {
        let mut tries = 0;
        let slot = loop {
            if let Some(s) = free_slot(f, col, b - size * 0.8, b + size * 0.25, size) {
                break s;
            }
            b += 1.0;
            tries += 1;
            if b > col.y1 || tries > 4000 {
                break (col.x0, col.x1);
            }
        };
        v.push(slot);
        b += lead;
    }
    v
}
