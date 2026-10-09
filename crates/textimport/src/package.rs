//! Bounded UTF-8 ZIP part reads for Office document packages.

use crate::ImportError;
use std::io::Read;

/// Largest decompressed zip part read (a small file can inflate to gigabytes).
pub(crate) const PART_LIMIT: u64 = 64 * 1024 * 1024;

/// A zip part as text. `Ok(None)` when the part is missing or isn't text; an error when it
/// decompresses to more than [`PART_LIMIT`].
pub(crate) fn part(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Result<Option<String>, ImportError> {
    part_capped(zip, name, PART_LIMIT)
}

pub(crate) fn part_capped(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str, limit: u64) -> Result<Option<String>, ImportError> {
    let Ok(f) = zip.by_name(name) else { return Ok(None) };
    let mut s = String::new();
    // Read one byte past the limit to tell "exactly at the limit" from "over it".
    if f.take(limit.saturating_add(1)).read_to_string(&mut s).is_err() {
        return Ok(None);
    }
    if s.len() as u64 > limit {
        return Err(ImportError::Corrupt(format!("{name} is larger than {} MiB uncompressed", limit / (1024 * 1024))));
    }
    Ok(Some(s))
}
