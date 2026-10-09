//! Write style definitions and their folder hierarchy.

use super::{
    CHAR_BUILTINS, El, Ex, NO_PARA_ID, OBJECT_BUILTINS, PARA_BUILTINS, bool_s, document, escape_id, names, num, p, pct, pkg_root, st,
    text_frame_pref, with_props,
};

impl Ex<'_> {
    pub(super) fn styles_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let styles = d.styles.clone();
        let mut root = pkg_root("Styles");

        // Character styles.
        let mut cg = Group::default();
        for cs in &styles.character {
            let id = names::style_self("CharacterStyle", CHAR_BUILTINS, &cs.name);
            let mut el =
                El::new("CharacterStyle").attr("Self", &id).attr("Imported", "false").attr("Name", names::style_name_out(CHAR_BUILTINS, &cs.name));
            let mut props = Vec::new();
            if cs.name != st::NO_CHAR_STYLE {
                let based = cs.based_on.as_deref().filter(|b| *b != st::NO_CHAR_STYLE);
                match based {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[No character style]")),
                }
                self.char_attrs(&mut el, &mut props, &cs.chars);
            }
            cg.insert(&cs.name, with_props(el, props));
        }
        let mut rc = El::new("RootCharacterStyleGroup").attr("Self", self.fresh());
        cg.emit(&mut rc, "CharacterStyleGroup", "");
        root.push(rc);

        // Paragraph styles.
        let mut pg = Group::default();
        for ps in &styles.paragraph {
            let id = names::style_self("ParagraphStyle", PARA_BUILTINS, &ps.name);
            let mut el =
                El::new("ParagraphStyle").attr("Self", &id).attr("Name", names::style_name_out(PARA_BUILTINS, &ps.name)).attr("Imported", "false");
            let mut props = Vec::new();
            let (pa, ca) = if ps.name == designcraft_doc::NO_PARA_STYLE {
                // The root style carries the complete attribute set.
                let mut pa = designcraft_doc::ParaProps::common([&designcraft_doc::ParaProps::default()]);
                pa.merge(&ps.para);
                let mut ca = designcraft_doc::CharProps::common([&designcraft_doc::CharProps::default()]);
                ca.merge(&ps.chars);
                (pa, ca)
            } else {
                let next = ps.next_style.as_deref().unwrap_or(&ps.name);
                el.set("NextStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, next));
                match ps.based_on.as_deref().filter(|b| *b != designcraft_doc::NO_PARA_STYLE) {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("ParagraphStyle", PARA_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[No paragraph style]")),
                }
                (ps.para.clone(), ps.chars.clone())
            };
            self.para_attrs(&mut el, &mut props, &pa);
            self.char_attrs(&mut el, &mut props, &ca);
            pg.insert(&ps.name, with_props(el, props));
        }
        let mut rp = El::new("RootParagraphStyleGroup").attr("Self", self.fresh());
        pg.emit(&mut rp, "ParagraphStyleGroup", "");
        root.push(rp);

        // Cell and table styles.
        let mut cells = El::new("RootCellStyleGroup")
            .attr("Self", self.fresh())
            .child(El::new("CellStyle").attr("Self", "CellStyle/$ID/[None]").attr("AppliedParagraphStyle", NO_PARA_ID).attr("Name", "$ID/[None]"));
        for cs in styles.cell.iter().filter(|c| c.name != designcraft_doc::NO_CELL_STYLE) {
            let mut el = El::new("CellStyle")
                .attr("Self", names::style_self("CellStyle", names::CELL_BUILTINS, &cs.name))
                .attr("Name", names::style_name_out(names::CELL_BUILTINS, &cs.name));
            if let Some(f) = &cs.fill {
                el.set("FillColor", self.sw(f));
            }
            if let Some(t) = cs.fill_tint {
                el.set("FillTint", pct(t as f64));
            }
            if let Some(i) = cs.insets {
                for (k, v) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().zip(i) {
                    el.set(k, num(v));
                }
            }
            if let Some(vj) = cs.vj {
                el.set("VerticalJustification", names::vj_out(vj));
            }
            if let Some(st) = &cs.stroke {
                for side in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"] {
                    self.cell_stroke_attrs(&mut el, side, st);
                }
            }
            if let Some(ps) = &cs.paragraph_style {
                el.set("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, ps));
            }
            cells = cells.child(with_props(el, vec![p("BasedOn", "string", "$ID/[None]")]));
        }
        root.push(cells);
        let mut tables = El::new("RootTableStyleGroup")
            .attr("Self", self.fresh())
            .child(El::new("TableStyle").attr("Self", "TableStyle/$ID/[No table style]").attr("Name", "$ID/[No table style]"))
            .child(with_props(
                El::new("TableStyle").attr("Self", "TableStyle/$ID/[Basic Table]").attr("Name", "$ID/[Basic Table]"),
                vec![p("BasedOn", "string", "$ID/[No table style]")],
            ));
        for ts in styles.table.iter().filter(|t| t.name != designcraft_doc::BASIC_TABLE) {
            let mut el = El::new("TableStyle")
                .attr("Self", names::style_self("TableStyle", names::TABLE_BUILTINS, &ts.name))
                .attr("Name", names::style_name_out(names::TABLE_BUILTINS, &ts.name));
            for (k, v) in [
                ("HeaderRegionCellStyle", &ts.header),
                ("BodyRegionCellStyle", &ts.body),
                ("FooterRegionCellStyle", &ts.footer),
                ("LeftColumnRegionCellStyle", &ts.left_column),
                ("RightColumnRegionCellStyle", &ts.right_column),
            ] {
                if let Some(n) = v {
                    el.set(k, names::style_self("CellStyle", names::CELL_BUILTINS, n));
                }
            }
            if let Some(b) = &ts.border {
                for side in ["Top", "Left", "Bottom", "Right"] {
                    self.cell_stroke_attrs(&mut el, &format!("{side}Border"), b);
                }
            }
            if let Some(a) = &ts.alt_rows {
                el.set("StartRowFillColor", self.sw(&a.first_color));
                el.set("StartRowFillCount", a.first);
                el.set("StartRowFillTint", pct(a.first_tint as f64));
                el.set("EndRowFillColor", self.sw(&a.next_color));
                el.set("EndRowFillCount", a.next);
                el.set("EndRowFillTint", pct(a.next_tint as f64));
            }
            if let Some(v) = ts.space_before {
                el.set("SpaceBefore", num(v));
            }
            if let Some(v) = ts.space_after {
                el.set("SpaceAfter", num(v));
            }
            tables = tables.child(with_props(el, vec![p("BasedOn", "string", "$ID/[Basic Table]")]));
        }
        root.push(tables);
        // Named numbered lists.
        for l in &self.d.settings.lists {
            root.push(
                El::new("NumberingList")
                    .attr("Self", format!("NumberingList/{}", escape_id(&l.name)))
                    .attr("Name", &l.name)
                    .attr("ContinueNumbersAcrossStories", bool_s(l.continue_across_stories))
                    .attr("ContinueNumbersAcrossDocuments", "false"),
            );
        }

        // Object styles.
        let mut og = Group::default();
        let mut have: Vec<&str> = styles.object.iter().map(|o| o.name.as_str()).collect();
        let defaults = designcraft_doc::Styles::default();
        let mut all: Vec<designcraft_doc::ObjectStyle> = styles.object.clone();
        for b in &defaults.object {
            if !have.contains(&b.name.as_str()) {
                all.push(b.clone());
                have.push(&b.name);
            }
        }
        for os in &all {
            let id = names::style_self("ObjectStyle", OBJECT_BUILTINS, &os.name);
            let mut el = El::new("ObjectStyle").attr("Self", &id).attr("Name", names::style_name_out(OBJECT_BUILTINS, &os.name));
            let mut props = Vec::new();
            if os.name != designcraft_doc::NO_OBJECT_STYLE {
                match os.based_on.as_deref().filter(|b| *b != designcraft_doc::NO_OBJECT_STYLE) {
                    Some(b) => props.push(p("BasedOn", "object", names::style_self("ObjectStyle", OBJECT_BUILTINS, b))),
                    None => props.push(p("BasedOn", "string", "$ID/[None]")),
                }
            }
            el.set("EnableFill", bool_s(os.fill.is_some()));
            el.set("EnableStroke", bool_s(os.stroke.is_some()));
            el.set("EnableParagraphStyle", bool_s(os.paragraph_style.is_some()));
            el.set("EnableTextFrameGeneralOptions", bool_s(os.text_frame.is_some()));
            if let Some(f) = &os.fill {
                el.set("FillColor", self.sw(&f.swatch));
                el.set("FillTint", pct(f.tint as f64));
            }
            if let Some(s) = &os.stroke {
                self.stroke_attrs(&mut el, s);
            }
            if let Some(ps) = &os.paragraph_style {
                el.set("AppliedParagraphStyle", names::style_self("ParagraphStyle", PARA_BUILTINS, ps));
            }
            let mut el = with_props(el, props);
            if let Some(tf) = &os.text_frame {
                el.push(text_frame_pref(tf));
            }
            og.insert(&os.name, el);
        }
        let mut ro = El::new("RootObjectStyleGroup").attr("Self", self.fresh());
        og.emit(&mut ro, "ObjectStyleGroup", "");
        root.push(ro);
        document(&root)
    }
}

/// Style folder tree (names `A/B/C` → groups A, B).
#[derive(Default)]
struct Group {
    styles: Vec<El>,
    groups: Vec<(String, Group)>,
}

impl Group {
    fn insert(&mut self, name: &str, el: El) {
        let builtin = name.starts_with('[');
        let mut segs: Vec<&str> = if builtin { vec![name] } else { name.split('/').collect() };
        let _leaf = segs.pop();
        let mut g = self;
        for s in segs {
            let i = match g.groups.iter().position(|(n, _)| n == s) {
                Some(i) => i,
                None => {
                    g.groups.push((s.to_string(), Group::default()));
                    g.groups.len() - 1
                }
            };
            g = &mut g.groups[i].1;
        }
        g.styles.push(el);
    }

    fn emit(self, parent: &mut El, kind: &str, path: &str) {
        for s in self.styles {
            parent.push(s);
        }
        for (name, g) in self.groups {
            let full = if path.is_empty() { name.clone() } else { format!("{path}:{name}") };
            let mut ge = El::new(kind).attr("Self", format!("{kind}/{}", escape_id(&full))).attr("Name", &name);
            g.emit(&mut ge, kind, &full);
            parent.push(ge);
        }
    }
}
