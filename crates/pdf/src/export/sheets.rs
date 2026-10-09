//! Page selection, bleed boxes, and printer-mark extents.

use super::{Document, PageSide, PdfError, PdfOptions, Rect, Result, Spread};

/// One output page: a document page or a spread, in spread coordinates.
pub(crate) struct Sheet {
    pub spread: usize,
    pub trim: Rect,
    pub bleed: Rect,
    pub media: Rect,
    /// Page name(s) for the page-information mark.
    pub label: String,
}

pub(crate) const MARK_LEN: f64 = 18.0;

fn bleed_lr(side: PageSide, b: [f64; 4]) -> (f64, f64) {
    // Bleed is top, bottom, inside, outside; single-sided pages read inside as left.
    if side == PageSide::Left { (b[3], b[2]) } else { (b[2], b[3]) }
}

/// The spread each exported PDF page shows (in page order).
pub fn sheet_spreads(doc: &Document, opts: &PdfOptions) -> Vec<usize> {
    sheets(doc, opts).map(|v| v.iter().map(|s| s.spread).collect()).unwrap_or_default()
}

pub(super) fn sheets(doc: &Document, opts: &PdfOptions) -> Result<Vec<Sheet>> {
    let count = doc.page_count();
    let pages: Vec<usize> = match &opts.pages {
        Some(v) => v.clone(),
        None => (0..count).collect(),
    };
    if pages.is_empty() {
        return Err(PdfError::NoPages);
    }
    if let Some(bad) = pages.iter().find(|p| **p >= count) {
        return Err(PdfError::BadPage(bad + 1));
    }
    let b = doc.settings.bleed.map(|v| if opts.bleed { v.max(0.0) } else { 0.0 });
    let mut out: Vec<Sheet> = Vec::new();
    let mut seen_spreads: Vec<usize> = Vec::new();
    for abs in pages {
        let (si, pi) = doc.page_loc(abs).ok_or(PdfError::BadPage(abs + 1))?;
        let sp: &Spread = &doc.spreads[si];
        let (trim, l, r, label) = if opts.spreads {
            if seen_spreads.contains(&si) {
                continue;
            }
            seen_spreads.push(si);
            let first = doc.first_page_of_spread(si);
            let (l, _) = sp.pages.first().map(|p| bleed_lr(p.side, b)).unwrap_or((b[2], b[3]));
            let (_, r) = sp.pages.last().map(|p| bleed_lr(p.side, b)).unwrap_or((b[2], b[3]));
            let names: Vec<String> = (0..sp.pages.len()).map(|i| doc.page_name(first + i)).collect();
            (sp.bounds(), l, r, names.join("–"))
        } else {
            let p = &sp.pages[pi];
            let (l, r) = bleed_lr(p.side, b);
            (p.bounds(), l, r, doc.page_name(abs))
        };
        let bleed = Rect::new(trim.x0 - l, trim.y0 - b[0], trim.x1 + r, trim.y1 + b[1]);
        let media = if opts.marks.any() {
            let m = mark_start(opts, b) + MARK_LEN + 6.0;
            trim.inflate(m, m)
        } else {
            bleed
        };
        out.push(Sheet { spread: si, trim, bleed, media, label });
    }
    Ok(out)
}

/// Distance from the trim edge where marks start: the offset, but never inside the bleed.
pub(crate) fn mark_start(opts: &PdfOptions, bleed: [f64; 4]) -> f64 {
    bleed.iter().copied().fold(opts.marks.offset.max(0.0), f64::max)
}
