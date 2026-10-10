//! One output page table for a complete publication. Component-local identifiers never alias.
use super::*;
use std::collections::BTreeMap;

pub(crate) struct PublicationSheets<'a> {
    pub docs: &'a [Document],
    pub sheets: Vec<Vec<Sheet>>,
    pub offsets: Vec<usize>,
    pub count: usize,
    components: BTreeMap<designcraft_doc::ComponentId, usize>,
    anchors: BTreeMap<(usize, u64), (usize, usize)>,
}

impl<'a> PublicationSheets<'a> {
    pub fn new(docs: &'a [Document], cache: &Cache, opts: &PdfOptions) -> Result<Self> {
        let total: usize = docs.iter().map(Document::page_count).sum();
        if total > 100_000 {
            return Err(PdfError::Write("publication PDF exceeds 100000 pages".into()));
        }
        if let Some(bad) = opts.pages.as_ref().and_then(|p| p.iter().find(|p| **p >= total)) {
            return Err(PdfError::BadPage(bad + 1));
        }
        let mut result = Self { docs, sheets: Vec::new(), offsets: Vec::new(), count: 0, components: BTreeMap::new(), anchors: BTreeMap::new() };
        let mut first = 0;
        for (part, doc) in docs.iter().enumerate() {
            if let Some(v) = &doc.publication {
                result.components.insert(v.component, part);
            }
            let mut local = opts.clone();
            local.pages = opts
                .pages
                .as_ref()
                .map(|pages| pages.iter().filter(|p| **p >= first && **p < first + doc.page_count()).map(|p| *p - first).collect());
            let sheets = if local.pages.as_ref().is_some_and(Vec::is_empty) { Vec::new() } else { sheets(doc, &local)? };
            result.offsets.push(result.count);
            result.count += sheets.len();
            result.sheets.push(sheets);
            first += doc.page_count();
        }
        if result.count == 0 {
            return Err(PdfError::NoPages);
        }
        let owners: BTreeMap<_, _> = docs
            .first()
            .and_then(|d| d.publication.as_ref())
            .map(|v| v.root.annotation_entries())
            .transpose()
            .map_err(|e| PdfError::Write(e.to_string()))?
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.identity.id, result.components.get(&e.component).copied().unwrap_or(usize::MAX)))
            .collect();
        for (part, doc) in docs.iter().enumerate() {
            for st in doc.stories.values() {
                let cs = cache.get(doc, st.id, None);
                collect_anchors(doc, st, &cs, None, part, part, &owners, &mut result.anchors, 0);
            }
        }
        Ok(result)
    }
    pub fn part(&self, component: Option<designcraft_doc::ComponentId>, fallback: usize) -> Option<usize> {
        match component {
            Some(id) => self.components.get(&id).copied(),
            None => Some(fallback),
        }
    }
    pub fn page(&self, part: usize, page: usize) -> Option<usize> {
        crate::links::sheet_of_page(self.docs.get(part)?, self.sheets.get(part)?, page).map(|i| self.offsets[part] + i)
    }
    pub fn anchor(&self, part: usize, id: u64) -> Option<usize> {
        self.anchors.get(&(part, id)).and_then(|(owner, page)| self.page(*owner, *page))
    }
    pub fn entity(&self, part: usize, id: u64) -> Option<usize> {
        let v = self.docs.get(part)?.publication.as_ref()?;
        let loc = v.root.editions.get(&v.edition)?.references.locations.get(&id)?;
        self.page(*self.components.get(&loc.component)?, loc.page)
    }
    pub fn outline(&self, cache: &Cache) -> Option<krilla::outline::Outline> {
        use krilla::{destination::XyzDestination, geom::Point, outline::OutlineNode};
        let mut outline = krilla::outline::Outline::new();
        let mut count = 0;
        for (part, doc) in self.docs.iter().enumerate() {
            if self.sheets[part].is_empty() {
                continue;
            }
            let children = crate::links::outline_nodes(doc, cache, &self.sheets[part], &|page| self.page(part, page), &|id| self.anchor(part, id));
            if self.docs.len() == 1 {
                for node in children {
                    outline.push_child(node);
                    count += 1;
                }
            } else {
                let mut node = OutlineNode::new(doc.title.clone(), XyzDestination::new(self.offsets[part], Point::from_xy(0., 0.)));
                for child in children {
                    node.push_child(child);
                }
                outline.push_child(node);
                count += 1;
            }
        }
        (count > 0).then_some(outline)
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_anchors(
    doc: &Document,
    story: &designcraft_doc::Story,
    cs: &designcraft_compose::ComposedStory,
    page: Option<usize>,
    display: usize,
    owner: usize,
    owners: &BTreeMap<u64, usize>,
    out: &mut BTreeMap<(usize, u64), (usize, usize)>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    let story = cs.source.as_deref().unwrap_or(story);
    for ft in &cs.frames {
        let Some(page) = page.or_else(|| doc.page_of_item(ft.frame)) else { continue };
        for ((at, _), a) in story.text.match_indices(designcraft_doc::ANCHOR_MARK).zip(&story.anchors) {
            if ft.range.contains(&at) {
                out.entry((owner, a.id)).or_insert((display, page));
            }
        }
        for t in &ft.tables {
            if let Some(source) = story.tables.get(&t.table) {
                for c in &t.cells {
                    if let Some(cell) = source.cell(c.row, c.col) {
                        collect_anchors(doc, &cell.text, &c.text, Some(page), display, owner, owners, out, depth + 1);
                    }
                }
            }
        }
        for n in &ft.notes {
            if let Some(source) = n.source_story.as_deref().or_else(|| story.notes.get(n.index).map(|n| &n.text)) {
                let owner = n.annotation.and_then(|id| owners.get(&id)).copied().unwrap_or(owner);
                collect_anchors(doc, source, &n.text, Some(page), display, owner, owners, out, depth + 1);
            }
        }
    }
}

/// Page-local clickable regions with the same destination resolution as PDF output.
pub struct PublicationNavigation {
    pub pages: Vec<Vec<crate::NavigationLink>>,
    pub warnings: Vec<String>,
}
pub fn navigation_documents(documents: &[Document], cache: &Cache) -> Result<PublicationNavigation> {
    let pages = PublicationSheets::new(documents, cache, &PdfOptions::default())?;
    let mut result = PublicationNavigation { pages: Vec::new(), warnings: Vec::new() };
    for (part, doc) in documents.iter().enumerate() {
        for index in 0..pages.sheets[part].len() {
            result.pages.push(crate::links::annotations_mapped(
                doc,
                cache,
                &pages.sheets[part],
                index,
                &|owner, p| pages.part(owner, part).and_then(|part| pages.page(part, p)),
                &|owner, id| pages.part(owner, part).and_then(|part| pages.anchor(part, id)),
                &|id| pages.entity(part, id),
                &mut result.warnings,
            ));
        }
    }
    Ok(result)
}
