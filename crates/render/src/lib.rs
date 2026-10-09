//! DesignCraft renderer: spreads → premultiplied RGBA pixels with `vello_cpu`.
//!
//! The renderer draws *document content only* (paper, page items, parent items, composed text,
//! placed images). Non-printing chrome — guides, frame edges, ports, selection — is drawn by the
//! UI as vector overlays so it stays crisp at any zoom.
//!
//! Callers pass a list of spreads with their placement on a shared canvas and a canvas → pixel
//! view transform, so the UI can render exactly its viewport with every visible spread.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod output;
pub use output::Rendered;
mod pdf;
pub use pdf::{is_pdf, pdf_page_box, pdf_page_count, pdf_page_png, pdf_page_size, render_pdf_page};
mod preview;
use preview::{blend_space_view, plate_pixmap};
pub use preview::{proof_view, separation_view};
mod decode;
use decode::halve;
pub use decode::{decode_pixmap, decode_pixmap_page, image_size};
mod items;
pub(crate) use items::overprints;
pub use items::{blend_mode, color_of};

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_color::BlendMode as DcBlend;
use designcraft_compose::{Cache, ComposedStory};
use designcraft_doc::{Content, Document, Item, PageSide, SpreadRef, StoryId, StrokeAlign, StrokeType};
use designcraft_geom::corners;
use designcraft_geom::{Affine, BezPath, Rect, Vec2};
use vello_cpu::kurbo;
use vello_cpu::peniko::{self, BlendMode, Compose, Mix};
use vello_cpu::{Pixmap, RenderContext, Resources};

pub use vello_cpu;

pub mod damage;
mod fx;

pub use fx::effect_outset;
pub mod glyphs;
mod pdf_layers;
pub use pdf_layers::{pdf_hide_layers, pdf_layers};
pub mod images;
mod text;

/// Largest side of a raster the renderer draws. vello_cpu keeps sizes in `u16` and rounds them
/// up to its tiles (4 px) and depth buckets (128 px), so a side near `u16::MAX` overflows inside
/// it: stay a few buckets below.
pub const MAX_SIDE: u32 = u16::MAX as u32 + 1 - 512;

/// Largest page raster [`Renderer::render_page`] makes (2 GiB of RGBA).
pub const MAX_PAGE_PIXELS: u64 = 1 << 29;

/// What to draw.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Canvas background (premultiplied RGBA8); `None` = transparent.
    pub background: Option<[u8; 4]>,
    /// Fill pages with paper colour.
    pub paper: bool,
    /// Skip non-printing items and layers (Preview mode, export).
    pub printing_only: bool,
    /// Items not to draw (live drag previews).
    pub hidden: Vec<designcraft_doc::ItemId>,
    /// Draw text smaller than this many pixels as grey bars ("greeking"); 0 = never.
    pub greek_below_px: f64,
    /// Drop shadow on pages (screen view).
    pub page_shadow: bool,
    /// Pink highlight behind text set in fonts that aren't installed (screen view).
    pub highlight_missing_fonts: bool,
    /// Preferences › Composition: yellow behind lines whose word spacing breaks the paragraph's
    /// limits (darker = worse), green behind text with custom tracking or kerning (screen view).
    pub highlight_hj: bool,
    pub highlight_keeps: bool,
    pub highlight_custom_tracking: bool,
    /// Conditional text indicators: an underline in each condition's colour (screen view).
    pub condition_indicators: bool,
    /// Editorial note anchors (screen view).
    pub note_indicators: bool,
    /// Track Changes: added text highlighted (screen view).
    pub change_markup: bool,
    /// View › Structure › Show Tag Markers: brackets around inline-tagged text (screen view).
    pub tag_markers: bool,
    /// Separations Preview: draw one process plate (0 = cyan … 3 = black) as grey ink coverage,
    /// from each object's own colour (CMYK as authored, others through the colour settings).
    pub plate: Option<u8>,
    /// View › Display Performance.
    pub quality: DisplayQuality,
    /// Preferences › Appearance of Black: show 100% K as rich (pure) black instead of the
    /// accurate dark grey.
    pub rich_black: bool,
    /// View › Overprint Preview: overprinting inks mix with what's beneath.
    pub overprint_preview: bool,
    /// Screen: spreads with transparency are shown as blended in the document's blend space (a
    /// CMYK blend space keeps their colours in the working CMYK gamut).
    pub blend_space_view: bool,
}

/// View › Display Performance: how placed graphics and effects are drawn on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayQuality {
    /// Graphics as grey boxes, no transparency effects.
    Fast,
    /// Low-resolution proxies (72 ppi at 100%).
    Typical,
    /// Full resolution.
    #[default]
    High,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            background: None,
            paper: true,
            printing_only: false,
            hidden: vec![],
            greek_below_px: 0.0,
            page_shadow: false,
            highlight_missing_fonts: false,
            highlight_hj: false,
            highlight_keeps: false,
            highlight_custom_tracking: false,
            condition_indicators: false,
            note_indicators: false,
            change_markup: false,
            tag_markers: false,
            plate: None,
            quality: DisplayQuality::High,
            rich_black: false,
            overprint_preview: false,
            blend_space_view: false,
        }
    }
}

/// A spread placed on the canvas: canvas = xf × spread (a translation, turned for a rotated spread view).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub spread: SpreadRef,
    pub xf: Affine,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub items: usize,
    pub glyphs: usize,
    pub micros: u64,
}

/// Reusable renderer: keeps the render context, the frame-space glyph paths of composed text
/// and (process-wide) decoded images between renders.
pub struct Renderer {
    ctx: Option<RenderContext>,
    resources: Resources,
    glyphs: text::GlyphCache,
    /// Composed stories looked up during the current render.
    stories: HashMap<(StoryId, Option<String>), Arc<ComposedStory>>,
    pub threads: u16,
    pub stats: FrameStats,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Frame<'a> {
    doc: &'a Document,
    cache: &'a Cache,
    view: Affine,
    /// Visible area in spread space.
    visible: Rect,
    /// Size of a device pixel in points.
    px: f64,
    opts: &'a RenderOptions,
    /// Drawing into a multithreaded context (no filter layers).
    mt: bool,
}

pub fn default_threads() -> u16 {
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(n) = std::env::var("DESIGNCRAFT_RENDER_THREADS").ok().and_then(|v| v.parse().ok()) {
            return n;
        }
        std::thread::available_parallelism().map(|n| (n.get().saturating_sub(1)).clamp(0, 8) as u16).unwrap_or(0)
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            ctx: None,
            resources: Resources::new(),
            glyphs: text::GlyphCache::default(),
            stories: HashMap::new(),
            threads: default_threads(),
            stats: FrameStats::default(),
        }
    }

    /// Drop cached glyph paths (they are rebuilt on demand).
    pub fn clear_caches(&mut self) {
        self.glyphs.clear();
        self.stories.clear();
    }

    /// Number of text frames whose glyph paths are cached.
    pub fn cached_frames(&self) -> usize {
        self.glyphs.len()
    }

    /// Render the given spreads into a `width`×`height` image with `view` (canvas → pixels).
    pub fn render(
        &mut self,
        doc: &Document,
        cache: &Cache,
        spreads: &[Placed],
        width: u32,
        height: u32,
        view: Affine,
        opts: &RenderOptions,
    ) -> Rendered {
        let start = now();
        let w = width.clamp(1, MAX_SIDE) as u16;
        let h = height.clamp(1, MAX_SIDE) as u16;
        let mut ctx = match self.ctx.take() {
            Some(mut c) if c.width() == w && c.height() == h && c.render_settings().num_threads == self.threads => {
                c.reset();
                c
            }
            _ => RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: self.threads, ..Default::default() }),
        };
        let mt = ctx.render_settings().num_threads > 0;
        RICH_BLACK.with(|r| r.set(opts.rich_black));
        PLATE.with(|p| p.set(opts.plate));
        self.stats = FrameStats::default();
        self.stories.clear();
        self.glyphs.tick();
        if let Some(bg) = opts.background {
            ctx.set_transform(Affine::IDENTITY);
            ctx.set_paint(peniko::Color::from_rgba8(bg[0], bg[1], bg[2], bg[3]));
            ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));
        }
        let px = 1.0 / view.determinant().abs().sqrt().max(1e-12);
        let canvas_visible = view.inverse().transform_rect_bbox(Rect::new(0.0, 0.0, w as f64, h as f64));
        for pl in spreads {
            let Some(sp) = doc.spread(pl.spread) else { continue };
            let sview = view * pl.xf;
            let visible = pl.xf.inverse().transform_rect_bbox(canvas_visible);
            let bounds = sp.bounds();
            // Cull spreads far outside the viewport (pasteboard items may extend; allow a margin).
            if !rect_overlaps(bounds.inflate(2000.0, 2000.0), visible) {
                continue;
            }
            let f = Frame { doc, cache, view: sview, visible, px, opts, mt };
            self.draw_spread(&mut ctx, &f, pl.spread);
        }
        ctx.flush();
        // Render straight into the returned buffer (no extra copy of the frame).
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        if let Some(pm) = vello_cpu::PixmapMut::new(w, h, &mut pixels) {
            ctx.render(pm, &mut self.resources);
        }
        if opts.blend_space_view && doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk && opts.plate.is_none() {
            let mut img = Rendered { width: w as u32, height: h as u32, pixels };
            for pl in spreads {
                let Some(sp) = doc.spread(pl.spread) else { continue };
                if sp.items.iter().any(|it| it.involves_transparency() && !it.hidden) {
                    let r = (view * pl.xf).transform_rect_bbox(sp.bounds());
                    blend_space_view(&mut img, r);
                }
            }
            pixels = img.pixels;
        }
        self.ctx = Some(ctx);
        self.stories.clear();
        self.stats.micros = now().saturating_sub(start);
        Rendered { width: w as u32, height: h as u32, pixels }
    }

    /// Render one document page (absolute index) at `scale` px/pt, optionally including bleed.
    pub fn render_page(&mut self, doc: &Document, cache: &Cache, abs: usize, scale: f64, bleed: bool, opts: &RenderOptions) -> Option<Rendered> {
        let (si, pi) = doc.page_loc(abs)?;
        let page = &doc.spreads[si].pages[pi];
        let mut r = page.bounds();
        if bleed {
            let b = doc.settings.bleed;
            let (l, rr) = if page.side == PageSide::Left { (b[3], b[2]) } else { (b[2], b[3]) };
            r = Rect::new(r.x0 - l, r.y0 - b[0], r.x1 + rr, r.y1 + b[1]);
        }
        let w = (r.width() * scale).round().max(1.0) as u32;
        let h = (r.height() * scale).round().max(1.0) as u32;
        if w > MAX_SIDE || h > MAX_SIDE || w as u64 * h as u64 > MAX_PAGE_PIXELS {
            log::warn!("page {abs} at {scale} px/pt would be {w}×{h} pixels: too large to render");
            return None;
        }
        let view = Affine::scale(scale) * Affine::translate(-r.origin().to_vec2());
        let mut o = opts.clone();
        o.background = Some([255, 255, 255, 255]);
        Some(self.render(doc, cache, &[Placed { spread: SpreadRef::Doc(si), xf: Affine::translate(Vec2::ZERO) }], w, h, view, &o))
    }

    fn draw_spread(&mut self, ctx: &mut RenderContext, f: &Frame, sr: SpreadRef) {
        let doc = f.doc;
        let Some(sp) = doc.spread(sr) else { return };
        // Paper.
        if f.opts.paper {
            let paper = doc.resolve_color(designcraft_color::swatch::PAPER, 1.0).unwrap_or(designcraft_color::Color::WHITE);
            if f.opts.page_shadow {
                ctx.set_transform(f.view);
                ctx.set_paint(peniko::Color::from_rgba8(0, 0, 0, 90));
                for p in &sp.pages {
                    let s = 3.0 * f.px;
                    ctx.fill_rect(&(p.bounds() + Vec2::new(s, s)));
                }
            }
            ctx.set_transform(f.view);
            ctx.set_paint(color_of(&paper, 1.0));
            // One rect for equal-height spreads avoids an anti-aliasing seam at the spine.
            if sp.pages.windows(2).all(|w| w[0].height == w[1].height) {
                ctx.fill_rect(&sp.bounds());
            } else {
                for p in &sp.pages {
                    ctx.fill_rect(&p.bounds());
                }
            }
        }
        // Layers back to front; within each layer parent items first (document spreads only).
        for layer in doc.layers.iter().rev() {
            if !layer.visible || f.opts.printing_only && !layer.printable {
                continue;
            }
            if let SpreadRef::Doc(si) = sr {
                let first = doc.first_page_of_spread(si);
                for (pi, page) in sp.pages.iter().enumerate() {
                    if !page.show_parent_items {
                        continue;
                    }
                    let Some((ppi, ppage)) = doc.parent_page_for(first + pi) else { continue };
                    let parent = &doc.parents[ppi];
                    let dx = page.x - parent.pages[ppage].x;
                    let page_name = doc.page_name(first + pi);
                    let pr = parent.pages[ppage].bounds();
                    for it in &parent.items {
                        if it.layer != layer.id || page.overridden.contains(&it.id) {
                            continue;
                        }
                        // Parent items belong to the parent page they sit on.
                        if parent.page_at_x(it.bounds().center().x) != Some(ppage) && parent.pages.len() > 1 {
                            continue;
                        }
                        let _ = pr;
                        self.draw_item(ctx, f, it, Affine::translate((dx, 0.0)), Some(&page_name));
                    }
                }
            }
            for it in &sp.items {
                if it.layer == layer.id {
                    self.draw_item(ctx, f, it, Affine::IDENTITY, None);
                }
            }
        }
    }
}

fn rect_overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

thread_local! {
    /// Object Layer Options of the placed PDF being drawn.
    static HIDDEN_LAYERS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Appearance of Black for the render on this thread: 100% K as pure black.
    static RICH_BLACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Separations Preview plate for the render on this thread.
    static PLATE: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

#[cfg(not(target_arch = "wasm32"))]
fn now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}
#[cfg(target_arch = "wasm32")]
fn now() -> u64 {
    0
}

pub use designcraft_images::mime as image_mime;

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::{Fill, ParaFormat};

    /// `Rendered` has public fields: a buffer that doesn't match the stated size used to panic
    /// in `to_png`.
    /// A canvas 65535 pixels wide (any render wider than that was clamped to it) panicked inside
    /// vello_cpu when it snapped the width up to its tiles; a huge page export now declines.
    #[test]
    fn very_wide_renders_do_not_panic() {
        let doc = Document::new(&NewDocument::default());
        let cache = Cache::new();
        let mut r = Renderer::new();
        let img = r.render(&doc, &cache, &[], 70_000, 4, Affine::IDENTITY, &RenderOptions::default());
        assert_eq!(img.width, MAX_SIDE);
        let page = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
        let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
        let img = r.render(&doc, &cache, &page, 70_000, 64, Affine::scale(120.0), &opts);
        assert_eq!(img.width, MAX_SIDE);
        assert!(r.render_page(&doc, &cache, 0, 500.0, true, &RenderOptions::default()).is_none());
    }

    #[test]
    fn to_png_survives_a_mismatched_buffer() {
        let short = Rendered { width: 4, height: 4, pixels: vec![255; 10] };
        let png = short.to_png();
        assert_eq!(image::load_from_memory(&png).map(|i| (i.width(), i.height())).ok(), Some((4, 4)));
        assert!(Rendered { width: 0, height: 0, pixels: vec![] }.to_png().len() < 1024);
    }

    #[test]
    fn proof_view_simulates_the_target() {
        use designcraft_color::cms::{ProofSetup, ProofTarget};
        let red = |a: u8| Rendered { width: 1, height: 1, pixels: vec![a, 0, 0, a] };
        let mut img = red(255);
        proof_view(&mut img, &ProofSetup { target: ProofTarget::Protanopia, ..Default::default() });
        assert!(img.pixels[1] > 40, "protanopia turns pure red olive: {:?}", img.pixels);
        let mut img = red(255);
        proof_view(&mut img, &ProofSetup { target: ProofTarget::MonitorRgb, ..Default::default() });
        assert!(img.pixels[0] > 245 && img.pixels[1] < 10, "monitor RGB is no simulation: {:?}", img.pixels);
        // Transparent pixels stay transparent; premultiplication is kept.
        let mut img = Rendered { width: 2, height: 1, pixels: vec![0, 0, 0, 0, 100, 0, 0, 128] };
        proof_view(&mut img, &ProofSetup { target: ProofTarget::WorkingCmyk, ..Default::default() });
        assert_eq!(&img.pixels[..4], &[0, 0, 0, 0]);
        assert!(img.pixels[4] <= 128 && img.pixels[7] == 128);
    }

    #[test]
    fn renders_page_with_text_and_fill() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (fid, _) =
            d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 576.0, 300.0), lid, "Hello DesignCraft", ParaFormat::default()).unwrap();
        let id = designcraft_doc::ItemId(d.alloc());
        let mut box_ =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(36.0, 400.0, 136.0, 500.0)));
        box_.fill = Fill::swatch("C=100 M=0 Y=0 K=0");
        d.insert_item(SpreadRef::Doc(0), box_, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        assert_eq!((img.width, img.height), (612, 792));
        // Paper is white, the box is cyan-ish, text drew some dark pixels.
        assert_eq!(img.pixel(300, 700), [255, 255, 255, 255]);
        let c = img.pixel(80, 450);
        assert!(c[0] < 80 && c[2] > 150, "{c:?}");
        let dark = (36..300).flat_map(|x| (36..60).map(move |y| (x, y))).filter(|&(x, y)| img.pixel(x, y)[0] < 100).count();
        assert!(dark > 50, "text pixels: {dark}");
        let _ = fid;
        assert!(r.stats.glyphs >= 15);
    }

    #[test]
    fn arrowheads_draw_at_line_ends() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let id = designcraft_doc::ItemId(d.alloc());
        let path = designcraft_geom::shapes::line(designcraft_geom::Point::new(100.0, 400.0), designcraft_geom::Point::new(400.0, 400.0));
        let mut line = Item::new(id, lid, designcraft_doc::Shape::GraphicLine, path);
        line.stroke = designcraft_doc::Stroke {
            swatch: designcraft_color::swatch::BLACK.into(),
            weight: 2.0,
            end: designcraft_doc::Arrowhead::TriangleWide,
            ..Default::default()
        };
        d.insert_item(SpreadRef::Doc(0), line, None).unwrap();
        // A dashed line with a gap colour: the gaps are painted.
        let id = designcraft_doc::ItemId(d.alloc());
        let path = designcraft_geom::shapes::line(designcraft_geom::Point::new(100.0, 500.0), designcraft_geom::Point::new(400.0, 500.0));
        let mut dashed = Item::new(id, lid, designcraft_doc::Shape::GraphicLine, path);
        dashed.stroke = designcraft_doc::Stroke {
            swatch: designcraft_color::swatch::BLACK.into(),
            weight: 4.0,
            kind: designcraft_doc::StrokeType::Dashed { pattern: vec![12.0, 4.0] },
            gap_swatch: "C=100 M=0 Y=0 K=0".into(),
            ..Default::default()
        };
        d.insert_item(SpreadRef::Doc(0), dashed, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        // The head is 6.4 × weight wide: ink 4 pt above the line near the tip, none at the start.
        assert!(img.pixel(393, 396)[0] < 128, "{:?}", img.pixel(393, 396));
        assert_eq!(img.pixel(105, 396), [255, 255, 255, 255]);
        // Nothing past the tip.
        assert_eq!(img.pixel(403, 400), [255, 255, 255, 255]);
        let (dash, gap) = (img.pixel(106, 500), img.pixel(114, 500));
        assert!(dash[0] < 80 && dash[2] < 80, "dash {dash:?}");
        assert!(gap[0] < 80 && gap[2] > 150, "gap {gap:?}");
    }

    #[test]
    fn overprint_preview_mixes_inks() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        for (sw, op) in [("C=0 M=0 Y=100 K=0", false), ("C=0 M=100 Y=0 K=0", true)] {
            let id = designcraft_doc::ItemId(d.alloc());
            let mut it =
                Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
            it.fill = Fill { overprint: op, ..Fill::swatch(sw) };
            d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        }
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let px = |on: bool, r: &mut Renderer| {
            r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { overprint_preview: on, ..Default::default() }).unwrap().pixel(150, 150)
        };
        let off = px(false, &mut r);
        let on = px(true, &mut r);
        assert!(off[2] > 100, "magenta knocks out: {off:?}");
        assert!(on[2] < 60 && on[0] > 180, "magenta over yellow → red: {on:?}");
    }

    #[test]
    fn cmyk_blend_space_shows_transparent_spreads_in_cmyk() {
        let mut d = Document::new(&NewDocument::default());
        d.swatches.push(designcraft_color::swatch::Swatch::color("Green", designcraft_color::Color::rgb8(0, 255, 0)));
        let lid = d.default_layer();
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
        it.fill = Fill::swatch("Green");
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let px = |d: &Document, r: &mut Renderer| {
            r.render_page(d, &cache, 0, 1.0, false, &RenderOptions { blend_space_view: true, ..Default::default() }).unwrap().pixel(150, 150)
        };
        // No transparency on the spread: shown as is.
        assert_eq!(px(&d, &mut r)[1], 255);
        // With transparency in a CMYK blend space the bright green leaves the RGB gamut's edge.
        d.item_mut(id).unwrap().opacity = 0.99;
        let cmyk = px(&d, &mut r);
        assert!(cmyk[0] > 20 || cmyk[1] < 240, "{cmyk:?}");
        d.settings.blend_space = designcraft_doc::BlendSpace::Rgb;
        let rgb = px(&d, &mut r);
        assert!(rgb[1] > 245 && rgb[0] < 10, "{rgb:?}");
    }

    #[test]
    fn knockout_group_hides_its_own_objects() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let mut kids = Vec::new();
        for (sw, x) in [("C=0 M=0 Y=100 K=0", 100.0), ("C=0 M=100 Y=0 K=0", 150.0)] {
            let id = designcraft_doc::ItemId(d.alloc());
            let mut it =
                Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(x, 100.0, x + 100.0, 200.0)));
            it.fill = Fill::swatch(sw);
            it.opacity = 0.5;
            kids.push(std::sync::Arc::new(it));
        }
        let gid = designcraft_doc::ItemId(d.alloc());
        let mut g = Item::new(gid, lid, designcraft_doc::Shape::Group, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 250.0, 200.0)));
        g.content = designcraft_doc::Content::Group { items: kids };
        d.insert_item(SpreadRef::Doc(0), g, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let overlap = |d: &Document, r: &mut Renderer| r.render_page(d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap().pixel(175, 150);
        let plain = overlap(&d, &mut r);
        d.item_mut(gid).unwrap().knockout = true;
        let ko = overlap(&d, &mut r);
        let magenta_only = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap().pixel(225, 150);
        assert_ne!(plain, ko, "{plain:?} {ko:?}");
        for c in 0..3 {
            assert!((ko[c] as i32 - magenta_only[c] as i32).abs() <= 3, "the overlap shows magenta alone: {ko:?} vs {magenta_only:?}");
        }
    }

    #[test]
    fn separations_split_plates_and_flag_ink_limits() {
        // Pure cyan (0, 255, 255) and rich black-ish (20, 20, 20).
        let mut img = Rendered { width: 2, height: 1, pixels: vec![0, 255, 255, 255, 20, 20, 20, 255] };
        let mut c = img.clone();
        separation_view(&mut c, Some(0), None);
        assert_eq!(c.pixel(0, 0)[0], 0, "cyan plate solid under cyan");
        let mut m = img.clone();
        separation_view(&mut m, Some(1), None);
        assert_eq!(m.pixel(0, 0)[0], 255, "no magenta");
        separation_view(&mut img, None, Some(0.5));
        assert_eq!(img.pixel(0, 0), [230, 40, 40, 255], "100% over a 50% limit");
    }

    #[test]
    fn black_overprints_only_with_the_preference() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        for (sw, r) in
            [("C=100 M=0 Y=0 K=0", Rect::new(100.0, 100.0, 200.0, 200.0)), (designcraft_color::swatch::BLACK, Rect::new(150.0, 100.0, 250.0, 200.0))]
        {
            let id = designcraft_doc::ItemId(d.alloc());
            let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
            it.fill = Fill::swatch(sw);
            d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        }
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let o = RenderOptions { overprint_preview: true, ..Default::default() };
        let on = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(175, 150);
        let alone = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(225, 150);
        d.settings.overprint_black = false;
        let off = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(175, 150);
        assert_eq!(off, alone, "knocks out: black alone");
        assert_ne!(on, alone, "overprints: the cyan shows through");
    }

    #[test]
    fn appearance_of_black_on_screen() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
        it.fill = Fill::swatch(designcraft_color::swatch::BLACK);
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let px = |rich: bool, r: &mut Renderer| {
            r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { rich_black: rich, ..Default::default() }).unwrap().pixel(150, 150)
        };
        assert!(px(false, &mut r)[0] > 20, "accurate: a dark grey");
        assert_eq!(px(true, &mut r)[..3], [0, 0, 0]);
    }

    #[test]
    fn display_performance_fast_draws_grey_boxes() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        // A 4×4 red PNG placed at 100,100 (100×100 pt).
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let aid = designcraft_doc::AssetId(d.alloc());
        d.assets.insert(
            aid,
            Arc::new(designcraft_doc::Asset {
                page: 0,
                id: aid,
                name: "red.png".into(),
                mime: "image/png".into(),
                link: None,
                data: Arc::new(png),
                pixels: Some((4, 4)),
            }),
        );
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
        it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
            asset: aid,
            size: (100.0, 100.0),
            xf: Affine::translate((100.0, 100.0)),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
        });
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let px = |q: DisplayQuality, r: &mut Renderer| {
            r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { quality: q, ..Default::default() }).unwrap().pixel(150, 150)
        };
        assert_eq!(px(DisplayQuality::High, &mut r)[..3], [255, 0, 0]);
        assert_eq!(px(DisplayQuality::Typical, &mut r)[..3], [255, 0, 0]);
        assert_eq!(px(DisplayQuality::Fast, &mut r)[..3], [178, 178, 178]);
    }

    #[test]
    fn missing_fonts_are_highlighted_on_screen_only() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 100.0), lid, "Missing font", ParaFormat::default()).unwrap();
        d.story_mut(sid).unwrap().format_chars(0..12, |f| f.over.font_family = Some("No Such Family".into()));
        let cache = Cache::new();
        let cs = cache.get(&d, sid, None);
        assert!(cs.styles.iter().any(|s| s.missing_font));
        let l = &cs.frames[0].lines[0];
        // A point inside the line box but between glyph strokes: the space after "Missing".
        let g = l.glyphs.iter().find(|g| g.byte == 7).unwrap();
        let (x, y) = ((g.x + g.adv / 2.0) as u32, (l.baseline - l.ascent * 0.5) as u32);
        let mut r = Renderer::new();
        r.threads = 0;
        let plain = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        let shown = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { highlight_missing_fonts: true, ..Default::default() }).unwrap();
        assert_eq!(plain.pixel(x, y)[1], 255, "no highlight in output");
        let p = shown.pixel(x, y);
        assert!(p[0] > 240 && p[1] < 200, "pink on screen: {p:?}");
    }

    #[test]
    fn renders_tables() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 400.0), lid, "", ParaFormat::default()).unwrap();
        let mut t = designcraft_doc::Table::new(5, 3, 3, 1, 0, 300.0);
        for c in 0..3 {
            let cell = t.cell_mut(0, c).unwrap();
            cell.fill = "C=100 M=0 Y=0 K=0".into();
            cell.text.insert(0, "Head");
            t.cell_mut(2, c).unwrap().text.insert(0, "WWWWWWWW");
        }
        d.story_mut(sid).unwrap().insert_table(0, t);
        let cache = Cache::new();
        let cs = cache.get(&d, sid, None);
        let tf = &cs.frames[0].tables[0];
        let head = tf.cell(0, 1).unwrap().rect;
        let body = tf.cell(2, 0).unwrap().rect;
        let mut r = Renderer::new();
        r.threads = 0;
        let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        // Header fill (cyan) near the right edge of the middle header cell.
        let c = img.pixel((head.x1 - 3.0) as u32, (head.y0 + 3.0) as u32);
        assert!(c[0] < 80 && c[2] > 150, "{c:?}");
        // The table border (black) on the left edge.
        let e = img.pixel(body.x0.round() as u32, body.center().y as u32);
        assert!(e[0] < 200, "{e:?}");
        // Cell text drew dark pixels inside the body cell.
        let dark = ((body.x0 + 5.0) as u32..(body.x1 - 2.0) as u32)
            .flat_map(|x| ((body.y0 + 2.0) as u32..(body.y1 - 2.0) as u32).map(move |y| (x, y)))
            .filter(|&(x, y)| img.pixel(x, y)[0] < 100)
            .count();
        assert!(dark > 30, "cell text pixels: {dark}");
    }
}
