//! Bounded word-shaping cache and uncached shaping fallback.

use super::{Arc, Feature, FontFace, HashMap, RwLock, ShapedGlyph};

type WordMap = HashMap<Box<str>, Arc<[ShapedGlyph]>>;
type WordKey = (u32, bool, bool, String, Option<&'static str>);

/// Shaped words (font units) by (face, vertical, caps, features, language), then text. Lookups
/// take read locks and misses are shaped outside any lock, so parallel composition doesn't
/// serialize here.
static WORD_CACHE: RwLock<Vec<(WordKey, Arc<RwLock<WordMap>>)>> = RwLock::new(Vec::new());
const WORD_CACHE_MAX: usize = 50_000;

fn word_map(key: WordKey) -> Arc<RwLock<WordMap>> {
    if let Some((_, m)) = WORD_CACHE.read().unwrap_or_else(|e| e.into_inner()).iter().find(|e| e.0 == key) {
        return m.clone();
    }
    let mut w = WORD_CACHE.write().unwrap_or_else(|e| e.into_inner());
    if let Some((_, m)) = w.iter().find(|e| e.0 == key) {
        return m.clone();
    }
    if w.len() > 64 {
        w.clear();
    }
    let m = Arc::new(RwLock::new(HashMap::new()));
    w.push((key, m.clone()));
    m
}

/// Shape `src` word by word through a cache (like a browser's word cache): the text is split
/// after each U+0020 so repeated words are shaped once. Shaping does not cross word spaces in the
/// scripts we lay out, and clusters stay byte offsets into `src`.
pub(super) fn shape_cached(
    face: &FontFace,
    src: &str,
    feats: &[Feature],
    lang: Option<&'static str>,
    caps: bool,
    vertical: bool,
    (before, after): (&str, &str),
) -> Vec<ShapedGlyph> {
    use designcraft_fonts::ShapeContext;
    let map = |c: char| if caps { c.to_uppercase().next().unwrap_or(c) } else { c };
    let shape = |s: &str, context: ShapeContext<'_>| {
        if vertical {
            designcraft_fonts::shape_vertical(face, s, feats, map, context)
        } else {
            designcraft_fonts::shape_with_context(face, s, feats, map, context)
        }
    };
    if src.len() < 2 || !src.contains(' ') || src.chars().any(designcraft_fonts::is_rtl) {
        return shape(src, ShapeContext { before, after, language: lang });
    }
    let words = word_map((face.id(), vertical, caps, format!("{feats:?}"), lang));
    let pieces: Vec<&str> = src.split_inclusive(' ').collect();
    let mut found: Vec<Option<Arc<[ShapedGlyph]>>> = {
        let r = words.read().unwrap_or_else(|e| e.into_inner());
        pieces.iter().map(|p| r.get(*p).cloned()).collect()
    };
    let mut fresh: Vec<(Box<str>, Arc<[ShapedGlyph]>)> = Vec::new();
    for (k, f) in found.iter_mut().enumerate() {
        if f.is_none() {
            let g: Arc<[ShapedGlyph]> = shape(pieces[k], ShapeContext { language: lang, ..Default::default() }).into();
            fresh.push((pieces[k].into(), g.clone()));
            *f = Some(g);
        }
    }
    if !fresh.is_empty() {
        let mut w = words.write().unwrap_or_else(|e| e.into_inner());
        if w.len() + fresh.len() > WORD_CACHE_MAX {
            w.clear();
        }
        w.extend(fresh);
    }
    let mut out = Vec::with_capacity(src.len());
    let mut start = 0;
    for (piece, glyphs) in pieces.iter().zip(found) {
        let glyphs = glyphs.unwrap_or_else(|| Arc::from(Vec::new()));
        out.extend(glyphs.iter().map(|g| ShapedGlyph { cluster: g.cluster + start, ..*g }));
        start += piece.len();
    }
    out
}
