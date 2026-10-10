#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use designcraft_doc::{Asset, AssetId, ParaFormat, SpreadRef, StoryId, geom::Rect, publication::ComponentRole, tei::TeiSource};

fn fixture() -> (Document, StoryId) {
    let mut d = Document::new(&Default::default());
    let layer = d.default_layer();
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(40.0, 40.0, 300.0, 500.0), layer, "Text\nwith notes", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().insert_note(0, "note", ParaFormat::default());
    let bytes = Arc::new(b"<TEI xmlns=\"http://www.tei-c.org/ns/1.0\">\r\n</TEI>".to_vec());
    d.tei_sources.push(Arc::new(TeiSource {
        id: 9,
        name: "original.xml".into(),
        byte_length: bytes.len(),
        bytes,
        metadata: vec![],
        options: Default::default(),
        import_settings: Default::default(),
        diagnostics: vec![],
        navigation: vec![],
        resources: vec![],
    }));
    let id = AssetId(d.alloc());
    d.assets.insert(
        id,
        Arc::new(Asset {
            id,
            name: "image.png".into(),
            mime: "image/png".into(),
            data: Arc::new(vec![1, 2, 3, 4]),
            pixels: Some((1, 1)),
            page: 0,
            link: None,
        }),
    );
    let view = Publication::from_document(&d, "Complete book").unwrap();
    let mut root = (*view.root).clone();
    root.add_document(&d, ComponentRole::BackMatter, None).unwrap();
    root.duplicate_edition(view.edition, "Reading").unwrap();
    (PublicationView { root: Arc::new(root), ..view }.project().unwrap(), sid)
}

#[test]
fn archive_preserves_every_component_edition_source_and_asset() {
    let (d, sid) = fixture();
    let before = d.publication.as_ref().unwrap();
    let bytes = super::super::save(&d).unwrap();
    let reopened = super::super::load(&bytes).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.story(sid).unwrap().notes[0].text.text, "note");
    let root = &reopened.publication.as_ref().unwrap().root;
    for (id, original) in &before.root.sources {
        assert_eq!(root.sources[id].bytes, original.bytes);
    }
    for (id, original) in &before.root.assets {
        assert_eq!(root.assets[id].data, original.data);
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("document.json").is_err());
    let mut meta = String::new();
    zip.by_name("meta.json").unwrap().read_to_string(&mut meta).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&meta).unwrap()["version"], 3);
}

fn without(bytes: &[u8], omit: &str) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut target = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut f = source.by_index(i).unwrap();
        if f.name() == omit {
            continue;
        }
        target.start_file(f.name(), zip::write::SimpleFileOptions::default()).unwrap();
        std::io::copy(&mut f, &mut target).unwrap();
    }
    target.finish().unwrap().into_inner()
}

#[test]
fn missing_or_corrupted_required_attachments_are_rejected() {
    let (doc, _) = fixture();
    let bytes = super::super::save(&doc).unwrap();
    let root = &doc.publication.as_ref().unwrap().root;
    let source = root.sources.keys().next().unwrap();
    let asset = root.assets.keys().next().unwrap();
    for name in [format!("sources/{}.xml", source.0), format!("assets/{}.png", asset.0), "publication.json".into()] {
        assert!(super::super::load(&without(&bytes, &name)).is_err(), "{name}");
    }
    let mut missing = doc.clone();
    Arc::make_mut(&mut missing.tei_sources[0]).bytes = Arc::new(Vec::new());
    assert!(super::super::save(&missing).is_err());
}

#[test]
fn early_version_three_review_migrates_only_when_section_was_absent() {
    let (mut doc, sid) = fixture();
    doc.story_mut(sid).unwrap().format_chars(3..7, |f| f.over.change = Some(designcraft_doc::ChangeMark::Inserted));
    doc.sync_publication().unwrap();
    let original = super::super::save(&doc).unwrap();
    for legacy in [true, false] {
        let mut source = zip::ZipArchive::new(Cursor::new(&original)).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..source.len() {
            let mut entry = source.by_index(i).unwrap();
            output.start_file(entry.name(), zip::write::SimpleFileOptions::default()).unwrap();
            if entry.name() == "publication.json" {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                fn clear(v: &mut serde_json::Value) {
                    match v {
                        serde_json::Value::Object(o) => {
                            o.remove("changeId");
                            for v in o.values_mut() {
                                clear(v);
                            }
                        }
                        serde_json::Value::Array(v) => {
                            for v in v {
                                clear(v);
                            }
                        }
                        _ => {}
                    }
                }
                clear(&mut json);
                if legacy {
                    json["publication"].as_object_mut().unwrap().remove("review");
                }
                output.write_all(&serde_json::to_vec(&json).unwrap()).unwrap();
            } else {
                std::io::copy(&mut entry, &mut output).unwrap();
            }
        }
        let bytes = output.finish().unwrap().into_inner();
        let result = super::super::load(&bytes);
        if legacy {
            let root = result.unwrap().publication.unwrap().root;
            assert!(!root.change_ranges().unwrap().is_empty());
            assert!(root.change_ranges().unwrap().iter().all(|r| r.id > 0));
        } else {
            assert!(result.is_err(), "a present but corrupt review section must not be repaired silently");
        }
    }
}
