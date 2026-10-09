//! Layers command implementations.

use super::{LAYER_COLORS, Layer, LayerId, Result, Value, bad, json, ok, str_param};
use crate::Session;

pub(super) fn layer_move(s: &mut Session, p: &Value) -> Result<Value> {
    let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
    let to = p.get("to").and_then(Value::as_u64).unwrap_or(0) as usize;
    s.edit(|d, _| {
        let i = d.layers.iter().position(|l| l.id == id).ok_or_else(|| bad("layer.move", "no such layer"))?;
        let l = d.layers.remove(i);
        let to = to.min(d.layers.len());
        d.layers.insert(to, l);
        ok()
    })
}

pub(super) fn layer_new(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").map(str::to_string);
    let r = s.edit(|d, _| {
        let id = LayerId(d.alloc());
        let n = d.layers.len();
        let name = name.clone().unwrap_or_else(|| format!("Layer {}", n + 1));
        d.layers.insert(
            0,
            Layer {
                id,
                name,
                color: LAYER_COLORS[n % LAYER_COLORS.len()].1,
                visible: true,
                locked: false,
                printable: true,
                show_guides: true,
                suppress_wrap_when_hidden: false,
            },
        );
        Ok(id)
    })?;
    s.doc_mut()?.active_layer = r;
    Ok(json!({"id": r.0}))
}

pub(super) fn layer_set(s: &mut Session, p: &Value) -> Result<Value> {
    let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
    let p = p.clone();
    s.edit(|d, _| {
        let l = d.layer_mut(id).ok_or_else(|| bad("layer.set", "no such layer"))?;
        if let Some(v) = p.get("name").and_then(Value::as_str) {
            l.name = v.into();
        }
        if let Some(v) = p.get("visible").and_then(Value::as_bool) {
            l.visible = v;
        }
        if let Some(v) = p.get("locked").and_then(Value::as_bool) {
            l.locked = v;
        }
        if let Some(v) = p.get("printable").and_then(Value::as_bool) {
            l.printable = v;
        }
        if let Some(c) = p.get("color").and_then(|c| serde_json::from_value(c.clone()).ok()) {
            l.color = c;
        }
        ok()
    })
}

pub(super) fn layer_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
    s.edit(|d, sel| {
        if d.layers.len() < 2 {
            return Err(bad("layer.delete", "a document needs at least one layer"));
        }
        let doomed: Vec<_> =
            d.spreads.iter().chain(d.parents.iter()).flat_map(|sp| sp.items.iter().filter(|i| i.layer == id).map(|i| i.id)).collect();
        for i in doomed {
            let _ = d.remove_item(i);
        }
        d.layers.retain(|l| l.id != id);
        *sel = Default::default();
        ok()
    })
}

pub(super) fn layer_merge(s: &mut Session, p: &Value) -> Result<Value> {
    let ids: Vec<LayerId> =
        p.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(LayerId).collect()).unwrap_or_default();
    let into = p.get("into").and_then(Value::as_u64).map(LayerId).or_else(|| ids.first().copied()).ok_or_else(|| bad("layer.merge", "no layers"))?;
    s.edit(|d, _| {
        if d.layer(into).is_none() || ids.iter().any(|l| d.layer(*l).is_none()) {
            return Err(bad("layer.merge", "no such layer"));
        }
        let gone: Vec<LayerId> = ids.iter().copied().filter(|l| *l != into).collect();
        for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
            if !sp.items.iter().any(|i| gone.contains(&i.layer)) {
                continue;
            }
            for it in &mut std::sync::Arc::make_mut(sp).items {
                if gone.contains(&it.layer) {
                    std::sync::Arc::make_mut(it).layer = into;
                }
            }
        }
        d.layers.retain(|l| !gone.contains(&l.id));
        Ok(json!({"merged": gone.len()}))
    })
}

pub(super) fn layer_delete_unused(s: &mut Session, _: &Value) -> Result<Value> {
    s.edit(|d, _| {
        let used: std::collections::HashSet<LayerId> =
            d.spreads.iter().chain(d.parents.iter()).flat_map(|sp| sp.items.iter().map(|i| i.layer)).collect();
        let before = d.layers.len();
        if used.is_empty() {
            // Nothing anywhere: keep the first layer.
            d.layers.truncate(1);
        } else {
            d.layers.retain(|l| used.contains(&l.id));
        }
        Ok(json!({"deleted": before - d.layers.len()}))
    })
}

pub(super) fn layer_others(s: &mut Session, p: &Value) -> Result<Value> {
    let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
    let f = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    let (hide, lock, show, unlock) = (f("hide"), f("lock"), f("show"), f("unlock"));
    s.edit(|d, _| {
        for l in &mut d.layers {
            if show {
                l.visible = true;
            }
            if unlock {
                l.locked = false;
            }
            if l.id != id {
                if hide {
                    l.visible = false;
                }
                if lock {
                    l.locked = true;
                }
            }
        }
        ok()
    })
}

pub(super) fn layer_activate(s: &mut Session, p: &Value) -> Result<Value> {
    let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
    let st = s.doc_mut()?;
    if st.doc.layer(id).is_none() {
        return Err(bad("layer.activate", "no such layer"));
    }
    st.active_layer = id;
    st.revision += 1;
    ok()
}
