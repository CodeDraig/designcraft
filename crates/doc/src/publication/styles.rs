//! Selective synchronization of named definitions. A baseline is the last definition explicitly
//! adopted by a component; different imported definitions have no baseline and remain overrides.
use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{CellStyle, CharacterStyle, ComponentId, Document, EditionId, ObjectStyle, ParagraphStyle, Result, Swatch, TableStyle};

use super::{Publication, invalid};
mod materialized;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "definition", rename_all = "camelCase")]
pub enum Definition {
    Paragraph(Box<ParagraphStyle>),
    Character(Box<CharacterStyle>),
    Object(Box<ObjectStyle>),
    Cell(Box<CellStyle>),
    Table(Box<TableStyle>),
    Swatch(Box<Swatch>),
    ExportTag { name: String, value: crate::ExportTag },
    CompositeFont(Box<crate::cjk::CompositeFont>),
    Mojikumi(Box<crate::cjk::MojikumiTable>),
}

impl Definition {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Paragraph(_) => "paragraph",
            Self::Character(_) => "character",
            Self::Object(_) => "object",
            Self::Cell(_) => "cell",
            Self::Table(_) => "table",
            Self::Swatch(_) => "swatch",
            Self::ExportTag { .. } => "exportTag",
            Self::CompositeFont(_) => "compositeFont",
            Self::Mojikumi(_) => "mojikumi",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Paragraph(s) => &s.name,
            Self::Character(s) => &s.name,
            Self::Object(s) => &s.name,
            Self::Cell(s) => &s.name,
            Self::Table(s) => &s.name,
            Self::Swatch(s) => &s.name,
            Self::ExportTag { name, .. } => name,
            Self::CompositeFont(s) => &s.name,
            Self::Mojikumi(s) => &s.name,
        }
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.kind(), self.name())
    }

    fn apply(&self, doc: &mut Document) {
        macro_rules! replace {
            ($items:expr, $value:expr) => {{
                if let Some(item) = $items.iter_mut().find(|s| s.name == $value.name) {
                    *item = $value.clone();
                } else {
                    $items.push($value.clone());
                }
            }};
        }
        match self {
            Self::Paragraph(s) => replace!(Arc::make_mut(&mut doc.styles).paragraph, s.as_ref()),
            Self::Character(s) => replace!(Arc::make_mut(&mut doc.styles).character, s.as_ref()),
            Self::Object(s) => replace!(Arc::make_mut(&mut doc.styles).object, s.as_ref()),
            Self::Cell(s) => replace!(Arc::make_mut(&mut doc.styles).cell, s.as_ref()),
            Self::Table(s) => replace!(Arc::make_mut(&mut doc.styles).table, s.as_ref()),
            Self::Swatch(s) => replace!(doc.swatches, s.as_ref()),
            Self::ExportTag { name, value } => {
                Arc::make_mut(&mut doc.styles).export_tags.insert(name.clone(), value.clone());
            }
            Self::CompositeFont(s) => replace!(Arc::make_mut(&mut doc.styles).composite_fonts, s.as_ref()),
            Self::Mojikumi(s) => replace!(Arc::make_mut(&mut doc.styles).mojikumi_tables, s.as_ref()),
        }
    }

    /// Named dependencies are shown in previews and checked against the final selection.
    pub fn dependencies(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut add = |kind: &str, name: Option<&String>| {
            if let Some(name) = name {
                out.push(format!("{kind}:{name}"));
            }
        };
        match self {
            Self::Paragraph(s) => {
                add("paragraph", s.based_on.as_ref());
                add("paragraph", s.next_style.as_ref());
            }
            Self::Character(s) => add("character", s.based_on.as_ref()),
            Self::Object(s) => {
                add("object", s.based_on.as_ref());
                add("paragraph", s.paragraph_style.as_ref());
            }
            Self::Cell(s) => add("paragraph", s.paragraph_style.as_ref()),
            Self::Table(s) => {
                for name in [&s.header, &s.body, &s.footer, &s.left_column, &s.right_column] {
                    add("cell", name.as_ref());
                }
            }
            Self::Swatch(s) => {
                if let designcraft_color::SwatchValue::Tint { base, .. } = &s.value {
                    add("swatch", Some(base));
                }
            }
            Self::ExportTag { name, .. } => {
                if let Some((prefix, name)) = name.split_once(':') {
                    out.push(format!("{}:{name}", if prefix == "p" { "paragraph" } else { "character" }));
                }
            }
            Self::CompositeFont(_) | Self::Mojikumi(_) => {}
        }
        out
    }
}

pub fn definitions(doc: &Document) -> BTreeMap<String, Definition> {
    doc.styles
        .paragraph
        .iter()
        .cloned()
        .map(|s| Definition::Paragraph(Box::new(s)))
        .chain(doc.styles.character.iter().cloned().map(|s| Definition::Character(Box::new(s))))
        .chain(doc.styles.object.iter().cloned().map(|s| Definition::Object(Box::new(s))))
        .chain(doc.styles.cell.iter().cloned().map(|s| Definition::Cell(Box::new(s))))
        .chain(doc.styles.table.iter().cloned().map(|s| Definition::Table(Box::new(s))))
        .chain(doc.swatches.iter().cloned().map(|s| Definition::Swatch(Box::new(s))))
        .chain(doc.styles.export_tags.iter().map(|(name, value)| Definition::ExportTag { name: name.clone(), value: value.clone() }))
        .chain(doc.styles.composite_fonts.iter().cloned().map(|s| Definition::CompositeFont(Box::new(s))))
        .chain(doc.styles.mojikumi_tables.iter().cloned().map(|s| Definition::Mojikumi(Box::new(s))))
        .map(|s| (s.key(), s))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleChange {
    pub component: ComponentId,
    pub component_title: String,
    pub key: String,
    pub before: Option<Definition>,
    pub after: Definition,
    /// Existing content differs from both the source and its last explicitly adopted definition.
    pub overridden: bool,
    pub changed: bool,
    pub dependencies: Vec<String>,
}

impl Publication {
    pub fn style_preview(
        &self,
        source_edition: EditionId,
        source: ComponentId,
        edition: EditionId,
        targets: &[ComponentId],
    ) -> Result<Vec<StyleChange>> {
        let definitions = definitions(&self.project(source_edition, source)?);
        let mut out = Vec::new();
        for &component in targets {
            if component == source && source_edition == edition {
                continue;
            }
            let target = self.project(edition, component)?;
            let existing = self::definitions(&target);
            let layout = self.editions.get(&edition).and_then(|e| e.components.get(&component)).ok_or_else(|| invalid("unknown style target"))?;
            for (key, after) in &definitions {
                let before = existing.get(key).cloned();
                let changed = before.as_ref() != Some(after);
                let overridden = changed && before.is_some() && layout.style_baselines.get(key) != before.as_ref();
                out.push(StyleChange {
                    component,
                    component_title: target.title.clone(),
                    key: key.clone(),
                    before,
                    after: after.clone(),
                    overridden,
                    changed,
                    dependencies: after.dependencies(),
                });
            }
        }
        Ok(out)
    }

    /// Apply selected rows from a preview of this exact root. The engine owns preview freshness.
    pub fn apply_styles(&mut self, edition: EditionId, changes: &[StyleChange]) -> Result<()> {
        let mut grouped: BTreeMap<ComponentId, Vec<&StyleChange>> = BTreeMap::new();
        for row in changes {
            grouped.entry(row.component).or_default().push(row);
        }
        for (component, rows) in grouped {
            let mut doc = self.project(edition, component)?;
            let previous = doc.styles.clone();
            for row in &rows {
                row.after.apply(&mut doc);
            }
            let available = definitions(&doc);
            for row in &rows {
                for dependency in row.after.dependencies() {
                    if !available.contains_key(&dependency) {
                        return Err(invalid(format!("{} requires {dependency}; include it in the style selection", row.key)));
                    }
                }
                match &row.after {
                    Definition::Paragraph(s) if s.based_on.as_ref().is_some_and(|p| doc.styles.para_based_on_cycles(&s.name, p)) => {
                        return Err(invalid(format!("{} introduces a paragraph style cycle", row.key)));
                    }
                    Definition::Character(s) if s.based_on.as_ref().is_some_and(|p| doc.styles.char_based_on_cycles(&s.name, p)) => {
                        return Err(invalid(format!("{} introduces a character style cycle", row.key)));
                    }
                    _ => {}
                }
            }
            materialized::refresh(&mut doc, &previous)?;
            // The edition owns materialized style attributes as well as its named definitions.
            // Capturing presentation here leaves canonical content and every other edition intact.
            let e = Arc::make_mut(self.editions.get_mut(&edition).ok_or_else(|| invalid("unknown edition"))?);
            let mut layout = super::ComponentLayout::capture(&doc)?;
            layout.style_baselines = e.components.get(&component).ok_or_else(|| invalid("unknown component"))?.style_baselines.clone();
            for row in rows {
                layout.style_baselines.insert(row.key.clone(), row.after.clone());
            }
            e.components.insert(component, Arc::new(layout));
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
}
