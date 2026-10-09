//! Map character and paragraph attributes from IDML.

use super::{CharAttrs, El, GridAlign, Importer, Kerning, ListType, ParaAttrs, Rule, SpanColumns, TabStop, names, tint, unescape_id};

impl Importer<'_> {
    pub(super) fn char_attrs(&mut self, e: &El) -> CharAttrs {
        let mut a = CharAttrs::default();
        if let Some(f) = e.prop("AppliedFont") {
            let f = f.trim();
            // Some writers append the style after a tab.
            let fam = f.split('\t').next().unwrap_or(f);
            if !fam.is_empty() && fam != "$ID/" {
                a.font_family = Some(fam.to_string());
            }
        }
        a.font_style = e.prop("FontStyle");
        a.size = e.num("PointSize");
        a.leading = e.prop("Leading").and_then(|l| names::leading_in(l.trim()));
        // Automatic kerning may carry an inactive numeric value (including the
        // 1e11 IDML sentinel). It must not become a multi-million-em advance.
        a.kerning = e.prop("KerningMethod").and_then(|k| match k.trim().trim_start_matches("$ID/") {
            "Optical" => Some(Kerning::Optical),
            "Metrics" | "Metrics - Roman Only" => Some(Kerning::Metrics),
            "None" => Some(Kerning::None),
            _ => None,
        });
        if a.kerning.is_none() {
            a.kerning = e.num("KerningValue").filter(|v| v.is_finite() && v.abs() < 1e10).map(Kerning::Manual);
        }
        a.tracking = e.num("Tracking");
        a.leading_aki = e.num("LeadingAki").map(|v| (v >= 0.0).then_some(v));
        a.trailing_aki = e.num("TrailingAki").map(|v| (v >= 0.0).then_some(v));
        a.tsume = e.num("Tsume").map(|v| (v / 100.0).clamp(0.0, 1.0));
        a.jidori = e.num("Jidori").map(|v| v.clamp(0.0, 10000.0) as u32);
        a.leading_model = e.prop("LeadingModel").and_then(|v| crate::cjk::leading_in(&v));
        a.character_alignment = e.prop("CharacterAlignment").and_then(|v| crate::cjk::alignment_in(&v));
        a.h_scale = e.num("HorizontalScale").map(|v| v / 100.0);
        a.v_scale = e.num("VerticalScale").map(|v| v / 100.0);
        a.baseline_shift = e.num("BaselineShift");
        a.skew = e.num("Skew");
        if let Some(r) = e.prop("FillColor") {
            a.fill = Some(self.swatch_ref(r.trim()));
        }
        a.fill_tint = tint(e.num("FillTint"));
        if let Some(r) = e.prop("StrokeColor") {
            a.stroke = Some(self.swatch_ref(r.trim()));
        }
        a.stroke_tint = tint(e.num("StrokeTint"));
        a.stroke_weight = e.num("StrokeWeight");
        a.capitalization = e.prop("Capitalization").and_then(|v| names::caps_in(v.trim()));
        a.position = e.prop("Position").and_then(|v| names::position_in(v.trim()));
        a.underline = e.boolean("Underline");
        a.strikethrough = e.boolean("StrikeThru");
        a.ligatures = e.boolean("Ligatures");
        // Underline / strikethrough options (-9999 = automatic).
        for k in ["Underline", "StrikeThru"] {
            let num = |n: &str| e.num(&format!("{k}{n}")).filter(|v| *v > -9000.0);
            let color = e.prop(&format!("{k}Color")).map(|r| self.swatch_ref(r.trim())).filter(|r| r != "Text Color" && !r.is_empty());
            let tint = e.num(&format!("{k}Tint")).filter(|v| *v >= 0.0).map(|v| (v / 100.0) as f32);
            if k == "Underline" {
                a.underline_weight = num("Weight").map(Some);
                a.underline_offset = num("Offset").map(Some);
                a.underline_color = color;
                a.underline_tint = tint;
            } else {
                a.strikethrough_weight = num("Weight").map(Some);
                a.strikethrough_offset = num("Offset").map(Some);
                a.strikethrough_color = color;
                a.strikethrough_tint = tint;
            }
        }
        // OpenType features (only when the element sets any).
        let mut otf_list: Vec<String> = Vec::new();
        let mut any = false;
        for (tag, _, attr) in designcraft_doc::otf::TOGGLES {
            if let Some(on) = e.boolean(attr) {
                designcraft_doc::otf::set(&mut otf_list, tag, on);
                any = true;
            }
        }
        if let Some(fig) = e.prop("OTFFigureStyle")
            && let Some((id, ..)) = designcraft_doc::otf::FIGURES.iter().find(|f| f.3 == fig.trim())
        {
            designcraft_doc::otf::set_figures(&mut otf_list, id);
            any = true;
        }
        if let Some(m) = e.num("OTFStylisticSets") {
            designcraft_doc::otf::set_stylistic_sets(&mut otf_list, m.max(0.0) as u32);
            any = true;
        }
        if any {
            a.otf_features = Some(otf_list);
        }
        a.glyph_form = e.prop("GlyphForm");
        a.no_break = e.boolean("NoBreak");
        a.tate_chu_yoko = e.boolean("Tatechuyoko");
        a.tate_chu_yoko_x_offset = e.num("TatechuyokoXOffset");
        a.tate_chu_yoko_y_offset = e.num("TatechuyokoYOffset");
        if let Some(on) = e.boolean("RubyFlag") {
            a.ruby = Some(if on { e.prop("RubyString").unwrap_or_default() } else { String::new() });
        }
        if let Some(k) = e.get("KentenKind") {
            a.kenten = Some(k != "None");
            a.kenten_character = crate::cjk::kenten_character(k).map(str::to_string);
            if k == "Custom" {
                a.kenten_character = e.prop("KentenCustomCharacter");
            }
        }
        a.digits = e.prop("DigitsType").and_then(|s| names::digits_in(s.trim()));
        a.character_direction = e.prop("CharacterDirection").and_then(|s| crate::arabic::direction_in(&s));
        a.allow_kashidas = e.prop("Kashidas").and_then(|s| crate::arabic::kashidas_in(&s));
        a.diacritic_position = e.prop("DiacriticPosition").and_then(|s| crate::arabic::diacritic_in(&s));
        a.diacritic_x_offset = e.num("XOffsetDiacritic").filter(|v| v.is_finite());
        a.diacritic_y_offset = e.num("YOffsetDiacritic").filter(|v| v.is_finite());
        a.positional_form = e.prop("PositionalForm");
        if let Some(v) = e.get("AppliedConditions").filter(|v| !v.trim().is_empty()) {
            let names: Vec<String> = v.split_whitespace().filter_map(|r| self.condition_names.get(r).cloned()).collect();
            if !names.is_empty() {
                a.conditions = Some(names);
            }
        }
        a.language = e.prop("AppliedLanguage").map(|l| l.trim().trim_start_matches("$ID/").to_string());
        a
    }

    pub(super) fn para_attrs(&mut self, e: &El) -> ParaAttrs {
        let mut a = ParaAttrs::default();
        let u = |k: &str| e.num(k).map(|v| v.max(0.0) as u32);
        let frac = |k: &str| e.num(k).map(|v| v / 100.0);
        a.align = e.prop("Justification").and_then(|v| names::align_in(v.trim()));
        a.kinsoku_hang = e.prop("KinsokuHangType").and_then(|v| match v.as_str() {
            "None" => Some(designcraft_doc::cjk::KinsokuHang::None),
            "KinsokuHangRegular" => Some(designcraft_doc::cjk::KinsokuHang::Regular),
            "KinsokuHangForce" => Some(designcraft_doc::cjk::KinsokuHang::Force),
            _ => None,
        });
        a.mojikumi = e.prop("Mojikumi");
        a.kinsoku_type = e.prop("KinsokuType");
        a.bunri_kinshi = e.boolean("BunriKinshi");
        a.rensuuji = e.boolean("Rensuuji");
        a.treat_ideographic_space_as_space = e.boolean("TreatIdeographicSpaceAsSpace");
        if let Some(k) = e.prop("KinsokuSet") {
            if k == "Nothing" || k == "None" {
                a.kinsoku = Some(Some(Default::default()));
            } else if let Some(set) = self.kinsoku.get(&k) {
                a.kinsoku = Some(Some(set.clone()));
            } else if let Some(set) = designcraft_doc::cjk::Kinsoku::named(&k) {
                a.kinsoku = Some(Some(set));
            }
        }
        a.direction = e.prop("ParagraphDirection").map(|v| {
            if v.trim() == "RightToLeftDirection" { designcraft_doc::TextDirection::RightToLeft } else { designcraft_doc::TextDirection::LeftToRight }
        });
        a.kashidas = e.prop("Kashidas").and_then(|s| crate::arabic::kashidas_in(&s));
        a.arabic_justification = e.prop("ParagraphJustification");
        a.paragraph_kashida_width = e.num("ParagraphKashidaWidth").filter(|v| v.is_finite()).map(Some);
        a.left_indent = e.num("LeftIndent");
        a.right_indent = e.num("RightIndent");
        a.first_line_indent = e.num("FirstLineIndent");
        a.last_line_indent = e.num("LastLineIndent");
        a.space_before = e.num("SpaceBefore");
        if let Some(r) = e.prop("AppliedNumberingList") {
            let r = r.trim().trim_start_matches("NumberingList/");
            a.list_name = Some(if r.starts_with("$ID/") { String::new() } else { unescape_id(r) });
        }
        match e.prop("NumberingContinue").as_deref().map(str::trim) {
            Some("false") => a.start_at = Some(e.num("NumberingStartAt").map(|n| n.max(1.0) as u32)),
            Some("true") => a.start_at = Some(None),
            _ => {}
        }
        // Nested and GREP styles.
        if let Some(l) = e.prop_el("AllNestedStyles") {
            let v: Vec<designcraft_doc::NestedStyle> = l
                .find_all("ListItem")
                .map(|it| {
                    let t = |k: &str| it.find(k).map(|x| x.text_content()).unwrap_or_default();
                    let d = it.find("Delimiter");
                    designcraft_doc::NestedStyle {
                        style: self.char_style_ref(t("AppliedCharacterStyle").trim()),
                        through: t("Inclusive").trim() != "false",
                        count: t("Repetition").trim().parse().unwrap_or(1),
                        until: names::nested_until_in(d.and_then(|d| d.get("type")).unwrap_or("enumeration"), &t("Delimiter")),
                    }
                })
                .collect();
            a.nested_styles = Some(v);
        }
        if let Some(l) = e.prop_el("AllNestedLineStyles") {
            let v: Vec<designcraft_doc::NestedLineStyle> = l
                .find_all("ListItem")
                .map(|it| {
                    let t = |k: &str| it.find(k).map(|x| x.text_content()).unwrap_or_default();
                    designcraft_doc::NestedLineStyle {
                        style: self.char_style_ref(t("AppliedCharacterStyle").trim()),
                        lines: t("LineCount").trim().parse().unwrap_or(1),
                    }
                })
                .collect();
            a.nested_line_styles = Some(v);
        }
        if let Some(l) = e.prop_el("AllGREPStyles") {
            let v: Vec<designcraft_doc::GrepStyle> = l
                .find_all("ListItem")
                .map(|it| {
                    let t = |k: &str| it.find(k).map(|x| x.text_content()).unwrap_or_default();
                    designcraft_doc::GrepStyle { style: self.char_style_ref(t("AppliedCharacterStyle").trim()), pattern: t("GrepExpression") }
                })
                .collect();
            a.grep_styles = Some(v);
        }
        a.space_after = e.num("SpaceAfter");
        a.drop_cap_lines = u("DropCapLines");
        a.drop_cap_chars = u("DropCapCharacters");
        if let Some(g) = e.prop("GridAlignment") {
            a.grid_align = Some(if g == "None" {
                GridAlign::None
            } else if e.boolean("GridAlignFirstLineOnly") == Some(true) {
                GridAlign::FirstLineOnly
            } else {
                GridAlign::AllLines
            });
        }
        a.composer = e
            .prop("Composer")
            .map(|c| if c.contains("Single") { designcraft_doc::Composer::SingleLine } else { designcraft_doc::Composer::Paragraph });
        a.hyphenate = e.boolean("Hyphenation");
        a.hyph_min_word = u("HyphenateWordsLongerThan");
        a.hyph_after_first = u("HyphenateAfterFirst");
        a.hyph_before_last = u("HyphenateBeforeLast");
        a.hyph_limit = u("HyphenateLadderLimit");
        a.hyph_zone = e.num("HyphenationZone");
        a.hyph_capitalized = e.boolean("HyphenateCapitalizedWords");
        a.hyph_last_word = e.boolean("HyphenateLastWord");
        a.hyph_across_column = e.boolean("HyphenateAcrossColumns");
        a.hyph_weight = e.num("HyphenWeight").map(|v| v / 10.0);
        a.word_space_min = frac("MinimumWordSpacing");
        a.word_space_desired = frac("DesiredWordSpacing");
        a.word_space_max = frac("MaximumWordSpacing");
        a.letter_space_min = frac("MinimumLetterSpacing");
        a.letter_space_desired = frac("DesiredLetterSpacing");
        a.letter_space_max = frac("MaximumLetterSpacing");
        a.glyph_scale_min = frac("MinimumGlyphScaling");
        a.glyph_scale_desired = frac("DesiredGlyphScaling");
        a.glyph_scale_max = frac("MaximumGlyphScaling");
        a.auto_leading = frac("AutoLeading");
        a.single_word_justify = e.prop("SingleWordJustification").and_then(|v| names::align_in(v.trim()));
        a.keep_with_next = u("KeepWithNext");
        a.keep_lines_together = e.boolean("KeepLinesTogether");
        a.keep_all_lines = e.boolean("KeepAllLinesTogether");
        a.keep_first = u("KeepFirstLines");
        a.keep_last = u("KeepLastLines");
        a.start_paragraph = e.prop("StartParagraph").and_then(|v| names::start_para_in(v.trim()));
        if let Some(t) = e.prop("SpanColumnType") {
            let n = e.prop("SpanSplitColumnCount").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
            a.span_columns = Some(match t.trim() {
                "SpanColumns" => SpanColumns::Span(n),
                "SplitColumns" => SpanColumns::Split(n.max(2)),
                _ => SpanColumns::Single,
            });
        }
        a.rule_above = self.rule(e, "RuleAbove");
        a.rule_below = self.rule(e, "RuleBelow");
        if let Some(tl) = e.prop_el("TabList") {
            let mut tabs = Vec::new();
            for li in tl.find_all("ListItem") {
                let get = |k: &str| li.find(k).map(|x| x.text_content());
                tabs.push(TabStop {
                    position: get("Position").and_then(|v| v.trim().parse().ok()).unwrap_or(0.0),
                    align: names::tab_align_in(get("Alignment").unwrap_or_default().trim()),
                    leader: get("Leader").unwrap_or_default(),
                    align_on: get("AlignmentCharacter").filter(|c| c != ".").unwrap_or_default(),
                });
            }
            a.tabs = Some(tabs);
        }
        a.list_type = e.prop("BulletsAndNumberingListType").map(|v| match v.trim() {
            "BulletList" => ListType::Bullets,
            "NumberedList" => ListType::Numbers,
            _ => ListType::None,
        });
        a.balance_ragged = e.prop("BalanceRaggedLines").map(|v| v.trim() != "NoBalancing" && v.trim() != "false");
        a.shading_on = e.boolean("ParagraphShadingOn");
        if let Some(c) = e.prop("ParagraphShadingColor") {
            a.shading_color = Some(self.swatch_ref(c.trim()));
        }
        a.shading_tint = tint(e.num("ParagraphShadingTint"));
        let sides = |f: &dyn Fn(&str) -> String| -> Option<[f64; 4]> {
            let v = ["Top", "Left", "Bottom", "Right"].map(|s| e.num(&f(s)));
            v.iter().any(Option::is_some).then(|| v.map(|x| x.unwrap_or(0.0)))
        };
        a.shading_offsets = sides(&|s| format!("ParagraphShading{s}Offset"));
        a.border_on = e.boolean("ParagraphBorderOn");
        if let Some(c) = e.prop("ParagraphBorderColor") {
            a.border_color = Some(self.swatch_ref(c.trim()));
        }
        a.border_tint = tint(e.num("ParagraphBorderTint"));
        a.border_weights = sides(&|s| format!("ParagraphBorder{s}LineWeight"));
        a.border_offsets = sides(&|s| format!("ParagraphBorder{s}Offset"));
        a
    }

    pub(super) fn rule(&mut self, e: &El, k: &str) -> Option<Rule> {
        let on = e.boolean(k);
        let weight = e.num(&format!("{k}LineWeight"));
        on.or(weight.map(|_| false))?;
        let mut r = Rule { on: on.unwrap_or(false), ..Rule::default() };
        if let Some(w) = weight {
            r.weight = w;
        }
        if let Some(c) = e.prop(&format!("{k}Color")) {
            let c = c.trim();
            if c.contains('/') {
                r.color = self.swatch_ref(c);
            }
        }
        if let Some(t) = tint(e.num(&format!("{k}Tint"))) {
            r.tint = t;
        }
        r.column_width = e.prop(&format!("{k}Width")).is_none_or(|w| w.trim() != "TextWidth");
        r.offset = e.num(&format!("{k}Offset")).unwrap_or(0.0);
        r.left_indent = e.num(&format!("{k}LeftIndent")).unwrap_or(0.0);
        r.right_indent = e.num(&format!("{k}RightIndent")).unwrap_or(0.0);
        Some(r)
    }
}
