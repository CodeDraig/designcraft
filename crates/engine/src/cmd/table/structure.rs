//! Table topology edits and selection remapping.

use super::{CellAddr, Result, Selection, Session, TextSel, Value, bad, count, delete_table, edit_table, json, ok, str_param, target};

pub(super) fn insert_row(s: &mut Session, p: &Value, above: Option<bool>) -> Result<Value> {
    let g = target(s, p, "table.insertRow")?;
    let above = above.unwrap_or(str_param(p, "where") == Some("above"));
    let n = count(p);
    let at = if above { g.range.r0 } else { g.range.r1 + 1 };
    let r = edit_table(s, &g, "table.insertRow", |t| {
        t.insert_rows(at, n);
        Ok(json!({"rows": t.nrows()}))
    })?;
    shift_selection(s, g.table, at, n, true);
    Ok(r)
}

/// After inserting rows/columns at `at`, selections at or after it follow their cells.
fn shift_selection(s: &mut Session, table: u64, at: usize, n: usize, rows: bool) {
    let Ok(st) = s.doc_mut() else { return };
    if let Some(t) = st.selection.text.as_mut()
        && let Some(c) = t.cell.as_mut()
        && c.table == table
    {
        let v = if rows { &mut c.row } else { &mut c.col };
        if *v >= at {
            *v += n;
        }
    }
    if let Some(ts) = st.selection.cells.as_mut()
        && ts.table == table
    {
        let (a, b) = if rows { (&mut ts.range.r0, &mut ts.range.r1) } else { (&mut ts.range.c0, &mut ts.range.c1) };
        if *a >= at {
            *a += n;
        }
        if *b >= at {
            *b += n;
        }
    }
}

pub(super) fn insert_col(s: &mut Session, p: &Value, left: Option<bool>) -> Result<Value> {
    let g = target(s, p, "table.insertColumn")?;
    let left = left.unwrap_or(str_param(p, "where") == Some("left"));
    let rtl = s
        .doc()?
        .doc
        .story(g.story)
        .and_then(|st| st.tables.get(&g.table))
        .is_some_and(|t| t.options.direction == designcraft_doc::TextDirection::RightToLeft);
    let n = count(p);
    let at = if left != rtl { g.range.c0 } else { g.range.c1 + 1 };
    let width = p.get("width").and_then(Value::as_f64);
    let r = edit_table(s, &g, "table.insertColumn", |t| {
        let w = width.unwrap_or_else(|| t.columns[g.range.c0.min(t.ncols() - 1)].width);
        t.insert_cols(at, n, w);
        Ok(json!({"columns": t.ncols()}))
    })?;
    shift_selection(s, g.table, at, n, false);
    Ok(r)
}

pub(super) fn delete_rows(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.deleteRow")?;
    let all = s.doc()?.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).is_some_and(|t| g.range.r0 == 0 && g.range.r1 + 1 >= t.nrows());
    if all {
        return delete_table(s, p);
    }
    edit_table(s, &g, "table.deleteRow", |t| {
        t.delete_rows(g.range.r0, g.range.r1 - g.range.r0 + 1);
        Ok(json!({"rows": t.nrows()}))
    })
}

pub(super) fn delete_cols(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.deleteColumn")?;
    let all = s.doc()?.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).is_some_and(|t| g.range.c0 == 0 && g.range.c1 + 1 >= t.ncols());
    if all {
        return delete_table(s, p);
    }
    edit_table(s, &g, "table.deleteColumn", |t| {
        t.delete_cols(g.range.c0, g.range.c1 - g.range.c0 + 1);
        Ok(json!({"columns": t.ncols()}))
    })
}

pub(super) fn merge(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.merge")?;
    let r = edit_table(s, &g, "table.merge", |t| {
        t.merge(g.range).map_err(|e| bad("table.merge", e))?;
        Ok(t.expand_range(g.range))
    })?;
    // The caret goes to the merged cell.
    let st = s.doc_mut()?;
    st.selection = Selection::text(TextSel {
        story: g.story,
        anchor: 0,
        focus: 0,
        frame: st.selection.text.and_then(|t| t.frame),
        cell: Some(CellAddr { table: g.table, row: r.r0, col: r.c0 }),
    });
    ok()
}

pub(super) fn split(s: &mut Session, p: &Value, horizontal: bool) -> Result<Value> {
    let g = target(s, p, "table.split")?;
    edit_table(s, &g, "table.split", |t| {
        t.split_cell(g.range.r0, g.range.c0, horizontal);
        Ok(json!({"rows": t.nrows(), "cols": t.ncols()}))
    })
}

pub(super) fn unmerge(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.unmerge")?;
    edit_table(s, &g, "table.unmerge", |t| {
        for r in g.range.r0..=g.range.r1 {
            for c in g.range.c0..=g.range.c1 {
                t.unmerge(r, c);
            }
        }
        ok()
    })
}

pub(super) fn move_rc(s: &mut Session, p: &Value, rows: bool) -> Result<Value> {
    const ID: &str = "table.move";
    let g = target(s, p, ID)?;
    let from = p.get("from").and_then(Value::as_u64).map_or(if rows { g.range.r0 } else { g.range.c0 }, |v| v as usize);
    let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| bad(ID, "`to` required"))? as usize;
    edit_table(s, &g, ID, |t| {
        if rows { t.move_row(from, to) } else { t.move_col(from, to) }.map_err(|e| bad(ID, e))?;
        Ok(json!({"from": from, "to": to}))
    })
}

pub(super) fn sort_rows(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.sortRows")?;
    let col = p.get("column").and_then(Value::as_u64).map_or(g.range.c0, |c| c as usize);
    let desc = p.get("descending").and_then(Value::as_bool).unwrap_or(false);
    edit_table(s, &g, "table.sortRows", |t| {
        let (nr, nc) = (t.nrows(), t.ncols());
        if col >= nc {
            return Err(bad("table.sortRows", format!("no column {col}")));
        }
        if t.cells.iter().any(|c| c.row_span > 1) {
            return Err(bad("table.sortRows", "unmerge cells that span rows first"));
        }
        let (h, f) = (t.header_rows(), t.footer_rows());
        let body: Vec<usize> = (h..nr - f).collect();
        let key = |r: usize| t.cell(r, col).map(|c| c.text.text.trim().to_string()).unwrap_or_default();
        let mut order = body.clone();
        order.sort_by(|a, b| {
            let (ka, kb) = (key(*a), key(*b));
            let o = match (ka.replace(',', "").parse::<f64>(), kb.replace(',', "").parse::<f64>()) {
                (Ok(x), Ok(y)) => x.total_cmp(&y),
                _ => ka.to_lowercase().cmp(&kb.to_lowercase()),
            };
            if desc { o.reverse() } else { o }
        });
        let rows = t.rows.clone();
        let cells = t.cells.clone();
        for (k, src) in order.iter().enumerate() {
            let dst = body[k];
            t.rows[dst] = rows[*src].clone();
            for c in 0..nc {
                t.cells[dst * nc + c] = cells[src * nc + c].clone();
            }
        }
        Ok(json!({"sorted": body.len()}))
    })
}
