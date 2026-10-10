//! Full-composition oracle. Compare actual geometry and source destinations, not a digest.
use crate::*;

/// Ignore revision counters and unused style-table entries, but compare every displayed and
/// navigable result, including nested notes/cells, generated Unicode, destinations and overset.
pub fn equivalent(a: &ComposedStory, b: &ComposedStory) -> Result<(), String> {
    compare(a, b, 0)
}
fn compare(a: &ComposedStory, b: &ComposedStory, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("oracle nesting exceeds 64 levels".into());
    }
    if a.story != b.story
        || a.text_len != b.text_len
        || a.overset_at != b.overset_at
        || a.frames.len() != b.frames.len()
        || a.annotation_overflow != b.annotation_overflow
        || a.annotation_progress != b.annotation_progress
        || a.diagnostics != b.diagnostics
        || a.source != b.source
        || a.render_document != b.render_document
    {
        return Err(format!("story {}: extent, source, diagnostics or overflow differs", a.story.0));
    }
    for (fi, (a_frame, b_frame)) in a.frames.iter().zip(&b.frames).enumerate() {
        let (x, y) = (a_frame, b_frame);
        if x.frame != y.frame
            || x.vertical != y.vertical
            || x.range != y.range
            || x.columns != y.columns
            || x.content_height != y.content_height
            || x.decos != y.decos
            || x.lines.len() != y.lines.len()
            || x.tables.len() != y.tables.len()
            || x.notes.len() != y.notes.len()
            || format!("{:?}", x.objects) != format!("{:?}", y.objects)
        {
            return Err(format!("story {}, frame {fi}: geometry or content differs", a.story.0));
        }
        for (li, (x, y)) in x.lines.iter().zip(&y.lines).enumerate() {
            if !incremental::same_line(x, y, &a.styles, &b.styles) {
                return Err(format!("story {}, frame {fi}, line {li}: glyphs, ranges or paragraph state differs", a.story.0));
            }
        }
        for (x, y) in x.tables.iter().zip(&y.tables) {
            if (x.table, x.anchor, x.column, x.rect, &x.strokes, x.line, x.first, x.last, x.cells.len())
                != (y.table, y.anchor, y.column, y.rect, &y.strokes, y.line, y.first, y.last, y.cells.len())
            {
                return Err(format!("frame {fi}: table continuation differs"));
            }
            for (x, y) in x.cells.iter().zip(&y.cells) {
                if (x.row, x.col, x.rect, &x.fill, x.origin, &x.source, x.clip, x.overset, x.repeated, &x.graphic)
                    != (y.row, y.col, y.rect, &y.fill, y.origin, &y.source, y.clip, y.overset, y.repeated, &y.graphic)
                {
                    return Err(format!("frame {fi}: table cell differs"));
                }
                compare(&x.text, &y.text, depth + 1)?;
            }
        }
        for (x, y) in x.notes.iter().zip(&y.notes) {
            if (x.id, x.index, x.annotation, &x.source_story, &x.source_range, x.column, &x.label, x.origin, x.rect, &x.source)
                != (y.id, y.index, y.annotation, &y.source_story, &y.source_range, y.column, &y.label, y.origin, y.rect, &y.source)
            {
                return Err(format!("frame {fi}: note continuation differs"));
            }
            compare(&x.text, &y.text, depth + 1)?;
        }
    }
    Ok(())
}

impl Cache {
    /// Compose again without checkpoints with the same running-head and legacy-reference inputs.
    /// This intentionally costs a full composition and leaves authored document state unchanged.
    pub fn verify(&self, doc: &Document, sid: StoryId) -> Result<(), String> {
        let cached = self.get(doc, sid, None);
        let opts = ComposeOptions {
            running: doc.story(sid).filter(|s| vars::has_running(doc, s)).map(|_| self.running_index(doc)),
            xrefs: doc.story(sid).filter(|s| xref::needs_index(s)).map(|_| self.xref_index(doc)),
            ..Default::default()
        };
        equivalent(&cached, &compose_story(doc, sid, &opts))
    }
}
