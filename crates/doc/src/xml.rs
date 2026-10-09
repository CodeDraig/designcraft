//! Document XML tags and style mappings.

use serde::{Deserialize, Serialize};

/// XML tags and mappings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct XmlSettings {
    pub root: String,
    pub tags: Vec<XmlTag>,
    /// Paragraph style → tag: tagged frames' paragraphs become elements of that name.
    pub style_map: Vec<(String, String)>,
    /// The loaded Document Type Definition (its text), for validation.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub dtd: String,
}

impl XmlSettings {
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.style_map.is_empty() && self.root.is_empty() && self.dtd.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlTag {
    pub name: String,
    pub color: [u8; 3],
}
