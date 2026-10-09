//! Rasterize and inspect placed PDF pages.

use super::{Pixmap, Rendered, pdf_hide_layers, vello_cpu};

/// MIME type guess for encoded image bytes.
/// Is this a PDF file (placed PDFs are graphics)?
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

/// Page count of a PDF.
pub fn pdf_page_count(bytes: &[u8]) -> Option<usize> {
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.to_vec())).ok()?;
    Some(pdf.pages().len())
}

/// Size of PDF page `page` (0-based), in points, as it shows (crop box, rotation applied).
pub fn pdf_page_size(bytes: &[u8], page: usize) -> Option<(f64, f64)> {
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.to_vec())).ok()?;
    let p = pdf.pages().get(page)?;
    let (w, h) = p.render_dimensions();
    Some((w as f64, h as f64))
}

/// PDF page `page` as PNG, its longer side about `max_side` pixels, with the `hidden` layers off.
pub fn pdf_page_png(bytes: &[u8], page: usize, max_side: u32, hidden: &[String]) -> Option<Vec<u8>> {
    let data = if hidden.is_empty() { bytes.to_vec() } else { pdf_hide_layers(bytes, hidden).unwrap_or_else(|| bytes.to_vec()) };
    let pm = render_pdf_page(&data, page, max_side)?;
    let pixels: Vec<u8> = pm.data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
    Some(Rendered { width: pm.width() as u32, height: pm.height() as u32, pixels }.to_png())
}

/// Rasterize PDF page `page` so its longer side is about `max_side` pixels (screen display; PDF
/// export embeds the page as vectors).
pub fn render_pdf_page(bytes: &[u8], page: usize, max_side: u32) -> Option<Pixmap> {
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.to_vec())).ok()?;
    let p = pdf.pages().get(page)?;
    let (w, h) = p.render_dimensions();
    let scale = (max_side as f32 / w.max(h).max(1.0)).min(8.0);
    let rs = hayro::RenderSettings { x_scale: scale, y_scale: scale, ..Default::default() };
    let pm = hayro::render(p, &hayro::RenderCache::new(), &hayro::hayro_interpret::InterpreterSettings::default(), &rs);
    let (pw, ph) = (pm.width(), pm.height());
    let data: Vec<vello_cpu::color::PremulRgba8> =
        pm.data_as_u8_slice().as_chunks::<4>().0.iter().map(|c| vello_cpu::color::PremulRgba8 { r: c[0], g: c[1], b: c[2], a: c[3] }).collect();
    Some(Pixmap::from_parts(data, pw, ph))
}

/// A page box of a PDF page (`crop`, `trim`, `bleed`, `art`, `media`) as (x, y, w, h) in points
/// from the top-left of the page as rendered (its visible crop box); missing boxes fall back to the
/// crop box, as PDF readers do.
pub fn pdf_page_box(bytes: &[u8], page: usize, kind: &str) -> Option<(f64, f64, f64, f64)> {
    use hayro::hayro_syntax::object::{Rect as PRect, dict::keys};
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.to_vec())).ok()?;
    let p = pdf.pages().get(page)?;
    let visible = p.intersected_crop_box();
    let key: &[u8] = match kind {
        "trim" => keys::TRIM_BOX,
        "bleed" => keys::BLEED_BOX,
        "art" => keys::ART_BOX,
        "media" => return Some(rel(visible, p.media_box())),
        _ => return Some(rel(visible, visible)),
    };
    let b = p.raw().get::<PRect>(key).map(|b| b.intersect(p.media_box())).unwrap_or(visible);
    fn rel(v: PRect, b: PRect) -> (f64, f64, f64, f64) {
        (b.x0 - v.x0, v.y1 - b.y1, b.width(), b.height())
    }
    Some(rel(visible, b))
}
