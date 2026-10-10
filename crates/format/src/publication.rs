//! Version 3 archives: one canonical publication, plus required binary attachments.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use designcraft_doc::{
    ComponentId, Document, EditionId, ResourceId,
    publication::{Publication, PublicationView},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{FormatError, MIME, VERSION, ext_for};

const MAX_JSON: usize = 256 * 1024 * 1024;
const MAX_ENTRY: usize = 256 * 1024 * 1024;
const MAX_TOTAL: usize = 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Archive {
    publication: Publication,
    active_edition: EditionId,
    active_component: ComponentId,
    /// Explicit lengths distinguish an intentional empty/missing-image record from a lost
    /// archive entry. Every nonempty asset is required on load.
    asset_lengths: BTreeMap<ResourceId, usize>,
}

fn io(error: impl std::fmt::Display) -> FormatError {
    FormatError::Io(error.to_string())
}

pub(super) fn save(doc: &Document) -> Result<Vec<u8>, FormatError> {
    let mut candidate = doc.clone();
    candidate.sync_publication().map_err(io)?;
    let view = candidate.publication.as_ref().ok_or_else(|| io("missing publication"))?;
    view.root.check().map_err(io)?;
    let archive = Archive {
        publication: (*view.root).clone(),
        active_edition: view.edition,
        active_component: view.component,
        asset_lengths: view.root.assets.iter().map(|(id, asset)| (*id, asset.data.len())).collect(),
    };
    let json = serde_json::to_vec(&archive).map_err(io)?;
    if json.len() > MAX_JSON {
        return Err(io("publication JSON exceeds 256 MiB"));
    }
    let mut total = json.len();
    let mut buffer = Cursor::new(Vec::new());
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        zip.start_file("mimetype", stored).map_err(io)?;
        zip.write_all(MIME.as_bytes()).map_err(io)?;
        zip.start_file("meta.json", deflate).map_err(io)?;
        let meta =
            json!({"format":"designcraft", "version":VERSION, "kind":"publication", "generator":concat!("DesignCraft ", env!("CARGO_PKG_VERSION"))});
        zip.write_all(meta.to_string().as_bytes()).map_err(io)?;
        zip.start_file("publication.json", deflate).map_err(io)?;
        zip.write_all(&json).map_err(io)?;
        for (id, source) in &view.root.sources {
            if source.bytes.is_empty() || source.bytes.len() != source.byte_length || source.byte_length > 64 * 1024 * 1024 {
                return Err(io(format!("missing or oversized preserved source {id}")));
            }
            total = total.checked_add(source.byte_length).ok_or_else(|| io("publication size overflow"))?;
            if total > MAX_TOTAL {
                return Err(io("publication attachments exceed 1 GiB"));
            }
            zip.start_file(format!("sources/{}.xml", id.0), deflate).map_err(io)?;
            zip.write_all(&source.bytes).map_err(io)?;
        }
        for (id, asset) in &view.root.assets {
            if asset.data.is_empty() {
                continue;
            }
            total = total.checked_add(asset.data.len()).ok_or_else(|| io("publication size overflow"))?;
            if asset.data.len() > MAX_ENTRY || total > MAX_TOTAL {
                return Err(io("publication asset exceeds 256 MiB or archive exceeds 1 GiB"));
            }
            zip.start_file(format!("assets/{}.{}", id.0, ext_for(&asset.mime)), stored).map_err(io)?;
            zip.write_all(&asset.data).map_err(io)?;
        }
        zip.finish().map_err(io)?;
    }
    Ok(buffer.into_inner())
}

fn read(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, limit: usize) -> Result<Vec<u8>, FormatError> {
    let file = zip.by_name(name).map_err(|_| FormatError::NotOurs(format!("missing required entry {name}")))?;
    if file.size() > limit as u64 {
        return Err(FormatError::NotOurs(format!("{name} exceeds its allowed size")));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() > limit {
        return Err(FormatError::NotOurs(format!("{name} exceeds its allowed size")));
    }
    Ok(bytes)
}

pub(super) fn load(mut zip: zip::ZipArchive<Cursor<&[u8]>>) -> Result<Document, FormatError> {
    let mut names = BTreeSet::new();
    for name in zip.file_names() {
        if !names.insert(name.to_owned()) {
            return Err(FormatError::NotOurs(format!("duplicate archive entry {name}")));
        }
    }
    let bytes = read(&mut zip, "publication.json", MAX_JSON)?;
    let has_review =
        serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| FormatError::NotOurs(e.to_string()))?["publication"].get("review").is_some();
    let mut archive: Archive = serde_json::from_slice(&bytes).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    let root = &mut archive.publication;
    let mut total = bytes.len();
    for (id, source) in &mut root.sources {
        if source.byte_length == 0 || source.byte_length > 64 * 1024 * 1024 {
            return Err(FormatError::NotOurs(format!("invalid length for source {id}")));
        }
        let name = format!("sources/{}.xml", id.0);
        let bytes = read(&mut zip, &name, source.byte_length)?;
        if bytes.len() != source.byte_length {
            return Err(FormatError::NotOurs(format!("incorrect length for {name}")));
        }
        total = total.checked_add(bytes.len()).ok_or_else(|| io("publication size overflow"))?;
        if total > MAX_TOTAL {
            return Err(io("publication attachments exceed 1 GiB"));
        }
        Arc::make_mut(source).bytes = Arc::new(bytes);
    }
    if root.assets.len() != archive.asset_lengths.len() {
        return Err(FormatError::NotOurs("asset manifest does not match publication".into()));
    }
    for (id, asset) in &mut root.assets {
        let length = *archive.asset_lengths.get(id).ok_or_else(|| FormatError::NotOurs(format!("missing asset length {id}")))?;
        if length == 0 {
            continue;
        }
        total = total.checked_add(length).ok_or_else(|| io("publication size overflow"))?;
        if length > MAX_ENTRY || total > MAX_TOTAL {
            return Err(io("publication asset exceeds 256 MiB or archive exceeds 1 GiB"));
        }
        let name = format!("assets/{}.{}", id.0, ext_for(&asset.mime));
        let bytes = read(&mut zip, &name, length)?;
        if bytes.len() != length {
            return Err(FormatError::NotOurs(format!("incorrect length for {name}")));
        }
        Arc::make_mut(asset).data = Arc::new(bytes);
    }
    if !has_review {
        root.migrate_legacy_review().map_err(io)?;
    }
    root.check().map_err(|e| FormatError::NotOurs(e.to_string()))?;
    PublicationView { root: Arc::new(archive.publication), edition: archive.active_edition, component: archive.active_component }
        .project()
        .map_err(io)
}

#[cfg(test)]
mod tests;
