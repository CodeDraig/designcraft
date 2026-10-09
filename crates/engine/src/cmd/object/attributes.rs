//! Attributes command implementations.

use super::{Result, Session, Value, json, set_flag, str_param};

pub(super) fn object_attributes(s: &mut Session, p: &Value) -> Result<Value> {
    let b = |k: &str| p.get(k).and_then(Value::as_bool);
    let (of, os, og, np) = (b("overprintFill"), b("overprintStroke"), b("overprintGap"), b("nonprinting"));
    set_flag(
        s,
        p,
        move |i| {
            if let Some(v) = of {
                i.fill.overprint = v;
            }
            if let Some(v) = os {
                i.stroke.overprint = v;
            }
            if let Some(v) = og {
                i.stroke.gap_overprint = v;
            }
            if let Some(v) = np {
                i.nonprinting = v;
            }
        },
        false,
    )
}

pub(super) fn object_find_by_label(s: &mut Session, p: &Value) -> Result<Value> {
    let label = str_param(p, "label").unwrap_or("");
    let d = &s.doc()?.doc;
    let mut ids = Vec::new();
    for sp in d.spreads.iter().chain(d.parents.iter()) {
        for it in &sp.items {
            it.walk(&mut |x| {
                if x.label == label {
                    ids.push(x.id.0);
                }
            });
        }
    }
    Ok(json!({"ids": ids}))
}
