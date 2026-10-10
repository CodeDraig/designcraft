use super::*;
use designcraft_doc::xref::clean;
const BOOK: &[u8] = include_bytes!("../../tests/fixtures/tei-book.xml");
#[test]
fn structured_readings_and_rich_note_fixture_keep_their_source() {
    let bytes = include_bytes!("../../tests/fixtures/tei-notes-apparatus.xml");
    let imported = import("notes.xml", bytes, &Default::default()).unwrap();
    assert_eq!(imported.story.notes.len(), 3);
    assert_eq!(imported.story.endnotes.len(), 3);
    let a = imported.story.endnotes[0].apparatus.as_ref().unwrap();
    assert_eq!(a.source_key, "colour");
    assert_eq!(a.choose("B"), Some(1));
    assert_eq!(a.readings[0].witnesses, ["A"]);
    assert_eq!(a.readings[1].witness_labels, ["Second manuscript"]);
    assert_eq!(a.readings[1].text.char_format_at(0).over.font_style.as_deref(), Some("Italic"));
    assert!(!a.edited(&imported.story));
    assert_eq!(clean(&imported.story.text[a.range(&imported.story).unwrap()]), "red");
    assert_eq!(imported.story.notes[1].text.tables.len(), 1);
    assert_eq!(imported.story.notes[2].text.objects.len(), 1);
    let payload = imported.tei.unwrap();
    assert_eq!(payload.links.len(), 2);
    assert_eq!(payload.source.bytes.as_slice(), bytes);
}
fn wrap(body: &str) -> String {
    format!(r#"<TEI xmlns="{NS}"><text><body>{body}</body></text></TEI>"#)
}
fn read(body: &str) -> Imported {
    import("test.tei", wrap(body).as_bytes(), &Default::default()).unwrap()
}

#[test]
fn full_book_content_and_original_are_preserved() {
    let i = import("book.xml", BOOK, &Default::default()).unwrap();
    i.story.check().unwrap();
    let t = clean(&i.story.text);
    for text in [
        "The gate opened",
        "old orchard",
        "Doctor Vale",
        "silver",
        "threshold",
        "A restored sentence.",
        "Under glass",
        "KEEPER — guardian",
        "The last page.",
    ] {
        assert!(t.contains(text), "missing {text}: {t}");
    }
    for text in ["gaet", "olde", "golden", "discarded"] {
        assert!(!t.contains(text), "alternative leaked: {text}");
    }
    assert_eq!(i.story.notes.len(), 1);
    assert!(i.story.notes[0].text.text.contains("\nIts handle"));
    assert_eq!(i.story.endnotes.len(), 3);
    assert!(i.story.endnotes[0].text.text.contains("B (Second manuscript): golden"));
    let table = i.story.tables.values().next().unwrap();
    assert_eq!(table.cell(0, 0).unwrap().col_span, 2);
    assert_eq!(table.cell(1, 0).unwrap().row_span, 2);
    let p = i.tei.unwrap();
    assert_eq!(*p.source.bytes, BOOK);
    assert_eq!(p.source.metadata[0].titles, ["The Glass Orchard"]);
    assert_eq!(p.graphics.len(), 1);
    assert_eq!(p.links.len(), 2);
    assert_eq!(p.source.navigation.iter().filter(|n| n.kind == "sourcePage").count(), 2);
    assert!(p.source.diagnostics.iter().any(|d| d.code == "specialized-content"));
}
#[test]
fn mixed_content_whitespace_entities_and_prefixes() {
    let i = read("<p>A <hi>bold</hi> word &amp; &#xE9;.</p><p xml:space=\"preserve\"> a  b </p><p>split<lb break=\"no\"/> \n word</p>");
    assert_eq!(i.story.text.replace(designcraft_doc::ANCHOR_MARK, ""), "A bold word & é.\n a  b \nsplitword");
    let boundary = read(r#"<p>A <hi xml:space="preserve">b</hi>C</p>"#);
    assert_eq!(clean(&boundary.story.text), "A bC");
    let xml = format!(r#"<t:TEI xmlns:t="{NS}"><t:text><t:body><t:p>prefix</t:p></t:body></t:text></t:TEI>"#);
    assert!(is_tei(xml.as_bytes()));
    assert!(clean(&import("x", xml.as_bytes(), &Default::default()).unwrap().story.text).contains("prefix"));
}
#[test]
fn witness_selection_and_fallback_are_explicit() {
    let x = wrap(r##"<p><app><lem wit="#A">one</lem><rdg wit="#B">two</rdg></app> <app><rdg>three</rdg><rdg>four</rdg></app></p>"##);
    let i = import("x", x.as_bytes(), &TeiOptions { witness: Some("B".into()) }).unwrap();
    let t = clean(&i.story.text);
    assert_eq!(t, "two three");
    assert!(i.tei.unwrap().source.diagnostics.iter().any(|d| d.code == "witness-fallback"));
}
#[test]
fn malformed_unsupported_and_ambiguous_input_fail() {
    for xml in
        ["<TEI>", "<TEI/>", &wrap("<p>&missing;</p>"), &wrap("<p xml:id=\"same\"/><p xml:id=\"same\"/>"), &wrap("<p><hi></p>"), &wrap("<p>&#0;</p>")]
    {
        assert!(import("x.tei", xml.as_bytes(), &Default::default()).is_err(), "accepted {xml}");
    }
    let deep = format!("{}x{}", "<seg>".repeat(130), "</seg>".repeat(130));
    assert!(import("x", wrap(&deep).as_bytes(), &Default::default()).is_err());
}
#[test]
fn malformed_xml_declarations_names_and_character_references_fail() {
    for fragment in [
        r#"<?xml version="1.0"?><p>misplaced declaration</p>"#,
        "<p>&#xffff;</p>",
        r#"<p n="&#xfffe;">invalid attribute reference</p>"#,
        r#"<p n="less<than">invalid attribute</p>"#,
        "<1invalid>bad name</1invalid>",
        "<p>unescaped ]]></p>",
        "<!DOCTYPE TEI><p>misplaced doctype</p>",
        "<?XML illegal?><p>invalid PI</p>",
    ] {
        let xml = wrap(fragment);
        assert!(import("bad.tei", xml.as_bytes(), &Default::default()).is_err(), "accepted {xml}");
    }
    for prefix in [
        r#"<?xml version="1.0" version="1.0"?>"#,
        r#"<?xml version="1.0" standalone="maybe"?>"#,
        r#" <?xml version="1.0"?>"#,
        "<![CDATA[ ]]>",
        "&#32;",
        "\u{a0}",
    ] {
        let xml = format!("{prefix}{}", wrap("<p>text</p>"));
        assert!(import("bad.tei", xml.as_bytes(), &Default::default()).is_err(), "accepted {xml}");
    }
    let good = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>{}"#, wrap("<p xml:id=\"cân-1\">&#x1F34E;</p>"));
    assert_eq!(clean(&import("good.tei", good.as_bytes(), &Default::default()).unwrap().story.text), "🍎");
}
#[test]
fn corpus_and_utf16() {
    let xml = format!(
        r#"<teiCorpus xmlns="{NS}"><TEI><text><body><p>First</p></body></text></TEI><TEI><text><body><p>Second</p></body></text></TEI></teiCorpus>"#
    );
    let mut bytes = vec![0xff, 0xfe];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let i = import("corpus.xml", &bytes, &Default::default()).unwrap();
    assert_eq!(clean(&i.story.text), "First\nSecond");
    assert_eq!(i.tei.unwrap().source.navigation.iter().filter(|n| n.kind == "book").count(), 2);
}

#[test]
fn encoding_declarations_must_match_bytes() {
    for xml in [
        r#"<?xml version="1.0" encoding="UTF-16"?><TEI xmlns="http://www.tei-c.org/ns/1.0"/>"#,
        r#"<?xml version="1.0" encoding="US-ASCII"?><TEI xmlns="http://www.tei-c.org/ns/1.0"><text>é</text></TEI>"#,
    ] {
        assert!(super::import("encoding.tei", xml.as_bytes(), &Default::default()).is_err());
    }
}

#[test]
fn groups_lists_and_unknown_wrappers_keep_reading_order() {
    let xml = wrap(
        r##"<head><choice><sic>Worng</sic><corr>Right</corr></choice></head><list type="ordered"><item>First</item><item>Second</item></list><p><app><lem>base</lem><rdgGrp wit="#B"><rdg>group reading</rdg></rdgGrp></app><strange> one <hi>copy</hi></strange></p>"##,
    );
    let i = import("groups.tei", xml.as_bytes(), &TeiOptions { witness: Some("B".into()) }).unwrap();
    assert_eq!(clean(&i.story.text), "Right\n1. First\n2. Second\ngroup reading one copy");
    let p = i.tei.unwrap();
    assert!(p.source.navigation.iter().any(|n| n.kind == "heading" && n.label == "Right"));
    assert!(p.source.diagnostics.iter().any(|d| d.code == "unmapped-element"));
}
#[test]
fn typed_ab_headings_and_reading_master_renditions() {
    let i = read(
        r#"<div><ab type="heading" rend="title">A Reading Master</ab><ab type="heading" rend="heading" xml:id="intro">Introduction</ab><p rend="p">Body text.</p><p rend="quotation">A quotation.</p><p rend="small">Small print.</p><ab type="heading" rend="versehead">The speaker</ab><lg><l>A verse line.</l></lg><div><ab type="heading">Nested heading</ab></div></div>"#,
    );
    assert_eq!(
        clean(&i.story.text),
        "A Reading Master\nIntroduction\nBody text.\nA quotation.\nSmall print.\nThe speaker\nA verse line.\nNested heading"
    );
    assert_eq!(
        i.story.paras.iter().map(|p| p.style.as_str()).collect::<Vec<_>>(),
        ["TEI Title", "TEI Heading 1", "TEI Body", "TEI Quote", "TEI Small", "TEI Verse Heading", "TEI Verse", "TEI Heading 2"]
    );
    let src = i.tei.unwrap().source;
    assert!(src.diagnostics.is_empty(), "{:?}", src.diagnostics);
    let headings: Vec<_> = src.navigation.iter().filter(|n| n.kind == "heading").collect();
    assert_eq!(
        headings.iter().map(|n| (n.label.as_str(), n.level)).collect::<Vec<_>>(),
        [("A Reading Master", 1), ("Introduction", 1), ("The speaker", 1), ("Nested heading", 2)]
    );
    assert!(headings.iter().all(|n| i.story.anchors.iter().any(|a| a.id == n.anchor)));
}
#[test]
fn resource_bases_external_entities_and_limits_are_explicit() {
    let i = read(
        r#"<div xml:base="plates/"><figure xml:base="sub/"><graphic url="orchard.svg"/></figure><include xmlns="http://www.w3.org/2001/XInclude" href="https://example.invalid/no-fetch"/></div>"#,
    );
    let p = i.tei.unwrap();
    assert_eq!(p.graphics[0].url, "plates/sub/orchard.svg");
    assert!(p.source.diagnostics.iter().any(|d| d.code == "foreign-element"));
    let external = format!(r#"<!DOCTYPE TEI [<!ENTITY secret SYSTEM "file:///never-read">]>{}"#, wrap("<p>&secret;</p>"));
    assert!(import("external.tei", external.as_bytes(), &Default::default()).is_err());
    assert!(import("too-large.tei", &vec![b' '; MAX_BYTES + 1], &Default::default()).is_err());
    assert!(import("table.tei", wrap(r#"<table><row><cell cols="999">Too wide</cell></row></table>"#).as_bytes(), &Default::default()).is_err());
}
