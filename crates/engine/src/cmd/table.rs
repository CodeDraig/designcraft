//! Table menu: create, convert, insert/delete rows and columns, merge, cell/row/column options,
//! table options and cell selection.
//!
//! Commands target the table at the text selection (a caret in a cell or next to a table anchor),
//! the selected cells, or an explicit `{story, table}`; `rows: [a, b]` / `cols: [a, b]` (or `row` /
//! `col`) narrow the cell range (default: the selected cells, else the caret's cell, else the
//! whole table).

mod creation;
use creation::{convert_from_text, convert_to_text, delete_table, insert};
mod structure;
use structure::{delete_cols, delete_rows, insert_col, insert_row, merge, move_rc, sort_rows, split, unmerge};
mod formatting;
use formatting::{distribute_cols, options, set_cell, set_col_width, set_row_height};
mod selection;
pub(crate) use selection::step_cell;
use selection::{get, select};
mod styles;
use styles::style_specs;
mod placement;
use placement::{drop_cells, place_graphic};

use designcraft_compose as compose;
use designcraft_doc::{
    AltFills, CellAddr, CellRange, CellStroke, Document, RowHeightMode, Selection, StoryId, StrokeType, Table, TableSel, TextSel,
    VerticalJustification,
};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, f64_or, has_doc, has_text, ok, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    let mut v = style_specs();
    v.extend(table_specs());
    v
}

fn table_specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "table.insert",
            "Create Table",
            [],
            Some("Cmd+Alt+Shift+T"),
            "{rows?: body rows (4), cols? (4), headerRows? (0), footerRows? (0), width?} — at the text insertion point",
            has_text,
            insert
        ),
        cmd!(
            "table.convertFromText",
            "Convert Text to Table",
            ["Table"],
            None,
            "{columnSeparator?: tab|comma} — selected paragraphs become rows",
            has_text,
            convert_from_text
        ),
        cmd!(
            "table.convertToText",
            "Convert Table to Text",
            ["Table"],
            None,
            "{} — cells separated by tabs, rows by paragraphs",
            in_table,
            convert_to_text
        ),
        cmd!("table.insertRow", "Insert Row", [], None, "{where?: above|below, count? (1)}", in_table, |s, p| insert_row(s, p, None)),
        cmd!("table.insertRowAbove", "Insert Row Above", ["Table", "Insert"], None, "{count?}", in_table, |s, p| insert_row(s, p, Some(true))),
        cmd!("table.insertRowBelow", "Insert Row Below", ["Table", "Insert"], None, "{count?}", in_table, |s, p| insert_row(s, p, Some(false))),
        cmd!("table.insertColumn", "Insert Column", [], None, "{where?: left|right, count? (1), width?}", in_table, |s, p| insert_col(s, p, None)),
        cmd!("table.insertColumnLeft", "Insert Column Left", ["Table", "Insert"], None, "{count?}", in_table, |s, p| insert_col(s, p, Some(true))),
        cmd!("table.insertColumnRight", "Insert Column Right", ["Table", "Insert"], None, "{count?}", in_table, |s, p| insert_col(s, p, Some(false))),
        cmd!("table.deleteRow", "Delete Row", ["Table", "Delete"], None, "{} — the rows of the target cells", in_table, delete_rows),
        cmd!("table.deleteColumn", "Delete Column", ["Table", "Delete"], None, "{} — the columns of the target cells", in_table, delete_cols),
        cmd!("table.delete", "Delete Table", ["Table", "Delete"], None, "{}", in_table, delete_table),
        cmd!("table.merge", "Merge Cells", ["Table"], None, "{} — merge the target cell range", in_table, merge),
        cmd!(
            "table.dropCells",
            "Drag Rows or Columns",
            [],
            None,
            "{frame, from: [x, y], to: [x, y]} — selected whole rows (or columns) dragged from `from` move to the row (column) at `to`; otherwise a text drag",
            has_doc,
            drop_cells
        ),
        cmd!(
            "table.placeGraphic",
            "Convert Cell to Graphic Cell",
            ["Table", "Convert Cell Type"],
            None,
            "{path | base64+name, fit?: proportional|fill (default proportional)} — an image in the target cell (its text is kept but hidden)",
            in_table,
            place_graphic
        ),
        cmd!(
            "table.textCell",
            "Convert Cell to Text Cell",
            ["Table", "Convert Cell Type"],
            None,
            "{} — drop the target cells' graphics",
            in_table,
            |s, p| {
                let g = target(s, p, "table.textCell")?;
                edit_table(s, &g, "table.textCell", |t| {
                    for r in g.range.r0..=g.range.r1 {
                        for c in g.range.c0..=g.range.c1 {
                            if let Some(cell) = t.cell_mut(r, c) {
                                cell.graphic = None;
                            }
                        }
                    }
                    ok()
                })
            }
        ),
        cmd!(
            "table.moveRow",
            "Move Row",
            [],
            None,
            "{from?, to} (0-based; from defaults to the target row) — like dragging a row",
            in_table,
            |s, p| move_rc(s, p, true)
        ),
        cmd!("table.moveColumn", "Move Column", [], None, "{from?, to} (0-based; from defaults to the target column)", in_table, |s, p| move_rc(
            s, p, false
        )),
        cmd!(
            "table.sortRows",
            "Sort",
            ["Table"],
            None,
            "{column?: 0-based (default: the target cell's), descending?: bool} — body rows by that column's text (numbers numerically)",
            in_table,
            sort_rows
        ),
        cmd!("table.unmerge", "Unmerge Cells", ["Table"], None, "{}", in_table, unmerge),
        cmd!(
            "table.splitHorizontally",
            "Split Cell Horizontally",
            ["Table"],
            None,
            "{} — the target cell becomes two, one above the other",
            in_table,
            |s, p| split(s, p, true)
        ),
        cmd!(
            "table.splitVertically",
            "Split Cell Vertically",
            ["Table"],
            None,
            "{} — the target cell becomes two, side by side",
            in_table,
            |s, p| split(s, p, false)
        ),
        cmd!(
            "table.setCell",
            "Cell Options",
            [],
            None,
            "{fill?: swatch, tint?, insets?: n | [t,l,b,r], vj?: top|center|bottom|justify, text?, stroke?: {weight?, color?, tint?, type?: solid|dashed|dotted, edges?: all|outer|inner|top|left|bottom|right}}",
            in_table,
            set_cell
        ),
        cmd!("table.setRowHeight", "Row Height", [], None, "{height, mode?: atLeast|exactly}", in_table, set_row_height),
        cmd!("table.setColumnWidth", "Column Width", [], None, "{width}", in_table, set_col_width),
        cmd!("table.distributeColumns", "Distribute Columns Evenly", ["Table"], None, "{}", in_table, distribute_cols),
        cmd!(
            "table.options",
            "Table Options",
            [],
            None,
            "{direction?: leftToRight|rightToLeft, border?: {weight?, color?, tint?, type?}, spaceBefore?, spaceAfter?, headerRows?, footerRows?, repeatHeader?, repeatFooter?, altRows?: {first, firstColor, firstTint, next, nextColor, nextTint, skipFirst, skipLast} | null, altCols?: … | null}",
            in_table,
            options
        ),
        cmd!(noundo "table.select", "Select Cells", [], None, "{story?, table?, rows?: [a,b], cols?: [a,b], what?: cell|row|column|table}", in_table_or_ids, select),
        cmd!(noundo "table.selectTable", "Select Table", ["Table", "Select"], None, "{}", in_table, |s, p| {
            select(s, &super::with_param(p, "what", json!("table")))
        }),
        cmd!(noundo "table.selectRow", "Select Row", ["Table", "Select"], None, "{}", in_table, |s, p| {
            select(s, &super::with_param(p, "what", json!("row")))
        }),
        cmd!(noundo "table.selectColumn", "Select Column", ["Table", "Select"], None, "{}", in_table, |s, p| {
            select(s, &super::with_param(p, "what", json!("column")))
        }),
        cmd!(noundo "table.nextCell", "Next Cell", [], None, "{}", in_cell, |s, _| step_cell(s, true)),
        cmd!(noundo "table.prevCell", "Previous Cell", [], None, "{}", in_cell, |s, _| step_cell(s, false)),
        cmd!(query "table.get", "Get Table", [], None, "{story?, table?} → rows, columns, cells (text, spans, fill), options", in_table_or_ids, get),
    ]
}

// ---------- enablement and targeting ----------

fn in_table(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    let st = s.active().ok_or("no document open")?;
    if st.selection.cells.is_some() || st.selection.text.is_some_and(|t| t.cell.is_some()) {
        return Ok(());
    }
    if let Some(t) = st.selection.text
        && st.doc.story(t.story).is_some_and(|x| x.table_at(t.focus).is_some())
    {
        return Ok(());
    }
    Err("place the insertion point in a table".into())
}

fn in_table_or_ids(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)
}

fn in_cell(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some_and(|t| t.cell.is_some())) { Ok(()) } else { Err("not in a table cell".into()) }
}

/// The table a command acts on and its target cell range.
struct Tgt {
    story: StoryId,
    table: u64,
    range: CellRange,
}

fn find_table(doc: &Document, id: u64) -> Option<StoryId> {
    doc.stories.iter().find(|(_, s)| s.tables.contains_key(&id)).map(|(k, _)| *k)
}

fn pair(p: &Value, many: &str, one: &str) -> Option<(usize, usize)> {
    if let Some(a) = p.get(many).and_then(Value::as_array) {
        let a0 = a.first()?.as_u64()? as usize;
        let a1 = a.get(1).and_then(Value::as_u64).map_or(a0, |v| v as usize);
        return Some((a0.min(a1), a0.max(a1)));
    }
    p.get(one).and_then(Value::as_u64).map(|v| (v as usize, v as usize))
}

fn target(s: &Session, p: &Value, cmd: &str) -> Result<Tgt> {
    let st = s.doc()?;
    let doc = &st.doc;
    let explicit_table = p.get("table").and_then(Value::as_u64);
    let explicit_story = p.get("story").and_then(Value::as_u64).map(StoryId);
    let (story, table, range) = if let Some(tid) = explicit_table {
        let sid = explicit_story.or_else(|| find_table(doc, tid)).ok_or_else(|| bad(cmd, format!("no table {tid}")))?;
        (sid, tid, None)
    } else if let Some(c) = st.selection.cells {
        (c.story, c.table, Some(c.range))
    } else if let Some(t) = st.selection.text {
        match t.cell {
            Some(c) => (t.story, c.table, Some(CellRange::cell(c.row, c.col))),
            None => {
                let tid = doc.story(t.story).and_then(|x| x.table_at(t.focus)).ok_or_else(|| bad(cmd, "the insertion point is not in a table"))?;
                (t.story, tid, None)
            }
        }
    } else {
        return Err(bad(cmd, "place the insertion point in a table"));
    };
    let t = doc.story(story).and_then(|x| x.tables.get(&table)).ok_or_else(|| bad(cmd, format!("no table {table}")))?;
    let (nr, nc) = (t.nrows(), t.ncols());
    let mut range = range.unwrap_or(CellRange { r0: 0, c0: 0, r1: nr - 1, c1: nc - 1 });
    if let Some((a, b)) = pair(p, "rows", "row") {
        range.r0 = a;
        range.r1 = b;
    }
    if let Some((a, b)) = pair(p, "cols", "col") {
        range.c0 = a;
        range.c1 = b;
    }
    if range.r1 >= nr || range.c1 >= nc {
        return Err(bad(cmd, format!("cell range out of bounds ({nr}×{nc} table)")));
    }
    Ok(Tgt { story, table, range })
}

/// Edit the target table; afterwards selections pointing into it are clamped to its new shape.
fn edit_table<T>(s: &mut Session, g: &Tgt, cmd: &str, f: impl FnOnce(&mut Table) -> Result<T>) -> Result<T> {
    let cmd = cmd.to_string();
    s.edit(|d, sel| {
        let st = d.story_mut(g.story).ok_or_else(|| bad(&cmd, "no story"))?;
        let t = st.table_mut(g.table).ok_or_else(|| bad(&cmd, "no table"))?;
        let r = f(t)?;
        clamp_selection(d, sel);
        Ok(r)
    })
}

/// Keep text/cell selections valid after the table changed shape (or disappeared).
fn clamp_selection(d: &Document, sel: &mut Selection) {
    if let Some(ts) = sel.cells {
        match d.story(ts.story).and_then(|x| x.tables.get(&ts.table)) {
            Some(t) => {
                let (mr, mc) = (t.nrows() - 1, t.ncols() - 1);
                let r = ts.range;
                sel.cells = Some(TableSel { range: CellRange::new(r.r0.min(mr), r.c0.min(mc), r.r1.min(mr), r.c1.min(mc)), ..ts });
            }
            None => sel.cells = None,
        }
    }
    if let Some(t) = sel.text
        && let Some(c) = t.cell
    {
        let story = d.story(t.story);
        match story.and_then(|x| x.tables.get(&c.table)) {
            Some(tb) => {
                let (r, col) = tb.owner(c.row.min(tb.nrows() - 1), c.col.min(tb.ncols() - 1));
                let len = tb.cell(r, col).map_or(0, |x| x.text.len());
                sel.text =
                    Some(TextSel { anchor: t.anchor.min(len), focus: t.focus.min(len), cell: Some(CellAddr { table: c.table, row: r, col }), ..t });
            }
            None => {
                let len = story.map_or(0, |x| x.len());
                sel.text = Some(TextSel { anchor: t.anchor.min(len), focus: t.anchor.min(len), cell: None, ..t });
            }
        }
    }
}

fn count(p: &Value) -> usize {
    p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000) as usize
}

fn color_param(p: &Value, key: &str) -> Option<String> {
    str_param(p, key).map(str::to_string)
}

// ---------- create / convert ----------

#[cfg(test)]
mod style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn cell_and_table_styles_apply_and_follow_edits() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2, "headerRows": 1})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Cell Head"})).unwrap();
        s.execute("style.cell.create", &json!({"name": "Head", "fill": "Black", "paragraphStyle": "Cell Head", "insets": 6})).unwrap();
        s.execute("style.cell.create", &json!({"name": "Body", "fill": "[Paper]"})).unwrap();
        s.execute("style.table.create", &json!({"name": "Data", "header": "Head", "body": "Body", "border": {"weight": 2, "color": "Black"}}))
            .unwrap();
        s.execute("style.table.apply", &json!({"name": "Data"})).unwrap();
        let t = |s: &Session| -> designcraft_doc::Table {
            let d = &s.doc().unwrap().doc;
            (**d.story(designcraft_doc::StoryId(sid)).unwrap().tables.values().next().unwrap()).clone()
        };
        let tb = t(&s);
        assert_eq!(tb.style, "Data");
        assert_eq!(tb.options.border.weight, 2.0);
        let head = tb.cell(0, 0).unwrap();
        assert_eq!((head.fill.as_str(), head.style.as_str(), head.insets), ("Black", "Head", [6.0; 4]));
        assert_eq!(head.text.paras[0].style, "Cell Head");
        assert_eq!(tb.cell(1, 1).unwrap().style, "Body");
        // Editing the cell style updates its cells.
        s.execute("style.cell.edit", &json!({"name": "Head", "fill": "C=100 M=0 Y=0 K=0"})).unwrap();
        assert_eq!(t(&s).cell(0, 1).unwrap().fill, "C=100 M=0 Y=0 K=0");
        // Editing the table style re-applies it.
        s.execute("style.table.edit", &json!({"name": "Data", "border": {"weight": 4}})).unwrap();
        assert_eq!(t(&s).options.border.weight, 4.0);
        assert!(s.execute("style.cell.apply", &json!({"name": "Nope"})).is_err());
    }
}

#[cfg(test)]
mod sort_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn sort_body_rows_by_a_column() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2, "headerRows": 1})).unwrap();
        let cells = [("Name", "Qty"), ("pear", "10"), ("Apple", "9"), ("fig", "100")];
        for (r, (a, b)) in cells.iter().enumerate() {
            s.execute("table.setCell", &json!({"rows": [r, r], "cols": [0, 0], "text": a})).unwrap();
            s.execute("table.setCell", &json!({"rows": [r, r], "cols": [1, 1], "text": b})).unwrap();
        }
        let col = |s: &Session, c: usize| -> Vec<String> {
            let d = &s.doc().unwrap().doc;
            let t = d.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
            (0..t.nrows()).map(|r| t.cell(r, c).unwrap().text.text.clone()).collect()
        };
        s.execute("table.sortRows", &json!({"column": 0})).unwrap();
        assert_eq!(col(&s, 0), ["Name", "Apple", "fig", "pear"], "header stays; case-insensitive");
        s.execute("table.sortRows", &json!({"column": 1, "descending": true})).unwrap();
        assert_eq!(col(&s, 1), ["Qty", "100", "10", "9"], "numbers by value");
        s.execute("table.splitVertically", &json!({"rows": [1, 1], "cols": [0, 0]})).unwrap();
        let r = s.execute("table.splitHorizontally", &json!({"rows": [0, 0], "cols": [1, 1]})).unwrap();
        assert_eq!((r["rows"].as_u64(), r["cols"].as_u64()), (Some(5), Some(3)));
    }

    #[test]
    fn graphic_cells_draw_their_image() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
        s.execute("table.rowHeight", &json!({"height": 100})).ok();
        // A red image into the first cell.
        let px: Vec<u8> = (0..40 * 40).flat_map(|_| [255, 0, 0, 255]).collect();
        let png = designcraft_render::Rendered { width: 40, height: 40, pixels: px }.to_png();
        s.execute("table.setCell", &json!({"rows": [0, 0], "cols": [0, 0], "text": ""})).ok();
        let g = s.execute("table.placeGraphic", &json!({"base64": super::super::file::base64_encode(&png), "name": "r.png", "fit": "fill"})).unwrap();
        assert_eq!((g["row"].as_u64(), g["col"].as_u64()), (Some(0), Some(0)));
        let d = s.doc().unwrap().doc.clone();
        let st = d.stories.values().find(|st| !st.tables.is_empty()).unwrap();
        let t = st.tables.values().next().unwrap();
        let cs = s.cache.get(&d, st.id, None);
        let cell = cs.frames[0].tables[0].cells.iter().find(|c| c.row == 0 && c.col == 0).unwrap().clone();
        let fx = d.item(cs.frames[0].frame).unwrap().xf;
        let centre = fx * cell.clip.center();
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let p = img.pixel(centre.x as u32, centre.y as u32);
        assert!(p[0] > 200 && p[1] < 60, "red in the graphic cell: {p:?}");
        assert!(t.cell(0, 0).unwrap().graphic.is_some());
        s.execute("table.textCell", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.stories.values().flat_map(|st| st.tables.values()).all(|t| t.cell(0, 0).unwrap().graphic.is_none()));
    }

    #[test]
    fn move_rows_and_columns() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2})).unwrap();
        for (r, c, t) in [(0, 0, "a"), (1, 0, "b"), (2, 0, "c"), (0, 1, "x")] {
            s.execute("table.setCell", &json!({"rows": [r, r], "cols": [c, c], "text": t})).unwrap();
        }
        let cell = |s: &Session, r: usize, c: usize| {
            let d = &s.doc().unwrap().doc;
            let t = d.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
            t.cell(r, c).unwrap().text.text.clone()
        };
        // Dragging a selected row onto the last row moves it there.
        let d = s.doc().unwrap().doc.clone();
        let (sid, tid) = d.stories.values().find_map(|st| st.tables.keys().next().map(|k| (st.id, *k))).unwrap();
        let fid = d.stories[&sid].frames[0];
        let cs = s.cache.get(&d, sid, None);
        let cells = &cs.frames[0].tables[0].cells;
        let centre = |r: usize| {
            let c = cells.iter().find(|c| c.row == r && c.col == 0).unwrap().rect.center();
            d.text_xf(d.item(fid).unwrap()) * c
        };
        let (from, to) = (centre(0), centre(2));
        s.execute("table.select", &json!({"story": sid.0, "table": tid, "rows": [0, 0], "what": "row"})).unwrap();
        let r2 = s.execute("table.dropCells", &json!({"frame": fid.0, "from": [from.x, from.y], "to": [to.x, to.y]})).unwrap();
        assert_eq!(r2["moved"], 1);
        assert_eq!([cell(&s, 0, 0), cell(&s, 1, 0), cell(&s, 2, 0)], ["b", "c", "a"]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!([cell(&s, 0, 0), cell(&s, 1, 0), cell(&s, 2, 0)], ["a", "b", "c"]);
        s.execute("table.moveRow", &json!({"from": 0, "to": 2})).unwrap();
        assert_eq!([cell(&s, 0, 0), cell(&s, 1, 0), cell(&s, 2, 0)], ["b", "c", "a"]);
        s.execute("table.moveColumn", &json!({"from": 1, "to": 0})).unwrap();
        assert_eq!((cell(&s, 2, 0), cell(&s, 2, 1)), ("x".to_string(), "a".to_string()));
    }
}

#[cfg(test)]
mod arabic_tests {
    use super::*;
    #[test]
    fn arabic_table_direction_and_physical_left_insertion_support_undo() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 400, 300], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
        s.execute("table.setCell", &json!({"rows": [0, 0], "cols": [0, 0], "text": "Right"})).unwrap();
        s.execute("table.options", &json!({"direction": "rightToLeft"})).unwrap();
        s.execute("table.select", &json!({"rows": [0, 0], "cols": [0, 0]})).unwrap();
        s.execute("table.insertColumnLeft", &json!({})).unwrap();
        let t = s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
        assert_eq!(t.ncols(), 3);
        assert_eq!(t.cell(0, 0).unwrap().text.text, "Right");
        assert!(t.cell(0, 1).unwrap().text.text.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        let t = s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
        assert_eq!(t.ncols(), 2);
        assert_eq!(t.options.direction, designcraft_doc::TextDirection::RightToLeft);
    }
}
