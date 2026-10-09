//! Reflowable EPUB 3 export (File → Export → EPUB (Reflowable)).
//!
//! Stories become XHTML in reading order (page, then top-to-bottom, left-to-right of their first
//! frame); paragraph styles become CSS classes (`p.<slug>`), character styles become `span`
//! classes, local overrides become inline styles. Placed graphics become `<figure>` elements at
//! their position in the reading order. Parent-page items (folios, running heads) are skipped.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod fixed;
pub use fixed::{FixedPage, export_fixed_epub};
mod css;
use css::{char_classes, override_css, para_classes};
pub use css::{slug, stylesheet};
mod content;
use content::body_html;
pub use content::story_html;
mod reading_order;
pub use reading_order::{Block, reading_order};
mod html;
pub use html::{HtmlOptions, export_html};
mod reflowable;
pub use reflowable::export_epub;

use std::collections::BTreeMap;

use designcraft_doc::{Align, CharAttrs, CharProps, Content, Document, Item, ItemId, ParaProps, SpreadRef, StoryId, Styles, story};

#[derive(Clone, Debug)]
pub struct EpubOptions {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: String,
    /// Stable identifier (urn:uuid:…); derived from the title when absent.
    pub identifier: Option<String>,
    /// Cover image (PNG), shown first and marked as the book's cover.
    pub cover: Option<Vec<u8>>,
}

impl Default for EpubOptions {
    fn default() -> Self {
        EpubOptions { title: None, author: None, language: "en".into(), identifier: None, cover: None }
    }
}

#[derive(Debug)]
pub struct EpubError(pub String);

impl std::fmt::Display for EpubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for EpubError {}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\u{E000}'..='\u{E1FF}' => {}
            '\u{2028}' => o.push_str("<br/>"),
            '\u{AD}' => o.push_str("&#173;"),
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::html::base64;
    use super::*;
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::geom::Rect;
    use designcraft_doc::{ParaFormat, SpreadRef};
    use std::io::Read;

    #[test]
    fn exports_valid_package_in_reading_order() {
        let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
        let lid = d.default_layer();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 400.0, 300.0, 500.0), lid, "Second <b> & co", ParaFormat::default()).unwrap();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "First\nline two", ParaFormat::default()).unwrap();
        let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(z.by_index(0).unwrap().name(), "mimetype");
        let mut html = String::new();
        z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
        let a = html.find("First").unwrap();
        let b = html.find("Second &lt;b&gt; &amp; co").unwrap();
        assert!(a < b, "reading order");
        assert!(html.contains("<p class=\"basic-paragraph\">"));
        let mut css = String::new();
        z.by_name("OEBPS/style.css").unwrap().read_to_string(&mut css).unwrap();
        assert!(css.contains("p.basic-paragraph"));
        assert!(z.by_name("OEBPS/nav.xhtml").is_ok());
    }

    #[test]
    fn styles_whose_names_slug_alike_keep_their_own_classes() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        for (name, size) in [("見出し", 30.0), ("本文", 10.0), ("Body Text", 12.0), ("Body-Text", 20.0)] {
            d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
                name: name.into(),
                based_on: None,
                next_style: None,
                para: Default::default(),
                chars: CharAttrs { size: Some(size), ..Default::default() },
                shortcut: String::new(),
            });
        }
        for (k, name) in ["見出し", "本文", "Body Text", "Body-Text"].into_iter().enumerate() {
            let y = 36.0 + 100.0 * k as f64;
            let pf = ParaFormat { style: name.into(), ..Default::default() };
            d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, y, 300.0, y + 50.0), lid, &format!("P{k}"), pf).unwrap();
        }
        let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut html = String::new();
        z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
        let mut css = String::new();
        z.by_name("OEBPS/style.css").unwrap().read_to_string(&mut css).unwrap();
        let class_of = |text: &str| {
            let end = html.find(&format!("\">{text}</p>")).unwrap();
            let start = html[..end].rfind("class=\"").unwrap() + 7;
            html[start..end].to_string()
        };
        let classes: Vec<String> = (0..4).map(|k| class_of(&format!("P{k}"))).collect();
        for (k, c) in classes.iter().enumerate() {
            assert!(!classes[..k].contains(c), "class `{c}` shared: {classes:?}");
            assert_eq!(css.matches(&format!("p.{c} {{")).count(), 1, "one rule for `{c}`:\n{css}");
        }
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("[Basic Paragraph]"), "basic-paragraph");
        assert_eq!(slug("Body First"), "body-first");
        assert_eq!(slug("1 Head"), "s-1-head");
    }

    #[test]
    fn html_is_one_self_contained_page() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Hello <web>", ParaFormat::default()).unwrap();
        let html = export_html(&d, &HtmlOptions { title: Some("T".into()), language: None });
        assert!(html.starts_with("<!DOCTYPE html>") && html.contains("<title>T</title>") && html.contains("Hello &lt;web&gt;"));
        assert!(html.contains("p.basic-paragraph"), "styles inline");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
