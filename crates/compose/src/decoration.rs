//! Construct paragraph borders and shading across column breaks.

use super::{ComposedStory, Deco, Line, ParaProps, Rect};

/// Paragraph shading and border, per column the paragraph sits in (a split paragraph gets one box
/// per part; the border's top edge on the first part, its bottom edge on the last).
pub(super) fn para_box_decos(out: &mut ComposedStory, pi: usize, pp: &ParaProps) {
    if !pp.shading_on && !pp.border_on {
        return;
    }
    let mut parts: Vec<(usize, Rect)> = Vec::new();
    for (fi, ft) in out.frames.iter().enumerate() {
        let mut cols_seen: Vec<u32> = ft.lines.iter().filter(|l| l.para == pi).map(|l| l.column).collect();
        cols_seen.dedup();
        for c in cols_seen {
            let lines: Vec<&Line> = ft.lines.iter().filter(|l| l.para == pi && l.column == c).collect();
            if let (Some(a), Some(z)) = (lines.first(), lines.last()) {
                parts.push((fi, Rect::new(a.x0, a.baseline - a.ascent, a.x1, z.baseline + z.descent)));
            }
        }
    }
    let n = parts.len();
    for (k, (fi, r)) in parts.into_iter().enumerate() {
        let ft = &mut out.frames[fi];
        if pp.shading_on {
            let [t, l, b, rr] = pp.shading_offsets;
            ft.decos
                .insert(0, Deco { rect: Rect::new(r.x0 - l, r.y0 - t, r.x1 + rr, r.y1 + b), color: pp.shading_color.clone(), tint: pp.shading_tint });
        }
        if pp.border_on {
            let [t, l, b, rr] = pp.border_offsets;
            let o = Rect::new(r.x0 - l, r.y0 - t, r.x1 + rr, r.y1 + b);
            let [wt, wl, wb, wr] = pp.border_weights.map(|w| w.max(0.0));
            let mut edge = |rect: Rect| ft.decos.push(Deco { rect, color: pp.border_color.clone(), tint: pp.border_tint });
            if k == 0 && wt > 0.0 {
                edge(Rect::new(o.x0 - wl, o.y0 - wt, o.x1 + wr, o.y0));
            }
            if k + 1 == n && wb > 0.0 {
                edge(Rect::new(o.x0 - wl, o.y1, o.x1 + wr, o.y1 + wb));
            }
            if wl > 0.0 {
                edge(Rect::new(o.x0 - wl, o.y0, o.x0, o.y1));
            }
            if wr > 0.0 {
                edge(Rect::new(o.x1, o.y0, o.x1 + wr, o.y1));
            }
        }
    }
}
