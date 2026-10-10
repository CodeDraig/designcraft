use std::collections::BTreeMap;

use super::*;

pub struct RenderContext<'a> {
    root: &'a Publication,
    references: &'a References,
    index: &'a SemanticIndex,
    layout: &'a ReferenceLayout,
    numbers: BTreeMap<String, BTreeMap<u64, String>>,
}

impl<'a> RenderContext<'a> {
    pub fn new(root: &'a Publication, references: &'a References, index: &'a SemanticIndex, layout: &'a ReferenceLayout) -> Result<Self> {
        let mut numbers = BTreeMap::new();
        for name in root.numbering.keys() {
            numbers.insert(
                name.clone(),
                root.numbers(name)?.into_iter().filter(|n| n.resolved).map(|n| (n.object.map_or(n.component.0, |id| id.0), n.label)).collect(),
            );
        }
        Ok(Self { root, references, index, layout, numbers })
    }

    /// Exact inputs for a cached evaluation. Query membership is always rechecked; only the
    /// selected entities and values consumed by this template invalidate its rendered text.
    pub fn input_signature(&self, field: &Field) -> Result<String> {
        let template = self.references.templates.get(&field.template).ok_or_else(|| invalid(format!("missing template {}", field.template)))?;
        template.check()?;
        let entities = self.index.query(&field.query)?;
        let mut key = format!("{field:?}{template:?}{entities:?}");
        for part in &template.parts {
            match &part.token {
                Token::PublicationTitle => key.push_str(&format!("{:?}", self.root.title)),
                Token::Metadata { key: name } => key.push_str(&format!("{:?}", self.root.metadata.get(name))),
                Token::Value { key: name } => key.push_str(&format!("{:?}", self.root.values.get(name))),
                Token::Number { sequence } => {
                    for e in &entities {
                        key.push_str(&format!("{:?}", self.numbers.get(sequence).and_then(|n| n.get(&e.id))));
                    }
                }
                Token::ComponentTitle => {
                    for e in &entities {
                        key.push_str(&format!("{:?}", self.root.components.get(&e.component).map(|c| &c.title)));
                    }
                }
                Token::Page => {
                    for e in &entities {
                        key.push_str(&format!("{:?}", self.layout.locations.get(&e.id).map(|l| &l.label)));
                    }
                }
                Token::GlobalPage => {
                    for e in &entities {
                        key.push_str(&format!("{:?}", self.layout.locations.get(&e.id).map(|l| l.global_page)));
                    }
                }
                _ => {}
            }
        }
        if field.use_ == FieldUse::RunningHead {
            key.push_str(&format!("{:?}", self.layout.first_body));
            for e in &entities {
                key.push_str(&format!("{:?}", self.layout.locations.get(&e.id)));
            }
        }
        Ok(key)
    }

    /// `page` supplies a component and its zero-based page. A missing page is a semantic preview.
    pub fn field(&self, field: &Field, page: Option<(ComponentId, usize)>) -> Result<(FieldText, Vec<Dependency>)> {
        let template = self.references.templates.get(&field.template).ok_or_else(|| invalid(format!("missing template {}", field.template)))?;
        template.check()?;
        if field.separator.len() > 1024 {
            return Err(invalid("field separator exceeds 1024 bytes"));
        }
        let mut entities = self.index.query(&field.query)?;
        let mut dependencies = vec![Dependency::ReadingOrder, Dependency::Template(template.name.clone())];
        dependencies.extend(entities.iter().map(|e| Dependency::Entity(e.id)));
        match field.use_ {
            FieldUse::First => entities.truncate(1),
            FieldUse::Join => {}
            FieldUse::RunningHead => {
                let Some((component, page)) = page else {
                    return Ok((FieldText::default(), dependencies));
                };
                dependencies.push(Dependency::Pagination(component));
                let body = self.layout.first_body.get(&component);
                if field.suppress_opening && body.and_then(|b| b.keys().next()).copied() == Some(page) {
                    return Ok((FieldText::default(), dependencies));
                }
                let first = body.and_then(|b| b.get(&page));
                let pick = entities
                    .into_iter()
                    .filter(|e| e.resolved && e.component == component)
                    .filter_map(|e| {
                        let location = self.layout.locations.get(&e.id)?;
                        let before = first.map_or(location.page < page, |first| location.order < *first);
                        before.then_some((location.order.clone(), e))
                    })
                    .max_by(|a, b| a.0.cmp(&b.0));
                entities = pick.into_iter().map(|(_, e)| e).collect();
            }
        }
        let mut out = FieldText::default();
        if entities.is_empty() && field.use_ != FieldUse::RunningHead {
            let message = "Reference query has no matching destination".to_string();
            out.unresolved.push(message);
            out.runs.push(RichRun { text: "[Unresolved reference]".into(), format: CharFormat::default(), target: None });
        }
        let mut size = 0usize;
        for (i, entity) in entities.iter().enumerate() {
            if i > 0 {
                out.runs.push(RichRun { text: field.separator.clone(), format: CharFormat::default(), target: None });
                size += field.separator.len();
            }
            if !entity.resolved {
                let text = format!("[Unresolved object {}]", entity.id);
                out.unresolved.push(text.clone());
                size += text.len();
                out.runs.push(RichRun { text, format: CharFormat::default(), target: None });
            } else {
                for part in &template.parts {
                    let missing = |kind: &str, name: &str, out: &mut FieldText| {
                        out.unresolved.push(format!("Missing {kind}: {name}"));
                        format!("[Missing {kind}: {name}]")
                    };
                    let text = match &part.token {
                        Token::Literal { text } => text.clone(),
                        Token::Title => entity.title.clone(),
                        Token::PublicationTitle => {
                            dependencies.push(Dependency::PublicationTitle);
                            self.root.title.clone()
                        }
                        Token::ComponentTitle => self.root.components.get(&entity.component).map(|c| c.title.clone()).unwrap_or_default(),
                        Token::Number { sequence } => {
                            dependencies.push(Dependency::Numbering(sequence.clone()));
                            self.numbers
                                .get(sequence)
                                .and_then(|n| n.get(&entity.id))
                                .cloned()
                                .unwrap_or_else(|| missing("number", sequence, &mut out))
                        }
                        Token::Page | Token::GlobalPage => {
                            dependencies.push(Dependency::Pagination(entity.component));
                            match self.layout.locations.get(&entity.id) {
                                Some(location) if matches!(part.token, Token::GlobalPage) => (location.global_page + 1).to_string(),
                                Some(location) => location.label.clone(),
                                None => {
                                    out.unresolved.push(format!("Unplaced destination {}", entity.id));
                                    "?".into()
                                }
                            }
                        }
                        Token::Metadata { key } => {
                            dependencies.push(Dependency::Metadata(key.clone()));
                            self.root.metadata.get(key).cloned().unwrap_or_else(|| missing("metadata", key, &mut out))
                        }
                        Token::Value { key } => {
                            dependencies.push(Dependency::Value(key.clone()));
                            self.root.values.get(key).cloned().unwrap_or_else(|| missing("value", key, &mut out))
                        }
                    };
                    size = size.saturating_add(text.len());
                    if size > 1024 * 1024 {
                        return Err(invalid("generated field exceeds 1 MiB; narrow the query or template"));
                    }
                    out.runs.push(RichRun { text, format: part.format.clone(), target: Some(entity.id) });
                }
            }
        }
        let mut unique = Vec::new();
        for dependency in dependencies {
            if !unique.contains(&dependency) {
                unique.push(dependency);
            }
        }
        out.unresolved.sort();
        out.unresolved.dedup();
        Ok((out, unique))
    }
}
