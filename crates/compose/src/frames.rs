//! Resolve document frames into composition geometry and text-wrap exclusions.

use super::{Document, ItemId, Rect, StoryId, TextFrameOptions, WrapMode};

/// A text-wrap exclusion in a frame's inner space.
#[derive(Clone, Debug, PartialEq)]
pub struct Exclusion {
    pub rect: Rect,
    pub mode: WrapMode,
}

/// One frame of the thread, ready for composition.
#[derive(Clone, Debug)]
pub struct FrameSpec {
    pub id: ItemId,
    /// Text area (inner space, after inset).
    pub area: Rect,
    pub opts: TextFrameOptions,
    /// Lines run top to bottom and follow each other right to left (the story is vertical); the
    /// area is the turned box.
    pub vertical: bool,
    pub exclusions: Vec<Exclusion>,
    pub page_name: Option<String>,
    /// Absolute document page the frame is on (None on parent pages).
    pub page: Option<usize>,
    /// Baseline grid in inner space: (first grid line y, increment).
    pub grid: Option<(f64, f64)>,
    /// The frame is on a left page (for towards/away-from-spine alignment).
    pub left_page: bool,
    /// The page and its margins in the frame's inner space (custom anchored objects).
    pub page_rect: Option<(Rect, Rect)>,
}

impl FrameSpec {
    pub fn columns(&self) -> Vec<Rect> {
        let a = self.area;
        let n = self.opts.columns.max(1);
        match self.opts.columns_kind {
            designcraft_doc::ColumnsKind::FixedWidth if self.opts.column_width > 0.0 => (0..n)
                .map(|i| {
                    let x = a.x0 + i as f64 * (self.opts.column_width + self.opts.gutter);
                    Rect::new(x, a.y0, x + self.opts.column_width, a.y1)
                })
                .collect(),
            _ => designcraft_doc::page::column_rects(a, n, self.opts.gutter),
        }
    }
}

/// Build the frame specs of a story from the document (geometry, wrap, page names, grid).
pub fn frame_specs(doc: &Document, sid: StoryId) -> Vec<FrameSpec> {
    let Some(st) = doc.story(sid) else { return vec![] };
    let mut out = Vec::with_capacity(st.frames.len());
    for &fid in &st.frames {
        let Some(loc) = doc.find(fid) else { continue };
        let Some(item) = doc.item_at(&loc) else { continue };
        let Some(tf) = item.text_frame() else { continue };
        let xf = doc.parent_xf(&loc) * item.xf;
        let inv = xf.inverse();
        let mut exclusions = Vec::new();
        let spread = doc.spread(loc.spread);
        if !tf.options.ignore_wrap
            && let Some(sp) = spread
        {
            for other in &sp.items {
                if other.id == item.id || other.wrap.mode == WrapMode::None || other.hidden {
                    continue;
                }
                if doc.layer(other.layer).is_some_and(|l| !l.visible) {
                    continue;
                }
                let o = other.wrap.offsets;
                let b = other.bounds();
                let r = Rect::new(b.x0 - o[1], b.y0 - o[0], b.x1 + o[3], b.y1 + o[2]);
                let inner = inv.transform_rect_bbox(r);
                exclusions.push(Exclusion { rect: inner, mode: other.wrap.mode });
            }
        }
        let (page_name, page, left_page) = match (loc.spread, spread) {
            (designcraft_doc::SpreadRef::Doc(si), Some(sp)) => {
                let c = item.bounds().center();
                let pi = sp.page_at_x(c.x).unwrap_or(0);
                let abs = doc.first_page_of_spread(si) + pi;
                (Some(doc.page_name(abs)), Some(abs), sp.pages.get(pi).is_some_and(|p| p.side == designcraft_doc::PageSide::Left))
            }
            (designcraft_doc::SpreadRef::Parent(pi), Some(sp)) => {
                let prefix = sp.parent.as_ref().map(|p| p.prefix.clone()).unwrap_or_else(|| "A".into());
                let _ = pi;
                (Some(prefix), None, false)
            }
            _ => (None, None, false),
        };
        // The page under the frame and its margins, in inner space (bounding boxes).
        let page_rect = match (loc.spread, spread) {
            (_, Some(sp)) => sp
                .page_at_x(item.bounds().center().x)
                .and_then(|pi| sp.pages.get(pi))
                .map(|pg| (inv.transform_rect_bbox(pg.bounds()), inv.transform_rect_bbox(pg.margin_rect()))),
            _ => None,
        };
        let g = &doc.settings.baseline_grid;
        let (inc, start) = tf.options.baseline_grid.unwrap_or((g.increment, g.start));
        // Grid lines are at spread y = start + n·inc (page tops are y = 0); map into inner space (translation only).
        let ty = xf.translation().y;
        let grid = (inc > 0.0).then_some((start - ty, inc));
        // Type on a path: one line as long as the path (from the start offset).
        let (area, opts) = match &tf.options.path {
            Some(pt) => {
                let len = designcraft_geom::warp::PathWarp::new(&item.path.to_bezpath(), false).length();
                let size = st
                    .paras
                    .first()
                    .map(|p| doc.styles.resolve_para(p).1)
                    .map(|base| st.runs().map(|(_, f)| doc.styles.resolve_char(&base, f).size).fold(base.size, f64::max))
                    .unwrap_or(12.0);
                let opts = TextFrameOptions {
                    columns: 1,
                    inset: [0.0; 4],
                    first_baseline: designcraft_doc::FirstBaseline::Ascent,
                    path: None,
                    ..tf.options.clone()
                };
                (Rect::new(0.0, 0.0, (len - pt.start).max(1.0), size * 1.6), opts)
            }
            None => (item.text_area(), tf.options.clone()),
        };
        // Vertical type: composed in the turned box; wraps and page rects turned with it.
        let vertical = doc.frame_vertical(item);
        let (area, exclusions, grid, page_rect) = if vertical {
            let v = designcraft_doc::vertical_text_xf(area).inverse();
            let ex = exclusions.into_iter().map(|e| Exclusion { rect: v.transform_rect_bbox(e.rect), ..e }).collect();
            let pr = page_rect.map(|(a, b)| (v.transform_rect_bbox(a), v.transform_rect_bbox(b)));
            (Rect::new(0.0, 0.0, area.height(), area.width()), ex, None, pr)
        } else {
            (area, exclusions, grid, page_rect)
        };
        out.push(FrameSpec {
            id: fid,
            area,
            opts,
            vertical,
            exclusions: if tf.options.path.is_some() { vec![] } else { exclusions },
            page_name,
            page,
            grid,
            left_page,
            page_rect,
        });
    }
    out
}
