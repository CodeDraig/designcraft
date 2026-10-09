//! Colour swatches, gradients, and palette interchange.

use super::{Result, Session, Styles, Swatch, SwatchValue, Value, bad, json, ok, str_param};

pub(super) fn create_swatch(s: &mut Session, p: &Value) -> Result<Value> {
    let c = p.get("color").ok_or_else(|| bad("swatch.create", "missing color"))?;
    let color = parse_color(c).ok_or_else(|| bad("swatch.create", "bad color"))?;
    let spot = p.get("spot").and_then(Value::as_bool).unwrap_or(false);
    let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| match color {
        designcraft_color::Color::Cmyk { c, m, y, k } => designcraft_color::swatch::cmyk_name(c, m, y, k),
        other => {
            let [r, g, b] = other.to_rgb();
            format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round())
        }
    });
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.swatch(n).is_some(), &name);
        d.swatches.push(Swatch {
            name: name.clone(),
            value: SwatchValue::Color {
                color,
                color_type: if spot { designcraft_color::ColorType::Spot } else { designcraft_color::ColorType::Process },
            },
            locked: false,
            named: true,
            hidden: false,
        });
        Ok(json!({"name": name}))
    })
}

pub fn parse_color(v: &Value) -> Option<designcraft_color::Color> {
    use designcraft_color::Color;
    match v {
        Value::String(s) => Color::from_hex(s),
        Value::Array(a) if a.len() >= 3 => {
            let f = |i: usize| a[i].as_f64().map(|x| if x > 1.0 { x / 255.0 } else { x } as f32);
            Some(Color::rgb(f(0)?, f(1)?, f(2)?))
        }
        Value::Object(o) => {
            let g = |k: &str| o.get(k).and_then(Value::as_f64).map(|x| if x > 1.0 { x / 100.0 } else { x } as f32);
            Some(Color::cmyk(g("c")?, g("m")?, g("y")?, g("k")?))
        }
        _ => None,
    }
}

/// The swatch name of a colour value ("C=… M=… Y=… K=…" / "R=… G=… B=…").
fn value_name(color: designcraft_color::Color) -> String {
    match color {
        designcraft_color::Color::Cmyk { c, m, y, k } => designcraft_color::swatch::cmyk_name(c, m, y, k),
        other => {
            let [r, g, b] = other.to_rgb();
            format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round())
        }
    }
}

pub(super) fn load_swatches(s: &mut Session, p: &Value) -> Result<Value> {
    let bytes = match (str_param(p, "path"), str_param(p, "base64")) {
        (_, Some(b)) => super::super::file::base64_decode(b),
        (Some(path), None) => std::fs::read(path).map_err(|e| bad("swatch.load", format!("{path}: {e}")))?,
        _ => return Err(bad("swatch.load", "give `path` or `base64`")),
    };
    let incoming = designcraft_color::ase::read(&bytes).map_err(|e| bad("swatch.load", e.to_string()))?;
    let replace = p.get("replace").and_then(Value::as_bool).unwrap_or(false);
    s.edit(|d, _| {
        let (mut added, mut replaced) = (0, 0);
        for w in incoming {
            match d.swatches.iter_mut().find(|x| x.name == w.name) {
                Some(x) if x.locked => {}
                Some(x) => {
                    if replace {
                        x.value = w.value;
                        x.hidden = false;
                        replaced += 1;
                    }
                }
                None => {
                    d.swatches.push(w);
                    added += 1;
                }
            }
        }
        Ok(json!({"added": added, "replaced": replaced}))
    })
}

pub(super) fn save_swatches(s: &mut Session, p: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let names: Option<Vec<String>> =
        p.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
    let chosen: Vec<Swatch> = d.swatches.iter().filter(|w| names.as_ref().is_none_or(|n| n.contains(&w.name))).cloned().collect();
    let bytes = designcraft_color::ase::write(&chosen);
    match str_param(p, "path") {
        Some(path) => {
            std::fs::write(path, &bytes).map_err(|e| bad("swatch.save", format!("{path}: {e}")))?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }
        None => Ok(json!({"base64": super::super::file::base64_encode(&bytes)})),
    }
}

fn gradient_json(name: &str, g: &designcraft_color::Gradient) -> Value {
    let stops: Vec<Value> = g
        .stops
        .iter()
        .map(|s| json!({"location": (s.offset as f64 * 1000.0).round() / 10.0, "color": s.color.to_hex(), "opacity": (s.opacity * 100.0).round(), "midpoint": (s.midpoint * 100.0).round()}))
        .collect();
    json!({"swatch": name, "kind": if g.kind == designcraft_color::GradientKind::Radial { "radial" } else { "linear" }, "stops": stops})
}

pub(super) fn apply_gradient(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_color::{GradientKind, GradientStop};
    let kind = match str_param(p, "kind") {
        Some("radial") => Some(GradientKind::Radial),
        Some("linear") => Some(GradientKind::Linear),
        Some(k) => return Err(bad("object.gradient", format!("unknown kind {k}"))),
        None => None,
    };
    let stops = match p.get("stops").and_then(Value::as_array) {
        Some(a) => {
            let mut v = Vec::new();
            for st in a {
                let color = st.get("color").and_then(parse_color).ok_or_else(|| bad("object.gradient", "a stop needs a color"))?;
                let pct = |k: &str, d: f64| st.get(k).and_then(Value::as_f64).unwrap_or(d).clamp(0.0, 100.0) as f32 / 100.0;
                v.push(GradientStop {
                    offset: pct("location", 0.0),
                    color,
                    opacity: pct("opacity", 100.0),
                    midpoint: pct("midpoint", 50.0).clamp(0.13, 0.87),
                });
            }
            if v.len() < 2 {
                return Err(bad("object.gradient", "a gradient needs at least two stops"));
            }
            Some(v)
        }
        None => None,
    };
    let reverse = p.get("reverse").and_then(Value::as_bool).unwrap_or(false);
    let angle = p.get("angle").and_then(Value::as_f64);
    let (from, to) = (super::super::point_param(p, "from"), super::super::point_param(p, "to"));
    let ids = super::super::targets(s, p)?;
    s.edit(|d, _| {
        let mut out = Value::Null;
        for id in &ids {
            let Some(it) = d.item(*id) else { continue };
            let cur_name = it.fill.swatch.clone();
            let cur = designcraft_color::swatch::resolve_gradient(&d.swatches, &cur_name).cloned();
            let mut g = cur.clone().unwrap_or_default();
            if let Some(k) = kind {
                g.kind = k;
            }
            if let Some(st) = &stops {
                g.stops = st.clone();
                g.sort();
            }
            if reverse {
                g.reverse();
            }
            // Keep the swatch while the gradient is unchanged, else reuse an equal gradient swatch
            // or add an unnamed one.
            let name = if cur.as_ref() == Some(&g) {
                cur_name
            } else if let Some(w) = d.swatches.iter().find(|w| matches!(&w.value, SwatchValue::Gradient { gradient } if *gradient == g)) {
                w.name.clone()
            } else {
                let name = Styles::unique_name(|n| d.swatch(n).is_some(), "Gradient");
                d.swatches.push(Swatch {
                    name: name.clone(),
                    value: SwatchValue::Gradient { gradient: g.clone() },
                    locked: false,
                    named: false,
                    hidden: true,
                });
                name
            };
            let inv = d.find(*id).map(|l| d.parent_xf(&l)).zip(d.item(*id).map(|i| i.xf)).map(|(pxf, xf)| (pxf * xf).inverse());
            let Some(it) = d.item_mut(*id) else { continue };
            it.fill.swatch = name.clone();
            it.fill.tint = 1.0;
            if let Some(a) = angle {
                it.fill.gradient_angle = Some(a);
                it.fill.gradient_vector = None;
            }
            if let (Some(a), Some(b), Some(inv)) = (from, to, inv) {
                let (a, b) = (inv * a, inv * b);
                if (b - a).hypot() > 1e-6 {
                    it.fill.gradient_vector = Some([a.x, a.y, b.x, b.y]);
                }
            }
            if out.is_null() {
                out = gradient_json(&name, &g);
            }
        }
        Ok(out)
    })
}

pub(super) fn apply_color(s: &mut Session, p: &Value) -> Result<Value> {
    let color = p.get("color").and_then(parse_color).ok_or_else(|| bad("object.color", "missing or bad color"))?;
    let stroke = str_param(p, "target") == Some("stroke");
    let ids = super::super::targets(s, p)?;
    s.edit(|d, _| {
        // Reuse a swatch holding exactly this colour, else add an unnamed one.
        let existing = d
            .swatches
            .iter()
            .find(|w| matches!(&w.value, SwatchValue::Color { color: c, color_type: designcraft_color::ColorType::Process } if *c == color))
            .map(|w| w.name.clone());
        let name = match existing {
            Some(n) => n,
            None => {
                let name = Styles::unique_name(|n| d.swatch(n).is_some(), &value_name(color));
                d.swatches.push(Swatch {
                    name: name.clone(),
                    value: SwatchValue::Color { color, color_type: designcraft_color::ColorType::Process },
                    locked: false,
                    named: false,
                    hidden: true,
                });
                name
            }
        };
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                if stroke {
                    it.stroke.swatch = name.clone();
                    it.stroke.tint = 1.0;
                } else {
                    it.fill.swatch = name.clone();
                    it.fill.tint = 1.0;
                }
            }
        }
        Ok(json!({"swatch": name}))
    })
}

pub(super) fn add_to_swatches(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let name = match str_param(p, "swatch") {
        Some(n) => n.to_string(),
        None => {
            let id = st.selection.items.first().copied().ok_or_else(|| bad("swatch.addToSwatches", "select an object or give `swatch`"))?;
            let it = st.doc.item(id).ok_or_else(|| bad("swatch.addToSwatches", "no such item"))?;
            if str_param(p, "target") == Some("stroke") { it.stroke.swatch.clone() } else { it.fill.swatch.clone() }
        }
    };
    let rename = str_param(p, "name").map(str::to_string);
    s.edit(|d, _| {
        let w = d.swatches.iter_mut().find(|w| w.name == name).ok_or_else(|| bad("swatch.addToSwatches", format!("no swatch `{name}`")))?;
        if w.locked {
            return Err(bad("swatch.addToSwatches", "special swatches are already listed"));
        }
        w.hidden = false;
        let Some(new) = rename.filter(|n| *n != name) else { return Ok(json!({"name": name})) };
        if d.swatches.iter().any(|w| w.name == new) {
            return Err(bad("swatch.addToSwatches", format!("a swatch named `{new}` exists")));
        }
        let w = d.swatches.iter_mut().find(|w| w.name == name).ok_or_else(|| bad("swatch.addToSwatches", format!("no swatch `{name}`")))?;
        w.name = new.clone();
        w.named = true;
        for g in &mut d.color_groups {
            for w in &mut g.swatches {
                if *w == name {
                    *w = new.clone();
                }
            }
        }
        // Everything using the colour follows the rename.
        for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
            let sp = std::sync::Arc::make_mut(sp);
            for top in &mut sp.items {
                rename_in(std::sync::Arc::make_mut(top), &name, &new);
            }
        }
        Ok(json!({"name": new}))
    })
}

/// Point fills and strokes of an item (and its children) at a renamed swatch.
pub(super) fn rename_in(it: &mut designcraft_doc::Item, from: &str, to: &str) {
    if it.fill.swatch == from {
        it.fill.swatch = to.to_string();
    }
    if it.stroke.swatch == from {
        it.stroke.swatch = to.to_string();
    }
    if let Some(kids) = it.children_mut() {
        for k in kids {
            rename_in(std::sync::Arc::make_mut(k), from, to);
        }
    }
}

pub(super) fn swatch_add_unnamed(s: &mut Session, _: &Value) -> Result<Value> {
    s.edit(|d, _| {
        let mut n = 0;
        for w in &mut d.swatches {
            if w.hidden {
                w.hidden = false;
                n += 1;
            }
        }
        Ok(json!({"added": n}))
    })
}

pub(super) fn swatch_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("").to_string();
    s.edit(|d, _| {
        if d.swatch(&name).is_some_and(|w| w.locked) {
            return Err(bad("swatch.delete", "special swatches can't be deleted"));
        }
        d.swatches.retain(|w| w.name != name);
        for g in &mut d.color_groups {
            g.swatches.retain(|w| *w != name);
        }
        ok()
    })
}

pub(super) fn ink_list(s: &mut Session, _: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let inks: Vec<Value> = d
        .swatches
        .iter()
        .filter(|w| matches!(w.value, designcraft_color::swatch::SwatchValue::Color { color_type: designcraft_color::swatch::ColorType::Spot, .. }))
        .map(|w| {
            let alias = d.inks.aliases.iter().find(|(a, _)| *a == w.name).map(|(_, t)| t.clone());
            json!({"name": w.name, "process": d.inks.resolve(&w.name).1, "alias": alias})
        })
        .collect();
    Ok(json!({"allToProcess": d.inks.all_to_process, "inks": inks}))
}

pub(super) fn ink_options(s: &mut Session, p: &Value) -> Result<Value> {
    let all = p.get("allToProcess").and_then(Value::as_bool);
    let ink = str_param(p, "ink").map(str::to_string);
    let to_process = p.get("toProcess").and_then(Value::as_bool);
    let alias = p.get("alias").cloned();
    s.edit(|d, _| {
        if let Some(v) = all {
            d.inks.all_to_process = v;
        }
        if let Some(name) = &ink {
            let is_spot = |n: &str| {
                d.swatch(n).is_some_and(|w| {
                    matches!(w.value, designcraft_color::swatch::SwatchValue::Color { color_type: designcraft_color::swatch::ColorType::Spot, .. })
                })
            };
            if !is_spot(name) {
                return Err(bad("ink.options", format!("`{name}` isn't a spot ink")));
            }
            if let Some(Value::String(to)) = &alias
                && (!is_spot(to) || to == name)
            {
                return Err(bad("ink.options", format!("can't alias to `{to}`")));
            }
            if let Some(v) = to_process {
                d.inks.to_process.retain(|x| x != name);
                if v {
                    d.inks.to_process.push(name.clone());
                }
            }
            match alias {
                Some(Value::String(to)) => {
                    d.inks.aliases.retain(|(a, _)| a != name);
                    d.inks.aliases.push((name.clone(), to));
                }
                Some(Value::Null) => d.inks.aliases.retain(|(a, _)| a != name),
                _ => {}
            }
        }
        ok()
    })
}

pub(super) fn swatch_new_color_group(s: &mut Session, p: &Value) -> Result<Value> {
    let names: Vec<String> =
        p.get("swatches").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let want = str_param(p, "name").map(str::to_string);
    s.edit(|d, _| {
        for n in &names {
            if d.swatch(n).is_none_or(|w| w.locked) {
                return Err(bad("swatch.newColorGroup", format!("`{n}` can't go in a group")));
            }
        }
        let taken = |n: &str| d.color_groups.iter().any(|g| g.name == n);
        let name = match want {
            Some(n) if taken(&n) => return Err(bad("swatch.newColorGroup", format!("a group named `{n}` exists"))),
            Some(n) => n,
            None => (1..=d.color_groups.len() + 1)
                .map(|i| format!("Color Group {i}"))
                .find(|n| !taken(n))
                .ok_or_else(|| bad("swatch.newColorGroup", "no free group name"))?,
        };
        for g in &mut d.color_groups {
            g.swatches.retain(|w| !names.contains(w));
        }
        d.color_groups.push(designcraft_doc::ColorGroup { name: name.clone(), swatches: names });
        Ok(json!({"name": name}))
    })
}

pub(super) fn swatch_move_to_group(s: &mut Session, p: &Value) -> Result<Value> {
    let names: Vec<String> =
        p.get("swatches").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let group = str_param(p, "group").map(str::to_string);
    s.edit(|d, _| {
        if let Some(g) = &group
            && !d.color_groups.iter().any(|x| x.name == *g)
        {
            return Err(bad("swatch.moveToGroup", format!("no color group `{g}`")));
        }
        for g in &mut d.color_groups {
            g.swatches.retain(|w| !names.contains(w));
        }
        if let Some(g) = group.and_then(|g| d.color_groups.iter_mut().find(|x| x.name == g)) {
            g.swatches.extend(names.iter().filter(|n| d.swatches.iter().any(|w| w.name == **n && !w.locked)).cloned());
        }
        ok()
    })
}

pub(super) fn swatch_ungroup_color_group(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("").to_string();
    s.edit(|d, _| {
        let n = d.color_groups.len();
        d.color_groups.retain(|g| g.name != name);
        if d.color_groups.len() == n {
            return Err(bad("swatch.ungroupColorGroup", format!("no color group `{name}`")));
        }
        ok()
    })
}

pub(super) fn swatch_rename_color_group(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, to) = (str_param(p, "name").unwrap_or("").to_string(), str_param(p, "to").unwrap_or("").trim().to_string());
    s.edit(|d, _| {
        if to.is_empty() || d.color_groups.iter().any(|g| g.name == to) {
            return Err(bad("swatch.renameColorGroup", format!("can't rename to `{to}`")));
        }
        let g =
            d.color_groups.iter_mut().find(|g| g.name == name).ok_or_else(|| bad("swatch.renameColorGroup", format!("no color group `{name}`")))?;
        g.name = to;
        ok()
    })
}
