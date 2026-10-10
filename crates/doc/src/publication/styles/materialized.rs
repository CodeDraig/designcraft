//! Update attributes that legacy style application copied onto cells, tables and objects.
//! A value different from the prior style is a local override and is retained.
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{Cell, CellStyle, Document, Item, ObjectStyle, Result, Story, Styles, Table, TableStyle};

fn update<T: Clone + PartialEq>(current: &mut T, before: Option<&T>, after: Option<&T>, default: &T) {
    if let Some(after) = after
        && current == before.unwrap_or(default)
    {
        *current = after.clone();
    }
}

fn paragraph(story: &mut Story, before: Option<&String>, after: Option<&String>) {
    let Some(after) = after else { return };
    for p in &mut story.paras {
        update(&mut p.style, before, Some(after), &crate::story::BASIC_PARAGRAPH.to_owned());
    }
    story.rev = story.rev.saturating_add(1);
}

fn cell(cell: &mut Cell, before: Option<&CellStyle>, after: &CellStyle) {
    let default = Cell::default();
    update(&mut cell.fill, before.and_then(|s| s.fill.as_ref()), after.fill.as_ref(), &default.fill);
    update(&mut cell.fill_tint, before.and_then(|s| s.fill_tint.as_ref()), after.fill_tint.as_ref(), &default.fill_tint);
    for edge in 0..4 {
        update(
            &mut cell.insets[edge],
            before.and_then(|s| s.insets.as_ref()).map(|v| &v[edge]),
            after.insets.as_ref().map(|v| &v[edge]),
            &default.insets[edge],
        );
        if !cell.border_overrides[edge] {
            update(&mut cell.strokes[edge], before.and_then(|s| s.stroke.as_ref()), after.stroke.as_ref(), &default.strokes[edge]);
        }
    }
    update(&mut cell.vj, before.and_then(|s| s.vj.as_ref()), after.vj.as_ref(), &default.vj);
    paragraph(&mut cell.text, before.and_then(|s| s.paragraph_style.as_ref()), after.paragraph_style.as_ref());
    cell.style = if after.name == crate::styles::NO_CELL_STYLE { String::new() } else { after.name.clone() };
}

fn region(style: &TableStyle, kind: crate::RowKind, col: usize, ncols: usize) -> Option<&String> {
    match kind {
        crate::RowKind::Header => style.header.as_ref(),
        crate::RowKind::Footer => style.footer.as_ref(),
        _ if col == 0 && style.left_column.is_some() => style.left_column.as_ref(),
        _ if col + 1 == ncols && style.right_column.is_some() => style.right_column.as_ref(),
        _ => style.body.as_ref(),
    }
}

fn table(table: &mut Table, before: &Styles, after: &Styles) {
    let name = if table.style.is_empty() { crate::styles::BASIC_TABLE } else { &table.style };
    let old = before.table.iter().find(|s| s.name == name);
    let new = after.table.iter().find(|s| s.name == name);
    let default = crate::TableOptions::default();
    if let Some(new) = new {
        update(&mut table.options.border, old.and_then(|s| s.border.as_ref()), new.border.as_ref(), &default.border);
        update(&mut table.options.space_before, old.and_then(|s| s.space_before.as_ref()), new.space_before.as_ref(), &default.space_before);
        update(&mut table.options.space_after, old.and_then(|s| s.space_after.as_ref()), new.space_after.as_ref(), &default.space_after);
        if table.options.alt_rows == old.and_then(|s| s.alt_rows.clone()) && new.alt_rows.is_some() {
            table.options.alt_rows = new.alt_rows.clone();
        }
    }
    let nc = table.ncols().max(1);
    for (i, c) in table.cells.iter_mut().enumerate() {
        let kind = table.rows.get(i / nc).map(|r| r.kind).unwrap_or_default();
        let col = if table.options.direction == crate::TextDirection::RightToLeft { nc - 1 - i % nc } else { i % nc };
        let old_region = old.and_then(|s| region(s, kind, col, nc));
        let new_region = new.and_then(|s| region(s, kind, col, nc));
        let name = if c.style.is_empty() { crate::styles::NO_CELL_STYLE } else { &c.style };
        let previous = before.cell.iter().find(|s| s.name == name);
        let next = if old_region != new_region && old_region.map_or(c.style.is_empty(), |s| c.style == *s) {
            new_region.and_then(|name| after.cell.iter().find(|s| s.name == *name))
        } else {
            after.cell.iter().find(|s| s.name == name)
        };
        if let Some(next) = next
            && previous != Some(next)
        {
            cell(c, previous, next);
        }
    }
}

fn object(
    item: &mut Item,
    before: &Styles,
    after: &Styles,
    paras: &mut BTreeMap<crate::StoryId, (Option<String>, Option<String>)>,
    depth: usize,
) -> Result<()> {
    if depth > 32 {
        return Err(super::super::invalid("object nesting exceeds publication style limit (32)"));
    }
    if let Some(new) = after.object_style(&item.object_style) {
        let old = before.object_style(&item.object_style);
        if old != Some(new) {
            let default = ObjectStyle::default();
            let old = old.unwrap_or(&default);
            update(&mut item.fill, old.fill.as_ref(), new.fill.as_ref(), &crate::Fill::none());
            update(&mut item.stroke, old.stroke.as_ref(), new.stroke.as_ref(), &crate::Stroke::none());
            if let Some(tf) = item.text_frame_mut() {
                update(&mut tf.options, old.text_frame.as_ref(), new.text_frame.as_ref(), &Default::default());
                paras.insert(tf.story, (old.paragraph_style.clone(), new.paragraph_style.clone()));
            }
        }
    }
    if let crate::Content::Group { items } = &mut item.content {
        for child in items {
            object(Arc::make_mut(child), before, after, paras, depth + 1)?;
        }
    }
    Ok(())
}

fn story(
    story: &mut Story,
    before: &Styles,
    after: &Styles,
    paras: &mut BTreeMap<crate::StoryId, (Option<String>, Option<String>)>,
    depth: usize,
) -> Result<()> {
    if depth > 32 {
        return Err(super::super::invalid("note/table nesting exceeds publication style limit (32)"));
    }
    for t in story.tables.values_mut() {
        let t = Arc::make_mut(t);
        table(t, before, after);
        for cell in &mut t.cells {
            self::story(&mut cell.text, before, after, paras, depth + 1)?;
        }
    }
    for n in &mut story.notes {
        self::story(&mut Arc::make_mut(n).text, before, after, paras, depth + 1)?;
    }
    for n in &mut story.endnotes {
        self::story(&mut Arc::make_mut(n).text, before, after, paras, depth + 1)?;
    }
    for o in &mut story.objects {
        object(&mut Arc::make_mut(o).item, before, after, paras, depth + 1)?;
    }
    story.rev = story.rev.saturating_add(1);
    Ok(())
}

pub(super) fn refresh(doc: &mut Document, before: &Styles) -> Result<()> {
    let after = doc.styles.clone();
    let mut paras = BTreeMap::new();
    for spread in doc.spreads.iter_mut().chain(&mut doc.parents) {
        for item in &mut Arc::make_mut(spread).items {
            object(Arc::make_mut(item), before, &after, &mut paras, 0)?;
        }
    }
    for st in doc.stories.values_mut() {
        story(Arc::make_mut(st), before, &after, &mut paras, 0)?;
    }
    for (id, (old, new)) in paras {
        if let Some(st) = doc.story_mut(id) {
            paragraph(st, old.as_ref(), new.as_ref());
        }
    }
    Ok(())
}
