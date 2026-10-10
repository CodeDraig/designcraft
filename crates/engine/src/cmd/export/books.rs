//! HTML, text, and EPUB export commands.

#[cfg(not(target_arch = "wasm32"))]
use super::EngineError;
use super::{Result, Session, Value, bad, json, rasterize_where, str_param, threaded, write_file};

pub(super) fn export_html(s: &mut Session, p: &Value) -> Result<Value> {
    let prepared = super::publication::prepare(s, p)?;
    let supplied = super::publication::defaults(p, &prepared.metadata);
    let p = &supplied;
    let opts =
        designcraft_epub::HtmlOptions { title: str_param(p, "title").map(str::to_string), language: str_param(p, "language").map(str::to_string) };
    let report = designcraft_epub::export_html_documents(&prepared.documents, &opts).map_err(|e| crate::EngineError::Other(e.to_string()))?;
    let mut warnings = prepared.warnings;
    warnings.extend(report.warnings);
    warnings.sort();
    warnings.dedup();
    if p["strict"] == true && !warnings.is_empty() {
        return Err(bad("file.exportHtml", warnings.join("; ")));
    }
    let text = report.text;
    match str_param(p, "path") {
        Some(path) => {
            write_file(path, text.as_bytes())?;
            Ok(json!({"path":path,"bytes":text.len(),"warnings":warnings}))
        }
        None => Ok(json!({"bytes":text.len(),"text":text,"warnings":warnings})),
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

/// Search/read-aloud text follows the composed fragments, including rich generated fields,
/// cells and continued notes. Extracting raw story ranges would omit those contents.
fn page_text(d: &designcraft_doc::Document, cache: &designcraft_compose::Cache, abs: usize) -> String {
    fn frame(cs: &designcraft_compose::ComposedStory, ft: &designcraft_compose::FrameText, source: &str, out: &mut String, depth: usize) {
        if depth > 32 {
            return;
        }
        let source = cs.source.as_ref().map_or(source, |s| s.text.as_str());
        for (line_index, line) in ft.lines.iter().enumerate() {
            let mut clusters = std::collections::BTreeMap::<usize, String>::new();
            let mut seen = std::collections::BTreeSet::new();
            for g in &line.glyphs {
                if let Some(generated) = &g.generated {
                    if let Some(text) = &generated.text {
                        clusters.entry(g.byte).or_default().push_str(text);
                    }
                } else if g.len > 0
                    && seen.insert((g.byte, g.len))
                    && let Some(text) = g.byte.checked_add(g.len).and_then(|end| source.get(g.byte..end))
                {
                    clusters.entry(g.byte).or_default().extend(text.chars().filter(|c| !('\u{e000}'..='\u{e1ff}').contains(c)));
                }
            }
            let text: String = clusters.into_values().collect();
            if !text.trim().is_empty() {
                out.push_str(text.trim_end_matches('\n'));
                out.push('\n');
            }
            for table in ft.tables.iter().filter(|t| t.line == line_index) {
                for cell in &table.cells {
                    for ft in &cell.text.frames {
                        frame(&cell.text, ft, &cell.source, out, depth + 1);
                    }
                }
            }
        }
        for note in &ft.notes {
            for ft in &note.text.frames {
                frame(&note.text, ft, &note.source, out, depth + 1);
            }
        }
    }
    let mut out = String::new();
    let order: Vec<_> = if let Some(v) = &d.publication {
        v.root.components.get(&v.component).map(|c| c.story_order.clone()).unwrap_or_default()
    } else {
        designcraft_epub::reading_order(d)
            .into_iter()
            .filter_map(|b| match b {
                designcraft_epub::Block::Story(id) => Some(id),
                _ => None,
            })
            .collect()
    };
    for sid in order {
        let Some(st) = d.story(sid) else { continue };
        let cs = cache.get(d, sid, None);
        for ft in &cs.frames {
            if d.page_of_item(ft.frame) == Some(abs) {
                frame(&cs, ft, &st.text, &mut out, 0);
            }
        }
    }
    out
}

pub(super) fn export_epub(s: &mut Session, p: &Value) -> Result<Value> {
    let prepared = super::publication::prepare(s, p)?;
    let supplied = super::publication::defaults(p, &prepared.metadata);
    let p = &supplied;
    let mut warnings = prepared.warnings;
    let mut documents = Vec::new();
    for doc in &prepared.documents {
        let (raster, count) = rasterize_where(doc, &s.cache, 144.0, |it| it.export_options.rasterize && !threaded(doc, it));
        if count > 0 {
            warnings.push(format!("Chapter {}: {count} objects rasterized by their export settings", doc.title));
        }
        documents.push(raster.unwrap_or_else(|| doc.clone()));
    }
    let first = documents.first().ok_or_else(|| bad("file.exportEpub", "no chapters to export"))?;
    let fixed = p["fixedLayout"] == true;
    let mut renderer = designcraft_render::Renderer::new();
    renderer.threads = designcraft_render::default_threads();
    let ropts = designcraft_render::RenderOptions { printing_only: true, ..Default::default() };
    let cover = if p["cover"] == true && !fixed {
        Some(
            renderer
                .render_page(first, &s.cache, 0, 2., false, &ropts)
                .ok_or_else(|| bad("file.exportEpub", "cover page could not be rendered"))?
                .to_png(),
        )
    } else {
        None
    };
    let opts = designcraft_epub::EpubOptions {
        title: str_param(p, "title").map(str::to_string),
        author: str_param(p, "author").map(str::to_string),
        language: str_param(p, "language").unwrap_or("en").into(),
        identifier: str_param(p, "identifier").map(str::to_string),
        cover,
    };
    let (bytes, page_count) = if fixed {
        let navigation = designcraft_pdf::navigation_documents(&documents, &s.cache).map_err(|e| bad("file.exportEpub", e.to_string()))?;
        warnings.extend(navigation.warnings);
        let mut links = navigation.pages.into_iter();
        let mut pages = Vec::new();
        for doc in &documents {
            for page in 0..doc.page_count() {
                let (si, pi) = doc.page_loc(page).ok_or_else(|| bad("file.exportEpub", "missing page"))?;
                let b = doc.spreads[si].pages[pi].bounds();
                let img = renderer
                    .render_page(doc, &s.cache, page, 2., false, &ropts)
                    .ok_or_else(|| bad("file.exportEpub", format!("could not render chapter {} page {}", doc.title, page + 1)))?;
                pages.push(designcraft_epub::FixedPage {
                    png: img.to_png(),
                    width: b.width(),
                    height: b.height(),
                    text: page_text(doc, &s.cache, page),
                    links: links
                        .next()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|link| {
                            let href = match link.target {
                                designcraft_pdf::NavigationTarget::Page(page) => format!("page{}.xhtml", page + 1),
                                designcraft_pdf::NavigationTarget::Url(url) => url,
                            };
                            if designcraft_epub::executable_link(&href) {
                                warnings.push(format!("Inactive executable link: {}", link.label));
                                return None;
                            }
                            Some(designcraft_epub::FixedLink {
                                rect: [link.rect.x0, link.rect.y0, link.rect.x1, link.rect.y1],
                                href,
                                label: link.label,
                            })
                        })
                        .collect(),
                });
            }
        }
        (designcraft_epub::export_fixed_epub(first, &pages, &opts).map_err(|e| crate::EngineError::Other(e.to_string()))?, Some(pages.len()))
    } else {
        let report = designcraft_epub::export_epub_documents(&documents, &opts).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        warnings.extend(report.warnings);
        (report.bytes, None)
    };
    warnings.sort();
    warnings.dedup();
    if p["strict"] == true && !warnings.is_empty() {
        return Err(bad("file.exportEpub", warnings.join("; ")));
    }
    match str_param(p, "path") {
        Some(path) => {
            write_file(path, &bytes)?;
            Ok(json!({"path":path,"bytes":bytes.len(),"pages":page_count,"warnings":warnings}))
        }
        None => Ok(json!({"base64":super::super::file::base64_encode(&bytes),"bytes":bytes.len(),"pages":page_count,"warnings":warnings})),
    }
}
