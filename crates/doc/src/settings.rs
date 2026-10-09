//! Persisted document setup, typography and grid preferences.

use crate::{NumberedList, StoryId};
use designcraft_geom::Unit;
use serde::{Deserialize, Serialize};

/// Edit › Transparency Blend Space: the colour space transparency is flattened in (CMYK for
/// print documents, RGB for screen ones, as InDesign defaults).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlendSpace {
    #[default]
    Cmyk,
    Rgb,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Intent {
    #[default]
    Print,
    Web,
    Mobile,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridRelative {
    #[default]
    TopOfPage,
    TopMargin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BaselineGrid {
    pub start: f64,
    pub increment: f64,
    pub relative_to: GridRelative,
    pub color: [u8; 3],
    /// Hidden below this zoom (1.0 = 100%).
    pub view_threshold: f64,
}

impl Default for BaselineGrid {
    fn default() -> Self {
        BaselineGrid { start: 36.0, increment: 12.0, relative_to: GridRelative::TopOfPage, color: [140, 205, 230], view_threshold: 0.75 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocumentGrid {
    pub horizontal: f64,
    pub vertical: f64,
    pub subdivisions: u32,
    pub color: [u8; 3],
    pub in_back: bool,
}

impl Default for DocumentGrid {
    fn default() -> Self {
        DocumentGrid { horizontal: 72.0, vertical: 72.0, subdivisions: 8, color: [200, 200, 200], in_back: true }
    }
}

/// Preferences › Advanced Type (stored with the document, like InDesign).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AdvancedType {
    pub superscript_size: f64,
    pub superscript_position: f64,
    pub subscript_size: f64,
    pub subscript_position: f64,
}

impl Default for AdvancedType {
    fn default() -> Self {
        AdvancedType { superscript_size: 58.3, superscript_position: 33.3, subscript_size: 58.3, subscript_position: 33.3 }
    }
}

/// File → Document Setup + the guide/grid/unit preferences stored with the document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocSettings {
    pub intent: Intent,
    pub page_width: f64,
    pub page_height: f64,
    pub facing_pages: bool,
    /// Binding: Right to Left — page 1 is a left page and spreads read from right to left.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub right_to_left_binding: bool,
    pub primary_text_frame: bool,
    /// Bleed: top, bottom, inside, outside.
    pub bleed: [f64; 4],
    pub slug: [f64; 4],
    pub horizontal_units: Unit,
    pub vertical_units: Unit,
    pub baseline_grid: BaselineGrid,
    pub grid: DocumentGrid,
    /// Pasteboard extent around spreads (horizontal, vertical).
    pub pasteboard: (f64, f64),
    pub margin_color: [u8; 3],
    pub column_color: [u8; 3],
    pub bleed_color: [u8; 3],
    pub slug_color: [u8; 3],
    pub keyboard_increment: f64,
    /// Named lists (Type › Bulleted and Numbered Lists › Define Lists).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lists: Vec<NumberedList>,
    /// The story in the primary text frames (Smart Text Reflow adds and removes pages for it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_story: Option<StoryId>,
    /// Chapter number (Numbering & Section Options › Document Chapter Numbering).
    pub chapter_number: u32,
    /// Object › Effects › Global Light: the angle (degrees) shadows that use it share.
    pub global_light: f64,
    /// Preferences › Advanced Type: superscript and subscript size and position (percent of the
    /// font size).
    pub advanced_type: AdvancedType,
    /// Edit › Transparency Blend Space.
    pub blend_space: BlendSpace,
    /// Preferences › Appearance of Black: `[Black]` at 100% overprints (Overprint Preview, output).
    pub overprint_black: bool,
    /// Type › Track Changes: edits are recorded as inserted / deleted text.
    pub track_changes: bool,
    /// Preferences › Composition › Draw Missing Glyphs from Fallback Fonts: characters the
    /// applied font lacks are drawn from other fonts. Off (InDesign's behaviour, and new
    /// documents'), they are drawn as the font's missing-glyph box and Preflight lists them.
    pub glyph_fallback: bool,
}

impl Default for DocSettings {
    fn default() -> Self {
        DocSettings {
            intent: Intent::Print,
            page_width: 612.0,
            page_height: 792.0,
            facing_pages: true,
            right_to_left_binding: false,
            primary_text_frame: false,
            bleed: [0.0; 4],
            slug: [0.0; 4],
            horizontal_units: Unit::Picas,
            vertical_units: Unit::Picas,
            baseline_grid: BaselineGrid::default(),
            grid: DocumentGrid::default(),
            pasteboard: (72.0, 72.0),
            margin_color: [255, 56, 255],
            column_color: [166, 40, 255],
            bleed_color: [255, 72, 103],
            slug_color: [100, 188, 221],
            keyboard_increment: 1.0,
            primary_story: None,
            lists: Vec::new(),
            chapter_number: 1,
            global_light: 120.0,
            advanced_type: AdvancedType::default(),
            blend_space: BlendSpace::Cmyk,
            overprint_black: true,
            track_changes: false,
            glyph_fallback: false,
        }
    }
}
