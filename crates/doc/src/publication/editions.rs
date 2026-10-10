//! Edition output choices are independent of shared authored content.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{Publication, invalid};
use crate::{ComponentId, ContentId, Document, EditionId, Result, StoryId};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct OutputSettings {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub identifier: Option<String>,
    /// Stable generated-story content identities. Authored chapters cannot be excluded here.
    pub excluded_sections: BTreeSet<ContentId>,
}

impl OutputSettings {
    pub fn check(&self, root: &Publication) -> Result<()> {
        if [&self.title, &self.author, &self.language, &self.identifier].into_iter().flatten().any(|s| s.len() > 4096) {
            return Err(invalid("edition output metadata is limited to 4096 bytes per value"));
        }
        if self.language.as_ref().is_some_and(|s| s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
            return Err(invalid("edition language must be a nonempty language tag, such as en or cy-GB"));
        }
        let available: BTreeSet<_> = root.content.keys().copied().collect();
        if !self.excluded_sections.is_subset(&available) {
            return Err(invalid("excludedSections must name existing generated section or index content identities"));
        }
        Ok(())
    }
}

impl Publication {
    pub fn story_included(&self, edition: EditionId, component: ComponentId, story: StoryId) -> bool {
        let Some(e) = self.editions.get(&edition) else { return false };
        let Some(content) = self.components.get(&component).and_then(|c| c.stories.get(&story)) else { return false };
        if e.output.excluded_sections.contains(content) && self.generated_destinations().any(|(_, c, s, _)| c == component && s == story) {
            return false;
        }
        if let Some(output) = self.note_output(component, story) {
            use super::annotations::Placement;
            return matches!(
                (self.note_policy(edition, &output.stream).placement, output.source),
                (Placement::ComponentEnd, Some(_)) | (Placement::PublicationEnd, None)
            );
        }
        true
    }
}

impl Document {
    pub fn publication_story_included(&self, story: StoryId) -> bool {
        self.publication.as_ref().is_none_or(|v| v.root.story_included(v.edition, v.component, story))
    }
}
