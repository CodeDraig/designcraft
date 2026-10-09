//! Pages command implementations.

use super::{Margins, Result, Value, adjust_layout, bad, document_setup, json, margin_boxes, ok, parent_ref, str_param};
use crate::Session;

pub(super) fn layout_pages_insert(s: &mut Session, p: &Value) -> Result<Value> {
    let count = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 9999) as usize;
    let st = s.doc()?;
    let n = st.doc.page_count();
    let after = p.get("after").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(n - 1);
    let parent = parent_ref(&st.doc, p.get("parent")).unwrap_or_else(|| st.doc.parents.first().map(|x| x.id));
    s.edit(|d, _| {
        let ids = d.insert_pages(Some(after), count, parent)?;
        Ok(json!({"pages": ids.len(), "total": d.page_count()}))
    })
}

pub(super) fn layout_rotate_spread_view(s: &mut Session, p: &Value) -> Result<Value> {
    let si = p.get("spread").and_then(Value::as_u64).unwrap_or(0) as usize;
    let angle = p.get("angle").and_then(Value::as_i64).unwrap_or(90);
    s.edit(|d, _| {
        let sp = d.spreads.get_mut(si).ok_or_else(|| bad("layout.rotateSpreadView", "no such spread"))?;
        let pg = std::sync::Arc::make_mut(sp).pages.first_mut().ok_or_else(|| bad("layout.rotateSpreadView", "empty spread"))?;
        pg.view_rotation = if angle == 0 { 0 } else { ((pg.view_rotation as i64 + angle.div_euclid(90)).rem_euclid(4)) as u8 };
        Ok(json!({"rotation": pg.view_rotation as u32 * 90}))
    })
}

pub(super) fn layout_pages_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let pages: Vec<usize> =
        p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect()).unwrap_or_default();
    s.edit(|d, sel| {
        d.delete_pages(&pages)?;
        sel.items.retain(|i| d.item(*i).is_some());
        sel.text = sel.text.filter(|t| d.story(t.story).is_some());
        ok()
    })
}

pub(super) fn layout_pages_duplicate_spread(s: &mut Session, p: &Value) -> Result<Value> {
    let si = p.get("spread").and_then(Value::as_u64).unwrap_or(0) as usize;
    s.edit(|d, _| {
        let sp = d.spreads.get(si).cloned().ok_or_else(|| bad("layout.pages.duplicateSpread", "no such spread"))?;
        let after = d.first_page_of_spread(si) + sp.pages.len() - 1;
        let parent = sp.pages.first().and_then(|p| p.parent);
        d.insert_pages(Some(after), sp.pages.len(), parent)?;
        // Copy items onto the new spread(s) at the same positions.
        let src = d.clone();
        let ids: Vec<_> = sp.items.iter().map(|i| i.id).collect();
        let (nsi, _) = d.page_loc(after + 1).ok_or_else(|| bad("layout.pages.duplicateSpread", "insert failed"))?;
        super::super::object::duplicate_from(d, &src, &ids, designcraft_doc::SpreadRef::Doc(nsi), designcraft_geom::Vec2::ZERO)?;
        ok()
    })
}

pub(super) fn layout_pages_apply_parent(s: &mut Session, p: &Value) -> Result<Value> {
    let pages: Vec<usize> =
        p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect()).unwrap_or_default();
    let parent = parent_ref(&s.doc()?.doc, p.get("parent")).unwrap_or(None);
    s.edit(|d, _| {
        d.apply_parent(&pages, parent)?;
        ok()
    })
}

pub(super) fn layout_parents_new(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("Parent").to_string();
    s.edit(|d, _| {
        let prefix = str_param(p, "prefix").map(str::to_string).unwrap_or_else(|| d.next_parent_prefix());
        let m = d.page(0).map(|p| (p.columns.count, p.columns.gutter, p.margins)).unwrap_or((1, 12.0, Margins::uniform(36.0)));
        let id = d.add_parent(&prefix, &name, m.0, m.1, m.2);
        Ok(json!({"id": id.0, "prefix": prefix}))
    })
}

pub(super) fn layout_margins_and_columns(s: &mut Session, p: &Value) -> Result<Value> {
    let pages: Option<Vec<usize>> =
        p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect());
    let p = p.clone();
    let adjust = p.get("adjustLayout").and_then(Value::as_bool).unwrap_or(false);
    s.edit(|d, _| {
        let before = adjust.then(|| margin_boxes(d));
        let all: Vec<usize> = pages.clone().unwrap_or_else(|| (0..d.page_count()).collect());
        for abs in all {
            let Some((si, pi)) = d.page_loc(abs) else { continue };
            let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[si]).pages[pi];
            match p.get("margins") {
                Some(Value::Number(n)) => pg.margins = Margins::uniform(n.as_f64().unwrap_or(36.0)),
                Some(v @ Value::Object(_)) => {
                    if let Ok(m) = serde_json::from_value(v.clone()) {
                        pg.margins = m;
                    }
                }
                _ => {}
            }
            if let Some(c) = p.get("columns").and_then(Value::as_u64) {
                pg.columns.count = (c as u32).clamp(1, 216);
            }
            if let Some(g) = p.get("gutter").and_then(Value::as_f64) {
                pg.columns.gutter = g.max(0.0);
            }
        }
        if let Some(b) = before {
            adjust_layout(d, &b);
        }
        ok()
    })
}

pub(super) fn layout_document_setup(s: &mut Session, p: &Value) -> Result<Value> {
    let p = p.clone();
    if p.as_object().is_none_or(|o| o.is_empty()) {
        return Ok(document_setup(&s.doc()?.doc));
    }
    let edges = |k: &str| -> Option<[f64; 4]> {
        match p.get(k)? {
            Value::Array(a) if a.len() == 4 => {
                let v: Vec<f64> = a.iter().filter_map(Value::as_f64).map(|v| v.max(0.0)).collect();
                (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
            }
            v => v.as_f64().map(|b| [b.max(0.0); 4]),
        }
    };
    let (bleed, slug) = (edges("bleed"), edges("slug"));
    let intent: Option<designcraft_doc::Intent> = p.get("intent").and_then(|v| serde_json::from_value(v.clone()).ok());
    let pages = p.get("pages").and_then(Value::as_u64).map(|n| n.clamp(1, 9999) as usize);
    let adjust = p.get("adjustLayout").and_then(Value::as_bool).unwrap_or(false);
    s.edit(|d, _| {
        let before = adjust.then(|| margin_boxes(d));
        if let Some(f) = p.get("facingPages").and_then(Value::as_bool) {
            d.settings.facing_pages = f;
        }
        if let Some(b) = p.get("binding").and_then(Value::as_str) {
            d.settings.right_to_left_binding = b == "rightToLeft";
        }
        if let Some(b) = bleed {
            d.settings.bleed = b;
        }
        if let Some(b) = slug {
            d.settings.slug = b;
        }
        if let Some(i) = intent {
            d.settings.intent = i;
        }
        if let Some(n) = p.get("startPage").and_then(Value::as_u64) {
            if !d.sections.iter().any(|x| x.start == 0) {
                d.sections.insert(
                    0,
                    designcraft_doc::Section {
                        start: 0,
                        start_number: None,
                        style: Default::default(),
                        prefix: String::new(),
                        marker: String::new(),
                        include_prefix: false,
                    },
                );
            }
            if let Some(first) = d.sections.iter_mut().find(|x| x.start == 0) {
                first.start_number = Some(n.max(1) as u32);
            }
        }
        // Number of Pages: add at the end (with the last page's parent) or remove from the end.
        if let Some(n) = pages {
            let have = d.page_count();
            if n > have {
                let parent = d.page_loc(have - 1).and_then(|(si, pi)| d.spreads[si].pages[pi].parent);
                d.insert_pages(Some(have - 1), n - have, parent)?;
            } else if n < have {
                d.delete_pages(&(n..have).collect::<Vec<_>>())?;
            }
        }
        let w = p.get("width").and_then(Value::as_f64);
        let h = p.get("height").and_then(Value::as_f64);
        // Pages with a liquid rule re-flow their objects (Adjust Layout aside).
        let liquid_before: Vec<(usize, usize, designcraft_geom::Rect)> = if adjust {
            vec![]
        } else {
            d.spreads
                .iter()
                .enumerate()
                .flat_map(|(si, sp)| sp.pages.iter().enumerate().filter(|(_, pg)| !pg.liquid.is_off()).map(move |(pi, pg)| (si, pi, pg.bounds())))
                .collect()
        };
        if w.is_some() || h.is_some() {
            d.settings.page_width = w.unwrap_or(d.settings.page_width);
            d.settings.page_height = h.unwrap_or(d.settings.page_height);
            for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
                let sp = std::sync::Arc::make_mut(sp);
                for pg in &mut sp.pages {
                    pg.width = d.settings.page_width;
                    pg.height = d.settings.page_height;
                }
                sp.relayout();
            }
        }
        d.repaginate();
        if let Some(b) = before {
            adjust_layout(d, &b);
        }
        for (si, pi, old) in liquid_before {
            super::super::liquid::apply(d, designcraft_doc::SpreadRef::Doc(si), pi, old);
        }
        Ok(document_setup(d))
    })
}

pub(super) fn layout_pages_to_spread(s: &mut Session, p: &Value) -> Result<Value> {
    let page = p.get("page").and_then(Value::as_u64).filter(|n| *n >= 1).ok_or_else(|| bad("layout.pages.toSpread", "missing page"))? as usize;
    let spread = p.get("spread").and_then(Value::as_u64).ok_or_else(|| bad("layout.pages.toSpread", "missing spread"))? as usize;
    s.edit(|d, _| {
        d.page_to_spread(page - 1, spread)?;
        Ok(json!({"spreads": d.spreads.len()}))
    })
}

pub(super) fn layout_spread_shuffle(s: &mut Session, p: &Value) -> Result<Value> {
    let si = p.get("spread").and_then(Value::as_u64).unwrap_or(0) as usize;
    let allow = p.get("allow").and_then(Value::as_bool).unwrap_or(true);
    s.edit(|d, _| {
        let sp = d.spreads.get_mut(si).ok_or_else(|| bad("layout.spreadShuffle", "no such spread"))?;
        std::sync::Arc::make_mut(sp).allow_shuffle = allow;
        if allow {
            d.repaginate();
        }
        ok()
    })
}

pub(super) fn layout_section(s: &mut Session, p: &Value) -> Result<Value> {
    let page = p.get("page").and_then(Value::as_u64).ok_or_else(|| bad("layout.section", "missing page"))? as usize;
    let n = s.doc()?.doc.page_count();
    if page == 0 || page > n {
        return Err(bad("layout.section", format!("no page {page}")));
    }
    let start = page - 1;
    let p = p.clone();
    s.edit(|d, _| {
        d.sections.retain(|x| x.start != start || start == 0);
        if p.get("remove").and_then(Value::as_bool).unwrap_or(false) && start != 0 {
            return ok();
        }
        let mut sec = d.sections.iter().find(|x| x.start == start).cloned().unwrap_or(designcraft_doc::Section {
            start,
            start_number: None,
            style: Default::default(),
            prefix: String::new(),
            marker: String::new(),
            include_prefix: false,
        });
        match p.get("startNumber") {
            Some(Value::Null) => sec.start_number = None,
            Some(v) => sec.start_number = v.as_u64().map(|v| v.max(1) as u32),
            None => {}
        }
        if let Some(v) = p.get("style") {
            sec.style = serde_json::from_value(v.clone()).map_err(|e| bad("layout.section", e.to_string()))?;
        }
        if let Some(v) = p.get("prefix").and_then(Value::as_str) {
            sec.prefix = v.into();
        }
        if let Some(v) = p.get("includePrefix").and_then(Value::as_bool) {
            sec.include_prefix = v;
        }
        if let Some(v) = p.get("marker").and_then(Value::as_str) {
            sec.marker = v.into();
        }
        d.sections.retain(|x| x.start != start);
        d.sections.push(sec);
        d.sections.sort_by_key(|x| x.start);
        Ok(json!({"names": (0..d.page_count()).map(|i| d.page_name(i)).collect::<Vec<_>>()}))
    })
}
