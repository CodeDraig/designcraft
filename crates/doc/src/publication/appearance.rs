//! Edition typography and table/object presentation, without a second copy of authored text.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{Cell, CharRun, Column, Item, ParaFormat, Result, Row, Story, Table, TableOptions};

use super::invalid;

const MAX_NESTING: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoryAppearance {
    paras: Vec<ParaFormat>,
    chars: Vec<CharRun>,
    tables: BTreeMap<u64, TableAppearance>,
    notes: BTreeMap<u64, StoryAppearance>,
    endnotes: BTreeMap<u64, StoryAppearance>,
    objects: BTreeMap<crate::ItemId, crate::anchored::AnchoredObject>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TableAppearance {
    options: TableOptions,
    style: String,
    /// Every surviving cell can identify its row/column after insertions and deletions.
    columns: BTreeMap<u64, Column>,
    rows: BTreeMap<u64, Row>,
    cells: BTreeMap<u64, CellAppearance>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CellAppearance {
    fill: String,
    fill_tint: f32,
    insets: [f64; 4],
    vj: crate::VerticalJustification,
    rotation: f64,
    strokes: [crate::CellStroke; 4],
    border_overrides: [bool; 4],
    style: String,
    text: StoryAppearance,
}

impl CellAppearance {
    fn capture(c: &Cell, depth: usize) -> Result<Self> {
        Ok(Self {
            fill: c.fill.clone(),
            fill_tint: c.fill_tint,
            insets: c.insets,
            vj: c.vj,
            rotation: c.rotation,
            strokes: c.strokes.clone(),
            border_overrides: c.border_overrides,
            style: c.style.clone(),
            text: StoryAppearance::capture_at(&c.text, depth)?,
        })
    }

    fn apply(&self, cell: &mut Cell, depth: usize) -> Result<()> {
        cell.fill = self.fill.clone();
        cell.fill_tint = self.fill_tint;
        cell.insets = self.insets;
        cell.vj = self.vj;
        cell.rotation = self.rotation;
        cell.strokes = self.strokes.clone();
        cell.border_overrides = self.border_overrides;
        cell.style = self.style.clone();
        self.text.apply_at(&mut cell.text, depth)
    }
}

impl TableAppearance {
    fn capture(table: &Table, depth: usize) -> Result<Self> {
        let mut rows = BTreeMap::new();
        let mut columns = BTreeMap::new();
        for (row, cells) in table.rows.iter().zip(table.cells.chunks(table.ncols().max(1))) {
            for (cell, column) in cells.iter().zip(&table.columns) {
                rows.insert(cell.id, row.clone());
                columns.insert(cell.id, column.clone());
            }
        }
        Ok(Self {
            options: table.options.clone(),
            style: table.style.clone(),
            columns,
            rows,
            cells: table.cells.iter().map(|c| Ok((c.id, CellAppearance::capture(c, depth)?))).collect::<Result<_>>()?,
        })
    }

    fn apply_geometry(&self, table: &mut Table) {
        table.options = self.options.clone();
        table.style = self.style.clone();
        let nc = table.ncols().max(1);
        for (i, column) in table.columns.iter_mut().enumerate() {
            if let Some(previous) = table.cells.iter().skip(i).step_by(nc).find_map(|c| self.columns.get(&c.id)) {
                *column = previous.clone();
            }
        }
        for (row, cells) in table.rows.iter_mut().zip(table.cells.chunks(nc)) {
            if let Some(previous) = cells.iter().find_map(|c| self.rows.get(&c.id)) {
                row.height = previous.height;
                row.mode = previous.mode;
            }
        }
    }
}

impl StoryAppearance {
    pub(super) fn capture(story: &Story) -> Result<Self> {
        Self::capture_at(story, 0)
    }

    fn capture_at(story: &Story, depth: usize) -> Result<Self> {
        nesting(depth)?;
        Ok(Self {
            paras: story.paras.clone(),
            chars: story.chars.clone(),
            tables: story.tables.iter().map(|(id, t)| Ok((*id, TableAppearance::capture(t, depth + 1)?))).collect::<Result<_>>()?,
            notes: story.notes.iter().map(|n| Ok((n.id, Self::capture_at(&n.text, depth + 1)?))).collect::<Result<_>>()?,
            endnotes: story.endnotes.iter().map(|n| Ok((n.id, Self::capture_at(&n.text, depth + 1)?))).collect::<Result<_>>()?,
            objects: story.objects.iter().map(|o| (o.item.id, (**o).clone())).collect(),
        })
    }

    pub(super) fn apply(&self, story: &mut Story) -> Result<()> {
        self.apply_at(story, 0)
    }

    fn apply_at(&self, story: &mut Story, depth: usize) -> Result<()> {
        nesting(depth)?;
        if self.paras.len() != story.paras.len() || self.chars.iter().try_fold(0usize, |n, r| n.checked_add(r.len)) != Some(story.len()) {
            return Err(invalid("edition formatting does not match canonical text"));
        }
        // Keep the shared Arc when this edition already uses the canonical default presentation.
        if *self == Self::capture_at(story, depth)? {
            return Ok(());
        }
        let mut paras = self.paras.clone();
        for (p, canonical) in paras.iter_mut().zip(&story.paras) {
            p.table = canonical.table;
            shared_attributes(&mut p.chars, &canonical.chars);
        }
        let chars = semantic_runs(&self.chars, &story.chars);
        story.paras = paras;
        story.chars = chars;
        for (id, table) in &mut story.tables {
            if let Some(appearance) = self.tables.get(id) {
                let table = Arc::make_mut(table);
                appearance.apply_geometry(table);
                for cell in &mut table.cells {
                    if let Some(a) = appearance.cells.get(&cell.id) {
                        a.apply(cell, depth + 1)?;
                    }
                }
            }
        }
        for n in &mut story.notes {
            if let Some(a) = self.notes.get(&n.id) {
                a.apply_at(&mut Arc::make_mut(n).text, depth + 1)?;
            }
        }
        for n in &mut story.endnotes {
            if let Some(a) = self.endnotes.get(&n.id) {
                a.apply_at(&mut Arc::make_mut(n).text, depth + 1)?;
            }
        }
        for object in &mut story.objects {
            if let Some(a) = self.objects.get(&object.item.id) {
                let object = Arc::make_mut(object);
                item_presentation(&a.item, &mut object.item);
                object.position = a.position.clone();
            }
        }
        Ok(())
    }
}

fn nesting(depth: usize) -> Result<()> {
    if depth > MAX_NESTING { Err(invalid("publication nested table/note limit exceeded (32)")) } else { Ok(()) }
}

/// Content-bearing character attributes stay canonical even when typography differs.
fn semantic_runs(appearance: &[CharRun], canonical: &[CharRun]) -> Vec<CharRun> {
    let mut out = Vec::new();
    let (mut i, mut j, mut x, mut y) = (0, 0, 0, 0);
    while let (Some(a), Some(c)) = (appearance.get(i), canonical.get(j)) {
        let len = (a.len - x).min(c.len - y);
        let mut format = a.format.clone();
        shared_attributes(&mut format.over, &c.format.over);
        if let Some(prior) = out.last_mut().filter(|r: &&mut CharRun| r.format == format) {
            prior.len += len;
        } else {
            out.push(CharRun { len, format });
        }
        x += len;
        y += len;
        if x == a.len {
            i += 1;
            x = 0;
        }
        if y == c.len {
            j += 1;
            y = 0;
        }
    }
    out
}

fn shared_attributes(appearance: &mut crate::CharAttrs, canonical: &crate::CharAttrs) {
    appearance.conditions = canonical.conditions.clone();
    appearance.change = canonical.change;
    appearance.change_id = canonical.change_id;
    appearance.language = canonical.language.clone();
    appearance.xml_tag = canonical.xml_tag.clone();
}

fn item_presentation(from: &Item, to: &mut Item) {
    macro_rules! copy { ($($field:ident),*) => { $(to.$field = from.$field.clone();)* }; }
    copy!(path, xf, fill, stroke, corners, opacity, blend, effects, wrap, hidden, nonprinting, object_style, liquid);
    if let (Some(a), Some(b)) = (from.text_frame(), to.text_frame_mut()) {
        b.options = a.options.clone();
    }
}

/// Carry an edition's presentation through a new shared story. Structure and semantic markers
/// always come from `canonical`; only surviving typography and layout are taken from `previous`.
pub(super) fn carry(previous: &Story, canonical: &Story) -> Result<Story> {
    carry_at(previous, canonical, 0)
}

fn carry_at(previous: &Story, canonical: &Story, depth: usize) -> Result<Story> {
    nesting(depth)?;
    let mut out = canonical.clone();
    let mut formatted = previous.clone();
    for edit in super::text_edits::edits(&previous.text, &canonical.text)?.into_iter().rev() {
        formatted.replace(edit.before, &canonical.text[edit.after]);
    }
    out.paras = formatted.paras.clone();
    for (p, source) in out.paras.iter_mut().zip(&canonical.paras) {
        p.table = source.table;
        shared_attributes(&mut p.chars, &source.chars);
    }
    out.chars = semantic_runs(&formatted.chars, &canonical.chars);
    for (id, table) in &mut out.tables {
        if let Some(old) = previous.tables.get(id) {
            let appearance = TableAppearance::capture(old, depth + 1)?;
            let table = Arc::make_mut(table);
            appearance.apply_geometry(table);
            let old_cells: BTreeMap<_, _> = old.cells.iter().map(|c| (c.id, c)).collect();
            for cell in &mut table.cells {
                if let Some(old) = old_cells.get(&cell.id) {
                    let text = carry_at(&old.text, &cell.text, depth + 1)?;
                    let a = CellAppearance::capture(old, depth + 1)?;
                    cell.text = old.text.clone();
                    a.apply(cell, depth + 1)?;
                    cell.text = text;
                }
            }
        }
    }
    for n in &mut out.notes {
        if let Some(old) = previous.notes.iter().find(|a| a.id == n.id) {
            Arc::make_mut(n).text = carry_at(&old.text, &n.text, depth + 1)?;
        }
    }
    for n in &mut out.endnotes {
        if let Some(old) = previous.endnotes.iter().find(|a| a.id == n.id) {
            Arc::make_mut(n).text = carry_at(&old.text, &n.text, depth + 1)?;
        }
    }
    for o in &mut out.objects {
        if let Some(old) = previous.objects.iter().find(|a| a.item.id == o.item.id) {
            let o = Arc::make_mut(o);
            item_presentation(&old.item, &mut o.item);
            o.position = old.position.clone();
        }
    }
    out.frames = previous.frames.clone();
    out.direction = previous.direction;
    out.vertical = previous.vertical;
    out.link = previous.link;
    out.rev = previous.rev.saturating_add(1);
    out.check().map_err(invalid)?;
    Ok(out)
}

pub(super) fn assign_cell_ids(story: &mut Story) -> Result<()> {
    assign_at(story, 0)
}

fn assign_at(story: &mut Story, depth: usize) -> Result<()> {
    nesting(depth)?;
    for table in story.tables.values_mut() {
        let needs = table.cells.iter().any(|c| c.id == 0) || table.cells.iter().map(|c| c.id).max().unwrap_or(0) > table.next_cell_id;
        let mut ids = BTreeSet::new();
        if table.cells.iter().any(|c| c.id != 0 && !ids.insert(c.id)) {
            return Err(invalid("duplicate table cell identity"));
        }
        let table = Arc::make_mut(table);
        if needs {
            table.next_cell_id = table.next_cell_id.max(ids.last().copied().unwrap_or(0));
            for cell in &mut table.cells {
                if cell.id == 0 {
                    table.next_cell_id = table.next_cell_id.checked_add(1).ok_or_else(|| invalid("table cell identity exhausted"))?;
                    cell.id = table.next_cell_id;
                }
            }
        }
        for cell in &mut table.cells {
            assign_at(&mut cell.text, depth + 1)?;
        }
    }
    for note in &mut story.notes {
        assign_at(&mut Arc::make_mut(note).text, depth + 1)?;
    }
    for note in &mut story.endnotes {
        assign_at(&mut Arc::make_mut(note).text, depth + 1)?;
    }
    Ok(())
}
