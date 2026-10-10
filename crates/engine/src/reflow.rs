//! Primary-text-frame page growth and shrinkage.

use super::{Arc, Session, cmd};

impl Session {
    /// Smart Text Reflow for the primary story: add threaded pages while it oversets; remove
    /// trailing pages whose only object is an empty frame of it.
    pub(super) fn smart_reflow(&mut self) -> crate::Result<()> {
        let Some(st) = self.active() else { return Ok(()) };
        if st.interaction.is_some() {
            return Ok(());
        }
        let Some(sid) = st.doc.settings.primary_story else { return Ok(()) };
        let Some(story) = st.doc.story(sid) else { return Ok(()) };
        let cs = self.cache.get(&st.doc, sid, None);
        let overset = cs.overset_at.is_some();
        let empty_tail = story.frames.len() > 1 && story.frames.last().is_some_and(|f| cs.frame(*f).is_none_or(|ft| ft.range.is_empty()));
        if !overset && !empty_tail {
            return Ok(());
        }
        let mut d = (*st.doc).clone();
        if overset {
            let result = cmd::place_text_autoflow_cached(&mut d, sid, 500, &self.cache);
            if d.publication.is_some() {
                result?;
            }
        } else {
            // Drop empty trailing frames on the last pages (one frame, nothing else there).
            while let Some(story) = d.story(sid) {
                if story.frames.len() < 2 {
                    break;
                }
                let Some(&last) = story.frames.last() else { break };
                let cs = self.cache.get(&d, sid, None);
                if cs.frame(last).is_some_and(|ft| !ft.range.is_empty()) {
                    break;
                }
                let Some(page) = d.page_of_item(last) else { break };
                let alone = d.spreads.iter().flat_map(|sp| sp.items.iter()).filter(|i| d.page_of_item(i.id) == Some(page)).count() == 1;
                if page + 1 != d.page_count() || !alone || d.page_count() < 2 {
                    break;
                }
                if d.remove_item(last).is_err() || d.delete_pages(&[page]).is_err() {
                    break;
                }
            }
        }
        d.sync_publication()?;
        if let Some(st) = self.active_mut() {
            st.doc = Arc::new(d);
            st.revision += 1;
        }
        Ok(())
    }
}
