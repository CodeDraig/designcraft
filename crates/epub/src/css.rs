//! Style class naming and CSS generation.

use super::{Align, BTreeMap, CharAttrs, CharProps, Document, ParaProps, Styles, story};
use std::fmt::Write as _;

/// CSS class name for a style name.
pub fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "basic".into()
    } else if s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("s-{s}")
    } else {
        s
    }
}

/// Class names for `names`, in order: [`slug`]s made unique with a `-2`, `-3`… suffix, so styles
/// whose names slug alike ("Body Text" / "Body-Text", or non-ASCII names) keep their own rules.
fn class_names<'a>(names: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, String> {
    let mut out = BTreeMap::new();
    let mut used = std::collections::BTreeSet::new();
    for name in names {
        let base = slug(name);
        let mut class = base.clone();
        let mut n = 1;
        while !used.insert(class.clone()) {
            n += 1;
            class = format!("{base}-{n}");
        }
        out.insert(name, class);
    }
    out
}

pub(super) fn para_classes(st: &Styles) -> BTreeMap<&str, String> {
    class_names(st.paragraph.iter().map(|s| s.name.as_str()))
}

pub(super) fn char_classes(st: &Styles) -> BTreeMap<&str, String> {
    class_names(st.character.iter().map(|s| s.name.as_str()))
}

fn hex(doc: &Document, swatch: &str, tint: f32) -> Option<String> {
    let c = doc.resolve_color(swatch, tint)?;
    let [r, g, b, _] = c.to_rgba8(1.0);
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

// CSS is embedded directly in HTML as well as stored in EPUB. Escape user style names and
// font names as CSS tokens, including '<', so a font name cannot close the style element.
fn css_string(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_') {
            out.push(c);
        } else {
            let _ = write!(out, "\\{:x} ", c as u32);
        }
    }
    out
}

fn selector(class: &str) -> String {
    class.split_whitespace().map(|c| format!("[class~=\"{}\"]", css_string(c))).collect()
}

fn char_css(doc: &Document, c: &CharProps, base_size: f64, out: &mut String) {
    let _ = write!(out, "font-family: \"{}\", serif; ", css_string(&c.font_family));
    let st = c.font_style.to_lowercase();
    if st.contains("semibold") {
        out.push_str("font-weight: 600; ");
    } else if st.contains("bold") {
        out.push_str("font-weight: bold; ");
    }
    if st.contains("italic") {
        out.push_str("font-style: italic; ");
    }
    let _ = write!(out, "font-size: {:.3}em; ", c.size / base_size);
    if c.tracking != 0.0 {
        let _ = write!(out, "letter-spacing: {:.3}em; ", c.tracking / 1000.0);
    }
    match c.capitalization {
        designcraft_doc::Capitalization::AllCaps => out.push_str("text-transform: uppercase; "),
        designcraft_doc::Capitalization::SmallCaps | designcraft_doc::Capitalization::OpenTypeAllSmallCaps => {
            out.push_str("font-variant: small-caps; ")
        }
        _ => {}
    }
    if c.underline {
        out.push_str("text-decoration: underline; ");
    }
    if c.strikethrough {
        out.push_str("text-decoration: line-through; ");
    }
    if let Some(h) = hex(doc, &c.fill, c.fill_tint) {
        let _ = write!(out, "color: {h}; ");
    }
}

fn para_css(p: &ParaProps, base_size: f64, line_px: f64, out: &mut String) {
    let align = match p.align {
        Align::Left | Align::TowardsSpine => "left",
        Align::Center => "center",
        Align::Right | Align::AwayFromSpine => "right",
        _ => "justify",
    };
    let _ = write!(out, "text-align: {align}; ");
    let em = |v: f64| v / base_size;
    let _ = write!(
        out,
        "margin: {:.3}em {:.3}em {:.3}em {:.3}em; text-indent: {:.3}em; line-height: {:.3}; ",
        em(p.space_before),
        em(p.right_indent),
        em(p.space_after),
        em(p.left_indent),
        em(p.first_line_indent),
        line_px
    );
    out.push_str(if p.hyphenate { "hyphens: auto; -webkit-hyphens: auto; " } else { "hyphens: manual; " });
}

/// The stylesheet for all paragraph and character styles.
pub fn stylesheet(doc: &Document) -> String {
    let st: &Styles = &doc.styles;
    let (_, basic) = st.resolve_para_style(story::BASIC_PARAGRAPH);
    let base = basic.size.max(1.0);
    let mut css = String::from(
        "body { margin: 0 5%; font-size: 1em; }\nfigure { margin: 1em 0; text-align: center; }\nfigure img { max-width: 100%; }\nh1, h2, h3 { font-size: inherit; margin: 0; }\n",
    );
    let (pc, cc) = (para_classes(st), char_classes(st));
    for ps in &st.paragraph {
        if ps.name == designcraft_doc::NO_PARA_STYLE {
            continue;
        }
        let (pp, cp) = st.resolve_para_style(&ps.name);
        let lead = match cp.leading {
            designcraft_doc::Leading::Auto => pp.auto_leading,
            designcraft_doc::Leading::Points(v) => v / cp.size.max(1.0),
        };
        let mut rule = String::new();
        para_css(&pp, base, lead, &mut rule);
        char_css(doc, &cp, base, &mut rule);
        let export = st.export_tag(&ps.name, false);
        let class = export.filter(|e| !e.class.is_empty()).map_or(pc[ps.name.as_str()].as_str(), |e| e.class.as_str());
        let tag = export.map(|e| e.tag.as_str()).filter(|t| super::content::is_tag(t, false)).unwrap_or("p");
        let class = if export.is_some_and(|e| !e.class.is_empty()) { selector(class) } else { format!(".{class}") };
        let _ = writeln!(css, "{tag}{class} {{ {rule}}}");
    }
    for cs in &st.character {
        if cs.name == story::NO_CHAR_STYLE {
            continue;
        }
        let mut chain = Vec::new();
        let mut current = Some(cs);
        while let Some(style) = current {
            if chain.len() >= 64 || chain.iter().any(|s: &&designcraft_doc::CharacterStyle| s.name == style.name) {
                break;
            }
            chain.push(style);
            current = style.based_on.as_deref().and_then(|name| st.char_style(name));
        }
        let rule: String = chain.into_iter().rev().map(|s| override_css(doc, &s.chars)).collect();
        let export = st.export_tag(&cs.name, true);
        let class = export.filter(|e| !e.class.is_empty()).map_or(cc[cs.name.as_str()].as_str(), |e| e.class.as_str());
        let tag = export.map(|e| e.tag.as_str()).filter(|t| super::content::is_tag(t, true)).unwrap_or("span");
        let class = if export.is_some_and(|e| !e.class.is_empty()) { selector(class) } else { format!(".{class}") };
        let _ = writeln!(css, "{tag}{class} {{ {rule}}}");
    }
    css
}

pub(super) fn override_css(doc: &Document, o: &CharAttrs) -> String {
    let mut s = String::new();
    if let Some(f) = &o.font_style {
        let f = f.to_lowercase();
        if f.contains("semibold") {
            s.push_str("font-weight: 600; ");
        } else if f.contains("bold") {
            s.push_str("font-weight: bold; ");
        } else {
            s.push_str("font-weight: normal; ");
        }
        if f.contains("italic") {
            s.push_str("font-style: italic; ");
        } else {
            s.push_str("font-style: normal; ");
        }
    }
    if let Some(family) = &o.font_family {
        let _ = write!(s, "font-family: \"{}\", serif; ", css_string(family));
    }
    if let Some(size) = o.size {
        let _ = write!(s, "font-size: {size}pt; ");
    }
    if let Some(tracking) = o.tracking {
        let _ = write!(s, "letter-spacing: {}em; ", tracking / 1000.0);
    }
    if let Some(position) = o.position {
        match position {
            designcraft_doc::Position::Superscript => s.push_str("vertical-align: super; font-size: .75em; "),
            designcraft_doc::Position::Subscript => s.push_str("vertical-align: sub; font-size: .75em; "),
            _ => s.push_str("vertical-align: baseline; "),
        }
    }
    if let Some(fill) = &o.fill
        && let Some(h) = hex(doc, fill, o.fill_tint.unwrap_or(1.0))
    {
        let _ = write!(s, "color: {h}; ");
    }
    if o.underline.is_some() || o.strikethrough.is_some() {
        let values: Vec<_> = [(o.underline, "underline"), (o.strikethrough, "line-through")]
            .into_iter()
            .filter_map(|(on, name)| (on == Some(true)).then_some(name))
            .collect();
        let _ = write!(s, "text-decoration: {}; ", if values.is_empty() { "none".into() } else { values.join(" ") });
    }
    if o.capitalization == Some(designcraft_doc::Capitalization::AllCaps) {
        s.push_str("text-transform: uppercase; ");
    } else if matches!(o.capitalization, Some(designcraft_doc::Capitalization::SmallCaps | designcraft_doc::Capitalization::OpenTypeAllSmallCaps)) {
        s.push_str("font-variant: small-caps; ");
    }
    s.trim_end().to_string()
}

pub(super) fn paragraph_overrides(doc: &Document, p: &designcraft_doc::ParaFormat) -> String {
    if p.para.is_empty() && p.chars.is_empty() {
        return String::new();
    }
    let (pp, cp) = doc.styles.resolve_para(p);
    let (_, basic) = doc.styles.resolve_para_style(story::BASIC_PARAGRAPH);
    let lead = match cp.leading {
        designcraft_doc::Leading::Auto => pp.auto_leading,
        designcraft_doc::Leading::Points(v) => v / cp.size.max(1.0),
    };
    let mut css = String::new();
    para_css(&pp, basic.size.max(1.0), lead, &mut css);
    css.push_str(&override_css(doc, &p.chars));
    css
}
