//! Placed PDF, SVG, and raster image embedding.

use super::{Arc, AssetId, BezPath, Exporter, Fill, Image, PdfError, PdfVersion, Rect, Result, Size, Surface, rgb, tf, to_path};
use designcraft_geom::kurbo::Shape as _;

impl Exporter<'_> {
    /// A placed PDF as krilla embeds it. krilla refuses a PDF newer than the file it writes (the
    /// export fails at the end with `VersionMismatch(Pdf17)`), and PDF/X-4 is PDF 1.6 while
    /// VectorCraft and most tools write 1.7. PDF 1.7 changed nothing in how a page draws, so a 1.7
    /// file goes in as 1.6 (its header relabelled, as `qpdf --force-version=1.6` does). A newer
    /// file is left out and the export fails naming it ([`Exporter::check_placed_versions`]).
    pub(super) fn placed_pdf(&mut self, asset: &designcraft_doc::Asset) -> Option<krilla::pdf::PdfDocument> {
        use hayro_syntax::PdfVersion as V;
        let Ok(pdf) = krilla::pdf::Pdf::new(asset.data.clone()) else {
            self.warn(format!("{}: can't read the placed PDF", asset.name));
            return None;
        };
        let max = match self.version {
            PdfVersion::Pdf14 => V::Pdf14,
            PdfVersion::Pdf15 => V::Pdf15,
            PdfVersion::Pdf16 => V::Pdf16,
            PdfVersion::Pdf17 => V::Pdf17,
            PdfVersion::Pdf20 => V::Pdf20,
        };
        let have = pdf.version();
        if have <= max {
            return Some(krilla::pdf::PdfDocument::new(Arc::new(pdf)));
        }
        if have == V::Pdf17
            && max == V::Pdf16
            && let Some(bytes) = relabel_pdf_header(&asset.data, b"%PDF-1.7", b"%PDF-1.6")
            && let Ok(pdf) = krilla::pdf::Pdf::new(Arc::new(bytes))
            && pdf.version() <= max
        {
            self.warn(format!("{}: a PDF 1.7 placed in a PDF 1.6 file, embedded as PDF 1.6", asset.name));
            return Some(krilla::pdf::PdfDocument::new(Arc::new(pdf)));
        }
        let v = match have {
            V::Pdf17 => "PDF 1.7",
            V::Pdf20 => "PDF 2.0",
            _ => "a newer PDF",
        };
        self.too_new.push(format!("{} is {v}", asset.name));
        None
    }

    /// Fail with the names of placed PDFs too new for this export (see [`Exporter::placed_pdf`]).
    pub(super) fn check_placed_versions(&self) -> Result<()> {
        if self.too_new.is_empty() {
            return Ok(());
        }
        let v = self.version.as_str();
        Err(PdfError::Write(format!(
            "a placed PDF is newer than the {v} this export writes ({}): save it as {v} or older and place it again",
            self.too_new.join(", ")
        )))
    }

    pub(super) fn load_image(&mut self, id: AssetId) -> Option<Image> {
        let asset = self.doc.assets.get(&id)?.clone();
        let key = (Arc::as_ptr(&asset.data) as usize, asset.page);
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let data = asset.data.clone();
        let fmt = image::guess_format(&data).ok();
        let img = match fmt {
            Some(image::ImageFormat::Jpeg) => Image::from_jpeg(data.clone().into(), true).ok(),
            // A CMYK TIFF keeps its ink values (`image` decodes it to RGB, and 100% K would print
            // as four-colour black). PDF/A exports are RGB only.
            Some(image::ImageFormat::Tiff) if !self.rgb_only && designcraft_images::is_cmyk_tiff(&data) => match cmyk_tiff(&data) {
                Some(img) => Some(img),
                None => {
                    self.warn(format!(
                        "{}: converted to RGB (a CMYK TIFF that is planar, has premultiplied alpha, uses other inks or is over 256 MiB)",
                        asset.name
                    ));
                    lossless(&data, fmt)
                }
            },
            _ if self.opts.compress_images => recompress(&data).or_else(|| lossless(&data, fmt)),
            _ => lossless(&data, fmt),
        };
        if img.is_none() {
            self.warn(format!("image `{}` could not be decoded and was skipped", asset.name));
        }
        self.images.insert(key, img.clone());
        img
    }

    pub(crate) fn graphic(&mut self, s: &mut Surface, g: &designcraft_doc::Graphic) {
        // Video / sound without a poster: a dark frame with a play mark.
        if let Some(asset) = self.doc.assets.get(&g.asset)
            && designcraft_doc::media_kind(&asset.mime).is_some()
        {
            let (w, h) = g.size;
            let (cx, cy, r) = (w / 2.0, h / 2.0, w.min(h) * 0.18);
            let mut tri = BezPath::new();
            tri.move_to((cx - r * 0.6, cy - r));
            tri.line_to((cx + r, cy));
            tri.line_to((cx - r * 0.6, cy + r));
            tri.close_path();
            s.push_transform(&tf(g.xf));
            s.set_stroke(None);
            for (bp, c) in [(Rect::new(0.0, 0.0, w, h).to_path(0.1), rgb::Color::new(48, 48, 52)), (tri, rgb::Color::new(235, 235, 235))] {
                if let Some(p) = to_path(&bp) {
                    s.set_fill(Some(Fill { paint: c.into(), ..Default::default() }));
                    s.draw_path(&p);
                }
            }
            s.set_fill(None);
            s.pop();
            return;
        }
        // Placed PDFs go in as vectors (the page as a form XObject).
        let doc = self.doc.clone();
        if let Some(asset) = doc.assets.get(&g.asset)
            && asset.data.starts_with(b"%PDF")
        {
            let Some(size) = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32) else { return };
            let key = (Arc::as_ptr(&asset.data) as usize, asset.page);
            let placed = match self.pdfs.get(&key) {
                Some(p) => p.clone(),
                None => {
                    let p = self.placed_pdf(asset);
                    self.pdfs.insert(key, p.clone());
                    p
                }
            };
            if let Some(placed) = placed {
                s.push_transform(&tf(g.xf));
                s.draw_pdf_page(&placed, size, asset.page as usize);
                s.pop();
            }
            return;
        }
        // Placed SVGs go in as vectors too.
        if let Some(asset) = self.doc.assets.get(&g.asset)
            && designcraft_images::is_svg(&asset.data)
        {
            let Some(size) = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32) else { return };
            let key = (Arc::as_ptr(&asset.data) as usize, asset.page);
            let tree = self.svgs.entry(key).or_insert_with(|| designcraft_images::svg_tree(&asset.data).map(Arc::new)).clone();
            match tree {
                Some(tree) => {
                    use krilla_svg::SurfaceExt;
                    s.push_transform(&tf(g.xf));
                    if s.draw_svg(&tree, size, krilla_svg::SvgSettings::default()).is_none() {
                        self.warn(format!("{}: parts of the placed SVG could not be drawn", asset.name));
                    }
                    s.pop();
                }
                None => self.warn(format!("{}: can't read the placed SVG", asset.name)),
            }
            return;
        }
        let Some(img) = self.load_image(g.asset) else { return };
        let Some(size) = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32) else { return };
        s.push_transform(&tf(g.xf));
        s.draw_image(img, size);
        s.pop();
    }
}

/// `data` with its `%PDF-x.y` header (within the first 1024 bytes, where readers look for it)
/// changed from `from` to `to`, both the same length. `None` when the header isn't `from`.
fn relabel_pdf_header(data: &[u8], from: &[u8], to: &[u8]) -> Option<Vec<u8>> {
    if from.len() != to.len() {
        return None;
    }
    let head = data.get(..data.len().min(1024))?;
    let at = head.windows(from.len()).position(|w| w == from)?;
    let mut out = data.to_vec();
    out.get_mut(at..at + to.len())?.copy_from_slice(to);
    Some(out)
}

fn lossless(data: &Arc<Vec<u8>>, fmt: Option<image::ImageFormat>) -> Option<Image> {
    let direct = match fmt {
        Some(image::ImageFormat::Png) => Image::from_png(data.clone().into(), true).ok(),
        Some(image::ImageFormat::Gif) => Image::from_gif(data.clone().into(), true).ok(),
        Some(image::ImageFormat::WebP) => Image::from_webp(data.clone().into(), true).ok(),
        _ => None,
    };
    direct.or_else(|| {
        // TIFF, BMP, PSD composites…
        let rgba = designcraft_images::decode_rgba(data)?;
        let (w, h) = rgba.dimensions();
        Some(Image::from_rgba8(rgba.into_raw(), w, h))
    })
}

/// A CMYK raster as a krilla image: its samples are written unchanged as DeviceCMYK, like a
/// CMYK JPEG's. An embedded ICC profile is left out, as it is for JPEGs: the inks are meant for
/// the output intent's press, and tagging them with another CMYK profile would let a RIP
/// convert 100% K to four-colour black.
#[derive(Clone, Hash)]
struct CmykImage(Arc<designcraft_images::CmykRaster>);

impl krilla::image::CustomImage for CmykImage {
    fn color_channel(&self) -> &[u8] {
        &self.0.cmyk
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        self.0.alpha.as_deref()
    }
    fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
        krilla::image::BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }
    fn color_space(&self) -> krilla::image::ImageColorspace {
        krilla::image::ImageColorspace::Cmyk
    }
}

/// A CMYK TIFF as a DeviceCMYK image. `None` when it can't be read as CMYK.
fn cmyk_tiff(data: &[u8]) -> Option<Image> {
    let r = designcraft_images::decode_cmyk_tiff(data)?;
    // krilla panics when the channel lengths don't match the size.
    let pixels = usize::try_from(r.width).ok()?.checked_mul(usize::try_from(r.height).ok()?)?;
    if r.cmyk.len() != pixels.checked_mul(4)? || r.alpha.as_ref().is_some_and(|a| a.len() != pixels) {
        return None;
    }
    Image::from_custom(CmykImage(Arc::new(r)), true).ok()
}

/// Opaque raster → JPEG (quality 90). `None` when the image has transparency or can't be decoded.
fn recompress(data: &[u8]) -> Option<Image> {
    let img = image::load_from_memory(data).ok()?;
    if img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p[3] < 255) {
        return None;
    }
    let rgb = img.to_rgb8();
    let mut buf = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90);
    image::ImageEncoder::write_image(enc, rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).ok()?;
    Image::from_jpeg(buf.into(), true).ok()
}
