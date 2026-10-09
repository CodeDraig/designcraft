//! Document → IDML package.

mod designmap;
use designmap::{anchor_self, topic_self, xref_format_self};
mod attributes;
mod fonts;
mod graphics;
mod preferences;
mod spreads;
mod styles;
use spreads::margin_el;
mod items;
use items::text_frame_pref;
mod stories;
mod tables;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;

use designcraft_color::{Color, ColorType, GradientKind, SwatchValue, swatch};
use designcraft_doc::{
    CharAttrs, CharFormat, ColumnsKind, Content, Document, Item, ListType, PageSide, ParaAttrs, ParaFormat, Rule, Shape, SpanColumns, Spread, Story,
    TextFrameOptions, story as st,
};
use designcraft_geom::{Affine, PathData, Rect};

use crate::names::{self, CHAR_BUILTINS, OBJECT_BUILTINS, PARA_BUILTINS, escape_id, num, pt};
use crate::xml::{El, Node, document, document_with_pi};
use crate::{DOM_VERSION, MIMETYPE, base64_encode};

const PKG_NS: &str = "http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging";
const NO_PARA_ID: &str = "ParagraphStyle/$ID/[No paragraph style]";
const NO_CHAR_ID: &str = "CharacterStyle/$ID/[No character style]";

/// Export options.
#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// Embed image data in the package (`<Contents>`). When false, images with a known link path
    /// are written as links only.
    pub embed_images: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions { embed_images: true }
    }
}

/// Export a document as an IDML package (ZIP bytes).
pub fn export_idml(doc: &Document) -> Vec<u8> {
    export_idml_with(doc, &ExportOptions::default())
}

pub fn export_idml_with(doc: &Document, opts: &ExportOptions) -> Vec<u8> {
    let mut ex = Ex::new(doc, opts);
    let parts = ex.build();
    zip_parts(&parts)
}

fn zip_parts(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut write = || -> zip::result::ZipResult<()> {
        w.start_file("mimetype", stored)?;
        w.write_all(MIMETYPE.as_bytes())?;
        for (name, data) in parts {
            w.start_file(name.as_str(), deflated)?;
            w.write_all(data)?;
        }
        Ok(())
    };
    // Writing into memory cannot fail except on internal errors; return what we have then.
    let _ = write();
    w.finish().map(|c| c.into_inner()).unwrap_or_default()
}

pub(crate) fn uid(n: u64) -> String {
    format!("u{n:x}")
}

fn p(name: &str, ty: &str, val: impl Into<String>) -> El {
    El::new(name).attr("type", ty).text(val)
}

/// Insert a `<Properties>` element as the first child when there are any.
fn with_props(mut el: El, props: Vec<El>) -> El {
    if !props.is_empty() {
        let mut pe = El::new("Properties");
        pe.children = props.into_iter().map(Node::El).collect();
        el.children.insert(0, Node::El(pe));
    }
    el
}

fn pkg_root(kind: &str) -> El {
    El::new(&format!("idPkg:{kind}")).attr("xmlns:idPkg", PKG_NS).attr("DOMVersion", DOM_VERSION)
}

fn bool_s(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn pct(v: f64) -> String {
    num(v * 100.0)
}

/// (model, space, value) for a colour.
fn color_value(c: &Color) -> (&'static str, String) {
    match *c {
        Color::Cmyk { c, m, y, k } => ("CMYK", format!("{} {} {} {}", pct(c as f64), pct(m as f64), pct(y as f64), pct(k as f64))),
        Color::Rgb { r, g, b } => ("RGB", format!("{} {} {}", num(r as f64 * 255.0), num(g as f64 * 255.0), num(b as f64 * 255.0))),
        Color::Gray { k } => ("CMYK", format!("0 0 0 {}", pct(k as f64))),
    }
}

/// The IDML spread origin in our spread coordinates.
pub(crate) fn spread_origin(sp: &Spread, facing: bool) -> (f64, f64) {
    let h = sp.pages.iter().map(|p| p.height).fold(0.0, f64::max);
    let b = sp.bounds();
    let ox = if facing {
        if let Some(r) = sp.pages.iter().find(|p| p.side == PageSide::Right) {
            r.x
        } else if let Some(l) = sp.pages.iter().rev().find(|p| p.side == PageSide::Left) {
            l.x + l.width
        } else {
            b.center().x
        }
    } else {
        b.center().x
    };
    (ox, h / 2.0)
}

fn affine_s(a: Affine) -> String {
    a.as_coeffs().iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")
}

struct Ex<'a> {
    d: &'a Document,
    opts: &'a ExportOptions,
    next: u64,
    swatch_ids: HashMap<String, String>,
    fonts: BTreeMap<String, BTreeSet<String>>,
    /// Item id → (prev frame, next frame) for threaded text frames.
    threads: HashMap<u64, (Option<u64>, Option<u64>)>,
    /// Cross-reference sources written so far: (Self, target anchor).
    xref_sources: Vec<(String, u64)>,
    /// Index topics (paths) and their See / See also cross-references.
    index_topics: BTreeSet<Vec<String>>,
    index_xrefs: Vec<(Vec<String>, &'static str, String)>,
}

impl<'a> Ex<'a> {
    fn new(d: &'a Document, opts: &'a ExportOptions) -> Self {
        let mut ex = Ex {
            d,
            opts,
            next: d.next_id.saturating_add(0x100),
            swatch_ids: HashMap::new(),
            fonts: BTreeMap::new(),
            threads: HashMap::new(),
            xref_sources: Vec::new(),
            index_topics: BTreeSet::new(),
            index_xrefs: Vec::new(),
        };
        ex.assign_swatch_ids();
        for s in d.stories.values() {
            for (i, f) in s.frames.iter().enumerate() {
                let prev = if i > 0 { Some(s.frames[i - 1].0) } else { None };
                let next = s.frames.get(i + 1).map(|f| f.0);
                ex.threads.insert(f.0, (prev, next));
            }
        }
        ex
    }

    fn fresh(&mut self) -> String {
        self.next += 1;
        uid(self.next)
    }

    fn build(&mut self) -> Vec<(String, Vec<u8>)> {
        let d = self.d;
        let mut parts = Vec::new();
        // Stories first (they register fonts).
        let mut story_parts = Vec::new();
        for s in d.stories.values() {
            let name = format!("Stories/Story_{}.xml", uid(s.id.0));
            let el = self.story_el(s);
            story_parts.push((name, document(&pkg_root("Story").child(el))));
        }
        let styles = self.styles_part();
        let mut masters = Vec::new();
        for sp in &d.parents {
            let name = format!("MasterSpreads/MasterSpread_{}.xml", uid(sp.id.0));
            let el = self.spread_el(sp, true);
            masters.push((name, document(&pkg_root("MasterSpread").child(el))));
        }
        let mut spreads = Vec::new();
        for (si, sp) in d.spreads.iter().enumerate() {
            let name = format!("Spreads/Spread_{}.xml", uid(sp.id.0));
            let el = self.spread_el_doc(sp, si);
            spreads.push((name, document(&pkg_root("Spread").child(el))));
        }
        let graphic = self.graphic_part();
        let fonts = self.fonts_part();
        let prefs = self.prefs_part();
        let designmap = self.designmap(&masters, &spreads, &story_parts);

        parts.push(("META-INF/container.xml".to_string(), CONTAINER.as_bytes().to_vec()));
        parts.push(("designmap.xml".to_string(), designmap));
        parts.push(("Resources/Graphic.xml".to_string(), graphic));
        parts.push(("Resources/Fonts.xml".to_string(), fonts));
        parts.push(("Resources/Styles.xml".to_string(), styles));
        parts.push(("Resources/Preferences.xml".to_string(), prefs));
        parts.extend(masters);
        parts.extend(spreads);
        parts.extend(story_parts);
        parts
    }

    // ---------- designmap ----------

    // ---------- Graphic.xml ----------

    // ---------- Fonts.xml ----------

    // ---------- Preferences.xml ----------

    // ---------- Styles.xml ----------

    // ---------- spreads ----------

    // ---------- stories ----------
}

/// IDML corner names for anchors 0..4 of a path (by position on its bounding box).
pub(crate) fn corner_names(path: &PathData) -> [&'static str; 4] {
    let default = ["TopLeft", "TopRight", "BottomRight", "BottomLeft"];
    let Some(sp) = path.subpaths.first() else { return default };
    if sp.anchors.len() != 4 {
        return default;
    }
    let Some(b) = path.bounds() else { return default };
    let c = b.center();
    let mut out = default;
    let mut seen = HashSet::new();
    for (i, a) in sp.anchors.iter().enumerate() {
        let n = match (a.p.y < c.y, a.p.x < c.x) {
            (true, true) => "TopLeft",
            (true, false) => "TopRight",
            (false, false) => "BottomRight",
            (false, true) => "BottomLeft",
        };
        if !seen.insert(n) {
            return default;
        }
        out[i] = n;
    }
    out
}

/// File path → IDML link URI (`file:/abs/path` with spaces percent-encoded).
pub(crate) fn path_to_uri(path: &str) -> String {
    if path.contains(':') && !path.starts_with('/') && path.chars().nth(1) != Some(':') {
        return path.to_string(); // already a URI
    }
    let mut out = String::from("file:");
    let p = path.replace('\\', "/");
    for c in p.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            c => out.push(c),
        }
    }
    out
}

const CONTAINER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n\t<rootfiles>\n\t\t<rootfile full-path=\"designmap.xml\" media-type=\"text/xml\"/>\n\t</rootfiles>\n</container>\n";

const STROKE_STYLES: &[&str] = &[
    "Triple_Stroke",
    "ThickThinThick",
    "ThinThickThin",
    "ThickThick",
    "ThickThin",
    "ThinThick",
    "ThinThin",
    "Japanese Dots",
    "White Diamond",
    "Left Slant Hash",
    "Right Slant Hash",
    "Straight Hash",
    "Wavy",
    "Canned Dotted",
    "Canned Dashed 3x2",
    "Canned Dashed 4x4",
    "Dashed",
    "Solid",
];
