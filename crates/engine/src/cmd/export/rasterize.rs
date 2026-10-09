//! PDF transparency and unsupported-artwork preparation.

/// Does this object (or anything in it) need the transparency flattener?
pub(super) fn transparent(it: &designcraft_doc::Item) -> bool {
    it.involves_transparency()
}

/// Transparency Flattener: each object involving transparency becomes an opaque image of its
/// area as it looks over what's beneath it (so the rest stays vector); spreads whose parent items
/// involve transparency are rasterised whole. Works on a copy of the document made for export.
/// Returns the copy and the number of rasterised regions.
pub(super) fn flatten(d: &designcraft_doc::Document, cache: &designcraft_compose::Cache, ppi: f64) -> (designcraft_doc::Document, usize) {
    use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, Shape, SpreadRef};
    let mut out = d.clone();
    let mut n = 0;
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let k = ppi / 72.0;
    let parent_transparent = |pid: Option<designcraft_doc::SpreadId>| {
        pid.and_then(|id| d.parents.iter().find(|p| p.id == id)).is_some_and(|p| p.items.iter().any(|it| transparent(it)))
    };
    // An opaque image item showing `img` over `r`.
    let image_item =
        |out: &mut designcraft_doc::Document, r: designcraft_geom::Rect, img: designcraft_render::Rendered, layer: designcraft_doc::LayerId| {
            let (w, h) = (img.width, img.height);
            let aid = AssetId(out.alloc());
            out.assets.insert(
                aid,
                std::sync::Arc::new(Asset {
                    page: 0,
                    id: aid,
                    name: format!("flattened-{}.png", aid.0),
                    mime: "image/png".into(),
                    link: None,
                    data: std::sync::Arc::new(img.to_png()),
                    pixels: Some((w, h)),
                }),
            );
            let mut it = Item::new(ItemId(out.alloc()), layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
            it.content = Content::Graphic(Graphic {
                asset: aid,
                size: (w as f64, h as f64),
                xf: designcraft_geom::Affine::translate((r.x0, r.y0))
                    * designcraft_geom::Affine::scale_non_uniform(r.width() / w as f64, r.height() / h as f64),
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
            });
            it.stroke.weight = 0.0;
            it
        };
    for (si, sp) in d.spreads.iter().enumerate() {
        let whole = sp.pages.iter().any(|pg| pg.show_parent_items && parent_transparent(pg.parent));
        if whole {
            // Parent items involve transparency: every page of the spread as one image.
            let first = d.first_page_of_spread(si);
            let lid = out.default_layer();
            let mut images = Vec::new();
            for (pi, pg) in sp.pages.iter().enumerate() {
                if let Some(img) = rr.render_page(d, cache, first + pi, k, false, &Default::default()) {
                    images.push((pg.bounds(), img));
                }
            }
            let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
            osp.items.clear();
            for pg in &mut osp.pages {
                pg.show_parent_items = false;
            }
            for (b, img) in images {
                let it = image_item(&mut out, b, img, lid);
                let _ = out.insert_item(SpreadRef::Doc(si), it, None);
                n += 1;
            }
            continue;
        }
        // Region by region, bottom to top.
        for (idx, it) in sp.items.iter().enumerate() {
            if it.hidden || !transparent(it) {
                continue;
            }
            let reach = it.bounds().inflate(
                designcraft_render::effect_outset(it) + it.stroke.extent() + 1.0,
                designcraft_render::effect_outset(it) + it.stroke.extent() + 1.0,
            );
            // Only the part on the spread's pages and pasteboard that prints.
            let (w, h) = ((reach.width() * k).ceil().max(1.0) as u32, (reach.height() * k).ceil().max(1.0) as u32);
            if w as u64 * h as u64 > 80_000_000 {
                continue;
            }
            // The stack up to and including this object.
            let mut below = d.clone();
            std::sync::Arc::make_mut(&mut below.spreads[si]).items.truncate(idx + 1);
            let view = designcraft_geom::Affine::scale(k) * designcraft_geom::Affine::translate((-reach.x0, -reach.y0));
            let opts = designcraft_render::RenderOptions { printing_only: true, ..Default::default() };
            let img = rr.render(
                &below,
                cache,
                &[designcraft_render::Placed { spread: SpreadRef::Doc(si), xf: designcraft_geom::Affine::translate(designcraft_geom::Vec2::ZERO) }],
                w,
                h,
                view,
                &opts,
            );
            let new = image_item(&mut out, reach, img, it.layer);
            let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
            let Some(pos) = osp.items.iter().position(|x| x.id == it.id) else { continue };
            if threaded(d, it) {
                // Keep the frame (its story flows on) under the image.
                osp.items.insert(pos + 1, std::sync::Arc::new(new));
            } else {
                osp.items[pos] = std::sync::Arc::new(new);
            }
            n += 1;
        }
    }
    (out, n)
}

/// A text frame in a story that runs through other frames (replacing it would reflow the story).
pub(super) fn threaded(d: &designcraft_doc::Document, it: &designcraft_doc::Item) -> bool {
    it.text_frame().and_then(|t| d.story(t.story)).is_some_and(|st| st.frames.len() > 1)
}

/// Soft effects other than the gradient feather (which PDF export draws itself).
fn raster_effects(it: &designcraft_doc::Item) -> bool {
    let e = &it.effects;
    // Knockout groups too: PDF output here has no knockout transparency groups.
    it.knockout
        || e.drop_shadow.on
        || e.feather > 0.0
        || e.inner_shadow.on
        || e.outer_glow.on
        || e.inner_glow.on
        || e.bevel.on
        || e.satin.on
        || e.directional_feather.on
}

/// A copy of `d` where spread-level objects with soft effects are 300 ppi images of their whole
/// appearance (transparent around them, so they composite over what's behind).
pub(super) fn rasterize_effects(d: &designcraft_doc::Document, cache: &designcraft_compose::Cache) -> (Option<designcraft_doc::Document>, usize) {
    rasterize_where(d, cache, 300.0, |it| raster_effects(it) && !threaded(d, it))
}

/// A copy of `d` where the spread-level objects `pred` picks are images of their appearance at
/// `ppi` (transparent around them).
pub(super) fn rasterize_where(
    d: &designcraft_doc::Document,
    cache: &designcraft_compose::Cache,
    ppi: f64,
    pred: impl Fn(&designcraft_doc::Item) -> bool,
) -> (Option<designcraft_doc::Document>, usize) {
    use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, Shape, SpreadRef};
    let todo: Vec<(usize, ItemId)> =
        d.spreads.iter().enumerate().flat_map(|(si, sp)| sp.items.iter().filter(|it| !it.hidden && pred(it)).map(move |it| (si, it.id))).collect();
    if todo.is_empty() {
        return (None, 0);
    }
    let mut out = d.clone();
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let k = ppi / 72.0;
    let mut n = 0;
    for (si, id) in todo {
        let Some(it) = d.item(id) else { continue };
        let reach = it.bounds().inflate(
            designcraft_render::effect_outset(it) + it.stroke.extent() + 2.0,
            designcraft_render::effect_outset(it) + it.stroke.extent() + 2.0,
        );
        // Just this object on its spread, no paper.
        let mut solo = d.clone();
        let sp = std::sync::Arc::make_mut(&mut solo.spreads[si]);
        sp.items.retain(|x| x.id == id);
        for pg in &mut sp.pages {
            pg.show_parent_items = false;
        }
        let (w, h) = ((reach.width() * k).ceil().max(1.0) as u32, (reach.height() * k).ceil().max(1.0) as u32);
        if w as u64 * h as u64 > 60_000_000 {
            continue;
        }
        let view = designcraft_geom::Affine::scale(k) * designcraft_geom::Affine::translate((-reach.x0, -reach.y0));
        let opts = designcraft_render::RenderOptions { paper: false, background: None, printing_only: true, ..Default::default() };
        let img = rr.render(
            &solo,
            cache,
            &[designcraft_render::Placed { spread: SpreadRef::Doc(si), xf: designcraft_geom::Affine::translate(designcraft_geom::Vec2::ZERO) }],
            w,
            h,
            view,
            &opts,
        );
        let aid = AssetId(out.alloc());
        out.assets.insert(
            aid,
            std::sync::Arc::new(Asset {
                page: 0,
                id: aid,
                name: format!("effects-{}.png", id.0),
                mime: "image/png".into(),
                link: None,
                data: std::sync::Arc::new(img.to_png()),
                pixels: Some((w, h)),
            }),
        );
        let mut img_item = Item::new(ItemId(out.alloc()), it.layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(reach));
        img_item.content = Content::Graphic(Graphic {
            asset: aid,
            size: (w as f64, h as f64),
            xf: designcraft_geom::Affine::translate((reach.x0, reach.y0)) * designcraft_geom::Affine::scale(1.0 / k),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
        });
        img_item.stroke.weight = 0.0;
        img_item.alt_text = it.alt_text.clone();
        img_item.export_options = it.export_options.clone();
        // Same place in the stacking order.
        let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
        if let Some(pos) = osp.items.iter().position(|x| x.id == id) {
            osp.items[pos] = std::sync::Arc::new(img_item);
            n += 1;
        }
    }
    (Some(out), n)
}

/// A copy of `d` where placed PDFs with Object Layer Options hidden layers are 300 ppi images.
pub(super) fn rasterize_layered(d: &designcraft_doc::Document) -> (Option<designcraft_doc::Document>, usize) {
    let ids: Vec<designcraft_doc::ItemId> =
        d.all_items().into_iter().filter(|id| d.item(*id).is_some_and(|it| !it.pdf_hidden_layers.is_empty() && it.graphic().is_some())).collect();
    if ids.is_empty() {
        return (None, 0);
    }
    let mut out = d.clone();
    let mut n = 0;
    for id in ids {
        let Some((g, hidden)) = out.item(id).and_then(|it| Some((it.graphic()?.clone(), it.pdf_hidden_layers.clone()))) else { continue };
        let Some(a) = out.assets.get(&g.asset).cloned() else { continue };
        if !designcraft_render::is_pdf(&a.data) {
            continue;
        }
        let side = (g.size.0.max(g.size.1) * 300.0 / 72.0).clamp(64.0, 8000.0) as u32;
        let Some(png) = designcraft_render::pdf_page_png(&a.data, a.page as usize, side, &hidden) else { continue };
        let px = designcraft_render::image_size(&png);
        let aid = designcraft_doc::AssetId(out.alloc());
        out.assets.insert(
            aid,
            std::sync::Arc::new(designcraft_doc::Asset {
                page: 0,
                id: aid,
                name: format!("{}.png", a.name),
                mime: "image/png".into(),
                link: None,
                data: std::sync::Arc::new(png),
                pixels: px,
            }),
        );
        if let Some(designcraft_doc::Content::Graphic(gg)) = out.item_mut(id).map(|it| &mut it.content) {
            gg.asset = aid;
        }
        n += 1;
    }
    (Some(out), n)
}
