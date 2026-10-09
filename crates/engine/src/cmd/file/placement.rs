//! Asset loading and placement cursor lifecycle.

#[cfg(not(target_arch = "wasm32"))]
use super::EngineError;
use super::{
    Affine, Arc, Asset, AssetId, Content, Graphic, Item, ItemId, Rect, Result, Selection, Session, Shape, Value, bad, base64_decode, json, ok,
    place_layout_page, shapes, str_param,
};

pub(super) fn file_place(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name, link) = if let Some(b) = str_param(p, "base64") {
        (base64_decode(b), str_param(p, "name").unwrap_or("image").to_string(), None)
    } else if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let b = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        #[cfg(target_arch = "wasm32")]
        let b: Vec<u8> = vec![];
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.into());
        (b, name, Some(path.to_string()))
    } else {
        s.ui_requests.push(crate::UiRequest::Pick { purpose: "place".into(), params: json!({}) });
        return ok();
    };
    // A page of another layout (IDML or DesignCraft): its objects, as one group.
    let lname = name.to_lowercase();
    if lname.ends_with(".idml") || lname.ends_with(".designcraft") {
        return place_layout_page(s, p, &name, &bytes);
    }
    // EPS: shown and printed through its preview (or a placeholder) at its bounding box size.
    let (bytes, eps_size) = if designcraft_images::is_eps(&bytes) {
        let (proxy, size) = designcraft_images::eps_proxy(&bytes).ok_or_else(|| bad("file.place", "the EPS has no bounding box"))?;
        (proxy, Some(size))
    } else {
        (bytes, None)
    };
    // Video and sound: a media frame (Window › Interactive › Media).
    if let Some(mime) = designcraft_doc::media_mime(&name) {
        return super::super::media::place_media(s, p, name, mime, bytes, link);
    }
    // Text files (plain, Word, RTF) flow into frames.
    if designcraft_textimport::is_text_file(&name) {
        return super::super::place_text::place_text(s, p, &name, &bytes);
    }
    // Image Import Options: which page of a PDF (1-based).
    let pdf_page = match p.get("pdfPage").and_then(Value::as_u64) {
        Some(n) if designcraft_render::is_pdf(&bytes) => {
            let count = designcraft_render::pdf_page_count(&bytes).unwrap_or(1);
            if n == 0 || n as usize > count {
                return Err(bad("file.place", format!("the PDF has {count} page(s)")));
            }
            n as u32 - 1
        }
        _ => 0,
    };
    let (pw, ph) = match pdf_page {
        0 => designcraft_render::image_size(&bytes).ok_or_else(|| bad("file.place", "unsupported or corrupt image"))?,
        n => designcraft_render::pdf_page_size(&bytes, n as usize)
            .map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32))
            .ok_or_else(|| bad("file.place", "can't read that PDF page"))?,
    };
    // 72 ppi by default unless the file says otherwise; scale so it fits the page when huge.
    let (nw, nh) = eps_size.unwrap_or((pw as f64, ph as f64));
    // Image Import Options › Crop to: the frame shows that box of the PDF page.
    let crop_box = match str_param(p, "pdfCrop") {
        Some(k) if designcraft_render::is_pdf(&bytes) => {
            if !["crop", "trim", "bleed", "art", "media"].contains(&k) {
                return Err(bad("file.place", format!("unknown pdfCrop `{k}` (crop, trim, bleed, art, media)")));
            }
            designcraft_render::pdf_page_box(&bytes, pdf_page as usize, k).filter(|b| b.2 > 0.0 && b.3 > 0.0)
        }
        Some(_) => None,
        None => None,
    };
    let target_frame = super::super::id_param(p, "frame").or_else(|| {
        let st = s.active()?;
        st.selection.items.iter().copied().find(|i| st.doc.item(*i).is_some_and(|it| matches!(it.content, Content::Unassigned | Content::Graphic(_))))
    });
    let spread = super::super::spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    let px = p.get("x").and_then(Value::as_f64);
    let py = p.get("y").and_then(Value::as_f64);
    let want_w = p.get("width").and_then(Value::as_f64);
    s.edit(|d, sel| {
        let aid = AssetId(d.alloc());
        let mime = designcraft_render::image_mime(&bytes).to_string();
        d.assets.insert(aid, Arc::new(Asset { page: pdf_page, id: aid, name, mime, link, data: Arc::new(bytes), pixels: Some((pw, ph)) }));
        // The width the whole page (or image) gets; the height follows the proportions.
        let w = match want_w {
            Some(w) => w,
            None => {
                let page_w = d.settings.page_width * 0.6;
                nw.min(page_w)
            }
        };
        let id = if let Some(fid) = target_frame {
            let it = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
            let r = it.inner_bounds();
            // Fill frame proportionally.
            let k = (r.width() / nw).max(r.height() / nh);
            let (gw, gh) = (nw * k, nh * k);
            it.content = Content::Graphic(Graphic {
                asset: aid,
                size: (nw, nh),
                xf: Affine::translate((r.x0 + (r.width() - gw) / 2.0, r.y0 + (r.height() - gh) / 2.0)) * Affine::scale(k),
                auto_fit: designcraft_doc::Fitting::FillProportionally,
                fit_align: 4,
                crop: [0.0; 4],
            });
            fid
        } else {
            let sp = d.spread(spread).ok_or_else(|| bad("file.place", "no such spread"))?;
            let pr = sp.pages.first().map(|pg| pg.margin_rect()).unwrap_or(Rect::new(36.0, 36.0, 300.0, 300.0));
            let (x, y) = (px.unwrap_or(pr.x0), py.unwrap_or(pr.y0));
            let id = ItemId(d.alloc());
            let k = w / nw;
            let (bx, by, bw, bh) = crop_box.unwrap_or((0.0, 0.0, nw, nh));
            let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(Rect::new(x, y, x + bw * k, y + bh * k)));
            it.object_style = d.styles.default_graphic_frame.clone();
            it.content = Content::Graphic(Graphic {
                asset: aid,
                size: (nw, nh),
                xf: Affine::translate((x - bx * k, y - by * k)) * Affine::scale(k),
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
            });
            d.insert_item(spread, it, None)?;
            id
        };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0, "asset": aid.0}))
    })
}

pub(crate) fn read_source(p: &Value) -> Result<(Vec<u8>, String, Option<String>)> {
    if let Some(b) = str_param(p, "base64") {
        return Ok((base64_decode(b), str_param(p, "name").unwrap_or("image").to_string(), None));
    }
    let path = str_param(p, "path").ok_or_else(|| bad("place", "missing `path` or `base64`"))?;
    #[cfg(not(target_arch = "wasm32"))]
    let b = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    #[cfg(target_arch = "wasm32")]
    let b: Vec<u8> = vec![];
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.into());
    Ok((b, name, Some(path.to_string())))
}

pub(super) fn place_load(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name, link) = read_source(p)?;
    let (pw, ph) = designcraft_render::image_size(&bytes).ok_or_else(|| bad("place.load", "unsupported or corrupt image"))?;
    let aid = s.edit(|d, _| {
        let aid = AssetId(d.alloc());
        let mime = designcraft_render::image_mime(&bytes).to_string();
        d.assets.insert(
            aid,
            Arc::new(Asset { page: 0, id: aid, name: name.clone(), mime, link: link.clone(), data: Arc::new(bytes), pixels: Some((pw, ph)) }),
        );
        Ok(aid)
    })?;
    s.loaded = Some((aid, (pw as f64, ph as f64)));
    s.set_tool("placeGun");
    Ok(json!({"asset": aid.0, "name": name, "width": pw, "height": ph}))
}

pub(super) fn place_drop(s: &mut Session, p: &Value) -> Result<Value> {
    let (aid, (nw, nh)) = s.loaded.ok_or_else(|| bad("place.drop", "nothing loaded in the place cursor"))?;
    let sr = super::super::spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    let frame = super::super::id_param(p, "frame");
    let rect = super::super::rect_param(p, "rect");
    let (x, y) = (super::super::f64_or(p, "x", 0.0), super::super::f64_or(p, "y", 0.0));
    let r = s.edit(|d, sel| {
        let id = match frame {
            Some(fid) => {
                let it = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
                let r = it.inner_bounds();
                let k = (r.width() / nw).max(r.height() / nh);
                it.content = Content::Graphic(Graphic {
                    asset: aid,
                    size: (nw, nh),
                    xf: Affine::translate((r.x0 + (r.width() - nw * k) / 2.0, r.y0 + (r.height() - nh * k) / 2.0)) * Affine::scale(k),
                    auto_fit: designcraft_doc::Fitting::FillProportionally,
                    fit_align: 4,
                    crop: [0.0; 4],
                });
                fid
            }
            None => {
                // Drag: fit proportionally into the dragged rect; click: actual size at the point.
                let (frame_r, k) = match rect {
                    Some(r) if r.width() > 2.0 && r.height() > 2.0 => {
                        let k = (r.width() / nw).min(r.height() / nh);
                        (Rect::new(r.x0, r.y0, r.x0 + nw * k, r.y0 + nh * k), k)
                    }
                    _ => (Rect::new(x, y, x + nw, y + nh), 1.0),
                };
                let id = ItemId(d.alloc());
                let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(frame_r));
                it.object_style = d.styles.default_graphic_frame.clone();
                it.content = Content::Graphic(Graphic {
                    asset: aid,
                    size: (nw, nh),
                    xf: Affine::translate((frame_r.x0, frame_r.y0)) * Affine::scale(k),
                    auto_fit: Default::default(),
                    fit_align: 4,
                    crop: [0.0; 4],
                });
                d.insert_item(sr, it, None)?;
                id
            }
        };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })?;
    s.loaded = None;
    s.set_tool("selection");
    Ok(r)
}
