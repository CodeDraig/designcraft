//! Math expressions: a LaTeX subset typeset into boxes and written as SVG (text in the bundled
//! serif, rules and radicals as paths). Covers fractions, super/subscripts, square and n-th roots,
//! big operators with limits, Greek letters, common symbols, `\text{}`, `\mathrm{}`, spacing
//! commands and `\left( … \right)`.

mod syntax;
use syntax::parse;
use syntax::{Class, Node};
mod layout;
use layout::layout;
use layout::{Elem, FALLBACK, FAMILY, Fonts};

use std::fmt::Write as _;

use designcraft_fonts::{FontDb, FontFace};

/// Typeset `latex` at `size` points: SVG bytes and the size in points.
pub fn to_svg(latex: &str, size: f64) -> (Vec<u8>, (f64, f64)) {
    let db = FontDb::global();
    let f = Fonts { upright: db.face(FAMILY, "Regular"), italic: db.face(FAMILY, "Italic"), fallback: db.face(FALLBACK, "Regular") };
    let b = layout(&f, &parse(latex), size);
    let pad = size * 0.1;
    let (w, h) = (b.w + 2.0 * pad, b.asc + b.desc + 2.0 * pad);
    let base = pad + b.asc;
    let mut svg = String::new();
    let _ = write!(svg, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.2}pt\" height=\"{h:.2}pt\" viewBox=\"0 0 {w:.3} {h:.3}\">");
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    for e in &b.els {
        match e {
            Elem::Text { x, y, s, size, italic } => {
                // Per character, so each uses a face that has it.
                let mut cx = *x;
                for c in s.chars() {
                    let face = f.face(c, *italic);
                    let fam = if std::ptr::eq(face, &*f.fallback) { FALLBACK } else { FAMILY };
                    let it = *italic && !std::ptr::eq(face, &*f.fallback);
                    let _ = write!(
                        svg,
                        "<text x=\"{:.3}\" y=\"{:.3}\" font-family=\"'{fam}'\" font-size=\"{size:.3}\"{}>{}</text>",
                        pad + cx,
                        base + y,
                        if it { " font-style=\"italic\"" } else { "" },
                        esc(&c.to_string())
                    );
                    cx += f.width(&c.to_string(), *size, *italic);
                }
            }
            Elem::Rule { x, y, w: rw, h: rh } => {
                let _ = write!(svg, "<rect x=\"{:.3}\" y=\"{:.3}\" width=\"{rw:.3}\" height=\"{rh:.3}\"/>", pad + x, base + y);
            }
            Elem::Line { pts, w: lw } => {
                let d: Vec<String> = pts.iter().map(|(x, y)| format!("{:.3},{:.3}", pad + x, base + y)).collect();
                let _ = write!(
                    svg,
                    "<polyline points=\"{}\" fill=\"none\" stroke=\"black\" stroke-width=\"{lw:.3}\" stroke-linejoin=\"round\"/>",
                    d.join(" ")
                );
            }
        }
    }
    svg.push_str("</svg>");
    (svg.into_bytes(), (w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_common_constructs() {
        let n = parse(r"\frac{a^2+b}{\sqrt[3]{x}}");
        let Node::Group(v) = n else { panic!() };
        assert!(matches!(&v[0], Node::Frac(_, _)));
        let n = parse(r"\sum_{i=1}^{n} i^2 \le \alpha");
        let Node::Group(v) = n else { panic!() };
        assert!(matches!(&v[0], Node::Scripts { base, sup: Some(_), sub: Some(_) } if matches!(**base, Node::Big(_))));
        assert!(v.iter().any(|x| *x == Node::Sym("≤".into(), Class::Rel)));
        assert!(v.iter().any(|x| *x == Node::Sym("α".into(), Class::Ident)));
        let n = parse(r"\left( \frac{1}{2} \right) + \text{if } x");
        let Node::Group(v) = n else { panic!() };
        assert!(matches!(&v[0], Node::Fenced(o, _, c) if o == "(" && c == ")"));
        assert!(v.contains(&Node::Text("if ".into())));
    }

    #[test]
    fn layout_sizes_and_svg() {
        let (svg, (w, h)) = to_svg(r"E = mc^2", 12.0);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.starts_with("<svg") && s.contains("font-style=\"italic\">E<") && s.contains(">2<"));
        assert!(w > 30.0 && h > 10.0, "{w} {h}");
        // A fraction is taller than its numerator alone; a big operator with limits taller still.
        let (_, (_, h1)) = to_svg("x", 12.0);
        let (_, (_, h2)) = to_svg(r"\frac{x}{y}", 12.0);
        let (_, (_, h3)) = to_svg(r"\sum_{i=0}^{n}", 12.0);
        assert!(h2 > h1 * 1.4 && h3 > h2, "{h1} {h2} {h3}");
        // The SVG parses and rasterises.
        let (svg, _) = to_svg(r"\sqrt{a^2+b^2} \ne \pi", 14.0);
        assert!(designcraft_images::svg_tree(&svg).is_some());
    }
}
