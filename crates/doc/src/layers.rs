//! Layer identity, visibility defaults and the layer color palette.

use crate::LayerId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    /// Selection / frame-edge colour.
    pub color: [u8; 3],
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "yes")]
    pub printable: bool,
    #[serde(default = "yes")]
    pub show_guides: bool,
    #[serde(default)]
    pub suppress_wrap_when_hidden: bool,
}

pub(super) fn yes() -> bool {
    true
}

/// InDesign's layer colour sequence (names are the conventional colour names).
pub const LAYER_COLORS: &[(&str, [u8; 3])] = &[
    ("Light Blue", [43, 155, 255]),
    ("Red", [255, 0, 0]),
    ("Green", [79, 255, 79]),
    ("Blue", [0, 0, 255]),
    ("Yellow", [255, 255, 79]),
    ("Magenta", [255, 79, 255]),
    ("Cyan", [0, 255, 255]),
    ("Gray", [128, 128, 128]),
    ("Black", [0, 0, 0]),
    ("Orange", [255, 102, 0]),
    ("Dark Green", [0, 84, 0]),
    ("Teal", [0, 153, 153]),
    ("Tan", [204, 153, 102]),
    ("Brown", [153, 51, 0]),
    ("Violet", [153, 51, 255]),
    ("Gold", [255, 153, 0]),
];
