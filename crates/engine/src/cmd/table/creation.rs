//! Table insertion, text conversion, and table removal.

use super::{CellAddr, Result, Selection, Session, Table, TextSel, Value, bad, compose, json, ok, str_param, target};

/// Width of the text column at the caret (the new table spans it).
fn column_width(s: &Session, t: &TextSel) -> f64 {
    let Some(st) = s.active() else { return 300.0 };
    let specs = compose::frame_specs(&st.doc, t.story);
    let spec = t.frame.and_then(|f| specs.iter().find(|x| x.id == f)).or(specs.first());
    spec.and_then(|f| f.columns().first().map(|c| c.width())).unwrap_or(300.0)
}

pub(super) fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let t = s.doc()?.selection.text.ok_or_else(|| bad("table.insert", "no insertion point"))?;
    if t.cell.is_some() {
        return Err(bad("table.insert", "tables can't be nested in cells"));
    }
    let rows = p.get("rows").and_then(Value::as_u64).unwrap_or(4).clamp(1, 500) as usize;
    let cols = p.get("cols").or_else(|| p.get("columns")).and_then(Value::as_u64).unwrap_or(4).clamp(1, 200) as usize;
    let header = p.get("headerRows").and_then(Value::as_u64).unwrap_or(0).min(100) as usize;
    let footer = p.get("footerRows").and_then(Value::as_u64).unwrap_or(0).min(100) as usize;
    let width = p.get("width").and_then(Value::as_f64).unwrap_or_else(|| column_width(s, &t));
    s.edit(|d, sel| {
        let id = d.alloc();
        let mut table = Table::new(id, rows, cols, header, footer, width);
        let st = d.story_mut(t.story).ok_or_else(|| bad("table.insert", "no story"))?;
        // Cells start with the paragraph format at the caret.
        let pf = st.paras[st.para_at(t.range().start)].clone();
        let cf = st.char_format_at(t.range().start).clone();
        for c in &mut table.cells {
            c.text.paras[0] = designcraft_doc::ParaFormat { table: None, ..pf.clone() };
            c.text.chars[0].format = cf.clone();
        }
        let r = t.range();
        st.delete(r.clone());
        st.insert_table(r.start, table);
        *sel = Selection::text(TextSel { story: t.story, anchor: 0, focus: 0, frame: t.frame, cell: Some(CellAddr { table: id, row: 0, col: 0 }) });
        Ok(json!({"story": t.story.0, "table": id}))
    })
}

pub(super) fn convert_from_text(s: &mut Session, p: &Value) -> Result<Value> {
    let t = s.doc()?.selection.text.ok_or_else(|| bad("table.convertFromText", "no text selected"))?;
    if t.cell.is_some() {
        return Err(bad("table.convertFromText", "select text outside tables"));
    }
    let sep = if str_param(p, "columnSeparator") == Some("comma") { ',' } else { '\t' };
    let width = p.get("width").and_then(Value::as_f64).unwrap_or_else(|| column_width(s, &t));
    s.edit(|d, sel| {
        let id = d.alloc();
        let st = d.story_mut(t.story).ok_or_else(|| bad("table.convertFromText", "no story"))?;
        let ranges = st.para_ranges();
        let (pa, pb) = (st.para_at(t.range().start), st.para_at(t.range().end));
        if (pa..=pb).any(|i| st.paras[i].table.is_some()) {
            return Err(bad("table.convertFromText", "the selection contains a table"));
        }
        let (a, b) = (ranges[pa].start, ranges[pb].end);
        let data: Vec<Vec<String>> = st.text[a..b].split('\n').map(|line| line.split(sep).map(|c| c.trim().to_string()).collect()).collect();
        let pf = st.paras[pa].clone();
        let cf = st.format_after(a).clone();
        let table = Table::from_strings(id, &data, width, &pf, &cf);
        st.delete(a..b);
        let anchor = st.insert_table(a, table);
        *sel = Selection::text(TextSel::caret(t.story, anchor));
        Ok(json!({"table": id, "rows": data.len()}))
    })
}

pub(super) fn convert_to_text(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.convertToText")?;
    s.edit(|d, sel| {
        let st = d.story_mut(g.story).ok_or_else(|| bad("table.convertToText", "no story"))?;
        let text = st.tables.get(&g.table).map(|t| t.plain_text()).unwrap_or_default();
        let a = st.table_anchor(g.table).ok_or_else(|| bad("table.convertToText", "no anchor"))?;
        st.replace(a..a + designcraft_doc::TABLE_ANCHOR.len_utf8(), &text);
        *sel = Selection::text(TextSel::caret(g.story, a + text.len()));
        ok()
    })
}

pub(super) fn delete_table(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.delete")?;
    s.edit(|d, sel| {
        let st = d.story_mut(g.story).ok_or_else(|| bad("table.delete", "no story"))?;
        let a = st.table_anchor(g.table).ok_or_else(|| bad("table.delete", "no anchor"))?;
        // Remove the anchor paragraph (with one separator) so no empty paragraph is left behind.
        let end = a + designcraft_doc::TABLE_ANCHOR.len_utf8();
        let r = if &st.text[end..] == "\n" && a > 0 && st.text[..a].ends_with('\n') {
            // Inserted at the end of the story: remove the empty paragraph after it too.
            a - 1..end + 1
        } else if st.text[end..].starts_with('\n') {
            a..end + 1
        } else if a > 0 && st.text[..a].ends_with('\n') {
            a - 1..end
        } else {
            a..end
        };
        st.delete(r.clone());
        *sel = Selection::text(TextSel::caret(g.story, r.start.min(st.len())));
        ok()
    })
}

// ---------- rows and columns ----------
