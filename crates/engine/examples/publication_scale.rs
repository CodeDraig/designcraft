//! Reproducible R8 acceptance. Run with a NEW output directory; never replaces prior evidence.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../tests/support/publication_fixture.rs"]
mod fixture;
use designcraft_engine::Session;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Instant};
fn command(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}
fn metrics(s: &mut Session, reset: bool) -> Value {
    command(s, "publication.layout.metrics", json!({"reset":reset}))
}
fn evidence(s: &mut Session, name: &str, started: Instant) -> Value {
    let elapsed = started.elapsed().as_secs_f64();
    let work = metrics(s, false);
    let start = Instant::now();
    let oracle = command(s, "publication.layout.verify", json!({}));
    let result = json!({"stage":name,"elapsedSeconds":elapsed,"oracleSeconds":start.elapsed().as_secs_f64(),"pages":fixture::page_count(&s.doc().unwrap().doc),"work":work,"oracle":oracle});
    println!(
        "{name}: {} pages, {} recomposed paragraphs, {} frames; {:.3}s",
        result["pages"], result["work"]["paragraphs"], result["work"]["frames"], elapsed
    );
    result
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("give a new acceptance output directory")?);
    std::fs::create_dir(&out)?;
    let mut stages = Vec::new();
    let started = Instant::now();
    let mut s = fixture::session(fixture::book(12, 75));
    println!("Generated 12 chapters, 900 sections, 3600 references in {:.3}s", started.elapsed().as_secs_f64());
    let started = Instant::now();
    command(&mut s, "publication.update", json!({}));
    stages.push(evidence(&mut s, "cold", started));
    assert_eq!(stages[0]["pages"], 900);
    command(&mut s, "file.save", json!({"path":out.join("baseline.designcraft")}));
    let v = s.doc()?.doc.publication.clone().unwrap();
    let component = v.root.ordered_components()?[5];
    command(&mut s, "publication.activate", json!({"component":component}));
    let sid = s.doc()?.doc.settings.primary_story.unwrap();
    let at = s.doc()?.doc.story(sid).unwrap().para_ranges()[36].start + designcraft_doc::ANCHOR_MARK.len_utf8();
    command(&mut s, "text.select", json!({"story":sid,"anchor":at,"focus":at+7}));
    metrics(&mut s, true);
    let started = Instant::now();
    command(&mut s, "text.insert", json!({"text":"Updated"}));
    stages.push(evidence(&mut s, "local-edit", started));
    assert!(stages[1]["work"]["paragraphs"].as_u64().unwrap() < 50);
    assert!(stages[1]["work"]["semanticHits"].as_u64().unwrap() > 0);
    assert!(stages[1]["work"]["referenceHits"].as_u64().unwrap() > 0);
    // Save/reopen must retain the already resolved edition and remain valid with a cold cache.
    command(&mut s, "file.save", json!({"path":out.join("local-edit.designcraft")}));
    let reopened = designcraft_format::load(&std::fs::read(out.join("local-edit.designcraft"))?)?;
    assert_eq!(*s.doc()?.doc, reopened);
    let standalone = fixture::chapter(99, 30);
    std::fs::write(out.join("inserted-chapter.designcraft"), designcraft_format::save(&standalone)?)?;
    metrics(&mut s, true);
    let started = Instant::now();
    let new = command(&mut s, "publication.addComponent", json!({"path":out.join("inserted-chapter.designcraft")}))["component"].clone();
    command(&mut s, "publication.moveComponent", json!({"component":new,"at":4}));
    stages.push(evidence(&mut s, "chapter-insertion", started));
    assert_eq!(stages[2]["pages"], 930);
    assert!(stages[2]["work"]["referenceChanges"].as_u64().unwrap() > 0);
    command(&mut s, "publication.activate", json!({"component":component}));
    let at = s.doc()?.doc.story(sid).unwrap().para_ranges()[35].start + designcraft_doc::ANCHOR_MARK.len_utf8();
    command(&mut s, "text.select", json!({"story":sid,"anchor":at,"focus":at}));
    metrics(&mut s, true);
    let started = Instant::now();
    command(
        &mut s,
        "text.insert",
        json!({"text":"A new account of the bridge and its two river banks changes the flow of this section. ".repeat(90)}),
    );
    stages.push(evidence(&mut s, "pagination-cascade", started));
    assert!(stages[3]["pages"].as_u64().unwrap() > 930);
    assert!(stages[3]["work"]["paragraphs"].as_u64().unwrap() > 30);
    metrics(&mut s, true);
    let started = Instant::now();
    command(&mut s, "style.paragraph.edit", json!({"name":"[Basic Paragraph]","chars":{"size":12.5}}));
    let preview = command(&mut s, "publication.styles.preview", json!({}));
    let rows: Vec<_> = preview["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["key"] == "paragraph:[Basic Paragraph]" && r["changed"] == true)
        .map(|r| r["row"].clone())
        .collect();
    command(&mut s, "publication.styles.apply", json!({"preview":preview["preview"],"rows":rows,"replaceOverrides":true}));
    stages.push(evidence(&mut s, "global-style", started));
    assert!(stages[4]["work"]["paragraphs"].as_u64().unwrap() >= 900);
    command(&mut s, "file.save", json!({"path":out.join("final.designcraft")}));
    let started = Instant::now();
    let pdf = command(&mut s, "file.exportPdf", json!({"path":out.join("atlas.pdf"),"strict":true}));
    let receipt = json!({"fixture":{"chapters":12,"pages":900,"references":3600,"source":"authored deterministic Rust generator","hardSectionStarts":true},"stages":stages,"pdf":pdf,"pdfSeconds":started.elapsed().as_secs_f64(),"memoryNote":"cache metrics estimate retained payload; /usr/bin/time -v records process peak RSS separately"});
    std::fs::write(out.join("receipt.json"), serde_json::to_vec_pretty(&receipt)?)?;
    println!("Completed {}", out.display());
    Ok(())
}
