//! Table drag-and-drop and graphic cell placement.

use super::{CellRange, Result, Session, StoryId, TableSel, Value, bad, clamp_selection, json, target};

/// The table cell under spread point `pt` in text frame `frame`: (story, table, row, col).
fn cell_at(s: &Session, frame: designcraft_doc::ItemId, pt: designcraft_geom::Point) -> Option<(StoryId, u64, usize, usize)> {
    let st = s.active()?;
    let loc = st.doc.find(frame)?;
    let it = st.doc.item_at(&loc)?;
    let sid = it.text_frame()?.story;
    let inner = (st.doc.parent_xf(&loc) * st.doc.text_xf(it)).inverse() * pt;
    let cs = s.cache.get(&st.doc, sid, None);
    let fi = cs.frames.iter().position(|f| f.frame == frame)?;
    let (table, row, col, _) = designcraft_compose::hit_cell(&cs, fi, inner)?;
    Some((sid, table, row, col))
}

pub(super) fn drop_cells(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.dropCells";
    let frame = super::super::id_param(p, "frame").ok_or_else(|| bad(ID, "`frame` required"))?;
    let pt = |k: &str| -> Result<designcraft_geom::Point> {
        let a = p.get(k).and_then(Value::as_array).ok_or_else(|| bad(ID, format!("`{k}` required")))?;
        Ok(designcraft_geom::Point::new(a.first().and_then(Value::as_f64).unwrap_or(0.0), a.get(1).and_then(Value::as_f64).unwrap_or(0.0)))
    };
    let (from, to) = (pt("from")?, pt("to")?);
    let sel = s.doc()?.selection.cells;
    let hit_from = cell_at(s, frame, from);
    let hit_to = cell_at(s, frame, to);
    if let (Some(ts), Some((sid, tid, fr, fc)), Some((_, tid2, tr, tc))) = (sel, hit_from, hit_to)
        && ts.story == sid
        && ts.table == tid
        && tid2 == tid
        && ts.range.contains(fr, fc)
    {
        let (nr, nc) = {
            let t = s.doc()?.doc.story(sid).and_then(|st| st.tables.get(&tid)).ok_or_else(|| bad(ID, "no table"))?;
            (t.nrows(), t.ncols())
        };
        let r = ts.range;
        let rows = r.c0 == 0 && r.c1 + 1 == nc;
        let cols = r.r0 == 0 && r.r1 + 1 == nr;
        if rows || cols {
            let (lo, hi, target) = if rows { (r.r0, r.r1, tr) } else { (r.c0, r.c1, tc) };
            if target >= lo && target <= hi {
                return Ok(json!({"moved": 0}));
            }
            let n = hi - lo + 1;
            let r2 = s.edit(|d, selm| {
                let t = d.story_mut(sid).and_then(|st| st.table_mut(tid)).ok_or_else(|| bad(ID, "no table"))?;
                let mv = |t: &mut designcraft_doc::Table, a: usize, b: usize| if rows { t.move_row(a, b) } else { t.move_col(a, b) };
                let new_lo = if target > hi {
                    for _ in 0..n {
                        mv(t, lo, target).map_err(|e| bad(ID, e))?;
                    }
                    target + 1 - n
                } else {
                    for k in 0..n {
                        mv(t, lo + k, target + k).map_err(|e| bad(ID, e))?;
                    }
                    target
                };
                let range =
                    if rows { CellRange::new(new_lo, r.c0, new_lo + n - 1, r.c1) } else { CellRange::new(r.r0, new_lo, r.r1, new_lo + n - 1) };
                selm.cells = Some(TableSel { range, ..ts });
                Ok(json!({"moved": n, "to": new_lo}))
            })?;
            return Ok(r2);
        }
    }
    // Not a row/column drag: select text from the press to the release.
    s.execute("text.placeCaret", &json!({"frame": frame.0, "point": [from.x, from.y]}))?;
    s.execute("text.extendTo", &json!({"frame": frame.0, "point": [to.x, to.y]}))
}

pub(super) fn place_graphic(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.placeGraphic";
    let g = target(s, p, ID)?;
    let (bytes, name, link) = super::super::file::read_source(p)?;
    let (pw, ph) = designcraft_render::image_size(&bytes).ok_or_else(|| bad(ID, "unsupported or corrupt image"))?;
    let fill = p.get("fit").and_then(Value::as_str) == Some("fill");
    s.edit(|d, sel| {
        let aid = designcraft_doc::AssetId(d.alloc());
        let mime = designcraft_render::image_mime(&bytes).to_string();
        d.assets.insert(
            aid,
            std::sync::Arc::new(designcraft_doc::Asset {
                page: 0,
                id: aid,
                name,
                mime,
                link,
                data: std::sync::Arc::new(bytes),
                pixels: Some((pw, ph)),
            }),
        );
        let st = d.story_mut(g.story).ok_or_else(|| bad(ID, "no story"))?;
        let t = st.table_mut(g.table).ok_or_else(|| bad(ID, "no table"))?;
        let (r, c) = t.owner(g.range.r0, g.range.c0);
        let cell = t.cell(r, c).ok_or_else(|| bad(ID, "no cell"))?;
        let (rs, cs) = (cell.row_span.max(1) as usize, cell.col_span.max(1) as usize);
        let w: f64 = t.columns[c..(c + cs).min(t.ncols())].iter().map(|x| x.width).sum();
        let h: f64 = t.rows[r..(r + rs).min(t.nrows())].iter().map(|x| x.height).sum();
        let ins = cell.insets;
        let (bw, bh) = ((w - ins[1] - ins[3]).max(1.0), (h - ins[0] - ins[2]).max(1.0));
        let (nw, nh) = (pw as f64, ph as f64);
        let k = if fill { (bw / nw).max(bh / nh) } else { (bw / nw).min(bh / nh) };
        let xf = designcraft_geom::Affine::translate(((bw - nw * k) / 2.0, (bh - nh * k) / 2.0)) * designcraft_geom::Affine::scale(k);
        let cell = t.cell_mut(r, c).ok_or_else(|| bad(ID, format!("no cell {r},{c}")))?;
        cell.graphic = Some(designcraft_doc::Graphic {
            asset: aid,
            size: (nw, nh),
            xf,
            auto_fit: if fill { designcraft_doc::Fitting::FillProportionally } else { designcraft_doc::Fitting::FitProportionally },
            fit_align: 4,
            crop: [0.0; 4],
        });
        clamp_selection(d, sel);
        Ok(json!({"asset": aid.0, "row": r, "col": c}))
    })
}
