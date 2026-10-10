use super::*;
use designcraft_doc::publication::editions::OutputSettings;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "publication.edition.get","Edition Settings",[],None,"{edition?} → output metadata, included generated sections, note policies and witness",has_publication,|s,p|{
            let v=view(s)?;let id=EditionId(optional_id(p,"edition")?.unwrap_or(v.edition.0));
            let e=v.root.editions.get(&id).ok_or_else(||bad("publication.edition.get","unknown edition"))?;
            let sections:Vec<_>=v.root.generated_destinations().filter(|(_,c,st,_)|v.root.note_output(*c,*st).is_none()).map(|(name,c,st,content)|json!({"name":name,"component":c,"story":st,"content":content,"included":v.root.story_included(id,c,st)})).collect();
            Ok(json!({"edition":id,"name":e.name,"output":e.output,"witness":e.apparatus_witness,"notePolicies":e.note_policies,"streams":v.root.note_streams,"sections":sections,"pages":e.components.values().map(|c|c.page_count()).sum::<usize>()}))
        }),
        cmd!(
            "publication.edition.configure",
            "Configure Edition",
            [],
            None,
            "{edition?, name?, output?: {title?:string|null, author?:string|null, language?:string|null, identifier?:string|null, excludedSections?:[contentId]}, notePolicies?:{stream:policy}, witness?:string|null} — one undoable edition configuration; generated text and layout update before committing",
            has_publication,
            configure
        ),
    ]
}

fn configure(s: &mut Session, p: &Value) -> Result<Value> {
    let id = EditionId(optional_id(p, "edition")?.unwrap_or(view(s)?.edition.0));
    let cache = s.cache.clone();
    mutate(s, |root, v| {
        let original = root.editions.get(&id).ok_or_else(|| bad("publication.edition.configure", "unknown edition"))?;
        let mut e = (**original).clone();
        if let Some(name) = p.get("name") {
            e.name = name
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.len() <= 256)
                .ok_or_else(|| bad("publication.edition.configure", "name must contain 1–256 bytes"))?
                .into();
        }
        if let Some(patch) = p.get("output") {
            let patch = patch.as_object().ok_or_else(|| bad("publication.edition.configure", "output must be an object"))?;
            let mut settings = serde_json::to_value(&e.output).map_err(|e| bad("publication.edition.configure", e.to_string()))?;
            for (k, value) in patch {
                settings[k] = value.clone();
            }
            e.output = serde_json::from_value::<OutputSettings>(settings).map_err(|e| bad("publication.edition.configure", e.to_string()))?;
            if patch.contains_key("excludedSections") {
                let available: std::collections::BTreeSet<_> =
                    root.generated_destinations().filter(|(_, c, s, _)| root.note_output(*c, *s).is_none()).map(|(_, _, _, id)| id).collect();
                if !e.output.excluded_sections.is_subset(&available) {
                    return Err(bad("publication.edition.configure", "excludedSections must name generated sections or indexes"));
                }
            }
            e.output.check(root)?;
        }
        if let Some(policies) = p.get("notePolicies") {
            for (name, patch) in policies.as_object().ok_or_else(|| bad("publication.edition.configure", "notePolicies must be an object"))? {
                if !root.note_streams.contains_key(name) {
                    return Err(bad("publication.edition.configure", format!("unknown note stream {name}")));
                }
                let mut value = serde_json::to_value(root.note_policy(id, name)).map_err(|e| bad("publication.edition.configure", e.to_string()))?;
                for (key, v) in patch.as_object().ok_or_else(|| bad("publication.edition.configure", "note policy must be an object"))? {
                    value[key] = v.clone();
                }
                let policy: designcraft_doc::publication::annotations::Policy =
                    serde_json::from_value(value).map_err(|e| bad("publication.edition.configure", e.to_string()))?;
                policy.check()?;
                e.note_policies.insert(name.clone(), policy);
            }
        }
        if let Some(w) = p.get("witness") {
            e.apparatus_witness = match w {
                Value::Null => None,
                Value::String(w) if w.len() <= 256 => Some(w.trim_start_matches('#').into()),
                _ => return Err(bad("publication.edition.configure", "witness must be a string of at most 256 bytes or null")),
            };
        }
        root.editions.insert(id, Arc::new(e));
        root.revision = root.revision.saturating_add(1);
        let mut selected = PublicationView { root: Arc::new(root.clone()), edition: id, component: v.component }.project()?;
        let report = super::update::refresh(&mut selected, true, &cache)?;
        *root = (*selected.publication.ok_or_else(|| bad("publication.edition.configure", "missing candidate publication"))?.root).clone();
        Ok(json!({"edition":id,"layout":report}))
    })
}

#[cfg(test)]
mod tests;
