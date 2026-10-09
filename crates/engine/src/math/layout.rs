//! Math box measurement and placement.

use super::{Class, FontFace, Node};

/// A positioned piece of the formula (baseline-relative: y grows downwards).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Elem {
    Text {
        x: f64,
        y: f64,
        s: String,
        size: f64,
        italic: bool,
    },
    Rule {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// A polyline (radical sign), stroke width `w`.
    Line {
        pts: Vec<(f64, f64)>,
        w: f64,
    },
}

/// A laid-out box: width, height above and depth below the baseline.
#[derive(Clone, Debug, Default)]
pub(super) struct MBox {
    pub(super) w: f64,
    pub(super) asc: f64,
    pub(super) desc: f64,
    pub(super) els: Vec<Elem>,
}

impl MBox {
    fn shifted(mut self, dx: f64, dy: f64) -> Vec<Elem> {
        for e in &mut self.els {
            match e {
                Elem::Text { x, y, .. } | Elem::Rule { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
                Elem::Line { pts, .. } => {
                    for p in pts {
                        p.0 += dx;
                        p.1 += dy;
                    }
                }
            }
        }
        self.els
    }
}

/// Typesetting fonts and metrics.
pub(super) struct Fonts {
    pub(super) upright: std::sync::Arc<FontFace>,
    pub(super) italic: std::sync::Arc<FontFace>,
    pub(super) fallback: std::sync::Arc<FontFace>,
}

impl Fonts {
    pub(super) fn face(&self, c: char, italic: bool) -> &FontFace {
        let f = if italic { &self.italic } else { &self.upright };
        if f.glyph_for(c) != 0 { f } else { &self.fallback }
    }
    pub(super) fn width(&self, s: &str, size: f64, italic: bool) -> f64 {
        s.chars()
            .map(|c| {
                let f = self.face(c, italic);
                f.advance(f.glyph_for(c)) / f.units_per_em().max(1.0) * size
            })
            .sum()
    }
}

pub(super) const FAMILY: &str = designcraft_fonts::DEFAULT_FAMILY;
pub(super) const FALLBACK: &str = "Source Sans 3";

fn text_box(f: &Fonts, s: &str, size: f64, italic: bool) -> MBox {
    let w = f.width(s, size, italic);
    MBox { w, asc: size * 0.72, desc: size * 0.22, els: vec![Elem::Text { x: 0.0, y: 0.0, s: s.to_string(), size, italic }] }
}

fn hbox(parts: Vec<(MBox, f64)>) -> MBox {
    // (box, space before) laid out along the baseline.
    let mut out = MBox::default();
    let mut x = 0.0;
    for (b, before) in parts {
        x += before;
        out.asc = out.asc.max(b.asc);
        out.desc = out.desc.max(b.desc);
        let w = b.w;
        out.els.extend(b.shifted(x, 0.0));
        x += w;
    }
    out.w = x.max(0.0);
    out
}

pub(super) fn layout(f: &Fonts, n: &Node, size: f64) -> MBox {
    let axis = size * 0.25;
    match n {
        Node::Sym(s, cl) => text_box(f, s, size, *cl == Class::Ident),
        Node::Text(s) => text_box(f, s, size, false),
        Node::Space(em) => MBox { w: em * size, ..Default::default() },
        Node::Group(list) => {
            let mut parts = Vec::new();
            let mut prev: Option<Class> = None;
            for (k, item) in list.iter().enumerate() {
                let cl = match item {
                    Node::Sym(_, c) => Some(*c),
                    Node::Scripts { base, .. } => match &**base {
                        Node::Sym(_, c) => Some(*c),
                        _ => None,
                    },
                    _ => None,
                };
                // A leading + or − (or after another operator) is a sign, not a binary operator.
                let cl = if cl == Some(Class::Bin) && (k == 0 || matches!(prev, Some(Class::Bin | Class::Rel | Class::Punct))) {
                    Some(Class::Plain)
                } else {
                    cl
                };
                let gap = |c: Option<Class>| match c {
                    Some(Class::Bin) => size * 4.0 / 18.0,
                    Some(Class::Rel) => size * 5.0 / 18.0,
                    _ => 0.0,
                };
                let mut before = gap(cl).max(gap(prev));
                if k == 0 {
                    before = 0.0;
                }
                if prev == Some(Class::Punct) {
                    before = before.max(size * 3.0 / 18.0);
                }
                parts.push((layout(f, item, size), before));
                prev = cl;
            }
            hbox(parts)
        }
        Node::Frac(a, b) => {
            let s2 = size * 0.85;
            let (na, nb) = (layout(f, a, s2), layout(f, b, s2));
            let w = na.w.max(nb.w) + size * 0.2;
            let t = (size * 0.06).max(0.4);
            let gap = size * 0.15;
            let num_base = -(axis + t / 2.0 + gap + na.desc);
            let den_base = -axis + t / 2.0 + gap + nb.asc;
            let mut out = MBox { w, asc: -num_base + na.asc, desc: den_base + nb.desc, els: Vec::new() };
            out.els.push(Elem::Rule { x: 0.0, y: -axis - t / 2.0, w, h: t });
            let (wa, wb) = (na.w, nb.w);
            out.els.extend(na.shifted((w - wa) / 2.0, num_base));
            out.els.extend(nb.shifted((w - wb) / 2.0, den_base));
            out
        }
        Node::Sqrt(idx, a) => {
            let inner = layout(f, a, size);
            let t = (size * 0.05).max(0.4);
            let gap = size * 0.12;
            let top = inner.asc + gap + t;
            let bottom = inner.desc;
            let hook = size * 0.55;
            let mut out = MBox { w: hook + inner.w + size * 0.1, asc: top, desc: bottom, els: Vec::new() };
            // The radical: a short rise, the long stroke down, then up to the bar.
            out.els.push(Elem::Line {
                pts: vec![(0.0, -axis * 0.6), (hook * 0.25, -axis), (hook * 0.55, bottom), (hook, -top + t / 2.0), (out.w, -top + t / 2.0)],
                w: t,
            });
            let iw = inner.w;
            let _ = iw;
            out.els.extend(inner.shifted(hook, 0.0));
            if let Some(i) = idx {
                let ib = layout(f, i, size * 0.55);
                let (ia, iw2) = (ib.asc, ib.w);
                out.els.extend(ib.shifted((hook * 0.3 - iw2).max(0.0) - 0.0, -axis - size * 0.1));
                out.asc = out.asc.max(axis + size * 0.1 + ia);
            }
            out
        }
        Node::Big(op) => {
            let mut b = text_box(f, op, size * 1.5, false);
            b.asc = size * 1.05;
            b.desc = size * 0.35;
            for e in &mut b.els {
                if let Elem::Text { y, .. } = e {
                    // Centre the glyph on the math axis.
                    *y = size * 0.32;
                }
            }
            b
        }
        Node::Scripts { base, sup, sub } => {
            let big = matches!(&**base, Node::Big(_));
            let b = layout(f, base, size);
            let s2 = size * 0.7;
            let (bw, basc, bdesc) = (b.w, b.asc, b.desc);
            let mut out = MBox { w: bw, asc: basc, desc: bdesc, els: b.els };
            if big {
                // Limits above and below.
                let mut w = bw;
                if let Some(sp) = sup {
                    let sb = layout(f, sp, s2);
                    let y = -basc - size * 0.1 - sb.desc;
                    w = w.max(sb.w);
                    let (sw, sa) = (sb.w, sb.asc);
                    out.els.extend(sb.shifted((bw - sw) / 2.0, y));
                    out.asc = out.asc.max(-y + sa);
                }
                if let Some(sbn) = sub {
                    let sb = layout(f, sbn, s2);
                    let y = bdesc + size * 0.1 + sb.asc;
                    w = w.max(sb.w);
                    let (sw, sd) = (sb.w, sb.desc);
                    out.els.extend(sb.shifted((bw - sw) / 2.0, y));
                    out.desc = out.desc.max(y + sd);
                }
                // Centre the operator over the widest limit.
                let dx = (w - bw) / 2.0;
                let els = std::mem::take(&mut out.els);
                out = MBox { w, asc: out.asc, desc: out.desc, els: MBox { els, ..Default::default() }.shifted(dx, 0.0) };
                return out;
            }
            let mut w = bw;
            if let Some(sp) = sup {
                let sb = layout(f, sp, s2);
                let y = -(size * 0.42).max(basc - sb.asc * 0.6);
                w = w.max(bw + sb.w + size * 0.05);
                let sa = sb.asc;
                out.els.extend(sb.shifted(bw + size * 0.05, y));
                out.asc = out.asc.max(-y + sa);
            }
            if let Some(sbn) = sub {
                let sb = layout(f, sbn, s2);
                let y = (size * 0.2).max(bdesc * 0.8);
                w = w.max(bw + sb.w + size * 0.05);
                let sd = sb.desc;
                out.els.extend(sb.shifted(bw + size * 0.05, y));
                out.desc = out.desc.max(y + sd);
            }
            out.w = w;
            out
        }
        Node::Fenced(open, body, close) => {
            let inner = layout(f, body, size);
            let h = (inner.asc + inner.desc).max(size);
            let k = h / size;
            let ds = size * k.max(1.0);
            let mk = |s: &str| -> MBox {
                if s.is_empty() {
                    return MBox::default();
                }
                let mut b = text_box(f, s, ds, false);
                // Scaled delimiters centred on the content.
                let mid = (inner.desc - inner.asc) / 2.0;
                for e in &mut b.els {
                    if let Elem::Text { y, .. } = e {
                        *y = mid + ds * 0.25;
                    }
                }
                b.asc = inner.asc.max(size * 0.72);
                b.desc = inner.desc.max(size * 0.22);
                b
            };
            let (l, r) = (mk(open), mk(close));
            hbox(vec![(l, 0.0), (inner, size * 0.05), (r, size * 0.05)])
        }
    }
}
