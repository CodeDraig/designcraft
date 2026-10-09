//! Discover installed font directories and read bounded font naming tables.

#[cfg(not(target_arch = "wasm32"))]
use super::{ScannedFace, parsing::face_names};

/// The names and font menu group of every face in the font file at `path`, reading only its table
/// directories and the tables that name and classify it (`name`, `OS/2`, `meta`; `cmap` only for a
/// font that declares no language or code page): a scan opens hundreds of font files, many of them
/// megabytes long. A variable font is cataloged under its default style; its named instances
/// appear once it loads.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn file_face_names(path: &std::path::Path) -> Vec<ScannedFace> {
    use std::io::{Read, Seek, SeekFrom};
    /// Caps on what a (possibly damaged) file can make the scan read.
    const MAX_FACES: u32 = 256;
    const MAX_NAME_TABLE: u32 = 1 << 20;
    const MAX_SMALL_TABLE: u32 = 64 << 10;
    const MAX_CMAP: u32 = 4 << 20;
    let Ok(mut file) = std::fs::File::open(path) else { return vec![] };
    let mut read_at = |offset: u64, len: usize| -> Option<Vec<u8>> {
        let mut buf = vec![0; len];
        file.seek(SeekFrom::Start(offset)).ok()?;
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    let be32 = |b: &[u8], at: usize| b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_be_bytes);
    let Some(head) = read_at(0, 12) else { return vec![] };
    // A collection lists where each face's table directory starts.
    let starts: Vec<u32> = if head.starts_with(b"ttcf") {
        let n = be32(&head, 8).unwrap_or(0).min(MAX_FACES) as usize;
        read_at(12, n * 4).map(|b| b.as_chunks::<4>().0.iter().map(|c| u32::from_be_bytes(*c)).collect()).unwrap_or_default()
    } else {
        vec![0]
    };
    starts
        .into_iter()
        .filter_map(|start| {
            let dir = read_at(start.into(), 12)?;
            let tables = u16::from_be_bytes(dir.get(4..6)?.try_into().ok()?) as usize;
            let records = read_at(u64::from(start) + 12, tables * 16)?;
            // Table `tag`, unless missing or over `max` bytes.
            let mut table = |tag: &[u8; 4], max: u32| -> Option<([u8; 4], Vec<u8>)> {
                let rec = records.as_chunks::<16>().0.iter().find(|r| r.starts_with(tag))?;
                let (offset, len) = (be32(rec, 8)?, be32(rec, 12)?);
                if len > max {
                    return None;
                }
                Some((*tag, read_at(offset.into(), len as usize)?))
            };
            // A font holding just these tables, to read them as the font itself would.
            let mut found = vec![table(b"name", MAX_NAME_TABLE)?];
            found.extend(table(b"OS/2", MAX_SMALL_TABLE));
            found.extend(table(b"meta", MAX_SMALL_TABLE));
            let font = crate::group::sfnt(found.clone())?;
            let f = skrifa::FontRef::new(&font).ok()?;
            let (family, style) = face_names(&f)?;
            let (group, native) = match crate::group::declared_group(&f) {
                Some(g) => (g, crate::group::classify(&f, &family).1),
                None => {
                    found.extend(table(b"cmap", MAX_CMAP));
                    let font = crate::group::sfnt(found)?;
                    crate::group::classify(&skrifa::FontRef::new(&font).ok()?, &family)
                }
            };
            Some(ScannedFace { family, style, group, native })
        })
        .collect()
}

/// The platform's font folders (the system's and the user's), scanned by [`super::FontDb::global`].
/// Empty on wasm.
pub fn system_font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if cfg!(target_arch = "wasm32") {
        return dirs;
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(Into::into));
        if let Some(h) = &home {
            dirs.push(h.join("Library/Fonts"));
        }
        // Fonts macOS ships as assets (PingFang, Yu Mincho, …): com_apple_MobileAsset_Font<N>.
        for parent in ["/System/Library/AssetsV2", "/System/Library/AssetsV2/PreinstalledAssetsV2/InstallWithOs"] {
            let Ok(entries) = std::fs::read_dir(parent) else { continue };
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("com_apple_MobileAsset_Font") {
                    dirs.push(e.path());
                }
            }
        }
        // Microsoft Office bundles its fonts (Calibri, Cambria, …) inside each app; read them in
        // place from the first installed one (they share the same set).
        let office = ["Microsoft Word", "Microsoft Excel", "Microsoft PowerPoint", "Microsoft Outlook"]
            .iter()
            .map(|app| std::path::PathBuf::from(format!("/Applications/{app}.app/Contents/Resources/DFonts")))
            .find(|p| p.is_dir());
        dirs.extend(office);
    } else if cfg!(windows) {
        let root = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        dirs.push(root.join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(std::path::PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
        }
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(Into::into));
        if let Some(h) = &home {
            dirs.push(h.join(".fonts"));
            dirs.push(h.join(".local/share/fonts"));
        }
    }
    dirs
}
