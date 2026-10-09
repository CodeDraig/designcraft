//! Map page items, graphics and object style overrides.

use super::{
    Affine, Arc, Asset, AssetId, Content, Corner, CornerOptions, DropShadow, Effects, El, Fill, Importer, Item, ItemId, OBJECT_BUILTINS, Shape,
    Story, StoryId, Stroke, TextFrame, TextFrameOptions, TextWrap, base64_decode, names, nums, parse_affine, path_of, sniff_image, swatch,
    text_frame_options, tint, unescape_id, uri_to_path,
};

impl Importer<'_> {
    /// Resolve an item attribute, falling back to its object style chain.
    pub(super) fn attr_or_style(&self, e: &El, k: &str) -> Option<String> {
        if let Some(v) = e.get(k) {
            return Some(v.to_string());
        }
        let mut os = e.get("AppliedObjectStyle").map(str::to_string);
        let mut depth = 0;
        while let Some(id) = os {
            depth += 1;
            if depth > 16 {
                break;
            }
            let Some(s) = self.object_els.get(&id) else { break };
            if let Some(v) = s.get(k) {
                return Some(v.to_string());
            }
            os = s.prop("BasedOn").map(|b| if b.starts_with("ObjectStyle/") { b } else { format!("ObjectStyle/{}", names::escape_id(&b)) });
        }
        None
    }

    pub(super) fn stroke_from(&mut self, e: &El, item: Option<&El>) -> Stroke {
        let g = |s: &Self, k: &str| -> Option<String> { if item.is_some() { s.attr_or_style(e, k) } else { e.get(k).map(str::to_string) } };
        let color = g(self, "StrokeColor");
        let mut s = Stroke::none();
        if let Some(c) = color {
            s.swatch = self.swatch_ref(&c);
        }
        if let Some(w) = g(self, "StrokeWeight").and_then(|v| v.parse().ok()) {
            s.weight = w;
        }
        if let Some(t) = g(self, "StrokeTint").and_then(|v| v.parse().ok()) {
            s.tint = tint(Some(t)).unwrap_or(1.0);
        }
        if let Some(t) = g(self, "StrokeType") {
            s.kind = names::stroke_type_in(&t);
        }
        if let Some(v) = g(self, "StrokeAlignment") {
            s.align = names::stroke_align_in(&v);
        }
        if let Some(v) = g(self, "EndCap") {
            s.cap = names::cap_in(&v);
        }
        if let Some(v) = g(self, "EndJoin") {
            s.join = names::join_in(&v);
        }
        if let Some(v) = g(self, "MiterLimit").and_then(|v| v.parse().ok()) {
            s.miter_limit = v;
        }
        if let Some(v) = g(self, "LeftLineEnd") {
            s.start = names::arrow_in(&v);
        }
        if let Some(v) = g(self, "RightLineEnd") {
            s.end = names::arrow_in(&v);
        }
        if let Some(v) = g(self, "GapColor") {
            s.gap_swatch = self.swatch_ref(&v);
        }
        if let Some(t) = g(self, "GapTint").and_then(|v| v.parse().ok()) {
            s.gap_tint = tint(Some(t)).unwrap_or(1.0);
        }
        s.overprint = g(self, "OverprintStroke").as_deref() == Some("true");
        s.gap_overprint = g(self, "OverprintGap").as_deref() == Some("true");
        s
    }

    pub(super) fn item(&mut self, e: &El, parent: Affine) -> Option<Item> {
        let tag = e.local();
        let shape = match tag {
            "Rectangle" | "TextFrame" => Shape::Rectangle,
            "Oval" => Shape::Oval,
            "Polygon" => Shape::Polygon,
            "GraphicLine" => Shape::GraphicLine,
            "Group" => Shape::Group,
            _ => return None,
        };
        let id = ItemId(self.alloc());
        if let Some(s) = e.get("Self") {
            self.item_ids.insert(s.to_string(), id);
        }
        let layer = e.get("ItemLayer").and_then(|l| self.layer_ids.get(l)).copied().unwrap_or_else(|| self.layers[0].id);
        let path = e.prop_el("PathGeometry").map(path_of).unwrap_or_default();
        let mut it = Item::new(id, layer, shape, path);
        it.xf = parent * parse_affine(e.get("ItemTransform"));
        it.name = e.get("Name").filter(|n| *n != "$ID/").unwrap_or("").to_string();
        it.locked = e.get("Locked") == Some("true");
        it.hidden = e.get("Visible") == Some("false");
        it.nonprinting = e.get("Nonprinting") == Some("true");
        it.object_style = e
            .get("AppliedObjectStyle")
            .map(|r| {
                self.object_names
                    .get(r)
                    .cloned()
                    .unwrap_or_else(|| names::style_name_in(OBJECT_BUILTINS, &unescape_id(r.trim_start_matches("ObjectStyle/"))))
            })
            .unwrap_or_else(|| designcraft_doc::NO_OBJECT_STYLE.into());
        // Fill and stroke.
        if tag != "Group" {
            if let Some(f) = self.attr_or_style(e, "FillColor") {
                it.fill.swatch = self.swatch_ref(&f);
            }
            if let Some(t) = self.attr_or_style(e, "FillTint").and_then(|v| v.parse().ok()) {
                it.fill.tint = tint(Some(t)).unwrap_or(1.0);
            }
            it.fill.overprint = self.attr_or_style(e, "OverprintFill").as_deref() == Some("true");
            if let Some(a) = e.num("GradientFillAngle")
                && a != 0.0
            {
                it.fill.gradient_angle = Some(a);
            }
            let start: Option<Vec<f64>> = e.prop("GradientFillStart").map(|s| s.split_whitespace().filter_map(|v| v.parse().ok()).collect());
            if let (Some([x, y]), Some(len)) = (start.as_deref().and_then(|v| <[f64; 2]>::try_from(v).ok()), e.num("GradientFillLength"))
                && len > 0.0
            {
                let a = e.num("GradientFillAngle").unwrap_or(0.0).to_radians();
                it.fill.gradient_vector = Some([x, y, x + len * a.cos(), y - len * a.sin()]);
            }
            it.stroke = self.stroke_from(e, Some(e));
            if it.stroke.weight <= 0.0 {
                it.stroke.swatch = swatch::NONE.into();
                it.stroke.weight = 1.0;
            }
        }
        // Corners.
        let cn = crate::export::corner_names(&it.path);
        let mut corners = CornerOptions::default();
        for (i, n) in cn.iter().enumerate() {
            let shape = e.get(&format!("{n}CornerOption")).map(names::corner_in).unwrap_or_default();
            let size = e.num(&format!("{n}CornerRadius")).unwrap_or(0.0);
            corners.corners[i] = Corner { shape, size };
        }
        if !corners.is_none() {
            it.corners = corners;
        }
        // Transparency.
        if let Some(t) = e.find("TransparencySetting") {
            if let Some(bs) = t.find("BlendingSetting") {
                if let Some(o) = bs.num("Opacity") {
                    it.opacity = (o / 100.0).clamp(0.0, 1.0) as f32;
                }
                if let Some(m) = bs.get("BlendMode") {
                    it.blend = names::blend_in(m);
                }
            }
            if let Some(ds) = t.find("DropShadowSetting")
                && ds.get("Mode") == Some("Drop")
            {
                let x = ds.num("XOffset").unwrap_or(4.95);
                let y = ds.num("YOffset").unwrap_or(4.95);
                let def = DropShadow::default();
                it.effects.drop_shadow = DropShadow {
                    on: true,
                    color: ds.get("EffectColor").map(|c| self.swatch_ref(c)).unwrap_or(def.color),
                    opacity: ds.num("Opacity").map(|o| (o / 100.0) as f32).unwrap_or(def.opacity),
                    angle: names_angle(x, y),
                    distance: x.hypot(y),
                    size: ds.num("Size").unwrap_or(def.size),
                    spread: ds.num("Spread").unwrap_or(def.spread),
                    global_light: false,
                };
            }
            if let Some(f) = t.find("FeatherSetting")
                && f.get("Mode").is_some_and(|m| m != "None")
            {
                it.effects = Effects { feather: f.num("Width").unwrap_or(9.0), ..it.effects.clone() };
            }
        }
        // Text wrap.
        if let Some(w) = e.find("TextWrapPreference") {
            let mut tw = TextWrap {
                mode: names::wrap_mode_in(w.get("TextWrapMode").unwrap_or("None")),
                invert: w.get("Inverse") == Some("true"),
                side: names::wrap_side_in(w.get("TextWrapSide").unwrap_or("BothSides")),
                ..TextWrap::default()
            };
            if let Some(o) = w.prop_el("TextWrapOffset") {
                tw.offsets =
                    [o.num("Top").unwrap_or(0.0), o.num("Left").unwrap_or(0.0), o.num("Bottom").unwrap_or(0.0), o.num("Right").unwrap_or(0.0)];
            }
            it.wrap = tw;
        }
        // Type on a path.
        if tag != "TextFrame"
            && let Some(tp) = e.find("TextPath")
        {
            let story_self = tp.get("ParentStory").unwrap_or("n").to_string();
            if let Some(story) = self.story_ids.get(&story_self).copied() {
                let options = TextFrameOptions {
                    path: Some(designcraft_doc::PathType {
                        start: tp.num("StartBracket").unwrap_or(0.0).max(0.0),
                        flip: tp.get("FlipPathEffect") == Some("Flipped"),
                        align: names::path_align_in(tp.get("TextAlignment").unwrap_or("")),
                    }),
                    ..Default::default()
                };
                it.content = Content::Text(TextFrame { story, options });
                self.ctx.threads.push((id, story_self, "n".into(), "n".into()));
            }
        }
        // Content.
        match tag {
            "TextFrame" => {
                let story_self = e.get("ParentStory").unwrap_or("n").to_string();
                let story = match self.story_ids.get(&story_self) {
                    Some(s) => *s,
                    None => {
                        let sid = StoryId(self.alloc());
                        self.stories.insert(sid, Story::new(sid));
                        self.story_ids.insert(format!("#orphan{}", sid.0), sid);
                        sid
                    }
                };
                let mut options = e.find("TextFramePreference").map(text_frame_options).unwrap_or_default();
                if let Some(g) = e.find("BaselineFrameGridOption")
                    && g.get("UseCustomBaselineFrameGrid") == Some("true")
                {
                    options.baseline_grid =
                        Some((g.num("StartingOffsetForBaselineFrameGrid").unwrap_or(0.0), g.num("BaselineFrameGridIncrement").unwrap_or(12.0)));
                }
                it.content = Content::Text(TextFrame { story, options });
                self.ctx.threads.push((
                    id,
                    story_self,
                    e.get("PreviousTextFrame").unwrap_or("n").to_string(),
                    e.get("NextTextFrame").unwrap_or("n").to_string(),
                ));
            }
            "Group" => {
                let mut kids = Vec::new();
                for c in e.elements() {
                    if let Some(k) = self.item(c, Affine::IDENTITY) {
                        kids.push(Arc::new(k));
                    }
                }
                it.content = Content::Group { items: kids };
                it.fill = Fill::none();
                it.stroke = Stroke::none();
            }
            _ => {
                if let Some(g) = e.elements().find(|c| matches!(c.local(), "Image" | "PDF" | "EPS" | "ImportedPage" | "WMF" | "PICT" | "SVG")) {
                    it.content = self.graphic(g);
                    if let (Some(ff), Content::Graphic(gr)) = (e.elements().find(|c| c.local() == "FrameFittingOption"), &mut it.content) {
                        gr.crop = [ff.num("TopCrop"), ff.num("LeftCrop"), ff.num("BottomCrop"), ff.num("RightCrop")].map(|v| v.unwrap_or(0.0));
                        gr.fit_align = ff.get("FittingAlignment").map_or(4, names::anchor_in);
                        if ff.get("AutoFit") == Some("true") {
                            gr.auto_fit = ff.get("FittingOnEmptyFrame").map_or(designcraft_doc::Fitting::FillProportionally, names::fitting_in);
                            if gr.auto_fit == designcraft_doc::Fitting::None {
                                gr.auto_fit = designcraft_doc::Fitting::FillProportionally;
                            }
                        }
                    }
                } else {
                    // Page items pasted into this frame (Paste Into).
                    let kids: Vec<Arc<Item>> = e
                        .elements()
                        .filter(|c| matches!(c.local(), "Rectangle" | "Oval" | "Polygon" | "GraphicLine" | "Group" | "TextFrame"))
                        .filter_map(|c| self.item(c, Affine::IDENTITY))
                        .map(Arc::new)
                        .collect();
                    if !kids.is_empty() {
                        it.content = Content::Group { items: kids };
                    }
                }
            }
        }
        Some(it)
    }

    pub(super) fn graphic(&mut self, g: &El) -> Content {
        let gxf = parse_affine(g.get("ItemTransform"));
        let (l, t, r, b) = match g.prop_el("GraphicBounds") {
            Some(gb) => {
                (gb.num("Left").unwrap_or(0.0), gb.num("Top").unwrap_or(0.0), gb.num("Right").unwrap_or(0.0), gb.num("Bottom").unwrap_or(0.0))
            }
            None => (0.0, 0.0, 0.0, 0.0),
        };
        let size = ((r - l).abs(), (b - t).abs());
        let link = g.find("Link");
        let uri = link.and_then(|k| k.get("LinkResourceURI")).map(uri_to_path);
        let mut data = g.prop_el("Contents").map(|c| base64_decode(&c.text_content())).unwrap_or_default();
        if data.is_empty()
            && let Some(p) = &uri
            && let Some(d) = (self.read_link)(p)
        {
            data = d;
        }
        let (mime, px) = sniff_image(&data);
        let mime = mime.map(str::to_string).unwrap_or_else(|| match link.and_then(|k| k.get("LinkResourceFormat")).unwrap_or("") {
            f if f.contains("JPEG") => "image/jpeg".into(),
            f if f.contains("TIFF") => "image/tiff".into(),
            f if f.contains("PDF") || g.local() == "PDF" => "application/pdf".into(),
            f if f.contains("GIF") => "image/gif".into(),
            _ => "image/png".into(),
        });
        let pixels = px.or_else(|| {
            let ppi = nums(g.get("ActualPpi").unwrap_or(""));
            (ppi.len() == 2 && size.0 > 0.0).then(|| ((size.0 * ppi[0] / 72.0).round() as u32, (size.1 * ppi[1] / 72.0).round() as u32))
        });
        let name =
            uri.as_deref().and_then(|p| p.rsplit(['/', '\\']).next()).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| "image".into());
        let id = AssetId(self.alloc());
        let link_path = uri.filter(|p| p.contains('/') || p.contains('\\'));
        self.assets.insert(id, Arc::new(Asset { page: 0, id, name, mime, link: link_path, data: Arc::new(data), pixels }));
        Content::Graphic(designcraft_doc::Graphic {
            asset: id,
            size,
            xf: gxf * Affine::translate((l, t)),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
        })
    }
}

/// Drop-shadow angle (degrees) from IDML offsets (inverse of export's mapping).
fn names_angle(x: f64, y: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 135.0;
    }
    let a = y.atan2(-x).to_degrees();
    if a < 0.0 { a + 360.0 } else { a }
}
