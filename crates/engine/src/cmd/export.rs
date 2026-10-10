//! File › Export: Adobe PDF (Print).

mod books;
mod publication;
use books::{export_epub, export_html, export_text, has_story_target};
mod rasterize;
use rasterize::{flatten, rasterize_effects, rasterize_layered, rasterize_where, threaded, transparent};

use designcraft_pdf::{Marks, PdfOptions, Standard};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.exportPdf", "Export PDF…", ["File"], None,
        "{path?, edition?: id (default active), scope?: publication|component (default publication for books), strict?: bool (fail on diagnostics before writing), pages?: \"1-3,5\" | [1,3] (1-based positions; default all), flatten?: high|medium|low|ppi (Transparency Flattener: spreads with transparency are rasterised), spreads?: bool, fullScreen?: bool, bookmarksPanel?: bool, pageLayout?: single|continuous|twoUp|twoUpCover|twoUpContinuous, view?: fitPage|fitWidth|actual, advanceSeconds?: number (interactive PDF), bleed?: bool (document bleed), marks?: bool | {crop?, bleed?, pageInfo?, weight?, offset?}, standard?: \"none\"|\"x4\"|\"a2b\", compressImages?: bool, tagged?: bool (structure tree: stories as paragraphs, figures with alt text), media?: bool (interactive: embed placed video and sound), title?, author?} → {path, bytes, pages, warnings} (no path: {base64, …})",
        has_doc, export_pdf),
        cmd!(noundo "file.exportEpub", "Export EPUB (Reflowable)…", ["File"], None,
        "{path?, edition?, scope?: publication|component, strict?: bool, title?, author?, identifier?, language?: \"en\", cover?: bool (the first page as the cover image), fixedLayout?: bool (pre-paginated: each page as an image with searchable rich text and clickable links)} → {path, bytes, pages, warnings} (no path: {base64, …})",
        has_doc, export_epub),
        cmd!(noundo "file.exportFixedEpub", "Export EPUB (Fixed Layout)…", ["File"], None,
        "{path?, edition?, scope?: publication|component, strict?: bool, title?, author?, identifier?, language?} — pre-paginated EPUB with searchable rich text and clickable page links → {path, bytes, pages, warnings} (no path: {base64, bytes})",
        has_doc, |s, p| {
            export_epub(s, &super::with_param(p, "fixedLayout", serde_json::json!(true)))
        }),
        cmd!(noundo "file.printBooklet", "Print Booklet…", ["File"], None,
        "{path?, type?: saddleStitch|twoUpConsecutive, spaceBetween? (pt)} — printer spreads as PDF (pages imposed in booklet order) → {path, bytes, sheets} (no path: {base64, …})",
        has_doc, print_booklet),
        cmd!(noundo "file.exportHtml", "Export HTML…", ["File"], None,
        "{path?, edition?, scope?: publication|component, strict?: bool, title?, language?} — one self-contained page with semantic text, rich notes, tables, images and references → {path, bytes, warnings} (no path: {text, bytes})",
        has_doc, export_html),
        cmd!(noundo "file.exportText", "Export Text…", ["File"], None,
        "{path?, format?: \"txt\"|\"rtf\"|\"tagged\" (Tagged Text; default from the path, else txt), story?, frame?} — the story being edited or of the selected frame → {path, bytes} (no path: {text, bytes})",
        has_story_target, export_text),
    ]
}

fn print_booklet(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.printBooklet";
    let kind = match str_param(p, "type").unwrap_or("saddleStitch") {
        "saddleStitch" => designcraft_pdf::BookletKind::SaddleStitch,
        "twoUpConsecutive" | "twoUp" => designcraft_pdf::BookletKind::TwoUpConsecutive,
        t => return Err(bad(ID, format!("unknown type `{t}` (saddleStitch, twoUpConsecutive)"))),
    };
    let opts = designcraft_pdf::BookletOptions { kind, space_between: p.get("spaceBetween").and_then(Value::as_f64).unwrap_or(0.0), title: None };
    let st = s.doc()?;
    let r = designcraft_pdf::export_booklet(&st.doc, &s.cache, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            std::fs::write(path, &r.bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "sheets": r.pages, "warnings": r.warnings}))
        }
        None => Ok(json!({"base64": super::file::base64_encode(&r.bytes), "bytes": r.bytes.len(), "sheets": r.pages, "warnings": r.warnings})),
    }
}

pub(crate) fn options(p: &Value, page_count: usize) -> Result<PdfOptions> {
    const ID: &str = "file.exportPdf";

    let pages = match p.get("pages") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(designcraft_pdf::parse_page_range(s, page_count).map_err(|e| bad(ID, e.to_string()))?),
        Some(Value::Array(a)) => {
            let mut v = Vec::new();
            for x in a {
                let n = x.as_u64().filter(|n| *n >= 1 && (*n as usize) <= page_count).ok_or_else(|| bad(ID, format!("bad page {x}")))?;
                v.push(n as usize - 1);
            }
            Some(v)
        }
        Some(Value::Number(n)) => {
            let n = n.as_u64().filter(|n| *n >= 1 && (*n as usize) <= page_count).ok_or_else(|| bad(ID, format!("bad page {n}")))?;
            Some(vec![n as usize - 1])
        }
        Some(v) => return Err(bad(ID, format!("bad `pages`: {v}"))),
    };
    let spreads = p.get("spreads").and_then(Value::as_bool).unwrap_or(false);
    let bleed = p.get("bleed").and_then(Value::as_bool).unwrap_or(false);
    let marks = match p.get("marks") {
        Some(Value::Bool(true)) => Marks::ALL,
        Some(Value::Object(m)) => {
            let b = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
            Marks {
                crop: b("crop"),
                bleed: b("bleed"),
                page_info: b("pageInfo"),
                weight: m.get("weight").and_then(Value::as_f64).unwrap_or(0.25),
                offset: m.get("offset").and_then(Value::as_f64).unwrap_or(6.0),
            }
        }
        _ => Marks::NONE,
    };
    let standard = match str_param(p, "standard") {
        Some(s) => Standard::parse(s).ok_or_else(|| bad(ID, format!("unknown standard `{s}` (none, x4, a2b)")))?,
        None => Standard::None,
    };
    Ok(PdfOptions {
        pages,
        spreads,
        bleed,
        marks,
        standard,
        compress_images: p.get("compressImages").and_then(Value::as_bool).unwrap_or(false),
        title: str_param(p, "title").map(str::to_string),
        author: str_param(p, "author").map(str::to_string),
        tagged: p.get("tagged").and_then(Value::as_bool).unwrap_or(false),
        media: p.get("media").and_then(Value::as_bool).unwrap_or(false),
        ..PdfOptions::default()
    })
}

/// Object number of the first page of a PDF (its page tree's first kid).
fn first_page_ref(pdf: &[u8]) -> Option<usize> {
    let s = String::from_utf8_lossy(pdf);
    let i = s.find("/Kids[")? + 6;
    s[i..].split_whitespace().next()?.parse().ok()
}

fn export_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    let prepared = publication::prepare(s, p)?;
    let supplied = publication::defaults(p, &prepared.metadata);
    let p = &supplied;
    let opts = options(p, prepared.documents.iter().map(designcraft_doc::Document::page_count).sum())?;
    // Transparency Flattener presets: High / Medium / Low Resolution, or a ppi.
    let ppi = match p.get("flatten") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::String(s)) if s == "high" => Some(300.0),
        Some(Value::String(s)) if s == "medium" => Some(150.0),
        Some(Value::String(s)) if s == "low" => Some(72.0),
        Some(Value::Bool(true)) => Some(300.0),
        Some(v) => Some(
            v.as_f64().filter(|v| (36.0..=1200.0).contains(v)).ok_or_else(|| bad("file.exportPdf", "`flatten`: high|medium|low or 36–1200 ppi"))?,
        ),
    };
    let mut documents = Vec::new();
    let mut fx_count = 0;
    let mut layer_count = 0;
    let mut flatten_count = 0;
    for source in &prepared.documents {
        let flattened = ppi.map(|ppi| flatten(source, &s.cache, ppi));
        if let Some((_, n)) = &flattened {
            flatten_count += n;
        }
        let base = flattened.as_ref().map_or(source, |f| &f.0);
        let (layered, n) = rasterize_layered(base);
        layer_count += n;
        let base = layered.as_ref().unwrap_or(base);
        let (effects, n) = rasterize_effects(base, &s.cache);
        fx_count += n;
        documents.push(effects.as_ref().unwrap_or(base).clone());
    }
    let mut r = designcraft_pdf::export_documents_with_report(&documents, &s.cache, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
    r.warnings.extend(prepared.warnings);
    let spreads = publication::selected_spreads(&documents, &opts);
    for cmyk in [false, true] {
        let blended: Vec<_> = spreads
            .iter()
            .map(|(part, si)| {
                documents[*part].settings.blend_space == if cmyk { designcraft_doc::BlendSpace::Cmyk } else { designcraft_doc::BlendSpace::Rgb }
                    && documents[*part].spreads.get(*si).is_some_and(|sp| sp.items.iter().any(|it| transparent(it)))
            })
            .collect();
        if blended.iter().any(|b| *b) {
            match designcraft_pdf::add_blend_space(&r.bytes, &blended, cmyk) {
                Some(bytes) => r.bytes = bytes,
                None => r.warnings.push("the transparency blend space couldn't be set".into()),
            }
        }
    }
    // Interactive PDF view options.
    let mut cat = String::new();
    if p.get("fullScreen").and_then(Value::as_bool) == Some(true) {
        cat.push_str("/PageMode/FullScreen");
    } else if p.get("bookmarksPanel").and_then(Value::as_bool) == Some(true) {
        cat.push_str("/PageMode/UseOutlines");
    }
    match str_param(p, "pageLayout") {
        Some("single") => cat.push_str("/PageLayout/SinglePage"),
        Some("continuous") => cat.push_str("/PageLayout/OneColumn"),
        Some("twoUp") => cat.push_str("/PageLayout/TwoPageLeft"),
        Some("twoUpCover") => cat.push_str("/PageLayout/TwoPageRight"),
        Some("twoUpContinuous") => cat.push_str("/PageLayout/TwoColumnRight"),
        Some(o) => return Err(bad("file.exportPdf", format!("unknown pageLayout `{o}`"))),
        None => {}
    }
    if let Some(v) = str_param(p, "view") {
        // The first page opens with this view (pages are objects in order, so refer by index 0).
        let dest = match v {
            "fitPage" => "/Fit",
            "fitWidth" => "/FitH null",
            "actual" => "/XYZ null null 1",
            o => return Err(bad("file.exportPdf", format!("unknown view `{o}` (fitPage, fitWidth, actual)"))),
        };
        if let Some(first) = first_page_ref(&r.bytes) {
            cat.push_str(&format!("/OpenAction[{first} 0 R{dest}]"));
        }
    }
    if !cat.is_empty() {
        match designcraft_pdf::add_catalog_entries(&r.bytes, &cat) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("the PDF view options couldn't be set".into()),
        }
    }
    // Flip pages every N seconds (presentation).
    if let Some(secs) = p.get("advanceSeconds").and_then(Value::as_f64).filter(|s| *s > 0.0) {
        let n = spreads.len();
        let dur: Vec<Option<String>> = (0..n).map(|_| Some(format!("/Dur {secs:.2}"))).collect();
        match designcraft_pdf::add_page_entries(&r.bytes, &dur) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("page timings couldn't be set".into()),
        }
    }
    // Page transitions (each spread's first page holds them).
    let trans: Vec<Option<designcraft_doc::PageTransition>> =
        spreads.into_iter().map(|(part, si)| documents[part].spreads.get(si).and_then(|sp| sp.pages.first()).and_then(|pg| pg.transition)).collect();
    if trans.iter().any(Option::is_some) {
        match designcraft_pdf::add_transitions(&r.bytes, &trans) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("page transitions couldn't be added to this PDF".into()),
        }
    }
    if fx_count > 0 {
        r.warnings.push(format!("{fx_count} object(s) with soft effects (shadows, glows, feathers, bevels) or knockout groups exported as images"));
    }
    if layer_count > 0 {
        r.warnings.push(format!("{layer_count} placed PDF(s) with hidden layers exported as images"));
    }
    if flatten_count > 0 {
        r.warnings.push(format!("transparency flattened: {flatten_count} region(s) rasterised"));
    }
    r.warnings.sort();
    r.warnings.dedup();
    if p["strict"] == true && !r.warnings.is_empty() {
        return Err(bad("file.exportPdf", r.warnings.join("; ")));
    }
    match str_param(p, "path") {
        Some(path) => {
            write_file(path, &r.bytes)?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "pages": r.pages, "warnings": r.warnings}))
        }
        None => Ok(json!({"base64": super::base64_encode(&r.bytes), "bytes": r.bytes.len(), "pages": r.pages, "warnings": r.warnings})),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}
#[cfg(target_arch = "wasm32")]
fn write_file(path: &str, _bytes: &[u8]) -> Result<()> {
    Err(EngineError::Other(format!("{path}: no file system on the web; omit `path` to get the bytes")))
}

#[cfg(test)]
mod text_tests {
    use serde_json::json;

    use crate::Session;

    fn unzip(b64: &str) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
        zip::ZipArchive::new(std::io::Cursor::new(super::super::file::base64_decode(b64))).unwrap()
    }

    fn read(z: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
        s
    }

    #[test]
    fn object_export_options() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // A plain rectangle rasterised for EPUB, centred, after a page break.
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 200, 200], "shape": "ellipse"})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [r["id"]]})).unwrap();
        s.execute(
            "object.exportOptions",
            &json!({"ids": [r["id"]], "rasterize": true, "align": "center", "pageBreakBefore": true, "altText": "A dot"}),
        )
        .unwrap();
        let e = s.execute("file.exportEpub", &json!({})).unwrap();
        let mut z = unzip(e["base64"].as_str().unwrap());
        let x = read(&mut z, "OEBPS/content.xhtml");
        assert!(
            x.contains("<figure style=\"text-align:center;page-break-before:always;break-before:page;\"><img") && x.contains("alt=\"A dot\""),
            "{x}"
        );
        // Tagged PDF: an artifact object adds no figure.
        let pdf = |s: &mut Session| {
            String::from_utf8_lossy(&super::super::file::base64_decode(
                s.execute("file.exportPdf", &json!({"tagged": true})).unwrap()["base64"].as_str().unwrap(),
            ))
            .matches("/S/Figure")
            .count()
        };
        let figures = pdf(&mut s);
        s.execute("object.exportOptions", &json!({"ids": [r["id"]], "artifact": true})).unwrap();
        assert!(pdf(&mut s) < figures);
    }

    #[test]
    fn epub_cover_and_fixed_layout() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Page one words", "caret": false})).unwrap();
        let r = s.execute("file.exportEpub", &json!({"cover": true})).unwrap();
        let mut z = unzip(r["base64"].as_str().unwrap());
        let opf = read(&mut z, "OEBPS/content.opf");
        assert!(opf.contains("cover-image") && opf.contains("<itemref idref=\"cover\"/>"));
        assert!(z.by_name("OEBPS/images/cover.png").is_ok());
        let r = s.execute("file.exportFixedEpub", &json!({})).unwrap();
        assert_eq!(r["pages"], 2);
        let mut z = unzip(r["base64"].as_str().unwrap());
        let opf = read(&mut z, "OEBPS/content.opf");
        assert!(opf.contains("rendition:layout\">pre-paginated") && opf.contains("page2.xhtml"));
        let p1 = read(&mut z, "OEBPS/page1.xhtml");
        assert!(p1.contains("width=612, height=792") && p1.contains("Page one words"), "{p1}");
    }

    #[test]
    fn flattener_rasterises_spreads_with_transparency() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [id]})).unwrap();
        // Opaque: nothing to flatten.
        let r = s.execute("file.exportPdf", &json!({"flatten": "low"})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("flattened")));
        s.execute("object.opacity", &json!({"ids": [id], "opacity": 0.5})).unwrap();
        // Text away from the transparency stays live text.
        s.execute("frame.create", &json!({"rect": [72, 400, 400, 450], "content": "text", "text": "Still vector", "caret": false})).unwrap();
        let r = s.execute("file.exportPdf", &json!({"flatten": "low"})).unwrap();
        let pdf = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert!(String::from_utf8_lossy(&pdf).contains("/FontFile"), "the text is still text");
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("flattened: 1 ")), "{r}");
        assert_eq!(r["pages"], 3);
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(3));
        // The document itself is untouched.
        assert!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(id.as_u64().unwrap())).is_some());
        assert!(s.execute("file.exportPdf", &json!({"flatten": 5})).is_err());
        // Interactive view options land in the catalog and on the pages.
        let r = s.execute("file.exportPdf", &json!({"fullScreen": true, "pageLayout": "single", "view": "fitPage", "advanceSeconds": 5})).unwrap();
        let t = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).to_string();
        assert!(
            t.contains("/PageMode/FullScreen") && t.contains("/PageLayout/SinglePage") && t.contains("/OpenAction[") && t.contains("/Dur 5.00"),
            "view options"
        );
        assert!(s.execute("file.exportPdf", &json!({"pageLayout": "sideways"})).is_err());
        // Unflattened, the page blends in the document's blend space (CMYK for print).
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        let text = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).to_string();
        assert_eq!(text.matches("/S/Transparency/CS/DeviceCMYK").count(), 1, "only the page with transparency");
    }

    #[test]
    fn export_story_as_text_and_rtf() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "One\nTwo"})).unwrap();
        assert!(s.execute("file.exportText", &json!({"frame": r["id"]})).unwrap()["text"] == "One\r\nTwo");
        let html = s.execute("file.exportHtml", &json!({})).unwrap();
        assert!(html["text"].as_str().unwrap().contains("One</p>"));
        let tagged = s.execute("file.exportText", &json!({"frame": r["id"], "format": "tagged"})).unwrap();
        assert!(tagged["text"].as_str().unwrap().contains("<ParaStyle:"));
        let rtf = s.execute("file.exportText", &json!({"frame": r["id"], "format": "rtf"})).unwrap();
        assert!(rtf["text"].as_str().unwrap().starts_with("{\\rtf1"));
        let dir = std::env::temp_dir().join(format!("dc-export-text-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("story.rtf");
        s.execute("file.exportText", &json!({"frame": r["id"], "path": path.to_string_lossy()})).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("{\\rtf1"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod tagged_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn tagged_pdf_has_structure_and_alt_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Hello"})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 320, 200, 420]})).unwrap();
        s.execute("object.altText", &json!({"text": "A plain box", "ids": [r["id"]]})).unwrap();
        let pdf = |s: &mut Session, tagged: bool| {
            let b64 = s.execute("file.exportPdf", &json!({"tagged": tagged})).unwrap()["base64"].as_str().unwrap().to_string();
            String::from_utf8_lossy(&super::super::file::base64_decode(&b64)).into_owned()
        };
        let t = pdf(&mut s, true);
        assert!(t.contains("/StructTreeRoot") && t.contains("/Figure") && t.contains("A plain box"), "tagged");
        assert!(t.contains("/P") && t.contains("/MarkInfo"));
        assert!(!pdf(&mut s, false).contains("/StructTreeRoot"));
    }
}

#[cfg(test)]
mod variable_font_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn variable_font_instance_exports_to_pdf() {
        let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Skia.ttf") else { return };
        designcraft_fonts::FontDb::global().add_font(data);
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Variable"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 8})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "Skia", "fontStyle": "Black"})).unwrap();
        let out = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(out["bytes"].as_u64().unwrap() > 1000);
        let fonts = s.execute("font.list", &json!({})).unwrap();
        assert!(fonts.to_string().contains("Black"), "{fonts}");
    }
}

#[cfg(test)]
mod booklet_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn booklet_imposes_pages_in_pairs() {
        use designcraft_pdf::{BookletKind, booklet_pairs};
        assert_eq!(booklet_pairs(8, BookletKind::SaddleStitch), [(Some(7), Some(0)), (Some(1), Some(6)), (Some(5), Some(2)), (Some(3), Some(4))]);
        assert_eq!(booklet_pairs(6, BookletKind::SaddleStitch)[0], (None, Some(0)), "padded with blanks");
        assert_eq!(booklet_pairs(3, BookletKind::TwoUpConsecutive), [(Some(0), Some(1)), (Some(2), None)]);
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        let r = s.execute("file.printBooklet", &json!({"spaceBetween": 18})).unwrap();
        assert_eq!(r["sheets"], 2);
        let pdf = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).into_owned();
        let mb = pdf.find("/MediaBox").map(|i| pdf[i..i + 40].to_string()).unwrap_or_default();
        assert!(mb.contains("1242"), "two 612 pt pages and an 18 pt gap: {mb}");
        assert!(s.execute("file.printBooklet", &json!({"type": "perfectBound"})).is_err());
        // Page 1's black box lands on the right half of the first sheet (page 4 is on the left).
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [id]})).unwrap();
        let r = s.execute("file.printBooklet", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        let px = designcraft_render::decode_pixmap_page(&bytes, 0).expect("rasterised");
        let (w, h) = (px.width() as f64, px.height() as f64);
        let at = |x: f64, y: f64| px.sample((x / 1224.0 * w) as u16, (y / 792.0 * h) as u16);
        let (ink, blank) = (at(612.0 + 200.0, 200.0), at(200.0, 200.0));
        assert!(ink.a > 200 && ink.r < 60, "right half has page 1: {ink:?}");
        assert!(blank.a < 30 || blank.r > 200, "left half (page 4) is empty: {blank:?}");
    }
}

#[cfg(test)]
mod placed_pdf_print_tests {
    use serde_json::json;

    use crate::Session;
    use crate::cmd::{base64_decode, base64_encode};

    /// A one-page PDF 1.7 with a black box, as VectorCraft writes a logo.
    fn logo() -> Vec<u8> {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 50})).unwrap();
        s.execute("frame.create", &json!({"rect": [10, 10, 90, 40]})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]"})).unwrap();
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        base64_decode(r["base64"].as_str().unwrap())
    }

    fn page_with(pdf: &[u8]) -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"base64": base64_encode(pdf), "name": "logo.pdf", "x": 72, "y": 72})).unwrap();
        s
    }

    /// A placed PDF 1.7 failed the PDF/X-4 (PDF 1.6) export with
    /// "PDF writer error: Pdf(PdfDocument(PdfDocumentRepr { .. }), VersionMismatch(Pdf17), None)".
    #[test]
    fn pdf_17_placed_in_pdfx4() {
        let logo = logo();
        assert!(logo.starts_with(b"%PDF-1.7"));
        let mut s = page_with(&logo);
        let r = s.execute("file.exportPdf", &json!({"standard": "x4"})).unwrap();
        assert!(r["warnings"].to_string().contains("logo.pdf: a PDF 1.7 placed in a PDF 1.6 file"), "{}", r["warnings"]);
        assert!(base64_decode(r["base64"].as_str().unwrap()).starts_with(b"%PDF-1.6"));
        // PDF 2.0 can't simply be relabelled: the export fails and names the file.
        let mut v20 = logo.clone();
        v20[5..8].copy_from_slice(b"2.0");
        let mut s = page_with(&v20);
        let e = s.execute("file.exportPdf", &json!({"standard": "x4"})).unwrap_err().to_string();
        assert!(e.contains("logo.pdf is PDF 2.0") && e.contains("PDF 1.6"), "{e}");
    }
}

#[cfg(test)]
mod print_group_tests {
    use serde_json::json;

    use crate::Session;
    use crate::cmd::{base64_decode, base64_encode};

    /// A one-page PDF 1.7 with a black box, as VectorCraft writes a logo.
    fn logo() -> Vec<u8> {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 50})).unwrap();
        s.execute("frame.create", &json!({"rect": [10, 10, 90, 40]})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]"})).unwrap();
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        base64_decode(r["base64"].as_str().unwrap())
    }

    fn page_with(pdf: &[u8]) -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"base64": base64_encode(pdf), "name": "logo.pdf", "x": 72, "y": 72})).unwrap();
        s
    }

    /// The placed PDF's transparency group blended in DeviceRGB in a print document
    /// (`/Group<</Type/Group/S/Transparency/I true/CS/DeviceRGB>>`).
    #[test]
    fn print_transparency_groups_blend_in_cmyk() {
        let mut s = page_with(&logo());
        let text = |s: &mut Session| {
            let r = s.execute("file.exportPdf", &json!({"compress": false})).unwrap();
            String::from_utf8_lossy(&base64_decode(r["base64"].as_str().unwrap())).into_owned()
        };
        let t = text(&mut s);
        assert!(t.contains("/S/Transparency/I true/CS/DeviceCMYK>>"), "CMYK group");
        assert!(!t.contains("DeviceRGB"), "no RGB left");
        assert!(!designcraft_pdf::has_rgb_groups(t.as_bytes()));
        // The file still reads, with the same page.
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        let bytes = base64_decode(r["base64"].as_str().unwrap());
        assert_eq!(hayro_syntax::Pdf::new(bytes).unwrap().pages().len(), 1);
        // An RGB blend space keeps RGB groups.
        s.execute("edit.transparencyBlendSpace", &json!({"space": "rgb"})).unwrap();
        let t = text(&mut s);
        assert!(t.contains("/CS/DeviceRGB") && designcraft_pdf::has_rgb_groups(t.as_bytes()));
    }
}
