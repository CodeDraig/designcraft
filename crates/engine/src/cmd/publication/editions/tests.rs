#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::cmd::publication::tests::publication;
use std::io::Read;

fn read_zip(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}
fn check_links(xml: &str) {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut ids = std::collections::BTreeSet::new();
    let mut links = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                for attr in e.attributes() {
                    let attr = attr.unwrap();
                    let value = attr.unescape_value().unwrap().into_owned();
                    if attr.key.as_ref() == b"id" {
                        assert!(ids.insert(value.clone()), "duplicate id {value}");
                    }
                    if attr.key.as_ref() == b"href" && value.starts_with('#') {
                        links.push(value[1..].to_string());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    for link in links {
        assert!(ids.contains(&link), "missing destination {link}");
    }
    assert!(!xml.chars().any(|c| ('\u{e000}'..='\u{e1ff}').contains(&c)));
}

#[test]
fn edition_configuration_is_independent_transactional_and_persistent() {
    let (mut s, sid, _, print) = publication();
    s.execute("publication.numbering.mark", &json!({"story":sid,"pos":0,"kind":"heading"})).unwrap();
    s.execute("publication.generated.define", &json!({"name":"Contents","title":"Unique generated contents","query":{"kind":"heading"}})).unwrap();
    s.execute("publication.notes.stream", &json!({"name":"Sources"})).unwrap();
    s.execute("publication.note.insert", &json!({"stream":"Sources","story":sid,"at":4,"text":"Unique source note"})).unwrap();
    let content = view(&s).unwrap().root.references.sections["Contents"].content;
    let reading = s.execute("publication.edition.create", &json!({"name":"Reading"})).unwrap()["edition"].clone();
    s.execute("publication.activate", &json!({"edition":print})).unwrap();
    s.execute("publication.edition.configure",&json!({"edition":reading,"output":{"title":"Reader book","language":"cy","excludedSections":[content]},"notePolicies":{"Sources":{"placement":"omit"}},"witness":"B"})).unwrap();
    assert_eq!(view(&s).unwrap().edition, print);
    let before = s.doc().unwrap().clone();
    let read = s.execute("file.exportHtml", &json!({"edition":reading})).unwrap();
    let html = read["text"].as_str().unwrap();
    assert!(html.contains("<title>Reader book</title>"));
    assert!(!html.contains("Unique source note") && !html.contains("Unique generated contents"));
    assert_eq!(s.doc().unwrap().doc, before.doc);
    assert_eq!(s.doc().unwrap().selection, before.selection);
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len());
    let print_html = s.execute("file.exportHtml", &json!({"edition":print})).unwrap();
    assert!(print_html["text"].as_str().unwrap().contains("Unique source note"));
    assert!(print_html["text"].as_str().unwrap().contains("Unique generated contents"));
    let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let restored = designcraft_format::load(&bytes).unwrap();
    assert_eq!(restored.publication, s.doc().unwrap().doc.publication);
    let before = s.doc().unwrap().clone();
    assert!(
        s.execute("publication.edition.configure", &json!({"edition":reading,"name":"Should roll back","output":{"excludedSections":[999999]}}))
            .is_err()
    );
    assert_eq!(s.doc().unwrap().doc, before.doc);
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.execute("publication.edition.get", &json!({"edition":reading})).unwrap()["output"]["title"].is_null());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.execute("publication.edition.get", &json!({"edition":reading})).unwrap()["output"]["title"], "Reader book");
}

#[test]
fn whole_publication_semantic_export_preserves_rich_notes_witnesses_and_all_links() {
    let mut s = Session::new();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../textimport/tests/fixtures/tei-notes-apparatus.xml");
    s.execute("file.openTei", &json!({"path":path})).unwrap();
    let original = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    s.execute("publication.convert", &json!({"title":"Collected witnesses"})).unwrap();
    let first = view(&s).unwrap().component;
    s.execute("publication.addComponent", &json!({"name":"Second witnesses","base64":crate::cmd::file::base64_encode(&original)})).unwrap();
    s.execute("publication.edition.configure", &json!({"witness":"B","notePolicies":{"Footnotes":{"placement":"publicationEnd"}}})).unwrap();
    let root = view(&s).unwrap().root;
    assert_eq!(root.annotation_entries().unwrap().len(), 12);
    let before = s.doc().unwrap().clone();
    let export = s.execute("file.exportEpub", &json!({"edition":view(&s).unwrap().edition})).unwrap();
    let bytes = crate::cmd::file::base64_decode(export["base64"].as_str().unwrap());
    let text = read_zip(&bytes, "OEBPS/content.xhtml");
    check_links(&text);
    check_links(&read_zip(&bytes, "OEBPS/content.opf"));
    assert_eq!(text.matches("class=\"note component-").count(), 12);
    assert_eq!(text.matches("Recorded harvests:").count(), 2);
    assert_eq!(text.matches("<table>").count(), 2);
    assert_eq!(text.matches("role=\"heading\" aria-level=\"1\"").count(), 2);
    assert_eq!(text.matches("role=\"doc-backlink\"").count(), 12);
    assert_eq!(text.matches("https://tei-c.org/").count(), 2);
    assert!(text.contains("blue") && text.contains("cloudy") && text.contains("Three trees and a gate."));
    assert!(text.contains("images/0-") && text.contains("images/1-"));
    assert!(text.find(&format!("id=\"component-{}\"", first.0)).unwrap() < text.rfind("class=\"notes\"").unwrap());
    assert_eq!(s.doc().unwrap().doc, before.doc);
    assert_eq!(s.doc().unwrap().selection, before.selection);
    let pdf = s.execute("file.exportPdf", &json!({})).unwrap();
    let bytes = crate::cmd::file::base64_decode(pdf["base64"].as_str().unwrap());
    let pdf_file = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
    assert_eq!(pdf_file.pages().len(), pdf["pages"].as_u64().unwrap() as usize);
    assert!(pdf_file.pages().len() >= 4);
    let note_links: Vec<_> = pdf_links(bytes.clone()).into_iter().filter(|(_, label, _)| label == "a return to the chapter").collect();
    let first_pages = root.editions[&view(&s).unwrap().edition].components[&first].page_count();
    assert_eq!(note_links.len(), 2, "{note_links:?}");
    assert!(note_links.iter().any(|(from, _, to)| *from >= first_pages && *to == 0), "{note_links:?}");
    assert!(note_links.iter().any(|(_, _, to)| *to == first_pages), "{note_links:?}");
    let literal = String::from_utf8_lossy(&bytes);
    assert!(literal.contains("https://tei-c.org/"), "external links from gathered notes were lost");
    assert!(!pdf["warnings"].to_string().contains("anchor "), "{}", pdf["warnings"]);
    let fixed = s.execute("file.exportFixedEpub", &json!({"scope":"component"})).unwrap();
    let fixed_bytes = crate::cmd::file::base64_decode(fixed["base64"].as_str().unwrap());
    let mut searchable = String::new();
    for n in 1..=fixed["pages"].as_u64().unwrap() {
        searchable.push_str(&read_zip(&fixed_bytes, &format!("OEBPS/page{n}.xhtml")));
    }
    assert!(searchable.contains("class=\"link\"") && searchable.contains("https://tei-c.org/"));
    assert!(searchable.contains("Recorded harvests") && searchable.contains("cloudy"), "{searchable}");
}

fn pdf_links(bytes: Vec<u8>) -> Vec<(usize, String, usize)> {
    use hayro_syntax::object::{Array, Dict, String as PdfString};
    let pdf = hayro_syntax::Pdf::new(bytes).unwrap();
    let mut out = Vec::new();
    for (page, p) in pdf.pages().iter().enumerate() {
        if let Some(annotations) = p.raw().get::<Array<'_>>("Annots") {
            for a in annotations.iter::<Dict<'_>>() {
                let Some(dest) = a.get::<Array<'_>>("Dest").and_then(|a| a.iter::<Dict<'_>>().next()) else { continue };
                let target = pdf.pages().iter().position(|p| p.raw().obj_id() == dest.obj_id()).unwrap();
                let label = a
                    .get::<PdfString<'_>>("Contents")
                    .map(|s| {
                        let b = s.as_bytes();
                        if b.starts_with(&[0xfe, 0xff]) {
                            String::from_utf16_lossy(&b[2..].chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect::<Vec<_>>())
                        } else {
                            String::from_utf8_lossy(b).into_owned()
                        }
                    })
                    .unwrap_or_default();
                out.push((page, label, target));
            }
        }
    }
    out
}

#[test]
fn pdf_links_cross_chapters_and_strict_failure_preserves_editor_and_output() {
    let (mut s, sid, first, _) = publication();
    let second = s.execute("publication.addComponent", &json!({"title":"Second"})).unwrap()["component"].clone();
    let frame = s.execute("frame.create", &json!({"rect":[40,40,400,600],"content":"text","text":"Second destination"})).unwrap();
    let target = s.execute("publication.numbering.mark", &json!({"story":frame["story"],"pos":0,"kind":"heading"})).unwrap()["object"].clone();
    s.execute("publication.activate", &json!({"component":first})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    s.execute("publication.reference.insert", &json!({"query":{"ids":[target]},"template":"Title and page"})).unwrap();
    let v = view(&s).unwrap();
    let expected = v.root.editions[&v.edition].components[&first].page_count();
    let r = s.execute("file.exportPdf", &json!({})).unwrap();
    let links = pdf_links(crate::cmd::file::base64_decode(r["base64"].as_str().unwrap()));
    assert!(
        links
            .iter()
            .any(|(page, label, dest)| *page == 0 && label == &format!("Publication reference {}", target.as_u64().unwrap()) && *dest == expected),
        "{links:?}"
    );
    let before = s.doc().unwrap().clone();
    let name = format!(
        "designcraft-strict-{}-{}.pdf",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    );
    let path = std::env::temp_dir().join(name);
    assert!(s.execute("file.exportPdf", &json!({"pages":[1],"strict":true,"path":path})).is_err());
    assert!(!path.exists());
    assert_eq!(before.doc, s.doc().unwrap().doc);
    assert_eq!(before.selection, s.doc().unwrap().selection);
    assert_eq!(before.history.undo.len(), s.doc().unwrap().history.undo.len());
    assert_ne!(json!(first), second);
}
