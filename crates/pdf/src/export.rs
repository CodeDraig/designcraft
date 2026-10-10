//! Document → PDF. The item walk mirrors `designcraft-render`'s `draw_spread` / `draw_item`.

mod publication;
pub use publication::{PublicationNavigation, navigation_documents};
mod sheets;
pub use sheets::sheet_spreads;
use sheets::sheets;
pub(crate) use sheets::{MARK_LEN, Sheet, mark_start};
mod merge;
pub use merge::merge_pdfs;
mod booklet;
pub use booklet::{BookletKind, BookletOptions, booklet_pairs, export_booklet};
mod paint;
pub(crate) use paint::{registration, solid_fill};
mod items;
mod tagging;
use tagging::TagEntry;
mod graphics;

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_color::swatch::{ColorType, SwatchValue};
use designcraft_color::{BlendMode as DcBlend, Color, GradientKind};
use designcraft_compose::Cache;
use designcraft_doc::{AssetId, Content, Document, Item, PageSide, Spread, StrokeAlign, StrokeType};
use designcraft_geom::kurbo::{PathEl, Shape as _};
use designcraft_geom::{Affine, BezPath, Rect, Vec2, corners};
use krilla::color::{cmyk, luma, rgb};
use krilla::configure::{Archival, ConfigurationBuilder, PdfVersion};
use krilla::geom::{Path, PathBuilder, Size, Transform};
use krilla::image::Image;
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LinearGradient, RadialGradient, SpreadMethod, Stop, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::tagging::{Artifact, ArtifactType, ContentTag, Identifier, Tag, TagGroup, TagTree};

use crate::{ExportReport, PdfError, PdfOptions, Result, Standard};

/// Export `doc` as PDF bytes.
pub fn export_pdf(doc: &Document, cache: &Cache, opts: &PdfOptions) -> Result<Vec<u8>> {
    export_pdf_with_report(doc, cache, opts).map(|r| r.bytes)
}

/// Like [`export_pdf`], also returning warnings about approximated or dropped features.
pub fn export_pdf_with_report(doc: &Document, cache: &Cache, opts: &PdfOptions) -> Result<ExportReport> {
    export_documents_with_report(std::slice::from_ref(doc), cache, opts)
}

/// Write all components into one PDF with a shared destination table. Page selections are
/// publication positions; components remain in publication reading order.
pub fn export_documents_with_report(documents: &[Document], cache: &Cache, opts: &PdfOptions) -> Result<ExportReport> {
    let doc = documents.first().ok_or(PdfError::NoPages)?;
    let pages = publication::PublicationSheets::new(documents, cache, opts)?;
    let warnings = Vec::new();
    let (version, archival) = match opts.standard {
        Standard::None => (PdfVersion::Pdf17, None),
        Standard::PdfX4 => (PdfVersion::Pdf16, None),
        Standard::PdfA2b => (PdfVersion::Pdf17, Some(Archival::A2_B)),
    };
    let mut cb = ConfigurationBuilder::new().with_version(version);
    if let Some(a) = archival {
        cb = cb.with_archival_validator(a);
    }
    let configuration = cb.finish().map_err(|e| PdfError::Write(format!("{e:?}")))?;
    let settings = krilla::SerializeSettings { compress_content_streams: opts.compress, configuration, ..Default::default() };
    let mut pdf = krilla::Document::new_with(settings);

    let title = opts.title.clone().unwrap_or_else(|| doc.publication.as_ref().map_or(&doc.title, |v| &v.root.title).clone());
    let mut meta = Metadata::new().creator("DesignCraft".into()).producer("DesignCraft".into());
    if !title.is_empty() {
        meta = meta.title(title.clone());
    }
    if let Some(a) = &opts.author {
        meta = meta.authors(vec![a.clone()]);
    }
    let created = opts.created.or_else(now_unix);
    if let Some(t) = created {
        let c = civil(t);
        meta = meta.creation_date(
            krilla::metadata::DateTime::new(c.0.clamp(0, 9999) as u16)
                .month(c.1)
                .day(c.2)
                .hour(c.3)
                .minute(c.4)
                .second(c.5)
                .utc_offset_hour(0)
                .utc_offset_minute(0),
        );
    }
    pdf.set_metadata(meta);

    let mut ex = Exporter {
        doc: DocumentContext::Borrowed(doc),
        cache,
        opts,
        clip: Rect::ZERO,
        warnings,
        images: HashMap::new(),
        pdfs: HashMap::new(),
        version,
        too_new: Vec::new(),
        svgs: HashMap::new(),
        fonts: HashMap::new(),
        reverse_cmaps: HashMap::new(),
        rgb_only: archival.is_some(),
        tags: Vec::new(),
        story_tags: HashMap::new(),
        tag_story: None,
        para_tags: HashMap::new(),
    };
    if ex.rgb_only {
        ex.warn("PDF/A: CMYK colours were converted to RGB (no CMYK output intent profile is available yet)");
    }
    if let Some(o) = pages.outline(cache) {
        pdf.set_outline(o);
    }
    let mut tags = TagTree::new();
    for (part, doc) in documents.iter().enumerate() {
        ex.doc = DocumentContext::Borrowed(doc);
        let sheets = &pages.sheets[part];
        for (sheet_idx, sh) in sheets.iter().enumerate() {
            let size = Size::from_wh(sh.media.width().max(1.0) as f32, sh.media.height().max(1.0) as f32).ok_or(PdfError::NoPages)?;
            let local = |r: Rect| {
                krilla::geom::Rect::from_ltrb(
                    (r.x0 - sh.media.x0) as f32,
                    (r.y0 - sh.media.y0) as f32,
                    (r.x1 - sh.media.x0) as f32,
                    (r.y1 - sh.media.y0) as f32,
                )
            };
            let settings = PageSettings::new(size).with_trim_box(local(sh.trim)).with_bleed_box(local(sh.bleed));
            let mut page = pdf.start_page_with(settings);
            let mut s = page.surface();
            s.push_transform(&tf(Affine::translate(-sh.media.origin().to_vec2())));
            ex.clip = sh.bleed;
            if let Some(clip) = to_path(&sh.bleed.to_path(0.1)) {
                s.push_clip_path(&clip, &FillRule::NonZero);
                ex.spread(&mut s, sh.spread);
                s.pop();
            }
            if opts.tagged {
                s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Page, None)));
            }
            ex.marks(&mut s, sh, &title, created);
            if opts.tagged {
                s.end_tagged();
            }
            s.pop();
            s.finish();
            for a in crate::links::annotations_mapped(
                doc,
                cache,
                sheets,
                sheet_idx,
                &|owner, p| pages.part(owner, part).and_then(|part| pages.page(part, p)),
                &|owner, id| pages.part(owner, part).and_then(|part| pages.anchor(part, id)),
                &|id| pages.entity(part, id),
                &mut ex.warnings,
            ) {
                if let Some(a) = a.annotation() {
                    page.add_annotation(a);
                }
            }
            page.finish();
        }
        if opts.tagged {
            for group in ex.tag_groups() {
                tags.push(group);
            }
        }
    }
    if opts.tagged {
        pdf.set_tag_tree(tags);
    }
    ex.check_placed_versions()?;
    let mut bytes = pdf.finish().map_err(|e| PdfError::Write(format!("{e:?}")))?;
    // Transparency blends in the document's blend space: CMYK for print (and always in PDF/X).
    let mut warnings = ex.warnings;
    if !ex.rgb_only && (doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk || opts.standard == Standard::PdfX4) {
        // The rewrite matches krilla's exact output; in PDF/X-4, `check_pdfx4` reports any
        // group that got past it, with its page.
        crate::pdfx::cmyk_group_spaces(&mut bytes);
    }
    // Form fields (Buttons and Forms) and, for interactive PDF, video and sound; not in PDF/X.
    let mut fields = Vec::new();
    let mut media = Vec::new();
    for (part, doc) in documents.iter().enumerate() {
        let mut f = crate::forms::collect(doc, &pages.sheets[part]);
        for (page, _, field) in &mut f {
            *page += pages.offsets[part];
            if documents.len() > 1 {
                field.name = format!("component-{part}.{}", field.name);
            }
        }
        fields.extend(f);
        if opts.media {
            let mut m = crate::forms::collect_media(doc, &pages.sheets[part]);
            for (page, ..) in &mut m {
                *page += pages.offsets[part];
            }
            media.extend(m);
        }
    }
    if !fields.is_empty() || !media.is_empty() {
        if opts.standard == Standard::PdfX4 {
            warnings.push("PDF/X-4: form fields and media were left out".into());
        } else {
            match crate::forms::add_fields(&bytes, &fields, &media) {
                Some(b) => bytes = b,
                None => warnings.push("form fields and media couldn't be added to this PDF".into()),
            }
        }
    }
    if opts.standard == Standard::PdfX4 {
        // Output intent and PDF/X identification, then check the result.
        match crate::pdfx::make_pdfx4(&bytes, &title) {
            Some(b) => bytes = b,
            None => warnings.push("PDF/X-4: the output intent couldn't be added".into()),
        }
        for issue in crate::pdfx::check_pdfx4(&bytes) {
            warnings.push(format!("PDF/X-4: {issue}"));
        }
    }
    warnings.dedup();
    Ok(ExportReport { bytes, pages: pages.count, warnings })
}

#[cfg(not(target_arch = "wasm32"))]
fn now_unix() -> Option<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}
#[cfg(target_arch = "wasm32")]
fn now_unix() -> Option<i64> {
    None
}

/// Unix seconds (UTC) → (year, month, day, hour, minute, second), proleptic Gregorian.
pub(crate) fn civil(t: i64) -> (i64, u8, u8, u8, u8, u8) {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month as u8, day as u8, (secs / 3600) as u8, (secs / 60 % 60) as u8, (secs % 60) as u8)
}

#[derive(Clone)]
pub(crate) enum DocumentContext<'a> {
    Borrowed(&'a Document),
    Composed(Arc<Document>),
}
impl std::ops::Deref for DocumentContext<'_> {
    type Target = Document;
    fn deref(&self) -> &Document {
        match self {
            Self::Borrowed(d) => d,
            Self::Composed(d) => d,
        }
    }
}

pub(crate) struct Exporter<'a> {
    pub doc: DocumentContext<'a>,
    pub cache: &'a Cache,
    pub opts: &'a PdfOptions,
    /// Current sheet's visible area (spread space) for culling.
    pub clip: Rect,
    pub warnings: Vec<String>,
    images: HashMap<(usize, u32), Option<Image>>,
    /// Placed PDFs (embedded as vector pages).
    pdfs: HashMap<(usize, u32), Option<krilla::pdf::PdfDocument>>,
    /// The PDF version written; placed PDFs must not be newer.
    version: PdfVersion,
    /// Placed PDFs too new to embed in this version ("name is PDF 2.0"): the export fails.
    too_new: Vec<String>,
    /// Parsed placed SVGs.
    svgs: HashMap<(usize, u32), Option<Arc<designcraft_images::usvg::Tree>>>,
    pub fonts: HashMap<u32, Option<krilla::text::Font>>,
    pub reverse_cmaps: HashMap<u32, Arc<HashMap<u32, char>>>,
    /// Convert CMYK to RGB (PDF/A: krilla needs a CMYK output profile we don't ship yet).
    pub rgb_only: bool,
    /// Tagged PDF: the structure in reading order (stories gather their frames' content).
    tags: Vec<TagEntry>,
    story_tags: HashMap<designcraft_doc::StoryId, Vec<Identifier>>,
    /// While drawing a story's frame for the structure tree: the story (text runs are tagged
    /// per paragraph, everything else in the frame is an artifact).
    pub(crate) tag_story: Option<designcraft_doc::StoryId>,
    /// Marked content of each (story, paragraph index).
    pub(crate) para_tags: HashMap<(designcraft_doc::StoryId, usize), Vec<Identifier>>,
}

pub(crate) fn tf(a: Affine) -> Transform {
    let c = a.as_coeffs();
    Transform::from_row(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32)
}

pub(crate) fn to_path(bp: &BezPath) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for el in bp.elements() {
        match *el {
            PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(a, p) => pb.quad_to(a.x as f32, a.y as f32, p.x as f32, p.y as f32),
            PathEl::CurveTo(a, b, p) => pb.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

pub(crate) fn norm(v: f32) -> NormalizedF32 {
    NormalizedF32::new(if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 }).unwrap_or(NormalizedF32::ONE)
}

impl Exporter<'_> {
    /// A print document, or a PDF/X file (whose output intent is CMYK).
    pub(crate) fn for_print(&self) -> bool {
        self.doc.settings.intent == designcraft_doc::Intent::Print || self.opts.standard == Standard::PdfX4
    }

    pub(crate) fn warn(&mut self, w: impl Into<String>) {
        let w = w.into();
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }
}
