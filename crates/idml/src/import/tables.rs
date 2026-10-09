//! Map IDML table cells, spans, strokes and row geometry.

use super::{
    AltFills, Cell, CellRange, CellStroke, El, Importer, RowHeightMode, StoryId, Table, VerticalJustification, names, swatch, tint, unescape_id,
};

impl Importer<'_> {
    /// `<Table>`: rows, columns and cells (cell `Name` is `column:row`).
    pub(super) fn table(&mut self, e: &El) -> Table {
        let id = self.alloc();
        let rows: Vec<&El> = e.find_all("Row").collect();
        let cols: Vec<&El> = e.find_all("Column").collect();
        let nr = rows.len().max(1);
        let nc = cols.len().max(1);
        let mut t = Table::new(id, nr, nc, 0, 0, 0.0);
        t.options.direction = if e.get("TableDirection") == Some("RightToLeftDirection") {
            designcraft_doc::TextDirection::RightToLeft
        } else {
            designcraft_doc::TextDirection::LeftToRight
        };
        for (r, re) in rows.iter().enumerate() {
            let auto = re.boolean("AutoGrow").unwrap_or(true);
            t.rows[r].mode = if auto { RowHeightMode::AtLeast } else { RowHeightMode::Exactly };
            t.rows[r].height = if auto { re.num("MinimumHeight").unwrap_or(3.0) } else { re.num("SingleRowHeight").unwrap_or(12.0) };
        }
        for (c, ce) in cols.iter().enumerate() {
            t.columns[c].width = ce.num("SingleColumnWidth").unwrap_or(72.0).max(3.0);
        }
        let h = e.num("HeaderRowCount").unwrap_or(0.0) as usize;
        let f = e.num("FooterRowCount").unwrap_or(0.0) as usize;
        if h.saturating_add(f) < nr {
            t.set_header_footer(h, f);
        }
        let stroke = |me: &mut Self, el: &El, prefix: &str, base: &CellStroke| -> CellStroke {
            let mut s = base.clone();
            if let Some(w) = el.num(&format!("{prefix}StrokeWeight")) {
                s.weight = w.max(0.0);
            }
            if let Some(c) = el.get(&format!("{prefix}StrokeColor")) {
                s.color = me.swatch_ref(c);
            }
            if let Some(v) = el.num(&format!("{prefix}StrokeTint")) {
                s.tint = tint(Some(v)).unwrap_or(1.0);
            }
            if let Some(v) = el.get(&format!("{prefix}StrokeType")) {
                s.kind = names::stroke_type_in(v);
            }
            s
        };
        t.options.border = stroke(self, e, "TopBorder", &t.options.border);
        t.options.space_before = e.num("SpaceBefore").unwrap_or(t.options.space_before);
        t.options.space_after = e.num("SpaceAfter").unwrap_or(t.options.space_after);
        t.options.repeat_header = e.get("HeaderBehavior") != Some("RepeatOnce");
        t.options.repeat_footer = e.get("FooterBehavior") != Some("RepeatOnce");
        for kind in ["Row", "Column"] {
            let Some(first) = e.num(&format!("Start{kind}FillCount")).filter(|n| *n > 0.0) else { continue };
            let alt = AltFills {
                first: first as u32,
                first_color: e.get(&format!("Start{kind}FillColor")).map(|c| self.swatch_ref(c)).unwrap_or_else(|| swatch::NONE.into()),
                first_tint: tint(e.num(&format!("Start{kind}FillTint"))).unwrap_or(1.0),
                next: e.num(&format!("End{kind}FillCount")).unwrap_or(1.0) as u32,
                next_color: e.get(&format!("End{kind}FillColor")).map(|c| self.swatch_ref(c)).unwrap_or_else(|| swatch::NONE.into()),
                next_tint: tint(e.num(&format!("End{kind}FillTint"))).unwrap_or(1.0),
                skip_first: e.num(&format!("SkipFirstAlternatingFill{kind}s")).unwrap_or(0.0) as u32,
                skip_last: e.num(&format!("SkipLastAlternatingFill{kind}s")).unwrap_or(0.0) as u32,
            };
            if kind == "Row" {
                t.options.alt_rows = Some(alt);
            } else {
                t.options.alt_cols = Some(alt);
            }
        }
        t.style = e
            .get("AppliedTableStyle")
            .map(|r| names::style_name_in(names::TABLE_BUILTINS, &unescape_id(r.trim_start_matches("TableStyle/"))))
            .filter(|n| n != designcraft_doc::BASIC_TABLE && n != "[No table style]")
            .unwrap_or_default();
        let mut regions = Vec::new();
        for ce in e.find_all("Cell") {
            let Some((c, r)) =
                ce.get("Name").and_then(|n| n.split_once(':')).and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)))
            else {
                continue;
            };
            if r >= nr || c >= nc {
                continue;
            }
            let rs = ce.num("RowSpan").unwrap_or(1.0).max(1.0) as usize;
            let cs = ce.num("ColumnSpan").unwrap_or(1.0).max(1.0) as usize;
            let text = self.story(StoryId(0), ce);
            let mut cell = Cell { text, ..Default::default() };
            cell.style = ce
                .get("AppliedCellStyle")
                .map(|r| names::style_name_in(names::CELL_BUILTINS, &unescape_id(r.trim_start_matches("CellStyle/"))))
                .filter(|n| n != designcraft_doc::NO_CELL_STYLE)
                .unwrap_or_default();
            if let Some(fc) = ce.get("FillColor") {
                cell.fill = self.swatch_ref(fc);
            }
            cell.fill_tint = tint(ce.num("FillTint")).unwrap_or(1.0);
            for (i, k) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().enumerate() {
                if let Some(v) = ce.num(k) {
                    cell.insets[i] = v.max(0.0);
                }
            }
            cell.vj = match ce.get("VerticalJustification") {
                Some("CenterAlign") => VerticalJustification::Center,
                Some("BottomAlign") => VerticalJustification::Bottom,
                Some("JustifyAlign") => VerticalJustification::Justify,
                _ => VerticalJustification::Top,
            };
            for (i, side) in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].iter().enumerate() {
                let base = cell.strokes[i].clone();
                cell.strokes[i] = stroke(self, ce, side, &base);
                // A positive edge priority records a local cell override, including an
                // explicit None stroke. Keep it when this edge meets the table perimeter.
                cell.border_overrides[i] = ce.num(&format!("{side}StrokePriority")).is_some_and(|priority| priority > 0.0);
            }
            if let Some(slot) = t.cell_mut(r, c) {
                *slot = cell;
            }
            if rs > 1 || cs > 1 {
                regions.push(CellRange { r0: r, c0: c, r1: (r + rs - 1).min(nr - 1), c1: (c + cs - 1).min(nc - 1) });
            }
        }
        for rg in regions {
            let _ = t.merge(rg);
        }
        t
    }
}
