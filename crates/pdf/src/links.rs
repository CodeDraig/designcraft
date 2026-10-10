//! Interactive PDF: hyperlink annotations (frames and text ranges → URLs, e-mail, pages) and the
//! bookmark outline.

use designcraft_doc::{Bookmark, Document, HyperlinkDest, HyperlinkSource, SpreadRef};
use designcraft_geom::{Affine, Rect};
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::destination::XyzDestination;
use krilla::outline::OutlineNode;

use crate::export::Sheet;

/// Index of the output sheet showing absolute page `abs`.
pub(crate) fn sheet_of_page(doc: &Document, sheets: &[Sheet], abs: usize) -> Option<usize> {
    let (si, pi) = doc.page_loc(abs)?;
    let page = doc.spreads.get(si)?.pages.get(pi)?.bounds();
    sheets.iter().position(|s| s.spread == si && s.trim.x0 <= page.center().x && page.center().x <= s.trim.x1)
}

/// A resolved link shared by PDF and fixed-layout EPUB delivery.
#[derive(Clone, Debug)]
pub enum NavigationTarget {
    Page(usize),
    Url(String),
}
#[derive(Clone, Debug)]
pub struct NavigationLink {
    pub rect: Rect,
    pub target: NavigationTarget,
    pub label: String,
}
impl NavigationLink {
    pub(crate) fn annotation(&self) -> Option<Annotation> {
        let r = self.rect;
        let rect = krilla::geom::Rect::from_ltrb(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32)?;
        let target = match &self.target {
            NavigationTarget::Page(i) => Target::Destination(XyzDestination::new(*i, krilla::geom::Point::from_xy(0., 0.)).into()),
            NavigationTarget::Url(u) => Target::Action(Action::Link(LinkAction::new(u.clone()))),
        };
        Some(Annotation::new_link(LinkAnnotation::new(rect, target), Some(self.label.clone())))
    }
}

struct LiteralLink {
    component: Option<designcraft_doc::ComponentId>,
    destination: HyperlinkDest,
    name: String,
    rect: Rect,
}

fn range_rects(ft: &designcraft_compose::FrameText, start: usize, end: usize, xf: Affine) -> Vec<Rect> {
    let mut out = Vec::new();
    for line in &ft.lines {
        let glyphs: Vec<_> = line.glyphs.iter().filter(|g| g.visible && g.byte >= start && g.byte < end).collect();
        if !glyphs.is_empty() {
            let x0 = glyphs.iter().map(|g| g.x.min(g.x + g.adv)).fold(f64::INFINITY, f64::min);
            let x1 = glyphs.iter().map(|g| g.x.max(g.x + g.adv)).fold(f64::NEG_INFINITY, f64::max);
            out.push(xf.transform_rect_bbox(Rect::new(x0, line.baseline - line.ascent, x1, line.baseline + line.descent)));
        }
    }
    out
}

/// Walk the actual painted text. A gathered note's links belong to its source component even
/// when the note is displayed in a different chapter with overlapping local identifiers.
#[allow(clippy::too_many_arguments)]
fn literal_rects(
    doc: &Document,
    story: &designcraft_doc::Story,
    cs: &designcraft_compose::ComposedStory,
    ft: &designcraft_compose::FrameText,
    xf: Affine,
    out: &mut Vec<LiteralLink>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    let doc = cs.render_document.as_deref().unwrap_or(doc);
    let story = cs.source.as_deref().unwrap_or(story);
    let component = doc.publication.as_ref().map(|v| v.component);
    for link in &doc.hyperlinks {
        let range = match link.source {
            HyperlinkSource::Anchors { start, end } => {
                story.anchor_pos(start).zip(story.anchor_pos(end)).map(|(a, b)| (a + designcraft_doc::ANCHOR_MARK.len_utf8(), b))
            }
            HyperlinkSource::Text { story: id, start, end } if depth == 0 && id == story.id => Some((start, end)),
            _ => None,
        };
        if let Some((a, b)) = range {
            out.extend(range_rects(ft, a, b, xf).into_iter().map(|rect| LiteralLink {
                component,
                destination: link.dest.clone(),
                name: link.name.clone(),
                rect,
            }));
        }
    }
    for ((at, _), x) in story.text.match_indices(designcraft_doc::XREF_MARK).zip(&story.xrefs).filter(|(_, x)| x.field.is_none()) {
        out.extend(range_rects(ft, at, at + designcraft_doc::XREF_MARK.len_utf8(), xf).into_iter().map(|rect| LiteralLink {
            component,
            destination: HyperlinkDest::Anchor(x.target),
            name: format!("Cross-reference: {}", x.format),
            rect,
        }));
    }
    for table in &ft.tables {
        if let Some(source) = story.tables.get(&table.table) {
            for cell in &table.cells {
                if let Some(source) = source.cell(cell.row, cell.col) {
                    for nested in &cell.text.frames {
                        literal_rects(doc, &source.text, &cell.text, nested, xf * Affine::translate(cell.origin.to_vec2()), out, depth + 1);
                    }
                }
            }
        }
    }
    for note in &ft.notes {
        if let Some(source) = note.source_story.as_deref().or_else(|| story.notes.get(note.index).map(|n| &n.text)) {
            for nested in &note.text.frames {
                literal_rects(doc, source, &note.text, nested, xf * Affine::translate(note.origin.to_vec2()), out, depth + 1);
            }
        }
    }
}

fn warn(warnings: &mut Vec<String>, message: String) {
    if !warnings.contains(&message) {
        warnings.push(message);
    }
}

/// Link annotations for the sheet at `idx`.
/// Destination callbacks use the output page table, including other publication components.
#[allow(clippy::too_many_arguments)]
pub(crate) fn annotations_mapped(
    doc: &Document,
    cache: &designcraft_compose::Cache,
    sheets: &[Sheet],
    idx: usize,
    page_destination: &dyn Fn(Option<designcraft_doc::ComponentId>, usize) -> Option<usize>,
    anchor_destination: &dyn Fn(Option<designcraft_doc::ComponentId>, u64) -> Option<usize>,
    entity_destination: &dyn Fn(u64) -> Option<usize>,
    warnings: &mut Vec<String>,
) -> Vec<NavigationLink> {
    let sh = &sheets[idx];
    let mut out = Vec::new();
    let component = doc.publication.as_ref().map(|v| v.component);
    let mut literal = Vec::new();
    for st in doc.stories.values() {
        let cs = cache.get(doc, st.id, None);
        for ft in &cs.frames {
            let (Some(loc), Some(it)) = (doc.find(ft.frame), doc.item(ft.frame)) else { continue };
            if loc.spread == SpreadRef::Doc(sh.spread) {
                literal_rects(doc, st, &cs, ft, doc.parent_xf(&loc) * doc.text_xf(it), &mut literal, 0);
            }
        }
    }
    for h in &doc.hyperlinks {
        if let HyperlinkSource::Item { id } = h.source
            && let (Some(loc), Some(it)) = (doc.find(id), doc.item(id))
            && loc.spread == SpreadRef::Doc(sh.spread)
            && !it.hidden
            && !it.nonprinting
        {
            literal.push(LiteralLink {
                component,
                destination: h.dest.clone(),
                name: h.name.clone(),
                rect: doc.parent_xf(&loc).transform_rect_bbox(it.bounds()),
            });
        }
    }
    for link in literal {
        let r = link.rect.intersect(sh.bleed);
        if r.width() <= 0.0 || r.height() <= 0.0 {
            continue;
        }
        let rect = Rect::new(r.x0 - sh.media.x0, r.y0 - sh.media.y0, r.x1 - sh.media.x0, r.y1 - sh.media.y0);
        let target = match &link.destination {
            HyperlinkDest::Url(u) => NavigationTarget::Url(u.clone()),
            HyperlinkDest::Email(e) => NavigationTarget::Url(format!("mailto:{e}")),
            HyperlinkDest::Anchor(a) => match anchor_destination(link.component, *a) {
                Some(i) => NavigationTarget::Page(i),
                None => {
                    warn(warnings, format!("Link {}: anchor {a} is absent from the exported pages", link.name));
                    continue;
                }
            },
            HyperlinkDest::Page(p) => match page_destination(link.component, *p) {
                Some(i) => NavigationTarget::Page(i),
                None => {
                    warn(warnings, format!("Link {}: page {} is absent from the exported pages", link.name, p + 1));
                    continue;
                }
            },
        };
        out.push(NavigationLink { rect, target, label: link.name });
    }
    // Buttons: their bounds act on release.
    let here = doc.spreads.get(sh.spread).and_then(|sp| {
        let first = doc.spreads[..sh.spread].iter().map(|s| s.pages.len()).sum::<usize>();
        sp.pages.iter().position(|p| p.bounds().intersect(sh.trim).area() > 0.0).map(|i| first + i)
    });
    let mut stack: Vec<(Affine, &std::sync::Arc<designcraft_doc::Item>)> =
        doc.spreads.get(sh.spread).map(|sp| sp.items.iter().map(|it| (Affine::IDENTITY, it)).collect()).unwrap_or_default();
    while let Some((xf, it)) = stack.pop() {
        stack.extend(it.shown_children().map(|c| (xf * it.xf, c)));
        let Some(action) = &it.button else { continue };
        if it.hidden {
            continue;
        }
        let r = xf.transform_rect_bbox(it.bounds()).intersect(sh.bleed);
        if r.width() <= 0. || r.height() <= 0. {
            continue;
        }
        let rect = Rect::new(r.x0 - sh.media.x0, r.y0 - sh.media.y0, r.x1 - sh.media.x0, r.y1 - sh.media.y0);
        use designcraft_doc::ButtonAction as B;
        let total = doc.page_count();
        let page = match action {
            B::GoToUrl { url } => {
                out.push(NavigationLink { rect, target: NavigationTarget::Url(url.clone()), label: it.name.clone() });
                continue;
            }
            B::GoToPage { page } => Some(page.to_owned()),
            B::GoToFirstPage => Some(0),
            B::GoToLastPage => total.checked_sub(1),
            B::GoToNextPage => here.map(|h| h + 1).filter(|p| *p < total),
            B::GoToPreviousPage => here.and_then(|h| h.checked_sub(1)),
        };
        let Some(i) = page.and_then(|p| page_destination(component, p)) else { continue };
        let target = NavigationTarget::Page(i);
        out.push(NavigationLink { rect, target, label: it.name.clone() });
    }
    if doc.publication.is_some() {
        let mut rectangles = Vec::new();
        for sid in doc.stories.keys() {
            let cs = cache.get(doc, *sid, None);
            for ft in &cs.frames {
                let (Some(loc), Some(it)) = (doc.find(ft.frame), doc.item(ft.frame)) else { continue };
                if loc.spread == SpreadRef::Doc(sh.spread) {
                    generated_rects(ft, doc.parent_xf(&loc) * doc.text_xf(it), &mut rectangles, 0);
                }
            }
        }
        for (entity, r) in rectangles {
            let Some(i) = entity_destination(entity) else {
                warn(warnings, format!("Publication reference {entity}: destination is absent from the exported pages"));
                continue;
            };
            let r = r.intersect(sh.bleed);
            if r.width() <= 0.0 || r.height() <= 0.0 {
                continue;
            }
            let rect = Rect::new(r.x0 - sh.media.x0, r.y0 - sh.media.y0, r.x1 - sh.media.x0, r.y1 - sh.media.y0);
            out.push(NavigationLink { rect, target: NavigationTarget::Page(i), label: format!("Publication reference {entity}") });
        }
    }
    out
}

/// The document outline from the bookmarks (bookmarks to pages that weren't exported are skipped).
pub(crate) fn outline_nodes(
    doc: &Document,
    _cache: &designcraft_compose::Cache,
    _sheets: &[Sheet],
    page_destination: &dyn Fn(usize) -> Option<usize>,
    anchor_destination: &dyn Fn(u64) -> Option<usize>,
) -> Vec<OutlineNode> {
    fn node(b: &Bookmark, resolve: &dyn Fn(usize) -> Option<usize>) -> Option<OutlineNode> {
        let i = resolve(b.page)?;
        let mut n = OutlineNode::new(b.name.clone(), XyzDestination::new(i, krilla::geom::Point::from_xy(0.0, 0.0)));
        for c in &b.children {
            if let Some(cn) = node(c, resolve) {
                n.push_child(cn);
            }
        }
        Some(n)
    }
    let mut o = Vec::new();
    for b in &doc.bookmarks {
        if let Some(n) = node(b, page_destination) {
            o.push(n);
        }
    }
    for source in &doc.tei_sources {
        for entry in &source.navigation {
            if entry.kind == "sourcePage" {
                continue;
            }
            if let Some(page) = anchor_destination(entry.anchor) {
                o.push(OutlineNode::new(entry.label.clone(), XyzDestination::new(page, krilla::geom::Point::from_xy(0.0, 0.0))));
            }
        }
    }
    if let Some(v) = &doc.publication
        && let Ok(index) = designcraft_doc::publication::references::SemanticIndex::build(&v.root)
    {
        let imported: std::collections::BTreeSet<_> = doc.tei_sources.iter().flat_map(|s| &s.navigation).map(|n| n.anchor).collect();
        for entry in
            index.entries.into_iter().filter(|e| e.component == v.component && e.kind == designcraft_doc::publication::numbering::ObjectKind::Heading)
        {
            let Some(target) = v.root.semantic.get(&designcraft_doc::SemanticId(entry.id)) else { continue };
            if imported.contains(&target.target.anchor) {
                continue;
            }
            if let Some(page) = anchor_destination(target.target.anchor) {
                o.push(OutlineNode::new(entry.title, XyzDestination::new(page, krilla::geom::Point::from_xy(0., 0.))));
            }
        }
    }
    o
}

/// Generated runs carry their own destination through wrapping, bidi and rich styling. Nested
/// cells and notes use the same geometry rather than treating a whole joined field as one link.
fn generated_rects(ft: &designcraft_compose::FrameText, xf: Affine, out: &mut Vec<(u64, Rect)>, depth: usize) {
    if depth > 32 {
        return;
    }
    for line in &ft.lines {
        let mut run: Option<(u64, Rect)> = None;
        for g in &line.glyphs {
            let target = g.generated.as_ref().and_then(|v| v.target);
            if let Some(id) = target {
                let r = Rect::new(g.x.min(g.x + g.adv), line.baseline - line.ascent, g.x.max(g.x + g.adv), line.baseline + line.descent);
                match run.as_mut() {
                    Some((old, bounds)) if *old == id => *bounds = bounds.union(r),
                    _ => {
                        if let Some((id, r)) = run.take() {
                            out.push((id, xf.transform_rect_bbox(r)));
                        }
                        run = Some((id, r));
                    }
                }
            } else if let Some((id, r)) = run.take() {
                out.push((id, xf.transform_rect_bbox(r)));
            }
        }
        if let Some((id, r)) = run {
            out.push((id, xf.transform_rect_bbox(r)));
        }
    }
    for table in &ft.tables {
        for cell in &table.cells {
            for nested in &cell.text.frames {
                generated_rects(nested, xf * Affine::translate(cell.origin.to_vec2()), out, depth + 1);
            }
        }
    }
    for note in &ft.notes {
        for nested in &note.text.frames {
            generated_rects(nested, xf * Affine::translate(note.origin.to_vec2()), out, depth + 1);
        }
    }
}
