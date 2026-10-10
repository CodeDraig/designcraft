//! Reflowable EPUB content, navigation, and package assembly.

use super::{Document, EpubError, EpubOptions, EpubReport, esc, slug};
use std::fmt::Write as _;
use std::io::Write as _;

/// Export the document as a reflowable EPUB 3.
pub fn export_epub(doc: &Document, opts: &EpubOptions) -> Result<Vec<u8>, EpubError> {
    export_epub_with_report(doc, opts).map(|r| r.bytes)
}

pub fn export_epub_with_report(doc: &Document, opts: &EpubOptions) -> Result<EpubReport, EpubError> {
    export_epub_documents(&super::semantic::documents(doc)?, opts)
}

pub fn export_epub_documents(docs: &[Document], opts: &EpubOptions) -> Result<EpubReport, EpubError> {
    let doc = docs.first().ok_or_else(|| EpubError("no content to export".into()))?;
    let io = |e: std::io::Error| EpubError(e.to_string());
    let zerr = |e: zip::result::ZipError| EpubError(e.to_string());
    let title = opts.title.clone().unwrap_or_else(|| doc.publication.as_ref().map_or(&doc.title, |v| &v.root.title).clone());
    let id = opts.identifier.clone().unwrap_or_else(|| format!("urn:designcraft:{}", slug(&title)));
    let rendered = super::semantic::build(docs, false)?;
    let (body, images, toc) = (&rendered.text, &rendered.assets, &rendered.toc);
    let lang = esc(&opts.language);
    let chapter = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/></head>\n<body>\n{body}</body>\n</html>\n",
        esc(&title)
    );
    let mut nav_items = String::new();
    for (a, label) in toc {
        let _ = write!(nav_items, "<li><a href=\"content.xhtml#{a}\">{}</a></li>", esc(label));
    }
    if nav_items.is_empty() {
        nav_items = format!("<li><a href=\"content.xhtml\">{}</a></li>", esc(&title));
    }
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title></head>\n<body><nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol>{nav_items}</ol></nav></body>\n</html>\n",
        esc(&title)
    );
    let mut manifest = String::from(
        "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n<item id=\"content\" href=\"content.xhtml\" media-type=\"application/xhtml+xml\"/>\n",
    );
    for (k, (name, asset)) in images.iter().enumerate() {
        let mime = &asset.mime;
        let _ = writeln!(manifest, "<item id=\"img{k}\" href=\"{name}\" media-type=\"{mime}\"/>");
    }
    // Cover: an image page first in reading order.
    if opts.cover.is_some() {
        manifest.push_str("<item id=\"cover-img\" href=\"images/cover.png\" media-type=\"image/png\" properties=\"cover-image\"/>\n<item id=\"cover\" href=\"cover.xhtml\" media-type=\"application/xhtml+xml\"/>\n");
    }
    let cover_spine = if opts.cover.is_some() { "<itemref idref=\"cover\"/>\n" } else { "" };
    let author = opts.author.as_deref().map(|a| format!("<dc:creator>{}</dc:creator>", esc(a))).unwrap_or_default();
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>{lang}</dc:language>{author}\n<meta property=\"dcterms:modified\">2026-01-01T00:00:00Z</meta>\n<meta name=\"generator\" content=\"DesignCraft\"/>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{cover_spine}<itemref idref=\"content\"/>\n</spine>\n</package>\n",
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
        for (name, data) in [
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/nav.xhtml", nav.as_bytes()),
            ("OEBPS/style.css", rendered.css.as_bytes()),
            ("OEBPS/content.xhtml", chapter.as_bytes()),
        ] {
            z.start_file(name, deflate).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        for (name, asset) in images {
            let data = &asset.data;
            z.start_file(format!("OEBPS/{name}"), stored).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        if let Some(c) = &opts.cover {
            z.start_file("OEBPS/images/cover.png", stored).map_err(zerr)?;
            z.write_all(c).map_err(io)?;
            let xhtml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><style>body{{margin:0;text-align:center}} img{{max-width:100%;max-height:100vh}}</style></head>\n<body epub:type=\"cover\"><img src=\"images/cover.png\" alt=\"{}\"/></body>\n</html>\n",
                esc(&title),
                esc(&title)
            );
            z.start_file("OEBPS/cover.xhtml", deflate).map_err(zerr)?;
            z.write_all(xhtml.as_bytes()).map_err(io)?;
        }
        z.finish().map_err(zerr)?;
    }
    Ok(EpubReport { bytes: buf.into_inner(), warnings: rendered.warnings })
}
