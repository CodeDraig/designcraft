//! Booklet imposition and sheet rendering.

use super::{
    Affine, Cache, Document, ExportReport, Exporter, FillRule, HashMap, Metadata, PageSettings, PdfError, PdfOptions, PdfVersion, Rect, Result, Size,
    tf, to_path,
};
use designcraft_geom::kurbo::Shape as _;

/// File › Print Booklet: how pages pair up on printer spreads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BookletKind {
    /// Folded and stapled: pages padded to a multiple of 4, outer pages together.
    #[default]
    SaddleStitch,
    /// Two pages side by side in reading order.
    TwoUpConsecutive,
}

#[derive(Clone, Debug, Default)]
pub struct BookletOptions {
    pub kind: BookletKind,
    /// Gap between the two pages of a printer spread (points).
    pub space_between: f64,
    pub title: Option<String>,
}

/// The printer spreads for `n` pages: (left, right) absolute page indices, `None` = blank.
pub fn booklet_pairs(n: usize, kind: BookletKind) -> Vec<(Option<usize>, Option<usize>)> {
    let page = |i: usize| (i < n).then_some(i);
    match kind {
        BookletKind::TwoUpConsecutive => (0..n.div_ceil(2)).map(|k| (page(2 * k), page(2 * k + 1))).collect(),
        BookletKind::SaddleStitch => {
            let m = n.div_ceil(4).max(1) * 4;
            (0..m / 2).map(|i| if i % 2 == 0 { (page(m - 1 - i), page(i)) } else { (page(i), page(m - 1 - i)) }).collect()
        }
    }
}

/// An imposed PDF: two document pages per sheet, in booklet order.
pub fn export_booklet(doc: &Document, cache: &Cache, opts: &BookletOptions) -> Result<ExportReport> {
    let n = doc.page_count();
    if n == 0 {
        return Err(PdfError::NoPages);
    }
    let pdf_opts = PdfOptions { title: opts.title.clone(), ..PdfOptions::default() };
    let settings = krilla::SerializeSettings { compress_content_streams: true, ..Default::default() };
    let mut pdf = krilla::Document::new_with(settings);
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let mut meta = Metadata::new().creator("DesignCraft".into()).producer("DesignCraft".into());
    if !title.is_empty() {
        meta = meta.title(title);
    }
    pdf.set_metadata(meta);
    let mut ex = Exporter {
        doc: super::DocumentContext::Borrowed(doc),
        cache,
        opts: &pdf_opts,
        clip: Rect::ZERO,
        warnings: Vec::new(),
        images: HashMap::new(),
        pdfs: HashMap::new(),
        // krilla's default.
        version: PdfVersion::Pdf17,
        too_new: Vec::new(),
        svgs: HashMap::new(),
        fonts: HashMap::new(),
        reverse_cmaps: HashMap::new(),
        rgb_only: false,
        tags: Vec::new(),
        story_tags: HashMap::new(),
        tag_story: None,
        para_tags: HashMap::new(),
    };
    let pairs = booklet_pairs(n, opts.kind);
    // Sheet size from the largest page.
    let (mut pw, mut ph) = (0.0f64, 0.0f64);
    for i in 0..n {
        if let Some((si, pi)) = doc.page_loc(i) {
            let p = &doc.spreads[si].pages[pi];
            pw = pw.max(p.width);
            ph = ph.max(p.height);
        }
    }
    let gap = opts.space_between.max(0.0);
    let size = Size::from_wh((2.0 * pw + gap) as f32, ph as f32).ok_or(PdfError::NoPages)?;
    for (left, right) in &pairs {
        let mut page = pdf.start_page_with(PageSettings::new(size));
        let mut s = page.surface();
        for (slot, abs) in [(0.0, left), (pw + gap, right)] {
            let Some((si, pi)) = abs.and_then(|a| doc.page_loc(a)) else { continue };
            let p = &doc.spreads[si].pages[pi];
            let r = Rect::new(p.x, 0.0, p.x + p.width, p.height);
            // Pages narrower than the slot sit against the fold.
            let dx = if slot == 0.0 { pw - p.width } else { 0.0 };
            s.push_transform(&tf(Affine::translate((slot + dx - p.x, (ph - p.height) / 2.0))));
            ex.clip = r;
            if let Some(clip) = to_path(&r.to_path(0.1)) {
                s.push_clip_path(&clip, &FillRule::NonZero);
                ex.spread(&mut s, si);
                s.pop();
            }
            s.pop();
        }
        s.finish();
        page.finish();
    }
    ex.check_placed_versions()?;
    let mut bytes = pdf.finish().map_err(|e| PdfError::Write(format!("{e:?}")))?;
    if doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk {
        crate::pdfx::cmyk_group_spaces(&mut bytes);
    }
    let mut warnings = ex.warnings;
    warnings.dedup();
    Ok(ExportReport { bytes, pages: pairs.len(), warnings })
}
