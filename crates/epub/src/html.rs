//! Self-contained HTML document export.

use super::{Document, EpubError, HtmlReport, esc};

/// Options for [`export_html`].
#[derive(Clone, Debug, Default)]
pub struct HtmlOptions {
    pub title: Option<String>,
    pub language: Option<String>,
}

/// File › Export › HTML: one self-contained page (styles inline, images as data URIs) with the
/// document's stories and graphics in reading order.
pub fn export_html(doc: &Document, opts: &HtmlOptions) -> String {
    export_html_with_report(doc, opts)
        .map(|r| r.text)
        .unwrap_or_else(|e| format!("<!DOCTYPE html><html><body><p>Export failed: {}</p></body></html>", esc(&e.to_string())))
}

pub fn export_html_with_report(doc: &Document, opts: &HtmlOptions) -> Result<HtmlReport, EpubError> {
    export_html_documents(&super::semantic::documents(doc)?, opts)
}

pub fn export_html_documents(docs: &[Document], opts: &HtmlOptions) -> Result<HtmlReport, EpubError> {
    let doc = docs.first().ok_or_else(|| EpubError("no content to export".into()))?;
    let title = opts.title.clone().unwrap_or_else(|| doc.publication.as_ref().map_or(&doc.title, |v| &v.root.title).clone());
    let lang = esc(opts.language.as_deref().unwrap_or("en"));
    let rendered = super::semantic::build(docs, true)?;
    let body = &rendered.text;
    let text = format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"DesignCraft\">\n<title>{}</title>\n<style>\nbody {{ max-width: 40em; margin: 2em auto; padding: 0 1em; }}\nfigure {{ margin: 1em 0; }} figure img {{ max-width: 100%; height: auto; }}\n{}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        esc(&title),
        rendered.css
    );
    Ok(HtmlReport { text, warnings: rendered.warnings })
}

pub(super) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= c.len() { T[(n >> shift & 63) as usize] as char } else { '=' });
        }
    }
    out
}
