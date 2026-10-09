//! Map document and parent spreads and their page geometry.

use super::{
    Affine, Arc, Columns, El, Guide, Importer, ItemId, Margins, Page, PageId, PageSide, ParentInfo, Point, Rect, Spread, SpreadId, margins_of, nums,
    parse_affine,
};

impl Importer<'_> {
    pub(super) fn spread(&mut self, e: &El, id: SpreadId, master: bool) -> Spread {
        let facing = self.settings.facing_pages;
        // Page rectangles in IDML spread coordinates.
        let mut pages: Vec<(Rect, &El)> = Vec::new();
        for p in e.find_all("Page") {
            let gb = nums(p.get("GeometricBounds").unwrap_or(""));
            let (t, l, bt, r) =
                if gb.len() == 4 { (gb[0], gb[1], gb[2], gb[3]) } else { (0.0, 0.0, self.settings.page_height, self.settings.page_width) };
            let xf = parse_affine(p.get("ItemTransform"));
            let a = xf * Point::new(l, t);
            let c = xf * Point::new(r, bt);
            pages.push((Rect::from_points(a, c), p));
        }
        let min_x = pages.iter().map(|(r, _)| r.x0).fold(f64::INFINITY, f64::min);
        let min_y = pages.iter().map(|(r, _)| r.y0).fold(f64::INFINITY, f64::min);
        let (min_x, min_y) = if min_x.is_finite() { (min_x, min_y) } else { (0.0, 0.0) };
        let shift = Affine::translate((-min_x, -min_y));
        let show = e.get("ShowMasterItems") != Some("false");
        let first_abs: usize = self.spreads.iter().map(|s| s.pages.len()).sum();
        let mut out_pages = Vec::new();
        for (i, (r, p)) in pages.iter().enumerate() {
            let pid = PageId(self.alloc());
            if let Some(s) = p.get("Self")
                && !master
            {
                self.page_index.insert(s.to_string(), first_abs + i);
            }
            let (margins, columns) = match p.find("MarginPreference") {
                Some(m) => margins_of(m),
                None => self.default_margins.clone().unwrap_or((Margins::uniform(36.0), Columns::default())),
            };
            let side = if !facing {
                PageSide::Single
            } else if r.x1 <= 1e-6 {
                PageSide::Left
            } else {
                PageSide::Right
            };
            let parent = p.get("AppliedMaster").and_then(|m| self.parent_ids.get(m)).copied();
            out_pages.push(Page {
                id: pid,
                width: r.width(),
                height: r.height(),
                x: r.x0 - min_x,
                margins,
                columns,
                parent,
                side,
                overridden: vec![],
                guides: Vec::<Guide>::new(),
                show_parent_items: show,
                liquid: Default::default(),
                transition: None,
                view_rotation: 0,
            });
        }
        let overrides: Vec<Vec<String>> =
            pages.iter().map(|(_, p)| p.get("OverrideList").unwrap_or("").split_whitespace().map(str::to_string).collect()).collect();
        let mut items = Vec::new();
        for c in e.elements() {
            if let Some(it) = self.item(c, shift) {
                items.push(Arc::new(it));
            }
        }
        for (pg, ov) in out_pages.iter_mut().zip(overrides) {
            pg.overridden = ov.iter().filter_map(|s| self.item_ids.get(s)).copied().filter(|i| self.is_parent_item(*i)).collect();
        }
        let parent = if master {
            let prefix =
                e.get("NamePrefix").map(str::to_string).unwrap_or_else(|| e.get("Name").unwrap_or("A").split('-').next().unwrap_or("A").to_string());
            let name = e.get("BaseName").map(str::to_string).unwrap_or_else(|| {
                let n = e.get("Name").unwrap_or("Parent");
                n.split_once('-').map(|(_, b)| b.to_string()).unwrap_or_else(|| n.to_string())
            });
            let based_on = pages.first().and_then(|(_, p)| p.get("AppliedMaster")).and_then(|m| self.parent_ids.get(m)).copied().filter(|b| *b != id);
            Some(ParentInfo { prefix, name, based_on })
        } else {
            None
        };
        for p in &mut out_pages {
            if master {
                p.parent = None;
            }
        }
        Spread { id, pages: out_pages, items, parent, allow_shuffle: e.get("AllowPageShuffle") != Some("false") }
    }

    pub(super) fn is_parent_item(&self, id: ItemId) -> bool {
        self.parents.iter().any(|p| {
            p.items.iter().any(|it| {
                let mut found = false;
                it.walk(&mut |i| found |= i.id == id);
                found
            })
        })
    }
}
