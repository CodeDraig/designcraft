//! Read IDML margins, frame options and Bézier path geometry.

use super::{Anchor, Columns, ColumnsKind, El, Margins, PathData, SubPath, TextFrameOptions, names, parse_point};

pub(super) fn margins_of(m: &El) -> (Margins, Columns) {
    let g = |k: &str, d: f64| m.num(k).unwrap_or(d);
    (
        Margins { top: g("Top", 36.0), bottom: g("Bottom", 36.0), inside: g("Left", 36.0), outside: g("Right", 36.0) },
        Columns { count: g("ColumnCount", 1.0).max(1.0) as u32, gutter: g("ColumnGutter", 12.0), positions: None },
    )
}

pub(super) fn text_frame_options(e: &El) -> TextFrameOptions {
    let mut o = TextFrameOptions::default();
    if let Some(v) = e.num("TextColumnCount") {
        o.columns = v.max(1.0) as u32;
    }
    if let Some(v) = e.num("TextColumnGutter") {
        o.gutter = v;
    }
    o.columns_kind = if e.get("UseFixedColumnWidth") == Some("true") {
        ColumnsKind::FixedWidth
    } else if e.get("UseFlexibleColumnWidth") == Some("true") {
        ColumnsKind::FlexibleWidth
    } else {
        ColumnsKind::FixedNumber
    };
    if o.columns_kind != ColumnsKind::FixedNumber
        && let Some(v) = e.num("TextColumnFixedWidth")
    {
        o.column_width = v;
    }
    o.balance_columns = e.get("VerticalBalanceColumns") == Some("true");
    if let Some(inset) = inset_spacing(e) {
        o.inset = inset;
    }
    if let Some(v) = e.get("VerticalJustification") {
        o.vertical_justification = names::vj_in(v);
    }
    if let Some(v) = e.num("VerticalThreshold") {
        o.vj_paragraph_spacing_limit = v;
    }
    if let Some(v) = e.get("FirstBaselineOffset") {
        o.first_baseline = names::first_baseline_in(v);
    }
    if let Some(v) = e.num("MinimumFirstBaselineOffset") {
        o.first_baseline_min = v;
    }
    o.ignore_wrap = e.get("IgnoreWrap") == Some("true");
    if let Some(v) = e.get("AutoSizingType") {
        o.auto_size = names::auto_size_in(v);
    }
    if let Some(v) = e.get("AutoSizingReferencePoint") {
        o.auto_size_ref = names::REF_POINTS.iter().position(|p| *p == v).unwrap_or(1) as u8;
    }
    o
}

/// InsetSpacing properties are scalar (every edge) or lists (top, left, bottom, right).
/// Finite negative insets remain supported.
fn inset_spacing(e: &El) -> Option<[f64; 4]> {
    let number = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    let property = e.prop_el("InsetSpacing")?;
    let mut items = property.elements();
    if property.get("type") == Some("list") || property.find("ListItem").is_some() {
        let mut inset = [0.0; 4];
        for edge in &mut inset {
            let item = items.next()?;
            if item.local() != "ListItem" || item.elements().next().is_some() {
                return None;
            }
            *edge = number(&item.text_content())?;
        }
        items.next().is_none().then_some(inset)
    } else if items.next().is_none() {
        number(&property.text_content()).map(|v| [v; 4])
    } else {
        None
    }
}

pub(super) fn path_of(pg: &El) -> PathData {
    let mut subs = Vec::new();
    for g in pg.find_all("GeometryPathType") {
        let closed = g.get("PathOpen") != Some("true");
        let mut anchors = Vec::new();
        if let Some(arr) = g.find("PathPointArray") {
            for p in arr.find_all("PathPointType") {
                let Some(a) = parse_point(p.get("Anchor")) else { continue };
                let hin = parse_point(p.get("LeftDirection")).unwrap_or(a);
                let hout = parse_point(p.get("RightDirection")).unwrap_or(a);
                anchors.push(Anchor::with_handles(a, hin, hout));
            }
        }
        if !anchors.is_empty() {
            subs.push(SubPath::new(anchors, closed));
        }
    }
    PathData::new(subs)
}
