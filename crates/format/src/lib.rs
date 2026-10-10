//! The native `.designcraft` format: a zip archive holding
//!
//! - `mimetype` (stored first, uncompressed): `application/vnd.designcraft+zip`
//! - `document.json`: the [`Document`] (pretty JSON, documented by the serde model in `designcraft-doc`)
//! - `sources/<id>.xml`: unchanged TEI sources referenced by the document (required)
//! - `assets/<id>.<ext>`: embedded placed files, byte-for-byte
//! - `meta.json`: format version and generator
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use designcraft_doc::{AssetId, Document};
use serde_json::{Value, json};

mod publication;

pub const MIME: &str = "application/vnd.designcraft+zip";
pub const VERSION: u32 = 3;
/// Standalone documents without structured apparatus retain the version 2 representation.
/// A document containing preserved reading structure requires version 3, as does a publication.
pub const DOCUMENT_VERSION: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a DesignCraft document: {0}")]
    NotOurs(String),
    #[error("document is from a newer DesignCraft (format {0}); please update")]
    TooNew(u32),
    #[error("{0}")]
    Io(String),
}

fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/tiff" => "tif",
        "application/pdf" => "pdf",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

/// Serialize a document to `.designcraft` bytes.
pub fn save(doc: &Document) -> Result<Vec<u8>, FormatError> {
    if doc.publication.is_some() {
        return publication::save(doc);
    }
    if doc.stories.values().any(|st| !designcraft_doc::publication::references::field_ids(&st.content).is_empty()) {
        return Err(FormatError::Io("publication fields require a version 3 publication archive".into()));
    }
    let io = |e: zip::result::ZipError| FormatError::Io(e.to_string());
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).map_err(io)?;
        z.write_all(MIME.as_bytes()).map_err(|e| FormatError::Io(e.to_string()))?;
        z.start_file("meta.json", deflate).map_err(io)?;
        let structured = doc.stories.values().any(|st| {
            let mut found = false;
            let mut st = (**st).clone();
            st.for_each_text_mut(&mut |s| {
                found |= s.notes.iter().chain(&s.endnotes).any(|n| n.apparatus.is_some() || n.annotation.is_some());
            });
            found
        });
        let meta = json!({"format": "designcraft", "kind":"document", "version": if structured {VERSION}else{DOCUMENT_VERSION}, "generator": concat!("DesignCraft ", env!("CARGO_PKG_VERSION"))});
        z.write_all(meta.to_string().as_bytes()).map_err(|e| FormatError::Io(e.to_string()))?;
        z.start_file("document.json", deflate).map_err(io)?;
        let json = serde_json::to_vec_pretty(doc).map_err(|e| FormatError::Io(e.to_string()))?;
        z.write_all(&json).map_err(|e| FormatError::Io(e.to_string()))?;
        for source in &doc.tei_sources {
            if source.bytes.len() != source.byte_length || source.bytes.is_empty() {
                return Err(FormatError::Io(format!("missing preserved TEI source {}", source.id)));
            }
            z.start_file(format!("sources/{}.xml", source.id), deflate).map_err(io)?;
            z.write_all(&source.bytes).map_err(|e| FormatError::Io(e.to_string()))?;
        }
        for (id, a) in &doc.assets {
            if a.data.is_empty() {
                continue;
            }
            // Already-compressed images are stored as-is.
            let opts = if matches!(a.mime.as_str(), "image/png" | "image/jpeg" | "image/gif" | "image/webp") { stored } else { deflate };
            z.start_file(format!("assets/{}.{}", id.0, ext_for(&a.mime)), opts).map_err(io)?;
            z.write_all(&a.data).map_err(|e| FormatError::Io(e.to_string()))?;
        }
        z.finish().map_err(io)?;
    }
    Ok(buf.into_inner())
}

/// Read the native `.designcraft` ZIP archive.
pub fn load(bytes: &[u8]) -> Result<Document, FormatError> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    let read = |z: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str| -> Option<Vec<u8>> {
        let mut f = z.by_name(name).ok()?;
        let mut v = Vec::new();
        f.read_to_end(&mut v).ok()?;
        Some(v)
    };
    if let Some(meta) = read(&mut z, "meta.json")
        && let Ok(m) = serde_json::from_slice::<Value>(&meta)
        && let Some(v) = m.get("version").and_then(Value::as_u64)
    {
        if v > u64::from(VERSION) {
            return Err(FormatError::TooNew(u32::try_from(v).unwrap_or(u32::MAX)));
        }
        if v == 3 && m.get("kind").and_then(Value::as_str) != Some("document") {
            return publication::load(z);
        }
    }
    let json = read(&mut z, "document.json").ok_or_else(|| FormatError::NotOurs("missing document.json".into()))?;
    let mut doc: Document = serde_json::from_slice(&json).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    let mut source_ids = std::collections::BTreeSet::new();
    for source in &mut doc.tei_sources {
        if !source_ids.insert(source.id) || source.byte_length == 0 || source.byte_length > 64 * 1024 * 1024 {
            return Err(FormatError::NotOurs("invalid preserved TEI source record".into()));
        }
        let name = format!("sources/{}.xml", source.id);
        let f = z.by_name(&name).map_err(|_| FormatError::NotOurs(format!("missing {name}")))?;
        let mut bytes = Vec::new();
        f.take(source.byte_length as u64 + 1).read_to_end(&mut bytes).map_err(|e| FormatError::Io(e.to_string()))?;
        if bytes.len() != source.byte_length {
            return Err(FormatError::NotOurs(format!("incorrect length for {name}")));
        }
        Arc::make_mut(source).bytes = Arc::new(bytes);
    }
    let names: Vec<String> = z.file_names().map(str::to_string).collect();
    for name in names {
        let Some(stem) = name.strip_prefix("assets/") else { continue };
        let Some(id) = stem.split('.').next().and_then(|s| s.parse::<u64>().ok()) else { continue };
        if let (Some(data), Some(a)) = (read(&mut z, &name), doc.assets.get_mut(&AssetId(id))) {
            Arc::make_mut(a).data = Arc::new(data);
        }
    }
    doc.check().map_err(|e| FormatError::NotOurs(e.to_string()))?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::geom::Rect;
    use designcraft_doc::{Asset, ParaFormat, SpreadRef};

    #[test]
    fn roundtrip_with_assets() {
        let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        let lid = d.default_layer();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 200.0, 200.0), lid, "héllo\nworld", ParaFormat::default()).unwrap();
        let aid = AssetId(d.alloc());
        d.assets.insert(
            aid,
            Arc::new(Asset {
                page: 0,
                id: aid,
                name: "x.png".into(),
                mime: "image/png".into(),
                link: None,
                data: Arc::new(vec![1, 2, 3, 4]),
                pixels: Some((1, 1)),
            }),
        );
        let bytes = save(&d).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let back = load(&bytes).unwrap();
        assert_eq!(back.page_count(), 3);
        assert_eq!(*back.assets[&aid].data, vec![1, 2, 3, 4]);
        assert_eq!(back.stories, d.stories);
    }

    #[test]
    fn rejects_single_file_json() {
        let d = Document::new(&NewDocument::default());
        assert!(matches!(load(&serde_json::to_vec(&d).unwrap()), Err(FormatError::NotOurs(_))));
    }

    #[test]
    fn story_direction_and_glyph_fallback_roundtrip() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 200.0, 400.0), lid, "縦書き", ParaFormat::default()).unwrap();
        for vertical in [false, true] {
            for fallback in [false, true] {
                d.story_mut(sid).unwrap().vertical = vertical;
                d.settings.glyph_fallback = fallback;
                let back = load(&save(&d).unwrap()).unwrap();
                assert_eq!(back.stories[&sid].vertical, vertical);
                assert_eq!(back.settings.glyph_fallback, fallback);
            }
        }
    }

    #[test]
    fn rejects_garbage_and_newer_versions() {
        assert!(load(b"nope").is_err());
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file("meta.json", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(br#"{"version": 99}"#).unwrap();
            z.finish().unwrap();
        }
        assert!(matches!(load(&buf.into_inner()), Err(FormatError::TooNew(99))));
    }
}

#[cfg(test)]
mod tei_tests {
    use super::*;
    use designcraft_doc::tei::{TeiOptions, TeiSource};
    fn document() -> Document {
        let mut d = Document::new(&Default::default());
        let bytes = Arc::new(b"<unchanged>\r\n &amp; </unchanged>".to_vec());
        d.tei_sources.push(Arc::new(TeiSource {
            id: 10,
            name: "original.xml".into(),
            byte_length: bytes.len(),
            bytes,
            metadata: vec![],
            options: TeiOptions::default(),
            import_settings: Default::default(),
            diagnostics: vec![],
            navigation: vec![],
            resources: vec![],
        }));
        d
    }
    fn archive(d: &Document, version: u32, source: Option<&[u8]>) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        z.start_file("meta.json", options).unwrap();
        z.write_all(json!({"version":version}).to_string().as_bytes()).unwrap();
        z.start_file("document.json", options).unwrap();
        z.write_all(&serde_json::to_vec(d).unwrap()).unwrap();
        if let Some(bytes) = source {
            z.start_file("sources/10.xml", options).unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap().into_inner()
    }
    #[test]
    fn originals_are_exact_and_required() {
        let d = document();
        let bytes = save(&d).unwrap();
        assert_eq!(load(&bytes).unwrap(), d);
        assert!(load(&archive(&d, 2, None)).is_err());
        assert!(load(&archive(&d, 2, Some(b"short"))).is_err());
        let mut missing = d.clone();
        Arc::make_mut(&mut missing.tei_sources[0]).bytes = Arc::new(Vec::new());
        assert!(save(&missing).is_err());
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut meta = String::new();
        z.by_name("meta.json").unwrap().read_to_string(&mut meta).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&meta).unwrap()["version"], 2);
    }
    #[test]
    fn version_one_remains_readable() {
        let d = Document::new(&Default::default());
        assert_eq!(load(&archive(&d, 1, None)).unwrap(), d);
    }
}
