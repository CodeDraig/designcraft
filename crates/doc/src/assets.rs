//! Embedded and linked asset records.

use crate::AssetId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// An embedded file (placed image / PDF). Bytes are stored next to the JSON in the native format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: AssetId,
    pub name: String,
    pub mime: String,
    /// Original location on disk (Links panel), if linked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip)]
    pub data: Arc<Vec<u8>>,
    /// Pixel size for raster images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixels: Option<(u32, u32)>,
    /// The page shown from a multi-page PDF (0-based; Image Import Options).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub page: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}
