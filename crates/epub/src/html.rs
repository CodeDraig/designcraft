//! Self-contained HTML document export.

use super::{Document, body_html, esc, stylesheet};

/// Options for [`export_html`].
#[derive(Clone, Debug, Default)]
pub struct HtmlOptions {
    pub title: Option<String>,
    pub language: Option<String>,
}

/// File › Export › HTML: one self-contained page (styles inline, images as data URIs) with the
/// document's stories and graphics in reading order.
pub fn export_html(doc: &Document, opts: &HtmlOptions) -> String {
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let lang = opts.language.clone().unwrap_or_else(|| "en".into());
    let (body, _, _) = body_html(doc, &|_, mime, data| format!("data:{mime};base64,{}", base64(data)));
    format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"DesignCraft\">\n<title>{}</title>\n<style>\nbody {{ max-width: 40em; margin: 2em auto; padding: 0 1em; }}\nfigure {{ margin: 1em 0; }} figure img {{ max-width: 100%; height: auto; }}\n{}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        esc(&title),
        stylesheet(doc)
    )
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
