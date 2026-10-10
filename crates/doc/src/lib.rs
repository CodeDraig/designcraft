//! The DesignCraft document model (pure data + serde).
//!
//! A [`Document`] holds spreads of pages, parent ("master") spreads, layers, stories, styles,
//! swatches, sections and embedded assets. Spreads, items and stories are `Arc`-shared so an
//! edit clones only what it touches and undo snapshots are O(1).
//!
//! Coordinates are points, y down. Each spread has its own space: pages sit side by side from
//! x = 0 with their tops at y = 0. Items live in spread space via `Item::xf`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod settings;
pub use settings::{AdvancedType, BaselineGrid, BlendSpace, DocSettings, DocumentGrid, GridRelative, Intent};
mod inks;
pub use inks::InkManager;
mod xml;
pub use xml::{XmlSettings, XmlTag};
mod navigation;
pub use navigation::{Bookmark, Hyperlink, HyperlinkDest, HyperlinkSource, Toc, TocEntry};
mod layers;
use layers::yes;
pub use layers::{LAYER_COLORS, Layer};
mod assets;
pub use assets::Asset;
pub mod tei;

pub mod anchored;
pub mod arabic;
pub mod arrow;
pub mod attrs;
pub mod build;
pub mod cjk;
pub mod datamerge;
mod edit;
pub mod endnotes;
pub mod ids;
pub mod index;
pub mod item;
pub mod notes;
pub mod otf;
pub mod page;
pub mod publication;
pub mod selection;
mod slice;
pub mod story;
pub mod styles;
pub mod table;
mod threading;
pub mod vars;
pub mod xref;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use anchored::{AnchorPosition, AnchoredObject, OBJECT_MARK};
pub use attrs::*;
pub use datamerge::{
    DataField, DataFieldKind, DataMerge, DataSource, Delimiter, Fingerprint, MergeOptions, Placeholder, PlaceholderAnchor, PlaceholderRole,
    SourceStatus,
};
pub use designcraft_color as color;
pub use designcraft_geom as geom;
pub use edit::{ItemLoc, ItemPath, SpreadRef, item_hit as edit_hit};
pub use endnotes::{ENDNOTE_REF, EditorialNote, EndnoteOptions, NOTE_MARK};
pub use ids::*;
pub use index::{INDEX_MARK, IndexRef};
pub use item::*;
pub use notes::{FOOTNOTE_REF, FOOTNOTE_TABLE, Footnote, FootnoteOptions};
pub use page::*;
pub use selection::*;
pub use story::*;
pub use styles::*;
pub use table::*;
pub use xref::{ANCHOR_MARK, CrossRef, TextAnchor, XREF_MARK, XrefFormat};

use designcraft_color::Swatch;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DocError {
    #[error("no such item {0}")]
    NoItem(ItemId),
    #[error("no such story {0}")]
    NoStory(StoryId),
    #[error("no such page {0}")]
    NoPage(usize),
    #[error("no such layer {0}")]
    NoLayer(LayerId),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, DocError>;

/// Largest `next_id` a valid document has (ids count up from it; 2^53 also stays exact in JSON
/// readers that use doubles).
pub const MAX_NEXT_ID: u64 = 1 << 53;

/// A named numbered list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NumberedList {
    pub name: String,
    /// Numbering continues from story to story (in page order).
    pub continue_across_stories: bool,
}

impl Default for NumberedList {
    fn default() -> Self {
        NumberedList { name: String::new(), continue_across_stories: true }
    }
}

/// A conditional-text condition: its indicator colour (screen only) and whether text with it shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub name: String,
    pub color: [u8; 3],
    pub visible: bool,
}

impl Document {
    /// A stroke type with named styles resolved (unknown names draw solid).
    pub fn stroke_kind(&self, t: &StrokeType) -> StrokeType {
        match t {
            StrokeType::Style { name } => self
                .stroke_styles
                .iter()
                .find(|s| s.name == *name)
                .map(|s| s.kind.clone())
                .filter(|k| !matches!(k, StrokeType::Style { .. }))
                .unwrap_or(StrokeType::Solid),
            t => t.clone(),
        }
    }

    /// Hyphenation exceptions by lowercase word: the break positions (char indices; empty = never
    /// hyphenate).
    pub fn hyphenation_exception_map(&self) -> std::collections::HashMap<String, Vec<usize>> {
        self.hyphenation_exceptions
            .iter()
            .map(|e| {
                let mut word = String::new();
                let mut breaks = Vec::new();
                for c in e.trim().chars() {
                    if c == '~' {
                        if breaks.last() != Some(&word.chars().count()) && !word.is_empty() {
                            breaks.push(word.chars().count());
                        }
                    } else {
                        word.extend(c.to_lowercase());
                    }
                }
                breaks.retain(|b| *b < word.chars().count());
                (word, breaks)
            })
            .collect()
    }

    /// Is text with these conditions hidden? (Conditioned text shows while any of its conditions
    /// is visible.)
    pub fn conditions_hide(&self, conds: &[String]) -> bool {
        !conds.is_empty() && conds.iter().all(|c| self.conditions.iter().any(|x| x.name == *c && !x.visible))
    }
}

/// An article: objects in reading order; `export` includes it when exporting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Article {
    pub name: String,
    pub items: Vec<ItemId>,
    #[serde(default = "yes")]
    pub export: bool,
}

/// A Swatches panel colour group: a named folder of swatches (by name).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorGroup {
    pub name: String,
    pub swatches: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub title: String,
    pub settings: DocSettings,
    pub spreads: Vec<Arc<Spread>>,
    pub parents: Vec<Arc<Spread>>,
    /// Top of the list = frontmost layer (Layers panel order).
    pub layers: Vec<Layer>,
    pub stories: BTreeMap<StoryId, Arc<Story>>,
    pub styles: Arc<Styles>,
    pub swatches: Vec<Swatch>,
    /// Swatches panel colour groups (folders), in panel order; swatches not in a group sit at
    /// the top level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color_groups: Vec<ColorGroup>,
    /// XML: the tags (Tags panel) and the paragraph style → tag map (Map Styles to Tags).
    #[serde(default, skip_serializing_if = "XmlSettings::is_empty")]
    pub xml: XmlSettings,
    /// Articles panel: named reading-order lists of objects (EPUB / HTML export order).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub articles: Vec<Article>,
    /// Custom stroke styles (Object › Stroke Styles).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stroke_styles: Vec<StrokeStyleDef>,
    /// Ink Manager: spot inks printed as process, and spot inks aliased to others.
    #[serde(default, skip_serializing_if = "InkManager::is_default")]
    pub inks: InkManager,
    /// Document Endnote Options, and the story shown in the endnote frame (generated).
    #[serde(default)]
    pub endnote_options: EndnoteOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endnote_story: Option<StoryId>,
    /// Conditional text conditions; text whose conditions are all hidden isn't composed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    pub sections: Vec<Section>,
    #[serde(default)]
    pub assets: BTreeMap<AssetId, Arc<Asset>>,
    /// Unchanged TEI originals and import receipts, retained through editing and undo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tei_sources: Vec<Arc<tei::TeiSource>>,
    /// Hyperlinks (Window → Interactive → Hyperlinks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hyperlinks: Vec<Hyperlink>,
    /// Data merge: the linked table, placeholders, and the last merge options.
    #[serde(default, skip_serializing_if = "DataMerge::is_empty")]
    pub data_merge: DataMerge,
    /// PDF bookmarks (Window → Interactive → Bookmarks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<Bookmark>,
    /// Words added to the document's user dictionary (spelling).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_words: Vec<String>,
    /// User dictionary hyphenation exceptions: `ex~am~ple` (breaks only at `~`), or a word with
    /// no `~` that never hyphenates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hyphenation_exceptions: Vec<String>,
    /// The generated table of contents (Layout → Table of Contents), kept for Update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc: Option<Toc>,
    /// Text variable definitions (instances in stories are `vars::var_char(index)`).
    #[serde(default = "vars::defaults")]
    pub text_variables: Vec<vars::TextVariable>,
    /// Type › Document Footnote Options.
    #[serde(default)]
    pub footnote_options: FootnoteOptions,
    /// Cross-reference formats (Type › Hyperlinks & Cross-References › Cross-Reference Formats).
    #[serde(default = "xref::default_formats")]
    pub xref_formats: Vec<XrefFormat>,
    /// The generated index (kept for Update Index).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<index::IndexSpec>,
    /// Creation / last save time (Unix seconds, UTC).
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub modified: i64,
    pub next_id: u64,
    /// While the document is open, the font scope of the fonts it brought (its `Document Fonts`
    /// folder): composition, export and the font menus look its fonts up there. 0: none. Not
    /// saved.
    #[serde(skip)]
    pub font_scope: u32,
    /// Active component/edition projection. The native archive writes its canonical publication
    /// separately; snapshots retain the whole root, including all inactive components.
    #[serde(skip)]
    pub publication: Option<publication::PublicationView>,
}

impl Document {
    /// Allocate a fresh id.
    pub fn alloc(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn page_count(&self) -> usize {
        self.spreads.iter().map(|s| s.pages.len()).sum()
    }

    /// (spread index, page index within spread) for an absolute page index.
    pub fn page_loc(&self, abs: usize) -> Option<(usize, usize)> {
        let mut n = 0;
        for (si, s) in self.spreads.iter().enumerate() {
            if abs < n + s.pages.len() {
                return Some((si, abs - n));
            }
            n += s.pages.len();
        }
        None
    }

    /// Absolute index of the first page of spread `si`.
    pub fn first_page_of_spread(&self, si: usize) -> usize {
        self.spreads[..si.min(self.spreads.len())].iter().map(|s| s.pages.len()).sum()
    }

    pub fn page(&self, abs: usize) -> Option<&Page> {
        let (s, p) = self.page_loc(abs)?;
        self.spreads[s].pages.get(p)
    }

    /// Iterate all pages with their absolute index and spread index.
    pub fn pages(&self) -> impl Iterator<Item = (usize, usize, &Page)> {
        self.spreads.iter().enumerate().flat_map(|(si, s)| s.pages.iter().map(move |p| (si, p))).enumerate().map(|(i, (si, p))| (i, si, p))
    }

    /// The section containing absolute page `abs`.
    pub fn section_of(&self, abs: usize) -> Option<&Section> {
        self.sections.iter().filter(|s| s.start <= abs).max_by_key(|s| s.start)
    }

    /// The page number (before formatting) of absolute page `abs`, per sections. A section without
    /// a start number continues from the previous section.
    pub fn page_number(&self, abs: usize) -> u32 {
        let Some(sec) = self.section_of(abs) else { return abs as u32 + 1 };
        let start = match sec.start_number {
            Some(n) => n,
            None if sec.start == 0 => 1,
            None => self.page_number(sec.start - 1) + 1,
        };
        start + (abs - sec.start) as u32
    }

    /// The displayed page name ("1", "iv", "A-3"…) for an absolute page index, per sections.
    pub fn page_name(&self, abs: usize) -> String {
        let Some(sec) = self.section_of(abs) else { return (abs + 1).to_string() };
        let num = sec.style.format(self.page_number(abs));
        if sec.include_prefix { format!("{}{}", sec.prefix, num) } else { num }
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }
    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }
    /// Stacking rank of a layer: 0 = backmost.
    pub fn layer_rank(&self, id: LayerId) -> usize {
        self.layers.iter().position(|l| l.id == id).map(|i| self.layers.len() - 1 - i).unwrap_or(0)
    }
    pub fn default_layer(&self) -> LayerId {
        self.layers.first().map(|l| l.id).unwrap_or_default()
    }

    pub fn story(&self, id: StoryId) -> Option<&Story> {
        self.stories.get(&id).map(|s| &**s)
    }
    pub fn story_mut(&mut self, id: StoryId) -> Option<&mut Story> {
        self.stories.get_mut(&id).map(Arc::make_mut)
    }

    pub fn swatch(&self, name: &str) -> Option<&Swatch> {
        self.swatches.iter().find(|s| s.name == name)
    }

    /// Resolve a swatch reference to a display colour.
    /// A shadow's light angle: its own, or the document's Global Light.
    pub fn light_angle(&self, angle: f64, global: bool) -> f64 {
        if global { self.settings.global_light } else { angle }
    }

    pub fn resolve_color(&self, swatch: &str, tint: f32) -> Option<designcraft_color::Color> {
        designcraft_color::swatch::resolve(&self.swatches, swatch, tint)
    }

    /// Validate structural invariants (ids unique, threads consistent, stories well-formed).
    pub fn check(&self) -> Result<()> {
        for f in &self.styles.composite_fonts {
            if f.entries.iter().any(|e| {
                ![e.relative_size, e.horizontal_scale, e.vertical_scale].iter().all(|v| v.is_finite() && *v > 0.0) || !e.baseline_shift.is_finite()
            }) {
                return Err(DocError::Invalid(format!("invalid composite font metrics: {}", f.name)));
            }
        }
        for t in &self.styles.mojikumi_tables {
            if t.overrides.iter().any(|r| ![r.minimum, r.desired, r.maximum].iter().all(|v| v.is_finite())) {
                return Err(DocError::Invalid(format!("non-finite mojikumi spacing: {}", t.name)));
            }
        }
        let mut ids = std::collections::HashSet::new();
        for sp in self.spreads.iter().chain(self.parents.iter()) {
            for it in &sp.items {
                let mut dup = None;
                it.walk(&mut |i| {
                    if !ids.insert(i.id.0) {
                        dup = Some(i.id);
                    }
                });
                if let Some(d) = dup {
                    return Err(DocError::Invalid(format!("duplicate item id {d}")));
                }
            }
        }
        for (sid, st) in &self.stories {
            st.check().map_err(DocError::Invalid)?;
            if st.id != *sid {
                return Err(DocError::Invalid(format!("story key {sid} != id {}", st.id)));
            }
            for f in &st.frames {
                let item = self.find(*f).ok_or(DocError::NoItem(*f))?;
                match self.item_at(&item).and_then(|i| i.text_frame().map(|t| t.story)) {
                    Some(s) if s == *sid => {}
                    other => return Err(DocError::Invalid(format!("frame {f} in story {sid} points at {other:?}"))),
                }
            }
        }
        // Every text frame is in its story's thread.
        for sp in self.spreads.iter().chain(self.parents.iter()) {
            for it in &sp.items {
                let mut bad = None;
                it.walk(&mut |i| {
                    if let Some(t) = i.text_frame()
                        && !self.stories.get(&t.story).is_some_and(|s| s.frames.contains(&i.id))
                    {
                        bad = Some(i.id);
                    }
                });
                if let Some(b) = bad {
                    return Err(DocError::Invalid(format!("text frame {b} not in its story's thread")));
                }
            }
        }
        if self.next_id < ids.iter().copied().max().unwrap_or(0) {
            return Err(DocError::Invalid("next_id behind existing ids".into()));
        }
        // Ids are allocated by counting up from `next_id`: leave room so that can't overflow.
        if self.next_id > MAX_NEXT_ID {
            return Err(DocError::Invalid(format!("next_id {} out of range", self.next_id)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

impl Document {
    /// Mutable styles (copy-on-write).
    pub fn styles_mut(&mut self) -> &mut Styles {
        Arc::make_mut(&mut self.styles)
    }
}
