//! Resolve IDML style definitions, groups and references.

use super::{
    AltFills, CHAR_BUILTINS, CellStroke, CharacterStyle, El, Fill, Importer, OBJECT_BUILTINS, ObjectStyle, PARA_BUILTINS, ParagraphStyle, names, st,
    swatch, text_frame_options, tint, unescape_id,
};

impl Importer<'_> {
    pub(super) fn styles(&mut self, top: &[El]) {
        for e in top.iter().filter(|e| e.local() == "Condition") {
            let (Some(id), Some(name)) = (e.get("Self"), e.get("Name")) else { continue };
            // Indicator colours: "r g b" (0–255) or a UI colour name (kept as the default blue).
            let nums: Vec<u8> = e
                .get("IndicatorColor")
                .unwrap_or("")
                .split_whitespace()
                .filter_map(|x| x.parse::<f64>().ok())
                .map(|x| x.clamp(0.0, 255.0) as u8)
                .collect();
            let color = if nums.len() == 3 { [nums[0], nums[1], nums[2]] } else { [79, 153, 255] };
            self.condition_names.insert(id.to_string(), name.to_string());
            self.conditions.push(designcraft_doc::Condition { name: name.to_string(), color, visible: e.get("Visible") != Some("false") });
        }
        for e in top.iter().filter(|e| e.local() == "NumberingList") {
            let name = e.get("Name").unwrap_or("").to_string();
            if !name.is_empty() && !name.starts_with("$ID/") && !self.lists.iter().any(|l| l.name == name) {
                self.lists
                    .push(designcraft_doc::NumberedList { name, continue_across_stories: e.get("ContinueNumbersAcrossStories") != Some("false") });
            }
        }
        let mut paras: Vec<(String, El)> = Vec::new();
        let mut chars: Vec<(String, El)> = Vec::new();
        let mut objects: Vec<(String, El)> = Vec::new();
        let mut cell_styles: Vec<(String, El)> = Vec::new();
        let mut table_styles: Vec<(String, El)> = Vec::new();
        fn walk(e: &El, style: &str, group: &str, out: &mut Vec<(String, El)>) {
            for c in e.elements() {
                if c.local() == style {
                    out.push((c.get("Self").unwrap_or("").to_string(), c.clone()));
                } else if c.local() == group {
                    walk(c, style, group, out);
                }
            }
        }
        for e in top {
            match e.local() {
                "RootParagraphStyleGroup" => walk(e, "ParagraphStyle", "ParagraphStyleGroup", &mut paras),
                "RootCharacterStyleGroup" => walk(e, "CharacterStyle", "CharacterStyleGroup", &mut chars),
                "RootObjectStyleGroup" => walk(e, "ObjectStyle", "ObjectStyleGroup", &mut objects),
                "RootCellStyleGroup" => walk(e, "CellStyle", "CellStyleGroup", &mut cell_styles),
                "RootTableStyleGroup" => walk(e, "TableStyle", "TableStyleGroup", &mut table_styles),
                _ => {}
            }
        }
        for (id, e) in &paras {
            let n = names::style_name_in(PARA_BUILTINS, e.get("Name").unwrap_or(""));
            self.para_names.insert(id.clone(), n);
        }
        for (id, e) in &chars {
            let n = names::style_name_in(CHAR_BUILTINS, e.get("Name").unwrap_or(""));
            self.char_names.insert(id.clone(), n);
        }
        for (id, e) in &objects {
            let n = names::style_name_in(OBJECT_BUILTINS, e.get("Name").unwrap_or(""));
            self.object_names.insert(id.clone(), n);
            self.object_els.insert(id.clone(), e.clone());
        }
        for (_, e) in &paras {
            let name = names::style_name_in(PARA_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = if name == designcraft_doc::NO_PARA_STYLE { None } else { self.based_on(e, true) };
            let next_style = e.get("NextStyle").map(|n| self.para_style_ref(n)).filter(|n| *n != name);
            let para = self.para_attrs(e);
            let chars = self.char_attrs(e);
            let s = ParagraphStyle { name: name.clone(), based_on, next_style, para, chars, shortcut: String::new() };
            match self.styles.para_mut(&name) {
                Some(slot) => *slot = s,
                None => self.styles.paragraph.push(s),
            }
        }
        for (_, e) in &chars {
            let name = names::style_name_in(CHAR_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = if name == st::NO_CHAR_STYLE { None } else { self.based_on(e, false) };
            let chars = self.char_attrs(e);
            let s = CharacterStyle { name: name.clone(), based_on, chars, shortcut: String::new() };
            match self.styles.char_style_mut(&name) {
                Some(slot) => *slot = s,
                None => self.styles.character.push(s),
            }
        }
        // Cell and table styles (the built-ins stay as they are).
        let cell_ref = |r: &str| names::style_name_in(names::CELL_BUILTINS, &unescape_id(r.trim_start_matches("CellStyle/")));
        for (_, e) in &cell_styles {
            let name = names::style_name_in(names::CELL_BUILTINS, e.get("Name").unwrap_or(""));
            if name == designcraft_doc::NO_CELL_STYLE {
                continue;
            }
            let mut cs = designcraft_doc::CellStyle { name: name.clone(), ..Default::default() };
            cs.fill = e.get("FillColor").map(|c| self.swatch_ref(c));
            cs.fill_tint = e.num("FillTint").and_then(|v| tint(Some(v)));
            let ins: Vec<Option<f64>> = ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().map(|k| e.num(k)).collect();
            if ins.iter().all(Option::is_some) {
                cs.insets = Some([ins[0].unwrap_or(0.0), ins[1].unwrap_or(0.0), ins[2].unwrap_or(0.0), ins[3].unwrap_or(0.0)]);
            }
            cs.vj = e.get("VerticalJustification").map(names::vj_in);
            if e.get("TopEdgeStrokeWeight").is_some() || e.get("TopEdgeStrokeColor").is_some() {
                let mut st = CellStroke::default();
                if let Some(w) = e.num("TopEdgeStrokeWeight") {
                    st.weight = w.max(0.0);
                }
                if let Some(c) = e.get("TopEdgeStrokeColor") {
                    st.color = self.swatch_ref(c);
                }
                cs.stroke = Some(st);
            }
            cs.paragraph_style = e.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)).filter(|n| n != designcraft_doc::NO_PARA_STYLE);
            match self.styles.cell.iter_mut().find(|c| c.name == name) {
                Some(slot) => *slot = cs,
                None => self.styles.cell.push(cs),
            }
        }
        for (_, e) in &table_styles {
            let name = names::style_name_in(names::TABLE_BUILTINS, e.get("Name").unwrap_or(""));
            if name == designcraft_doc::BASIC_TABLE || name == "[No table style]" {
                continue;
            }
            let mut ts = designcraft_doc::TableStyle { name: name.clone(), ..Default::default() };
            ts.header = e.get("HeaderRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            ts.body = e.get("BodyRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            ts.footer = e.get("FooterRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            ts.left_column = e.get("LeftColumnRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            ts.right_column = e.get("RightColumnRegionCellStyle").map(cell_ref).filter(|n| n != designcraft_doc::NO_CELL_STYLE);
            if e.get("TopBorderStrokeWeight").is_some() {
                let mut b = CellStroke::default();
                if let Some(w) = e.num("TopBorderStrokeWeight") {
                    b.weight = w.max(0.0);
                }
                if let Some(c) = e.get("TopBorderStrokeColor") {
                    b.color = self.swatch_ref(c);
                }
                ts.border = Some(b);
            }
            if let Some(first) = e.num("StartRowFillCount").filter(|n| *n > 0.0) {
                ts.alt_rows = Some(AltFills {
                    first: first as u32,
                    first_color: e.get("StartRowFillColor").map(|c| self.swatch_ref(c)).unwrap_or_else(|| swatch::NONE.into()),
                    first_tint: tint(e.num("StartRowFillTint")).unwrap_or(1.0),
                    next: e.num("EndRowFillCount").unwrap_or(1.0) as u32,
                    next_color: e.get("EndRowFillColor").map(|c| self.swatch_ref(c)).unwrap_or_else(|| swatch::NONE.into()),
                    next_tint: tint(e.num("EndRowFillTint")).unwrap_or(1.0),
                    skip_first: 0,
                    skip_last: 0,
                });
            }
            ts.space_before = e.num("SpaceBefore");
            ts.space_after = e.num("SpaceAfter");
            match self.styles.table.iter_mut().find(|c| c.name == name) {
                Some(slot) => *slot = ts,
                None => self.styles.table.push(ts),
            }
        }
        for (_, e) in &objects {
            let name = names::style_name_in(OBJECT_BUILTINS, e.get("Name").unwrap_or(""));
            let based_on = e.prop("BasedOn").map(|b| {
                self.object_names
                    .get(&b)
                    .cloned()
                    .unwrap_or_else(|| names::style_name_in(OBJECT_BUILTINS, &unescape_id(b.trim_start_matches("ObjectStyle/"))))
            });
            let based_on = based_on.filter(|b| *b != designcraft_doc::NO_OBJECT_STYLE && *b != name);
            let enabled = |k: &str, attr: &str| match e.get(k) {
                Some(v) => v == "true",
                None => e.get(attr).is_some(),
            };
            let fill = enabled("EnableFill", "FillColor").then(|| {
                let sw = e.get("FillColor").map(|r| self.swatch_ref(r)).unwrap_or_else(|| swatch::NONE.into());
                Fill { swatch: sw, tint: tint(e.num("FillTint")).unwrap_or(1.0), ..Fill::none() }
            });
            let stroke = enabled("EnableStroke", "StrokeColor").then(|| self.stroke_from(e, None));
            let paragraph_style = enabled("EnableParagraphStyle", "AppliedParagraphStyle")
                .then(|| e.get("AppliedParagraphStyle").map(|r| self.para_style_ref(r)))
                .flatten();
            let text_frame =
                if e.get("EnableTextFrameGeneralOptions") == Some("true") { e.find("TextFramePreference").map(text_frame_options) } else { None };
            let s = ObjectStyle { name: name.clone(), based_on, fill, stroke, paragraph_style, text_frame };
            match self.styles.object.iter_mut().find(|o| o.name == name) {
                Some(slot) => *slot = s,
                None => self.styles.object.push(s),
            }
        }
    }

    pub(super) fn based_on(&self, e: &El, para: bool) -> Option<String> {
        let b = e.prop("BasedOn")?;
        let b = b.trim();
        if para {
            if let Some(n) = self.para_names.get(b) {
                return Some(n.clone());
            }
            let n = names::style_name_in(PARA_BUILTINS, &unescape_id(b.trim_start_matches("ParagraphStyle/")));
            Some(n)
        } else {
            if let Some(n) = self.char_names.get(b) {
                return Some(n.clone()).filter(|n| n != st::NO_CHAR_STYLE);
            }
            let n = names::style_name_in(CHAR_BUILTINS, &unescape_id(b.trim_start_matches("CharacterStyle/")));
            Some(n).filter(|n| n != st::NO_CHAR_STYLE)
        }
    }

    pub(super) fn para_style_ref(&self, r: &str) -> String {
        self.para_names.get(r).cloned().unwrap_or_else(|| names::style_name_in(PARA_BUILTINS, &unescape_id(r.trim_start_matches("ParagraphStyle/"))))
    }

    pub(super) fn char_style_ref(&self, r: &str) -> String {
        self.char_names.get(r).cloned().unwrap_or_else(|| names::style_name_in(CHAR_BUILTINS, &unescape_id(r.trim_start_matches("CharacterStyle/"))))
    }
}
