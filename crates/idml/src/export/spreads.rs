//! Write spreads and their page records.

use super::{Affine, El, Ex, PageSide, Rect, Spread, bool_s, num, spread_origin, uid};

impl Ex<'_> {
    pub(super) fn spread_el_doc(&mut self, sp: &Spread, si: usize) -> El {
        let first = self.d.first_page_of_spread(si);
        let mut el = self.spread_common(sp, false, first);
        el.name = "Spread".into();
        el
    }

    pub(super) fn spread_el(&mut self, sp: &Spread, _master: bool) -> El {
        let mut el = self.spread_common(sp, true, 0);
        el.name = "MasterSpread".into();
        el
    }

    pub(super) fn spread_common(&mut self, sp: &Spread, master: bool, first_page: usize) -> El {
        let d = self.d;
        let facing = d.settings.facing_pages;
        let (ox, oy) = spread_origin(sp, facing);
        let mut el = El::new("Spread").attr("Self", uid(sp.id.0));
        if let (true, Some(info)) = (master, &sp.parent) {
            el.set("Name", info.label());
            el.set("NamePrefix", &info.prefix);
            el.set("BaseName", &info.name);
        }
        el.set("ShowMasterItems", bool_s(sp.pages.iter().all(|p| p.show_parent_items)));
        el.set("PageCount", sp.pages.len());
        if !master {
            el.set("BindingLocation", sp.pages.iter().filter(|p| p.side == PageSide::Left).count());
            el.set("AllowPageShuffle", bool_s(sp.allow_shuffle));
        }
        el.set("ItemTransform", "1 0 0 1 0 0");
        let based_on = sp.parent.as_ref().and_then(|i| i.based_on);
        for (i, pg) in sp.pages.iter().enumerate() {
            let name = if master { sp.parent.as_ref().map(|i| i.prefix.clone()).unwrap_or_default() } else { d.page_name(first_page + i) };
            let applied = if master { based_on } else { pg.parent };
            let pe = El::new("Page")
                .attr("Self", uid(pg.id.0))
                .attr("Name", name)
                .attr("AppliedMaster", applied.map(|s| uid(s.0)).unwrap_or_else(|| "n".into()))
                .attr("OverrideList", "")
                .attr("GeometricBounds", format!("0 0 {} {}", num(pg.height), num(pg.width)))
                .attr("ItemTransform", format!("1 0 0 1 {} {}", num(pg.x - ox), num(-oy)))
                .child(margin_el(pg));
            el.push(pe);
        }
        let origin = Affine::translate((-ox, -oy));
        for it in &sp.items {
            let e = self.item_el(it, origin * it.xf);
            el.push(e);
        }
        el
    }
}

pub(super) fn margin_el(pg: &designcraft_doc::Page) -> El {
    let m = pg.margin_rect();
    let cols = designcraft_doc::column_rects(Rect::new(0.0, 0.0, m.width(), m.height()), pg.columns.count.max(1), pg.columns.gutter);
    let pos: Vec<String> = cols.iter().flat_map(|c| [num(c.x0), num(c.x1)]).collect();
    El::new("MarginPreference")
        .attr("ColumnCount", pg.columns.count.max(1))
        .attr("ColumnGutter", num(pg.columns.gutter))
        .attr("Top", num(pg.margins.top))
        .attr("Bottom", num(pg.margins.bottom))
        .attr("Left", num(pg.margins.inside))
        .attr("Right", num(pg.margins.outside))
        .attr("ColumnDirection", "Horizontal")
        .attr("ColumnsPositions", pos.join(" "))
}
