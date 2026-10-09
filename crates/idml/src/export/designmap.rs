//! Write document identity, relationships and the package design map.

use super::{BTreeSet, DOM_VERSION, El, Ex, HashSet, PKG_NS, bool_s, document_with_pi, escape_id, names, num, p, uid, with_props};

impl Ex<'_> {
    pub(super) fn designmap(&mut self, masters: &[(String, Vec<u8>)], spreads: &[(String, Vec<u8>)], stories: &[(String, Vec<u8>)]) -> Vec<u8> {
        let d = self.d;
        let story_list: Vec<String> = d.stories.keys().map(|k| uid(k.0)).collect();
        let mut root = El::new("Document")
            .attr("xmlns:idPkg", PKG_NS)
            .attr("DOMVersion", DOM_VERSION)
            .attr("Self", "d")
            .attr("StoryList", story_list.join(" "))
            .attr("Name", format!("{}.indd", d.title))
            .attr("ZeroPoint", "0 0")
            .attr("ActiveLayer", d.layers.first().map(|l| uid(l.id.0)).unwrap_or_else(|| "n".into()));
        root.push(
            El::new("Language")
                .attr("Self", "Language/$ID/[No Language]")
                .attr("Name", "$ID/[No Language]")
                .attr("SingleQuotes", "''")
                .attr("DoubleQuotes", "\"\"")
                .attr("PrimaryLanguageName", "$ID/[No Language]")
                .attr("SublanguageName", "$ID/[No Language]")
                .attr("Id", "0")
                .attr("HyphenationVendor", "$ID/")
                .attr("SpellingVendor", "$ID/"),
        );
        root.push(
            El::new("Language")
                .attr("Self", "Language/$ID/English%3a USA")
                .attr("Name", "$ID/English: USA")
                .attr("SingleQuotes", "\u{2018}\u{2019}")
                .attr("DoubleQuotes", "\u{201C}\u{201D}")
                .attr("PrimaryLanguageName", "$ID/English")
                .attr("SublanguageName", "$ID/USA")
                .attr("Id", "269")
                .attr("HyphenationVendor", "Hunspell")
                .attr("SpellingVendor", "Hunspell"),
        );
        let mut sets = std::collections::BTreeMap::new();
        for a in d.styles.paragraph.iter().map(|p| &p.para).chain(d.stories.values().flat_map(|s| s.paras.iter().map(|p| &p.para))) {
            if let Some(Some(k)) = &a.kinsoku
                && !k.name.is_empty()
            {
                sets.insert(&k.name, k);
            }
        }
        for k in sets.values() {
            root.push(
                El::new("KinsokuTable")
                    .attr("Self", format!("KinsokuTable/{}", escape_id(&k.name)))
                    .attr("Name", &k.name)
                    .attr("CantBeginLineChars", &k.no_start)
                    .attr("CantEndLineChars", &k.no_end)
                    .attr("CantBeSeparatedChars", &k.inseparable)
                    .attr("HangingPunctuationChars", &k.hanging),
            );
        }
        for t in &d.styles.mojikumi_tables {
            let mut e = El::new("MojikumiTable").attr("Self", format!("MojikumiTable/{}", escape_id(&t.name))).attr("Name", &t.name);
            if !t.based_on.is_empty() {
                e.set("BasedOnMojikumiSet", &t.based_on);
            }
            if !t.overrides.is_empty() {
                let mut list = El::new("OverrideMojikumiAkiList");
                for r in &t.overrides {
                    list.push(
                        El::new("OverrideMojikumiAkiType")
                            .attr("TargetMojikumiClass", r.target_class)
                            .attr("SideMojikumiClass", r.side_class)
                            .attr("SideIsAfterTarget", bool_s(r.after))
                            .attr("Minimum", num(r.minimum))
                            .attr("Desired", num(r.desired))
                            .attr("Maximum", num(r.maximum))
                            .attr("CompressionPriority", r.priority)
                            .attr("AkiDoesNotFloat", bool_s(r.does_not_float)),
                    );
                }
                e.push(El::new("Properties").child(list));
            }
            root.push(e);
        }
        root.push(El::new("idPkg:Graphic").attr("src", "Resources/Graphic.xml"));
        root.push(El::new("idPkg:Fonts").attr("src", "Resources/Fonts.xml"));
        root.push(El::new("idPkg:Styles").attr("src", "Resources/Styles.xml"));
        root.push(
            El::new("NumberingList")
                .attr("Self", "NumberingList/$ID/[Default]")
                .attr("Name", "$ID/[Default]")
                .attr("ContinueNumbersAcrossStories", "false")
                .attr("ContinueNumbersAcrossDocuments", "false"),
        );
        root.push(El::new("idPkg:Preferences").attr("src", "Resources/Preferences.xml"));
        for c in &d.conditions {
            root.push(
                El::new("Condition")
                    .attr("Self", format!("Condition/{}", escape_id(&c.name)))
                    .attr("Name", &c.name)
                    .attr("IndicatorColor", format!("{} {} {}", c.color[0], c.color[1], c.color[2]))
                    .attr("IndicatorMethod", "UseUnderline")
                    .attr("UnderlineIndicatorAppearance", "Solid")
                    .attr("Visible", bool_s(c.visible)),
            );
        }
        // Variables InDesign's cross-reference page/chapter blocks rely on.
        for (name, ty) in [("XRefChapterNumber", "XrefChapterNumberType"), ("XRefPageNumber", "XrefPageNumberType")] {
            let n = format!("<?AID 001b?>TV {name}");
            root.push(El::new("TextVariable").attr("Self", format!("dTextVariablen{n}")).attr("Name", &n).attr("VariableType", ty));
        }
        for l in &d.layers {
            let color = match names::layer_color_out(l.color) {
                Some(n) => p("LayerColor", "enumeration", n),
                None => {
                    let mut e = El::new("LayerColor").attr("type", "list");
                    for c in l.color {
                        e.push(p("ListItem", "double", num(c as f64)));
                    }
                    e
                }
            };
            let el = El::new("Layer")
                .attr("Self", uid(l.id.0))
                .attr("Name", &l.name)
                .attr("Visible", bool_s(l.visible))
                .attr("Locked", bool_s(l.locked))
                .attr("IgnoreWrap", bool_s(l.suppress_wrap_when_hidden))
                .attr("ShowGuides", bool_s(l.show_guides))
                .attr("LockGuides", "false")
                .attr("UI", "true")
                .attr("Expendable", "true")
                .attr("Printable", bool_s(l.printable));
            root.push(with_props(el, vec![color]));
        }
        for (name, _) in masters {
            root.push(El::new("idPkg:MasterSpread").attr("src", name));
        }
        for (name, _) in spreads {
            root.push(El::new("idPkg:Spread").attr("src", name));
        }
        // Sections.
        let mut secs = d.sections.clone();
        secs.sort_by_key(|s| s.start);
        if secs.is_empty() || secs[0].start != 0 {
            secs.insert(
                0,
                designcraft_doc::Section {
                    start: 0,
                    start_number: Some(1),
                    style: Default::default(),
                    prefix: String::new(),
                    marker: String::new(),
                    include_prefix: false,
                },
            );
        }
        let total = d.page_count();
        for (i, s) in secs.iter().enumerate() {
            if s.start >= total {
                continue;
            }
            let end = secs.get(i + 1).map(|n| n.start).unwrap_or(total).min(total);
            let Some(page) = d.page(s.start) else { continue };
            let mut el = El::new("Section")
                .attr("Self", self.fresh())
                .attr("Length", end.saturating_sub(s.start).max(1))
                .attr("Name", &s.prefix)
                .attr("ContinueNumbering", bool_s(s.start_number.is_none()))
                .attr("IncludeSectionPrefix", bool_s(s.include_prefix));
            if let Some(n) = s.start_number {
                el.set("PageNumberStart", n);
            }
            el = el.attr("Marker", &s.marker).attr("PageStart", uid(page.id.0)).attr("SectionPrefix", &s.prefix);
            root.push(with_props(el, vec![p("PageNumberStyle", "enumeration", names::number_style_out(s.style))]));
        }
        if !self.index_topics.is_empty() || !self.index_xrefs.is_empty() {
            root.push(self.index_el());
        }
        // Formats before the stories whose sources apply them.
        for (i, f) in d.xref_formats.iter().enumerate() {
            root.push(xref_format_el(i, f));
        }
        for (name, _) in stories {
            root.push(El::new("idPkg:Story").attr("src", name));
        }
        for (src, target) in self.xref_sources.clone() {
            let mut h = El::new("Hyperlink")
                .attr("Self", self.fresh())
                .attr("Name", "Cross-Reference")
                .attr("Source", &src)
                .attr("Visible", "false")
                .attr("Highlight", "None")
                .attr("Width", "Thin")
                .attr("BorderStyle", "Solid")
                .attr("Hidden", "false")
                .attr("DestinationUniqueKey", target & 0xFFFF_FFFF);
            let mut props = El::new("Properties");
            props.push(p("Destination", "object", anchor_self(target)));
            h.push(props);
            root.push(h);
        }
        // Root colour group: the Swatches panel order.
        let mut cg =
            El::new("ColorGroup").attr("Self", "ColorGroup/[Root Color Group]").attr("Name", "[Root Color Group]").attr("IsRootColorGroup", "true");
        let mut seen = HashSet::new();
        for s in &d.swatches {
            let id = self.sw(&s.name);
            if seen.insert(id.clone()) {
                let n = seen.len() - 1;
                cg.push(El::new("ColorGroupSwatch").attr("Self", format!("u{:x}ColorGroupSwatch{n:x}", self.next + 1)).attr("SwatchItemRef", id));
            }
        }
        self.next += 1;
        root.push(cg);
        document_with_pi(
            &root,
            &format!("<?aid style=\"50\" type=\"document\" readerVersion=\"6.0\" featureSet=\"257\" product=\"{DOM_VERSION}(1)\" ?>"),
        )
    }

    /// `<Index>`: nested topics with their See / See also cross-references.
    pub(super) fn index_el(&mut self) -> El {
        let mut topics = self.index_topics.clone();
        for (_, _, target) in &self.index_xrefs {
            topics.insert(vec![target.clone()]);
        }
        fn build(ex: &mut Ex, topics: &BTreeSet<Vec<String>>, prefix: &[String]) -> Vec<El> {
            let mut out = Vec::new();
            for t in topics.iter().filter(|t| t.len() == prefix.len() + 1 && t.starts_with(prefix)) {
                let mut el = El::new("Topic").attr("Self", topic_self(t)).attr("SortOrder", "").attr("Name", t.last().map_or("", String::as_str));
                for (from, kind, target) in ex.index_xrefs.clone() {
                    if from == *t {
                        el.push(
                            El::new("CrossReference")
                                .attr("Self", ex.fresh())
                                .attr("ReferencedTopic", topic_self(std::slice::from_ref(&target)))
                                .attr("CrossReferenceType", kind)
                                .attr("CustomTypeString", ""),
                        );
                    }
                }
                for c in build(ex, topics, t) {
                    el.push(c);
                }
                out.push(el);
            }
            out
        }
        let mut el = El::new("Index").attr("Self", "ix");
        for t in build(self, &topics, &[]) {
            el.push(t);
        }
        el
    }
}

pub(super) fn anchor_self(id: u64) -> String {
    format!("HyperlinkTextDestination/a{id}")
}

pub(super) fn xref_format_self(i: usize) -> String {
    format!("CrossReferenceFormat/f{i}")
}

/// A cross-reference format as InDesign building blocks.
fn xref_format_el(i: usize, f: &designcraft_doc::XrefFormat) -> El {
    let me = xref_format_self(i);
    let mut el = El::new("CrossReferenceFormat").attr("Self", &me).attr("Name", &f.name).attr("AppliedCharacterStyle", "n");
    let mut k = 0;
    let mut block = |el: &mut El, kind: &str, text: &str, delim: &str, include: bool| {
        el.push(
            El::new("BuildingBlock")
                .attr("Self", format!("{me}BuildingBlock{k}"))
                .attr("BlockType", kind)
                .attr("AppliedCharacterStyle", "n")
                .attr("CustomText", if text.is_empty() { "$ID/" } else { text })
                .attr("AppliedDelimiter", if delim.is_empty() { "$ID/" } else { delim })
                .attr("IncludeDelimiter", bool_s(include)),
        );
        k += 1;
    };
    let mut rest = f.definition.as_str();
    while let Some(a) = rest.find('<') {
        if a > 0 {
            block(&mut el, "CustomStringBuildingBlock", &rest[..a], "", false);
        }
        let Some(b) = rest[a..].find('>') else { break };
        let tag = rest[a + 1..a + b].trim().trim_end_matches('/').trim();
        let attr = |k: &str| tag.find(&format!("{k}=\"")).and_then(|i| tag[i + k.len() + 2..].split('"').next()).unwrap_or("").to_string();
        match tag.split_whitespace().next().unwrap_or("") {
            "fullPara" => block(&mut el, "FullParagraphBuildingBlock", "", "", false),
            "paraText" => block(&mut el, "ParagraphTextBuildingBlock", "", "", false),
            "paraNum" => block(&mut el, "ParagraphNumberBuildingBlock", "", "", false),
            "pageNum" => block(&mut el, "PageNumberBuildingBlock", "", "", false),
            "txtAnchrName" => block(&mut el, "BookmarkNameBuildingBlock", "", "", false),
            "chapNum" => block(&mut el, "ChapterNumberBuildingBlock", "", "", false),
            "fileName" => block(&mut el, "FileNameBuildingBlock", "", "", false),
            "partialPara" => block(&mut el, "PartialParagraphBuildingBlock", "", &attr("delim"), attr("includeDelim") == "true"),
            _ => block(&mut el, "CustomStringBuildingBlock", &rest[a..a + b + 1], "", false),
        }
        rest = &rest[a + b + 1..];
    }
    if !rest.is_empty() {
        block(&mut el, "CustomStringBuildingBlock", rest, "", false);
    }
    el
}

pub(super) fn topic_self(path: &[String]) -> String {
    let mut s = String::from("ix");
    for n in path {
        s.push_str("Topicn");
        s.push_str(&escape_id(n));
    }
    s
}
