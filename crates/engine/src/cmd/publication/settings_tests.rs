#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::tests::publication;
use super::*;

#[test]
fn shared_values_and_variable_definitions_survive_editions_edits_and_save() {
    let (mut s, sid, component, print) = publication();
    let reading = s.execute("publication.edition.create", &json!({"name":"Reading"})).unwrap()["edition"].clone();
    s.execute("publication.metadata", &json!({"title":"Collected Works","metadata":{"author":"E. Jones"},"values":{"imprint":"First"}})).unwrap();
    s.execute("variables.define", &json!({"name":"Imprint","type":"publicationValue","key":"imprint"})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    s.execute("variables.insert", &json!({"name":"Imprint"})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let old = s.cache.get(&doc, sid, None);
    s.execute("publication.metadata", &json!({"values":{"imprint":"Second edition of the collected works"}})).unwrap();
    let current = s.doc().unwrap().doc.clone();
    let now = s.cache.get(&current, sid, None);
    assert!(!Arc::ptr_eq(&old, &now), "changing a shared value invalidates composed variable text");
    s.execute("publication.activate", &json!({"edition":print})).unwrap();
    let doc = &s.doc().unwrap().doc;
    assert_eq!(doc.variable_value(doc.variable("Imprint").unwrap(), None).unwrap(), "Second edition of the collected works");
    assert!(doc.story(sid).unwrap().text.starts_with(designcraft_doc::vars::var_char(doc.variable("Imprint").unwrap()).unwrap()));
    let bytes = designcraft_format::save(doc).unwrap();
    let restored = designcraft_format::load(&bytes).unwrap();
    let v = restored.publication.unwrap();
    assert_eq!(v.root.title, "Collected Works");
    let d =
        designcraft_doc::publication::PublicationView { edition: EditionId(reading.as_u64().unwrap()), component, root: v.root }.project().unwrap();
    assert_eq!(d.variable_value(d.variable("Imprint").unwrap(), None).unwrap(), "Second edition of the collected works");
}

#[test]
fn style_preview_preserves_imported_overrides_and_selectively_updates_one_edition() {
    let (mut s, sid, source, print) = publication();
    s.execute("style.paragraph.create", &json!({"name":"Body","chars":{"size":12}})).unwrap();
    let mut target = view(&s).unwrap().root.project(print, source).unwrap();
    target.title = "Override chapter".into();
    target.styles_mut().para_mut("Body").unwrap().chars.size = Some(24.0);
    target.story_mut(sid).unwrap().paras[0].style = "Body".into();
    target.story_mut(sid).unwrap().paras[0].chars.tracking = Some(35.0);
    let bytes = designcraft_format::save(&target).unwrap();
    let target = s.execute("publication.addComponent", &json!({"name":"override.designcraft","base64":super::super::base64_encode(&bytes)})).unwrap()
        ["component"]
        .clone();
    let reading = s.execute("publication.edition.create", &json!({"name":"Reading"})).unwrap()["edition"].clone();
    s.execute("publication.activate", &json!({"edition":print})).unwrap();
    let preview = s.execute("publication.styles.preview", &json!({"source":source})).unwrap();
    let row = preview["rows"].as_array().unwrap().iter().find(|r| r["key"] == "paragraph:Body").unwrap();
    assert_eq!(row["overridden"], true);
    let before = s.doc().unwrap().clone();
    let params = json!({"preview":preview["preview"],"rows":[row["row"]]});
    assert!(s.execute("publication.styles.apply", &params).is_err());
    assert!(Arc::ptr_eq(&before.doc, &s.doc().unwrap().doc));
    assert_eq!(s.execute("publication.styles.apply", &json!({"preview":preview["preview"]})).unwrap()["applied"], 0);
    let mut params = params;
    params["replaceOverrides"] = json!(true);
    s.execute("publication.styles.apply", &params).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    assert_eq!(s.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(12.0));
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().paras[0].chars.tracking, Some(35.0));
    s.execute("publication.activate", &json!({"edition":reading})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(24.0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(24.0));
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("publication.activate", &json!({"edition":print})).unwrap();
    // Editing content must not discard the synchronization baseline.
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    s.execute("text.insert", &json!({"text":"Edited "})).unwrap();
    s.execute("publication.activate", &json!({"component":source})).unwrap();
    s.execute("style.paragraph.edit", &json!({"name":"Body","chars":{"size":14}})).unwrap();
    let preview = s.execute("publication.styles.preview", &json!({"source":source,"components":[target]})).unwrap();
    let row = preview["rows"].as_array().unwrap().iter().find(|r| r["key"] == "paragraph:Body").unwrap();
    assert_eq!(row["overridden"], false, "previously adopted, unchanged target can follow its source");
    s.execute("publication.metadata", &json!({"values":{"imprint":"Changed after preview"}})).unwrap();
    let before = s.doc().unwrap().clone();
    assert!(s.execute("publication.styles.apply", &json!({"preview":preview["preview"]})).unwrap_err().to_string().contains("stale"));
    assert!(Arc::ptr_eq(&before.doc, &s.doc().unwrap().doc));
    assert_eq!(before.history.undo.len(), s.doc().unwrap().history.undo.len());
    let preview = s.execute("publication.styles.preview", &json!({"source":source,"components":[target]})).unwrap();
    s.execute("publication.styles.apply", &json!({"preview":preview["preview"]})).unwrap();
    s.execute("publication.activate", &json!({"component":target})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(14.0));
}

#[test]
fn numbering_follows_tree_and_text_order_with_independent_scopes_and_missing_targets() {
    let (mut s, sid, first, edition) = publication();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":13})).unwrap();
    s.execute("text.insert", &json!({"text":"Alpha\nBeta"})).unwrap();
    let beta = s.execute("publication.numbering.mark", &json!({"kind":"figure","story":sid,"pos":6})).unwrap()["object"].clone();
    let alpha = s.execute("publication.numbering.mark", &json!({"kind":"figure","story":sid,"pos":0})).unwrap()["object"].clone();
    s.execute("publication.numbering.define", &json!({"name":"Figures","kind":"figure","prefix":"Fig. ","restart":"component"})).unwrap();
    let numbered = s.execute("publication.numbering.resolve", &json!({"name":"Figures"})).unwrap();
    assert_eq!(numbered[0]["object"], alpha);
    assert_eq!(numbered[1]["object"], beta);
    assert_eq!(numbered[0]["label"], "Fig. 1");
    assert_eq!(numbered[1]["title"], "Beta");
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":0})).unwrap();
    s.execute("text.insert", &json!({"text":"New introductory paragraph\n"})).unwrap();
    assert_eq!(s.execute("publication.numbering.resolve", &json!({"name":"Figures"})).unwrap()[0]["title"], "Alpha");
    let front = s.execute("publication.addComponent", &json!({"title":"Preface","role":"frontMatter"})).unwrap()["component"].clone();
    let second = s.execute("publication.addComponent", &json!({"title":"Chapter two"})).unwrap()["component"].clone();
    s.execute("publication.numbering.define", &json!({"name":"Chapters","role":"chapter","prefix":"Chapter "})).unwrap();
    s.execute("publication.numbering.define", &json!({"name":"Front","role":"frontMatter","style":"lowerRoman"})).unwrap();
    assert_eq!(s.execute("publication.numbering.resolve", &json!({"name":"Front"})).unwrap()[0]["label"], "i");
    s.execute("publication.moveComponent", &json!({"component":second,"at":0})).unwrap();
    let numbered = s.execute("publication.numbering.resolve", &json!({"name":"Chapters"})).unwrap();
    assert_eq!(numbered[0]["component"], second);
    assert_eq!(numbered[1]["component"], json!(first));
    s.execute("publication.numbering.define", &json!({"name":"Scoped","scope":front,"kind":"chapter","start":5})).unwrap();
    assert_eq!(s.execute("publication.numbering.resolve", &json!({"name":"Scoped"})).unwrap()[0]["label"], "5");
    s.execute("publication.activate", &json!({"component":first,"edition":edition})).unwrap();
    let anchor = view(&s).unwrap().root.semantic[&designcraft_doc::SemanticId(beta.as_u64().unwrap())].target.anchor;
    let at = s.doc().unwrap().doc.story(sid).unwrap().anchor_pos(anchor).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":at,"focus":at+3})).unwrap();
    s.execute("text.insert", &json!({"text":""})).unwrap();
    let numbered = s.execute("publication.numbering.resolve", &json!({"name":"Figures"})).unwrap();
    assert!(numbered.as_array().unwrap().iter().any(|r| r["object"] == beta && r["resolved"] == false && r["number"].is_null()));
    let bytes = designcraft_format::save(&s.doc().unwrap().doc).unwrap();
    let restored = designcraft_format::load(&bytes).unwrap();
    assert_eq!(restored.publication.as_ref().unwrap().root.numbering.len(), 4);
    assert_eq!(restored.publication.as_ref().unwrap().root.semantic.len(), 2);
}

#[test]
fn numbered_note_passages_use_nested_anchors_and_reject_unbounded_labels() {
    let (mut s, sid, _, _) = publication();
    s.execute("text.select", &json!({"story":sid,"anchor":4,"focus":4})).unwrap();
    s.execute("footnote.insert", &json!({"text":"Nested caption"})).unwrap();
    s.execute("publication.numbering.mark", &json!({"kind":"figure"})).unwrap();
    s.execute("publication.numbering.define", &json!({"name":"Figures","kind":"figure"})).unwrap();
    let list = s.execute("publication.numbering.resolve", &json!({"name":"Figures"})).unwrap();
    assert_eq!(list[0]["title"], "Nested caption");
    assert_eq!(list[0]["resolved"], true);
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("publication.numbering.define", &json!({"name":"Figures","kind":"figure","style":"symbols","start":100000})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}

#[test]
fn insert_bound_publication_values_and_numbers_is_one_edit_and_reuses_definitions() {
    let (mut s, sid, component, _) = publication();
    s.execute("publication.metadata", &json!({"values":{"publisher":"Small Press"}})).unwrap();
    s.execute("publication.numbering.define", &json!({"name":"Chapters","prefix":"Chapter "})).unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":8})).unwrap();
    let before = s.doc().unwrap().clone();
    let var = s.execute("publication.value.insert", &json!({"key":"publisher"})).unwrap()["variable"].as_u64().unwrap() as usize;
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    assert_eq!(s.doc().unwrap().doc.variable_value(var, None).unwrap(), "Small Press");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before.doc);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.execute("publication.value.insert", &json!({"key":"publisher"})).unwrap()["variable"], json!(var));
    let chapter = s.execute("publication.numbering.insert", &json!({"name":"Chapters"})).unwrap()["variable"].as_u64().unwrap() as usize;
    assert_eq!(s.doc().unwrap().doc.variable_value(chapter, None).unwrap(), "Chapter 1");
    let next = s.execute("publication.addComponent", &json!({"title":"Before"})).unwrap()["component"].clone();
    s.execute("publication.moveComponent", &json!({"component":next,"at":0})).unwrap();
    s.execute("publication.activate", &json!({"component":component})).unwrap();
    assert_eq!(s.doc().unwrap().doc.variable_value(chapter, None).unwrap(), "Chapter 2");
}
