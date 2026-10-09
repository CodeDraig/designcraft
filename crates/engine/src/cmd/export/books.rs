//! HTML, text, and EPUB export commands.

#[cfg(not(target_arch = "wasm32"))]
use super::EngineError;
use super::{Result, Session, Value, bad, json, rasterize_where, str_param, threaded, write_file};

pub(super) fn export_html(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let opts =
        designcraft_epub::HtmlOptions { title: str_param(p, "title").map(str::to_string), language: str_param(p, "language").map(str::to_string) };
    let text = designcraft_epub::export_html(&st.doc, &opts);
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            std::fs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            Ok(json!({"path": path, "bytes": text.len()}))
        }
        None => Ok(json!({"bytes": text.len(), "text": text})),
    }
}

pub(super) fn has_story_target(s: &Session) -> std::result::Result<(), String> {
    super::super::text::story_of(s, &Value::Null).map(|_| ()).ok_or_else(|| "edit text or select a text frame".into())
}

pub(super) fn export_text(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.exportText";
    let sid = super::super::text::story_of(s, p).ok_or_else(|| bad(ID, "edit text or select a text frame"))?;
    let doc = &s.doc()?.doc;
    let story = doc.stories.get(&sid).ok_or_else(|| bad(ID, format!("no story {}", sid.0)))?;
    let path = str_param(p, "path");
    let format = match str_param(p, "format") {
        Some(f @ ("rtf" | "tagged")) => f,
        Some("txt") | Some("text") => "txt",
        Some(f) => return Err(bad(ID, format!("unknown format `{f}` (txt, rtf, tagged)"))),
        None if path.is_some_and(|x| x.to_ascii_lowercase().ends_with(".rtf")) => "rtf",
        None => "txt",
    };
    let text = match format {
        "rtf" => designcraft_textimport::export::rtf(doc, story),
        "tagged" => designcraft_textimport::tagged::tagged_text(doc, story),
        _ => designcraft_textimport::export::plain_text(story),
    };
    match path {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            std::fs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            Ok(json!({"path": path, "bytes": text.len()}))
        }
        None => Ok(json!({"bytes": text.len(), "text": text})),
    }
}

/// Text of the frames on page `abs`, in line order (for fixed-layout pages).
fn page_text(d: &designcraft_doc::Document, cache: &designcraft_compose::Cache, abs: usize) -> String {
    let mut out = String::new();
    for st in d.stories.values() {
        let cs = cache.get(d, st.id, None);
        for ft in &cs.frames {
            if d.page_of_item(ft.frame) != Some(abs) {
                continue;
            }
            for l in &ft.lines {
                let r = l.range.start.min(st.text.len())..l.range.end.min(st.text.len());
                let t: String = st.text[r].chars().filter(|c| !('\u{E000}'..='\u{F8FF}').contains(c)).collect();
                if !t.trim().is_empty() {
                    out.push_str(t.trim_end_matches('\n'));
                    out.push('\n');
                }
            }
        }
    }
    out
}

pub(super) fn export_epub(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    // Object Export Options › Rasterize: those objects go in as images.
    let (raster, _) = rasterize_where(&st.doc, &s.cache, 144.0, |it| it.export_options.rasterize && !threaded(&st.doc, it));
    let epub_doc: &designcraft_doc::Document = raster.as_ref().unwrap_or(&st.doc);
    let fixed = p.get("fixedLayout").and_then(Value::as_bool).unwrap_or(false);
    let want_cover = p.get("cover").and_then(Value::as_bool).unwrap_or(false);
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let ropts = designcraft_render::RenderOptions { printing_only: true, ..Default::default() };
    let cover = (want_cover && !fixed).then(|| rr.render_page(&st.doc, &s.cache, 0, 2.0, false, &ropts).map(|img| img.to_png())).flatten();
    let opts = designcraft_epub::EpubOptions {
        title: p.get("title").and_then(Value::as_str).map(str::to_string),
        author: p.get("author").and_then(Value::as_str).map(str::to_string),
        language: p.get("language").and_then(Value::as_str).unwrap_or("en").to_string(),
        identifier: None,
        cover,
    };
    if fixed {
        let d = &st.doc;
        let mut pages = Vec::new();
        for i in 0..d.page_count() {
            let Some((si, pi)) = d.page_loc(i) else { continue };
            let b = d.spreads[si].pages[pi].bounds();
            let Some(img) = rr.render_page(d, &s.cache, i, 2.0, false, &ropts) else { continue };
            pages.push(designcraft_epub::FixedPage { png: img.to_png(), width: b.width(), height: b.height(), text: page_text(d, &s.cache, i) });
        }
        let bytes = designcraft_epub::export_fixed_epub(d, &pages, &opts).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        return match p.get("path").and_then(Value::as_str) {
            Some(path) => {
                write_file(path, &bytes)?;
                Ok(serde_json::json!({"path": path, "bytes": bytes.len(), "pages": pages.len()}))
            }
            None => Ok(serde_json::json!({"base64": super::super::base64_encode(&bytes), "bytes": bytes.len(), "pages": pages.len()})),
        };
    }
    let bytes = designcraft_epub::export_epub(epub_doc, &opts).map_err(|e| crate::EngineError::Other(e.to_string()))?;
    match p.get("path").and_then(Value::as_str) {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            std::fs::write(path, &bytes).map_err(|e| crate::EngineError::Other(format!("{path}: {e}")))?;
            Ok(serde_json::json!({"path": path, "bytes": bytes.len()}))
        }
        None => Ok(serde_json::json!({"base64": super::super::file::base64_encode(&bytes), "bytes": bytes.len()})),
    }
}
