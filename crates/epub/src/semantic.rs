//! Semantic book content, independent of how its frames happen to paginate.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::sync::Arc;

use designcraft_doc::{
    Asset, CharFormat, Content, Document, Footnote, HyperlinkDest, HyperlinkSource, Item, Story,
    publication::{PublicationView, annotations::Placement, references::FieldUse},
};

use super::{EpubError, esc};

pub(super) struct Body {
    pub text: String,
    pub css: String,
    pub assets: BTreeMap<String, Arc<Asset>>,
    pub toc: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

pub(super) fn documents(doc: &Document) -> Result<Vec<Document>, EpubError> {
    if let Some(v) = &doc.publication {
        v.root
            .ordered_components()
            .map_err(|e| EpubError(e.to_string()))?
            .into_iter()
            .map(|component| PublicationView { component, ..v.clone() }.project().map_err(|e| EpubError(e.to_string())))
            .collect()
    } else {
        Ok(vec![doc.clone()])
    }
}

struct PendingNote {
    part: usize,
    id: String,
    callout: String,
    label: String,
    stream: String,
    end: bool,
    note: Arc<Footnote>,
}

struct Writer<'a> {
    docs: &'a [Document],
    embedded: bool,
    result: Body,
    notes: VecDeque<PendingNote>,
    emitted: BTreeSet<String>,
    entities: BTreeMap<u64, String>,
    requested: BTreeSet<String>,
}

fn component(doc: &Document, part: usize) -> u64 {
    doc.publication.as_ref().map_or(part as u64, |v| v.component.0)
}
fn chapter(doc: &Document, part: usize) -> String {
    format!("component-{}", component(doc, part))
}
fn anchor(part: usize, id: u64) -> String {
    format!("a-{part}-{id}")
}
fn error(message: impl Into<String>) -> EpubError {
    EpubError(message.into())
}

pub(super) fn build(docs: &[Document], embedded: bool) -> Result<Body, EpubError> {
    if docs.is_empty() || docs.len() > 10_000 {
        return Err(error("export needs 1–10000 components"));
    }
    let mut w = Writer {
        docs,
        embedded,
        result: Body {
            text: String::new(),
            css: String::from(
                "table{border-collapse:collapse;max-width:100%}td,th{border:1px solid;padding:.3em} img{max-width:100%;height:auto}.noteref{vertical-align:super;font-size:.75em}.notes{border-top:1px solid;margin-top:2em}.note{margin:1em 0}.unresolved{color:#a00}.pagebreak{display:inline-block;width:0;height:0;overflow:hidden}p,h1,h2,h3,h4,h5,h6,blockquote{white-space:pre-wrap}\n",
            ),
            assets: BTreeMap::new(),
            toc: Vec::new(),
            warnings: Vec::new(),
        },
        notes: VecDeque::new(),
        emitted: BTreeSet::new(),
        entities: BTreeMap::new(),
        requested: BTreeSet::new(),
    };
    for (part, doc) in docs.iter().enumerate() {
        doc.check().map_err(|e| error(e.to_string()))?;
        if let Some(v) = &doc.publication {
            w.entities.insert(v.component.0, chapter(doc, part));
            for entity in v.root.semantic.values().filter(|e| e.target.component == v.component) {
                w.entities.insert(entity.id.0, anchor(part, entity.target.anchor));
            }
            for index in v.root.indexes.values() {
                for (id, target) in index.targets().filter(|(_, t)| t.component == v.component) {
                    w.entities.insert(id, anchor(part, target.anchor));
                }
                if index.destination.as_ref().is_some_and(|d| d.component == v.component && doc.publication_story_included(d.story)) {
                    for id in index.topics.keys() {
                        w.entities.insert(*id, format!("topic-{id}"));
                    }
                }
            }
            for entry in v.root.annotation_entries().map_err(|e| error(e.to_string()))?.into_iter().filter(|e| e.component == v.component) {
                if v.root.note_policy(v.edition, &entry.identity.stream).placement != Placement::Omit {
                    w.entities.insert(entry.identity.id, format!("note-{}", entry.identity.id));
                    w.entities.insert(entry.identity.callout, format!("callout-{}", entry.identity.callout));
                }
            }
        }
    }
    let mut body = String::new();
    for (part, doc) in docs.iter().enumerate() {
        let id = chapter(doc, part);
        w.emitted.insert(id.clone());
        w.result.toc.push((id.clone(), doc.title.clone()));
        let _ = writeln!(body, "<section id=\"{id}\" class=\"{id}\" aria-label=\"{}\">", esc(&doc.title));
        // Scope style names to their owning chapter: identical names need not mean identical typography.
        for line in super::stylesheet(doc).lines() {
            if let Some((selectors, rule)) = line.split_once('{') {
                let selectors = selectors
                    .split(',')
                    .map(|s| if s.trim() == "body" { format!(".{id}") } else { format!(".{id} {}", s.trim()) })
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = writeln!(w.result.css, "{selectors} {{{rule}");
            }
        }
        let order = if let Some(v) = &doc.publication {
            let c = v.root.components.get(&v.component).ok_or_else(|| error("missing publication component"))?;
            // Keep canonical story order; place free illustrations before their next story
            // in the document's article/spatial reading order rather than at the book's end.
            let mut pending = Vec::new();
            let mut before = BTreeMap::new();
            for block in super::reading_order(doc) {
                match block {
                    super::Block::Story(id) => {
                        before.insert(id, std::mem::take(&mut pending));
                    }
                    image => pending.push(image),
                }
            }
            let mut blocks = Vec::new();
            for sid in &c.story_order {
                blocks.extend(before.remove(sid).unwrap_or_default());
                blocks.push(super::Block::Story(*sid));
            }
            blocks.extend(before.into_values().flatten());
            blocks.extend(pending);
            blocks
        } else {
            let mut blocks = super::reading_order(doc);
            if !doc.articles.iter().any(|a| a.export) {
                let present: BTreeSet<_> = blocks
                    .iter()
                    .filter_map(|b| match b {
                        super::Block::Story(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                blocks.extend(doc.stories.values().filter(|s| !present.contains(&s.id) && s.frames.is_empty()).map(|s| super::Block::Story(s.id)));
            }
            blocks
        };
        let mut seen = BTreeSet::new();
        for block in order {
            match block {
                super::Block::Story(sid) if seen.insert(sid) && doc.publication_story_included(sid) => {
                    if let Some(v) = &doc.publication {
                        // Notes are emitted from their canonical callouts, not from layout-only carriers.
                        if v.root.note_output(v.component, sid).is_some() {
                            continue;
                        }
                        if doc.story(sid).is_some_and(|s| {
                            s.text.chars().all(|c| c.is_whitespace() || c == designcraft_doc::XREF_MARK)
                                && !s.xrefs.is_empty()
                                && s.xrefs.iter().all(|x| {
                                    x.field.and_then(|id| v.root.references.fields.get(&id)).is_some_and(|f| f.use_ == FieldUse::RunningHead)
                                })
                        }) {
                            continue;
                        }
                    } else if doc.endnote_story == Some(sid) {
                        continue;
                    }
                    if let Some(st) = doc.story(sid) {
                        if st.frames.is_empty() && !st.text.is_empty() {
                            w.warn(format!("Chapter {}: unplaced story {} included in semantic reading order", doc.title, sid.0));
                        }
                        let key = format!("story-{part}-{}", sid.0);
                        w.emitted.insert(key.clone());
                        let _ = writeln!(body, "<section id=\"{key}\">{}</section>", w.story(part, st, &key, 0)?);
                    }
                }
                super::Block::Image(id) => {
                    if let Some(it) = doc.item(id) {
                        body.push_str(&w.item(part, it, false)?);
                    }
                }
                _ => {}
            }
        }
        body.push_str("</section>\n");
    }
    if !w.notes.is_empty() {
        body.push_str("<section class=\"notes\" epub:type=\"endnotes\" role=\"doc-endnotes\"><h1>Notes</h1>\n");
    }
    let mut count = 0usize;
    while let Some(n) = w.notes.pop_front() {
        count += 1;
        if count > 100_000 {
            return Err(error("semantic export exceeds 100000 notes"));
        }
        if !w.emitted.insert(n.id.clone()) {
            continue;
        }
        let owner = chapter(&docs[n.part], n.part);
        let kind = if n.end { "endnote" } else { "footnote" };
        let role = if n.end { "doc-endnote" } else { "doc-footnote" };
        let _ = writeln!(
            body,
            "<aside id=\"{}\" class=\"note {owner}\" epub:type=\"{kind}\" role=\"{role}\" data-stream=\"{}\"><a href=\"#{}\" role=\"doc-backlink\">{}</a>{}</aside>",
            n.id,
            esc(&n.stream),
            n.callout,
            esc(&n.label),
            w.story(n.part, &n.note.text, &n.id, 1)?
        );
    }
    if count > 0 {
        body.push_str("</section>\n");
    }
    for destination in w.requested.clone() {
        if !w.emitted.contains(&destination) {
            w.warn(format!("Destination {destination} is unavailable in this edition"));
            let _ = writeln!(body, "<aside id=\"{}\" class=\"unresolved\">Unresolved destination: {}</aside>", esc(&destination), esc(&destination));
        }
    }
    if body.len() > 256 * 1024 * 1024 {
        return Err(error("semantic book exceeds 256 MiB"));
    }
    w.result.text = body;
    w.result.warnings.sort();
    w.result.warnings.dedup();
    Ok(w.result)
}

impl Writer<'_> {
    fn warn(&mut self, s: String) {
        self.result.warnings.push(s);
    }
    fn link(&mut self, target: String, text: &str) -> String {
        self.requested.insert(target.clone());
        format!("<a href=\"#{}\">{text}</a>", esc(&target))
    }
    fn destination(&mut self, part: usize, dest: &HyperlinkDest) -> String {
        match dest {
            HyperlinkDest::Url(s) => {
                if super::executable_link(s) {
                    self.warn("Inactive external link with unsupported executable scheme".into());
                    String::new()
                } else {
                    esc(s)
                }
            }
            HyperlinkDest::Email(s) => format!("mailto:{}", esc(s)),
            HyperlinkDest::Anchor(id) => {
                let a = anchor(part, *id);
                self.requested.insert(a.clone());
                format!("#{a}")
            }
            HyperlinkDest::Page(page) => {
                self.warn(format!(
                    "Chapter {}: fixed page {} link points to the chapter start in reflowable output",
                    self.docs[part].title,
                    page + 1
                ));
                format!("#{}", chapter(&self.docs[part], part))
            }
        }
    }
    fn story(&mut self, part: usize, original: &Story, path: &str, depth: usize) -> Result<String, EpubError> {
        if depth > 32 {
            return Err(error("semantic content nesting exceeds 32 levels"));
        }
        let doc = &self.docs[part];
        let mut st = original.clone();
        // Work on a derived copy so edition witness selection never overwrites authored text.
        if let Some(witness) = doc.publication.as_ref().and_then(|v| v.root.editions.get(&v.edition)).and_then(|e| e.apparatus_witness.as_deref()) {
            let mut choices = Vec::new();
            for n in original.notes.iter().chain(&original.endnotes) {
                if let Some(a) = &n.apparatus {
                    if a.edited(original) {
                        self.warn(format!("Apparatus {}: edited passage retained", a.source_key));
                        continue;
                    }
                    if let Some((range, reading)) = a.range(original).zip(a.choose(witness).and_then(|i| a.readings.get(i))) {
                        choices.push((range, reading.text.clone()));
                    }
                }
            }
            choices.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
            for (range, reading) in choices {
                let at = range.start;
                st.replace(range, "");
                st.insert_story_preserving_anchors(at, &reading);
            }
        }
        let pc = super::para_classes(&doc.styles);
        let mut heading_anchors: BTreeMap<u64, usize> =
            doc.tei_sources.iter().flat_map(|s| &s.navigation).filter(|n| n.kind == "heading").map(|n| (n.anchor, n.level.clamp(1, 6))).collect();
        if let Some(v) = &doc.publication {
            heading_anchors.extend(
                v.root
                    .semantic
                    .values()
                    .filter(|e| e.target.component == v.component && e.kind == designcraft_doc::publication::numbering::ObjectKind::Heading)
                    .map(|e| (e.target.anchor, e.level.clamp(1, 6) as usize)),
            );
        }
        let headings: BTreeMap<usize, usize> = st
            .text
            .match_indices(designcraft_doc::ANCHOR_MARK)
            .zip(&st.anchors)
            .filter_map(|((at, _), a)| heading_anchors.get(&a.id).map(|level| (at, *level)))
            .collect();
        let mut out = String::new();
        let ranges = st.para_ranges();
        let mut list_counter = 0u32;
        let mut named_lists = BTreeMap::new();
        for (pi, range) in ranges.into_iter().enumerate() {
            let Some(p) = st.paras.get(pi) else { return Err(error("missing paragraph format")) };
            if let Some(tid) = p.table {
                // Destination markers may precede a table's dedicated paragraph marker.
                out.push_str(&self.inline(part, &st, range.clone(), path, depth)?);
                if let Some(t) = st.tables.get(&tid) {
                    out.push_str(&self.table(part, t, path, depth + 1)?);
                } else {
                    out.push_str("<p class=\"unresolved\">[Missing table]</p>");
                    self.warn(format!("Missing table {tid}"));
                }
                continue;
            }
            let export = doc.styles.export_tag(&p.style, false);
            let tag = export
                .map(|t| t.tag.as_str())
                .filter(|s| matches!(*s, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "figcaption" | "aside" | "div"))
                .unwrap_or("p");
            let class = export
                .filter(|t| !t.class.is_empty())
                .map(|t| t.class.clone())
                .or_else(|| pc.get(p.style.as_str()).cloned())
                .unwrap_or_else(|| super::slug(&p.style));
            let heading = headings.range(range.clone()).next().map(|(_, level)| *level);
            let mut text = self.inline(part, &st, range, path, depth)?;
            let (pp, cp) = doc.styles.resolve_para(p);
            let label = match pp.list_type {
                designcraft_doc::ListType::Bullets => Some(format!("{}{}", pp.bullet_char, pp.list_separator)),
                designcraft_doc::ListType::Numbers => {
                    let counter = if pp.list_name.is_empty() {
                        &mut list_counter
                    } else {
                        named_lists.entry(pp.list_name.clone()).or_insert_with(|| doc.list_start(st.id, &pp.list_name))
                    };
                    *counter = pp.start_at.unwrap_or(counter.checked_add(1).ok_or_else(|| error("list numbering exhausted"))?).max(1);
                    if *counter > 100_000 {
                        return Err(error("list numbering exceeds 100000"));
                    }
                    Some(format!("{}.{}", pp.number_style.format(*counter), pp.list_separator))
                }
                designcraft_doc::ListType::None => {
                    list_counter = 0;
                    None
                }
            };
            if let Some(label) = label {
                text = format!("<span class=\"list-label\">{}</span>{text}", esc(&label));
            }
            let rtl = pp.direction == designcraft_doc::TextDirection::RightToLeft;
            let css = super::css::paragraph_overrides(doc, p);
            let language = (cp.language != designcraft_doc::CharProps::default().language).then(|| language(&cp.language)).flatten();
            let mut attrs = format!(
                "{}{}{}",
                if rtl { " dir=\"rtl\"" } else { "" },
                if css.is_empty() { String::new() } else { format!(" style=\"{}\"", esc(&css)) },
                language.map(|l| format!(" lang=\"{}\"", esc(l))).unwrap_or_default()
            );
            if let Some(level) = heading
                && !matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
            {
                let _ = write!(attrs, " role=\"heading\" aria-level=\"{level}\"");
            }
            let _ = writeln!(out, "<{tag} class=\"{}\"{}>{}</{tag}>", esc(&class), attrs, if text.is_empty() { "&#160;" } else { &text });
        }
        Ok(out)
    }
    fn inline(&mut self, part: usize, st: &Story, range: std::ops::Range<usize>, path: &str, depth: usize) -> Result<String, EpubError> {
        let doc = &self.docs[part];
        let mut out = String::new();
        let mut plain = String::new();
        let mut current: Option<(CharFormat, Option<String>)> = None;
        let mut anchors = st.text[..range.start].matches(designcraft_doc::ANCHOR_MARK).count();
        let mut notes = st.text[..range.start].matches(designcraft_doc::FOOTNOTE_REF).count();
        let mut ends = st.text[..range.start].matches(designcraft_doc::ENDNOTE_REF).count();
        let mut xrefs = st.text[..range.start].matches(designcraft_doc::XREF_MARK).count();
        let mut objects = st.text[..range.start].matches(designcraft_doc::OBJECT_MARK).count();
        let positions: BTreeMap<_, _> = st.text.match_indices(designcraft_doc::ANCHOR_MARK).zip(&st.anchors).map(|((at, _), a)| (a.id, at)).collect();
        let mut events = Vec::new();
        for (i, h) in doc.hyperlinks.iter().enumerate() {
            let span = match h.source {
                HyperlinkSource::Text { story, start, end } if story == st.id => Some((start, end)),
                HyperlinkSource::Anchors { start, end } => {
                    positions.get(&start).zip(positions.get(&end)).map(|(a, b)| (a + designcraft_doc::ANCHOR_MARK.len_utf8(), *b))
                }
                _ => None,
            };
            if let Some((a, b)) = span.filter(|(a, b)| a < b && *a < range.end && *b > range.start) {
                let href = self.destination(part, &h.dest);
                events.push((a.max(range.start), i, Some(href)));
                events.push((b, i, None));
            }
        }
        events.sort_by_key(|(at, id, _)| (*at, *id));
        let mut events = events.into_iter().peekable();
        let mut active = BTreeMap::new();
        let mut runs = st.runs().filter(|(r, _)| r.end > range.start).peekable();
        for (offset, c) in st.text[range.clone()].char_indices() {
            let at = range.start + offset;
            while runs.peek().is_some_and(|(r, _)| r.end <= at) {
                runs.next();
            }
            let f = runs.peek().map_or_else(|| st.char_format_at(at).clone(), |(_, f)| (*f).clone());
            while events.peek().is_some_and(|(pos, _, _)| *pos <= at) {
                if let Some((_, id, href)) = events.next() {
                    if let Some(href) = href.filter(|href| !href.is_empty()) {
                        active.insert(id, href);
                    } else {
                        active.remove(&id);
                    }
                }
            }
            if f.over.change == Some(designcraft_doc::ChangeMark::Deleted) || f.over.conditions.as_deref().is_some_and(|c| doc.conditions_hide(c)) {
                match c {
                    designcraft_doc::ANCHOR_MARK => anchors += 1,
                    designcraft_doc::FOOTNOTE_REF => notes += 1,
                    designcraft_doc::ENDNOTE_REF => ends += 1,
                    designcraft_doc::XREF_MARK => xrefs += 1,
                    designcraft_doc::OBJECT_MARK => objects += 1,
                    _ => {}
                }
                continue;
            }
            let href = active.first_key_value().map(|(_, href)| href.clone());
            let key = (f.clone(), href);
            if current.as_ref() != Some(&key) || is_control(c) {
                if let Some((format, link)) = current.take() {
                    out.push_str(&self.formatted(part, &plain, &format, link.as_deref()));
                    plain.clear();
                }
                current = Some(key);
            }
            match c {
                designcraft_doc::ANCHOR_MARK => {
                    if let Some(a) = st.anchors.get(anchors) {
                        let id = anchor(part, a.id);
                        self.emitted.insert(id.clone());
                        if let Some(heading) = doc.tei_sources.iter().flat_map(|s| &s.navigation).find(|n| n.anchor == a.id && n.kind != "sourcePage")
                        {
                            if !self.result.toc.iter().any(|(key, _)| key == &id) {
                                self.result.toc.push((id.clone(), heading.label.clone()));
                            }
                        } else if let Some(v) = &doc.publication
                            && let Some(entity) = v.root.semantic.values().find(|e| {
                                e.target.component == v.component
                                    && e.target.anchor == a.id
                                    && e.kind == designcraft_doc::publication::numbering::ObjectKind::Heading
                            })
                        {
                            let title = entity.title.clone().unwrap_or_else(|| {
                                st.para_ranges()
                                    .get(st.para_at(at))
                                    .and_then(|r| st.text.get(r.clone()))
                                    .map(designcraft_doc::xref::clean)
                                    .unwrap_or_default()
                            });
                            if !self.result.toc.iter().any(|(key, _)| key == &id) {
                                self.result.toc.push((id.clone(), title));
                            }
                        }
                        let source = doc.tei_sources.iter().flat_map(|s| &s.navigation).find(|n| n.anchor == a.id && n.kind == "sourcePage");
                        let _ = write!(
                            out,
                            "<span id=\"{id}\"{}></span>",
                            source
                                .map(|s| format!(" class=\"pagebreak\" epub:type=\"pagebreak\" role=\"doc-pagebreak\" title=\"{}\"", esc(&s.label)))
                                .unwrap_or_default()
                        );
                    }
                    anchors += 1;
                }
                designcraft_doc::FOOTNOTE_REF | designcraft_doc::ENDNOTE_REF => {
                    let end = c == designcraft_doc::ENDNOTE_REF;
                    let index = if end { ends } else { notes };
                    if end {
                        ends += 1;
                    } else {
                        notes += 1;
                    }
                    if let Some(n) = if end { st.endnotes.get(index) } else { st.notes.get(index) } {
                        out.push_str(&self.note(part, n, path, end, index)?);
                    }
                }
                designcraft_doc::XREF_MARK => {
                    if let Some(x) = st.xrefs.get(xrefs) {
                        if let Some(id) = x.field {
                            if let Some(v) = &doc.publication {
                                for index in v.root.indexes.values() {
                                    for (topic, field) in &index.fields {
                                        if *field == id {
                                            let key = format!("topic-{topic}");
                                            if self.emitted.insert(key.clone()) {
                                                let _ = write!(out, "<span id=\"{key}\"></span>");
                                            }
                                        }
                                    }
                                }
                            }
                            let value = doc.publication_field(id, None);
                            self.result.warnings.extend(value.unresolved);
                            for run in value.runs {
                                let text = self.formatted(part, &run.text, &run.format, None);
                                if let Some(target) = run.target {
                                    let key = self.entities.get(&target).cloned().unwrap_or_else(|| format!("unresolved-{target}"));
                                    out.push_str(&self.link(key, &text));
                                } else {
                                    out.push_str(&text);
                                }
                            }
                        } else if let Some(values) = doc.xref_values(x.target) {
                            let definition = doc.xref_format(&x.format).map_or("<fullPara />", |f| f.definition.as_str());
                            let text = esc(&designcraft_doc::xref::expand(definition, &values));
                            out.push_str(&self.link(anchor(part, x.target), &text));
                        } else {
                            out.push_str("<span class=\"unresolved\">[Unresolved reference]</span>");
                            self.warn(format!("Unresolved anchor {}", x.target));
                        }
                    }
                    xrefs += 1;
                }
                designcraft_doc::OBJECT_MARK => {
                    if let Some(o) = st.objects.get(objects) {
                        out.push_str(&self.item(part, &o.item, true)?);
                    }
                    objects += 1;
                }
                designcraft_doc::TABLE_ANCHOR
                | designcraft_doc::index::INDEX_MARK
                | designcraft_doc::story::INDENT_HERE
                | designcraft_doc::NOTE_MARK => {}
                designcraft_doc::story::FORCED_LINE_BREAK => out.push_str("<br/>"),
                designcraft_doc::story::COLUMN_BREAK | designcraft_doc::story::FRAME_BREAK | designcraft_doc::story::PAGE_BREAK => {
                    out.push_str("<br class=\"layout-break\"/>")
                }
                designcraft_doc::story::RIGHT_INDENT_TAB => out.push(' '),
                c if designcraft_doc::vars::var_index(c).is_some() => {
                    let value = designcraft_doc::vars::var_index(c).and_then(|i| doc.variable_value(i, None));
                    if let Some(v) = value {
                        out.push_str(&esc(&v));
                    } else {
                        out.push_str("[Unresolved variable]");
                        self.warn("Unresolved text variable".into());
                    }
                }
                c if ('\u{e000}'..='\u{e0ff}').contains(&c) => {
                    out.push_str("[Unsupported marker]");
                    self.warn(format!("Unsupported marker U+{:04X} in {path}", c as u32));
                }
                _ => plain.push(c),
            }
        }
        if let Some((format, link)) = current {
            out.push_str(&self.formatted(part, &plain, &format, link.as_deref()));
        }
        let _ = depth;
        Ok(out)
    }
    fn formatted(&self, part: usize, text: &str, f: &CharFormat, href: Option<&str>) -> String {
        if text.is_empty() {
            return String::new();
        }
        let doc = &self.docs[part];
        let et = doc.styles.export_tag(&f.style, true);
        let tag = et
            .map(|e| e.tag.as_str())
            .filter(|s| matches!(*s, "span" | "em" | "strong" | "i" | "b" | "code" | "sup" | "sub" | "small" | "mark" | "cite" | "q"))
            .unwrap_or("span");
        let class = if f.style != designcraft_doc::NO_CHAR_STYLE {
            et.filter(|e| !e.class.is_empty())
                .map(|e| e.class.clone())
                .or_else(|| super::char_classes(&doc.styles).get(f.style.as_str()).cloned())
                .unwrap_or_else(|| super::slug(&f.style))
        } else {
            String::new()
        };
        let css = super::override_css(doc, &f.over);
        let lang = f.over.language.as_deref().and_then(language);
        let mut value = esc(text).replace('\u{2028}', "<br/>");
        if !class.is_empty() || !css.is_empty() || lang.is_some() {
            value = format!(
                "<{tag}{}{}{}>{value}</{tag}>",
                if class.is_empty() { String::new() } else { format!(" class=\"{}\"", esc(&class)) },
                if css.is_empty() { String::new() } else { format!(" style=\"{}\"", esc(&css)) },
                lang.map(|l| format!(" lang=\"{}\"", esc(l))).unwrap_or_default()
            );
        }
        if let Some(href) = href {
            value = format!("<a href=\"{href}\">{value}</a>");
        }
        value
    }
    fn note(&mut self, part: usize, n: &Arc<Footnote>, path: &str, end: bool, index: usize) -> Result<String, EpubError> {
        let doc = &self.docs[part];
        let (id, callout, label, stream, placement) = if let (Some(identity), Some(v)) = (&n.annotation, &doc.publication) {
            (
                format!("note-{}", identity.id),
                format!("callout-{}", identity.callout),
                v.root.note_label(v.edition, identity.id).unwrap_or_else(|| "?".into()),
                identity.stream.clone(),
                v.root.note_policy(v.edition, &identity.stream).placement,
            )
        } else {
            (
                format!("note-{path}-{end}-{}", n.id),
                format!("callout-{path}-{end}-{}", n.id),
                (index + 1).to_string(),
                if end { "Endnotes" } else { "Footnotes" }.into(),
                if end { Placement::ComponentEnd } else { Placement::Footnote },
            )
        };
        if placement == Placement::Omit {
            return Ok(String::new());
        }
        let mut note = n.clone();
        let mut text_label = label.clone();
        if let (Some(identity), Some(v)) = (&n.annotation, &doc.publication) {
            text_label = v.root.note_text_label(v.edition, identity.id).unwrap_or_else(|| label.clone());
            if let Some(stream) = v.root.note_streams.get(&identity.stream)
                && !stream.preserve_paragraph_styles
                && let Some(p) = Arc::make_mut(&mut note).text.paras.first_mut()
            {
                p.style = stream.para_style.clone();
            }
        }
        self.emitted.insert(callout.clone());
        self.requested.insert(id.clone());
        self.notes.push_back(PendingNote {
            part,
            id: id.clone(),
            callout: callout.clone(),
            label: text_label,
            stream,
            end: matches!(placement, Placement::ComponentEnd | Placement::PublicationEnd),
            note,
        });
        Ok(format!("<a id=\"{callout}\" class=\"noteref\" href=\"#{id}\" epub:type=\"noteref\" role=\"doc-noteref\">{}</a>", esc(&label)))
    }
    fn table(&mut self, part: usize, t: &designcraft_doc::Table, path: &str, depth: usize) -> Result<String, EpubError> {
        let mut out = String::from("<table>");
        for (row, r) in t.rows.iter().enumerate() {
            out.push_str("<tr>");
            for col in 0..t.columns.len() {
                if t.owner(row, col) != (row, col) {
                    continue;
                }
                let Some(cell) = t.cell(row, col) else { return Err(error("missing table cell")) };
                let tag = if r.kind == designcraft_doc::RowKind::Header { "th" } else { "td" };
                let _ = write!(out, "<{tag} rowspan=\"{}\" colspan=\"{}\">", cell.row_span, cell.col_span);
                if let Some(g) = &cell.graphic {
                    out.push_str(&self.graphic(part, g, "", true, None)?);
                } else {
                    out.push_str(&self.story(part, &cell.text, &format!("{path}-t{}-{row}-{col}", t.id), depth)?);
                }
                let _ = write!(out, "</{tag}>");
            }
            out.push_str("</tr>");
        }
        out.push_str("</table>");
        Ok(out)
    }
    fn item(&mut self, part: usize, it: &Item, inline: bool) -> Result<String, EpubError> {
        if it.hidden || it.nonprinting {
            return Ok(String::new());
        }
        let mut value = match &it.content {
            Content::Graphic(g) => self.graphic(part, g, &it.alt_text, inline, it.media.as_ref())?,
            _ => {
                self.warn(format!("Object {} ({}) cannot be represented in reflowable output; a visible description was retained", it.id.0, it.name));
                format!("<span class=\"unresolved\">[{}]</span>", esc(if it.alt_text.is_empty() { &it.name } else { &it.alt_text }))
            }
        };
        if !inline && value.starts_with("<figure>") {
            let mut css = String::new();
            let options = &it.export_options;
            if matches!(options.align.as_str(), "left" | "center" | "right") {
                css.push_str(&format!("text-align:{};", options.align));
            }
            if options.page_break_before {
                css.push_str("page-break-before:always;break-before:page;");
            }
            if !css.is_empty() {
                value = value.replacen("<figure>", &format!("<figure style=\"{css}\">"), 1);
            }
        }
        if let Some(link) =
            self.docs[part].hyperlinks.iter().find(|h| matches!(h.source,HyperlinkSource::Item{id} if id==it.id)).map(|h| h.dest.clone())
        {
            let target = self.destination(part, &link);
            if !target.is_empty() {
                value = format!("<a href=\"{target}\">{value}</a>");
            }
        }
        Ok(value)
    }
    fn graphic(
        &mut self,
        part: usize,
        g: &designcraft_doc::Graphic,
        alt: &str,
        inline: bool,
        media: Option<&designcraft_doc::MediaOptions>,
    ) -> Result<String, EpubError> {
        let Some(asset) = self.docs[part].assets.get(&g.asset).filter(|a| !a.data.is_empty()).cloned() else {
            self.warn(format!("Missing image {} in chapter {}", g.asset.0, self.docs[part].title));
            return Ok("<span class=\"unresolved\">[Missing illustration]</span>".into());
        };
        let ext = match asset.mime.as_str() {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            "audio/mpeg" => "mp3",
            "audio/ogg" => "ogg",
            "audio/wav" => "wav",
            "video/mp4" => "mp4",
            "video/webm" => "webm",
            _ => {
                self.warn(format!("Illustration {} has unsupported MIME type {}; description retained", asset.name, asset.mime));
                return Ok(format!("<span class=\"unresolved\">[Illustration: {}]</span>", esc(if alt.is_empty() { &asset.name } else { alt })));
            }
        };
        let name = format!("images/{part}-{}.{}", g.asset.0, ext);
        let src = if self.embedded { format!("data:{};base64,{}", asset.mime, super::html::base64(&asset.data)) } else { name.clone() };
        self.result.assets.insert(name, asset.clone());
        let tag = if inline { "span" } else { "figure" };
        let alt = esc(if alt.is_empty() { &asset.name } else { alt });
        if let Some(kind) = designcraft_doc::media_kind(&asset.mime) {
            let mut attrs = String::new();
            if media.is_none_or(|m| m.controls) {
                attrs.push_str(" controls=\"controls\"");
            }
            if media.is_some_and(|m| m.play_on_page_load) {
                attrs.push_str(" autoplay=\"autoplay\"");
            }
            if media.is_some_and(|m| m.looping) {
                attrs.push_str(" loop=\"loop\"");
            }
            if let Some(poster) = media.and_then(|m| m.poster) {
                let asset = self.docs[part].assets.get(&poster).cloned();
                if let Some(asset) = asset.filter(|a| a.mime.starts_with("image/") && !a.data.is_empty()) {
                    let ext = match asset.mime.as_str() {
                        "image/jpeg" => Some("jpg"),
                        "image/png" => Some("png"),
                        "image/gif" => Some("gif"),
                        "image/webp" => Some("webp"),
                        "image/svg+xml" => Some("svg"),
                        _ => None,
                    };
                    if let Some(ext) = ext {
                        let name = format!("images/{part}-{}.{ext}", poster.0);
                        let src =
                            if self.embedded { format!("data:{};base64,{}", asset.mime, super::html::base64(&asset.data)) } else { name.clone() };
                        self.result.assets.insert(name, asset);
                        let _ = write!(attrs, " poster=\"{src}\"");
                    } else {
                        self.warn(format!("Media {}: unsupported poster image", g.asset.0));
                    }
                } else {
                    self.warn(format!("Media {}: missing poster image", g.asset.0));
                }
            }
            return Ok(format!("<{tag}><{kind} src=\"{src}\"{attrs}>{alt}</{kind}></{tag}>"));
        }
        Ok(format!("<{tag}><img src=\"{src}\" alt=\"{alt}\"/></{tag}>"))
    }
}

fn is_control(c: char) -> bool {
    ('\u{e000}'..='\u{e1ff}').contains(&c) || c == designcraft_doc::story::FORCED_LINE_BREAK
}

fn language(s: &str) -> Option<&str> {
    designcraft_doc::language_tag(s)
        .or_else(|| (!s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).then_some(s))
}
