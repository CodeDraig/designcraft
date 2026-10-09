//! Table, row, column, and cell appearance.

use super::{
    AltFills, CellStroke, Result, RowHeightMode, Session, StrokeType, Value, VerticalJustification, bad, color_param, edit_table, f64_or, json, ok,
    str_param, target,
};

fn parse_stroke(v: &Value, mut base: CellStroke) -> CellStroke {
    if let Some(w) = v.get("weight").and_then(Value::as_f64) {
        base.weight = w.max(0.0);
    }
    if let Some(c) = color_param(v, "color") {
        base.color = c;
    }
    if let Some(t) = v.get("tint").and_then(Value::as_f64) {
        base.tint = t.clamp(0.0, 1.0) as f32;
    }
    match str_param(v, "type") {
        Some("dashed") => base.kind = StrokeType::Dashed { pattern: vec![base.weight.max(1.0) * 3.0, base.weight.max(1.0) * 2.0] },
        Some("dotted") => base.kind = StrokeType::Dotted,
        Some("solid") => base.kind = StrokeType::Solid,
        _ => {}
    }
    base
}

pub(super) fn set_cell(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.setCell")?;
    let fill = color_param(p, "fill");
    let tint = p.get("tint").and_then(Value::as_f64).map(|t| t.clamp(0.0, 1.0) as f32);
    let insets: Option<[f64; 4]> = match p.get("insets") {
        Some(Value::Number(n)) => n.as_f64().map(|v| [v.max(0.0); 4]),
        Some(Value::Array(a)) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(0.0).max(0.0)).collect();
            Some([v[0], v[1], v[2], v[3]])
        }
        _ => None,
    };
    let vj = match str_param(p, "vj").or_else(|| str_param(p, "verticalJustification")) {
        Some("top") => Some(VerticalJustification::Top),
        Some("center") => Some(VerticalJustification::Center),
        Some("bottom") => Some(VerticalJustification::Bottom),
        Some("justify") => Some(VerticalJustification::Justify),
        Some(o) => return Err(bad("table.setCell", format!("unknown vj `{o}`"))),
        None => None,
    };
    let text = str_param(p, "text").map(str::to_string);
    let stroke = p.get("stroke").cloned();
    edit_table(s, &g, "table.setCell", |t| {
        let owners = t.owners();
        let nc = t.ncols();
        let rg = g.range;
        for r in rg.r0..=rg.r1 {
            for c in rg.c0..=rg.c1 {
                if owners[r * nc + c] != (r, c) {
                    continue;
                }
                let Some(cell) = t.cell_mut(r, c) else { continue };
                if let Some(f) = &fill {
                    cell.fill = f.clone();
                }
                if let Some(ti) = tint {
                    cell.fill_tint = ti;
                }
                if let Some(i) = insets {
                    cell.insets = i;
                }
                if let Some(v) = vj {
                    cell.vj = v;
                }
                if let Some(tx) = &text {
                    let len = cell.text.len();
                    cell.text.replace(0..len, tx);
                }
                if let Some(sv) = &stroke {
                    let edges = str_param(sv, "edges").unwrap_or("all");
                    for (i, on) in [
                        ("top", r == rg.r0),
                        ("left", c == rg.c0),
                        ("bottom", r + cell.row_span as usize - 1 == rg.r1),
                        ("right", c + cell.col_span as usize - 1 == rg.c1),
                    ]
                    .iter()
                    .enumerate()
                    .map(|(i, (name, outer))| (i, edges == "all" || edges == *name || (edges == "outer" && *outer) || (edges == "inner" && !*outer)))
                    {
                        if on {
                            cell.strokes[i] = parse_stroke(sv, cell.strokes[i].clone());
                        }
                    }
                }
            }
        }
        // Neighbouring cells share edges: mirror the edges along the range boundary.
        if let Some(sv) = &stroke {
            let edges = str_param(sv, "edges").unwrap_or("all");
            if matches!(edges, "all" | "outer" | "bottom") && rg.r1 + 1 < t.nrows() {
                for c in rg.c0..=rg.c1 {
                    let s0 = t.cell(rg.r1, c).map(|x| x.strokes[2].clone());
                    if let (Some(s0), Some(n)) = (s0, t.cell_mut(rg.r1 + 1, c)) {
                        n.strokes[0] = s0;
                    }
                }
            }
            if matches!(edges, "all" | "outer" | "right") && rg.c1 + 1 < t.ncols() {
                for r in rg.r0..=rg.r1 {
                    let s0 = t.cell(r, rg.c1).map(|x| x.strokes[3].clone());
                    if let (Some(s0), Some(n)) = (s0, t.cell_mut(r, rg.c1 + 1)) {
                        n.strokes[1] = s0;
                    }
                }
            }
        }
        ok()
    })
}

pub(super) fn set_row_height(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.setRowHeight")?;
    let h = p.get("height").and_then(Value::as_f64).map(|h| h.clamp(0.0, 10000.0));
    let mode = match str_param(p, "mode") {
        Some("exactly") => Some(RowHeightMode::Exactly),
        Some("atLeast") => Some(RowHeightMode::AtLeast),
        Some(o) => return Err(bad("table.setRowHeight", format!("unknown mode `{o}`"))),
        None => None,
    };
    if h.is_none() && mode.is_none() {
        return Err(bad("table.setRowHeight", "missing height"));
    }
    edit_table(s, &g, "table.setRowHeight", |t| {
        for row in &mut t.rows[g.range.r0..=g.range.r1] {
            if let Some(h) = h {
                row.height = h;
            }
            if let Some(m) = mode {
                row.mode = m;
            }
        }
        ok()
    })
}

pub(super) fn set_col_width(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.setColumnWidth")?;
    let w = p.get("width").and_then(Value::as_f64).ok_or_else(|| bad("table.setColumnWidth", "missing width"))?.clamp(3.0, 10000.0);
    edit_table(s, &g, "table.setColumnWidth", |t| {
        for c in &mut t.columns[g.range.c0..=g.range.c1] {
            c.width = w;
        }
        ok()
    })
}

pub(super) fn distribute_cols(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.distributeColumns")?;
    edit_table(s, &g, "table.distributeColumns", |t| {
        let cols = &mut t.columns[g.range.c0..=g.range.c1];
        let avg = cols.iter().map(|c| c.width).sum::<f64>() / cols.len() as f64;
        cols.iter_mut().for_each(|c| c.width = avg);
        ok()
    })
}

pub(super) fn parse_alt(v: &Value) -> Option<AltFills> {
    if v.is_null() {
        return None;
    }
    let d = AltFills::default();
    Some(AltFills {
        first: v.get("first").and_then(Value::as_u64).map_or(d.first, |x| x as u32),
        first_color: color_param(v, "firstColor").unwrap_or(d.first_color),
        first_tint: v.get("firstTint").and_then(Value::as_f64).map_or(d.first_tint, |x| x as f32),
        next: v.get("next").and_then(Value::as_u64).map_or(d.next, |x| x as u32),
        next_color: color_param(v, "nextColor").unwrap_or(d.next_color),
        next_tint: v.get("nextTint").and_then(Value::as_f64).map_or(d.next_tint, |x| x as f32),
        skip_first: v.get("skipFirst").and_then(Value::as_u64).map_or(0, |x| x as u32),
        skip_last: v.get("skipLast").and_then(Value::as_u64).map_or(0, |x| x as u32),
    })
}

pub(super) fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.options")?;
    let p = p.clone();
    edit_table(s, &g, "table.options", |t| {
        let o = &mut t.options;
        if let Some(v) = p.get("direction") {
            o.direction = serde_json::from_value(v.clone()).map_err(|e| bad("table.options", e.to_string()))?;
        }
        if let Some(b) = p.get("border") {
            o.border = parse_stroke(b, o.border.clone());
        }
        o.space_before = f64_or(&p, "spaceBefore", o.space_before);
        o.space_after = f64_or(&p, "spaceAfter", o.space_after);
        if let Some(b) = p.get("repeatHeader").and_then(Value::as_bool) {
            o.repeat_header = b;
        }
        if let Some(b) = p.get("repeatFooter").and_then(Value::as_bool) {
            o.repeat_footer = b;
        }
        if let Some(v) = p.get("altRows") {
            o.alt_rows = parse_alt(v);
        }
        if let Some(v) = p.get("altCols") {
            o.alt_cols = parse_alt(v);
        }
        let h = p.get("headerRows").and_then(Value::as_u64).map(|v| v as usize);
        let f = p.get("footerRows").and_then(Value::as_u64).map(|v| v as usize);
        if h.is_some() || f.is_some() {
            let (h, f) = (h.unwrap_or(t.header_rows()), f.unwrap_or(t.footer_rows()));
            // Add rows when there aren't enough for the requested header/footer plus one body row.
            let need = h + f + 1;
            if t.nrows() < need {
                let at = t.nrows();
                t.insert_rows(at, need - at);
            }
            t.set_header_footer(h, f);
        }
        Ok(json!({"options": t.options}))
    })
}

// ---------- selection ----------
