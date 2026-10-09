//! Composed glyph, line and frame geometry consumed by rendering and editing.

use super::{Arc, ItemId, Point, Range, Rect, StoryId, TableFrag};

/// Character appearance shared by many glyphs (indexed from [`PlacedGlyph::style`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RunStyle {
    pub fill: String,
    pub fill_tint: f32,
    pub stroke: String,
    pub stroke_tint: f32,
    pub stroke_weight: f64,
    pub underline: bool,
    pub strikethrough: bool,
    /// The underline and strikethrough bars (Underline / Strikethrough Options resolved).
    pub underline_rule: Rule,
    pub strike_rule: Rule,
    pub skew: f64,
    pub size: f64,
    /// The run's font isn't installed (shown in a substitute; highlighted on screen).
    pub missing_font: bool,
    /// Tracking or manual kerning (Highlight Custom Tracking/Kerning).
    pub custom_tracking: bool,
    /// The first condition applied (its indicator colour underlines the text on screen).
    pub condition: Option<String>,
    /// Added while tracking changes (marked on screen).
    pub inserted: bool,
    /// XML element the text is tagged with (tag markers on screen).
    pub xml_tag: Option<String>,
    /// Ruby over the run, and kenten (emphasis dots).
    pub ruby: Option<String>,
    pub kenten: bool,
    pub kenten_character: String,
}

/// An underline or strikethrough bar: its top edge `offset` below the baseline (negative =
/// above), thickness and colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub offset: f64,
    pub weight: f64,
    pub color: String,
    pub tint: f32,
}

impl RunStyle {
    /// The bars this run draws between `x0` and `x1` on a line at `baseline`.
    pub fn rules(&self, x0: f64, x1: f64, baseline: f64) -> impl Iterator<Item = (&Rule, Rect)> {
        [(self.underline, &self.underline_rule), (self.strikethrough, &self.strike_rule)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(move |(_, r)| (r, Rect::new(x0, baseline + r.offset, x1, baseline + r.offset + r.weight)))
    }
}

/// A positioned glyph. `x` is absolute in frame inner space; `y` is relative to the line baseline.
#[derive(Clone, Debug)]
pub struct PlacedGlyph {
    pub face: designcraft_fonts::FaceRef,
    pub gid: u32,
    pub x: f64,
    pub y: f64,
    pub adv: f64,
    pub sx: f64,
    pub sy: f64,
    pub style: u32,
    pub byte: usize,
    pub len: usize,
    /// Control characters (tabs, breaks, markers' carriers) are not drawn.
    pub visible: bool,
    /// Stays upright in vertical frames (CJK ideographs, kana, hangul, full-width forms).
    pub upright: bool,
    /// Tate-chu-yoko placement (see [`crate::shape::Glyph::tcy`]).
    pub tcy: Option<[f64; 3]>,
    /// Set right to left (an odd bidi level): the caret before it is at its right edge.
    pub rtl: bool,
}

impl PlacedGlyph {
    /// In a vertical frame, the turn that sets this glyph upright, applied after drawing it at `x`
    /// on `baseline`: it hangs from its vertical origin ([`designcraft_fonts::FontFace::v_origin`],
    /// centred across) at `x` on the line's centre, the middle of the font's em box; a tate-chu-yoko
    /// group sits across that centre.
    pub fn vertical_xf(&self, baseline: f64) -> Option<designcraft_geom::Affine> {
        let turn = -std::f64::consts::FRAC_PI_2;
        // The em box centre above the baseline, in ems.
        let (top, bottom) = self.face.em_box();
        let centre = (top + bottom) / 2.0 / self.face.units_per_em();
        if let Some([along, across, em]) = self.tcy {
            let c = Point::new(self.x + along, baseline + self.y - em * centre);
            return Some(designcraft_geom::Affine::rotate_about(turn, c) * designcraft_geom::Affine::translate((c.x + across - self.x, 0.0)));
        }
        if !self.upright {
            return None;
        }
        // Turning about c takes the glyph's vertical origin (half across, `origin` up) to the
        // centre point at `x`: c is where the two points' perpendicular bisector meets the turn.
        let half = self.face.advance(self.gid) / 2.0 * self.sx;
        let origin = self.face.v_origin(self.gid) * self.sy;
        let mid = centre * self.face.units_per_em() * self.sy;
        let c = Point::new(self.x + (origin - mid + half) / 2.0, baseline + self.y + (half - mid - origin) / 2.0);
        Some(designcraft_geom::Affine::rotate_about(turn, c))
    }
}

/// Characters set upright (unrotated) in vertical text.
pub fn upright_in_vertical(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FFF | 0x3000..=0x30FF | 0xFE10..=0xFE4F | 0x3100..=0x31FF | 0x3200..=0x9FFF
        | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFF01..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFF)
}

#[derive(Clone, Debug)]
pub struct Line {
    pub column: u32,
    /// Baseline y in frame inner space.
    pub baseline: f64,
    /// Line slot (column minus wrap) horizontal extent.
    pub x0: f64,
    pub x1: f64,
    pub ascent: f64,
    pub descent: f64,
    pub leading: f64,
    /// Story byte range (excluding the paragraph separator).
    pub range: Range<usize>,
    pub para: usize,
    pub glyphs: Vec<PlacedGlyph>,
    pub hyphenated: bool,
    pub first_in_para: bool,
    pub last_in_para: bool,
    /// x after the last glyph (caret at line end).
    pub end_x: f64,
    /// Word-space ratio actually used vs desired (H&J violation highlighting), 1.0 = desired.
    pub spacing: f64,
    /// H&J violation: 0 = within the paragraph's minimum/maximum word spacing, 1–3 = how far out
    /// (Preferences › Composition › Highlight H&J Violations shades).
    pub hj: u8,
    /// The paragraph's Keep Options couldn't be honoured here (Highlight Keep Violations).
    pub keep_violation: bool,
}

/// A paragraph rule or shading rectangle in frame inner space.
#[derive(Clone, Debug, PartialEq)]
pub struct Deco {
    pub rect: Rect,
    pub color: String,
    pub tint: f32,
}

#[derive(Clone, Debug, Default)]
pub struct FrameText {
    pub frame: ItemId,
    /// Vertical Type frame (upright glyphs turn back in it).
    pub vertical: bool,
    pub lines: Vec<Line>,
    pub decos: Vec<Deco>,
    /// Byte range of the story shown in this frame.
    pub range: Range<usize>,
    /// Column rectangles (inner space) for drawing column guides / hit testing.
    pub columns: Vec<Rect>,
    /// Height the text needs (for auto-sizing), from the text area top.
    pub content_height: f64,
    /// Table fragments placed in this frame (each also has a glyph-less line in `lines`).
    pub tables: Vec<TableFrag>,
    /// Footnotes placed at the bottom of this frame's columns.
    pub notes: Vec<PlacedNote>,
    /// Anchored objects placed in this frame's lines.
    pub objects: Vec<PlacedObject>,
}

/// An anchored object placed in the text.
#[derive(Clone, Debug)]
pub struct PlacedObject {
    /// Index in the story's [`designcraft_doc::Story::objects`].
    pub index: usize,
    /// Top-left of the object in frame inner space.
    pub origin: Point,
    pub size: (f64, f64),
    /// The line it sits on (or above).
    pub line: usize,
    pub(super) x: f64,
    pub(super) text_ascent: f64,
}

/// A footnote composed at the bottom of a column.
#[derive(Clone, Debug)]
pub struct PlacedNote {
    /// Footnote id (in the story's [`designcraft_doc::Story::notes`]) and its index there.
    pub id: u64,
    pub index: usize,
    pub column: u32,
    /// The number shown.
    pub label: String,
    /// Where the footnote's composed text (its frame 0) sits in the frame.
    pub origin: Point,
    /// Area of the footnote in frame inner space.
    pub rect: Rect,
    pub text: Arc<ComposedStory>,
    /// The footnote's story text (exporters map glyphs back to text).
    pub source: Arc<str>,
}

#[derive(Clone, Debug, Default)]
pub struct ComposedStory {
    pub story: StoryId,
    pub rev: u64,
    pub frames: Vec<FrameText>,
    /// First byte that didn't fit (overset text), if any.
    pub overset_at: Option<usize>,
    pub styles: Vec<RunStyle>,
    pub text_len: usize,
}

impl ComposedStory {
    pub fn is_overset(&self) -> bool {
        self.overset_at.is_some()
    }
    pub fn frame(&self, id: ItemId) -> Option<&FrameText> {
        self.frames.iter().find(|f| f.frame == id)
    }
    /// Number of lines in all frames.
    pub fn line_count(&self) -> usize {
        self.frames.iter().map(|f| f.lines.len()).sum()
    }
}
