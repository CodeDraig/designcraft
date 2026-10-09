//! Map story byte positions to visual carets and hit-test composed lines.

use super::{ComposedStory, Line, Point};

/// Caret geometry for story byte `pos`: (frame index, x, baseline, ascent, descent).
pub fn caret(cs: &ComposedStory, pos: usize) -> Option<(usize, f64, f64, f64, f64)> {
    let (fi, l) = caret_line(cs, pos)?;
    Some((fi, caret_x(l, pos), l.baseline, l.ascent, l.descent))
}

/// The line the caret at `pos` is drawn on (and its frame index).
fn caret_line(cs: &ComposedStory, pos: usize) -> Option<(usize, &Line)> {
    let mut best: Option<(usize, &Line)> = None;
    for (fi, ft) in cs.frames.iter().enumerate() {
        for l in &ft.lines {
            if pos >= l.range.start && pos <= l.range.end {
                // Prefer the line where pos is not at its end (except the last line of a paragraph).
                let at_end = pos == l.range.end && !l.last_in_para;
                if best.is_none() || !at_end {
                    best = Some((fi, l));
                    if !at_end {
                        break;
                    }
                }
            }
        }
        if best.is_some_and(|(_, l)| pos < l.range.end || l.last_in_para) {
            break;
        }
    }
    best
}

/// The caret's x on line `l` for story byte `pos` (bidi-aware).
pub fn caret_x(l: &Line, pos: usize) -> f64 {
    for g in &l.glyphs {
        if g.len > 0 && pos >= g.byte && pos < g.byte + g.len {
            // Inside a multi-char cluster (ligature): interpolate.
            let t = (pos - g.byte) as f64 / g.len as f64;
            return if g.rtl { g.x + g.adv * (1.0 - t) } else { g.x + g.adv * t };
        }
        if g.len > 0 && g.byte >= pos {
            return if g.rtl { g.x + g.adv } else { g.x };
        }
    }
    match l.glyphs.iter().rev().find(|g| g.len > 0) {
        Some(g) if g.rtl => g.x,
        _ => l.end_x,
    }
}

/// Caret positions on a line with their x, for lines with right-to-left text.
fn caret_stops(l: &Line) -> Vec<(usize, f64)> {
    let mut ps: Vec<usize> = l.glyphs.iter().filter(|g| g.len > 0).flat_map(|g| [g.byte, g.byte + g.len]).collect();
    ps.extend([l.range.start, l.range.end]);
    ps.sort_unstable();
    ps.dedup();
    ps.into_iter().filter(|p| l.range.contains(p) || *p == l.range.end).map(|p| (p, caret_x(l, p))).collect()
}

/// Bidi caret movement: the position one step to the left (or right) of `pos` on its line as
/// drawn. `None` when the line has no right-to-left text or `pos` is at that side's edge of the
/// line: then move in text order ([`line_rtl`] says which way that runs).
pub fn visual_step(cs: &ComposedStory, pos: usize, left: bool) -> Option<usize> {
    let (_, l) = caret_line(cs, pos)?;
    if !l.glyphs.iter().any(|g| g.rtl) {
        return None;
    }
    let x = caret_x(l, pos);
    let stops = caret_stops(l);
    let side = |s: &&(usize, f64)| if left { s.1 < x - 0.01 } else { s.1 > x + 0.01 };
    let pick = stops.iter().filter(side);
    if left { pick.max_by(|a, b| a.1.total_cmp(&b.1)) } else { pick.min_by(|a, b| a.1.total_cmp(&b.1)) }.map(|s| s.0)
}

/// Does the line at `pos` start (in text order) with right-to-left text?
pub fn line_rtl(cs: &ComposedStory, pos: usize) -> bool {
    caret_line(cs, pos).and_then(|(_, l)| l.glyphs.iter().find(|g| g.len > 0)).is_some_and(|g| g.rtl)
}

/// Story byte nearest to point `p` (frame inner space) in frame `fi`.
pub fn hit(cs: &ComposedStory, fi: usize, p: Point) -> Option<usize> {
    let ft = cs.frames.get(fi)?;
    if ft.lines.is_empty() {
        return Some(ft.range.start);
    }
    // Closest line vertically (within its column).
    let l = ft
        .lines
        .iter()
        .filter(|l| p.x >= l.x0 - 20.0 && p.x <= l.x1 + 20.0 || ft.columns.len() <= 1)
        .min_by(|a, b| line_dist(a, p.y).total_cmp(&line_dist(b, p.y)))
        .or_else(|| ft.lines.first())?;
    if l.glyphs.iter().any(|g| g.rtl) {
        // Bidi: the caret position drawn nearest the point.
        return caret_stops(l).into_iter().min_by(|a, b| (a.1 - p.x).abs().total_cmp(&(b.1 - p.x).abs())).map(|s| s.0);
    }
    let mut best = l.range.start;
    let mut prev_mid = f64::NEG_INFINITY;
    for g in l.glyphs.iter().filter(|g| g.len > 0) {
        let mid = g.x + g.adv / 2.0;
        if p.x < mid && p.x >= prev_mid {
            return Some(g.byte);
        }
        prev_mid = mid;
        best = g.byte + g.len;
    }
    // After the last glyph: line end (before the paragraph separator / break).
    Some(if l.last_in_para { l.range.end } else { best.min(l.range.end) })
}

fn line_dist(l: &Line, y: f64) -> f64 {
    let top = l.baseline - l.ascent;
    let bot = l.baseline + l.descent;
    if y < top {
        top - y
    } else if y > bot {
        y - bot
    } else {
        0.0
    }
}
