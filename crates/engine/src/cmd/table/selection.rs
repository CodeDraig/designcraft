//! Table selection and cell navigation.

use super::{CellAddr, CellRange, Result, Selection, Session, TableSel, TextSel, Tgt, Value, bad, edit_table, json, ok, str_param, target};

pub(super) fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.select")?;
    let (nr, nc) = {
        let st = s.doc()?;
        let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or_else(|| bad("table.select", "no table"))?;
        (t.nrows(), t.ncols())
    };
    let mut r = g.range;
    match str_param(p, "what") {
        Some("table") => r = CellRange { r0: 0, c0: 0, r1: nr - 1, c1: nc - 1 },
        Some("row") => {
            r.c0 = 0;
            r.c1 = nc - 1;
        }
        Some("column") => {
            r.r0 = 0;
            r.r1 = nr - 1;
        }
        _ => {}
    }
    let st = s.doc_mut()?;
    let frame = st.selection.text.and_then(|t| t.frame);
    st.selection = Selection {
        cells: Some(TableSel { story: g.story, table: g.table, range: r }),
        text: Some(TextSel { story: g.story, anchor: 0, focus: 0, frame, cell: Some(CellAddr { table: g.table, row: r.r0, col: r.c0 }) }),
        ..Default::default()
    };
    st.revision += 1;
    Ok(json!({"story": g.story.0, "table": g.table, "range": r}))
}

/// Move the caret to the next/previous cell (Tab / Shift-Tab), selecting its text. Tab in the last
/// cell adds a row.
pub(crate) fn step_cell(s: &mut Session, forward: bool) -> Result<Value> {
    let t = s.doc()?.selection.text.ok_or_else(|| bad("table.nextCell", "no caret"))?;
    let c = t.cell.ok_or_else(|| bad("table.nextCell", "not in a cell"))?;
    let (owners, nr, nc) = {
        let st = s.doc()?;
        let tb = st.doc.story(t.story).and_then(|x| x.tables.get(&c.table)).ok_or_else(|| bad("table.nextCell", "no table"))?;
        (tb.owners(), tb.nrows(), tb.ncols())
    };
    let cur = c.row * nc + c.col;
    let next =
        if forward { (cur + 1..nr * nc).find(|&i| owners[i] == (i / nc, i % nc)) } else { (0..cur).rev().find(|&i| owners[i] == (i / nc, i % nc)) };
    let (row, col) = match next {
        Some(i) => (i / nc, i % nc),
        None if forward => {
            let g = Tgt { story: t.story, table: c.table, range: CellRange::cell(nr - 1, 0) };
            edit_table(s, &g, "table.nextCell", |tb| {
                tb.insert_rows(nr, 1);
                ok()
            })?;
            (nr, 0)
        }
        None => return ok(),
    };
    let st = s.doc_mut()?;
    let len = st.doc.text_story(t.story, Some(CellAddr { table: c.table, row, col })).map_or(0, |x| x.len());
    st.selection = Selection::text(TextSel { anchor: 0, focus: len, cell: Some(CellAddr { table: c.table, row, col }), ..t });
    st.revision += 1;
    Ok(json!({"row": row, "col": col}))
}

pub(super) fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.get")?;
    let st = s.doc()?;
    let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or_else(|| bad("table.get", "no table"))?;
    let owners = t.owners();
    let cells: Vec<Value> = (0..t.nrows())
        .flat_map(|r| (0..t.ncols()).map(move |c| (r, c)))
        .filter(|&(r, c)| owners[r * t.ncols() + c] == (r, c))
        .filter_map(|(r, c)| {
            let cell = t.cell(r, c)?;
            Some(json!({"row": r, "col": c, "rowSpan": cell.row_span, "colSpan": cell.col_span, "text": cell.text.text, "fill": cell.fill,
                "insets": cell.insets, "vj": cell.vj, "style": cell.style}))
        })
        .collect();
    Ok(json!({
        "story": g.story.0, "table": g.table, "anchor": st.doc.story(g.story).and_then(|x| x.table_anchor(g.table)),
        "rows": t.rows, "columns": t.columns, "headerRows": t.header_rows(), "footerRows": t.footer_rows(),
        "cells": cells, "options": t.options, "range": g.range, "style": t.style,
    }))
}

// ---------- cell and table styles ----------
