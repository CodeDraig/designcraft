//! Reading-order tags and content structure.

use super::{Affine, Artifact, ArtifactType, Content, ContentTag, Exporter, Item, Surface, Tag, TagGroup, TagTree};

impl Exporter<'_> {
    /// A spread or parent item, inside its structure tag when tagging.
    pub(super) fn top_item(&mut self, s: &mut Surface, it: &Item, parent: Affine, page_name: Option<&str>, on_parent: bool) {
        if !self.opts.tagged || it.hidden || it.nonprinting {
            return self.item(s, it, parent, page_name);
        }
        let story = it.text_frame().map(|t| t.story);
        let figure = !it.alt_text.is_empty() || matches!(it.content, Content::Graphic(_)) || it.has_nested_items();
        if on_parent || it.export_options.artifact || (story.is_none() && !figure) {
            // Page furniture and decoration.
            let kind = if on_parent { ArtifactType::Page } else { ArtifactType::Other };
            s.start_tagged(ContentTag::Artifact(Artifact::new(kind, None)));
            self.item(s, it, parent, page_name);
            s.end_tagged();
            return;
        }
        if let Some(sid) = story {
            // Text runs tag themselves by paragraph; the rest of the frame is an artifact.
            if let std::collections::hash_map::Entry::Vacant(e) = self.story_tags.entry(sid) {
                e.insert(Vec::new());
                self.tags.push(TagEntry::Story(sid));
            }
            self.tag_story = Some(sid);
            self.item(s, it, parent, page_name);
            self.tag_story = None;
            return;
        }
        let id = s.start_tagged(ContentTag::Other);
        self.item(s, it, parent, page_name);
        s.end_tagged();
        let alt = (!it.alt_text.is_empty()).then(|| it.alt_text.clone());
        let mut g = TagGroup::new(Tag::Figure(alt));
        g.push(id);
        self.tags.push(TagEntry::Figure(g));
    }

    /// The structure tree: stories (one paragraph each, its frames in thread order of appearance)
    /// and figures, in the order they first appear.
    pub(super) fn tag_tree(&mut self) -> TagTree {
        let mut tree = TagTree::new();
        for e in std::mem::take(&mut self.tags) {
            match e {
                TagEntry::Story(sid) => {
                    // One structure element per paragraph: headings and quotes from the
                    // paragraph style's Export Tagging, else P.
                    let mut paras: Vec<usize> = self.para_tags.keys().filter(|(s, _)| *s == sid).map(|(_, p)| *p).collect();
                    paras.sort_unstable();
                    let story = self.doc.story(sid);
                    for pi in paras {
                        let ids = self.para_tags.remove(&(sid, pi)).unwrap_or_default();
                        let style = story.and_then(|st| st.paras.get(pi)).map(|p| p.style.clone()).unwrap_or_default();
                        let tag = self.doc.styles.export_tag(&style, false).map(|e| e.tag.clone()).unwrap_or_default();
                        let text = || story.and_then(|st| st.para_ranges().get(pi).map(|r| st.text[r.clone()].trim().to_string()));
                        let kind: krilla::tagging::TagKind = match tag.as_str() {
                            h if h.len() == 2 && h.starts_with('h') => {
                                let n = h[1..].parse::<u16>().unwrap_or(1).clamp(1, 6);
                                Tag::Hn(std::num::NonZeroU16::new(n).unwrap_or(std::num::NonZeroU16::MIN), text()).into()
                            }
                            "blockquote" => Tag::BlockQuote.into(),
                            _ => Tag::P.into(),
                        };
                        let mut g = TagGroup::new(kind);
                        for id in ids {
                            g.push(id);
                        }
                        tree.push(g);
                    }
                }
                TagEntry::Figure(g) => tree.push(g),
            }
        }
        tree
    }
}

pub(super) enum TagEntry {
    Story(designcraft_doc::StoryId),
    Figure(TagGroup),
}
