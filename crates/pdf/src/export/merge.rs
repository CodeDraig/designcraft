//! Assembly of already rendered PDF parts.

use super::{Arc, Metadata, PageSettings, PdfError, Result, Size};

/// One PDF from several (a book's documents), every page carried over as vectors; each part
/// comes with its pages' sizes (points).
pub fn merge_pdfs(parts: &[(Vec<u8>, Vec<(f32, f32)>)], title: Option<&str>) -> Result<Vec<u8>> {
    let mut pdf = krilla::Document::new_with(krilla::SerializeSettings::default());
    let mut meta = Metadata::new().creator("DesignCraft".into()).producer("DesignCraft".into());
    if let Some(t) = title {
        meta = meta.title(t.to_string());
    }
    pdf.set_metadata(meta);
    let mut pages = 0;
    for (bytes, sizes) in parts {
        let src = krilla::pdf::Pdf::new(Arc::new(bytes.clone())).map_err(|e| PdfError::Write(format!("{e:?}")))?;
        let doc = krilla::pdf::PdfDocument::new(Arc::new(src));
        for (i, &(w, h)) in sizes.iter().enumerate() {
            let size = Size::from_wh(w.max(1.0), h.max(1.0)).ok_or(PdfError::NoPages)?;
            let mut page = pdf.start_page_with(PageSettings::new(size));
            let mut s = page.surface();
            s.draw_pdf_page(&doc, size, i);
            s.finish();
            page.finish();
            pages += 1;
        }
    }
    if pages == 0 {
        return Err(PdfError::NoPages);
    }
    pdf.finish().map_err(|e| PdfError::Write(format!("{e:?}")))
}
