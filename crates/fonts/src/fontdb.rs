//! Font database: bundled OFL fonts, user fonts, the installed system fonts (cataloged the first
//! time a lookup needs them, native only), outline cache.

mod fallbacks;
#[cfg(not(target_arch = "wasm32"))]
use fallbacks::SYSTEM_FALLBACKS;
use fallbacks::cjk_chain;
mod parsing;
use parsing::enumerate_faces;
mod lookup;
pub use lookup::{ScopedFonts, base_style};
use lookup::{style_italic, style_weight};

mod face;
use face::FontBytes;
use face::make_face;
pub use face::{FaceRef, FontFace, FontSource};
mod discovery;
#[cfg(not(target_arch = "wasm32"))]
use discovery::file_face_names;
pub use discovery::system_font_dirs;
mod outline;
use outline::font_outline;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use kurbo::BezPath;
use skrifa::GlyphId;
#[cfg(not(target_arch = "wasm32"))]
use skrifa::MetadataProvider;
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::string::StringId;

use crate::group::{FamilyInfo, FontGroup};

/// The family used when a requested family is unknown (the UI sans).
pub const FALLBACK_FAMILY: &str = "Source Sans 3";

/// The bundled fonts (OFL), as font file bytes. Japanese fonts are not bundled: they come from
/// the optional craft-fonts build input ([`crate::japanese_document_fonts`]).
pub fn bundled() -> &'static [&'static [u8]] {
    BUNDLED
}

static BUNDLED: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/SourceSans3-Regular.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-Semibold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-Bold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-It.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-It.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-Semibold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-Bold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-BoldIt.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
];

/// The folder beside a document whose fonts the document brings with it (as File › Package
/// writes it).
pub const DOCUMENT_FONTS_FOLDER: &str = "Document Fonts";
/// Caps on what a `Document Fonts` folder (untrusted files) can make [`FontDb::load_document_fonts`]
/// read: font files read, bytes per file and bytes in all.
pub const MAX_DOCUMENT_FONT_FILES: usize = 200;
pub const MAX_DOCUMENT_FONT_BYTES: u64 = 256 << 20;
pub const MAX_DOCUMENT_FONTS_TOTAL: u64 = 512 << 20;

/// What [`FontDb::load_document_fonts`] did: the scope the document looks its fonts up in (0 when
/// the folder gave none), the faces in it, and a note for each file skipped.
#[derive(Debug, Default)]
pub struct DocumentFonts {
    pub scope: u32,
    pub faces: usize,
    pub skipped: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
struct CatalogEntry {
    family: String,
    style: String,
    path: std::path::PathBuf,
    /// The font menu group and native family name, read by the scan.
    group: FontGroup,
    native: Option<String>,
}

/// What the scan reads of one face in a font file: its names and font menu group.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq)]
struct ScannedFace {
    family: String,
    style: String,
    group: FontGroup,
    native: Option<String>,
}

/// Process-wide font database: the fonts every document shares (bundled, installed, added), and
/// each open document's own fonts under its scope (see [`FontDb::scoped`]).
pub struct FontDb {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    first_lookup_barrier: Option<Arc<std::sync::Barrier>>,
    faces: RwLock<Vec<Arc<FontFace>>>,
    /// Each open document's fonts (its `Document Fonts` folder), by scope.
    scopes: RwLock<HashMap<u32, Arc<[Arc<FontFace>]>>>,
    /// The document font files read, by path: (length, modification time, faces). A document
    /// opened again finds its unchanged files here instead of reading them again.
    #[cfg(not(target_arch = "wasm32"))]
    document_files: Mutex<HashMap<std::path::PathBuf, (u64, Option<std::time::SystemTime>, Vec<Arc<FontFace>>)>>,
    /// Variable font instances (`Style {wght:650}`), made on first use, by base face and style.
    instances: RwLock<HashMap<(u32, String), Arc<FontFace>>>,
    outlines: Mutex<HashMap<(u32, u32), Arc<BezPath>>>,
    #[cfg(not(target_arch = "wasm32"))]
    catalog: RwLock<Vec<CatalogEntry>>,
    /// The folders the system font scan reads (the platform's font folders for [`FontDb::global`]).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    font_dirs: Vec<std::path::PathBuf>,
    /// Set once the font folders have been scanned. Lookups by family name wait for the first
    /// scan, so what they find doesn't depend on what ran before them.
    #[cfg(not(target_arch = "wasm32"))]
    cataloged: std::sync::OnceLock<()>,
    /// System fallback state: enabled, and characters no system font covers.
    #[cfg(not(target_arch = "wasm32"))]
    sys: Mutex<SysFallback>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct SysFallback {
    enabled: bool,
    misses: std::collections::HashSet<char>,
    /// CJK chain families already looked up in the catalog (loaded, or not installed).
    chain_tried: std::collections::HashSet<&'static str>,
}

/// A face's family and style names (the default instance's style for a variable font).
/// Is a face of `family` and `style` among `faces`?
fn has_face(faces: &[Arc<FontFace>], family: &str, style: &str) -> bool {
    faces.iter().any(|f| f.family.eq_ignore_ascii_case(family) && f.style.eq_ignore_ascii_case(style))
}

/// The compiled-in Source Sans 3 Regular, for a database that somehow has no faces at all.
pub(crate) fn last_resort_face() -> Arc<FontFace> {
    static FACE: std::sync::OnceLock<Arc<FontFace>> = std::sync::OnceLock::new();
    FACE.get_or_init(|| {
        // The font is compiled in (`include_bytes!`), so parsing it can't depend on input; the
        // `last_resort_face_parses` test proves it on every run.
        #[allow(clippy::expect_used)]
        let face = make_face(FontBytes::Static(BUNDLED[0]), FontSource::Bundled, 0, FALLBACK_FAMILY.into(), "Regular".into(), Vec::new())
            .expect("the compiled-in Source Sans 3 Regular parses");
        Arc::new(face)
    })
    .clone()
}

/// Scopes are never reused: a closed document's scope finds no fonts ever after.
#[cfg(not(target_arch = "wasm32"))]
static NEXT_SCOPE: AtomicU32 = AtomicU32::new(1);
const OUTLINE_CACHE_MAX: usize = 50_000;

impl FontDb {
    /// A database holding the bundled fonts, then the craft-fonts Japanese faces (Mincho first,
    /// so they are the fallback for Japanese text after the requested and bundled fonts; empty
    /// without `CRAFT_FONTS_DIR`), whose system font scan reads `font_dirs`.
    pub fn with_font_dirs(font_dirs: Vec<std::path::PathBuf>) -> Self {
        let mut faces = Vec::new();
        let craft = crate::japanese_document_fonts().into_iter().map(|f| f.bytes);
        for data in BUNDLED.iter().copied().chain(craft) {
            for (i, family, style, coords) in enumerate_faces(data) {
                if let Some(f) = make_face(FontBytes::Static(data), FontSource::Bundled, i, family, style, coords) {
                    faces.push(Arc::new(f));
                }
            }
        }
        Self {
            #[cfg(all(test, not(target_arch = "wasm32")))]
            first_lookup_barrier: None,
            faces: RwLock::new(faces),
            scopes: RwLock::new(HashMap::new()),
            #[cfg(not(target_arch = "wasm32"))]
            document_files: Mutex::new(HashMap::new()),
            instances: RwLock::new(HashMap::new()),
            outlines: Mutex::new(HashMap::new()),
            #[cfg(not(target_arch = "wasm32"))]
            catalog: RwLock::new(Vec::new()),
            font_dirs,
            #[cfg(not(target_arch = "wasm32"))]
            cataloged: std::sync::OnceLock::new(),
            #[cfg(not(target_arch = "wasm32"))]
            sys: Mutex::new(SysFallback { enabled: true, ..Default::default() }),
        }
    }

    /// Enable or disable the lazy system-font fallback for characters the loaded fonts lack
    /// (native only; on by default).
    pub fn set_system_fallback(&self, on: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sys.lock().unwrap_or_else(|e| e.into_inner()).enabled = on;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = on;
    }

    /// Process-wide database: the bundled fonts, then the installed system fonts, cataloged the
    /// first time a lookup by family name needs them (or ahead of time by
    /// [`FontDb::scan_in_background`]).
    pub fn global() -> &'static FontDb {
        static DB: std::sync::OnceLock<FontDb> = std::sync::OnceLock::new();
        DB.get_or_init(|| FontDb::with_font_dirs(system_font_dirs()))
    }

    fn read_faces(&self) -> std::sync::RwLockReadGuard<'_, Vec<Arc<FontFace>>> {
        self.faces.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The system font catalog, scanned first if it hasn't been yet.
    #[cfg(not(target_arch = "wasm32"))]
    fn read_catalog(&self) -> std::sync::RwLockReadGuard<'_, Vec<CatalogEntry>> {
        self.ensure_catalog();
        self.catalog.read().unwrap_or_else(|e| e.into_inner())
    }

    /// What the document with font scope `scope` sees: its own fonts ahead of the shared ones.
    /// Scope 0 (a document without fonts of its own) or a closed scope sees the shared fonts.
    pub fn scoped(&self, scope: u32) -> ScopedFonts<'_> {
        let own = if scope == 0 { None } else { self.scopes.read().unwrap_or_else(|e| e.into_inner()).get(&scope).cloned() };
        ScopedFonts { db: self, own }
    }

    /// The shared fonts' family names (see [`ScopedFonts::families`]).
    pub fn families(&self) -> Vec<String> {
        self.scoped(0).families()
    }

    /// The shared fonts' styles of `family` (see [`ScopedFonts::styles`]).
    pub fn styles(&self, family: &str) -> Vec<String> {
        self.scoped(0).styles(family)
    }

    /// Add a user font (TTF/OTF/TTC bytes). Returns the number of faces added (0 if unparseable or
    /// every face was already present).
    pub fn add_font(&self, bytes: Vec<u8>) -> usize {
        self.add_faces(bytes, FontSource::Memory)
    }

    /// Add the faces of a font file from `source` to the shared fonts, skipping the family and
    /// style already loaded.
    fn add_faces(&self, bytes: Vec<u8>, source: FontSource) -> usize {
        let faces = self.new_faces(bytes, source);
        self.insert_faces(faces)
    }

    /// The faces of a font file from `source` whose family and style aren't loaded yet.
    fn new_faces(&self, bytes: Vec<u8>, source: FontSource) -> Vec<FontFace> {
        let data = Arc::new(bytes);
        enumerate_faces(&data)
            .into_iter()
            .filter(|(_, family, style, _)| !has_face(&self.read_faces(), family, style))
            .filter_map(|(i, family, style, coords)| make_face(FontBytes::Owned(data.clone()), source.clone(), i, family, style, coords))
            .collect()
    }

    /// Add `faces` to the shared fonts all at once, so a lookup sees all of them or none (not a
    /// family's bold without its regular), skipping a family and style another thread has added
    /// meanwhile. Returns the number added.
    fn insert_faces(&self, new: Vec<FontFace>) -> usize {
        let mut faces = self.faces.write().unwrap_or_else(|e| e.into_inner());
        let mut added = 0;
        for f in new {
            if !has_face(&faces, &f.family, &f.style) {
                faces.push(Arc::new(f));
                added += 1;
            }
        }
        added
    }

    /// Catalog the installed fonts unless that has been done: the first caller scans the font
    /// folders, any other waits for that scan to finish.
    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_catalog(&self) {
        self.cataloged.get_or_init(|| {
            self.scan_font_dirs();
        });
    }

    /// Catalog the installed fonts on a background thread, so the first lookup by family name
    /// (opening a file, the font menus) doesn't wait for the scan. A no-op once they are
    /// cataloged, and on wasm.
    pub fn scan_in_background(&'static self) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.cataloged.get().is_none() {
            // A failed spawn leaves the scan to the first lookup that needs it.
            let _ = std::thread::Builder::new().name("font-scan".into()).spawn(move || self.ensure_catalog());
        }
    }

    /// Scan the font folders again (fonts installed or removed since), cataloging the faces
    /// found (native only). Font data is loaded when a cataloged family is first resolved.
    /// Returns the number of faces cataloged.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_system_fonts(&self) -> usize {
        // The first scan, or a rescan once it is done (never both at once).
        let mut first = None;
        self.cataloged.get_or_init(|| first = Some(self.scan_font_dirs()));
        first.unwrap_or_else(|| self.scan_font_dirs())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scan_font_dirs(&self) -> usize {
        let mut found = Vec::new();
        let mut stack = self.font_dirs.clone();
        // Each folder once, however links lead back to it.
        let mut visited = std::collections::HashSet::new();
        while let Some(d) = stack.pop() {
            if !visited.insert(std::fs::canonicalize(&d).unwrap_or_else(|_| d.clone())) {
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
                if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
                    continue;
                }
                for f in file_face_names(&p) {
                    found.push(CatalogEntry { family: f.family, style: f.style, path: p.clone(), group: f.group, native: f.native });
                }
            }
        }
        let n = found.len();
        log::debug!("cataloged {n} system font faces");
        *self.catalog.write().unwrap_or_else(|e| e.into_inner()) = found;
        n
    }

    /// Load the files of the installed `family`. Returns whether any face was added.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_cataloged(&self, family: &str) -> bool {
        let paths: Vec<std::path::PathBuf> = {
            let cat = self.read_catalog();
            let mut p: Vec<_> = cat.iter().filter(|c| c.family.eq_ignore_ascii_case(family)).map(|c| c.path.clone()).collect();
            p.sort();
            p.dedup();
            p
        };
        // The whole family arrives at once (see `insert_faces`).
        let mut faces = Vec::new();
        for p in paths {
            if let Ok(data) = std::fs::read(&p) {
                faces.extend(self.new_faces(data, FontSource::Installed(p)));
            }
        }
        self.insert_faces(faces) > 0
    }

    /// Load the font files (.ttf, .otf, .ttc, .otc; not in subfolders) of a document's `Document
    /// Fonts` folder under a new scope, for that document alone: looked up through
    /// [`FontDb::scoped`], they come before the shared faces of the same family and style, until
    /// [`FontDb::close_scope`]. A file loaded before and unchanged since (same length and
    /// modification time) isn't read again: its faces are shared. The files are untrusted: past
    /// [`MAX_DOCUMENT_FONT_FILES`] (in name order), files over [`MAX_DOCUMENT_FONT_BYTES`] or past
    /// [`MAX_DOCUMENT_FONTS_TOTAL`] in all, and files that aren't fonts are skipped with a note. A
    /// missing folder loads nothing (scope 0).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_document_fonts(&self, folder: &std::path::Path) -> DocumentFonts {
        let mut out = DocumentFonts::default();
        let Ok(entries) = std::fs::read_dir(folder) else { return out };
        let mut files: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
                matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) && p.is_file()
            })
            .collect();
        files.sort();
        let mut total = 0u64;
        let mut own: Vec<Arc<FontFace>> = Vec::new();
        for (n, path) in files.iter().enumerate() {
            if n == MAX_DOCUMENT_FONT_FILES {
                out.skipped.push(format!("{} more font files not loaded (at most {MAX_DOCUMENT_FONT_FILES} are)", files.len() - n));
                break;
            }
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            // The listed length skips oversized files unread.
            let meta = std::fs::metadata(path).ok();
            let listed = meta.as_ref().map_or(0, |m| m.len());
            let faces = if listed > MAX_DOCUMENT_FONT_BYTES {
                Err(too_large())
            } else if total.saturating_add(listed) > MAX_DOCUMENT_FONTS_TOTAL {
                Err(over_total())
            } else {
                let modified = meta.and_then(|m| m.modified().ok());
                self.document_file(path, listed, modified, MAX_DOCUMENT_FONTS_TOTAL.saturating_sub(total))
            };
            match faces {
                Err(r) => out.skipped.push(format!("{name}: {r}")),
                Ok((faces, len)) => {
                    total = total.saturating_add(len);
                    // A family and style the folder already gave stays with its first file.
                    for f in faces {
                        if !own.iter().any(|o| o.family.eq_ignore_ascii_case(&f.family) && o.style.eq_ignore_ascii_case(&f.style)) {
                            own.push(f);
                        }
                    }
                }
            }
        }
        for s in &out.skipped {
            log::warn!("{}: {s}", folder.display());
        }
        if !own.is_empty() {
            out.scope = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
            out.faces = own.len();
            self.scopes.write().unwrap_or_else(|e| e.into_inner()).insert(out.scope, own.into());
        }
        out
    }

    /// The faces of the document font file at `path` (`listed` bytes long, modified at
    /// `modified`): those read before if it hasn't changed since, else read now, within `room`
    /// bytes of the folder's total → (faces, bytes), or why the file is skipped.
    #[cfg(not(target_arch = "wasm32"))]
    fn document_file(
        &self,
        path: &std::path::Path,
        listed: u64,
        modified: Option<std::time::SystemTime>,
        room: u64,
    ) -> Result<(Vec<Arc<FontFace>>, u64), String> {
        use std::io::Read;
        if let Some((len, m, faces)) = self.document_files.lock().unwrap_or_else(|e| e.into_inner()).get(path)
            && (*len, *m) == (listed, modified)
        {
            return Ok((faces.clone(), listed));
        }
        // One byte past the limit catches a file that grew since it was listed.
        let mut data = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_DOCUMENT_FONT_BYTES + 1).read_to_end(&mut data))
            .map_err(|e| format!("can't be read ({e})"))?;
        let len = data.len() as u64;
        if len > MAX_DOCUMENT_FONT_BYTES {
            return Err(too_large());
        }
        if listed.max(len) > room {
            return Err(over_total());
        }
        let data = Arc::new(data);
        let faces: Vec<Arc<FontFace>> = enumerate_faces(&data)
            .into_iter()
            .filter_map(|(i, family, style, coords)| {
                make_face(FontBytes::Owned(data.clone()), FontSource::Document(path.to_path_buf()), i, family, style, coords)
            })
            .map(Arc::new)
            .collect();
        if faces.is_empty() {
            return Err("not a font DesignCraft can read".into());
        }
        self.document_files.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), (listed, modified, faces.clone()));
        Ok((faces, len))
    }

    /// Close a document's font scope: its fonts are no longer found (in it or anywhere). Their
    /// files stay read, for the document opening again.
    pub fn close_scope(&self, scope: u32) {
        self.scopes.write().unwrap_or_else(|e| e.into_inner()).remove(&scope);
    }

    /// The shared fonts' face for `family` + `style` (see [`ScopedFonts::face`]).
    pub fn face(&self, family: &str, style: &str) -> Arc<FontFace> {
        self.scoped(0).face(family, style)
    }

    /// Is `family` among the shared fonts (loaded, or installed on the system)?
    pub fn has_family(&self, family: &str) -> bool {
        self.scoped(0).has_family(family)
    }

    fn is_loaded(&self, family: &str) -> bool {
        self.read_faces().iter().any(|f| f.family.eq_ignore_ascii_case(family))
    }

    /// A shared variable font's axes (see [`ScopedFonts::axes`]).
    pub fn axes(&self, family: &str, style: &str) -> Vec<(String, String, f32, f32, f32)> {
        self.scoped(0).axes(family, style)
    }

    /// A shared face other than `exclude` that covers `c` (see [`ScopedFonts::fallback_for`]).
    pub fn fallback_for(&self, c: char, exclude: u32, language: Option<&str>) -> Option<Arc<FontFace>> {
        self.scoped(0).fallback_for(c, exclude, language)
    }

    /// Load a system font covering `c` (preferred fallback families first, then any cataloged
    /// file under 40 MB). Returns true if one was loaded. Misses are remembered.
    #[cfg(not(target_arch = "wasm32"))]
    fn system_fallback(&self, c: char) -> bool {
        if c.is_control() || c.is_whitespace() {
            return false;
        }
        {
            let sys = self.sys.lock().unwrap_or_else(|e| e.into_inner());
            if !sys.enabled || sys.misses.contains(&c) {
                return false;
            }
        }
        let covered = |db: &FontDb| db.read_faces().iter().any(|f| f.covers(c));
        let cataloged: Vec<String> = self.read_catalog().iter().map(|e| e.family.clone()).collect();
        for fam in SYSTEM_FALLBACKS {
            if !self.is_loaded(fam) && cataloged.iter().any(|f| f.eq_ignore_ascii_case(fam)) {
                self.load_cataloged(fam);
                if covered(self) {
                    return true;
                }
            }
        }
        let mut paths: Vec<std::path::PathBuf> = self.catalog.read().unwrap_or_else(|e| e.into_inner()).iter().map(|e| e.path.clone()).collect();
        paths.sort();
        paths.dedup();
        for p in paths {
            if std::fs::metadata(&p).map(|m| m.len() > 40 << 20).unwrap_or(true) {
                continue;
            }
            let Ok(data) = std::fs::read(&p) else { continue };
            let hit =
                enumerate_faces(&data).iter().any(|(i, _, _, _)| skrifa::FontRef::from_index(&data, *i).is_ok_and(|f| f.charmap().map(c).is_some()));
            if hit && self.add_faces(data, FontSource::Installed(p)) > 0 && covered(self) {
                return true;
            }
        }
        self.sys.lock().unwrap_or_else(|e| e.into_inner()).misses.insert(c);
        false
    }

    /// The box drawn for `face`'s missing glyph (.notdef) when the font's own has no outline, in
    /// font units, y-down: a hollow rectangle across the glyph's advance, cap height tall. `None`
    /// when the font draws its own.
    pub fn missing_box(&self, face: &FontFace) -> Option<BezPath> {
        if !font_outline(face, 0).elements().is_empty() {
            return None;
        }
        let adv = Some(face.advance(0)).filter(|a| a.is_finite() && *a > 0.0).unwrap_or(face.upem * 0.5);
        let top = Some(face.cap_height).filter(|h| h.is_finite() && *h > 0.0).unwrap_or(face.upem * 0.7);
        let outer = kurbo::Rect::new(adv * 0.1, -top, adv * 0.9, 0.0);
        let t = (face.upem * 0.05).min(outer.width() / 4.0).min(top / 4.0);
        let inner = outer.inset(-t);
        let mut p = BezPath::new();
        // The inner rectangle runs the other way round, so filling leaves it empty.
        p.move_to((outer.x0, outer.y0));
        p.line_to((outer.x1, outer.y0));
        p.line_to((outer.x1, outer.y1));
        p.line_to((outer.x0, outer.y1));
        p.close_path();
        p.move_to((inner.x0, inner.y0));
        p.line_to((inner.x0, inner.y1));
        p.line_to((inner.x1, inner.y1));
        p.line_to((inner.x1, inner.y0));
        p.close_path();
        Some(p)
    }

    /// Glyph outline in font units, y-down (flipped), cached per (face, glyph). A `.notdef`
    /// without an outline is drawn as [`FontDb::missing_box`], so a missing character never
    /// disappears.
    pub fn outline(&self, face: &FontFace, gid: u32) -> Arc<BezPath> {
        let key = (face.id, gid);
        if let Some(p) = self.outlines.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return p.clone();
        }
        let drawn = font_outline(face, gid);
        let p = Arc::new(if gid == 0 && drawn.elements().is_empty() { self.missing_box(face).unwrap_or(drawn) } else { drawn });
        let mut cache = self.outlines.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= OUTLINE_CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, p.clone());
        p
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn too_large() -> String {
    format!("larger than {} MB", MAX_DOCUMENT_FONT_BYTES >> 20)
}

#[cfg(not(target_arch = "wasm32"))]
fn over_total() -> String {
    format!("over the {} MB the folder's fonts may take in all", MAX_DOCUMENT_FONTS_TOTAL >> 20)
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "tests_sysfonts.rs"]
mod tests_sysfonts;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "tests_fallback.rs"]
mod tests_fallback;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "tests_docfonts.rs"]
mod tests_docfonts;

#[cfg(test)]
#[path = "tests_vertical.rs"]
mod tests_vertical;
