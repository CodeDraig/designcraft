//! Write document, grid and footnote preferences.

use super::{CHAR_BUILTINS, El, Ex, NO_CHAR_ID, Node, PARA_BUILTINS, bool_s, document, margin_el, names, num, pct, pkg_root, pt, st};

impl Ex<'_> {
    pub(super) fn prefs_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let s = &d.settings;
        let mut root = pkg_root("Preferences");
        root.push(
            El::new("DocumentPreference")
                .attr("PageHeight", num(s.page_height))
                .attr("PageWidth", num(s.page_width))
                // InDesign creates this many pages before reading the spreads and replaces only the first
                // spread, so (as InDesign itself writes) the value must be 1.
                .attr("PagesPerDocument", 1)
                .attr("FacingPages", bool_s(s.facing_pages))
                .attr("DocumentBleedTopOffset", num(s.bleed[0]))
                .attr("DocumentBleedBottomOffset", num(s.bleed[1]))
                .attr("DocumentBleedInsideOrLeftOffset", num(s.bleed[2]))
                .attr("DocumentBleedOutsideOrRightOffset", num(s.bleed[3]))
                .attr("DocumentBleedUniformSize", bool_s(s.bleed.iter().all(|b| *b == s.bleed[0])))
                .attr("SlugTopOffset", num(s.slug[0]))
                .attr("SlugBottomOffset", num(s.slug[1]))
                .attr("SlugInsideOrLeftOffset", num(s.slug[2]))
                .attr("SlugRightOrOutsideOffset", num(s.slug[3]))
                .attr("DocumentSlugUniformSize", bool_s(s.slug.iter().all(|b| *b == s.slug[0])))
                .attr("PreserveLayoutWhenShuffling", "true")
                .attr("AllowPageShuffle", "true")
                .attr("OverprintBlack", bool_s(s.overprint_black))
                .attr("PageBinding", if s.right_to_left_binding { "RightToLeft" } else { "LeftToRight" })
                .attr("ColumnDirection", "Horizontal")
                .attr(
                    "Intent",
                    match s.intent {
                        designcraft_doc::Intent::Print => "PrintIntent",
                        designcraft_doc::Intent::Web => "WebIntent",
                        designcraft_doc::Intent::Mobile => "MobileIntent",
                    },
                )
                .attr("CreatePrimaryTextFrame", bool_s(s.primary_text_frame))
                .attr("ColumnGuideLocked", "true")
                .attr("MasterTextFrame", bool_s(s.primary_text_frame))
                .attr("SnippetImportUsesOriginalLocation", "false"),
        );
        let page = d.parents.first().and_then(|p| p.pages.first()).or_else(|| d.page(0));
        if let Some(pg) = page {
            root.push(margin_el(pg));
        }
        root.push(
            El::new("ViewPreference")
                .attr("HorizontalMeasurementUnits", names::unit_out(s.horizontal_units))
                .attr("VerticalMeasurementUnits", names::unit_out(s.vertical_units))
                .attr("RulerOrigin", "SpreadOrigin")
                .attr("CursorKeyIncrement", num(s.keyboard_increment))
                .attr("PointsPerInch", "72"),
        );
        let bg = &s.baseline_grid;
        root.push(
            El::new("GridPreference")
                .attr("HorizontalGridlineDivision", num(s.grid.horizontal))
                .attr("VerticalGridlineDivision", num(s.grid.vertical))
                .attr("HorizontalGridSubdivision", s.grid.subdivisions)
                .attr("VerticalGridSubdivision", s.grid.subdivisions)
                .attr("GridsInBack", bool_s(s.grid.in_back))
                .attr("BaselineStart", num(bg.start))
                .attr("BaselineDivision", num(bg.increment))
                .attr("BaselineViewThreshold", pct(bg.view_threshold))
                .attr(
                    "BaselineGridRelativeOption",
                    match bg.relative_to {
                        designcraft_doc::GridRelative::TopOfPage => "TopOfPageOfBaselineGridRelativeOption",
                        designcraft_doc::GridRelative::TopMargin => "TopOfMarginOfBaselineGridRelativeOption",
                    },
                ),
        );
        root.push(El::new("PasteboardPreference").attr("PasteboardMargins", pt(s.pasteboard.0, s.pasteboard.1)));
        root.push(self.footnote_option_el());
        root.push(El::new("TransparencyPreference").attr(
            "BlendingSpace",
            match s.blend_space {
                designcraft_doc::BlendSpace::Cmyk => "CMYK",
                designcraft_doc::BlendSpace::Rgb => "RGB",
            },
        ));
        let a = &s.advanced_type;
        root.push(
            El::new("TextPreference")
                .attr("SuperscriptSize", num(a.superscript_size))
                .attr("SuperscriptPosition", num(a.superscript_position))
                .attr("SubscriptSize", num(a.subscript_size))
                .attr("SubscriptPosition", num(a.subscript_position)),
        );
        document(&root)
    }

    pub(super) fn footnote_option_el(&self) -> El {
        let o = &self.d.footnote_options;
        let marker = if o.ref_char_style == st::NO_CHAR_STYLE {
            NO_CHAR_ID.to_string()
        } else {
            names::style_self("CharacterStyle", CHAR_BUILTINS, &o.ref_char_style)
        };
        let enumv = |name: &str, v: &str| {
            let mut e = El::new(name).attr("type", "enumeration");
            e.children.push(Node::Text(v.into()));
            e
        };
        let mut color = El::new("RuleColor").attr("type", "object");
        color.children.push(Node::Text(self.sw(&o.rule.color)));
        let mut el = El::new("FootnoteOption")
            .attr("StartAt", o.start_at)
            .attr("Prefix", &o.prefix)
            .attr("Suffix", &o.suffix)
            .attr("FootnoteTextStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, &o.para_style))
            .attr("FootnoteMarkerStyle", marker)
            .attr("SeparatorText", &o.separator)
            .attr("SpaceBetween", num(o.space_between))
            .attr("Spacer", num(o.space_before))
            .attr("FootnoteFirstBaselineOffset", names::first_baseline_out(o.first_baseline))
            .attr("FootnoteMinimumFirstBaselineOffset", num(o.first_baseline_min))
            .attr("EnableStraddling", bool_s(o.span_columns))
            .attr("RuleOn", bool_s(o.rule.on))
            .attr("RuleLineWeight", num(o.rule.weight))
            .attr("RuleTint", num(o.rule.tint as f64 * 100.0))
            .attr("RuleLeftIndent", num(o.rule.left_indent))
            .attr("RuleWidth", num(o.rule.width))
            .attr("RuleOffset", num(o.rule.offset));
        let restart = names::NOTE_RESTART.iter().find(|r| r.0 == o.restart).map_or("DontRestart", |r| r.1);
        let affix = names::NOTE_AFFIX.iter().find(|r| r.0 == o.affix_in).map_or("NoPrefixSuffix", |r| r.1);
        let mut props = El::new("Properties");
        props.push(enumv("FootnoteNumberingStyle", names::note_style_out(o.style)));
        props.push(enumv("RestartNumbering", restart));
        props.push(enumv("ShowPrefixSuffix", affix));
        props.push(enumv("MarkerPositioning", names::note_marker_out(o.ref_position)));
        props.push(color);
        el.push(props);
        el
    }
}
