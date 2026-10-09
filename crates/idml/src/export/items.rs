//! Write page-item geometry, object appearance and placed graphics.

use super::{
    Affine, ColumnsKind, Content, El, Ex, Item, Node, OBJECT_BUILTINS, PathData, Shape, TextFrameOptions, affine_s, base64_encode, bool_s,
    corner_names, names, num, p, path_to_uri, pct, pt, uid, with_props,
};

impl Ex<'_> {
    pub(super) fn item_el(&mut self, it: &Item, xf: Affine) -> El {
        let d = self.d;
        let path_text = matches!(&it.content, Content::Text(t) if t.options.path.is_some());
        let tag = match (&it.content, it.shape) {
            (Content::Text(_), _) if path_text => match it.shape {
                Shape::GraphicLine => "GraphicLine",
                Shape::Oval => "Oval",
                Shape::Rectangle => "Rectangle",
                _ => "Polygon",
            },
            (Content::Text(_), _) => "TextFrame",
            (Content::Group { .. }, Shape::Group) => "Group",
            (_, Shape::Oval) => "Oval",
            (_, Shape::Polygon) | (_, Shape::Path) => "Polygon",
            (_, Shape::GraphicLine) => "GraphicLine",
            _ => "Rectangle",
        };
        let mut el = El::new(tag).attr("Self", uid(it.id.0));
        if let Content::Text(t) = &it.content
            && let Some(pt) = &t.options.path
        {
            // Type on a path: a TextPath child holds the story.
            let len = designcraft_geom::warp::PathWarp::new(&it.path.to_bezpath(), false).length();
            el.push(
                El::new("TextPath")
                    .attr("Self", format!("{}tp", uid(it.id.0)))
                    .attr("ParentStory", uid(t.story.0))
                    .attr("PreviousTextFrame", "n")
                    .attr("NextTextFrame", "n")
                    .attr("PathEffect", "RainbowPathEffect")
                    .attr("PathAlignment", "CenterPathAlignment")
                    .attr("TextAlignment", names::path_align_out(pt.align))
                    .attr("FlipPathEffect", if pt.flip { "Flipped" } else { "NotFlipped" })
                    .attr("StartBracket", num(pt.start))
                    .attr("EndBracket", num(len)),
            );
        } else if let Content::Text(t) = &it.content {
            let (prev, next) = self.threads.get(&it.id.0).copied().unwrap_or((None, None));
            el.set("ParentStory", uid(t.story.0));
            el.set("PreviousTextFrame", prev.map(uid).unwrap_or_else(|| "n".into()));
            el.set("NextTextFrame", next.map(uid).unwrap_or_else(|| "n".into()));
        }
        if tag != "Group" {
            el.set(
                "ContentType",
                match it.content {
                    Content::Text(_) => "TextType",
                    Content::Graphic(_) => "GraphicType",
                    _ => "Unassigned",
                },
            );
        }
        el.set("Name", if it.name.is_empty() { "$ID/".to_string() } else { it.name.clone() });
        el.set("ItemLayer", uid(it.layer.0));
        el.set("Visible", bool_s(!it.hidden));
        el.set("Locked", bool_s(it.locked));
        el.set("Nonprinting", bool_s(it.nonprinting));
        let os = if it.object_style.is_empty() { designcraft_doc::NO_OBJECT_STYLE } else { &it.object_style };
        el.set("AppliedObjectStyle", names::style_self("ObjectStyle", OBJECT_BUILTINS, os));
        el.set("ItemTransform", affine_s(xf));
        el.set("FillColor", self.sw(&it.fill.swatch));
        el.set("FillTint", pct(it.fill.tint as f64));
        el.set("OverprintFill", bool_s(it.fill.overprint));
        if let Some([x0, y0, x1, y1]) = it.fill.gradient_vector {
            // Gradient Swatch tool vector: start, length and angle in the item's space.
            el.set("GradientFillStart", format!("{} {}", num(x0), num(y0)));
            el.set("GradientFillLength", num((x1 - x0).hypot(y1 - y0)));
            el.set("GradientFillAngle", num((-(y1 - y0)).atan2(x1 - x0).to_degrees()));
        } else if let Some(a) = it.fill.gradient_angle {
            el.set("GradientFillAngle", num(a));
        }
        self.stroke_attrs(&mut el, &it.stroke);
        // Corner options: map path-order corners to the named corners by geometry.
        if !it.corners.is_none() {
            let names_by_index = corner_names(&it.path);
            for (i, c) in it.corners.corners.iter().enumerate() {
                let n = names_by_index[i];
                el.set(&format!("{n}CornerOption"), names::corner_out(c.shape));
                el.set(&format!("{n}CornerRadius"), num(c.size));
            }
        }
        let mut props = Vec::new();
        if tag != "Group" && !it.path.is_empty() {
            props.push(path_geometry(&it.path));
        }
        let mut el = with_props(el, props);
        // Transparency.
        let ds = &it.effects.drop_shadow;
        if it.opacity < 1.0 || it.blend != designcraft_color::BlendMode::Normal || ds.on || it.effects.feather > 0.0 {
            let mut t = El::new("TransparencySetting");
            t.push(El::new("BlendingSetting").attr("BlendMode", names::blend_out(it.blend)).attr("Opacity", pct(it.opacity as f64)));
            if ds.on {
                let a = self.d.light_angle(ds.angle, ds.global_light).to_radians();
                t.push(
                    El::new("DropShadowSetting")
                        .attr("Mode", "Drop")
                        .attr("XOffset", num(-ds.distance * a.cos()))
                        .attr("YOffset", num(ds.distance * a.sin()))
                        .attr("Size", num(ds.size))
                        .attr("Spread", num(ds.spread))
                        .attr("Opacity", pct(ds.opacity as f64))
                        .attr("EffectColor", self.sw(&ds.color)),
                );
            }
            if it.effects.feather > 0.0 {
                t.push(El::new("FeatherSetting").attr("Mode", "Standard").attr("Width", num(it.effects.feather)));
            }
            el.push(t);
        }
        if let Content::Text(t) = &it.content {
            el.push(text_frame_pref(&t.options));
            if let Some((start, inc)) = t.options.baseline_grid {
                el.push(
                    El::new("BaselineFrameGridOption")
                        .attr("UseCustomBaselineFrameGrid", "true")
                        .attr("StartingOffsetForBaselineFrameGrid", num(start))
                        .attr("BaselineFrameGridIncrement", num(inc)),
                );
            }
        }
        let w = &it.wrap;
        el.push(with_props(
            El::new("TextWrapPreference")
                .attr("Inverse", bool_s(w.invert))
                .attr("ApplyToMasterPageOnly", "false")
                .attr("TextWrapSide", names::wrap_side_out(w.side))
                .attr("TextWrapMode", names::wrap_mode_out(w.mode)),
            vec![
                El::new("TextWrapOffset")
                    .attr("Top", num(w.offsets[0]))
                    .attr("Left", num(w.offsets[1]))
                    .attr("Bottom", num(w.offsets[2]))
                    .attr("Right", num(w.offsets[3])),
            ],
        ));
        match &it.content {
            Content::Group { items } => {
                for c in items {
                    let e = self.item_el(c, c.xf);
                    el.push(e);
                }
            }
            Content::Graphic(g) => {
                if g.auto_fit != designcraft_doc::Fitting::None || g.fit_align != 4 || g.crop != [0.0; 4] {
                    el.push(
                        El::new("FrameFittingOption")
                            .attr("AutoFit", bool_s(g.auto_fit != designcraft_doc::Fitting::None))
                            .attr("TopCrop", num(g.crop[0]))
                            .attr("LeftCrop", num(g.crop[1]))
                            .attr("BottomCrop", num(g.crop[2]))
                            .attr("RightCrop", num(g.crop[3]))
                            .attr("FittingOnEmptyFrame", names::fitting_out(g.auto_fit))
                            .attr("FittingAlignment", names::anchor_out(g.fit_align)),
                    );
                }
                if let Some(asset) = d.assets.get(&g.asset) {
                    let e = self.image_el(asset, g);
                    el.push(e);
                }
            }
            _ => {}
        }
        el
    }

    pub(super) fn image_el(&mut self, asset: &designcraft_doc::Asset, g: &designcraft_doc::Graphic) -> El {
        let is_pdf = asset.mime == "application/pdf";
        let mut el = El::new(if is_pdf { "PDF" } else { "Image" }).attr("Self", self.fresh());
        if let Some((pw, ph)) = asset.pixels
            && g.size.0 > 0.0
            && g.size.1 > 0.0
        {
            el.set("ActualPpi", pt(pw as f64 * 72.0 / g.size.0, ph as f64 * 72.0 / g.size.1));
        }
        el.set("ItemTransform", affine_s(g.xf));
        let embed = !asset.data.is_empty() && (self.opts.embed_images || asset.link.is_none());
        let mut props = Vec::new();
        if embed {
            let mut c = El::new("Contents");
            c.children.push(Node::CData(base64_encode(&asset.data)));
            props.push(c);
        }
        props.push(El::new("GraphicBounds").attr("Left", "0").attr("Top", "0").attr("Right", num(g.size.0)).attr("Bottom", num(g.size.1)));
        let mut el = with_props(el, props);
        let uri = match &asset.link {
            Some(l) => path_to_uri(l),
            None => format!("file:{}", asset.name),
        };
        el.push(
            El::new("Link")
                .attr("Self", self.fresh())
                .attr("AssetURL", "$ID/")
                .attr("AssetID", "$ID/")
                .attr("LinkResourceURI", uri)
                .attr("LinkResourceFormat", format!("$ID/{}", format_name(&asset.mime)))
                .attr("StoredState", if embed { "Embedded" } else { "Normal" })
                .attr("LinkClassID", "35906")
                .attr("LinkClientID", "257")
                .attr("LinkResourceModified", "false")
                .attr("LinkObjectModified", "false")
                .attr("ShowInUI", "true")
                .attr("CanEmbed", "true")
                .attr("CanUnembed", "true")
                .attr("CanPackage", "true")
                .attr("ImportPolicy", "NoAutoImport")
                .attr("ExportPolicy", "NoAutoExport"),
        );
        el
    }
}

pub(super) fn text_frame_pref(o: &TextFrameOptions) -> El {
    let mut inset = El::new("InsetSpacing").attr("type", "list");
    for v in o.inset {
        inset.push(p("ListItem", "unit", num(v)));
    }
    let el = El::new("TextFramePreference")
        .attr("TextColumnCount", o.columns.max(1))
        .attr("TextColumnGutter", num(o.gutter))
        .attr("UseFixedColumnWidth", bool_s(o.columns_kind == ColumnsKind::FixedWidth))
        .attr("UseFlexibleColumnWidth", bool_s(o.columns_kind == ColumnsKind::FlexibleWidth))
        .attr("VerticalBalanceColumns", bool_s(o.balance_columns))
        .attr("FirstBaselineOffset", names::first_baseline_out(o.first_baseline))
        .attr("MinimumFirstBaselineOffset", num(o.first_baseline_min))
        .attr("VerticalJustification", names::vj_out(o.vertical_justification))
        .attr("VerticalThreshold", num(o.vj_paragraph_spacing_limit))
        .attr("IgnoreWrap", bool_s(o.ignore_wrap))
        .attr("AutoSizingType", names::auto_size_out(o.auto_size))
        .attr("AutoSizingReferencePoint", names::REF_POINTS[(o.auto_size_ref as usize).min(8)]);
    let el = if o.column_width > 0.0 { el.attr("TextColumnFixedWidth", num(o.column_width)) } else { el };
    with_props(el, vec![inset])
}

fn path_geometry(path: &PathData) -> El {
    let mut pg = El::new("PathGeometry");
    for sp in &path.subpaths {
        let mut arr = El::new("PathPointArray");
        for a in &sp.anchors {
            arr.push(
                El::new("PathPointType")
                    .attr("Anchor", pt(a.p.x, a.p.y))
                    .attr("LeftDirection", pt(a.h_in.x, a.h_in.y))
                    .attr("RightDirection", pt(a.h_out.x, a.h_out.y)),
            );
        }
        pg.push(El::new("GeometryPathType").attr("PathOpen", bool_s(!sp.closed)).child(arr));
    }
    pg
}

fn format_name(mime: &str) -> &'static str {
    match mime {
        "image/png" => "Portable Network Graphics (PNG)",
        "image/jpeg" => "JPEG",
        "image/gif" => "GIF",
        "image/tiff" => "TIFF",
        "application/pdf" => "Adobe Portable Document Format (PDF)",
        _ => "Portable Network Graphics (PNG)",
    }
}
