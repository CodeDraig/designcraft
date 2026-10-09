//! Layout menu and Pages/Layers panels: pages, spreads, parents, margins & columns, layers.

mod pages;

mod layers;

use designcraft_doc::{LAYER_COLORS, Layer, LayerId, Margins, SpreadId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, ok, str_param};
use crate::Result;

/// Each page's margin box, by spread and page id.
fn margin_boxes(d: &designcraft_doc::Document) -> Vec<(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)> {
    d.spread_refs()
        .filter_map(|r| d.spread(r).map(|sp| (r, sp)))
        .flat_map(|(r, sp)| sp.pages.iter().map(move |pg| (r, pg.id, pg.margin_rect())))
        .collect()
}

/// Layout Adjustment: objects on each page move and resize from its old margin box to its new
/// one (content keeps its size; frames that auto-fit refit).
fn adjust_layout(d: &mut designcraft_doc::Document, before: &[(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)]) {
    use designcraft_geom::Affine;
    let after = margin_boxes(d);
    for (r, pid, old) in before {
        let Some((_, _, new)) = after.iter().find(|(r2, p2, _)| r2 == r && p2 == pid) else { continue };
        if old == new || old.width() < 1e-6 || old.height() < 1e-6 {
            continue;
        }
        let m = Affine::translate((new.x0, new.y0))
            * Affine::scale_non_uniform(new.width() / old.width(), new.height() / old.height())
            * Affine::translate((-old.x0, -old.y0));
        let Some(sp) = d.spread(*r) else { continue };
        // The page's objects (by centre, measured against the old boxes).
        let ids: Vec<designcraft_doc::ItemId> = sp
            .items
            .iter()
            .filter(|it| {
                let c = it.bounds().center();
                // Pages share a spread side by side: the margin box's page is the one under the centre's x.
                before
                    .iter()
                    .filter(|(r2, _, _)| r2 == r)
                    .min_by(|a, b| dist(a.2, c.x).total_cmp(&dist(b.2, c.x)))
                    .is_some_and(|(_, p2, _)| p2 == pid)
            })
            .map(|it| it.id)
            .collect();
        for id in ids {
            let Some(it) = d.item_mut(id) else { continue };
            if matches!(it.content, designcraft_doc::Content::Group { .. }) {
                it.xf = m * it.xf;
            } else {
                let inner = it.xf.inverse() * m * it.xf;
                it.path.transform(inner);
                let ib = it.inner_bounds();
                if let designcraft_doc::Content::Graphic(g) = &mut it.content {
                    // Content keeps its size but moves with the frame; auto-fit refits.
                    let o = inner * designcraft_geom::Point::ZERO;
                    g.xf = Affine::translate(o.to_vec2()) * g.xf;
                    if let Some(xf) = g.fitted(ib, g.auto_fit) {
                        g.xf = xf;
                    }
                }
            }
        }
    }
}

/// Horizontal distance from `x` to a margin box (0 inside).
fn dist(r: designcraft_geom::Rect, x: f64) -> f64 {
    if x < r.x0 {
        r.x0 - x
    } else if x > r.x1 {
        x - r.x1
    } else {
        0.0
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "layout.pages.insert",
            "Insert Pages…",
            ["Layout", "Pages"],
            Some("Cmd+Shift+P"),
            "{count?: 1, after?: page index (default: last), parent?: \"A\"|null}",
            has_doc,
            pages::layout_pages_insert
        ),
        cmd!(
            "layout.rotateSpreadView",
            "Rotate Spread",
            [],
            None,
            "{spread?: index (0), angle: 90 (clockwise) | -90 | 180 | 0 (clear)} — turn one spread on screen (output is unaffected)",
            has_doc,
            pages::layout_rotate_spread_view
        ),
        cmd!("layout.pages.delete", "Delete Pages", ["Layout", "Pages"], None, "{pages: [index]}", has_doc, pages::layout_pages_delete),
        cmd!("layout.pages.move", "Move Pages…", ["Layout", "Pages"], None, "{from, to}", has_doc, |s, p| {
            let from = p.get("from").and_then(Value::as_u64).ok_or_else(|| bad("layout.pages.move", "missing from"))? as usize;
            let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| bad("layout.pages.move", "missing to"))? as usize;
            s.edit(|d, _| {
                d.move_page(from, to)?;
                ok()
            })
        }),
        cmd!(
            "layout.pages.duplicateSpread",
            "Duplicate Spread",
            ["Layout", "Pages"],
            None,
            "{spread}",
            has_doc,
            pages::layout_pages_duplicate_spread
        ),
        cmd!(
            "layout.pages.applyParent",
            "Apply Parent to Pages…",
            ["Layout", "Pages"],
            None,
            "{pages: [index], parent: \"A\"|null}",
            has_doc,
            pages::layout_pages_apply_parent
        ),
        cmd!("layout.parents.new", "New Parent…", ["Layout", "Pages"], None, "{prefix?, name?: \"Parent\"}", has_doc, pages::layout_parents_new),
        cmd!(noundo "layout.parents.edit", "Edit Parents", [], None, "{on: bool} — show parent spreads on the canvas", has_doc, |s, p| {
            let st = s.doc_mut()?;
            st.editing_parents = p.get("on").and_then(Value::as_bool).unwrap_or(!st.editing_parents);
            st.selection = Default::default();
            st.revision += 1;
            ok()
        }),
        cmd!(
            "layout.marginsAndColumns",
            "Margins and Columns…",
            ["Layout"],
            None,
            "{pages?: [index] (default all), margins?: number|{top,bottom,inside,outside}, columns?, gutter?, adjustLayout?: bool (objects follow the margins)}",
            has_doc,
            pages::layout_margins_and_columns
        ),
        cmd!(
            "layout.documentSetup",
            "Document Setup…",
            ["File"],
            Some("Cmd+Alt+P"),
            "{width?, height?, pages?: count, startPage?: n, facingPages?, binding?: leftToRight|rightToLeft, intent?: print|web|mobile, bleed?, slug?: n | [top, bottom, inside, outside], adjustLayout?: bool (objects follow the new page size)} → the document setup",
            has_doc,
            pages::layout_document_setup
        ),
        cmd!(
            "layout.pages.toSpread",
            "Add Page to Spread",
            ["Layout", "Pages"],
            None,
            "{page (1-based), spread (0-based)} — the page joins that spread (up to 10 pages; the spread stops shuffling)",
            has_doc,
            pages::layout_pages_to_spread
        ),
        cmd!(
            "layout.spreadShuffle",
            "Allow Selected Spread to Shuffle",
            ["Layout", "Pages"],
            None,
            "{spread (0-based), allow: bool} — off keeps the spread's pages together when pages are added or removed",
            has_doc,
            pages::layout_spread_shuffle
        ),
        cmd!(
            "layout.pageSize",
            "Page Size",
            [],
            None,
            "{pages: [1-based page numbers], width?, height?, preset?: \"A4\"|\"Letter\"|…} — pages of their own size (Page tool); objects on the pages keep their place on them",
            has_doc,
            page_size
        ),
        cmd!(
            "layout.section",
            "Numbering & Section Options…",
            ["Layout"],
            None,
            "{page (1-based; starts a section there), startNumber?: n|null (continue), style?: arabic|upperRoman|lowerRoman|upperLetters|lowerLetters, prefix?, includePrefix?, marker?, remove?: bool}",
            has_doc,
            pages::layout_section
        ),
        cmd!("layer.move", "Move Layer", [], None, "{id, to: index (0 = top/frontmost)}", has_doc, layers::layer_move),
        cmd!(noundo "layer.selectItems", "Select All on Layer", [], None, "{id}", has_doc, |s, p| {
            let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
            let st = s.doc_mut()?;
            let ids: Vec<_> = st.doc.spreads.iter().flat_map(|sp| sp.items.iter().filter(|i| i.layer == id && !i.locked).map(|i| i.id)).collect();
            st.selection = designcraft_doc::Selection::items(ids);
            st.revision += 1;
            ok()
        }),
        cmd!("layer.new", "New Layer…", [], None, "{name?}", has_doc, layers::layer_new),
        cmd!("layer.set", "Layer Options…", [], None, "{id, name?, visible?, locked?, printable?, color?: [r,g,b]}", has_doc, layers::layer_set),
        cmd!("layer.delete", "Delete Layer", [], None, "{id}", has_doc, layers::layer_delete),
        cmd!(
            "layer.merge",
            "Merge Layers",
            [],
            None,
            "{ids: [layer ids], into?: layer id (default: the first)} — their objects move to `into`, the other layers are deleted",
            has_doc,
            layers::layer_merge
        ),
        cmd!(
            "layer.deleteUnused",
            "Delete Unused Layers",
            [],
            None,
            "{} — layers without objects (one layer always stays)",
            has_doc,
            layers::layer_delete_unused
        ),
        cmd!(
            "layer.others",
            "Hide/Lock Others",
            [],
            None,
            "{id, hide?: bool, lock?: bool, show?: true (Show All Layers), unlock?: true (Unlock All Layers)} — Hide Others / Lock Others act on every layer but `id`",
            has_doc,
            layers::layer_others
        ),
        cmd!(noundo "layer.activate", "Set Active Layer", [], None, "{id}", has_doc, layers::layer_activate),
    ]
}

/// `"A"` → that parent's id; `null` → Some(None) (= [None]); missing → None (use default).
fn parent_ref(d: &designcraft_doc::Document, v: Option<&Value>) -> Option<Option<SpreadId>> {
    match v? {
        Value::Null => Some(None),
        Value::String(s) if s == "[None]" => Some(None),
        Value::String(s) => Some(d.parents.iter().find(|p| p.parent.as_ref().is_some_and(|i| i.prefix == *s || i.label() == *s)).map(|p| p.id)),
        _ => None,
    }
}

#[allow(dead_code)]
fn _r(_: Result<()>) {}

/// Layout › Page Size (the Page tool): resize pages one by one. Left pages grow to the left
/// (the spine stays put); objects move with their page.
fn page_size(s: &mut crate::Session, p: &Value) -> Result<Value> {
    let pages: Vec<usize> =
        p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|n| n as usize).collect()).unwrap_or_default();
    let n = s.doc()?.doc.page_count();
    if pages.is_empty() || pages.iter().any(|x| *x == 0 || *x > n) {
        return Err(bad("layout.pageSize", "give `pages` (1-based)"));
    }
    let (mut w, mut h) = (p.get("width").and_then(Value::as_f64), p.get("height").and_then(Value::as_f64));
    if let Some(name) = str_param(p, "preset") {
        let pr = designcraft_doc::build::PRESETS
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| bad("layout.pageSize", format!("no preset `{name}`")))?;
        w = Some(pr.width);
        h = Some(pr.height);
    }
    s.edit(|d, _| {
        resize_pages(d, &pages, w, h);
        Ok(json!({"pages": pages.len()}))
    })
}

/// Give `pages` (1-based) their own size. Objects stay on their page; pages with a liquid rule
/// re-flow their objects by it.
pub(crate) fn resize_pages(d: &mut designcraft_doc::Document, pages: &[usize], w: Option<f64>, h: Option<f64>) {
    {
        for page in pages {
            let Some((si, pi)) = d.page_loc(page - 1) else { continue };
            let sp = std::sync::Arc::make_mut(&mut d.spreads[si]);
            let old: Vec<f64> = sp.pages.iter().map(|x| x.x).collect();
            let old_bounds = sp.pages[pi].bounds();
            let pg = &mut sp.pages[pi];
            let dw = w.map_or(0.0, |w| w.max(1.0) - pg.width);
            if let Some(w) = w {
                pg.width = w.max(1.0);
            }
            if let Some(h) = h {
                pg.height = h.max(1.0);
            }
            let left = pg.side == designcraft_doc::PageSide::Left;
            sp.relayout();
            // Keep the spine: shift the whole spread back when a left page grows.
            if left && dw != 0.0 {
                for q in &mut sp.pages {
                    q.x -= dw;
                }
            }
            let new: Vec<f64> = sp.pages.iter().map(|x| x.x).collect();
            // Objects follow their page.
            let shifts: Vec<(f64, f64, f64)> =
                sp.pages.iter().enumerate().map(|(k, q)| (old[k], old[k] + q.width - if k == pi { dw } else { 0.0 }, new[k] - old[k])).collect();
            for it in &mut sp.items {
                let cx = it.bounds().center().x;
                if let Some((_, _, dx)) = shifts.iter().find(|(a, b, _)| cx >= *a && cx < *b)
                    && *dx != 0.0
                {
                    let it = std::sync::Arc::make_mut(it);
                    it.xf = designcraft_geom::Affine::translate((*dx, 0.0)) * it.xf;
                }
            }
            // Page guides move with their page.
            for (k, q) in sp.pages.iter_mut().enumerate() {
                let dx = new[k] - old[k];
                if dx != 0.0 {
                    for g in q.guides.iter_mut().filter(|g| g.orientation == designcraft_doc::Orientation::Vertical) {
                        g.position += dx;
                    }
                }
            }
            // Liquid rules: objects were carried along with the page's left edge.
            let moved = old_bounds + designcraft_geom::Vec2::new(new[pi] - old[pi], 0.0);
            super::liquid::apply(d, designcraft_doc::SpreadRef::Doc(si), pi, moved);
        }
    }
}

/// File › Document Setup values.
fn document_setup(d: &designcraft_doc::Document) -> Value {
    let st = &d.settings;
    let start = d.sections.iter().find(|x| x.start == 0).and_then(|x| x.start_number).unwrap_or(1);
    json!({"width": st.page_width, "height": st.page_height, "pages": d.page_count(), "startPage": start, "facingPages": st.facing_pages,
        "binding": if st.right_to_left_binding { "rightToLeft" } else { "leftToRight" }, "intent": st.intent, "bleed": st.bleed, "slug": st.slug})
}

#[cfg(test)]
mod setup_tests {
    use serde_json::json;

    #[test]
    fn document_setup_pages_start_bleed_slug() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let r = s.execute("layout.documentSetup", &json!({})).unwrap();
        assert_eq!(r["pages"], 2);
        assert_eq!(r["startPage"], 1);
        let r = s.execute("layout.documentSetup", &json!({"pages": 5, "startPage": 2, "bleed": 9, "slug": [0, 18, 0, 0], "intent": "web"})).unwrap();
        assert_eq!(r["pages"], 5);
        assert_eq!(r["startPage"], 2);
        assert_eq!(r["bleed"], json!([9.0, 9.0, 9.0, 9.0]));
        assert_eq!(r["slug"], json!([0.0, 18.0, 0.0, 0.0]));
        assert_eq!(r["intent"], "web");
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.page_name(0), "2");
        assert_eq!(d.page(0).unwrap().side, designcraft_doc::PageSide::Left);
        assert_eq!(d.spreads[0].pages.len(), 2, "pages 2–3 form the first spread");
        let r = s.execute("layout.documentSetup", &json!({"pages": 3})).unwrap();
        assert_eq!(r["pages"], 3);
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn right_to_left_binding_reverses_spreads() {
        use designcraft_doc::PageSide::{Left, Right};
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        // A frame on page 2 stays on page 2.
        let w = s.doc().unwrap().doc.settings.page_width;
        let f = s.execute("frame.create", &json!({"rect": [w + 72.0, 72, w + 200.0, 200], "content": "text", "text": "two"})).unwrap();
        let r = s.execute("layout.documentSetup", &json!({"binding": "rightToLeft"})).unwrap();
        assert_eq!(r["binding"], "rightToLeft");
        let d = &s.doc().unwrap().doc;
        let shape: Vec<Vec<(designcraft_doc::PageSide, f64)>> = d.spreads.iter().map(|sp| sp.pages.iter().map(|p| (p.side, p.x)).collect()).collect();
        // Page 1 alone on the left; pages 2–3 with 2 on the right; page 4 alone on the right.
        assert_eq!(shape, vec![vec![(Left, 0.0)], vec![(Right, w), (Left, 0.0)], vec![(Right, 0.0)]]);
        let (si, _) = d.page_loc(1).unwrap();
        assert_eq!(si, 1);
        let it = d.item(designcraft_doc::ItemId(f["id"].as_u64().unwrap())).unwrap();
        assert_eq!(d.spreads[si].page_at_x(it.bounds().center().x), Some(0), "the frame is still on page 2");
        assert!(it.bounds().x0 >= w, "page 2 is the right page now");
        d.check().unwrap();
        // And back.
        s.execute("layout.documentSetup", &json!({"binding": "leftToRight"})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.spreads[1].pages.iter().map(|p| (p.side, p.x)).collect::<Vec<_>>(), vec![(Left, 0.0), (Right, w)]);
    }
}

#[cfg(test)]
mod layer_tests {
    use serde_json::json;

    #[test]
    fn merge_delete_unused_hide_and_lock_others() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let l1 = s.doc().unwrap().doc.layers[0].id.0;
        let l2 = s.execute("layer.new", &json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap();
        let l3 = s.execute("layer.new", &json!({"name": "Empty"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": l2})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.others", &json!({"id": l2, "hide": true, "lock": true})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.layers.iter().all(|l| (l.id.0 == l2) == l.visible && (l.id.0 != l2) == l.locked));
        s.execute("layer.others", &json!({"id": l2, "show": true, "unlock": true})).unwrap();
        assert!(s.doc().unwrap().doc.layers.iter().all(|l| l.visible && !l.locked));
        let r = s.execute("layer.deleteUnused", &json!({})).unwrap();
        assert_eq!(r["deleted"], 2, "the empty default layer and Empty");
        let _ = (l1, l3);
        let l4 = s.execute("layer.new", &json!({"name": "Into"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.merge", &json!({"ids": [l4, l2]})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.item(designcraft_doc::ItemId(f)).unwrap().layer.0, l4);
    }
}

#[cfg(test)]
mod page_size_tests {
    use serde_json::json;

    #[test]
    fn page_size_per_page_keeps_objects_on_their_page() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        // Page 3 is the right page of spread 2 (pages 2–3); an object on it.
        let d = s.doc().unwrap().doc.clone();
        let (si, pi) = d.page_loc(2).unwrap();
        let x0 = d.spreads[si].pages[pi].x;
        let id = s.execute("frame.create", &json!({"spread": si, "rect": [x0 + 100.0, 100, x0 + 200.0, 200]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.pageSize", &json!({"pages": [2], "width": 300, "height": 500})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let left = &d.spreads[si].pages[0];
        assert_eq!((left.width, left.height), (300.0, 500.0));
        assert_eq!(d.spreads[si].pages[1].x, x0, "the spine stays");
        let it = d.item(designcraft_doc::ItemId(id)).unwrap();
        assert_eq!(it.bounds().x0, x0 + 100.0);
        assert_eq!(d.page_of_item(it.id), Some(2));
        s.execute("layout.pageSize", &json!({"pages": [3], "preset": "A4"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!((d.spreads[si].pages[1].width - 595.276).abs() < 0.01);
        assert!(s.execute("layout.pageSize", &json!({"pages": [9], "width": 100})).is_err());
    }
}

#[cfg(test)]
mod spread_tests {
    use serde_json::json;

    #[test]
    fn three_page_spread_survives_page_inserts() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 5})).unwrap();
        // [1] [2 3] [4 5]: page 4 joins spread 1 → [1] [2 3 4] [5].
        let d = s.doc().unwrap().doc.clone();
        let (si, pi) = d.page_loc(3).unwrap();
        let x = d.spreads[si].pages[pi].x;
        let id = s.execute("frame.create", &json!({"spread": si, "rect": [x + 50.0, 50, x + 150.0, 150]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.pages.toSpread", &json!({"page": 4, "spread": 1})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.spreads[1].pages.len(), 3);
        assert!(!d.spreads[1].allow_shuffle);
        assert_eq!(d.page_of_item(designcraft_doc::ItemId(id)), Some(3), "the object went with its page");
        // Adding a page at the start reshuffles the others but keeps the 3-page spread.
        s.execute("layout.pages.insert", &json!({"after": 0, "count": 1})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.spreads.iter().any(|sp| sp.pages.len() == 3 && !sp.allow_shuffle));
        assert_eq!(d.page_count(), 6);
        d.check().unwrap();
    }
}

#[cfg(test)]
mod page_numbering_view_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn section_and_absolute_page_numbering_view() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        s.execute("layout.section", &json!({"page": 1, "startNumber": 1, "style": "lowerRoman"})).unwrap();
        s.execute("layout.section", &json!({"page": 3, "startNumber": 1})).unwrap();
        assert_eq!((0..4).map(|i| s.page_label(i)).collect::<Vec<_>>(), ["i", "ii", "1", "2"]);
        assert_eq!(s.resolve_page("ii"), Some(1));
        assert_eq!(s.resolve_page("1"), Some(2), "section name first");
        assert_eq!(s.resolve_page("+1"), Some(0), "+n is a position");
        s.execute("prefs.set", &json!({"absolutePageNumbers": true})).unwrap();
        assert_eq!((0..4).map(|i| s.page_label(i)).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
        assert_eq!(s.resolve_page("1"), Some(0));
        assert_eq!(s.resolve_page("9"), None);
    }
}

#[cfg(test)]
mod adjust_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn adjust_layout_follows_the_new_page_size_and_margins() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        // Letter, 36 pt margins: a frame filling the left half of the margin box.
        let id = s.execute("frame.create", &json!({"rect": [36, 36, 306, 756]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.documentSetup", &json!({"width": 1224, "adjustLayout": true})).unwrap();
        let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert!((b.x0 - 36.0).abs() < 1e-6 && (b.x1 - 612.0).abs() < 1e-6, "half of the wider margin box: {b:?}");
        s.execute("layout.marginsAndColumns", &json!({"margins": 72, "adjustLayout": true})).unwrap();
        let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert!((b.x0 - 72.0).abs() < 1e-6 && (b.y0 - 72.0).abs() < 1e-6 && (b.y1 - 720.0).abs() < 1e-6, "{b:?}");
        // Without the option objects stay put.
        s.execute("layout.marginsAndColumns", &json!({"margins": 36})).unwrap();
        let b2 = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert_eq!(b, b2);
    }
}
