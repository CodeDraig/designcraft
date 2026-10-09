//! Resolve story threads, index topics and cross-reference formats.

use super::{Arc, BTreeMap, El, HashMap, HashSet, Importer, Item, ItemId, StoryId};

impl Importer<'_> {
    /// Index topics (recursively); at the root, their See / See also cross-references too.
    pub(super) fn index_topics_of(&mut self, e: &El, prefix: &[String]) {
        for t in e.find_all("Topic") {
            let mut path = prefix.to_vec();
            path.push(t.get("Name").unwrap_or("").to_string());
            if let Some(me) = t.get("Self") {
                self.index_topics.insert(me.to_string(), path.clone());
            }
            self.index_topics_of(t, &path);
        }
        if !prefix.is_empty() {
            return;
        }
        let mut stack = vec![(e, Vec::<String>::new())];
        while let Some((el, path)) = stack.pop() {
            for t in el.find_all("Topic") {
                let mut p = path.clone();
                p.push(t.get("Name").unwrap_or("").to_string());
                for x in t.find_all("CrossReference") {
                    let target = x.get("ReferencedTopic").and_then(|r| self.index_topics.get(r)).map(|p| p.join(": ")).unwrap_or_default();
                    use designcraft_doc::index::IndexRange as R;
                    let range = match x.get("CrossReferenceType") {
                        Some("SeeAlso") | Some("SeeAlsoHerein") => R::SeeAlso(target),
                        _ => R::See(target),
                    };
                    self.index_xrefs.push(designcraft_doc::IndexRef { topics: p.clone(), sort: vec![], range });
                }
                stack.push((t, p));
            }
        }
    }

    /// `CrossReferenceFormat`: building blocks back to a format definition.
    pub(super) fn xref_format(&mut self, e: &El) {
        let name = e.get("Name").unwrap_or("Format").to_string();
        let mut def = String::new();
        for b in e.find_all("BuildingBlock") {
            let text = |k: &str| b.get(k).filter(|v| *v != "$ID/").unwrap_or("").to_string();
            match b.get("BlockType").unwrap_or("") {
                "CustomStringBuildingBlock" => def.push_str(&text("CustomText")),
                "FullParagraphBuildingBlock" => def.push_str("<fullPara />"),
                "ParagraphTextBuildingBlock" => def.push_str("<paraText />"),
                "ParagraphNumberBuildingBlock" => def.push_str("<paraNum />"),
                "PageNumberBuildingBlock" => def.push_str("<pageNum />"),
                "BookmarkNameBuildingBlock" => def.push_str("<txtAnchrName />"),
                "ChapterNumberBuildingBlock" => def.push_str("<chapNum />"),
                "FileNameBuildingBlock" => def.push_str("<fileName />"),
                "PartialParagraphBuildingBlock" => def.push_str(&format!(
                    "<partialPara delim=\"{}\" includeDelim=\"{}\" />",
                    text("AppliedDelimiter"),
                    b.get("IncludeDelimiter") == Some("true")
                )),
                _ => {}
            }
        }
        if let Some(me) = e.get("Self") {
            self.xref_format_names.insert(me.to_string(), name.clone());
        }
        match self.xref_formats.iter_mut().find(|f| f.name == name) {
            Some(f) => f.definition = def,
            None => self.xref_formats.push(designcraft_doc::XrefFormat { name, definition: def }),
        }
    }

    pub(super) fn link_threads(&mut self) {
        let threads = std::mem::take(&mut self.ctx.threads);
        let by_self: HashMap<String, ItemId> = self.item_ids.clone();
        let mut per_story: BTreeMap<StoryId, Vec<usize>> = BTreeMap::new();
        for (i, (id, _, _, _)) in threads.iter().enumerate() {
            let sid = self.frame_story(*id);
            if let Some(sid) = sid {
                per_story.entry(sid).or_default().push(i);
            }
        }
        for (sid, idx) in per_story {
            let members: HashSet<ItemId> = idx.iter().map(|i| threads[*i].0).collect();
            let next_of: HashMap<ItemId, ItemId> =
                idx.iter().filter_map(|i| by_self.get(&threads[*i].3).filter(|n| members.contains(n)).map(|n| (threads[*i].0, *n))).collect();
            let has_prev: HashSet<ItemId> = next_of.values().copied().collect();
            let mut order = Vec::new();
            let mut seen = HashSet::new();
            for i in &idx {
                let head = threads[*i].0;
                if has_prev.contains(&head) || seen.contains(&head) {
                    continue;
                }
                let mut cur = Some(head);
                while let Some(c) = cur {
                    if !seen.insert(c) {
                        break;
                    }
                    order.push(c);
                    cur = next_of.get(&c).copied();
                }
            }
            for i in &idx {
                let f = threads[*i].0;
                if seen.insert(f) {
                    order.push(f);
                }
            }
            if let Some(s) = self.stories.get_mut(&sid) {
                s.frames = order;
            }
        }
    }

    pub(super) fn frame_story(&self, id: ItemId) -> Option<StoryId> {
        fn find(items: &[Arc<Item>], id: ItemId) -> Option<StoryId> {
            for it in items {
                let mut r = None;
                it.walk(&mut |i| {
                    if i.id == id {
                        r = i.text_frame().map(|t| t.story);
                    }
                });
                if r.is_some() {
                    return r;
                }
            }
            None
        }
        self.spreads.iter().chain(self.parents.iter()).find_map(|s| find(&s.items, id))
    }
}
