use crate::ImportedStyle;
use designcraft_doc::{Align, Capitalization, CharAttrs, Leading, ParaAttrs, Position};

pub(super) fn paragraphs() -> Vec<ImportedStyle> {
    let mut out = Vec::new();
    for (name, size, font, before, after) in [
        ("Body", 11.0, "Regular", 0.0, 6.0),
        ("Small", 9.0, "Regular", 0.0, 4.0),
        ("Title", 26.0, "Bold", 18.0, 18.0),
        ("Subtitle", 17.0, "Regular", 8.0, 12.0),
        ("Heading 1", 19.0, "Bold", 18.0, 10.0),
        ("Heading 2", 15.0, "Bold", 14.0, 8.0),
        ("Heading 3", 12.0, "Bold", 10.0, 6.0),
        ("Quote", 11.0, "Regular", 6.0, 6.0),
        ("Verse", 11.0, "Regular", 0.0, 0.0),
        ("Verse Heading", 11.0, "Bold", 8.0, 2.0),
        ("Speaker", 11.0, "Bold", 8.0, 2.0),
        ("Stage", 11.0, "Italic", 2.0, 4.0),
        ("List", 11.0, "Regular", 0.0, 3.0),
        ("Caption", 10.0, "Italic", 4.0, 8.0),
        ("Note", 9.0, "Regular", 0.0, 4.0),
        ("Apparatus", 9.0, "Regular", 0.0, 4.0),
        ("Table", 10.0, "Regular", 0.0, 0.0),
        ("Bibliography", 10.0, "Regular", 0.0, 5.0),
    ] {
        let mut para = ParaAttrs { space_before: Some(before), space_after: Some(after), ..Default::default() };
        if name.starts_with("Heading") || matches!(name, "Title" | "Subtitle" | "Speaker" | "Verse Heading") {
            para.keep_with_next = Some(1);
        }
        if matches!(name, "Quote" | "Verse" | "Verse Heading" | "List") {
            para.left_indent = Some(18.0);
        }
        if name == "Quote" {
            para.right_indent = Some(18.0);
        }
        if name == "Verse" {
            para.hyphenate = Some(false);
        }
        out.push(ImportedStyle {
            name: format!("TEI {name}"),
            based_on: None,
            para,
            chars: CharAttrs { size: Some(size), leading: Some(Leading::Points(size * 1.3)), font_style: Some(font.into()), ..Default::default() },
        });
    }
    out
}

pub(super) fn heading(depth: usize) -> &'static str {
    match depth {
        0 | 1 => "Heading 1",
        2 => "Heading 2",
        _ => "Heading 3",
    }
}

/// Named block renditions used by reading masters; these remain editable paragraph styles.
pub(super) fn paragraph_rendition(token: &str, depth: usize) -> Option<&'static str> {
    match token.trim() {
        "p" => Some("Body"),
        "quotation" => Some("Quote"),
        "small" => Some("Small"),
        "title" => Some("Title"),
        "heading" => Some(heading(depth)),
        "versehead" => Some("Verse Heading"),
        _ => None,
    }
}

/// Supported TEI rendition tokens and the small CSS subset representable by the native model.
/// Returns false for an unrepresented declaration, so the caller records it.
pub(super) fn apply(token: &str, chars: &mut CharAttrs, para: &mut ParaAttrs) -> bool {
    let token = token.trim();
    if token.is_empty() {
        return true;
    }
    let (key, value) = token.split_once(':').map(|(k, v)| (k.trim(), v.trim())).unwrap_or((token, ""));
    let italic = chars.font_style.as_deref().is_some_and(|s| s.contains("Italic"));
    let bold = chars.font_style.as_deref().is_some_and(|s| s.contains("Bold"));
    match (key, value) {
        ("italic" | "italics" | "i", _) | ("font-style", "italic" | "oblique") => {
            chars.font_style = Some(if bold { "Bold Italic" } else { "Italic" }.into())
        }
        ("bold" | "b", _) | ("font-weight", "bold" | "700" | "800" | "900") => {
            chars.font_style = Some(if italic { "Bold Italic" } else { "Bold" }.into())
        }
        ("font-style", "normal") => chars.font_style = Some(if bold { "Bold" } else { "Regular" }.into()),
        ("font-weight", "normal" | "400") => chars.font_style = Some(if italic { "Italic" } else { "Regular" }.into()),
        ("smallcaps" | "small-caps" | "sc", _) | ("font-variant", "small-caps") => chars.capitalization = Some(Capitalization::SmallCaps),
        ("uppercase", _) | ("text-transform", "uppercase") => chars.capitalization = Some(Capitalization::AllCaps),
        ("underline" | "u", _) | ("text-decoration", "underline") => chars.underline = Some(true),
        ("strikethrough", _) | ("text-decoration", "line-through") => chars.strikethrough = Some(true),
        ("sup" | "superscript", _) | ("vertical-align", "super") => chars.position = Some(Position::Superscript),
        ("sub" | "subscript", _) | ("vertical-align", "sub") => chars.position = Some(Position::Subscript),
        ("center" | "centred" | "centered", _) | ("text-align", "center") => para.align = Some(Align::Center),
        ("right", _) | ("text-align", "right") => para.align = Some(Align::Right),
        ("left", _) | ("text-align", "left") => para.align = Some(Align::Left),
        ("justify", _) | ("text-align", "justify") => para.align = Some(Align::LeftJustified),
        ("font-family", v) if !v.is_empty() => chars.font_family = Some(v.trim_matches(['\'', '"']).into()),
        ("font-size", v) => {
            if let Some(n) = points(v).filter(|n| *n > 0.0 && *n <= 1000.0) {
                chars.size = Some(n);
            } else {
                return false;
            }
        }
        ("margin-left", v) | ("text-indent", v) | ("margin-right", v) | ("margin-top", v) | ("margin-bottom", v) => {
            let Some(n) = points(v) else { return false };
            match key {
                "margin-left" => para.left_indent = Some(n),
                "margin-right" => para.right_indent = Some(n),
                "text-indent" => para.first_line_indent = Some(n),
                "margin-top" => para.space_before = Some(n),
                _ => para.space_after = Some(n),
            }
        }
        _ => return false,
    }
    true
}
pub(super) fn points(s: &str) -> Option<f64> {
    let s = s.trim();
    let (v, scale) = [("pt", 1.0), ("px", 0.75), ("in", 72.0), ("cm", 72.0 / 2.54), ("mm", 72.0 / 25.4)]
        .iter()
        .find_map(|(u, scl)| s.strip_suffix(u).map(|v| (v, *scl)))
        .unwrap_or((s, 1.0));
    v.trim().parse::<f64>().ok().map(|n| n * scale).filter(|n| n.is_finite() && n.abs() <= 15552.0)
}
