//! Fixed-layout pages, navigation, and EPUB packaging.

use super::{Document, EpubError, EpubOptions, esc, slug};
use std::fmt::Write as _;
use std::io::Write as _;

/// A page of a fixed-layout EPUB: its image (PNG), size in points and text (kept for search,
/// read-aloud and accessibility).
#[derive(Clone, Debug)]
pub struct FixedPage {
    pub png: Vec<u8>,
    pub width: f64,
    pub height: f64,
    pub text: String,
}

/// A pre-paginated (fixed-layout) EPUB: one XHTML page per document page.
pub fn export_fixed_epub(doc: &Document, pages: &[FixedPage], opts: &EpubOptions) -> Result<Vec<u8>, EpubError> {
    let io = |e: std::io::Error| EpubError(e.to_string());
    let zerr = |e: zip::result::ZipError| EpubError(e.to_string());
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let lang = &opts.language;
    let id = opts.identifier.clone().unwrap_or_else(|| format!("urn:designcraft:{}", slug(&title)));
    let mut manifest = String::from("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    let mut spine = String::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut nav_items = String::new();
    for (i, p) in pages.iter().enumerate() {
        let n = i + 1;
        let (w, h) = (p.width.round().max(1.0) as u32, p.height.round().max(1.0) as u32);
        let xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width={w}, height={h}\"/><title>{} — {n}</title>\n<style>body{{margin:0;width:{w}px;height:{h}px;position:relative}} img{{position:absolute;left:0;top:0;width:{w}px;height:{h}px}} .t{{position:absolute;left:0;top:0;width:{w}px;color:transparent;font-size:1px;overflow:hidden}}</style></head>\n<body><img src=\"pages/p{n}.png\" alt=\"\"/><div class=\"t\">{}</div></body>\n</html>\n",
            esc(&title),
            esc(&p.text).replace('\n', "<br/>")
        );
        let _ = writeln!(
            manifest,
            "<item id=\"page{n}\" href=\"page{n}.xhtml\" media-type=\"application/xhtml+xml\"/>\n<item id=\"pimg{n}\" href=\"pages/p{n}.png\" media-type=\"image/png\"{}/>",
            if n == 1 { " properties=\"cover-image\"" } else { "" }
        );
        let _ = writeln!(spine, "<itemref idref=\"page{n}\"/>");
        let _ = write!(nav_items, "<li><a href=\"page{n}.xhtml\">{n}</a></li>");
        files.push((format!("OEBPS/page{n}.xhtml"), xhtml.into_bytes()));
        files.push((format!("OEBPS/pages/p{n}.png"), p.png.clone()));
    }
    let (vw, vh) = pages.first().map_or((612, 792), |p| (p.width.round() as u32, p.height.round() as u32));
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title></head>\n<body><nav epub:type=\"page-list\" id=\"toc\"><h1>Pages</h1><ol>{nav_items}</ol></nav></body>\n</html>\n",
        esc(&title)
    );
    let author = opts.author.as_deref().map(|a| format!("<dc:creator>{}</dc:creator>", esc(a))).unwrap_or_default();
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\" prefix=\"rendition: http://www.idpf.org/vocab/rendition/#\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>{lang}</dc:language>{author}\n<meta property=\"dcterms:modified\">2026-01-01T00:00:00Z</meta>\n<meta property=\"rendition:layout\">pre-paginated</meta>\n<meta property=\"rendition:spread\">auto</meta>\n<meta name=\"original-resolution\" content=\"{vw}x{vh}\"/>\n<meta name=\"generator\" content=\"DesignCraft\"/>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{spine}</spine>\n</package>\n",
        esc(&id),
        esc(&title)
    );
    let container = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>\n";
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).map_err(zerr)?;
        z.write_all(b"application/epub+zip").map_err(io)?;
        for (name, data) in
            [("META-INF/container.xml", container.as_bytes()), ("OEBPS/content.opf", opf.as_bytes()), ("OEBPS/nav.xhtml", nav.as_bytes())]
        {
            z.start_file(name, deflate).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        for (name, data) in &files {
            let o = if name.ends_with(".png") { stored } else { deflate };
            z.start_file(name.as_str(), o).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        z.finish().map_err(zerr)?;
    }
    Ok(buf.into_inner())
}
