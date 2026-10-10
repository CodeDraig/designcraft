use super::{Result, Session, bad, mutate, optional_id};
use designcraft_doc::{
    SemanticId,
    publication::numbering::{ObjectKind, SemanticObject, TextTarget},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rule {
    style: String,
    kind: ObjectKind,
    #[serde(default = "one")]
    level: u8,
}
fn one() -> u8 {
    1
}

pub(super) fn scan(s: &mut Session, p: &Value) -> Result<Value> {
    let rules: Vec<Rule> =
        serde_json::from_value(p.get("styles").cloned().unwrap_or(json!([]))).map_err(|e| bad("publication.semantic.scan", e.to_string()))?;
    if rules.iter().any(|r| r.kind == ObjectKind::Chapter || r.level == 0 || r.level > 9) || rules.len() > 128 {
        return Err(bad("publication.semantic.scan", "supply at most 128 heading/figure/table style rules, with levels 1–9"));
    }
    let component = optional_id(p, "component")?.map(designcraft_doc::ComponentId);
    let tei = p.get("tei").and_then(Value::as_bool).unwrap_or(true);
    mutate(s, |root, v| {
        let mut added = 0usize;
        for cid in root.ordered_components()?.into_iter().filter(|id| component.is_none_or(|c| c == *id)) {
            let mut d = root.project(v.edition, cid)?;
            let existing: std::collections::BTreeSet<_> =
                root.semantic.values().filter(|o| o.target.component == cid).map(|o| o.target.anchor).collect();
            let navigation: Vec<_> = if tei {
                d.tei_sources.iter().flat_map(|source| source.navigation.iter()).filter(|n| n.kind == "heading").cloned().collect()
            } else {
                vec![]
            };
            let mut found = Vec::new();
            for (&sid, story) in &mut d.stories {
                Arc::make_mut(story).for_each_text_mut(&mut |st| {
                    for n in &navigation {
                        if st.anchor_pos(n.anchor).is_some() && !existing.contains(&n.anchor) {
                            found.push((sid, n.anchor, ObjectKind::Heading, n.level.clamp(1, 9) as u8));
                        }
                    }
                    let ranges = st.para_ranges();
                    for (i, range) in ranges.into_iter().enumerate().rev() {
                        if let Some(rule) = st.paras.get(i).and_then(|p| rules.iter().find(|r| r.style == p.style)) {
                            let anchor = st
                                .anchors
                                .iter()
                                .find(|a| st.anchor_pos(a.id).is_some_and(|p| p >= range.start && p < range.end))
                                .map(|a| a.id)
                                .unwrap_or_else(|| st.insert_anchor(range.start, "Publication destination"));
                            if !existing.contains(&anchor) && !found.iter().any(|f| f.1 == anchor) {
                                found.push((sid, anchor, rule.kind, rule.level));
                            }
                        }
                    }
                });
            }
            root.capture(v.edition, cid, &d)?;
            for (sid, anchor, kind, level) in found {
                let content = *root
                    .components
                    .get(&cid)
                    .and_then(|c| c.stories.get(&sid))
                    .ok_or_else(|| bad("publication.semantic.scan", "missing content"))?;
                let id = SemanticId(root.alloc()?);
                root.semantic.insert(id, SemanticObject { id, kind, target: TextTarget { component: cid, content, anchor }, level, title: None });
                added += 1;
            }
        }
        if added > 0 {
            root.revision = root.revision.saturating_add(1);
        }
        Ok(json!({"added":added,"objects":root.semantic.len()}))
    })
}
