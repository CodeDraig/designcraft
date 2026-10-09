//! Loaded font bytes, metrics and stable glyph-facing handles.

use super::{Arc, AtomicU32, FontGroup, GlyphId, Location, LocationRef, Ordering, RwLock, Size, style_italic, style_weight};
use skrifa::MetadataProvider;

#[derive(Clone)]
pub(super) enum FontBytes {
    Static(&'static [u8]),
    Owned(Arc<Vec<u8>>),
}

/// Where a face's font came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontSource {
    /// Compiled into DesignCraft.
    Bundled,
    /// Font data handed to [`super::FontDb::add_font`], with no file behind it.
    Memory,
    /// An installed font file (the system's or the user's font folders).
    Installed(std::path::PathBuf),
    /// A font file in a document's `Document Fonts` folder ([`super::FontDb::load_document_fonts`]).
    Document(std::path::PathBuf),
}

impl FontSource {
    /// The font file, if the face was loaded from one.
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            FontSource::Installed(p) | FontSource::Document(p) => Some(p),
            FontSource::Bundled | FontSource::Memory => None,
        }
    }
}

/// One loaded font face.
pub struct FontFace {
    pub(super) id: u32,
    /// Typographic family name (e.g. "Source Sans 3").
    pub family: String,
    /// Typographic style name (e.g. "Semibold", "Italic").
    pub style: String,
    /// usWeightClass-style weight (400 = regular).
    pub weight: f32,
    pub italic: bool,
    /// Where the font came from (a file to copy when packaging, a document's own font).
    pub source: FontSource,
    pub(super) bytes: FontBytes,
    pub(super) index: u32,
    pub upem: f64,
    /// Ascender in font units (positive = up).
    pub ascent: f64,
    /// Descender in font units (positive = down).
    pub descent: f64,
    /// Cap height and x height in font units (estimated from the ascent when the font has no OS/2
    /// values).
    pub cap_height: f64,
    pub x_height: f64,
    pub shaper: harfrust::ShaperData,
    /// Variable fonts: the named instance's axis settings (user units; empty = default instance),
    /// the normalised location and the shaper's view of it.
    pub coords: Vec<([u8; 4], f32)>,
    location: Location,
    pub instance: Option<harfrust::ShaperInstance>,
    /// Basic Multilingual Plane coverage bitset, built on first use.
    bmp: std::sync::OnceLock<Box<[u64]>>,
    /// The ideographic em box, read on first use.
    em: std::sync::OnceLock<(f64, f64)>,
    /// The font menu group and native family name, read on first use.
    group: std::sync::OnceLock<(FontGroup, Option<String>)>,
}

/// A cheap `Copy` handle to a face. Faces are never unloaded, so the handle lives for the rest of
/// the process; unlike cloning an `Arc`, copying it touches no shared reference count (which made
/// parallel composition scale negatively).
#[derive(Clone, Copy)]
pub struct FaceRef(&'static FontFace);

impl FaceRef {
    /// The handle for a loaded face (memoised per face).
    pub fn of(face: &Arc<FontFace>) -> FaceRef {
        static LEAKED: RwLock<Vec<Option<&'static FontFace>>> = RwLock::new(Vec::new());
        let id = face.id as usize;
        if let Some(Some(f)) = LEAKED.read().unwrap_or_else(|e| e.into_inner()).get(id) {
            return FaceRef(f);
        }
        let mut w = LEAKED.write().unwrap_or_else(|e| e.into_inner());
        if w.len() <= id {
            w.resize(id + 1, None);
        }
        let f: &'static FontFace = w[id].unwrap_or_else(|| {
            let keep: &'static Arc<FontFace> = Box::leak(Box::new(face.clone()));
            keep
        });
        w[id] = Some(f);
        FaceRef(f)
    }
    pub fn get(self) -> &'static FontFace {
        self.0
    }
}

impl std::ops::Deref for FaceRef {
    type Target = FontFace;
    fn deref(&self) -> &FontFace {
        self.0
    }
}

impl std::fmt::Debug for FaceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for FaceRef {
    fn eq(&self, o: &FaceRef) -> bool {
        self.0.id == o.0.id
    }
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FontFace({} {})", self.family, self.style)
    }
}

impl FontFace {
    pub fn data(&self) -> &[u8] {
        match &self.bytes {
            FontBytes::Static(b) => b,
            FontBytes::Owned(v) => v.as_slice(),
        }
    }
    pub fn skrifa(&self) -> Option<skrifa::FontRef<'_>> {
        skrifa::FontRef::from_index(self.data(), self.index).ok()
    }
    pub fn hb(&self) -> Option<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(self.data(), self.index).ok()
    }
    /// Where in the design space glyphs, metrics and outlines come from.
    pub fn location(&self) -> LocationRef<'_> {
        (&self.location).into()
    }
    /// Is this a named instance of a variable font?
    pub fn is_variable(&self) -> bool {
        !self.coords.is_empty()
    }
    /// Face index within the font file (collections); 0 for plain fonts.
    pub fn index(&self) -> u32 {
        self.index
    }
    /// Unique id of this face within the process.
    pub fn id(&self) -> u32 {
        self.id
    }
    /// Does the face map `c` to a glyph?
    pub fn covers(&self, c: char) -> bool {
        let cp = c as u32;
        if cp < 0x1_0000 {
            let bits = self.bmp.get_or_init(|| {
                let mut b = vec![0u64; 1024].into_boxed_slice();
                if let Some(f) = self.skrifa() {
                    for (cp, _) in f.charmap().mappings() {
                        if cp < 0x1_0000 {
                            b[(cp / 64) as usize] |= 1 << (cp % 64);
                        }
                    }
                }
                b
            });
            return bits[(cp / 64) as usize] & (1 << (cp % 64)) != 0;
        }
        self.skrifa().is_some_and(|f| f.charmap().map(c).is_some())
    }
    /// Units per em.
    pub fn units_per_em(&self) -> f64 {
        self.upem
    }
    /// (ascent, descent) in font units, both positive.
    pub fn vertical_metrics(&self) -> (f64, f64) {
        (self.ascent, self.descent)
    }
    /// Every mapped character and its glyph id, sorted by code point (the Glyphs panel).
    pub fn chars(&self) -> Vec<(char, u32)> {
        let Some(f) = self.skrifa() else { return vec![] };
        let mut v: Vec<(char, u32)> = f.charmap().mappings().filter_map(|(cp, g)| char::from_u32(cp).map(|c| (c, g.to_u32()))).collect();
        v.sort_unstable_by_key(|x| x.0);
        v.dedup_by_key(|x| x.0);
        v
    }
    /// Advance width of glyph `gid` in font units.
    pub fn advance(&self, gid: u32) -> f64 {
        self.skrifa()
            .and_then(|f| f.glyph_metrics(Size::unscaled(), self.location()).advance_width(GlyphId::new(gid)))
            .map(|a| a as f64)
            .unwrap_or(self.upem * 0.5)
    }
    /// Vertical advance (down the line) of glyph `gid` in font units: `vmtx`, else one em.
    pub fn v_advance(&self, gid: u32) -> f64 {
        crate::vertical::VMetrics::new(self).advance(GlyphId::new(gid))
    }
    /// Height of glyph `gid`'s vertical origin above its baseline in font units: `VORG`, else its
    /// top plus its `vmtx` top side bearing, else the em box top. Across, the origin is at half the
    /// horizontal advance. Upright glyphs in vertical text hang from it on the line's centre.
    pub fn v_origin(&self, gid: u32) -> f64 {
        crate::vertical::VMetrics::new(self).origin(GlyphId::new(gid))
    }
    /// The ideographic em box (top, bottom) in font units, y up (Japanese fonts: typically 880,
    /// -120): `BASE` `idtp`/`ideo`, else the OS/2 typo ascender/descender, else the ascender and
    /// descender, the last two centred on one em.
    pub fn em_box(&self) -> (f64, f64) {
        *self.em.get_or_init(|| crate::vertical::em_box(self))
    }
    /// The font menu group the font is for and, for a CJK group, its family name in that language
    /// (see [`crate::FamilyInfo`]); read from the font once.
    pub fn menu_group(&self) -> (FontGroup, Option<&str>) {
        let (g, n) = self.group.get_or_init(|| self.skrifa().map(|f| crate::group::classify(&f, &self.family)).unwrap_or_default());
        (*g, n.as_deref())
    }
    /// Glyph id for `c` (0 = .notdef).
    pub fn glyph_for(&self, c: char) -> u32 {
        self.skrifa().and_then(|f| f.charmap().map(c)).map(|g| g.to_u32()).unwrap_or(0)
    }

    /// GDEF identifies marks even when shaping merges their source cluster with a base.
    pub fn glyph_is_mark(&self, gid: u32) -> bool {
        use skrifa::raw::TableProvider;
        self.skrifa()
            .and_then(|f| f.gdef().ok())
            .and_then(|gdef| gdef.glyph_class_def()?.ok())
            .is_some_and(|classes| classes.get(GlyphId::new(gid)) == 3)
    }

    /// Does the font's licence forbid passing it on? OS/2 `fsType` Restricted License embedding
    /// (bit 1) without a less restrictive bit (Preview & Print, Editable). Fonts without an OS/2
    /// table aren't restricted.
    pub fn restricted_licence(&self) -> bool {
        use skrifa::raw::TableProvider;
        let fs_type = self.skrifa().and_then(|f| f.os2().ok()).map_or(0, |t| t.fs_type());
        fs_type & 0x0002 != 0 && fs_type & 0x000C == 0
    }
}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub(super) fn make_face(
    bytes: FontBytes,
    source: FontSource,
    index: u32,
    family: String,
    style: String,
    coords: Vec<([u8; 4], f32)>,
) -> Option<FontFace> {
    let data: &[u8] = match &bytes {
        FontBytes::Static(b) => b,
        FontBytes::Owned(v) => v.as_slice(),
    };
    let f = skrifa::FontRef::from_index(data, index).ok()?;
    let settings: Vec<(skrifa::Tag, f32)> = coords.iter().map(|(t, v)| (skrifa::Tag::new(t), *v)).collect();
    let location = if coords.is_empty() { Location::default() } else { f.axes().location(settings.iter().copied()) };
    let m = f.metrics(Size::unscaled(), &location);
    let a = f.attributes();
    let hb = harfrust::FontRef::from_index(data, index).ok()?;
    let shaper = harfrust::ShaperData::new(&hb);
    let instance = (!coords.is_empty())
        .then(|| harfrust::ShaperInstance::from_variations(&hb, settings.iter().map(|(t, v)| harfrust::Variation { tag: *t, value: *v })));
    let axis = |tag: &[u8; 4]| coords.iter().find(|(t, _)| t == tag).map(|(_, v)| *v);
    let weight = axis(b"wght").unwrap_or(if coords.is_empty() { a.weight.value() } else { style_weight(&style) });
    let italic = axis(b"ital")
        .map(|v| v >= 0.5)
        .or(axis(b"slnt").map(|v| v.abs() > 0.1))
        .unwrap_or(!matches!(a.style, skrifa::attribute::Style::Normal) || style_italic(&style));
    Some(FontFace {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        family,
        style,
        weight,
        italic,
        upem: m.units_per_em.max(1) as f64,
        ascent: m.ascent as f64,
        descent: -(m.descent as f64),
        cap_height: m.cap_height.map(|v| v as f64).filter(|v| *v > 0.0).unwrap_or(m.ascent as f64 * 0.72),
        x_height: m.x_height.map(|v| v as f64).filter(|v| *v > 0.0).unwrap_or(m.ascent as f64 * 0.5),
        shaper,
        coords,
        location,
        instance,
        source,
        bytes,
        index,
        bmp: std::sync::OnceLock::new(),
        em: std::sync::OnceLock::new(),
        group: std::sync::OnceLock::new(),
    })
}
