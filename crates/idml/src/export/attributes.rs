//! Write paragraph, character and stroke attribute mappings.

use super::{
    CHAR_BUILTINS, CharAttrs, CharFormat, El, Ex, ListType, NO_CHAR_ID, PARA_BUILTINS, ParaAttrs, ParaFormat, Rule, SpanColumns, bool_s, escape_id,
    names, num, p, pct, st, with_props,
};

impl Ex<'_> {
    pub(super) fn char_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &CharAttrs) {
        if let Some(v) = a.leading_aki {
            el.set("LeadingAki", num(v.unwrap_or(-1.0)));
        }
        if let Some(v) = a.trailing_aki {
            el.set("TrailingAki", num(v.unwrap_or(-1.0)));
        }
        if let Some(v) = a.tsume {
            el.set("Tsume", num(v * 100.0));
        }
        if let Some(v) = a.jidori {
            el.set("Jidori", v);
        }
        if let Some(v) = a.leading_model {
            el.set("LeadingModel", crate::cjk::leading_out(v));
        }
        if let Some(v) = a.character_alignment {
            el.set("CharacterAlignment", crate::cjk::alignment_out(v));
        }
        if let Some(f) = &a.font_family {
            props.push(p("AppliedFont", "string", f.clone()));
            let style = a.font_style.clone().unwrap_or_else(|| "Regular".into());
            self.note_font(f, &style);
        }
        if let Some(v) = &a.font_style {
            el.set("FontStyle", v);
        }
        if let Some(v) = a.size {
            el.set("PointSize", num(v));
        }
        if let Some(l) = a.leading {
            let (v, ty) = names::leading_out(l);
            props.push(p("Leading", ty, v));
        }
        if let Some(k) = a.kerning {
            match k {
                designcraft_doc::Kerning::Metrics => el.set("KerningMethod", "$ID/Metrics"),
                designcraft_doc::Kerning::Optical => el.set("KerningMethod", "$ID/Optical"),
                designcraft_doc::Kerning::None => el.set("KerningMethod", "None"),
                designcraft_doc::Kerning::Manual(v) => el.set("KerningValue", num(v)),
            }
        }
        if let Some(v) = a.tracking {
            el.set("Tracking", num(v));
        }
        if let Some(v) = a.h_scale {
            el.set("HorizontalScale", pct(v));
        }
        if let Some(v) = a.v_scale {
            el.set("VerticalScale", pct(v));
        }
        if let Some(v) = a.baseline_shift {
            el.set("BaselineShift", num(v));
        }
        if let Some(v) = a.skew {
            el.set("Skew", num(v));
        }
        if let Some(v) = &a.fill {
            el.set("FillColor", self.sw(v));
        }
        if let Some(v) = a.fill_tint {
            el.set("FillTint", pct(v as f64));
        }
        if let Some(v) = &a.stroke {
            el.set("StrokeColor", self.sw(v));
        }
        if let Some(v) = a.stroke_tint {
            el.set("StrokeTint", pct(v as f64));
        }
        if let Some(v) = a.stroke_weight {
            el.set("StrokeWeight", num(v));
        }
        if let Some(v) = a.capitalization {
            el.set("Capitalization", names::caps_out(v));
        }
        if let Some(v) = a.position {
            el.set("Position", names::position_out(v));
        }
        if let Some(v) = a.underline {
            el.set("Underline", bool_s(v));
        }
        if let Some(v) = a.strikethrough {
            el.set("StrikeThru", bool_s(v));
        }
        if let Some(v) = a.ligatures {
            el.set("Ligatures", bool_s(v));
        }
        for (k, w, o, c, t) in [
            ("Underline", a.underline_weight, a.underline_offset, &a.underline_color, a.underline_tint),
            ("StrikeThru", a.strikethrough_weight, a.strikethrough_offset, &a.strikethrough_color, a.strikethrough_tint),
        ] {
            if let Some(w) = w {
                el.set(&format!("{k}Weight"), num(w.unwrap_or(-9999.0)));
            }
            if let Some(o) = o {
                el.set(&format!("{k}Offset"), num(o.unwrap_or(-9999.0)));
            }
            if let Some(c) = c.as_ref().filter(|c| !c.is_empty()) {
                el.set(&format!("{k}Color"), self.sw(c));
            }
            if let Some(t) = t {
                el.set(&format!("{k}Tint"), num(t as f64 * 100.0));
            }
        }
        if let Some(list) = &a.otf_features {
            for (tag, _, attr) in designcraft_doc::otf::TOGGLES {
                el.set(attr, bool_s(designcraft_doc::otf::is_on(list, tag)));
            }
            let fig = designcraft_doc::otf::figures(list);
            if let Some(f) = designcraft_doc::otf::FIGURES.iter().find(|f| f.0 == fig) {
                el.set("OTFFigureStyle", f.3);
            }
            el.set("OTFStylisticSets", designcraft_doc::otf::stylistic_sets(list).to_string());
        }
        if let Some(v) = &a.glyph_form {
            el.set("GlyphForm", v);
        }
        if let Some(v) = a.no_break {
            el.set("NoBreak", bool_s(v));
        }
        if let Some(v) = a.digits {
            el.set("DigitsType", names::digits_out(v));
        }
        if let Some(v) = a.character_direction {
            el.set("CharacterDirection", crate::arabic::direction_out(v));
        }
        if let Some(v) = a.allow_kashidas {
            el.set("Kashidas", if v { "DefaultKashidas" } else { "KashidasOff" });
        }
        if let Some(v) = a.diacritic_position {
            el.set("DiacriticPosition", crate::arabic::diacritic_out(v));
        }
        if let Some(v) = a.diacritic_x_offset {
            el.set("XOffsetDiacritic", num(v));
        }
        if let Some(v) = a.diacritic_y_offset {
            el.set("YOffsetDiacritic", num(v));
        }
        if let Some(v) = &a.positional_form {
            el.set("PositionalForm", v);
        }
        if let Some(r) = a.ruby.as_ref() {
            el.set("RubyFlag", bool_s(!r.is_empty()));
            el.set("RubyString", r.as_str());
        }
        if let Some(k) = a.kenten {
            el.set("KentenKind", if k { "KentenSesameDot" } else { "None" });
        }
        if let Some(c) = &a.kenten_character
            && !c.is_empty()
        {
            el.set("KentenKind", "Custom");
            el.set("KentenCustomCharacter", c);
        }
        if let Some(v) = a.tate_chu_yoko_x_offset {
            el.set("TatechuyokoXOffset", num(v));
        }
        if let Some(v) = a.tate_chu_yoko_y_offset {
            el.set("TatechuyokoYOffset", num(v));
        }
        if let Some(v) = a.tate_chu_yoko {
            el.set("Tatechuyoko", bool_s(v));
        }
        if let Some(list) = &a.conditions {
            el.set("AppliedConditions", list.iter().map(|c| format!("Condition/{}", escape_id(c))).collect::<Vec<_>>().join(" "));
        }
        if let Some(v) = &a.language {
            el.set("AppliedLanguage", format!("$ID/{v}"));
        }
    }

    pub(super) fn para_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &ParaAttrs) {
        if let Some(v) = &a.mojikumi {
            props.push(p("Mojikumi", if v.starts_with("MojikumiTable/") { "object" } else { "enumeration" }, v));
        }
        if let Some(v) = &a.kinsoku_type {
            el.set("KinsokuType", v);
        }
        if let Some(v) = a.kinsoku_hang {
            el.set(
                "KinsokuHangType",
                match v {
                    designcraft_doc::cjk::KinsokuHang::None => "None",
                    designcraft_doc::cjk::KinsokuHang::Regular => "KinsokuHangRegular",
                    designcraft_doc::cjk::KinsokuHang::Force => "KinsokuHangForce",
                },
            );
        }
        if let Some(v) = a.bunri_kinshi {
            el.set("BunriKinshi", bool_s(v));
        }
        if let Some(v) = a.rensuuji {
            el.set("Rensuuji", bool_s(v));
        }
        if let Some(v) = a.treat_ideographic_space_as_space {
            el.set("TreatIdeographicSpaceAsSpace", bool_s(v));
        }
        if let Some(Some(k)) = &a.kinsoku {
            props.push(p(
                "KinsokuSet",
                if k.name.is_empty() { "enumeration" } else { "object" },
                if k.name.is_empty() { "Nothing".into() } else { format!("KinsokuTable/{}", escape_id(&k.name)) },
            ));
        }
        if let Some(n) = &a.list_name {
            el.set(
                "AppliedNumberingList",
                if n.is_empty() { "NumberingList/$ID/[Default]".to_string() } else { format!("NumberingList/{}", escape_id(n)) },
            );
        }
        if let Some(v) = a.start_at {
            match v {
                Some(n) => {
                    el.set("NumberingContinue", "false");
                    el.set("NumberingStartAt", n);
                }
                None => el.set("NumberingContinue", "true"),
            }
        }
        // Nested and GREP styles: property lists of records.
        if let Some(list) = &a.nested_styles {
            let mut l = El::new("AllNestedStyles").attr("type", "list");
            for ns in list {
                let (ty, delim) = names::nested_until_out(&ns.until);
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &ns.style)))
                        .child(p("Delimiter", ty, delim))
                        .child(p("Repetition", "long", ns.count.to_string()))
                        .child(p("Inclusive", "boolean", bool_s(ns.through))),
                );
            }
            props.push(l);
        }
        if let Some(list) = &a.nested_line_styles {
            let mut l = El::new("AllNestedLineStyles").attr("type", "list");
            for ns in list {
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &ns.style)))
                        .child(p("LineCount", "long", ns.lines.to_string())),
                );
            }
            props.push(l);
        }
        if let Some(list) = &a.grep_styles {
            let mut l = El::new("AllGREPStyles").attr("type", "list");
            for g in list {
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &g.style)))
                        .child(p("GrepExpression", "string", g.pattern.clone())),
                );
            }
            props.push(l);
        }
        macro_rules! n {
            ($f:ident, $k:literal) => {
                if let Some(v) = a.$f {
                    el.set($k, num(v as f64));
                }
            };
            ($f:ident, $k:literal, pct) => {
                if let Some(v) = a.$f {
                    el.set($k, pct(v as f64));
                }
            };
        }
        macro_rules! b {
            ($f:ident, $k:literal) => {
                if let Some(v) = a.$f {
                    el.set($k, bool_s(v));
                }
            };
        }
        if let Some(v) = a.align {
            el.set("Justification", names::align_out(v));
        }
        if let Some(v) = a.direction {
            el.set(
                "ParagraphDirection",
                if v == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" },
            );
        }
        if let Some(v) = a.kashidas {
            el.set("Kashidas", if v { "DefaultKashidas" } else { "KashidasOff" });
        }
        if let Some(v) = &a.arabic_justification {
            el.set("ParagraphJustification", v);
        }
        if let Some(Some(v)) = a.paragraph_kashida_width {
            el.set("ParagraphKashidaWidth", num(v));
        }
        n!(left_indent, "LeftIndent");
        n!(right_indent, "RightIndent");
        n!(first_line_indent, "FirstLineIndent");
        n!(last_line_indent, "LastLineIndent");
        n!(space_before, "SpaceBefore");
        n!(space_after, "SpaceAfter");
        n!(drop_cap_lines, "DropCapLines");
        n!(drop_cap_chars, "DropCapCharacters");
        if let Some(g) = a.grid_align {
            match g {
                designcraft_doc::GridAlign::None => el.set("GridAlignment", "None"),
                designcraft_doc::GridAlign::AllLines => {
                    el.set("GridAlignment", "AlignToBaseline");
                    el.set("GridAlignFirstLineOnly", "false");
                }
                designcraft_doc::GridAlign::FirstLineOnly => {
                    el.set("GridAlignment", "AlignToBaseline");
                    el.set("GridAlignFirstLineOnly", "true");
                }
            }
        }
        if let Some(c) = a.composer {
            el.set(
                "Composer",
                match c {
                    designcraft_doc::Composer::Paragraph => "HL Composer",
                    designcraft_doc::Composer::SingleLine => "HL Single",
                },
            );
        }
        b!(hyphenate, "Hyphenation");
        n!(hyph_min_word, "HyphenateWordsLongerThan");
        n!(hyph_after_first, "HyphenateAfterFirst");
        n!(hyph_before_last, "HyphenateBeforeLast");
        n!(hyph_limit, "HyphenateLadderLimit");
        n!(hyph_zone, "HyphenationZone");
        b!(hyph_capitalized, "HyphenateCapitalizedWords");
        b!(hyph_last_word, "HyphenateLastWord");
        b!(hyph_across_column, "HyphenateAcrossColumns");
        if let Some(v) = a.hyph_weight {
            el.set("HyphenWeight", num((v * 10.0).round()));
        }
        n!(word_space_min, "MinimumWordSpacing", pct);
        n!(word_space_desired, "DesiredWordSpacing", pct);
        n!(word_space_max, "MaximumWordSpacing", pct);
        n!(letter_space_min, "MinimumLetterSpacing", pct);
        n!(letter_space_desired, "DesiredLetterSpacing", pct);
        n!(letter_space_max, "MaximumLetterSpacing", pct);
        n!(glyph_scale_min, "MinimumGlyphScaling", pct);
        n!(glyph_scale_desired, "DesiredGlyphScaling", pct);
        n!(glyph_scale_max, "MaximumGlyphScaling", pct);
        n!(auto_leading, "AutoLeading", pct);
        if let Some(v) = a.single_word_justify {
            el.set("SingleWordJustification", names::align_out(v));
        }
        n!(keep_with_next, "KeepWithNext");
        b!(keep_lines_together, "KeepLinesTogether");
        b!(keep_all_lines, "KeepAllLinesTogether");
        n!(keep_first, "KeepFirstLines");
        n!(keep_last, "KeepLastLines");
        if let Some(v) = a.start_paragraph {
            el.set("StartParagraph", names::start_para_out(v));
        }
        if let Some(sc) = a.span_columns {
            match sc {
                SpanColumns::Single => el.set("SpanColumnType", "SingleColumn"),
                SpanColumns::Span(n) | SpanColumns::Split(n) => {
                    el.set("SpanColumnType", if matches!(sc, SpanColumns::Span(_)) { "SpanColumns" } else { "SplitColumns" });
                    props.push(if n == 0 {
                        p("SpanSplitColumnCount", "enumeration", "All")
                    } else {
                        p("SpanSplitColumnCount", "long", n.to_string())
                    });
                }
            }
        }
        if let Some(r) = &a.rule_above {
            self.rule(el, props, "RuleAbove", r);
        }
        if let Some(r) = &a.rule_below {
            self.rule(el, props, "RuleBelow", r);
        }
        if let Some(tabs) = &a.tabs {
            let mut tl = El::new("TabList").attr("type", "list");
            for t in tabs {
                tl.push(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("Alignment", "enumeration", names::tab_align_out(t.align)))
                        .child(p("AlignmentCharacter", "string", if t.align_on.is_empty() { ".".to_string() } else { t.align_on.clone() }))
                        .child(p("Leader", "string", t.leader.clone()))
                        .child(p("Position", "unit", num(t.position))),
                );
            }
            props.push(tl);
        }
        if let Some(l) = a.list_type {
            el.set(
                "BulletsAndNumberingListType",
                match l {
                    ListType::None => "NoList",
                    ListType::Bullets => "BulletList",
                    ListType::Numbers => "NumberedList",
                },
            );
        }
        if let Some(b) = a.balance_ragged {
            props.push(p("BalanceRaggedLines", "enumeration", if b { "FullyBalanced" } else { "NoBalancing" }));
        }
        b!(shading_on, "ParagraphShadingOn");
        if let Some(c) = &a.shading_color {
            props.push(p("ParagraphShadingColor", "object", self.sw(c)));
        }
        n!(shading_tint, "ParagraphShadingTint", pct);
        const SIDES: [&str; 4] = ["Top", "Left", "Bottom", "Right"];
        if let Some(o) = a.shading_offsets {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphShading{side}Offset"), num(o[i]));
            }
        }
        b!(border_on, "ParagraphBorderOn");
        if let Some(c) = &a.border_color {
            props.push(p("ParagraphBorderColor", "object", self.sw(c)));
        }
        n!(border_tint, "ParagraphBorderTint", pct);
        if let Some(w) = a.border_weights {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphBorder{side}LineWeight"), num(w[i]));
            }
        }
        if let Some(o) = a.border_offsets {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphBorder{side}Offset"), num(o[i]));
            }
        }
    }

    pub(super) fn rule(&mut self, el: &mut El, props: &mut Vec<El>, k: &str, r: &Rule) {
        el.set(k, bool_s(r.on));
        el.set(&format!("{k}LineWeight"), num(r.weight));
        el.set(&format!("{k}Tint"), pct(r.tint as f64));
        el.set(&format!("{k}Width"), if r.column_width { "ColumnWidth" } else { "TextWidth" });
        el.set(&format!("{k}Offset"), num(r.offset));
        el.set(&format!("{k}LeftIndent"), num(r.left_indent));
        el.set(&format!("{k}RightIndent"), num(r.right_indent));
        props.push(p(&format!("{k}Color"), "object", self.sw(&r.color)));
    }

    pub(super) fn stroke_attrs(&self, el: &mut El, s: &designcraft_doc::Stroke) {
        el.set("StrokeColor", self.sw(&s.swatch));
        el.set("StrokeWeight", num(if s.is_none() { 0.0 } else { s.weight }));
        el.set("StrokeTint", pct(s.tint as f64));
        el.set("StrokeType", names::stroke_type_out(&s.kind));
        el.set("StrokeAlignment", names::stroke_align_out(s.align));
        el.set("EndCap", names::cap_out(s.cap));
        el.set("EndJoin", names::join_out(s.join));
        el.set("MiterLimit", num(s.miter_limit));
        el.set("LeftLineEnd", names::arrow_out(s.start));
        el.set("RightLineEnd", names::arrow_out(s.end));
        el.set("GapColor", self.sw(&s.gap_swatch));
        el.set("GapTint", pct(s.gap_tint as f64));
        el.set("OverprintStroke", bool_s(s.overprint));
        el.set("OverprintGap", bool_s(s.gap_overprint));
    }

    pub(super) fn psr_el(&mut self, pf: &ParaFormat) -> El {
        let mut el = El::new("ParagraphStyleRange").attr("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, &pf.style));
        let mut props = Vec::new();
        self.para_attrs(&mut el, &mut props, &pf.para);
        self.char_attrs(&mut el, &mut props, &pf.chars);
        with_props(el, props)
    }

    pub(super) fn csr_el(&mut self, f: &CharFormat) -> El {
        let id = if f.style.is_empty() || f.style == st::NO_CHAR_STYLE {
            NO_CHAR_ID.to_string()
        } else {
            names::style_self("CharacterStyle", CHAR_BUILTINS, &f.style)
        };
        let mut el = El::new("CharacterStyleRange").attr("AppliedCharacterStyle", id);
        let mut props = Vec::new();
        self.char_attrs(&mut el, &mut props, &f.over);
        with_props(el, props)
    }
}
