//! Object creation and the Object menu: frames, lines, transforms, arrange, group, lock/hide,
//! fill/stroke, content type, Text Frame Options, corners, fitting, text wrap.

mod creation;
pub(crate) use creation::polygon_in;
mod appearance;

mod attributes;

use creation::{frame_create, line_create};
mod duplication;
pub use duplication::duplicate_from;
mod transform;
use transform::{
    apply_about_center, clear_transformations, step_and_repeat, transform_again, transform_info, transform_move, transform_resize, transform_rotate,
    transform_set,
};
pub(crate) use transform::{bake, scale_item};
mod arrangement;
pub(crate) use arrangement::RemoveKeep;
use arrangement::{align, all_flag, arrange, distribute, group, set_flag, ungroup};
mod text_frames;
use text_frames::{content_type, text_frame_options};
mod gaps;
use gaps::gap_move;
mod fitting;
use fitting::{clipping_path, fit, fitting_options};
mod paths;
use paths::{path_append, path_close, path_create, path_move_anchors};

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_doc::{
    Content, Document, Fill, Item, ItemId, ParaFormat, Selection, Shape, SpreadRef, Story, StoryId, Stroke, TextFrame, TextFrameOptions, TextSel,
};
use designcraft_geom::{Affine, Point, Rect, Vec2, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, f64_or, has_doc, has_selection, ok, point_param, rect_param, spread_param, str_param, targets};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "frame.create",
            "Create Frame",
            [],
            None,
            "{spread?, rect: [x0,y0,x1,y1] (spread coords), shape?: rectangle|ellipse|polygon, content?: graphic|text|unassigned, sides?: 6, text?: string, caret?: bool, vertical?: bool (text: a new vertical story)}",
            has_doc,
            frame_create
        ),
        cmd!("line.create", "Create Line", [], None, "{spread?, a: [x,y], b: [x,y]}", has_doc, line_create),
        cmd!("transform.move", "Move", ["Object", "Transform"], None, "{dx, dy, copy?: bool, ids?, toSpread?}", has_selection, transform_move),
        cmd!(
            "transform.resize",
            "Resize",
            [],
            None,
            "{from: rect, to: rect, content?: bool (scale content), distribute?: bool (Live Distribute: objects keep their size, their centres spread with the bounds), ids?}",
            has_selection,
            transform_resize
        ),
        cmd!("transform.rotate", "Rotate", ["Object", "Transform"], None, "{angle (degrees, CCW), ids?}", has_selection, transform_rotate),
        cmd!(
            "transform.scale",
            "Scale",
            ["Object", "Transform"],
            None,
            "{sx, sy, ids?} (about the selection centre)",
            has_selection,
            transform::transform_scale
        ),
        cmd!(
            "transform.again",
            "Transform Again",
            ["Object", "Transform Again"],
            Some("Cmd+Alt+3"),
            "{individually?: bool, sequence?: bool, ids?} — repeat the last transform (or the whole sequence applied to this selection), on the selection as a whole or on each object about its own centre",
            has_selection,
            transform_again
        ),
        cmd!(
            "transform.clear",
            "Clear Transformations",
            ["Object", "Transform"],
            None,
            "{ids?} — remove rotation, shear and scaling (the object keeps its centre)",
            has_selection,
            clear_transformations
        ),
        cmd!("transform.shear", "Shear", ["Object", "Transform"], None, "{angle (degrees), ids?}", has_selection, |s, p| {
            let a = f64_or(p, "angle", 0.0).clamp(-85.0, 85.0).to_radians().tan();
            apply_about_center(s, p, Affine::new([1.0, 0.0, a, 1.0, 0.0, 0.0]))
        }),
        cmd!(
            "object.matchAttributes",
            "Apply Attributes (Eyedropper)",
            [],
            None,
            "{from: id, ids?} — copy fill, stroke, corners, opacity, effects, wrap",
            has_selection,
            appearance::object_match_attributes
        ),
        cmd!("transform.flip", "Flip", ["Object", "Transform"], None, "{axis: horizontal|vertical, ids?}", has_selection, |s, p| {
            let h = str_param(p, "axis") != Some("vertical");
            apply_about_center(s, p, if h { Affine::scale_non_uniform(-1.0, 1.0) } else { Affine::scale_non_uniform(1.0, -1.0) })
        }),
        cmd!(
            "transform.set",
            "Transform Panel",
            [],
            None,
            "{x?, y?, width?, height?, scaleX? (%), scaleY? (%), rotation? (°), shear? (°), ref?: 0..8, ids?} — reference-point based geometry; rotation and shear are absolute (Transformations are Totals decides whether nested objects measure them on the pasteboard)",
            has_selection,
            transform_set
        ),
        cmd!(query "transform.info", "Transform Values", [], None, "{ids?} → {scaleX, scaleY (%), rotation, shear (°), content?: the same for a placed graphic} of the first target", has_selection, transform_info),
        cmd!("object.arrange", "Arrange", ["Object", "Arrange"], None, "{to: front|forward|backward|back, ids?}", has_selection, arrange),
        cmd!("object.bringToFront", "Bring to Front", ["Object", "Arrange"], Some("Cmd+Shift+]"), "{}", has_selection, |s, _| s
            .execute("object.arrange", &json!({"to": "front"}))),
        cmd!("object.bringForward", "Bring Forward", ["Object", "Arrange"], Some("Cmd+]"), "{}", has_selection, |s, _| s
            .execute("object.arrange", &json!({"to": "forward"}))),
        cmd!("object.sendBackward", "Send Backward", ["Object", "Arrange"], Some("Cmd+["), "{}", has_selection, |s, _| s
            .execute("object.arrange", &json!({"to": "backward"}))),
        cmd!("object.sendToBack", "Send to Back", ["Object", "Arrange"], Some("Cmd+Shift+["), "{}", has_selection, |s, _| s
            .execute("object.arrange", &json!({"to": "back"}))),
        cmd!("object.group", "Group", ["Object"], Some("Cmd+G"), "{ids?}", has_selection, group),
        cmd!("object.ungroup", "Ungroup", ["Object"], Some("Cmd+Shift+G"), "{ids?}", has_selection, ungroup),
        cmd!("object.lock", "Lock", ["Object"], Some("Cmd+L"), "{ids?}", has_selection, |s, p| set_flag(s, p, |i| i.locked = true, true)),
        cmd!("object.unlockAll", "Unlock All on Spread", ["Object"], Some("Cmd+Alt+L"), "{}", has_doc, |s, _| all_flag(s, |i| i.locked = false)),
        cmd!("object.hide", "Hide", ["Object"], Some("Cmd+3"), "{ids?}", has_selection, |s, p| set_flag(s, p, |i| i.hidden = true, true)),
        cmd!("object.showAll", "Show All on Spread", ["Object"], Some("Cmd+Alt+3"), "{}", has_doc, |s, _| all_flag(s, |i| i.hidden = false)),
        cmd!(
            "object.setFlags",
            "Set Visibility / Lock",
            [],
            None,
            "{ids: [id], hidden?: bool, locked?: bool} — Layers panel eye/lock per object",
            has_doc,
            arrangement::object_set_flags
        ),
        cmd!("object.fill", "Fill", [], None, "{swatch, tint?: 0..1, ids?}", has_selection, |s, p| {
            let sw = str_param(p, "swatch").ok_or_else(|| bad("object.fill", "missing swatch"))?.to_string();
            let tint = f64_or(p, "tint", 1.0) as f32;
            set_flag(s, p, move |i| i.fill = Fill { swatch: sw.clone(), tint, ..i.fill.clone() }, false)
        }),
        cmd!(
            "object.stroke",
            "Stroke",
            [],
            None,
            "{swatch?, tint?, weight?, align?: center|inside|outside, type?: {kind:…}, cap?: butt|round|projecting, join?: miter|round|bevel, miterLimit?, start?, end?: none|simple|simpleWide|triangle|triangleWide|barbed|curved|circle|circleSolid|square|squareSolid|bar, gapSwatch?, gapTint?, gapOverprint?: bool, ids?}",
            has_selection,
            appearance::object_stroke
        ),
        cmd!("object.opacity", "Opacity", [], None, "{opacity: 0..1, blend?: normal|multiply|…, ids?}", has_selection, appearance::object_opacity),
        cmd!(
            "object.dropShadow",
            "Drop Shadow",
            ["Object", "Effects"],
            Some("Cmd+Alt+M"),
            "{on?: bool, distance?, angle?, globalLight?: bool, opacity?, size?, spread? (%), color? (swatch), ids?}",
            has_selection,
            appearance::object_drop_shadow
        ),
        cmd!(
            "object.innerShadow",
            "Inner Shadow",
            ["Object", "Effects"],
            None,
            "{on?: bool, distance?, angle?, globalLight?: bool, opacity?, size?, choke? (%), color?, ids?}",
            has_selection,
            appearance::object_inner_shadow
        ),
        cmd!(
            "object.outerGlow",
            "Outer Glow",
            ["Object", "Effects"],
            None,
            "{on?: bool, opacity?, size?, spread? (%), color?, ids?}",
            has_selection,
            appearance::object_outer_glow
        ),
        cmd!(
            "object.exportOptions",
            "Object Export Options…",
            ["Object"],
            None,
            "{ids?, artifact?: bool (tagged PDF decoration), rasterize?: bool (EPUB/HTML image), align?: left|center|right|\"\", pageBreakBefore?: bool, altText?}",
            has_selection,
            appearance::object_export_options
        ),
        cmd!(
            "object.innerGlow",
            "Inner Glow",
            ["Object", "Effects"],
            None,
            "{on?: bool, opacity?, size?, choke? (%), center?: bool (glow from the centre), color?, ids?}",
            has_selection,
            appearance::object_inner_glow
        ),
        cmd!(
            "object.bevel",
            "Bevel and Emboss",
            ["Object", "Effects"],
            None,
            "{on?: bool, size?, depth? (%), angle?, globalLight?: bool, highlight?, highlightOpacity?, shadow?, shadowOpacity?, ids?}",
            has_selection,
            appearance::object_bevel
        ),
        cmd!(
            "object.satin",
            "Satin",
            ["Object", "Effects"],
            None,
            "{on?: bool, opacity?, angle?, distance?, size?, invert?: bool, color?, ids?}",
            has_selection,
            appearance::object_satin
        ),
        cmd!(
            "object.directionalFeather",
            "Directional Feather",
            ["Object", "Effects"],
            None,
            "{on?: bool, widths?: [top, left, bottom, right] | number, ids?}",
            has_selection,
            appearance::object_directional_feather
        ),
        cmd!("object.feather", "Basic Feather", ["Object", "Effects"], None, "{width (0 = off), ids?}", has_selection, |s, p| {
            let w = f64_or(p, "width", 9.0).max(0.0);
            set_flag(s, p, move |i| i.effects.feather = w, false)
        }),
        cmd!("object.content", "Content", ["Object", "Content"], None, "{type: graphic|text|unassigned, ids?}", has_selection, content_type),
        cmd!(
            "object.textFrameOptions",
            "Text Frame Options…",
            ["Object"],
            Some("Cmd+B"),
            "{columns?, gutter?, inset?: number|[t,l,b,r], verticalJustification?: top|center|bottom|justify, firstBaseline?, autoSize?, ignoreWrap?, balanceColumns?, vertical?: bool (sets the story direction of the frames' stories, as Type ▸ Story Direction), ids?}",
            has_selection,
            text_frame_options
        ),
        cmd!(
            "object.cornerOptions",
            "Corner Options…",
            ["Object"],
            None,
            "{shape: none|rounded|inverseRounded|inset|bevel|fancy, size, ids?}",
            has_selection,
            |s, p| {
                let shape =
                    p.get("shape").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(designcraft_geom::corners::CornerShape::Rounded);
                let size = f64_or(p, "size", 12.0);
                set_flag(s, p, move |i| i.corners = designcraft_geom::corners::CornerOptions::uniform(shape, size), false)
            }
        ),
        cmd!(
            "object.textWrap",
            "Text Wrap",
            ["Window"],
            Some("Cmd+Alt+W"),
            "{mode: none|boundingBox|contour|jumpObject|jumpToNextColumn, offset?: number|[t,l,b,r], invert?: bool, ids?}",
            has_selection,
            appearance::object_text_wrap
        ),
        cmd!(
            "object.fit",
            "Fitting",
            ["Object", "Fitting"],
            None,
            "{mode: fillProportionally|fitProportionally|fitContentToFrame|centerContent|fitFrameToContent, ids?}",
            has_selection,
            fit
        ),
        cmd!(
            "object.fittingOptions",
            "Frame Fitting Options…",
            ["Object", "Fitting"],
            None,
            "{autoFit?: bool, fitting?: none|fillProportionally|fitProportionally|fitContentToFrame|centerContent, align?: 0..8 (reference point, 4 = centre), crop?: n | [t, l, b, r] (negative adds space), ids?}",
            has_selection,
            fitting_options
        ),
        cmd!("object.rename", "Rename", [], None, "{id, name}", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            set_flag(s, p, move |i| i.name = name.clone(), false)
        }),
        cmd!(
            "edit.stepAndRepeat",
            "Step and Repeat…",
            ["Edit"],
            Some("Cmd+Alt+U"),
            "{count?: 1, dx?, dy?, rows?, columns?, ids?} — copies of the selection (or ids) offset by (dx, dy); with rows/columns, a grid",
            has_selection,
            step_and_repeat
        ),
        cmd!("path.create", "Create Path", [], None, "{spread?, anchors: [{p:[x,y], in?:[x,y], out?:[x,y]}], closed?: bool}", has_doc, path_create),
        cmd!("path.appendAnchor", "Add Anchor", [], None, "{id, anchor: {p, in?, out?}} (spread coords)", has_doc, path_append),
        cmd!("path.close", "Close Path", ["Object", "Paths"], None, "{id}", has_doc, path_close),
        cmd!("path.moveAnchors", "Move Anchors", [], None, "{id, anchors: [[subpath, index]], dx, dy, handle?: in|out}", has_doc, path_move_anchors),
        cmd!(
            "object.align",
            "Align",
            ["Window", "Object & Layout", "Align"],
            None,
            "{edge: left|hcenter|right|top|vcenter|bottom, to?: selection|keyObject|margins|page|spread, ids?}",
            has_selection,
            align
        ),
        cmd!(
            "object.distribute",
            "Distribute",
            ["Window", "Object & Layout", "Align"],
            None,
            "{axis: horizontal|vertical, by?: centers|spacing, spacing?: points, ids?}",
            has_selection,
            distribute
        ),
        cmd!(
            noundo "tool.polygonSettings",
            "Polygon Settings…",
            [],
            None,
            "{sides?: 3–100, starInset?: 0–100 (%)} → the settings new polygons use",
            super::always,
            creation::tool_polygon_settings
        ),
        cmd!(
            "gap.move",
            "Move Gap",
            [],
            None,
            "{at: [x, y] (spread), delta | dx/dy, spread?} — Gap tool: the gap at `at` between objects (or an object and the page edge) moves by `delta`; the objects on both sides resize → {axis, changed}",
            has_doc,
            gap_move
        ),
        cmd!(
            "frame.grid",
            "Gridify",
            [],
            None,
            "{rect, cols, rows, gutter? (pt, 12), shape?, content?, spread?} — a grid of frames filling `rect` (frame tools: arrow keys while dragging)",
            has_doc,
            text_frames::frame_grid
        ),
        cmd!(
            "object.clippingPath",
            "Clipping Path…",
            ["Object", "Clipping Path"],
            None,
            "{type?: alpha|edges (default alpha), threshold?: 0–255 (alpha: opaque above; edges: darker than 255 − threshold counts; default 25), tolerance?: px (2), ids?} — traces the placed graphic and makes the outline the frame (Convert Clipping Path to Frame)",
            has_selection,
            clipping_path
        ),
        cmd!(
            "object.primaryTextFrame",
            "Primary Text Frame",
            ["Object"],
            None,
            "{on?: bool (default: toggle), ids?} — the selected text frame's story becomes the primary story (Smart Text Reflow adds and removes pages for it)",
            has_selection,
            text_frames::object_primary_text_frame
        ),
        cmd!(
            "object.attributes",
            "Attributes",
            ["Window", "Output", "Attributes"],
            None,
            "{overprintFill?, overprintStroke?, overprintGap?, nonprinting?: bool, ids?} — Attributes panel",
            has_selection,
            attributes::object_attributes
        ),
        cmd!(
            "object.globalLight",
            "Global Light…",
            ["Object", "Effects"],
            None,
            "{angle} — the light angle shared by shadows that use Global Light",
            has_doc,
            |s, p| {
                let a = p.get("angle").and_then(Value::as_f64).ok_or_else(|| bad("object.globalLight", "angle required"))?;
                let a = (a + 180.0).rem_euclid(360.0) - 180.0;
                s.edit(|d, _| {
                    d.settings.global_light = a;
                    Ok(json!({"angle": a}))
                })
            }
        ),
        cmd!(
            "object.label",
            "Script Label",
            ["Window", "Utilities", "Script Label"],
            None,
            "{label, ids?} — free text scripts and agents can find objects by",
            has_selection,
            |s, p| {
                let label = str_param(p, "label").unwrap_or("").to_string();
                set_flag(s, p, move |i| i.label = label.clone(), false)
            }
        ),
        cmd!(
            "object.altText",
            "Object Export Options",
            ["Object"],
            None,
            "{text, ids?} — alternative text for tagged PDF and EPUB",
            has_selection,
            |s, p| {
                let text = str_param(p, "text").unwrap_or("").to_string();
                set_flag(s, p, move |i| i.alt_text = text.clone(), false)
            }
        ),
        cmd!(
            "object.transparencyGroup",
            "Group Transparency",
            [],
            None,
            "{isolate?: bool, knockout?: bool, ids?} — Effects panel: Isolate Blending, Knockout Group",
            has_selection,
            appearance::object_transparency_group
        ),
        cmd!(query "object.findByLabel", "Find by Script Label", [], None, "{label} → ids of objects whose label is `label`", has_doc, attributes::object_find_by_label),
        cmd!("object.setLayer", "Move to Layer", [], None, "{layer, ids?}", has_selection, |s, p| {
            let l = designcraft_doc::LayerId(p.get("layer").and_then(Value::as_u64).unwrap_or(0));
            set_flag(s, p, move |i| i.layer = l, false)
        }),
    ]
}

#[cfg(test)]
mod polygon_tests {
    use super::*;

    #[test]
    fn polygon_settings_make_stars() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("tool.polygonSettings", &json!({"sides": 5, "starInset": 50})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200], "shape": "polygon"})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let it = d.item(ItemId(id)).unwrap();
        // A 5-point star has 10 vertices, fitted to the rect.
        assert_eq!(it.path.subpaths[0].anchors.len(), 10);
        let b = it.bounds();
        assert!((b.x0 - 100.0).abs() < 1e-6 && (b.y1 - 200.0).abs() < 1e-6, "{b:?}");
        // Explicit parameters win over the settings.
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50], "shape": "polygon", "sides": 3, "starInset": 0})).unwrap()["id"]
            .as_u64()
            .unwrap();
        assert_eq!(s.doc().unwrap().doc.item(ItemId(id)).unwrap().path.subpaths[0].anchors.len(), 3);
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn scale_strokes_and_dimensions_with_stroke() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [100, 100, 200, 150]})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        s.execute("object.stroke", &json!({"weight": 4, "swatch": "Black"})).unwrap();
        let weight = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().stroke.weight;
        let bounds = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().bounds();
        s.execute("transform.scale", &json!({"sx": 2.0})).unwrap();
        assert_eq!(weight(&s), 8.0);
        assert_eq!(bounds(&s), Rect::new(50.0, 75.0, 250.0, 175.0));
        s.execute("prefs.set", &json!({"scaleStrokes": false})).unwrap();
        s.execute("transform.scale", &json!({"sx": 0.5})).unwrap();
        assert_eq!(weight(&s), 8.0);
        assert!(s.execute("prefs.set", &json!({"nope": 1})).is_err());
        // Width 108 with an 8 pt centred stroke: the path is 100 wide.
        s.execute("transform.set", &json!({"width": 108})).unwrap();
        assert!((bounds(&s).width() - 100.0).abs() < 1e-9, "{:?}", bounds(&s));
        s.execute("prefs.set", &json!({"dimensionsIncludeStroke": false})).unwrap();
        s.execute("transform.set", &json!({"width": 120})).unwrap();
        assert!((bounds(&s).width() - 120.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod gradient_tests {
    use super::*;

    #[test]
    fn gradient_tool_vector_and_unnamed_gradients() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [100, 100, 300, 200]})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        let swatches = |s: &Session| s.doc().unwrap().doc.swatches.len();
        let n = swatches(&s);
        // The default White→Black gradient, dragged from the left edge to the middle.
        let r = s.execute("object.gradient", &json!({"from": [100, 150], "to": [200, 150]})).unwrap();
        assert_eq!(r["kind"], "linear");
        assert_eq!(r["stops"][1]["color"], "#000000");
        assert_eq!(swatches(&s), n + 1, "an unnamed gradient");
        let fill = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().fill.clone();
        assert_eq!(fill(&s).gradient_vector, Some([100.0, 150.0, 200.0, 150.0]));
        // Same gradient again: no new swatch. Reverse / radial: a new unnamed gradient.
        s.execute("object.gradient", &json!({})).unwrap();
        assert_eq!(swatches(&s), n + 1);
        let r = s.execute("object.gradient", &json!({"kind": "radial", "reverse": true})).unwrap();
        assert_eq!(r["kind"], "radial");
        assert_eq!(r["stops"][0]["color"], "#000000");
        let r = s
            .execute(
                "object.gradient",
                &json!({"stops": [{"location": 0, "color": "#ff0000"}, {"location": 60, "color": "#0000ff", "midpoint": 30}]}),
            )
            .unwrap();
        assert_eq!(r["stops"][1]["location"], 60.0);
        assert_eq!(r["stops"][1]["midpoint"], 30.0);
        assert!(s.execute("object.gradient", &json!({"stops": [{"location": 0, "color": "#ff0000"}]})).is_err());
        // Scaling moves the vector with the shape.
        s.execute("transform.scale", &json!({"sx": 2.0})).unwrap();
        assert_eq!(fill(&s).gradient_vector, Some([0.0, 150.0, 200.0, 150.0]));
        // An angle replaces the vector.
        s.execute("object.gradient", &json!({"angle": 90})).unwrap();
        assert_eq!(fill(&s).gradient_vector, None);
        assert_eq!(fill(&s).gradient_angle, Some(90.0));
    }
}

#[cfg(test)]
mod again_tests {
    use super::*;

    #[test]
    fn transform_again_sequence_and_clear() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = ItemId(s.execute("frame.create", &json!({"rect": [100, 100, 200, 150]})).unwrap()["id"].as_u64().unwrap());
        let b = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().bounds();
        assert!(s.execute("transform.again", &json!({})).is_err());
        s.execute("transform.move", &json!({"dx": 10, "dy": 0})).unwrap();
        s.execute("transform.again", &json!({})).unwrap();
        assert_eq!(b(&s).x0, 120.0);
        s.execute("transform.rotate", &json!({"angle": 90})).unwrap();
        // Sequence: move 10 then rotate 90 again.
        s.execute("transform.again", &json!({"sequence": true})).unwrap();
        // The 100×50 frame is now 180° round from where it started: still 100 wide.
        assert!((b(&s).width() - 100.0).abs() < 1e-6, "{:?}", b(&s));
        // Clear Transformations: 180° in total → upright again, same centre.
        let c = b(&s).center();
        s.execute("transform.clear", &json!({})).unwrap();
        let it = s.doc().unwrap().doc.item(id).unwrap().clone();
        assert_eq!(it.xf.as_coeffs()[..4], [1.0, 0.0, 0.0, 1.0]);
        assert!((it.bounds().center() - c).hypot() < 1e-6);
        assert!((it.bounds().width() - 100.0).abs() < 1e-6);
        // Step and repeat via Transform Again on a moved copy.
        let n = s.doc().unwrap().doc.spreads[0].items.len();
        s.execute("transform.move", &json!({"dx": 0, "dy": 60, "copy": true})).unwrap();
        s.execute("transform.again", &json!({})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.spreads[0].items.len(), n + 2);
        let ys: Vec<f64> = d.spreads[0].items[n..].iter().map(|i| i.bounds().y0.round()).collect();
        assert_eq!(ys[1] - ys[0], 60.0, "{ys:?}");
    }
}

#[cfg(test)]
mod label_tests {
    use super::*;

    #[test]
    fn script_label_alt_text_and_group_transparency() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [60, 10, 90, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.label", &json!({"label": "price", "ids": [a, b]})).unwrap();
        s.execute("object.altText", &json!({"text": "A red square", "ids": [a]})).unwrap();
        assert_eq!(s.execute("object.findByLabel", &json!({"label": "price"})).unwrap()["ids"], json!([a, b]));
        assert_eq!(s.execute("object.findByLabel", &json!({"label": "nope"})).unwrap()["ids"], json!([]));
        assert_eq!(s.doc().unwrap().doc.item(ItemId(a)).unwrap().alt_text, "A red square");
        s.execute("object.transparencyGroup", &json!({"knockout": true, "ids": [b]})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(b)).unwrap().clone();
        assert!(it.knockout && !it.isolate);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.doc().unwrap().doc.item(ItemId(b)).unwrap().knockout);
    }
}

#[cfg(test)]
mod light_tests {
    use super::*;

    #[test]
    fn global_light_moves_shadows_that_use_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.dropShadow", &json!({"on": true, "angle": 45, "ids": [a]})).unwrap();
        let angle = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            let ds = &d.item(ItemId(a)).unwrap().effects.drop_shadow;
            d.light_angle(ds.angle, ds.global_light)
        };
        s.execute("object.globalLight", &json!({"angle": 200})).unwrap();
        assert_eq!(angle(&s), 45.0, "own angle");
        s.execute("object.dropShadow", &json!({"globalLight": true, "on": true, "ids": [a]})).unwrap();
        assert_eq!(angle(&s), -160.0, "normalised to -180..180");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(angle(&s), 45.0);
    }
}

#[cfg(test)]
mod transform_values_tests {
    use super::*;

    #[test]
    fn absolute_rotation_shear_and_totals() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 150]})).unwrap()["id"].as_u64().unwrap();
        let info = |s: &mut Session, id: u64| s.execute("transform.info", &json!({"ids": [id]})).unwrap();
        let centre = |s: &Session| s.doc().unwrap().doc.item(ItemId(a)).unwrap().bounds().center();
        let c0 = centre(&s);
        s.execute("transform.set", &json!({"rotation": 30, "ids": [a]})).unwrap();
        s.execute("transform.set", &json!({"rotation": 30, "ids": [a]})).unwrap();
        assert_eq!(info(&mut s, a)["rotation"], 30.0, "absolute, not cumulative");
        assert!((centre(&s) - c0).hypot() < 1e-9, "about the centre reference point");
        s.execute("transform.set", &json!({"shear": 20, "ids": [a]})).unwrap();
        let i = info(&mut s, a);
        assert_eq!((i["rotation"].as_f64(), i["shear"].as_f64()), (Some(30.0), Some(20.0)));
        assert_eq!(i["scaleX"], 100.0);
        // Nested: a group rotated 10° around the rotated frame.
        let b = s.execute("frame.create", &json!({"rect": [300, 100, 350, 150]})).unwrap()["id"].as_u64().unwrap();
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        s.execute("transform.rotate", &json!({"angle": 10, "ids": [g]})).unwrap();
        assert_eq!(info(&mut s, a)["rotation"], 40.0, "totals");
        s.execute("prefs.set", &json!({"transformationsAreTotals": false})).unwrap();
        assert_eq!(info(&mut s, a)["rotation"], 30.0, "relative to the group");
        s.execute("transform.set", &json!({"rotation": 0, "shear": 0, "ids": [a]})).unwrap();
        s.execute("prefs.set", &json!({"transformationsAreTotals": true})).unwrap();
        assert_eq!(info(&mut s, a)["rotation"], 10.0);
        // Scale is relative to what's shown: a frame always shows 100%.
        s.execute("transform.set", &json!({"scaleX": 200, "ids": [b]})).unwrap();
        assert_eq!(info(&mut s, b)["scaleX"], 100.0);
    }
}

#[cfg(test)]
mod attributes_tests {
    use super::*;

    #[test]
    fn attributes_panel_flags() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.attributes", &json!({"overprintFill": true, "overprintGap": true, "nonprinting": true})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(a)).unwrap().clone();
        assert!(it.fill.overprint && it.stroke.gap_overprint && it.nonprinting && !it.stroke.overprint);
        // IDML keeps them.
        let bytes = designcraft_idml::export_idml(&s.doc().unwrap().doc);
        let back = designcraft_idml::import_idml(&bytes).unwrap();
        let it = back.spreads.iter().flat_map(|sp| sp.items.iter()).find(|i| i.fill.overprint).expect("overprint fill survives");
        assert!(it.stroke.gap_overprint);
    }
}

#[cfg(test)]
mod fitting_tests {
    use super::*;

    #[test]
    fn separations_come_from_source_colours() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let c = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "C=100 M=0 Y=0 K=0", "ids": [c]})).unwrap();
        let k = s.execute("frame.create", &json!({"rect": [300, 100, 400, 200]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [k]})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let plate = |p: u8| {
            let mut rr = designcraft_render::Renderer::new();
            rr.threads = 0;
            rr.render_page(&d, &s.cache, 0, 1.0, false, &designcraft_render::RenderOptions { plate: Some(p), ..Default::default() }).unwrap()
        };
        let (cyan, black) = (plate(0), plate(3));
        // Cyan ink only on the cyan frame; black ink only on the black one (no rich-black guess).
        assert!(cyan.pixel(150, 150)[0] < 10 && cyan.pixel(350, 150)[0] > 245, "{:?} {:?}", cyan.pixel(150, 150), cyan.pixel(350, 150));
        assert!(black.pixel(350, 150)[0] < 10 && black.pixel(150, 150)[0] > 245);
        assert!(cyan.pixel(20, 20)[0] > 245, "paper has no ink");
    }

    #[test]
    fn inner_glow_bevel_and_satin_change_the_inside() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "C=100 M=0 Y=0 K=0", "ids": [id]})).unwrap();
        let shot = |s: &Session| {
            let d = s.doc().unwrap().doc.clone();
            let mut rr = designcraft_render::Renderer::new();
            rr.threads = 0;
            rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap()
        };
        let plain = shot(&s);
        // Inner glow (paper colour, screened): the edge lightens, the centre doesn't.
        s.execute("object.innerGlow", &json!({"ids": [id], "on": true, "size": 20})).unwrap();
        let g = shot(&s);
        assert!(g.pixel(103, 200)[0] > plain.pixel(103, 200)[0] + 30, "edge lighter");
        assert_eq!(g.pixel(200, 200), plain.pixel(200, 200));
        s.execute("object.innerGlow", &json!({"ids": [id], "on": false})).unwrap();
        // Bevel: lit and shaded sides differ.
        s.execute("object.bevel", &json!({"ids": [id], "on": true, "size": 20, "angle": 120, "globalLight": false})).unwrap();
        let b = shot(&s);
        let (top_left, bottom_right) = (b.pixel(104, 104), b.pixel(296, 296));
        assert!(top_left[1] as i32 > bottom_right[1] as i32 + 30, "{top_left:?} {bottom_right:?}");
        s.execute("object.bevel", &json!({"ids": [id], "on": false})).unwrap();
        // Satin darkens somewhere inside.
        s.execute("object.satin", &json!({"ids": [id], "on": true, "distance": 40, "size": 10})).unwrap();
        let st = shot(&s);
        let darker = (110..290)
            .step_by(10)
            .flat_map(|x| (110..290).step_by(10).map(move |y| (x, y)))
            .any(|(x, y)| (st.pixel(x, y)[1] as i32) < plain.pixel(x, y)[1] as i32 - 30);
        assert!(darker);
        s.execute("object.satin", &json!({"ids": [id], "on": false})).unwrap();
        // Directional feather: only the left edge fades.
        s.execute("object.directionalFeather", &json!({"ids": [id], "on": true, "widths": [0, 40, 0, 0]})).unwrap();
        let df = shot(&s);
        assert!(df.pixel(102, 200)[0] > 200, "left edge faded to paper: {:?}", df.pixel(102, 200));
        assert_eq!(df.pixel(298, 200), plain.pixel(298, 200), "right edge untouched");
        // A knockout group exports as an image too.
        let other = s.execute("frame.create", &json!({"rect": [400, 100, 450, 150]})).unwrap()["id"].clone();
        let other2 = s.execute("frame.create", &json!({"rect": [420, 120, 470, 170]})).unwrap()["id"].clone();
        let g = s.execute("object.group", &json!({"ids": [other, other2]})).unwrap()["id"].clone();
        s.execute("object.transparencyGroup", &json!({"ids": [g], "knockout": true})).unwrap();
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("2 object(s)")), "{r}");
        // PDF export carries effects (as an image of the object).
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("soft effects")), "{r}");
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert!(String::from_utf8_lossy(&bytes).contains("/Subtype/Image"));
    }

    #[test]
    fn live_distribute_spreads_objects_without_resizing() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 120, 120]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [180, 100, 200, 120]})).unwrap()["id"].as_u64().unwrap();
        s.execute("transform.resize", &json!({"ids": [a, b], "from": [100, 100, 200, 120], "to": [100, 100, 300, 120], "distribute": true})).unwrap();
        let d = &s.doc().unwrap().doc;
        let (ra, rb) = (d.item(ItemId(a)).unwrap().bounds(), d.item(ItemId(b)).unwrap().bounds());
        assert!((ra.width() - 20.0).abs() < 1e-9 && (rb.width() - 20.0).abs() < 1e-9, "sizes kept");
        assert!((ra.center().x - 120.0).abs() < 1e-9, "{ra:?}");
        assert!((rb.center().x - 280.0).abs() < 1e-9, "{rb:?}");
    }

    #[test]
    fn frame_fitting_options_align_crop_and_auto_fit() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // A 40×20 pt image placed into a 200×200 frame.
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
        let content = |s: &Session| {
            let it = s.doc().unwrap().doc.item(ItemId(f)).unwrap().clone();
            let Content::Graphic(g) = &it.content else { panic!("graphic") };
            it.xf.transform_rect_bbox(g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1)))
        };
        // Fit proportionally from the top-left: 200 wide, 100 high, against the top.
        s.execute("object.fittingOptions", &json!({"ids": [f], "fitting": "fitProportionally", "align": 0})).unwrap();
        let c = content(&s);
        assert!((c.x0 - 100.0).abs() < 1e-6 && (c.y0 - 100.0).abs() < 1e-6 && (c.width() - 200.0).abs() < 1e-6, "{c:?}");
        // Crop 10 pt on every side: the content spreads past the frame.
        s.execute("object.fittingOptions", &json!({"ids": [f], "crop": 10, "align": 4})).unwrap();
        let c = content(&s);
        assert!((c.width() - 220.0).abs() < 1e-6, "{c:?}");
        // Auto-fit: resizing the frame refits the content.
        s.execute("object.fittingOptions", &json!({"ids": [f], "autoFit": true, "fitting": "fillProportionally", "crop": 0})).unwrap();
        s.execute("transform.resize", &json!({"from": [100, 100, 300, 300], "to": [100, 100, 500, 300], "ids": [f]})).unwrap();
        let c = content(&s);
        assert!((c.width() - 400.0).abs() < 1e-6, "fills the wider frame: {c:?}");
        // IDML keeps the options.
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        let g = back.spreads.iter().flat_map(|sp| sp.items.iter()).find_map(|i| i.graphic().cloned()).unwrap();
        assert_eq!(g.auto_fit, designcraft_doc::Fitting::FillProportionally);
    }
}

#[cfg(test)]
mod primary_frame_tests {
    use super::*;

    #[test]
    fn toggle_primary_text_frame() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 300], "content": "text", "text": "x", "caret": false})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("object.primaryTextFrame", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.primary_story.map(|x| x.0), Some(sid));
        s.execute("object.primaryTextFrame", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.primary_story, None, "toggles off");
        let g = s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].clone();
        assert!(s.execute("object.primaryTextFrame", &json!({"ids": [g]})).is_err());
    }
}

#[cfg(test)]
mod clipping_tests {
    use super::*;

    #[test]
    fn clipping_path_from_alpha_traces_the_shape() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // 40×40 image: an opaque disc on transparency.
        let mut px = vec![0u8; 40 * 40 * 4];
        for y in 0..40 {
            for x in 0..40 {
                let (dx, dy) = (x as f64 + 0.5 - 20.0, y as f64 + 0.5 - 20.0);
                if (dx * dx + dy * dy).sqrt() < 15.0 {
                    px[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4].copy_from_slice(&[200, 0, 0, 255]);
                }
            }
        }
        let png = designcraft_render::Rendered { width: 40, height: 40, pixels: px }.to_png();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 140, 140]})).unwrap()["id"].as_u64().unwrap();
        s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "disc.png", "frame": f})).unwrap();
        s.execute("object.clippingPath", &json!({"ids": [f], "tolerance": 0.5})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(f)).unwrap().clone();
        let b = it.bounds();
        // The disc spans 5..35 of the 40 px image placed at 100..140.
        assert!((b.x0 - 105.0).abs() < 1.5 && (b.x1 - 135.0).abs() < 1.5, "{b:?}");
        assert_eq!(it.shape, Shape::Path);
        assert!(s.execute("object.clippingPath", &json!({"ids": [f], "threshold": 255})).is_err());
    }
}

#[cfg(test)]
mod gap_tests {
    use super::*;

    #[test]
    fn moving_a_gap_resizes_both_sides() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 300]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [220, 100, 320, 300]})).unwrap()["id"].as_u64().unwrap();
        let r = s.execute("gap.move", &json!({"at": [210, 200], "delta": 30})).unwrap();
        assert_eq!((r["axis"].as_str(), r["changed"].as_u64()), (Some("x"), Some(2)));
        let bb = |s: &Session, id: u64| s.doc().unwrap().doc.item(ItemId(id)).unwrap().bounds();
        assert_eq!((bb(&s, a).x1, bb(&s, b).x0), (230.0, 250.0), "the gap moved, its width kept");
        assert_eq!(bb(&s, b).x1, 320.0);
        assert!(s.execute("gap.move", &json!({"at": [150, 200], "delta": 5})).is_err(), "inside an object");
    }
}

#[cfg(test)]
mod line_end_tests {
    use super::*;

    /// "One\rTwo" made one paragraph (the CR stayed in the text) instead of two.
    #[test]
    fn carriage_returns_separate_paragraphs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "One\rTwo\r\nThree"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let story = |s: &Session| s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(story(&s).text, "One\nTwo\nThree");
        assert_eq!(story(&s).paras.len(), 3);
        // The caret is at the end: typed text gets the same treatment.
        s.execute("text.insert", &json!({"text": "\rFour", "raw": true})).unwrap();
        assert_eq!(story(&s).text, "One\nTwo\nThree\nFour");
        assert_eq!(story(&s).paras.len(), 4);
    }
}

#[cfg(test)]
mod named_target_tests {
    use super::*;

    /// With a text caret (or nothing) selected, a command given `ids` failed
    /// with "command `object.textFrameOptions` is not available right now: nothing selected".
    #[test]
    fn ids_act_without_a_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Hello"})).unwrap();
        assert!(s.doc().unwrap().selection.items.is_empty(), "frame.create leaves a text caret");
        let e = s.execute("object.textFrameOptions", &json!({"columns": 2})).unwrap_err().to_string();
        assert!(e.contains("nothing selected"), "{e}");
        s.execute("object.textFrameOptions", &json!({"ids": [r["id"]], "columns": 2})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        let columns = |s: &Session| s.doc().unwrap().doc.item(id).and_then(|i| i.text_frame().map(|t| t.options.columns));
        assert_eq!(columns(&s), Some(2));
        s.execute("transform.set", &json!({"id": r["id"], "x": 100, "ref": 0})).unwrap();
        let e = s.execute("object.textFrameOptions", &json!({"ids": [99999], "columns": 1})).unwrap_err().to_string();
        assert!(e.contains("no object with id 99999"), "{e}");
        assert_eq!(columns(&s), Some(2));
        // An empty `ids` names nothing (as `targets` reads it), even beside an `id`.
        let e = s.execute("object.textFrameOptions", &json!({"ids": [], "id": r["id"], "columns": 3})).unwrap_err().to_string();
        assert!(e.contains("nothing selected"), "{e}");
        assert_eq!(columns(&s), Some(2));
        // Commands documented with `ids` honour `id` too.
        assert_eq!(s.execute("conveyor.collect", &json!({"id": r["id"]})).unwrap()["count"], 1);
    }
}
