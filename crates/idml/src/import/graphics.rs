//! Resolve named and unnamed IDML colors, gradients and inks.

use super::{Color, ColorType, El, Gradient, GradientKind, GradientStop, HashSet, Importer, Styles, Swatch, SwatchValue, lab, names, nums, swatch};

impl Importer<'_> {
    pub(super) fn color_of(e: &El) -> Option<(Color, ColorType)> {
        let v = nums(e.get("ColorValue").unwrap_or(""));
        let space = e.get("Space").unwrap_or("CMYK");
        let c = match space {
            "RGB" if v.len() >= 3 => Color::rgb((v[0] / 255.0) as f32, (v[1] / 255.0) as f32, (v[2] / 255.0) as f32),
            "LAB" if v.len() >= 3 => lab_to_color(v[0] as f32, v[1] as f32, v[2] as f32),
            _ if v.len() >= 4 => Color::cmyk((v[0] / 100.0) as f32, (v[1] / 100.0) as f32, (v[2] / 100.0) as f32, (v[3] / 100.0) as f32),
            _ => return None,
        };
        let ty = if e.get("Model") == Some("Spot") { ColorType::Spot } else { ColorType::Process };
        Some((c, ty))
    }

    pub(super) fn graphics(&mut self, top: &[El]) {
        let mut order: Vec<(String, Swatch)> = Vec::new();
        // Colours.
        for e in top.iter().filter(|e| e.local() == "Color") {
            let Some(id) = e.get("Self") else { continue };
            let Some((c, ty)) = Self::color_of(e) else { continue };
            match id {
                "Color/Paper" => {
                    if let Some(s) = self.swatches.iter_mut().find(|s| s.name == swatch::PAPER) {
                        s.value = SwatchValue::Paper { color: if c == Color::cmyk(0.0, 0.0, 0.0, 0.0) { Color::WHITE } else { c } };
                    }
                    continue;
                }
                "Color/Black" | "Color/Registration" => continue,
                _ => {}
            }
            let name = e.get("Name").unwrap_or("");
            if e.get("Visible") == Some("false") || name.is_empty() || name == "$ID/" || e.get("Model") == Some("Registration") {
                self.hidden_colors.insert(id.to_string(), c);
                continue;
            }
            let named = !name.starts_with("C=") && !name.starts_with("R=");
            self.swatch_names.insert(id.to_string(), name.to_string());
            order.push((
                id.to_string(),
                Swatch { name: name.to_string(), value: SwatchValue::Color { color: c, color_type: ty }, locked: false, named, hidden: false },
            ));
        }
        // Tints (after colours: they reference them).
        for e in top.iter().filter(|e| e.local() == "Tint") {
            let Some(id) = e.get("Self") else { continue };
            let base = e.get("BaseColor").map(|b| self.swatch_ref(b)).unwrap_or_else(|| swatch::BLACK.to_string());
            let t = e.num("TintValue").unwrap_or(100.0);
            let name = match e.get("Name") {
                Some(n) if !n.is_empty() && n != "$ID/" => n.to_string(),
                _ => format!("{base} {}%", names::num(t)),
            };
            self.swatch_names.insert(id.to_string(), name.clone());
            order.push((
                id.to_string(),
                Swatch { name, value: SwatchValue::Tint { base, tint: (t / 100.0) as f32 }, locked: false, named: true, hidden: false },
            ));
        }
        // Custom stroke styles.
        for e in top.iter().filter(|e| matches!(e.local(), "StripedStrokeStyle" | "DashedStrokeStyle" | "DottedStrokeStyle")) {
            let (Some(id), Some(name)) = (e.get("Self"), e.get("Name")) else { continue };
            let nums = |k: &str| e.get(k).unwrap_or("").split_whitespace().filter_map(|x| x.parse::<f64>().ok()).collect::<Vec<_>>();
            let kind = match e.local() {
                "StripedStrokeStyle" => designcraft_doc::StrokeType::Stripes {
                    bands: nums("StripeArray").as_chunks::<2>().0.iter().map(|c| (c[0] / 100.0, (c[1] - c[0]) / 100.0)).collect(),
                },
                "DashedStrokeStyle" => designcraft_doc::StrokeType::Dashed { pattern: nums("DashArray") },
                _ => designcraft_doc::StrokeType::Dotted,
            };
            let _ = id;
            self.stroke_styles.push(designcraft_doc::StrokeStyleDef { name: name.to_string(), kind });
        }
        // Ink Manager.
        for e in top.iter().filter(|e| e.local() == "Ink") {
            let Some(name) = e
                .get("Name")
                .filter(|n| !n.starts_with("$ID/") && !["Process Cyan", "Process Magenta", "Process Yellow", "Process Black"].contains(n))
            else {
                continue;
            };
            if e.get("ConvertToProcess") == Some("true") {
                self.inks.to_process.push(name.to_string());
            }
            if let Some(a) = e.get("AliasInkName").filter(|a| !a.is_empty() && *a != name && !a.starts_with("$ID/")) {
                self.inks.aliases.push((name.to_string(), a.to_string()));
            }
        }
        for e in top.iter().filter(|e| e.local() == "Gradient") {
            let Some(id) = e.get("Self") else { continue };
            let mut stops: Vec<GradientStop> = Vec::new();
            for s in e.find_all("GradientStop") {
                let c = s.get("StopColor").map(|r| self.color_for_ref(r, &order)).unwrap_or(Color::BLACK);
                let loc = s.num("Location").unwrap_or(0.0) / 100.0;
                if let (Some(prev), Some(m)) = (stops.last_mut(), s.num("Midpoint")) {
                    prev.midpoint = (m / 100.0) as f32;
                }
                stops.push(GradientStop { offset: loc as f32, color: c, opacity: 1.0, midpoint: 0.5 });
            }
            let kind = if e.get("Type") == Some("Radial") { GradientKind::Radial } else { GradientKind::Linear };
            let unnamed = e.get("Visible") == Some("false") || e.get("Name").is_none_or(|n| n.is_empty() || n == "$ID/");
            let name = if unnamed { "Gradient".to_string() } else { e.get("Name").unwrap_or_default().to_string() };
            let sw =
                Swatch { name, value: SwatchValue::Gradient { gradient: Gradient { kind, stops } }, locked: false, named: !unnamed, hidden: false };
            if unnamed {
                // Unnamed gradients become swatches only when something uses them.
                self.hidden_swatches.insert(id.to_string(), sw);
            } else {
                self.swatch_names.insert(id.to_string(), sw.name.clone());
                order.push((id.to_string(), sw));
            }
        }
        // Panel order from the root colour group.
        let group_order: Vec<String> = top
            .iter()
            .filter(|e| e.local() == "ColorGroup")
            .flat_map(|g| g.find_all("ColorGroupSwatch").filter_map(|s| s.get("SwatchItemRef").map(str::to_string)).collect::<Vec<_>>())
            .collect();
        if !group_order.is_empty() {
            order.sort_by_key(|(id, _)| group_order.iter().position(|g| g == id).unwrap_or(usize::MAX));
        }
        let mut names: HashSet<String> = self.swatches.iter().map(|s| s.name.clone()).collect();
        for (id, mut s) in order {
            if names.contains(&s.name) {
                let base = s.name.clone();
                s.name = Styles::unique_name(|n| names.contains(n), &base);
                self.swatch_names.insert(id, s.name.clone());
            }
            names.insert(s.name.clone());
            self.swatches.push(s);
        }
        // Colour groups other than the root one become Swatches panel folders.
        for g in top.iter().filter(|e| e.local() == "ColorGroup" && e.get("IsRootColorGroup") != Some("true")) {
            let swatches: Vec<String> =
                g.find_all("ColorGroupSwatch").filter_map(|s| s.get("SwatchItemRef")).filter_map(|r| self.swatch_names.get(r).cloned()).collect();
            self.color_groups.push(designcraft_doc::ColorGroup { name: g.get("Name").unwrap_or("Color Group").to_string(), swatches });
        }
    }

    pub(super) fn color_for_ref(&self, r: &str, pending: &[(String, Swatch)]) -> Color {
        if let Some(c) = self.hidden_colors.get(r) {
            return *c;
        }
        match r {
            "Color/Black" => return Color::cmyk(0.0, 0.0, 0.0, 1.0),
            "Color/Paper" => return Color::WHITE,
            "Color/Registration" => return Color::cmyk(1.0, 1.0, 1.0, 1.0),
            _ => {}
        }
        if let Some((_, s)) = pending.iter().find(|(id, _)| id == r) {
            match &s.value {
                SwatchValue::Color { color, .. } => return *color,
                SwatchValue::Tint { base, tint } => {
                    let mut all: Vec<Swatch> = self.swatches.clone();
                    all.extend(pending.iter().map(|(_, s)| s.clone()));
                    return designcraft_color::swatch::resolve(&all, base, *tint).unwrap_or(Color::BLACK);
                }
                _ => {}
            }
        }
        Color::BLACK
    }

    /// A swatch reference (Self id) → our swatch name; unnamed colours become value-named swatches.
    pub(super) fn swatch_ref(&mut self, r: &str) -> String {
        if let Some(n) = self.swatch_names.get(r) {
            return n.clone();
        }
        if r == "n" || r.is_empty() {
            return swatch::NONE.into();
        }
        if let Some(mut sw) = self.hidden_swatches.remove(r) {
            sw.name = Styles::unique_name(|n| self.swatches.iter().any(|s| s.name == n), &sw.name);
            self.swatch_names.insert(r.to_string(), sw.name.clone());
            let name = sw.name.clone();
            self.swatches.push(sw);
            return name;
        }
        if let Some(c) = self.hidden_colors.get(r).copied() {
            let name = match c {
                Color::Cmyk { c, m, y, k } => designcraft_color::swatch::cmyk_name(c, m, y, k),
                Color::Rgb { r, g, b } => format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round()),
                Color::Gray { k } => format!("K={}", (k * 100.0).round()),
            };
            if !self.swatches.iter().any(|s| s.name == name) {
                self.swatches.push(Swatch {
                    name: name.clone(),
                    value: SwatchValue::Color { color: c, color_type: ColorType::Process },
                    locked: false,
                    named: false,
                    hidden: false,
                });
            }
            self.swatch_names.insert(r.to_string(), name.clone());
            return name;
        }
        swatch::NONE.into()
    }
}

fn lab_to_color(l: f32, a: f32, b: f32) -> Color {
    let rgb = lab::xyz_to_srgb(lab::lab_to_xyz(lab::Lab::new(l, a, b)));
    Color::rgb(rgb[0], rgb[1], rgb[2])
}
