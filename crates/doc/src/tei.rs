//! Preserved TEI input and its import receipt. The source is immutable; layout edits never
//! pretend to be a new TEI transcription. Source bytes live in the native ZIP, not its JSON.

use crate::AssetId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct TeiOptions {
    /// xml:id of the witness used for the reading edition, with or without a leading #.
    pub witness: Option<String>,
}

/// Layout choices recorded in addition to the reading-edition options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TeiImportSettings {
    pub autoflow: bool,
    pub remove_styles: bool,
    pub style_map: std::collections::BTreeMap<String, String>,
    pub style_conflicts: String,
}
impl Default for TeiImportSettings {
    fn default() -> Self {
        Self { autoflow: false, remove_styles: false, style_map: Default::default(), style_conflicts: "useExisting".into() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeiMetadata {
    pub titles: Vec<String>,
    pub contributors: Vec<String>,
    pub edition: Vec<String>,
    pub publication: Vec<String>,
    pub sources: Vec<String>,
    pub identifiers: Vec<String>,
    pub languages: Vec<String>,
    pub rights: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeiDiagnostic {
    pub code: String,
    pub element: String,
    pub xml_id: Option<String>,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeiNavigation {
    pub label: String,
    pub anchor: u64,
    /// heading, book, or sourcePage.
    pub kind: String,
    pub level: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeiResource {
    pub url: String,
    pub asset: Option<AssetId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeiSource {
    pub id: u64,
    pub name: String,
    pub byte_length: usize,
    #[serde(skip)]
    pub bytes: Arc<Vec<u8>>,
    pub metadata: Vec<TeiMetadata>,
    pub options: TeiOptions,
    #[serde(default)]
    pub import_settings: TeiImportSettings,
    pub diagnostics: Vec<TeiDiagnostic>,
    pub navigation: Vec<TeiNavigation>,
    pub resources: Vec<TeiResource>,
}
