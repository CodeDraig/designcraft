//! Named cell and table styles.

use super::{CommandSpec, Document, Result, Session, Value, bad, cmd, color_param, edit_table, has_doc, in_table, json, ok, str_param, target};

pub(super) fn style_specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "style.cell.create",
            "New Cell Style…",
            [],
            None,
            "{name, fromSelection?: bool (the target cell's look), fill?, tint?, insets?: n|[t,l,b,r], vj?: top|center|bottom|justify, stroke?: {weight, color, tint?}, paragraphStyle?} → {name}",
            has_doc,
            |s, p| cell_style_create(s, p)
        ),
        cmd!(
            "style.cell.apply",
            "Apply Cell Style",
            [],
            None,
            "{name, story?, table?} — the target cells (the whole table when the cursor is in it)",
            in_table,
            cell_style_apply
        ),
        cmd!(
            "style.cell.edit",
            "Cell Style Options…",
            [],
            None,
            "{name, …same as style.cell.create} — cells using the style follow",
            has_doc,
            cell_style_edit
        ),
        cmd!(
            "style.table.create",
            "New Table Style…",
            [],
            None,
            "{name, header?, body?, footer?, leftColumn?, rightColumn?: cell style names, border?: {weight, color}, altRows?: {first, firstColor, next, nextColor}, spaceBefore?, spaceAfter?} → {name}",
            has_doc,
            table_style_create
        ),
        cmd!("style.table.apply", "Apply Table Style", [], None, "{name, story?, table?}", in_table, table_style_apply),
        cmd!(
            "style.table.edit",
            "Table Style Options…",
            [],
            None,
            "{name, …same as style.table.create} — tables using the style follow",
            has_doc,
            table_style_edit
        ),
    ]
}

fn stroke_param(v: Option<&Value>) -> Option<designcraft_doc::CellStroke> {
    let o = v?.as_object()?;
    let mut st = designcraft_doc::CellStroke::default();
    if let Some(w) = o.get("weight").and_then(Value::as_f64) {
        st.weight = w.max(0.0);
    }
    if let Some(c) = o.get("color").and_then(Value::as_str) {
        st.color = c.to_string();
    }
    if let Some(t) = o.get("tint").and_then(Value::as_f64) {
        st.tint = t.clamp(0.0, 1.0) as f32;
    }
    Some(st)
}

/// Fill in the cell style fields given in `p`.
fn cell_style_fields(cs: &mut designcraft_doc::CellStyle, p: &Value, cmd: &str) -> Result<()> {
    if let Some(f) = color_param(p, "fill") {
        cs.fill = Some(f);
    }
    if let Some(t) = p.get("tint").and_then(Value::as_f64) {
        cs.fill_tint = Some(t.clamp(0.0, 1.0) as f32);
    }
    match p.get("insets") {
        Some(Value::Number(n)) => cs.insets = n.as_f64().map(|v| [v.max(0.0); 4]),
        Some(Value::Array(a)) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(0.0).max(0.0)).collect();
            cs.insets = Some([v[0], v[1], v[2], v[3]]);
        }
        _ => {}
    }
    if let Some(v) = p.get("vj") {
        cs.vj = Some(serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("vj: {e}")))?);
    }
    if let Some(st) = stroke_param(p.get("stroke")) {
        cs.stroke = Some(st);
    }
    if let Some(ps) = str_param(p, "paragraphStyle") {
        cs.paragraph_style = Some(ps.to_string());
    }
    Ok(())
}

fn cell_style_create(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Cell Style 1").to_string();
    let mut cs = designcraft_doc::CellStyle::default();
    if p.get("fromSelection").and_then(Value::as_bool).unwrap_or(false) {
        let g = target(s, p, "style.cell.create")?;
        let d = &s.doc()?.doc;
        let cell = d
            .story(g.story)
            .and_then(|st| st.tables.get(&g.table))
            .and_then(|t| t.cell(g.range.r0, g.range.c0))
            .ok_or_else(|| bad("style.cell.create", "no cell"))?;
        cs.fill = Some(cell.fill.clone());
        cs.fill_tint = Some(cell.fill_tint);
        cs.insets = Some(cell.insets);
        cs.vj = Some(cell.vj);
        cs.stroke = Some(cell.strokes[0].clone());
        cs.paragraph_style = cell.text.paras.first().map(|p| p.style.clone());
    }
    cell_style_fields(&mut cs, p, "style.cell.create")?;
    s.edit(|d, _| {
        let name = designcraft_doc::Styles::unique_name(|n| d.styles.cell.iter().any(|c| c.name == n), &base);
        cs.name = name.clone();
        d.styles_mut().cell.push(cs);
        Ok(json!({"name": name}))
    })
}

fn cell_style_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.cell.apply", "missing name"))?.to_string();
    let cs = s
        .doc()?
        .doc
        .styles
        .cell
        .iter()
        .find(|c| c.name == name)
        .cloned()
        .ok_or_else(|| bad("style.cell.apply", format!("no cell style `{name}`")))?;
    let g = target(s, p, "style.cell.apply")?;
    edit_table(s, &g, "style.cell.apply", |t| {
        let owners = t.owners();
        let nc = t.ncols();
        let mut n = 0;
        for r in g.range.r0..=g.range.r1 {
            for c in g.range.c0..=g.range.c1 {
                if owners[r * nc + c] == (r, c)
                    && let Some(cell) = t.cell_mut(r, c)
                {
                    cs.apply_to(cell);
                    n += 1;
                }
            }
        }
        Ok(json!({"cells": n}))
    })
}

/// Re-apply cell styles named in `names` to every cell using them (after a style edit).
fn reapply_cell_styles(d: &mut Document, names: &[String]) {
    let styles = d.styles.cell.clone();
    for sid in d.stories.keys().copied().collect::<Vec<_>>() {
        let Some(st) = d.story_mut(sid) else { continue };
        for t in st.tables.values_mut() {
            if !t.cells.iter().any(|c| names.contains(&c.style)) {
                continue;
            }
            for cell in &mut std::sync::Arc::make_mut(t).cells {
                if names.contains(&cell.style)
                    && let Some(cs) = styles.iter().find(|x| x.name == cell.style)
                {
                    cs.apply_to(cell);
                }
            }
        }
        st.rev += 1;
    }
}

fn cell_style_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.cell.edit", "missing name"))?.to_string();
    let p = p.clone();
    s.edit(|d, _| {
        let cs = d.styles_mut().cell.iter_mut().find(|c| c.name == name).ok_or_else(|| bad("style.cell.edit", format!("no cell style `{name}`")))?;
        cell_style_fields(cs, &p, "style.cell.edit")?;
        reapply_cell_styles(d, std::slice::from_ref(&name));
        ok()
    })
}

fn table_style_fields(ts: &mut designcraft_doc::TableStyle, p: &Value) {
    for (k, f) in [
        ("header", &mut ts.header),
        ("body", &mut ts.body),
        ("footer", &mut ts.footer),
        ("leftColumn", &mut ts.left_column),
        ("rightColumn", &mut ts.right_column),
    ] {
        if let Some(v) = p.get(k) {
            *f = v.as_str().map(str::to_string);
        }
    }
    if let Some(b) = stroke_param(p.get("border")) {
        ts.border = Some(b);
    }
    if let Some(a) = p.get("altRows").and_then(Value::as_object) {
        let n = |k: &str, d: u32| a.get(k).and_then(Value::as_u64).map_or(d, |v| v as u32);
        let c = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or(designcraft_color::swatch::NONE).to_string();
        let t = |k: &str| a.get(k).and_then(Value::as_f64).map_or(1.0, |v| v.clamp(0.0, 1.0) as f32);
        ts.alt_rows = Some(designcraft_doc::AltFills {
            first: n("first", 1),
            first_color: c("firstColor"),
            first_tint: t("firstTint"),
            next: n("next", 1),
            next_color: c("nextColor"),
            next_tint: t("nextTint"),
            skip_first: n("skipFirst", 0),
            skip_last: n("skipLast", 0),
        });
    }
    if let Some(v) = p.get("spaceBefore").and_then(Value::as_f64) {
        ts.space_before = Some(v);
    }
    if let Some(v) = p.get("spaceAfter").and_then(Value::as_f64) {
        ts.space_after = Some(v);
    }
}

fn table_style_create(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Table Style 1").to_string();
    let mut ts = designcraft_doc::TableStyle::default();
    table_style_fields(&mut ts, p);
    s.edit(|d, _| {
        let name = designcraft_doc::Styles::unique_name(|n| d.styles.table.iter().any(|c| c.name == n), &base);
        ts.name = name.clone();
        d.styles_mut().table.push(ts);
        Ok(json!({"name": name}))
    })
}

fn table_style_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.table.apply", "missing name"))?.to_string();
    let doc = &s.doc()?.doc;
    let ts = doc.styles.table.iter().find(|c| c.name == name).cloned().ok_or_else(|| bad("style.table.apply", format!("no table style `{name}`")))?;
    let cells = doc.styles.cell.clone();
    let g = target(s, p, "style.table.apply")?;
    edit_table(s, &g, "style.table.apply", |t| {
        ts.apply_to(t, &cells);
        ok()
    })
}

fn table_style_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.table.edit", "missing name"))?.to_string();
    let p = p.clone();
    s.edit(|d, _| {
        let ts =
            d.styles_mut().table.iter_mut().find(|c| c.name == name).ok_or_else(|| bad("style.table.edit", format!("no table style `{name}`")))?;
        table_style_fields(ts, &p);
        let (ts, cells) = (ts.clone(), d.styles.cell.clone());
        for sid in d.stories.keys().copied().collect::<Vec<_>>() {
            let Some(st) = d.story_mut(sid) else { continue };
            for t in st.tables.values_mut() {
                if t.style == name {
                    ts.apply_to(std::sync::Arc::make_mut(t), &cells);
                }
            }
            st.rev += 1;
        }
        ok()
    })
}
