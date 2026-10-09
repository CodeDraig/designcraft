//! IDML package → Document.

mod attributes;
mod geometry;
mod graphics;
mod items;
mod preferences;
mod references;
mod spreads;
mod stories;
mod styles;
mod tables;
use geometry::{margins_of, path_of, text_frame_options};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use designcraft_color::cms::lab;
use designcraft_color::{Color, ColorType, Gradient, GradientKind, GradientStop, Swatch, SwatchValue, swatch};
use designcraft_doc::{
    AltFills, Asset, AssetId, BaselineGrid, Cell, CellRange, CellStroke, CharAttrs, CharFormat, CharRun, CharacterStyle, Columns, ColumnsKind,
    Content, DocSettings, Document, DropShadow, Effects, Fill, GridAlign, GridRelative, Guide, Item, ItemId, Kerning, LAYER_COLORS, Layer, LayerId,
    ListType, Margins, ObjectStyle, Page, PageId, PageSide, ParaAttrs, ParaFormat, ParagraphStyle, ParentInfo, RowHeightMode, Rule, Section, Shape,
    SpanColumns, Spread, SpreadId, Story, StoryId, Stroke, Styles, TabStop, Table, TextFrame, TextFrameOptions, TextWrap, VerticalJustification,
    story as st,
};
use designcraft_geom::corners::{Corner, CornerOptions};
use designcraft_geom::{Affine, Anchor, PathData, Point, Rect, SubPath};

use crate::names::{self, CHAR_BUILTINS, OBJECT_BUILTINS, PARA_BUILTINS, unescape_id};
use crate::xml::{El, Node, parse};
use crate::{IdmlError, MIMETYPE, Result, base64_decode, sniff_image};

/// Import an IDML package. Linked images are read from disk when available (not on wasm).
pub fn import_idml(bytes: &[u8]) -> Result<Document> {
    import_idml_with(bytes, &default_reader)
}

fn default_reader(path: &str) -> Option<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read(path).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

/// Import with a custom reader for linked files (`path` → bytes).
pub fn import_idml_with(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Document> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| IdmlError::NotIdml(e.to_string()))?;
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| IdmlError::NotIdml(e.to_string()))?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        // The size is what the archive claims: don't reserve more than a sane part up front.
        let mut buf = Vec::with_capacity((f.size() as usize).min(1 << 24));
        f.read_to_end(&mut buf).map_err(|e| IdmlError::Part { part: name.clone(), msg: e.to_string() })?;
        files.insert(name, buf);
    }
    if let Some(m) = files.get("mimetype")
        && String::from_utf8_lossy(m).trim() != MIMETYPE
    {
        return Err(IdmlError::NotIdml(format!("unexpected mimetype `{}`", String::from_utf8_lossy(m).trim())));
    }
    let dm = files.get("designmap.xml").ok_or_else(|| IdmlError::NotIdml("missing designmap.xml".into()))?;
    let root = parse(dm).map_err(|msg| IdmlError::Part { part: "designmap.xml".into(), msg })?;
    if root.local() != "Document" {
        return Err(IdmlError::NotIdml(format!("designmap root is <{}>", root.name)));
    }
    // Flatten includes: every `idPkg:*` child is replaced by the children of its part's root.
    let mut top: Vec<El> = Vec::new();
    for c in root.elements() {
        if c.name.starts_with("idPkg:") {
            let Some(src) = c.get("src") else { continue };
            let Some(data) = files.get(src) else { continue };
            let part = parse(data).map_err(|msg| IdmlError::Part { part: src.into(), msg })?;
            top.extend(part.elements().cloned());
        } else {
            top.push(c.clone());
        }
    }
    let mut im = Importer::new(read_link);
    im.run(&root, &top)?;
    let d = im.finish(&root)?;
    d.check().map_err(|e| IdmlError::Invalid(e.to_string()))?;
    Ok(d)
}

struct ItemCtx {
    /// Self → (story self, prev self, next self)
    threads: Vec<(ItemId, String, String, String)>,
}

struct Importer<'r> {
    read_link: &'r dyn Fn(&str) -> Option<Vec<u8>>,
    next_id: u64,
    settings: DocSettings,
    default_margins: Option<(Margins, Columns)>,
    swatches: Vec<Swatch>,
    /// Swatch Self → our swatch name.
    swatch_names: HashMap<String, String>,
    /// Hidden (unnamed) colours by Self.
    hidden_colors: HashMap<String, Color>,
    /// Unnamed swatches (gradients) added on first use.
    hidden_swatches: HashMap<String, Swatch>,
    color_groups: Vec<designcraft_doc::ColorGroup>,
    conditions: Vec<designcraft_doc::Condition>,
    condition_names: HashMap<String, String>,
    inks: designcraft_doc::InkManager,
    stroke_styles: Vec<designcraft_doc::StrokeStyleDef>,
    styles: Styles,
    kinsoku: HashMap<String, designcraft_doc::cjk::Kinsoku>,
    para_names: HashMap<String, String>,
    char_names: HashMap<String, String>,
    lists: Vec<designcraft_doc::NumberedList>,
    object_names: HashMap<String, String>,
    /// Object style Self → element (attribute fallback for page items).
    object_els: HashMap<String, El>,
    layers: Vec<Layer>,
    layer_ids: HashMap<String, LayerId>,
    stories: BTreeMap<StoryId, Story>,
    story_ids: HashMap<String, StoryId>,
    parents: Vec<Spread>,
    parent_ids: HashMap<String, SpreadId>,
    spreads: Vec<Spread>,
    page_index: HashMap<String, usize>,
    item_ids: HashMap<String, ItemId>,
    assets: BTreeMap<AssetId, Arc<Asset>>,
    ctx: ItemCtx,
    sections: Vec<Section>,
    footnote_options: designcraft_doc::FootnoteOptions,
    /// Text destination Self → anchor id.
    anchor_ids: HashMap<String, u64>,
    /// Cross-reference source Self → (story, index in its cross-references).
    xref_srcs: HashMap<String, (StoryId, usize)>,
    /// Cross-reference format Self → name.
    xref_format_names: HashMap<String, String>,
    xref_formats: Vec<designcraft_doc::XrefFormat>,
    /// Index topic Self → topic path; topic See / See also cross-references.
    index_topics: HashMap<String, Vec<String>>,
    index_xrefs: Vec<designcraft_doc::IndexRef>,
}

fn nums(s: &str) -> Vec<f64> {
    s.split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

fn parse_affine(s: Option<&str>) -> Affine {
    match s.map(nums) {
        Some(v) if v.len() == 6 => Affine::new([v[0], v[1], v[2], v[3], v[4], v[5]]),
        _ => Affine::IDENTITY,
    }
}

fn parse_point(s: Option<&str>) -> Option<Point> {
    let v = nums(s?);
    (v.len() >= 2).then(|| Point::new(v[0], v[1]))
}

/// IDML tint (percent, `-1` = default) → 0..1.
fn tint(v: Option<f64>) -> Option<f32> {
    v.map(|t| if t < 0.0 { 1.0 } else { (t / 100.0) as f32 })
}

impl<'r> Importer<'r> {
    fn new(read_link: &'r dyn Fn(&str) -> Option<Vec<u8>>) -> Self {
        let specials: Vec<Swatch> = designcraft_color::default_swatches().into_iter().filter(|s| s.locked).collect();
        let mut swatch_names = HashMap::new();
        swatch_names.insert("Swatch/None".to_string(), swatch::NONE.to_string());
        swatch_names.insert("Color/Paper".to_string(), swatch::PAPER.to_string());
        swatch_names.insert("Color/Black".to_string(), swatch::BLACK.to_string());
        swatch_names.insert("Color/Registration".to_string(), swatch::REGISTRATION.to_string());
        let mut styles = Styles::default();
        // Built-ins are replaced by the file's definitions when present.
        styles.paragraph.iter_mut().for_each(|s| s.based_on = None);
        Importer {
            read_link,
            next_id: 0,
            settings: DocSettings::default(),
            default_margins: None,
            swatches: specials,
            swatch_names,
            hidden_colors: HashMap::new(),
            hidden_swatches: HashMap::new(),
            color_groups: Vec::new(),
            conditions: Vec::new(),
            condition_names: HashMap::new(),
            inks: Default::default(),
            stroke_styles: Vec::new(),
            styles,
            kinsoku: HashMap::new(),
            para_names: HashMap::new(),
            char_names: HashMap::new(),
            lists: Vec::new(),
            object_names: HashMap::new(),
            object_els: HashMap::new(),
            layers: Vec::new(),
            layer_ids: HashMap::new(),
            stories: BTreeMap::new(),
            story_ids: HashMap::new(),
            parents: Vec::new(),
            parent_ids: HashMap::new(),
            spreads: Vec::new(),
            page_index: HashMap::new(),
            item_ids: HashMap::new(),
            assets: BTreeMap::new(),
            ctx: ItemCtx { threads: Vec::new() },
            sections: Vec::new(),
            footnote_options: Default::default(),
            anchor_ids: HashMap::new(),
            xref_srcs: HashMap::new(),
            xref_format_names: HashMap::new(),
            xref_formats: designcraft_doc::xref::default_formats(),
            index_topics: HashMap::new(),
            index_xrefs: Vec::new(),
        }
    }

    fn alloc(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn run(&mut self, _root: &El, top: &[El]) -> Result<()> {
        // Preferences first (page size, facing pages).
        for e in top {
            match e.local() {
                "DocumentPreference" => self.doc_prefs(e),
                "TransparencyPreference" => {
                    if let Some(v) = e.get("BlendingSpace") {
                        self.settings.blend_space = if v == "RGB" { designcraft_doc::BlendSpace::Rgb } else { designcraft_doc::BlendSpace::Cmyk };
                    }
                }
                "TextPreference" => {
                    let a = &mut self.settings.advanced_type;
                    for (k, v) in [
                        ("SuperscriptSize", &mut a.superscript_size),
                        ("SuperscriptPosition", &mut a.superscript_position),
                        ("SubscriptSize", &mut a.subscript_size),
                        ("SubscriptPosition", &mut a.subscript_position),
                    ] {
                        if let Some(x) = e.num(k) {
                            *v = x;
                        }
                    }
                }
                "MarginPreference" => {
                    let (m, c) = margins_of(e);
                    self.default_margins = Some((m, c));
                }
                "ViewPreference" => {
                    if let Some(u) = e.get("HorizontalMeasurementUnits") {
                        self.settings.horizontal_units = names::unit_in(u);
                    }
                    if let Some(u) = e.get("VerticalMeasurementUnits") {
                        self.settings.vertical_units = names::unit_in(u);
                    }
                    if let Some(v) = e.num("CursorKeyIncrement") {
                        self.settings.keyboard_increment = v;
                    }
                }
                "GridPreference" => self.grid_prefs(e),
                "PasteboardPreference" => {
                    if let Some(v) = e.get("PasteboardMargins").map(nums)
                        && v.len() == 2
                        && v[0] >= 0.0
                        && v[1] >= 0.0
                    {
                        self.settings.pasteboard = (v[0], v[1]);
                    }
                }
                _ => {}
            }
        }
        for e in top.iter().filter(|e| e.local() == "CompositeFont") {
            let entries = e
                .find_all("CompositeFontEntry")
                .map(|c| designcraft_doc::cjk::CompositeFontEntry {
                    name: c.get("Name").unwrap_or("").into(),
                    characters: c.get("CustomCharacters").unwrap_or("").into(),
                    family: c.prop("AppliedFont").unwrap_or_default(),
                    style: c.get("FontStyle").unwrap_or("Regular").trim_start_matches("$ID/").into(),
                    relative_size: c.num("RelativeSize").unwrap_or(100.0) / 100.0,
                    horizontal_scale: c.num("HorizontalScale").unwrap_or(100.0) / 100.0,
                    vertical_scale: c.num("VerticalScale").unwrap_or(100.0) / 100.0,
                    baseline_shift: c.num("BaselineShift").unwrap_or(0.0) / 100.0,
                    scale_option: c.boolean("ScaleOption").unwrap_or(true),
                })
                .collect();
            self.styles.composite_fonts.push(designcraft_doc::cjk::CompositeFont { name: unescape_id(e.get("Name").unwrap_or("")), entries });
        }
        for e in top.iter().filter(|e| e.local() == "KinsokuTable") {
            if e.get("CantBeginLineChars").is_some() || e.get("CantEndLineChars").is_some() {
                self.kinsoku.insert(
                    e.get("Self").unwrap_or("").into(),
                    designcraft_doc::cjk::Kinsoku {
                        name: e.get("Name").unwrap_or("").into(),
                        no_start: e.get("CantBeginLineChars").unwrap_or("").into(),
                        no_end: e.get("CantEndLineChars").unwrap_or("").into(),
                        inseparable: e.get("CantBeSeparatedChars").unwrap_or("").into(),
                        hanging: e.get("HangingPunctuationChars").unwrap_or("").into(),
                    },
                );
            }
        }
        for e in top.iter().filter(|e| e.local() == "MojikumiTable") {
            let overrides = e
                .prop_el("OverrideMojikumiAkiList")
                .map(|l| {
                    l.find_all("OverrideMojikumiAkiType")
                        .map(|r| designcraft_doc::cjk::MojikumiAki {
                            target_class: r.num("TargetMojikumiClass").unwrap_or(0.0) as i16,
                            side_class: r.num("SideMojikumiClass").unwrap_or(0.0) as i16,
                            after: r.boolean("SideIsAfterTarget").unwrap_or(false),
                            minimum: r.num("Minimum").unwrap_or(0.0),
                            desired: r.num("Desired").unwrap_or(0.0),
                            maximum: r.num("Maximum").unwrap_or(0.0),
                            priority: r.num("CompressionPriority").unwrap_or(0.0) as i16,
                            does_not_float: r.boolean("AkiDoesNotFloat").unwrap_or(false),
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.styles.mojikumi_tables.push(designcraft_doc::cjk::MojikumiTable {
                name: unescape_id(e.get("Name").unwrap_or("")),
                based_on: e.get("BasedOnMojikumiSet").unwrap_or("").into(),
                overrides,
            });
        }
        self.graphics(top);
        self.styles(top);
        if let Some(e) = top.iter().find(|e| e.local() == "FootnoteOption") {
            self.footnote_option(e);
        }
        for e in top.iter().filter(|e| e.local() == "Layer") {
            let id = LayerId(self.alloc());
            let color = match e.prop_el("LayerColor") {
                Some(c) if c.get("type") == Some("list") => {
                    let v: Vec<f64> = c.find_all("ListItem").filter_map(|i| i.text_content().trim().parse().ok()).collect();
                    if v.len() == 3 { [v[0] as u8, v[1] as u8, v[2] as u8] } else { LAYER_COLORS[0].1 }
                }
                Some(c) => names::layer_color_in(c.text_content().trim()).unwrap_or(LAYER_COLORS[self.layers.len() % LAYER_COLORS.len()].1),
                None => LAYER_COLORS[self.layers.len() % LAYER_COLORS.len()].1,
            };
            self.layers.push(Layer {
                id,
                name: e.get("Name").unwrap_or("Layer").to_string(),
                color,
                visible: e.get("Visible") != Some("false"),
                locked: e.get("Locked") == Some("true"),
                printable: e.get("Printable") != Some("false"),
                show_guides: e.get("ShowGuides") != Some("false"),
                suppress_wrap_when_hidden: e.get("IgnoreWrap") == Some("true"),
            });
            if let Some(s) = e.get("Self") {
                self.layer_ids.insert(s.to_string(), id);
            }
        }
        if self.layers.is_empty() {
            let id = LayerId(self.alloc());
            self.layers.push(Layer {
                id,
                name: "Layer 1".into(),
                color: LAYER_COLORS[0].1,
                visible: true,
                locked: false,
                printable: true,
                show_guides: true,
                suppress_wrap_when_hidden: false,
            });
        }
        for e in top.iter().filter(|e| e.local() == "CrossReferenceFormat") {
            self.xref_format(e);
        }
        if let Some(ix) = top.iter().find(|e| e.local() == "Index") {
            self.index_topics_of(ix, &[]);
        }
        // Stories (ids first so frames can reference them).
        for e in top.iter().filter(|e| e.local() == "Story") {
            let id = StoryId(self.alloc());
            if let Some(s) = e.get("Self") {
                self.story_ids.insert(s.to_string(), id);
            }
            let story = self.story(id, e);
            self.stories.insert(id, story);
        }
        // Topic cross-references (See / See also) become markers at the start of the first story
        // with text.
        if !self.index_xrefs.is_empty()
            && let Some(st) = self.stories.values_mut().find(|s| !s.text.is_empty())
        {
            for r in std::mem::take(&mut self.index_xrefs).into_iter().rev() {
                st.insert_index_ref(0, r);
            }
        }
        // Cross-references point at their hyperlink's destination.
        for h in top.iter().filter(|e| e.local() == "Hyperlink") {
            let (Some(src), Some(dest)) = (h.get("Source"), h.prop("Destination")) else { continue };
            let (Some(&(sid, k)), Some(&target)) = (self.xref_srcs.get(src), self.anchor_ids.get(dest.trim())) else { continue };
            if let Some(x) = self.stories.get_mut(&sid).and_then(|st| st.xrefs.get_mut(k)) {
                Arc::make_mut(x).target = target;
            }
        }
        // Parent spreads: ids first (pages reference other parents).
        let masters: Vec<&El> = top.iter().filter(|e| e.local() == "MasterSpread").collect();
        let mut master_ids = Vec::new();
        for m in &masters {
            let id = SpreadId(self.alloc());
            if let Some(s) = m.get("Self") {
                self.parent_ids.insert(s.to_string(), id);
            }
            master_ids.push(id);
        }
        for (m, id) in masters.iter().zip(master_ids) {
            let sp = self.spread(m, id, true);
            self.parents.push(sp);
        }
        for e in top.iter().filter(|e| e.local() == "Spread") {
            let id = SpreadId(self.alloc());
            let sp = self.spread(e, id, false);
            self.spreads.push(sp);
        }
        // Sections.
        for e in top.iter().filter(|e| e.local() == "Section") {
            let Some(start) = e.get("PageStart").and_then(|p| self.page_index.get(p)).copied() else { continue };
            let cont = e.get("ContinueNumbering") != Some("false");
            self.sections.push(Section {
                start,
                start_number: if cont && start > 0 { None } else { Some(e.num("PageNumberStart").unwrap_or(1.0).max(1.0) as u32) },
                style: e.prop("PageNumberStyle").map(|s| names::number_style_in(s.trim())).unwrap_or_default(),
                prefix: e.get("SectionPrefix").unwrap_or("").to_string(),
                marker: e.get("Marker").unwrap_or("").to_string(),
                include_prefix: e.get("IncludeSectionPrefix") == Some("true"),
            });
        }
        self.sections.sort_by_key(|s| s.start);
        self.sections.dedup_by_key(|s| s.start);
        if self.sections.first().is_none_or(|s| s.start != 0) {
            self.sections.insert(
                0,
                Section {
                    start: 0,
                    start_number: Some(1),
                    style: Default::default(),
                    prefix: String::new(),
                    marker: String::new(),
                    include_prefix: false,
                },
            );
        }
        self.link_threads();
        Ok(())
    }

    // ---------- swatches ----------

    // ---------- styles ----------

    // ---------- stories ----------

    // ---------- spreads and items ----------

    fn finish(mut self, root: &El) -> Result<Document> {
        let title = root.get("Name").map(|n| n.trim_end_matches(".indd").to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Untitled".into());
        if self.spreads.is_empty() {
            return Err(IdmlError::Invalid("document has no spreads".into()));
        }
        self.settings.lists = std::mem::take(&mut self.lists);
        // Make sure built-in paragraph styles exist and are first.
        let d = Document {
            title,
            settings: self.settings.clone(),
            spreads: std::mem::take(&mut self.spreads).into_iter().map(Arc::new).collect(),
            parents: std::mem::take(&mut self.parents).into_iter().map(Arc::new).collect(),
            layers: std::mem::take(&mut self.layers),
            stories: std::mem::take(&mut self.stories).into_iter().map(|(k, v)| (k, Arc::new(v))).collect(),
            styles: Arc::new(std::mem::take(&mut self.styles)),
            swatches: std::mem::take(&mut self.swatches),
            color_groups: std::mem::take(&mut self.color_groups),
            conditions: std::mem::take(&mut self.conditions),
            inks: std::mem::take(&mut self.inks),
            stroke_styles: std::mem::take(&mut self.stroke_styles),
            articles: vec![],
            xml: Default::default(),
            endnote_options: Default::default(),
            endnote_story: None,
            sections: std::mem::take(&mut self.sections),
            assets: std::mem::take(&mut self.assets),
            hyperlinks: vec![],
            data_merge: Default::default(),
            bookmarks: vec![],
            user_words: vec![],
            hyphenation_exceptions: vec![],
            toc: None,
            text_variables: designcraft_doc::vars::defaults(),
            footnote_options: std::mem::take(&mut self.footnote_options),
            xref_formats: std::mem::take(&mut self.xref_formats),
            index: None,
            created: designcraft_doc::vars::now(),
            modified: 0,
            next_id: self.next_id,
            font_scope: 0,
        };
        Ok(d)
    }
}

/// IDML link URI → file system path (`file:/a%20b` → `/a b`, `file:///C:/x` → `C:/x`).
pub(crate) fn uri_to_path(uri: &str) -> String {
    let rest = uri.strip_prefix("file://").or_else(|| uri.strip_prefix("file:")).unwrap_or(uri);
    // `file:///C:/…` → `/C:/…` → `C:/…`
    let rest = if rest.len() > 3 && rest.starts_with('/') && rest.as_bytes()[2] == b':' { &rest[1..] } else { rest };
    let mut out = Vec::with_capacity(rest.len());
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
