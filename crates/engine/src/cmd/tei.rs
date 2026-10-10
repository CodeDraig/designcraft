//! TEI import transactions, local resource resolution, and preserved-source inspection.
use super::{CommandSpec, always, bad, cmd, has_doc, str_param};
use crate::{DocState, Result, Session};
use designcraft_doc::{
    self as doc, Asset, AssetId, Content, Document, Graphic, HyperlinkDest, HyperlinkSource, ItemId, ParaFormat, SpreadRef, StoryId, tei::TeiOptions,
};
use designcraft_geom::{Affine, Rect, shapes};
use designcraft_textimport::{Imported, tei};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
const MAX_RESOURCE: usize = 64 * 1024 * 1024;
const MAX_RESOURCES: usize = 256 * 1024 * 1024;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.openTei", "Open TEI Book", [], None, "{path | base64 + name, tei?: {witness?}, resources?: {URI: {path | base64, name?}}, styleMap?, styleConflicts?} → a new unsaved document and import report", always, open),
        cmd!(query "tei.inspect", "Inspect TEI Import", [], None, "{path | base64 + name, tei?: {witness?}} → metadata, witnesses, styles, resources, diagnostics; no document mutation", always, inspect),
        cmd!(query "tei.report", "TEI Import Report", [], None, "{source?} → stored metadata, options, diagnostics, resources and navigation with current pages", has_doc, report),
        cmd!(query "tei.source", "Preserved TEI Source", [], None, "{source?} → {id, name, base64}; unchanged original, most recent source by default", has_doc, source),
        cmd!(noundo "tei.goTo", "Go To TEI Source Marker", [], None, "{anchor} → selects the destination and returns its current 1-based page", has_doc, go_to),
    ]
}

pub(super) fn read_path(path: &str, cap: usize) -> Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| bad("file.openTei", format!("{path}: {e}")))?;
        let mut bytes = Vec::new();
        file.take(cap as u64 + 1).read_to_end(&mut bytes).map_err(|e| bad("file.openTei", e.to_string()))?;
        if bytes.len() > cap {
            return Err(bad("file.openTei", format!("{path} exceeds {} MiB", cap / 1024 / 1024)));
        }
        Ok(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, cap);
        Err(bad("file.openTei", "browser imports require bytes, not filesystem paths"))
    }
}
pub(super) fn decoded(b64: &str, cap: usize) -> Result<Vec<u8>> {
    if b64.len() > cap.div_ceil(3) * 4 + 1024 {
        return Err(bad("file.openTei", "encoded input exceeds its size limit"));
    }
    let compact: String = b64.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let bytes = super::file::base64_decode(&compact);
    if bytes.len() > cap || super::file::base64_encode(&bytes) != compact {
        return Err(bad("file.openTei", "invalid base64 or input exceeds size limit"));
    }
    Ok(bytes)
}
fn input(p: &Value) -> Result<(String, Vec<u8>)> {
    if let Some(b64) = str_param(p, "base64") {
        return Ok((str_param(p, "name").unwrap_or("book.tei").into(), decoded(b64, tei::MAX_BYTES)?));
    }
    let path = str_param(p, "path").ok_or_else(|| bad("file.openTei", "give path or base64 and name"))?;
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.into());
    Ok((name, read_path(path, tei::MAX_BYTES)?))
}
pub(super) fn import(name: &str, bytes: &[u8], p: &Value) -> Result<Imported> {
    let options: TeiOptions =
        serde_json::from_value(p.get("tei").cloned().unwrap_or_else(|| json!({}))).map_err(|e| bad("file.openTei", e.to_string()))?;
    tei::import(name, bytes, &options).map_err(|e| bad("file.openTei", e.to_string()))
}
fn inspect(_: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = input(p)?;
    let imported = import(&name, &bytes, p)?;
    let payload = imported.tei.as_ref().ok_or_else(|| bad("tei.inspect", "missing TEI payload"))?;
    Ok(
        json!({"name":name,"metadata":payload.source.metadata,"witnesses":payload.witnesses,"styles":imported.para_styles.iter().map(|s|&s.name).collect::<Vec<_>>(),"resources":payload.source.resources,"diagnostics":payload.source.diagnostics}),
    )
}
pub(super) fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = input(p)?;
    open_bytes(s, p, &name, &bytes)
}
pub(super) fn open_bytes(s: &mut Session, p: &Value, name: &str, bytes: &[u8]) -> Result<Value> {
    let imported = import(name, bytes, p)?;
    let title = imported
        .tei
        .as_ref()
        .and_then(|i| i.source.metadata.iter().find_map(|m| m.titles.first()))
        .filter(|t| !t.is_empty())
        .cloned()
        .unwrap_or_else(|| std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "TEI book".into()));
    let d = Document::new(&doc::build::NewDocument { title, ..Default::default() });
    let mut candidate = Session::new();
    candidate.add_document(DocState::new(d, None));
    let mut options = p.clone();
    options["autoflow"] = json!(true);
    let mut receipt = super::place_text::place_imported(&mut candidate, &options, imported)?;
    let doc = (*candidate.doc()?.doc).clone();
    doc.check()?;
    let mut state = DocState::new(doc, None);
    // Keep imported books unsaved/dirty, just like a newly edited document.
    state.saved_doc = Arc::new(Document::new(&Default::default()));
    state.revision = 1;
    let index = s.add_document(state);
    receipt["index"] = json!(index);
    Ok(receipt)
}
fn selected_source<'a>(s: &'a Session, p: &Value) -> Result<&'a doc::tei::TeiSource> {
    let sources = &s.doc()?.doc.tei_sources;
    match p.get("source").and_then(Value::as_u64) {
        Some(id) => sources.iter().find(|v| v.id == id),
        None => sources.last(),
    }
    .map(|s| &**s)
    .ok_or_else(|| bad("tei.source", "no such preserved TEI source"))
}
fn source(s: &mut Session, p: &Value) -> Result<Value> {
    let v = selected_source(s, p)?;
    Ok(json!({"id":v.id,"name":v.name,"base64":super::file::base64_encode(&v.bytes)}))
}
fn report(s: &mut Session, p: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let index = s.cache.xref_index(d);
    let id = p.get("source").and_then(Value::as_u64);
    let mut out = Vec::new();
    for src in d.tei_sources.iter().filter(|src| id.is_none_or(|id| src.id == id)) {
        let mut v = serde_json::to_value(&**src).map_err(|e| bad("tei.report", e.to_string()))?;
        v["navigation"] = json!(
            src.navigation
                .iter()
                .map(|n| json!({"label":n.label,"anchor":n.anchor,"kind":n.kind,"level":n.level,"page":index.page_index(n.anchor).map(|p|p+1)}))
                .collect::<Vec<_>>()
        );
        out.push(v);
    }
    if id.is_some() && out.is_empty() {
        return Err(bad("tei.report", "no such preserved TEI source"));
    }
    Ok(json!(out))
}
fn go_to(s: &mut Session, p: &Value) -> Result<Value> {
    let a = p.get("anchor").and_then(Value::as_u64).ok_or_else(|| bad("tei.goTo", "anchor required"))?;
    let d = &s.doc()?.doc;
    let (story, at) = d.find_anchor(a).ok_or_else(|| bad("tei.goTo", "anchor was removed"))?;
    let page = s.cache.xref_index(d).page_index(a);
    let frame = d.story(story).and_then(|s| s.frames.first().copied());
    s.doc_mut()?.selection = doc::Selection::text(doc::TextSel { story, anchor: at, focus: at, frame, cell: None });
    Ok(json!({"story":story.0,"pos":at,"page":page.map(|p|p+1)}))
}

/// Allocate all imported identifiers before insertion and embed resources in the candidate doc.
/// Nothing escapes this edit until every subsequent layout/check step has succeeded.
pub(super) fn prepare(d: &mut Document, imp: &mut Imported, p: &Value) -> Result<()> {
    let Some(payload) = imp.tei.as_mut() else { return Ok(()) };
    if d.next_id >= (1 << 28) {
        return Err(bad("file.openTei", "document identifier space is exhausted for imported anchors"));
    }
    let mut anchors = BTreeMap::new();
    let mut items = BTreeMap::new();
    imp.story.for_each_encoding_mut(&mut |st| {
        st.id = StoryId(d.alloc());
        for a in &mut st.anchors {
            let new = d.alloc() << 24;
            anchors.insert(a.id, new);
            Arc::make_mut(a).id = new;
        }
        for o in &mut st.objects {
            let o = Arc::make_mut(o);
            let id = d.alloc();
            items.insert(o.item.id.0, id);
            o.item.id = ItemId(id);
            o.item.layer = d.default_layer();
        }
    });
    imp.story.for_each_encoding_mut(&mut |st| {
        for x in &mut st.xrefs {
            if let Some(id) = anchors.get(&x.target) {
                Arc::make_mut(x).target = *id;
            }
        }
    });
    imp.story.for_each_encoding_mut(&mut |st| {
        let content = Arc::make_mut(&mut st.content);
        for n in content.notes.iter_mut().chain(content.endnotes.iter_mut()) {
            if let Some(a) = &mut Arc::make_mut(n).apparatus {
                if let Some(id) = anchors.get(&a.start) {
                    a.start = *id;
                }
                if let Some(id) = anchors.get(&a.end) {
                    a.end = *id;
                }
            }
        }
    });
    for nav in &mut payload.source.navigation {
        if let Some(id) = anchors.get(&nav.anchor) {
            nav.anchor = *id;
        }
    }
    for link in &mut payload.links {
        link.id = d.alloc();
        if let HyperlinkSource::Anchors { start, end } = &mut link.source {
            *start = *anchors.get(start).ok_or_else(|| bad("file.openTei", "missing link start"))?;
            *end = *anchors.get(end).ok_or_else(|| bad("file.openTei", "missing link end"))?;
        }
        if let HyperlinkDest::Anchor(a) = &mut link.dest {
            *a = *anchors.get(a).ok_or_else(|| bad("file.openTei", "missing link target"))?;
        }
    }
    let frame = super::id_param(p, "frame").and_then(|id| d.item(id));
    let area = super::rect_param(p, "rect")
        .or_else(|| frame.map(|i| i.inner_bounds()))
        .or_else(|| d.page(0).map(|p| p.margin_rect()))
        .unwrap_or(Rect::new(0.0, 0.0, 432.0, 648.0));
    let max_w = area.width().max(1.0);
    let max_h = (area.height() - 36.0).max(1.0);
    let mut total = 0usize;
    let mut resolved: BTreeMap<String, Option<(AssetId, (u32, u32))>> = BTreeMap::new();
    for request in &payload.graphics {
        let item_id = *items.get(&request.item).ok_or_else(|| bad("file.openTei", "missing illustration anchor"))?;
        let image = if let Some(found) = resolved.get(&request.url) {
            *found
        } else {
            let resource = resource(&request.url, p)?;
            let image = match resource {
                Some(bytes) => {
                    total = total.saturating_add(bytes.len());
                    if total > MAX_RESOURCES {
                        return Err(bad("file.openTei", "illustrations exceed 256 MiB"));
                    }
                    if let Some(size) = designcraft_render::image_size(&bytes).filter(|(w, h)| *w > 0 && *h > 0) {
                        if u64::from(size.0) * u64::from(size.1) > 100_000_000 {
                            return Err(bad("file.openTei", format!("illustration {:?} exceeds 100 million pixels", request.url)));
                        }
                        let id = AssetId(d.alloc());
                        let mime = designcraft_render::image_mime(&bytes).into();
                        d.assets.insert(
                            id,
                            Arc::new(Asset { id, name: request.url.clone(), mime, link: None, data: Arc::new(bytes), pixels: Some(size), page: 0 }),
                        );
                        Some((id, size))
                    } else {
                        None
                    }
                }
                None => None,
            };
            resolved.insert(request.url.clone(), image);
            image
        };
        let (width, height) = if let Some((_, (pw, ph))) = image {
            let ratio = pw as f64 / ph as f64;
            let w = request.width.or_else(|| request.height.map(|h| h * ratio)).unwrap_or(pw as f64).max(1.0);
            let h = w / ratio;
            let scale = (max_w / w).min(max_h / h).min(1.0);
            (w * scale, h * scale)
        } else {
            (120.0f64.min(max_w), 80.0f64.min(max_h))
        };
        imp.story.for_each_encoding_mut(&mut |st| {
            for obj in &mut st.objects {
                if obj.item.id.0 == item_id {
                    let obj = Arc::make_mut(obj);
                    obj.item.path = shapes::rectangle(Rect::new(0.0, 0.0, width, height));
                    if let Some((asset, (pw, ph))) = image {
                        obj.item.stroke = doc::Stroke::none();
                        obj.item.content = Content::Graphic(Graphic {
                            asset,
                            size: (pw as f64, ph as f64),
                            xf: Affine::scale(width / pw as f64),
                            auto_fit: Default::default(),
                            fit_align: 4,
                            crop: [0.0; 4],
                        });
                    }
                }
            }
        });
        for resource in payload.source.resources.iter_mut().filter(|r| r.url == request.url) {
            resource.asset = image.map(|v| v.0);
        }
        if image.is_none() {
            let mut diagnostic = request.location.clone();
            diagnostic.message =
                format!("Illustration {:?} is missing or unsupported; a visible placeholder is retained. {}", request.url, request.description);
            payload.source.diagnostics.push(diagnostic);
        }
    }
    imp.story.for_each_encoding_mut(&mut |st| {
        for t in st.tables.values_mut() {
            let table = Arc::make_mut(t);
            let total: f64 = table.columns.iter().map(|c| c.width).sum();
            if total > max_w {
                for c in &mut table.columns {
                    c.width *= max_w / total;
                }
            }
        }
    });
    payload.source.import_settings = doc::tei::TeiImportSettings {
        autoflow: p["autoflow"].as_bool().unwrap_or(false),
        remove_styles: p["removeStyles"].as_bool().unwrap_or(false),
        style_conflicts: p["styleConflicts"].as_str().unwrap_or("useExisting").into(),
        style_map: p["styleMap"]
            .as_object()
            .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
            .unwrap_or_default(),
    };
    payload.source.id = d.alloc();
    imp.story.for_each_encoding_mut(&mut |st| {
        let c = Arc::make_mut(&mut st.content);
        for n in c.notes.iter_mut().chain(c.endnotes.iter_mut()) {
            if let Some(a) = &mut Arc::make_mut(n).apparatus {
                a.source = payload.source.id;
            }
        }
    });
    imp.warnings = payload.source.diagnostics.iter().map(|d| format!("{}:{} {}: {}", d.line, d.column, d.code, d.message)).collect();
    Ok(())
}
fn resource(url: &str, p: &Value) -> Result<Option<Vec<u8>>> {
    if let Some(r) = p.get("resources").and_then(|r| r.get(url)) {
        if let Some(b) = r.get("base64").and_then(Value::as_str) {
            return decoded(b, MAX_RESOURCE).map(Some);
        }
        if let Some(path) = r.get("path").and_then(Value::as_str) {
            return read_path(path, MAX_RESOURCE).map(Some);
        }
        return Err(bad("file.openTei", format!("resource {url:?} needs base64 or path")));
    }
    if url.is_empty() || url.contains(":") || url.starts_with("//") {
        return Ok(None);
    }
    let Some(path) = str_param(p, "path") else { return Ok(None) };
    let source = std::path::Path::new(path);
    let base = source.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut decoded = Vec::new();
    let mut chars = url.as_bytes().iter().copied();
    while let Some(c) = chars.next() {
        if c == b'%' {
            let a = chars.next().and_then(|c| (c as char).to_digit(16));
            let b = chars.next().and_then(|c| (c as char).to_digit(16));
            let (Some(a), Some(b)) = (a, b) else { return Err(bad("file.openTei", format!("invalid percent encoding in illustration URI {url:?}"))) };
            decoded.push((a * 16 + b) as u8);
        } else {
            decoded.push(c);
        }
    }
    let local = String::from_utf8(decoded).map_err(|_| bad("file.openTei", "illustration URI is not UTF-8"))?;
    let path = base.join(local);
    #[cfg(not(target_arch = "wasm32"))]
    if !path.is_file() {
        return Ok(None);
    }
    read_path(&path.to_string_lossy(), MAX_RESOURCE).map(Some)
}

pub(super) fn attach(d: &mut Document, imp: &Imported) -> Option<u64> {
    let p = imp.tei.as_ref()?;
    d.hyperlinks.extend(p.links.iter().cloned());
    d.tei_sources.push(Arc::new(p.source.clone()));
    Some(p.source.id)
}
pub(super) fn endnotes(d: &mut Document, flow: bool) -> Result<usize> {
    if d.ordered_endnotes().is_empty() {
        return Ok(0);
    }
    let mut added = 0;
    if d.endnote_story.is_none() {
        let last = d.page_count().saturating_sub(1);
        d.insert_pages(Some(last), 1, None)?;
        added += 1;
        let (si, pi) = d.page_loc(last + 1).ok_or_else(|| bad("file.openTei", "endnote page missing"))?;
        let area = d.spreads[si].pages[pi].margin_rect();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(si), area, d.default_layer(), "", ParaFormat::default())?;
        d.endnote_story = Some(sid);
        d.endnote_options.para_style = if d.styles.para("TEI Note").is_some() { "TEI Note" } else { doc::BASIC_PARAGRAPH }.into();
        d.endnote_options.heading_style = if d.styles.para("TEI Heading 1").is_some() { "TEI Heading 1" } else { doc::BASIC_PARAGRAPH }.into();
    }
    d.sync_endnote_story();
    if flow && let Some(sid) = d.endnote_story {
        added += super::place_text::autoflow(d, sid, 2000)?;
    }
    Ok(added)
}

#[cfg(test)]
mod tests;
