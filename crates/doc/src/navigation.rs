//! Document hyperlink targets, bookmarks and generated table-of-contents settings.

use crate::{ItemId, StoryId};
use serde::{Deserialize, Serialize};

/// What a hyperlink is attached to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum HyperlinkSource {
    Item {
        id: ItemId,
    },
    Text {
        story: StoryId,
        start: usize,
        end: usize,
    },
    /// Boundaries that move with text edits (used by imported TEI references).
    Anchors {
        start: u64,
        end: u64,
    },
}

/// Where a hyperlink goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum HyperlinkDest {
    Url(String),
    Email(String),
    /// Absolute page index.
    Page(usize),
    /// A text destination whose page is resolved after composition.
    Anchor(u64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hyperlink {
    pub id: u64,
    pub name: String,
    pub source: HyperlinkSource,
    pub dest: HyperlinkDest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TocEntry {
    /// Paragraph style whose paragraphs are listed.
    pub style: String,
    /// 1-based nesting level.
    #[serde(default = "one")]
    pub level: u8,
}

fn one() -> u8 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Toc {
    pub story: StoryId,
    pub title: String,
    pub entries: Vec<TocEntry>,
    #[serde(default)]
    pub page_numbers: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub name: String,
    /// Absolute page index.
    pub page: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Bookmark>,
}

impl HyperlinkSource {
    /// Current text range, resolving marker boundaries after edits and threading.
    pub fn text_range(&self, doc: &crate::Document) -> Option<(StoryId, usize, usize)> {
        match self {
            Self::Text { story, start, end } => Some((*story, *start, *end)),
            Self::Anchors { start, end } => {
                let (sid, a) = doc.find_anchor(*start)?;
                let (end_sid, b) = doc.find_anchor(*end)?;
                (sid == end_sid && a <= b).then_some((sid, a, b.max(a + crate::ANCHOR_MARK.len_utf8())))
            }
            Self::Item { .. } => None,
        }
    }
}
