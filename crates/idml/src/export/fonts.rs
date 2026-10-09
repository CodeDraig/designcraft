//! Collect used fonts and emit the IDML font resource.

use super::{El, Ex, bool_s, document, escape_id, num, p, pkg_root};

impl Ex<'_> {
    pub(super) fn note_font(&mut self, family: &str, style: &str) {
        self.fonts.entry(family.to_string()).or_default().insert(style.to_string());
    }

    pub(super) fn fonts_part(&mut self) -> Vec<u8> {
        let mut root = pkg_root("Fonts");
        for f in &self.d.styles.composite_fonts {
            let mut el = El::new("CompositeFont").attr("Self", format!("CompositeFont/{}", escape_id(&f.name))).attr("Name", &f.name);
            for (i, e) in f.entries.iter().enumerate() {
                self.note_font(&e.family, &e.style);
                el.push(
                    El::new("CompositeFontEntry")
                        .attr("Self", format!("dcComposite{}_{i}", escape_id(&f.name)))
                        .attr("Name", &e.name)
                        .attr("CustomCharacters", &e.characters)
                        .attr("FontStyle", &e.style)
                        .attr("RelativeSize", num(e.relative_size * 100.0))
                        .attr("HorizontalScale", num(e.horizontal_scale * 100.0))
                        .attr("VerticalScale", num(e.vertical_scale * 100.0))
                        .attr("BaselineShift", num(e.baseline_shift * 100.0))
                        .attr("ScaleOption", bool_s(e.scale_option))
                        .child(El::new("Properties").child(p("AppliedFont", "string", &e.family))),
                );
            }
            root.push(el);
        }
        let fonts = std::mem::take(&mut self.fonts);
        for (fam, styles) in &fonts {
            let fid = format!("di{:x}", {
                self.next += 1;
                self.next
            });
            let mut fe = El::new("FontFamily").attr("Self", &fid).attr("Name", fam);
            for s in styles {
                fe.push(
                    El::new("Font")
                        .attr("Self", format!("{fid}Fontn{fam} {s}"))
                        .attr("FontFamily", fam)
                        .attr("Name", format!("{fam} {s}"))
                        .attr("PostScriptName", format!("$ID/{}-{}", fam.replace(' ', ""), s.replace(' ', "")))
                        .attr("Status", "NotAvailable")
                        .attr("FontStyleName", s)
                        .attr("FontType", "OpenTypeTT")
                        .attr("WritingScript", "0")
                        .attr("FullName", format!("{fam} {s}"))
                        .attr("FullNameNative", format!("{fam} {s}"))
                        .attr("FontStyleNameNative", s)
                        .attr("PlatformName", "$ID/")
                        .attr("Version", "$ID/")
                        .attr("TypekitID", "$ID/"),
                );
            }
            root.push(fe);
        }
        self.fonts = fonts;
        document(&root)
    }
}
