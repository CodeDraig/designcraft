//! Map document, grid and footnote preferences from IDML.

use super::{BaselineGrid, El, GridRelative, Importer, names};

impl Importer<'_> {
    pub(super) fn doc_prefs(&mut self, e: &El) {
        let s = &mut self.settings;
        if let Some(v) = e.get("OverprintBlack") {
            s.overprint_black = v != "false";
        }
        if let Some(v) = e.num("PageWidth") {
            s.page_width = v;
        }
        if let Some(v) = e.num("PageHeight") {
            s.page_height = v;
        }
        if let Some(v) = e.boolean("FacingPages") {
            s.facing_pages = v;
        }
        if let Some(v) = e.get("PageBinding") {
            s.right_to_left_binding = v == "RightToLeft";
        }
        for (i, k) in ["DocumentBleedTopOffset", "DocumentBleedBottomOffset", "DocumentBleedInsideOrLeftOffset", "DocumentBleedOutsideOrRightOffset"]
            .iter()
            .enumerate()
        {
            if let Some(v) = e.num(k) {
                s.bleed[i] = v;
            }
        }
        for (i, k) in ["SlugTopOffset", "SlugBottomOffset", "SlugInsideOrLeftOffset", "SlugRightOrOutsideOffset"].iter().enumerate() {
            if let Some(v) = e.num(k) {
                s.slug[i] = v;
            }
        }
        if let Some(i) = e.get("Intent") {
            s.intent = match i {
                "WebIntent" => designcraft_doc::Intent::Web,
                "MobileIntent" => designcraft_doc::Intent::Mobile,
                _ => designcraft_doc::Intent::Print,
            };
        }
        if let Some(v) = e.boolean("CreatePrimaryTextFrame") {
            s.primary_text_frame = v;
        }
    }

    pub(super) fn grid_prefs(&mut self, e: &El) {
        let s = &mut self.settings;
        let bg: &mut BaselineGrid = &mut s.baseline_grid;
        if let Some(v) = e.num("BaselineStart") {
            bg.start = v;
        }
        if let Some(v) = e.num("BaselineDivision") {
            bg.increment = v;
        }
        if let Some(v) = e.num("BaselineViewThreshold") {
            bg.view_threshold = v / 100.0;
        }
        if let Some(v) = e.get("BaselineGridRelativeOption") {
            bg.relative_to = if v.contains("Margin") { GridRelative::TopMargin } else { GridRelative::TopOfPage };
        }
        if let Some(v) = e.num("HorizontalGridlineDivision") {
            s.grid.horizontal = v;
        }
        if let Some(v) = e.num("VerticalGridlineDivision") {
            s.grid.vertical = v;
        }
        if let Some(v) = e.num("HorizontalGridSubdivision") {
            s.grid.subdivisions = v.max(1.0) as u32;
        }
        if let Some(v) = e.boolean("GridsInBack") {
            s.grid.in_back = v;
        }
    }

    /// Preferences › `FootnoteOption` (after styles: it names the footnote text and marker styles).
    pub(super) fn footnote_option(&mut self, e: &El) {
        let mut o = designcraft_doc::FootnoteOptions::default();
        if let Some(v) = e.num("StartAt") {
            o.start_at = v.max(0.0) as u32;
        }
        o.prefix = e.get("Prefix").unwrap_or("").into();
        o.suffix = e.get("Suffix").unwrap_or("").into();
        if let Some(r) = e.get("FootnoteTextStyle") {
            o.para_style = self.para_style_ref(r);
        }
        if let Some(r) = e.get("FootnoteMarkerStyle") {
            o.ref_char_style = self.char_style_ref(r);
        }
        if let Some(v) = e.get("SeparatorText") {
            o.separator = v.into();
        }
        o.space_between = e.num("SpaceBetween").unwrap_or(0.0);
        o.space_before = e.num("Spacer").unwrap_or(0.0);
        if let Some(v) = e.prop("FootnoteFirstBaselineOffset") {
            o.first_baseline = names::first_baseline_in(v.trim());
        }
        o.first_baseline_min = e.num("FootnoteMinimumFirstBaselineOffset").unwrap_or(0.0);
        o.span_columns = e.boolean("EnableStraddling").unwrap_or(false);
        o.rule.on = e.boolean("RuleOn").unwrap_or(true);
        o.rule.weight = e.num("RuleLineWeight").unwrap_or(1.0);
        o.rule.tint = (e.num("RuleTint").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0) as f32;
        o.rule.left_indent = e.num("RuleLeftIndent").unwrap_or(0.0);
        o.rule.width = e.num("RuleWidth").unwrap_or(72.0);
        o.rule.offset = e.num("RuleOffset").unwrap_or(0.0);
        if let Some(c) = e.prop("RuleColor") {
            o.rule.color = self.swatch_ref(c.trim());
        }
        if let Some(v) = e.prop("FootnoteNumberingStyle") {
            o.style = names::note_style_in(v.trim());
        }
        if let Some(v) = e.prop("RestartNumbering") {
            o.restart = names::NOTE_RESTART.iter().find(|r| r.1 == v.trim()).map_or(Default::default(), |r| r.0);
        }
        if let Some(v) = e.prop("ShowPrefixSuffix") {
            o.affix_in = names::NOTE_AFFIX.iter().find(|r| r.1 == v.trim()).map_or(Default::default(), |r| r.0);
        }
        if let Some(v) = e.prop("MarkerPositioning") {
            o.ref_position = names::note_marker_in(v.trim());
        }
        self.footnote_options = o;
    }
}
