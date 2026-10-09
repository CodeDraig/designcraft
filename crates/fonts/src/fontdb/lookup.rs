//! Scoped face selection, font menus, and variable instances.

use super::{Arc, BezPath, FALLBACK_FAMILY, FamilyInfo, FontDb, FontFace, cjk_chain, last_resort_face, make_face};
use skrifa::MetadataProvider;

/// One document's view of the [`FontDb`] ([`FontDb::scoped`]): the fonts of its `Document Fonts`
/// folder, then the shared fonts (bundled, installed, added). Other documents' fonts aren't in it.
#[derive(Clone)]
pub struct ScopedFonts<'a> {
    pub(super) db: &'a FontDb,
    /// The document's own faces, in the folder's file order (none without a scope).
    pub(super) own: Option<Arc<[Arc<FontFace>]>>,
}

impl ScopedFonts<'_> {
    fn own(&self) -> &[Arc<FontFace>] {
        self.own.as_deref().unwrap_or(&[])
    }

    /// Family names available (the document's, loaded and cataloged system fonts), sorted and
    /// deduplicated.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.own().iter().chain(self.db.read_faces().iter()).map(|f| f.family.clone()).collect();
        #[cfg(not(target_arch = "wasm32"))]
        v.extend(self.db.read_catalog().iter().map(|c| c.family.clone()));
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    /// The available families as the font menus list them (see [`ScopedFonts::families`]), each
    /// with its group and native name, unsorted ([`crate::sort_for_menu`] orders them).
    pub fn family_infos(&self) -> Vec<FamilyInfo> {
        let mut out: Vec<FamilyInfo> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for f in self.own().iter().chain(self.db.read_faces().iter()) {
            if seen.insert(f.family.to_lowercase()) {
                let (group, native) = f.menu_group();
                out.push(FamilyInfo { family: f.family.clone(), group, native: native.map(str::to_string) });
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        for c in self.db.read_catalog().iter() {
            if seen.insert(c.family.to_lowercase()) {
                out.push(FamilyInfo { family: c.family.clone(), group: c.group, native: c.native.clone() });
            }
        }
        out
    }

    /// Style names available for `family` (Regular first, then by weight).
    pub fn styles(&self, family: &str) -> Vec<String> {
        let mut v: Vec<(bool, f32, String)> = self
            .own()
            .iter()
            .chain(self.db.read_faces().iter())
            .filter(|f| f.family.eq_ignore_ascii_case(family) && !f.style.contains('{'))
            .map(|f| (f.italic, f.weight, f.style.clone()))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        for c in self.db.read_catalog().iter() {
            if c.family.eq_ignore_ascii_case(family) && !v.iter().any(|(_, _, s)| s.eq_ignore_ascii_case(&c.style)) {
                v.push((style_italic(&c.style), style_weight(&c.style), c.style.clone()));
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        v.dedup_by(|a, b| a.2 == b.2);
        v.into_iter().map(|t| t.2).collect()
    }

    /// Resolve a family + style to a face, falling back to the closest style of the family, then to
    /// Source Sans 3 Regular. Installed system fonts are found by name whatever ran before.
    pub fn face(&self, family: &str, style: &str) -> Arc<FontFace> {
        if let Some(f) = self.find(family, style) {
            return f;
        }
        #[cfg(all(test, not(target_arch = "wasm32")))]
        if let Some(barrier) = &self.db.first_lookup_barrier {
            barrier.wait();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.db.load_cataloged(family);
            // Another lookup may have inserted the family while this one was loading it.
            // Zero newly inserted faces does not mean that the family is unavailable.
            if let Some(f) = self.find(family, style) {
                return f;
            }
        }
        self.find(FALLBACK_FAMILY, style)
            .or_else(|| self.find(FALLBACK_FAMILY, "Regular"))
            .or_else(|| self.db.read_faces().first().cloned())
            .unwrap_or_else(last_resort_face)
    }

    /// Is `family` available (the document's, loaded, or installed on the system)?
    pub fn has_family(&self, family: &str) -> bool {
        if self.own().iter().any(|f| f.family.eq_ignore_ascii_case(family)) || self.db.is_loaded(family) {
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.db.read_catalog().iter().any(|c| c.family.eq_ignore_ascii_case(family)) {
            return true;
        }
        false
    }

    /// A variable font's axes: (tag, name, min, default, max), empty for static fonts.
    pub fn axes(&self, family: &str, style: &str) -> Vec<(String, String, f32, f32, f32)> {
        let face = self.face(family, base_style(style));
        let Some(f) = face.skrifa() else { return vec![] };
        f.axes()
            .iter()
            .map(|a| {
                let tag = String::from_utf8_lossy(&a.tag().to_be_bytes()).to_string();
                let name = f.localized_strings(a.name_id()).english_or_first().map(|s| s.to_string()).unwrap_or_else(|| tag.clone());
                (tag, name, a.min_value(), a.default_value(), a.max_value())
            })
            .collect()
    }

    /// Glyph outline in font units, y-down (flipped), cached per (face, glyph); see
    /// [`FontDb::outline`].
    pub fn outline(&self, face: &FontFace, gid: u32) -> Arc<BezPath> {
        self.db.outline(face, gid)
    }

    /// See [`FontDb::missing_box`].
    pub fn missing_box(&self, face: &FontFace) -> Option<BezPath> {
        self.db.missing_box(face)
    }

    /// `Style {wght:650,wdth:90}`: the named style's font at those axis values, made on first use.
    fn instance(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        let open = style.find('{')?;
        let base = self.find(family, style[..open].trim())?;
        if base.skrifa().is_none_or(|f| f.axes().is_empty()) {
            return Some(base);
        }
        let key = (base.id, style.to_string());
        if let Some(f) = self.db.instances.read().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Some(f.clone());
        }
        let mut coords = base.coords.clone();
        for kv in style[open + 1..].trim_end_matches('}').split(',') {
            let (k, v) = kv.split_once(':')?;
            let k = k.trim().as_bytes();
            let v: f32 = v.trim().parse().ok()?;
            if k.len() != 4 {
                return None;
            }
            let tag = [k[0], k[1], k[2], k[3]];
            match coords.iter_mut().find(|(t, _)| *t == tag) {
                Some(c) => c.1 = v,
                None => coords.push((tag, v)),
            }
        }
        let f = make_face(base.bytes.clone(), base.source.clone(), base.index, base.family.clone(), style.to_string(), coords)?;
        Some(self.db.instances.write().unwrap_or_else(|e| e.into_inner()).entry(key).or_insert_with(|| Arc::new(f)).clone())
    }

    fn find(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        if style.contains('{') {
            return self.instance(family, style);
        }
        let faces = self.db.read_faces();
        // The document's fonts first: they stand in for other fonts of the same name.
        let named = |f: &&Arc<FontFace>| f.family.eq_ignore_ascii_case(family);
        let cands: Vec<&Arc<FontFace>> = self.own().iter().filter(named).chain(faces.iter().filter(named)).collect();
        if cands.is_empty() {
            return None;
        }
        let ns = norm(style);
        if let Some(f) = cands.iter().find(|f| norm(&f.style) == ns) {
            return Some((*f).clone());
        }
        let (tw, ti) = (style_weight(style), style_italic(style));
        cands
            .iter()
            .min_by(|a, b| {
                let sa = (a.weight - tw).abs() + if a.italic != ti { 1000.0 } else { 0.0 };
                let sb = (b.weight - tw).abs() + if b.italic != ti { 1000.0 } else { 0.0 };
                sa.total_cmp(&sb)
            })
            .map(|f| (*f).clone())
    }

    /// A face other than `exclude` that covers `c` in `language` (a BCP 47 tag, `None` when
    /// unknown). CJK characters try their language's chain first (see `cjk_chain`); then the
    /// first face (fallback family first, then load order, the document's fonts last) that covers
    /// `c`. On native, system fonts are cataloged and loaded lazily the first time no loaded face
    /// covers a character.
    pub fn fallback_for(&self, c: char, exclude: u32, language: Option<&str>) -> Option<Arc<FontFace>> {
        if let Some(chain) = cjk_chain(c, language)
            && let Some(f) = chain.iter().find_map(|family| self.chain_face(family, c, exclude))
        {
            return Some(f);
        }
        if let Some(f) = self.loaded_fallback(c, exclude) {
            return Some(f);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.db.system_fallback(c) {
            return self.loaded_fallback(c, exclude);
        }
        None
    }

    /// The face of `family` (the document's first, regular first) that covers `c`, loading the
    /// installed family the first time a chain asks for it (native, while the system fallback is
    /// on).
    fn chain_face(&self, family: &'static str, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.db.is_loaded(family) {
            // Held while loading, so a parallel lookup waits for the font instead of passing it by.
            let mut sys = self.db.sys.lock().unwrap_or_else(|e| e.into_inner());
            if sys.enabled && sys.chain_tried.insert(family) {
                self.db.load_cataloged(family);
            }
        }
        let faces = self.db.read_faces();
        let mut order: Vec<&Arc<FontFace>> =
            self.own().iter().chain(faces.iter()).filter(|f| f.id != exclude && f.family.eq_ignore_ascii_case(family)).collect();
        order.sort_by_key(|f| (f.italic, (f.weight - 400.0).abs() as i32));
        order.into_iter().find(|f| f.covers(c)).cloned()
    }

    fn loaded_fallback(&self, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        let faces = self.db.read_faces();
        let mut order: Vec<&Arc<FontFace>> = faces.iter().chain(self.own().iter()).filter(|f| f.id != exclude).collect();
        order.sort_by_key(|f| (!f.family.eq_ignore_ascii_case(FALLBACK_FAMILY), f.italic, (f.weight - 400.0).abs() as i32));
        order.into_iter().find(|f| f.covers(c)).cloned()
    }
}

/// The named style part of a style with axis settings (`Bold {wght:650}` → `Bold`).
pub fn base_style(style: &str) -> &str {
    style.split('{').next().unwrap_or(style).trim()
}

pub(super) fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Weight implied by a style name.
pub(super) fn style_weight(style: &str) -> f32 {
    let s = norm(style);
    const TABLE: &[(&str, f32)] = &[
        ("extralight", 200.0),
        ("ultralight", 200.0),
        ("semibold", 600.0),
        ("demibold", 600.0),
        ("extrabold", 800.0),
        ("ultrabold", 800.0),
        ("hairline", 100.0),
        ("thin", 100.0),
        ("light", 300.0),
        ("medium", 500.0),
        ("bold", 700.0),
        ("black", 900.0),
        ("heavy", 900.0),
    ];
    TABLE.iter().find(|(k, _)| s.contains(k)).map(|(_, w)| *w).unwrap_or(400.0)
}

pub(super) fn style_italic(style: &str) -> bool {
    let s = norm(style);
    s.contains("italic") || s.contains("oblique") || s == "it"
}
