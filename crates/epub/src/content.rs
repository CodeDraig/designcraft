//! Story and document content in reading order.

use super::{Document, StoryId, char_classes, esc, override_css, para_classes, slug, story};
use std::fmt::Write as _;

/// Elements Export Tagging may choose (paragraph or character level).
pub(super) fn is_tag(t: &str, character: bool) -> bool {
    if character {
        matches!(t, "span" | "em" | "strong" | "i" | "b" | "code" | "sup" | "sub" | "small" | "mark" | "cite" | "q")
    } else {
        matches!(t, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "li" | "figcaption" | "aside" | "div")
    }
}

/// One story as XHTML paragraphs.
pub fn story_html(doc: &Document, sid: StoryId) -> String {
    let Some(st) = doc.story(sid) else { return String::new() };
    let (pc, cc) = (para_classes(&doc.styles), char_classes(&doc.styles));
    let mut out = String::new();
    for (pi, r) in st.para_ranges().iter().enumerate() {
        let pf = &st.paras[pi];
        // Export Tagging: the style's element and class.
        let et = doc.styles.export_tag(&pf.style, false);
        let tag = et.map(|e| e.tag.as_str()).filter(|t| is_tag(t, false)).unwrap_or("p");
        let class =
            et.map(|e| e.class.clone()).filter(|c| !c.is_empty()).or_else(|| pc.get(pf.style.as_str()).cloned()).unwrap_or_else(|| slug(&pf.style));
        let text_empty = st.text[r.clone()].trim().is_empty();
        let rtl = doc.styles.resolve_para(pf).0.direction == designcraft_doc::TextDirection::RightToLeft;
        let _ = write!(out, "<{tag} class=\"{class}\"{}>", if rtl { " dir=\"rtl\"" } else { "" });
        if text_empty {
            out.push_str("&#160;");
        }
        for (rr, f) in st.runs() {
            let a = rr.start.max(r.start);
            let b = rr.end.min(r.end);
            if a >= b {
                continue;
            }
            // Hidden conditional text isn't exported.
            if f.over.conditions.as_deref().is_some_and(|c| doc.conditions_hide(c)) || f.over.change == Some(designcraft_doc::ChangeMark::Deleted) {
                continue;
            }
            let raw = &st.text[a..b];
            let t = if raw.chars().any(|c| designcraft_doc::vars::var_index(c).is_some()) {
                let sub: String = raw
                    .chars()
                    .map(|c| match designcraft_doc::vars::var_index(c) {
                        Some(i) => doc.variable_value(i, None).unwrap_or_default(),
                        None => c.to_string(),
                    })
                    .collect();
                esc(&sub).replace('\u{2028}', "<br/>")
            } else {
                esc(raw).replace('\u{2028}', "<br/>")
            };
            let et = (f.style != story::NO_CHAR_STYLE).then(|| doc.styles.export_tag(&f.style, true)).flatten();
            let ctag = et.map(|e| e.tag.as_str()).filter(|t| is_tag(t, true)).unwrap_or("span");
            let cls = if f.style != story::NO_CHAR_STYLE {
                let class = et
                    .map(|e| e.class.clone())
                    .filter(|c| !c.is_empty())
                    .or_else(|| cc.get(f.style.as_str()).cloned())
                    .unwrap_or_else(|| slug(&f.style));
                format!(" class=\"{class}\"")
            } else {
                String::new()
            };
            let style = override_css(doc, &f.over);
            let style_attr = if style.is_empty() { String::new() } else { format!(" style=\"{style}\"") };
            if cls.is_empty() && style_attr.is_empty() {
                out.push_str(&t);
            } else {
                let _ = write!(out, "<{ctag}{cls}{style_attr}>{t}</{ctag}>");
            }
        }
        let _ = writeln!(out, "</{tag}>");
    }
    out
}
