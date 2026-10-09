//! Table anchors and cell stories within document text.

use super::{Arc, Story, StoryId, TABLE_ANCHOR, Table};

impl crate::Document {
    /// The story text edits go to: a cell's story when `cell` is set, else the story itself.
    pub fn text_story(&self, story: StoryId, cell: Option<crate::CellAddr>) -> Option<&Story> {
        let st = self.story(story)?;
        match cell {
            Some(c) if c.table == crate::FOOTNOTE_TABLE => st.note(c.row as u64).map(|n| &n.text),
            Some(c) => st.tables.get(&c.table)?.cell(c.row, c.col).map(|x| &x.text),
            None => Some(st),
        }
    }

    /// Mutable [`crate::Document::text_story`] (a cell edit bumps the containing story's revision).
    pub fn text_story_mut(&mut self, story: StoryId, cell: Option<crate::CellAddr>) -> Option<&mut Story> {
        let st = self.story_mut(story)?;
        match cell {
            Some(c) if c.table == crate::FOOTNOTE_TABLE => st.note_mut(c.row as u64).map(|n| &mut n.text),
            Some(c) => st.table_mut(c.table)?.cell_mut(c.row, c.col).map(|x| &mut x.text),
            None => Some(st),
        }
    }
}

impl Story {
    /// Visit this story and every text inside it (table cells, footnotes), mutably.
    pub fn for_each_text_mut(&mut self, f: &mut impl FnMut(&mut Story)) {
        f(self);
        for t in self.tables.values_mut() {
            for c in &mut std::sync::Arc::make_mut(t).cells {
                c.text.for_each_text_mut(f);
            }
        }
        for n in &mut self.notes {
            std::sync::Arc::make_mut(n).text.for_each_text_mut(f);
        }
    }

    /// The table whose anchor paragraph is `pi`.
    pub fn para_table(&self, pi: usize) -> Option<&Arc<Table>> {
        self.tables.get(&self.paras.get(pi)?.table?)
    }

    /// Byte offset of a table's anchor character.
    pub fn table_anchor(&self, id: u64) -> Option<usize> {
        let pi = self.paras.iter().position(|p| p.table == Some(id))?;
        let r = self.para_ranges().get(pi)?.clone();
        self.text[r.clone()].find(TABLE_ANCHOR).map(|k| r.start + k)
    }

    /// The table anchored at or right before byte `pos` (a caret next to the anchor).
    pub fn table_at(&self, pos: usize) -> Option<u64> {
        let pi = self.para_at(pos);
        self.paras.get(pi)?.table
    }

    /// Mutable table (copy-on-write); bumps the story revision.
    pub fn table_mut(&mut self, id: u64) -> Option<&mut Table> {
        let t = self.tables.get_mut(&id)?;
        self.rev += 1;
        Some(Arc::make_mut(t))
    }

    /// Insert `table` at `pos`, giving its anchor its own paragraph. Returns the anchor's byte offset.
    pub fn insert_table(&mut self, pos: usize, table: Table) -> usize {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        let pi = self.para_at(pos);
        let pstart = self.para_ranges()[pi].start;
        let mut s = String::new();
        if pos > pstart {
            s.push('\n');
        }
        let anchor = pos + s.len();
        s.push(TABLE_ANCHOR);
        if !self.text[pos..].starts_with('\n') {
            s.push('\n');
        }
        let id = table.id;
        self.tables.insert(id, Arc::new(table));
        // Insert the text first (fix-up would drop an unreferenced table), then point the paragraph at it.
        self.insert_raw(pos, &s);
        let api = self.para_at(anchor);
        self.paras[api].table = Some(id);
        self.fix_tables();
        self.rev += 1;
        anchor
    }

    /// Keep paragraphs and tables consistent after an edit: a paragraph keeps its table only if it
    /// still contains an anchor character, every table is referenced once, and tables nobody
    /// references are dropped.
    pub(crate) fn fix_tables(&mut self) {
        if self.tables.is_empty() && self.paras.iter().all(|p| p.table.is_none()) {
            return;
        }
        let ranges = self.para_ranges();
        let mut used = std::collections::HashSet::new();
        for (p, r) in self.paras.iter_mut().zip(ranges) {
            if let Some(id) = p.table
                && (!self.text[r].contains(TABLE_ANCHOR) || !self.tables.contains_key(&id) || !used.insert(id))
            {
                p.table = None;
            }
        }
        self.tables.retain(|id, _| used.contains(id));
    }

    /// Check table invariants (part of [`Story::check`]).
    pub(crate) fn check_tables(&self) -> Result<(), String> {
        let ranges = self.para_ranges();
        let mut used = std::collections::HashSet::new();
        for (pi, p) in self.paras.iter().enumerate() {
            let Some(id) = p.table else { continue };
            let t = self.tables.get(&id).ok_or_else(|| format!("story {}: paragraph {pi} points at missing table {id}", self.id.0))?;
            if t.id != id {
                return Err(format!("story {}: table key {id} != id {}", self.id.0, t.id));
            }
            if self.text[ranges[pi].clone()].matches(TABLE_ANCHOR).count() != 1 {
                return Err(format!("story {}: table {id} paragraph must hold exactly one anchor", self.id.0));
            }
            if !used.insert(id) {
                return Err(format!("story {}: table {id} anchored twice", self.id.0));
            }
            t.check()?;
        }
        if used.len() != self.tables.len() {
            return Err(format!("story {}: {} tables but {} anchored", self.id.0, self.tables.len(), used.len()));
        }
        Ok(())
    }
}
