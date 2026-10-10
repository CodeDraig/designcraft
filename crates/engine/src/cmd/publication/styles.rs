use std::sync::Arc;

use designcraft_doc::{
    ComponentId, EditionId,
    publication::{Publication, styles::StyleChange},
};
use serde_json::{Value, json};

use super::{CommandSpec, Result, Session, bad, cmd, has_publication, mutate, optional_id, view};

#[derive(Clone, Debug)]
pub(crate) struct Preview {
    id: u64,
    root: Arc<Publication>,
    edition: EditionId,
    rows: Vec<StyleChange>,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "publication.styles.preview", "Preview Publication Style Synchronization", [], None,
            "{source?: component, sourceEdition?, edition?, components?: [id]} → {preview, rows:[{row,component,key,before,after,overridden,changed,dependencies}]} — no document edits or composition",
            has_publication, preview),
        cmd!(
            "publication.styles.apply",
            "Apply Publication Style Synchronization",
            [],
            None,
            "{preview, rows?: [row], replaceOverrides?: false} — omitted rows selects changed non-overrides; chosen overrides require replaceOverrides:true; one undo step; stale previews rejected",
            has_publication,
            apply
        ),
    ]
}

fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    let v = view(s)?;
    let source = ComponentId(optional_id(p, "source")?.unwrap_or(v.component.0));
    let source_edition = EditionId(optional_id(p, "sourceEdition")?.unwrap_or(v.edition.0));
    let edition = EditionId(optional_id(p, "edition")?.unwrap_or(v.edition.0));
    let components: Vec<ComponentId> = match p.get("components") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| bad("publication.styles.preview", e.to_string()))?,
        None => v.root.ordered_components()?,
    };
    let source_doc = v.root.project(source_edition, source)?;
    let count = designcraft_doc::publication::styles::definitions(&source_doc).len();
    if count.saturating_mul(components.len()) > 100_000 {
        return Err(bad("publication.styles.preview", "more than 100000 rows; select fewer components"));
    }
    let rows = v.root.style_preview(source_edition, source, edition, &components)?;
    let st = s.doc_mut()?;
    st.publication_preview_id =
        st.publication_preview_id.checked_add(1).ok_or_else(|| bad("publication.styles.preview", "preview counter exhausted"))?;
    let id = st.publication_preview_id;
    let response = rows
        .iter()
        .enumerate()
        .map(|(row, change)| {
            let mut value = serde_json::to_value(change).map_err(|e| bad("publication.styles.preview", e.to_string()))?;
            value["row"] = json!(row);
            Ok(value)
        })
        .collect::<Result<Vec<_>>>()?;
    st.publication_style_preview = Some(Preview { id, root: v.root, edition, rows });
    Ok(json!({"preview":id,"rows":response}))
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let id = optional_id(p, "preview")?.ok_or_else(|| bad("publication.styles.apply", "preview required"))?;
    let preview = s.doc()?.publication_style_preview.clone().ok_or_else(|| bad("publication.styles.apply", "create a style preview first"))?;
    if preview.id != id || !Arc::ptr_eq(&view(s)?.root, &preview.root) {
        return Err(bad("publication.styles.apply", "stale style preview: the publication changed; preview again"));
    }
    let selected: Vec<usize> = match p.get("rows") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| bad("publication.styles.apply", e.to_string()))?,
        None => preview.rows.iter().enumerate().filter(|(_, r)| r.changed && !r.overridden).map(|(i, _)| i).collect(),
    };
    let replace = p.get("replaceOverrides").and_then(Value::as_bool).unwrap_or(false);
    let mut changes = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for row in selected {
        if !seen.insert(row) {
            return Err(bad("publication.styles.apply", "duplicate row selection"));
        }
        let row = preview.rows.get(row).ok_or_else(|| bad("publication.styles.apply", "row out of range"))?;
        if row.overridden && !replace {
            return Err(bad(
                "publication.styles.apply",
                format!("{} in {} is an override; explicitly enable replacement", row.key, row.component_title),
            ));
        }
        changes.push(row.clone());
    }
    if changes.is_empty() {
        return Ok(json!({"applied":0}));
    }
    mutate(s, |root, _| {
        root.apply_styles(preview.edition, &changes)?;
        Ok(())
    })?;
    s.doc_mut()?.publication_style_preview = None;
    Ok(json!({"applied":changes.len()}))
}
