//! Story and document content in reading order.

use super::{BTreeMap, Block, Content, Document, StoryId, char_classes, esc, override_css, para_classes, reading_order, slug, story};
use std::fmt::Write as _;

/// Elements Export Tagging may choose (paragraph or character level).
fn is_tag(t: &str, character: bool) -> bool {
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
                esc(&sub)
            } else {
                esc(raw)
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

fn ext(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "video/ogg" => "ogv",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/aac" => "aac",
        _ => "png",
    }
}

/// The content in reading order as HTML sections and figures; `img_src` names each image
/// (a package path, or a data URI for single-file HTML). Returns (body, images by name, table of
/// contents).
#[allow(clippy::type_complexity)]
pub(super) fn body_html<'a>(
    doc: &'a Document,
    img_src: &dyn Fn(&str, &str, &[u8]) -> String,
) -> (String, BTreeMap<String, (&'a str, &'a [u8])>, Vec<(String, String)>) {
    let order = reading_order(doc);
    let mut body = String::new();
    let mut images: BTreeMap<String, (&str, &[u8])> = BTreeMap::new();
    let mut toc: Vec<(String, String)> = Vec::new();
    for (k, b) in order.iter().enumerate() {
        match b {
            Block::Story(sid) => {
                let anchor = format!("s{k}");
                // First paragraph text as the TOC label.
                if let Some(st) = doc.story(*sid) {
                    let first: String =
                        st.text.split('\n').next().unwrap_or("").chars().filter(|c| !('\u{E000}'..='\u{E0FF}').contains(c)).take(60).collect();
                    if !first.trim().is_empty() {
                        toc.push((anchor.clone(), first));
                    }
                }
                let _ = write!(body, "<section id=\"{anchor}\">\n{}</section>\n", story_html(doc, *sid));
            }
            Block::Image(iid) => {
                let Some(Content::Graphic(g)) = doc.item(*iid).map(|i| &i.content) else { continue };
                let Some(a) = doc.assets.get(&g.asset) else { continue };
                if a.data.is_empty() {
                    continue;
                }
                // Video and sound: an HTML5 player (with the poster, when there is one).
                if let Some(kind) = designcraft_doc::media_kind(&a.mime) {
                    let name = format!("media/{}.{}", g.asset.0, ext(&a.mime));
                    images.insert(name.clone(), (a.mime.as_str(), a.data.as_slice()));
                    let m = doc.item(*iid).and_then(|i| i.media.clone()).unwrap_or_default();
                    let mut attrs = String::new();
                    for (on, attr) in
                        [(m.controls, " controls=\"controls\""), (m.play_on_page_load, " autoplay=\"autoplay\""), (m.looping, " loop=\"loop\"")]
                    {
                        if on {
                            attrs.push_str(attr);
                        }
                    }
                    if let Some(pa) = m.poster.and_then(|p| doc.assets.get(&p)) {
                        let pn = format!("images/{}.{}", pa.id.0, ext(&pa.mime));
                        images.insert(pn.clone(), (pa.mime.as_str(), pa.data.as_slice()));
                        if kind == "video" {
                            attrs.push_str(&format!(" poster=\"{}\"", img_src(&pn, &pa.mime, &pa.data)));
                        }
                    }
                    let _ = writeln!(body, "<figure><{kind} src=\"{}\"{attrs}>{}</{kind}></figure>", img_src(&name, &a.mime, &a.data), esc(&a.name));
                    continue;
                }
                let name = format!("images/{}.{}", g.asset.0, ext(&a.mime));
                images.insert(name.clone(), (a.mime.as_str(), a.data.as_slice()));
                // Alt text from Object Export Options, else the file name.
                let alt = doc.item(*iid).map(|i| i.alt_text.as_str()).filter(|t| !t.is_empty()).unwrap_or(&a.name);
                // Object Export Options: alignment and a page break before.
                let eo = doc.item(*iid).map(|i| i.export_options.clone()).unwrap_or_default();
                let mut css = String::new();
                if matches!(eo.align.as_str(), "left" | "center" | "right") {
                    css.push_str(&format!("text-align:{};", eo.align));
                }
                if eo.page_break_before {
                    css.push_str("page-break-before:always;break-before:page;");
                }
                let style = if css.is_empty() { String::new() } else { format!(" style=\"{css}\"") };
                let _ = writeln!(body, "<figure{style}><img src=\"{}\" alt=\"{}\"/></figure>", img_src(&name, &a.mime, &a.data), esc(alt));
            }
        }
    }
    (body, images, toc)
}
