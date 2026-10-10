use super::{
    Graphic, location, styles, words,
    xml::{Element, NS, Node, err},
};
use crate::ImportError;
use designcraft_doc::{
    self as doc, CharFormat, CharRun, Hyperlink, HyperlinkDest, HyperlinkSource, ParaFormat, Story, StoryId,
    tei::{TeiDiagnostic, TeiNavigation, TeiOptions},
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct State {
    chars: CharFormat,
    para: ParaFormat,
    preserve: bool,
    base: String,
    depth: usize,
    verse: bool,
    in_note: bool,
    in_table: bool,
    quote: bool,
    list: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            chars: CharFormat::default(),
            para: para("Body"),
            preserve: false,
            base: String::new(),
            depth: 0,
            verse: false,
            in_note: false,
            in_table: false,
            quote: false,
            list: None,
        }
    }
}
fn para(name: &str) -> ParaFormat {
    ParaFormat { style: format!("TEI {name}"), ..Default::default() }
}

/// Append-only construction avoids repeatedly normalizing an entire book for every XML token.
pub(super) struct Builder {
    pub story: Story,
    space: bool,
    suppress_space: bool,
    block: bool,
}
impl Builder {
    pub fn new(id: StoryId) -> Self {
        Self { story: Story::new(id), space: false, suppress_space: false, block: false }
    }
    fn raw(&mut self, text: &str, f: &CharFormat) {
        if text.is_empty() {
            return;
        }
        if self.story.text.is_empty() {
            self.story.chars.clear();
        }
        self.story.text.push_str(text);
        if let Some(last) = self.story.chars.last_mut().filter(|r| r.format == *f) {
            last.len += text.len();
        } else {
            self.story.chars.push(CharRun { len: text.len(), format: f.clone() });
        }
        for _ in text.matches('\n') {
            let format = self.story.paras.last().cloned().unwrap_or_default();
            self.story.paras.push(format);
        }
    }
    fn visible_on_line(&self) -> bool {
        self.story.text.rsplit('\n').next().unwrap_or("").chars().any(|c| !matches!(c, doc::ANCHOR_MARK))
    }
    fn start(&mut self, pf: &ParaFormat) {
        if self.visible_on_line() {
            self.raw("\n", &CharFormat::default());
        }
        if let Some(p) = self.story.paras.last_mut() {
            *p = pf.clone();
        }
        self.space = false;
        self.block = false;
    }
    fn end(&mut self) {
        self.block = true;
        self.space = false;
    }
    fn prepare(&mut self, state: &State) {
        if self.block {
            self.start(&state.para);
        }
        if self.story.text.is_empty()
            && let Some(p) = self.story.paras.first_mut()
        {
            *p = state.para.clone();
        }
    }
    fn text(&mut self, text: &str, state: &State) {
        if state.preserve {
            if text.is_empty() {
                return;
            }
            self.prepare(state);
            if self.space && self.visible_on_line() && !self.suppress_space {
                self.raw(" ", &state.chars);
            }
            self.space = false;
            let text = if self.suppress_space { text.trim_start_matches([' ', '\t', '\r', '\n']) } else { text };
            if !text.is_empty() {
                self.suppress_space = false;
            }
            self.raw(text, &state.chars);
            return;
        }
        let mut buf = String::new();
        for c in text.chars() {
            if matches!(c, ' ' | '\t' | '\r' | '\n') {
                if !self.suppress_space {
                    self.space = true;
                }
            } else {
                self.prepare(state);
                if self.space && (!buf.is_empty() || self.visible_on_line()) && !self.suppress_space {
                    buf.push(' ');
                }
                self.space = false;
                self.suppress_space = false;
                buf.push(c);
            }
        }
        self.raw(&buf, &state.chars);
    }
    fn literal(&mut self, text: &str, state: &State) {
        self.prepare(state);
        self.raw(text, &state.chars);
    }
    fn anchor(&mut self, id: u64, name: &str, state: &State) {
        self.prepare(state);
        self.raw(&doc::ANCHOR_MARK.to_string(), &state.chars);
        self.story.anchors.push(Arc::new(doc::TextAnchor { id, name: name.into() }));
    }
    pub fn finish(mut self) -> Story {
        if self.story.chars.is_empty() {
            self.story.chars.push(CharRun { len: 0, format: CharFormat::default() });
        }
        self.story
    }
}

#[derive(Clone)]
struct PendingLink {
    start: u64,
    end: u64,
    target: String,
    label: String,
    location: TeiDiagnostic,
}
#[derive(Clone)]
pub(super) struct Context {
    pub options: TeiOptions,
    pub diagnostics: Vec<TeiDiagnostic>,
    pub navigation: Vec<TeiNavigation>,
    pub graphics: Vec<Graphic>,
    pub links: Vec<Hyperlink>,
    pub witnesses: BTreeMap<String, String>,
    pub renditions: BTreeMap<String, (String, String)>,
    pub characters: BTreeMap<String, String>,
    pub texts: usize,
    ids: BTreeMap<String, u64>,
    pending: Vec<PendingLink>,
    serial: u64,
}
impl Context {
    pub fn new(options: TeiOptions) -> Self {
        Self {
            options,
            diagnostics: vec![],
            navigation: vec![],
            graphics: vec![],
            links: vec![],
            witnesses: BTreeMap::new(),
            renditions: BTreeMap::new(),
            characters: BTreeMap::new(),
            texts: 0,
            ids: BTreeMap::new(),
            pending: vec![],
            serial: 0,
        }
    }
    fn id(&mut self) -> u64 {
        self.serial += 1;
        self.serial
    }
    pub fn next_story(&mut self) -> StoryId {
        StoryId(self.id())
    }
    pub fn warn(&mut self, e: &Element, code: &str, message: &str) {
        self.diagnostics.push(location(e, code, message));
    }
    fn anchor(&mut self, e: &Element, out: &mut Builder, state: &State) -> u64 {
        let id = self.id();
        let name = if e.attr("xml:id").is_empty() { format!("{} at {}:{}", e.name, e.line, e.column) } else { e.attr("xml:id").into() };
        if !e.attr("xml:id").is_empty() {
            self.ids.insert(e.attr("xml:id").into(), id);
        }
        out.anchor(id, &name, state);
        id
    }
    fn children(&mut self, e: &Element, out: &mut Builder, state: &State) -> Result<(), ImportError> {
        for node in &e.children {
            match node {
                Node::Text(t) => out.text(t, state),
                Node::Element(c) => self.render(c, out, state)?,
            }
        }
        Ok(())
    }
    fn state(&mut self, e: &Element, inherited: &State) -> State {
        let mut s = inherited.clone();
        match e.attr("xml:space") {
            "preserve" => s.preserve = true,
            "default" => s.preserve = false,
            "" => {}
            _ => self.warn(e, "xml-space", "Unknown xml:space value; inherited whitespace behavior retained."),
        }
        if !e.attr("xml:lang").is_empty() {
            s.chars.over.language = Some(e.attr("xml:lang").into());
            let lang = e.attr("xml:lang").split('-').next().unwrap_or("");
            if matches!(lang, "ar" | "he" | "fa" | "ur" | "syr") {
                s.para.para.direction = Some(doc::TextDirection::RightToLeft);
            }
        }
        if !e.attr("xml:base").is_empty() {
            s.base = join_uri(&s.base, e.attr("xml:base"));
        }
        let mut declarations = Vec::new();
        for key in e.attr("rendition").split_whitespace() {
            match self.renditions.get(key.trim_start_matches('#')) {
                Some((scheme, text)) if matches!(scheme.as_str(), "css" | "") => declarations.extend(text.split(';').map(str::to_string)),
                _ => self.warn(e, "rendition", "Unresolved or unsupported rendition; native semantic style used."),
            }
        }
        if e.attr("rend").contains(':') {
            declarations.extend(e.attr("rend").split(';').map(str::to_string));
        } else {
            declarations.extend(e.attr("rend").split_whitespace().map(str::to_string));
        }
        for token in declarations {
            if e.namespace == NS
                && matches!(e.name.as_str(), "p" | "ab" | "head")
                && let Some(style) = styles::paragraph_rendition(&token, s.depth)
            {
                s.para.style = format!("TEI {style}");
            } else if !styles::apply(&token, &mut s.chars.over, &mut s.para.para) {
                self.warn(e, "rendition", &format!("Unrepresented rendition {token:?}; retained in source."));
            }
        }
        s
    }
    pub fn render(&mut self, e: &Element, out: &mut Builder, inherited: &State) -> Result<(), ImportError> {
        if e.is("teiHeader") {
            return Ok(());
        }
        if e.namespace != NS {
            let s = self.state(e, inherited);
            self.warn(e, "foreign-element", "Foreign vocabulary is not typeset semantically; readable content retained.");
            if e.children.is_empty() {
                out.text(&format!("[{}]", e.name), &s);
            } else {
                self.children(e, out, &s)?;
            }
            return Ok(());
        }
        let heading = e.is("head") || (e.is("ab") && e.attr("type") == "heading");
        let mut s = inherited.clone();
        let block_style = match e.name.as_str() {
            _ if heading => Some(styles::heading(s.depth)),
            "p" | "ab" => Some(if s.in_note {
                "Note"
            } else if s.quote {
                "Quote"
            } else {
                "Body"
            }),
            "docTitle" | "titlePart" => Some("Title"),
            "byline" | "docAuthor" | "docImprint" | "docDate" => Some("Subtitle"),
            "l" => Some("Verse"),
            "speaker" => Some("Speaker"),
            "stage" => Some("Stage"),
            "item" | "castItem" => Some("List"),
            "bibl" | "biblStruct" => Some("Bibliography"),
            "figDesc" => Some("Caption"),
            "argument" | "epigraph" | "salute" | "signed" | "dateline" | "trailer" => Some("Quote"),
            _ => None,
        };
        if let Some(style) = block_style {
            s.para.style = format!("TEI {style}");
        }
        s = self.state(e, &s);
        if block_style.is_some() {
            out.start(&s.para);
        }
        let nav = heading || matches!(e.name.as_str(), "pb" | "text");
        let anchor = if !e.attr("xml:id").is_empty() || nav { Some(self.anchor(e, out, &s)) } else { None };
        match e.name.as_str() {
            "TEI" | "teiCorpus" => self.children(e, out, &s)?,
            "text" => {
                self.texts += 1;
                if let Some(anchor) = anchor {
                    self.navigation.push(TeiNavigation {
                        label: if e.attr("n").is_empty() { format!("Text {}", self.texts) } else { e.attr("n").into() },
                        anchor,
                        kind: "book".into(),
                        level: 0,
                    });
                }
                self.children(e, out, &s)?;
                out.end();
            }
            "front" | "body" | "back" | "group" | "titlePage" | "div" | "div1" | "div2" | "div3" | "div4" | "div5" | "div6" | "div7" | "sp"
            | "castList" | "listBibl" | "opener" | "closer" => {
                if e.name.starts_with("div") {
                    s.depth += 1;
                }
                self.children(e, out, &s)?;
                out.end();
            }
            _ if heading => {
                let start = out.story.len();
                self.children(e, out, &s)?;
                if let Some(anchor) = anchor {
                    let label = words(&out.story.text[start..].replace(doc::ANCHOR_MARK, ""));
                    self.navigation.push(TeiNavigation { label, anchor, kind: "heading".into(), level: s.depth.max(1) });
                }
            }
            "lg" => {
                out.end();
                s.verse = true;
                self.children(e, out, &s)?;
                if let Some(p) = out.story.paras.last_mut() {
                    p.para.space_after = Some(8.0);
                }
                out.end();
            }
            "l" => {
                s.verse = true;
                self.children(e, out, &s)?;
            }
            "list" => {
                s.list = Some(e.attr("type").into());
                let mut number = 0;
                for child in &e.children {
                    if let Node::Element(item) = child {
                        if item.is("item") && matches!(s.list.as_deref(), Some("ordered" | "numbered")) && item.attr("n").is_empty() {
                            number += 1;
                            let mut numbered = item.clone();
                            numbered.attrs.insert("n".into(), format!("{number}."));
                            self.render(&numbered, out, &s)?;
                        } else {
                            self.render(item, out, &s)?;
                        }
                    }
                }
                out.end();
            }
            "item" => {
                if !e.attr("n").is_empty() {
                    out.text(&format!("{} ", e.attr("n")), &s);
                } else if !matches!(s.list.as_deref(), Some("gloss" | "glossary")) {
                    out.text("• ", &s);
                }
                self.children(e, out, &s)?;
            }
            "label" => {
                s.chars.over.font_style = Some("Bold".into());
                self.children(e, out, &s)?;
                out.text(" ", &s);
            }
            "lb" => {
                if e.attr("break") == "no" {
                    out.space = false;
                    out.suppress_space = true;
                } else if s.verse || s.preserve {
                    out.literal("\u{2028}", &s);
                    out.space = false;
                } else {
                    out.text(" ", &s);
                }
            }
            "pb" => {
                if let Some(anchor) = anchor {
                    self.navigation.push(TeiNavigation {
                        label: if e.attr("n").is_empty() { format!("Source page at line {}", e.line) } else { e.attr("n").into() },
                        anchor,
                        kind: "sourcePage".into(),
                        level: 0,
                    });
                }
                if e.attr("break") == "no" {
                    out.space = false;
                    out.suppress_space = true;
                }
            }
            "cb" | "milestone" => {
                if anchor.is_none() {
                    self.anchor(e, out, &s);
                }
            }
            "choice" => {
                let selected = ["corr", "reg", "expan", "orig", "sic", "abbr"].iter().find_map(|n| e.child(n)).or_else(|| e.elements().next());
                if let Some(c) = selected {
                    self.render(c, out, &s)?;
                } else {
                    self.children(e, out, &s)?;
                }
            }
            "app" => self.apparatus(e, out, &s)?,
            "note" => self.note(e, out, &s)?,
            "del" => self.warn(e, "reading-deletion", "Deleted text omitted from the reading edition and retained in the source."),
            "supplied" => {
                out.text("[", &s);
                self.children(e, out, &s)?;
                out.text("]", &s);
            }
            "unclear" => {
                self.children(e, out, &s)?;
                out.text(" [?]", &s);
            }
            "gap" => {
                out.text(&format!("[gap{}]", if e.attr("reason").is_empty() { String::new() } else { format!(": {}", e.attr("reason")) }), &s);
            }
            "space" => {
                out.text(" ", &s);
            }
            "ref" | "ptr" => self.reference(e, out, &s)?,
            "figure" => {
                out.end();
                for c in e.elements() {
                    if c.is("head") || c.is("figDesc") {
                        let mut caption = s.clone();
                        caption.para = para("Caption");
                        out.start(&caption.para);
                        self.children(c, out, &caption)?;
                        out.end();
                    } else {
                        self.render(c, out, &s)?;
                    }
                }
                out.end();
            }
            "graphic" | "binaryObject" | "media" => self.graphic(e, out, &s)?,
            "table" => self.table(e, out, &s)?,
            "g" => {
                let mapped = self.characters.get(e.attr("ref").trim_start_matches('#')).cloned();
                if let Some(mapped) = mapped {
                    let decoded: Option<String> = mapped
                        .split_whitespace()
                        .map(|v| v.strip_prefix("U+").and_then(|v| u32::from_str_radix(v, 16).ok()).and_then(char::from_u32))
                        .collect();
                    out.text(decoded.as_deref().unwrap_or(&mapped), &s);
                } else if !e.text().is_empty() {
                    self.children(e, out, &s)?;
                } else {
                    out.text("[unmapped character]", &s);
                    self.warn(e, "character", "No Unicode mapping found for character reference.");
                }
            }
            "hi" | "emph" | "foreign" | "mentioned" | "term" => {
                if e.attr("rend").is_empty() && e.attr("rendition").is_empty() {
                    s.chars.over.font_style = Some("Italic".into());
                }
                self.children(e, out, &s)?;
            }
            "role" => {
                s.chars.over.font_style = Some("Bold".into());
                self.children(e, out, &s)?;
            }
            "roleDesc" => {
                if out.visible_on_line() {
                    out.text(" — ", &s);
                }
                s.chars.over.font_style = Some("Italic".into());
                self.children(e, out, &s)?;
            }
            "quote" | "cit" | "q" => {
                s.quote = true;
                self.children(e, out, &s)?;
            }
            "formula" | "facsimile" | "surface" | "zone" | "standOff" | "fs" | "entry" => {
                self.warn(
                    e,
                    "specialized-content",
                    "Specialized TEI encoding is retained in the source; readable text is shown without specialized layout.",
                );
                if e.text().trim().is_empty() {
                    out.text(&format!("[{}]", e.name), &s);
                } else {
                    self.children(e, out, &s)?;
                }
            }
            "p" | "ab" | "docTitle" | "titlePart" | "byline" | "docAuthor" | "docImprint" | "docDate" | "speaker" | "stage" | "castItem"
            | "castGroup" | "bibl" | "biblStruct" | "figDesc" | "argument" | "epigraph" | "salute" | "signed" | "dateline" | "trailer" | "add"
            | "corr" | "reg" | "expan" | "orig" | "sic" | "abbr" | "seg" | "s" | "w" | "pc" | "name" | "persName" | "placeName" | "orgName"
            | "date" | "time" | "num" | "measure" | "title" | "author" | "editor" | "publisher" | "pubPlace" | "idno" | "analytic" | "monogr"
            | "imprint" | "biblScope" | "anchor" | "lem" | "rdg" | "rdgGrp" | "subst" => self.children(e, out, &s)?,
            _ => {
                self.warn(e, "unmapped-element", "Element has no dedicated mapping; readable child content retained once.");
                if e.children.is_empty() {
                    out.text(&format!("[{}]", e.name), &s);
                } else {
                    self.children(e, out, &s)?;
                }
            }
        }
        if block_style.is_some() {
            out.end();
        }
        if out.story.len() > super::MAX_BYTES {
            return Err(err("rendered text exceeds 64 MiB"));
        }
        Ok(())
    }

    fn note(&mut self, e: &Element, out: &mut Builder, s: &State) -> Result<(), ImportError> {
        if s.in_note || matches!(e.attr("place"), "inline" | "intext") {
            if s.in_note {
                self.warn(e, "nested-note", "Nested note retained inline in its containing note.");
            }
            out.text(" [", s);
            self.children(e, out, s)?;
            out.text("]", s);
            return Ok(());
        }
        let end =
            s.in_table || matches!(e.attr("place"), "end" | "margin" | "left" | "right" | "top" | "bottommargin" | "opposite" | "outer" | "inner");
        if s.in_table {
            self.warn(e, "table-note", "Table note converted to an endnote so it flows with the book.");
        }
        if end && !s.in_table && e.attr("place") != "end" {
            self.warn(e, "marginal-note", "Marginal note converted to an endnote.");
        }
        let mut ns = s.clone();
        ns.in_note = true;
        ns.para = para("Note");
        let mut note = Builder::new(self.next_story());
        self.children(e, &mut note, &ns)?;
        let note = Arc::new(doc::Footnote { id: self.id(), text: note.finish(), annotation: None, apparatus: None });
        if end {
            out.literal(&doc::ENDNOTE_REF.to_string(), s);
            out.story.endnotes.push(note);
        } else {
            out.literal(&doc::FOOTNOTE_REF.to_string(), s);
            out.story.notes.push(note);
        }
        Ok(())
    }
    fn apparatus(&mut self, e: &Element, out: &mut Builder, s: &State) -> Result<(), ImportError> {
        fn readings<'a>(e: &'a Element, inherited: &'a str, out: &mut Vec<(&'a Element, &'a str)>) {
            let witness = if e.attr("wit").is_empty() { inherited } else { e.attr("wit") };
            for c in e.elements() {
                if c.is("lem") || c.is("rdg") {
                    out.push((c, if c.attr("wit").is_empty() { witness } else { c.attr("wit") }));
                } else if c.is("rdgGrp") {
                    readings(c, witness, out);
                }
            }
        }
        let mut all = Vec::new();
        readings(e, "", &mut all);
        let selected = self
            .options
            .witness
            .as_deref()
            .and_then(|w| all.iter().copied().find(|(_, wit)| wit.split_whitespace().any(|v| v.trim_start_matches('#') == w.trim_start_matches('#'))))
            .or_else(|| all.iter().copied().find(|(r, _)| r.is("lem")))
            .or_else(|| all.first().copied());
        let Some((chosen, chosen_wit)) = selected else {
            self.warn(e, "apparatus", "Apparatus has no lemma or reading; child text retained.");
            return self.children(e, out, s);
        };
        let witness_matched = self
            .options
            .witness
            .as_deref()
            .is_some_and(|w| chosen_wit.split_whitespace().any(|v| v.trim_start_matches('#') == w.trim_start_matches('#')));
        if !witness_matched && !all.iter().any(|(r, _)| r.is("lem")) {
            self.warn(e, "apparatus-fallback", "No applicable witness or lemma supplied; first reading used in the reading edition.");
        }
        if let Some(w) = &self.options.witness
            && !all.iter().any(|(_, wit)| wit.split_whitespace().any(|v| v.trim_start_matches('#') == w.trim_start_matches('#')))
        {
            self.warn(e, "witness-fallback", "Selected witness has no reading here; lemma or first reading used.");
        }
        if out.space && out.visible_on_line() && !out.suppress_space {
            out.raw(" ", &s.chars);
            out.space = false;
        }
        let start = self.id();
        out.anchor(start, "Apparatus reading start", s);
        let reading_start = out.story.len();
        self.render(chosen, out, s)?;
        let imported_text = out.story.text.get(reading_start..).unwrap_or("").to_string();
        let end = self.id();
        out.anchor(end, "Apparatus reading end", s);
        let mut structured = Vec::new();
        let imported_reading = all.iter().position(|(r, _)| std::ptr::eq(*r, chosen)).unwrap_or(0);
        for (reading, witnesses) in &all {
            let mut isolated = Context::new(self.options.clone());
            isolated.serial = self.serial;
            isolated.witnesses = self.witnesses.clone();
            isolated.renditions = self.renditions.clone();
            isolated.characters = self.characters.clone();
            let mut b = Builder::new(isolated.next_story());
            let mut state = s.clone();
            state.in_note = true;
            isolated.render(reading, &mut b, &state)?;
            if !isolated.pending.is_empty() {
                self.warn(reading,"apparatus-links","Links in a retained alternate reading keep their labels and original XML; the edition substitution does not create link destinations for those alternate-only anchors.");
            }
            self.diagnostics.extend(isolated.diagnostics);
            self.serial = isolated.serial;
            self.graphics.extend(isolated.graphics);
            structured.push(doc::publication::annotations::Reading {
                lemma: reading.is("lem"),
                witnesses: witnesses.split_whitespace().map(|s| s.trim_start_matches('#').to_string()).collect(),
                witness_labels: witnesses
                    .split_whitespace()
                    .map(|s| self.witnesses.get(s.trim_start_matches('#')).cloned().unwrap_or_else(|| s.trim_start_matches('#').into()))
                    .collect(),
                text: b.finish(),
            });
        }
        let apparatus = doc::publication::annotations::Apparatus {
            source_key: if e.attr("xml:id").is_empty() { format!("{}:{}", e.line, e.column) } else { e.attr("xml:id").into() },
            source: 0,
            start,
            end,
            imported_reading,
            imported_text,
            readings: structured,
        };
        let mut note = Builder::new(self.next_story());
        let mut ns = s.clone();
        ns.in_note = true;
        ns.para = para("Apparatus");
        note.start(&ns.para);
        note.text(&format!("{} — ", words(&chosen.text())), &ns);
        let mut alternatives = 0;
        for (reading, witnesses) in all.into_iter().filter(|(r, _)| !std::ptr::eq(*r, chosen)) {
            if alternatives > 0 {
                note.text("; ", &ns);
            }
            let wit = witnesses
                .split_whitespace()
                .map(|id| {
                    let id = id.trim_start_matches('#');
                    self.witnesses.get(id).filter(|s| !s.is_empty()).map_or_else(|| id.to_string(), |label| format!("{id} ({label})"))
                })
                .collect::<Vec<_>>()
                .join(", ");
            if !wit.is_empty() {
                note.text(&format!("{wit}: "), &ns);
            }
            self.render(reading, &mut note, &ns)?;
            if reading.text().trim().is_empty() {
                note.text("[omitted]", &ns);
            }
            alternatives += 1;
        }
        if alternatives > 0 || !s.in_note {
            let note = note.finish();
            if s.in_note {
                out.text(&format!(" [{}]", doc::xref::clean(&note.text)), s);
                self.warn(e, "nested-apparatus", "Apparatus within a note retained inline in that note.");
            } else {
                out.literal(&doc::ENDNOTE_REF.to_string(), s);
                out.story.endnotes.push(Arc::new(doc::Footnote { id: self.id(), text: note, annotation: None, apparatus: Some(apparatus) }));
            }
        }
        Ok(())
    }
    fn reference(&mut self, e: &Element, out: &mut Builder, s: &State) -> Result<(), ImportError> {
        let target = e.attr("target").split_whitespace().next().unwrap_or("");
        if e.attr("target").split_whitespace().count() > 1 {
            self.warn(e, "multi-target-reference", "First target is active; all targets retained in the source.");
        }
        let start = self.id();
        out.anchor(start, "Link start", s);
        if e.children.is_empty() {
            out.text(target, s);
        } else {
            self.children(e, out, s)?;
        }
        let end = self.id();
        out.anchor(end, "Link end", s);
        self.pending.push(PendingLink {
            start,
            end,
            target: if target.starts_with('#') { target.into() } else { join_uri(&s.base, target) },
            label: words(&e.text()),
            location: location(e, "unresolved-reference", "Reference target was not imported; label retained without an active link."),
        });
        Ok(())
    }
    pub fn resolve_links(&mut self) {
        for p in std::mem::take(&mut self.pending) {
            let dest = if let Some(id) = p.target.strip_prefix('#') {
                self.ids.get(id).copied().map(HyperlinkDest::Anchor)
            } else if let Some(email) = p.target.strip_prefix("mailto:") {
                Some(HyperlinkDest::Email(email.into()))
            } else if p.target.starts_with("https://") || p.target.starts_with("http://") {
                Some(HyperlinkDest::Url(p.target.clone()))
            } else {
                None
            };
            if let Some(dest) = dest {
                let id = self.id();
                self.links.push(Hyperlink {
                    id,
                    name: if p.label.is_empty() { p.target } else { p.label },
                    source: HyperlinkSource::Anchors { start: p.start, end: p.end },
                    dest,
                });
            } else {
                self.diagnostics.push(p.location);
            }
        }
    }
    fn graphic(&mut self, e: &Element, out: &mut Builder, s: &State) -> Result<(), ImportError> {
        let url = join_uri(&s.base, e.attr("url"));
        let id = self.id();
        let mut item = doc::Item::new(
            doc::ItemId(id),
            doc::LayerId(0),
            doc::Shape::Rectangle,
            doc::geom::shapes::rectangle(doc::geom::Rect::new(0.0, 0.0, 120.0, 80.0)),
        );
        item.stroke = doc::Stroke::default();
        item.alt_text = words(&e.text());
        item.name = if url.is_empty() { format!("Unsupported {}", e.name) } else { url.clone() };
        out.prepare(s);
        out.literal(&doc::OBJECT_MARK.to_string(), s);
        out.story.objects.push(Arc::new(doc::AnchoredObject::new(
            item,
            doc::AnchorPosition::AboveLine { align: doc::anchored::AnchorAlign::Center, space_before: 6.0, space_after: 6.0 },
        )));
        let description = if e.attr("n").is_empty() { words(&e.text()) } else { e.attr("n").into() };
        self.graphics.push(Graphic {
            item: id,
            url,
            description,
            width: styles::points(e.attr("width")),
            height: styles::points(e.attr("height")),
            location: location(e, "missing-resource", "Illustration could not be resolved; placeholder retained."),
        });
        Ok(())
    }
    fn table(&mut self, e: &Element, out: &mut Builder, s: &State) -> Result<(), ImportError> {
        if let Some(head) = e.child("head") {
            let mut caption = s.clone();
            caption.para = para("Caption");
            out.start(&caption.para);
            self.children(head, out, &caption)?;
            out.end();
        }
        let rows: Vec<_> = e.elements().filter(|r| r.is("row")).collect();
        if rows.is_empty() {
            self.warn(e, "table", "Table has no rows; child text retained.");
            return self.children(e, out, s);
        }
        if rows.len() > 10_000 {
            return Err(err("table exceeds 10000 rows"));
        }
        let mut occupied = BTreeSet::new();
        let mut placements = Vec::new();
        let mut cols = 0;
        for (r, row) in rows.iter().enumerate() {
            let mut c = 0usize;
            for cell in row.elements().filter(|c| c.is("cell")) {
                while occupied.contains(&(r, c)) {
                    c += 1;
                }
                let span = |key: &str| -> Result<usize, ImportError> {
                    if cell.attr(key).is_empty() {
                        Ok(1)
                    } else {
                        cell.attr(key)
                            .parse::<usize>()
                            .ok()
                            .filter(|n| *n > 0 && *n <= 10000)
                            .ok_or_else(|| err(format!("invalid table {key} span at line {}", cell.line)))
                    }
                };
                let (rs, cs) = (span("rows")?, span("cols")?);
                if r + rs > rows.len() || c + cs > 256 {
                    return Err(err("table span exceeds its rows or 256 columns"));
                }
                for rr in r..r + rs {
                    for cc in c..c + cs {
                        if !occupied.insert((rr, cc)) {
                            return Err(err("overlapping table spans"));
                        }
                    }
                }
                placements.push((r, c, rs, cs, cell));
                c += cs;
                cols = cols.max(c);
            }
        }
        if cols == 0 || rows.len() * cols > 1_000_000 {
            return Err(err("empty or excessively large table grid"));
        }
        let id = self.id();
        let mut table = doc::Table::new(id, rows.len(), cols, 0, 0, 432.0);
        for (r, row) in rows.iter().enumerate() {
            if row.attr("role") == "label" {
                table.rows[r].kind = doc::RowKind::Header;
            }
        }
        for (r, c, rs, cs, cell) in placements {
            let mut state = self.state(cell, s);
            state.para = para("Table");
            state.in_table = true;
            if rows[r].attr("role") == "label" || cell.attr("role") == "label" {
                state.chars.over.font_style = Some("Bold".into());
            }
            let mut b = Builder::new(self.next_story());
            if !cell.attr("xml:id").is_empty() {
                self.anchor(cell, &mut b, &state);
            }
            self.children(cell, &mut b, &state)?;
            let target = table.cell_mut(r, c).ok_or_else(|| err("invalid table cell"))?;
            target.row_span = rs as u32;
            target.col_span = cs as u32;
            target.text = b.finish();
        }
        out.start(&s.para);
        out.literal(&doc::TABLE_ANCHOR.to_string(), s);
        if let Some(p) = out.story.paras.last_mut() {
            p.table = Some(id);
        }
        out.story.tables.insert(id, Arc::new(table));
        out.end();
        Ok(())
    }
}

/// XML Base resolution for local resources and absolute URL bases. Nothing is fetched here.
pub fn join_uri(base: &str, reference: &str) -> String {
    if reference.is_empty() {
        return base.into();
    }
    if reference.contains("://") || reference.starts_with('/') || reference.starts_with("data:") {
        return reference.into();
    }
    if base.is_empty() {
        return reference.into();
    }
    let prefix = if base.ends_with('/') { base } else { base.rsplit_once('/').map(|(p, _)| p).unwrap_or("") };
    if prefix.is_empty() { reference.into() } else { format!("{}/{reference}", prefix.trim_end_matches('/')) }
}
