//! Resolve language-aware paragraph hyphenation with bounded dictionary caches.

use super::{Document, Glyph, HashMap, Mutex, ParaProps, Range, hyphen, shape};

type LimitsKey = (usize, usize, usize, bool);

/// Mark glyphs after which a hyphen may be inserted (dictionary/pattern points within the
/// paragraph's limits; words with discretionary hyphens break only there).
/// Whether a language (a name or locale code, see [`designcraft_doc::language_tag`]) uses the
/// English hyphenation and spelling dictionaries.
pub fn is_english(language: &str) -> bool {
    // InDesign's English names first: composition asks this for every run.
    language.starts_with("English") || designcraft_doc::language_tag(language).is_some_and(|t| designcraft_doc::language_subtag(t) == "en")
}

/// Byte ranges of paragraph `prange` set in a language other than English (no English
/// hyphenation there).
pub(super) fn foreign_ranges(
    doc: &Document,
    story: &designcraft_doc::Story,
    prange: Range<usize>,
    base: &designcraft_doc::CharProps,
) -> Vec<Range<usize>> {
    if is_english(&base.language)
        && story.runs().all(|(_, f)| f.over.language.as_deref().is_none_or(is_english) && f.style == designcraft_doc::story::NO_CHAR_STYLE)
    {
        return vec![];
    }
    story
        .runs()
        .filter(|(r, _)| r.start < prange.end && r.end > prange.start)
        .filter(|(_, f)| !is_english(&doc.styles.resolve_char(base, f).language))
        .map(|(r, _)| r)
        .collect()
}

pub(super) fn hyphenation_points(
    text: &str,
    glyphs: &[Glyph],
    pp: &ParaProps,
    exceptions: &HashMap<String, Vec<usize>>,
    foreign: &[Range<usize>],
) -> Vec<bool> {
    let mut out = vec![false; glyphs.len()];
    if !pp.hyphenate {
        return out;
    }
    let lim = hyphen::Limits {
        min_word: pp.hyph_min_word as usize,
        after_first: pp.hyph_after_first as usize,
        before_last: pp.hyph_before_last as usize,
        capitalized: pp.hyph_capitalized,
    };
    // Words repeat a lot: remember their points per set of limits.
    static CACHE: Mutex<Vec<(LimitsKey, HashMap<Box<str>, Box<[usize]>>)>> = Mutex::new(Vec::new());
    let key = (lim.min_word, lim.after_first, lim.before_last, lim.capitalized);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let idx = match guard.iter().position(|e| e.0 == key) {
        Some(i) => i,
        None => {
            if guard.len() > 32 {
                guard.clear();
            }
            guard.push((key, HashMap::new()));
            guard.len() - 1
        }
    };
    let cache = &mut guard[idx].1;
    if cache.len() > 100_000 {
        cache.clear();
    }
    let mut i = 0;
    while i < glyphs.len() {
        if !glyphs[i].is_letter() || glyphs[i].len == 0 {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < glyphs.len() && (glyphs[j].is_letter() || glyphs[j].len == 0 || glyphs[j].ch == shape::SOFT_HYPHEN) && !glyphs[j].no_break {
            j += 1;
        }
        // A discretionary hyphen in a word (or right before it) replaces automatic hyphenation.
        let discretionary = glyphs[i..j].iter().any(|g| g.ch == shape::SOFT_HYPHEN) || (i > 0 && glyphs[i - 1].ch == shape::SOFT_HYPHEN);
        let (a, b) = (glyphs[i].byte, glyphs[j - 1].byte + glyphs[j - 1].len);
        if !discretionary && b > a && b <= text.len() && text.is_char_boundary(a) && text.is_char_boundary(b) {
            let word = &text[a..b];
            // Do not hyphenate the paragraph's last word unless allowed.
            let is_last_word = !pp.hyph_last_word && glyphs[j..].iter().all(|g| !g.is_letter());
            let in_foreign = foreign.iter().any(|r| r.contains(&a));
            if !is_last_word && !in_foreign {
                let user = (!exceptions.is_empty()).then(|| exceptions.get(&word.to_lowercase())).flatten();
                let pts: &[usize] = match user {
                    // User dictionary exceptions win over the patterns and the built-in list.
                    Some(v) => v,
                    None => cache.entry(word.into()).or_insert_with(|| hyphen::hyphen_points(word, &lim).into_boxed_slice()),
                };
                for &p in pts.iter() {
                    // char index p → byte → glyph whose cluster ends at that byte.
                    let byte = a + word.char_indices().nth(p).map(|(bi, _)| bi).unwrap_or(word.len());
                    if let Some(k) = (i..j).find(|&k| glyphs[k].byte + glyphs[k].len == byte && glyphs[k].len > 0) {
                        out[k] = true;
                    }
                }
            }
        }
        i = j.max(i + 1);
    }
    out
}
