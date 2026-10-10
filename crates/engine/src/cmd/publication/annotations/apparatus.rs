use super::*;
use designcraft_doc::{Footnote, Story};

pub(in super::super) fn diagnostics(root: &Publication, edition: designcraft_doc::EditionId) -> Result<Vec<String>> {
    let Some(witness) = root.editions.get(&edition).and_then(|e| e.apparatus_witness.as_deref()) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    let mut owners = std::collections::BTreeMap::new();
    for e in root.annotation_entries()? {
        let Some(a) = &e.note.apparatus else { continue };
        if let std::collections::btree_map::Entry::Vacant(slot) = owners.entry(e.component) {
            slot.insert(root.project(edition, e.component)?);
        }
        let owner = owners.get(&e.component).and_then(|d| d.story(e.story)).and_then(|s| at_path(s, &e.path));
        let selected = a.choose(witness).and_then(|i| a.readings.get(i));
        let message = if owner.is_none_or(|s| a.edited(s)) {
            Some("Edited passage retained; witness selection has not overwritten it.")
        } else if selected.is_some_and(|r| r.text.text.contains('\n') || !r.text.tables.is_empty() || !r.text.objects.is_empty())
            || owner.and_then(|s| a.range(s).map(|r| s.text[r].contains('\n'))).unwrap_or(false)
        {
            Some("Block reading retained in the structured apparatus and original XML; imported passage remains in the page layout.")
        } else if !a.readings.iter().any(|r| r.witnesses.iter().any(|w| w.trim_start_matches('#') == witness)) {
            Some("Witness has no reading here; lemma or first reading selected.")
        } else {
            None
        };
        if let Some(message) = message {
            out.push(format!("Apparatus {} (chapter {}): {message}", a.source_key, e.component.0));
        }
    }
    Ok(out)
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "publication.apparatus.list","Critical Readings",[],None,"{} → structured readings, witness associations, selection and edited-passage status",has_publication,|s,_| {
            let v=view(s)?;let witness=v.root.editions.get(&v.edition).and_then(|e|e.apparatus_witness.as_deref());
            let mut out=Vec::new();for e in v.root.annotation_entries()? {if let Some(a)=&e.note.apparatus {
                let d=v.root.project(v.edition,e.component)?;let owner=d.story(e.story).and_then(|s|at_path(s,&e.path));
                let edited=owner.is_none_or(|s|a.edited(s));
                let selected=witness.and_then(|w|a.choose(w)).unwrap_or(a.imported_reading);
                let supported=owner.and_then(|s|a.range(s).map(|r|(s,r))).is_some_and(|(s,r)|!s.text[r].contains('\n'))
                    && a.readings.get(selected).is_some_and(|r|!r.text.text.contains('\n') && r.text.tables.is_empty() && r.text.objects.is_empty());
                out.push(json!({"note":e.identity.id,"component":e.component,"source":a.source,"key":a.source_key,"witness":witness,"selected":selected,
                    "edited":edited,"display":if edited{"edited passage retained"}else if !supported{"complex reading retained in source and apparatus"}else{"selected reading"},"readings":a.readings}));
            }}Ok(json!({"witness":witness,"entries":out}))
        }),
        cmd!(
            "publication.apparatus.witness",
            "Edition Witness",
            [],
            None,
            "{witness: string|null} — select matching readings, falling back to lemma then first reading; edited passages stay intact",
            has_publication,
            |s, p| {
                let witness = match p.get("witness") {
                    Some(Value::Null) => None,
                    Some(Value::String(w)) if w.len() <= 256 => Some(w.trim_start_matches('#').to_string()),
                    _ => return Err(bad("publication.apparatus.witness", "witness must be a string of at most 256 bytes or null")),
                };
                mutate(s, |r, v| {
                    Arc::make_mut(r.editions.get_mut(&v.edition).ok_or_else(|| bad("publication.apparatus.witness", "edition missing"))?)
                        .apparatus_witness = witness;
                    r.revision = r.revision.saturating_add(1);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "publication.apparatus.recover",
            "Recover Preserved Apparatus",
            [],
            None,
            "{component?, source?, apply?:false} — preview or attach structured readings from unchanged TEI; never replaces edited passage or note text; ambiguous matches stay unresolved",
            has_publication,
            recover
        ),
    ]
}

#[derive(Clone)]
struct Candidate {
    source: u64,
    apparatus: Apparatus,
    note: Option<Entry>,
    range: Option<std::ops::Range<usize>>,
    problem: Option<String>,
}

fn candidates(root: &Publication, edition: designcraft_doc::EditionId, component: ComponentId, source: Option<u64>) -> Result<Vec<Candidate>> {
    let d = root.project(edition, component)?;
    let notes = root.annotation_entries()?;
    let mut out = Vec::new();
    for saved in d.tei_sources.iter().filter(|s| source.is_none_or(|id| id == s.id)) {
        let mut parsed = designcraft_textimport::tei::import(&saved.name, &saved.bytes, &saved.options)
            .map_err(|e| bad("publication.apparatus.recover", e.to_string()))?;
        // Reuse only already-preserved assets; recovery never resolves a path or fetches a resource.
        if let Some(payload) = &parsed.tei {
            for request in &payload.graphics {
                let asset = saved.resources.iter().find(|r| r.url == request.url).and_then(|r| r.asset).and_then(|id| d.assets.get(&id));
                if let Some(asset) = asset
                    && let Some((width, height)) = asset.pixels
                {
                    parsed.story.for_each_encoding_mut(&mut |st| {
                        for object in &mut st.objects {
                            if object.item.id.0 == request.item {
                                let object = Arc::make_mut(object);
                                let ratio = f64::from(width) / f64::from(height.max(1));
                                let w = request.width.or_else(|| request.height.map(|h| h * ratio)).unwrap_or(f64::from(width)).clamp(1., 400.);
                                object.item.path = designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0., 0., w, w / ratio));
                                object.item.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
                                    asset: asset.id,
                                    size: (f64::from(width), f64::from(height)),
                                    xf: designcraft_geom::Affine::scale(w / f64::from(width.max(1))),
                                    auto_fit: Default::default(),
                                    fit_align: 4,
                                    crop: [0.; 4],
                                });
                            }
                        }
                    });
                }
            }
        }
        parsed.story.for_each_encoding_mut(&mut |st| {
            for p in &mut st.paras {
                if let Some(name) = saved.import_settings.style_map.get(&p.style) {
                    p.style = name.clone();
                }
            }
            for c in &mut st.chars {
                if let Some(name) = saved.import_settings.style_map.get(&c.format.style) {
                    c.format.style = name.clone();
                }
            }
        });
        let mut story = parsed.story;
        let mut fresh = Vec::new();
        story.for_each_text_mut(&mut |s| {
            for n in s.notes.iter().chain(&s.endnotes) {
                if let Some(a) = &n.apparatus {
                    fresh.push((a.clone(), designcraft_doc::xref::clean(&n.text.text)));
                }
            }
        });
        for (mut a, text) in fresh {
            a.source = saved.id;
            if notes.iter().any(|e| {
                e.component == component && e.note.apparatus.as_ref().is_some_and(|old| old.source == a.source && old.source_key == a.source_key)
            }) {
                continue;
            }
            let mut matches = Vec::new();
            let mut ambiguous_id = false;
            for e in notes.iter().filter(|e| e.component == component && e.note.apparatus.is_none()) {
                let Some(owner) = d.story(e.story).and_then(|s| at_path(s, &e.path)) else { continue };
                let named: Vec<_> = owner.anchors.iter().filter(|anchor| anchor.name == a.source_key).collect();
                if named.len() > 1 {
                    ambiguous_id = true;
                    continue;
                }
                let by_id = named
                    .first()
                    .and_then(|anchor| owner.anchor_pos(anchor.id))
                    .map(|at| at + designcraft_doc::ANCHOR_MARK.len_utf8())
                    .filter(|at| *at <= e.at);
                let same_note = designcraft_doc::xref::clean(&e.note.text.text) == text;
                let suffix = owner.text.get(..e.at).and_then(|s| s.strip_suffix(&a.imported_text)).map(str::len);
                if by_id.is_some() || same_note {
                    matches.push((e.clone(), by_id.or(suffix).map(|start| start..e.at), by_id));
                }
            }
            // An xml:id identifies the first following note in that same parent text.
            if matches.iter().any(|(_, _, a)| a.is_some()) {
                matches.retain(|(_, _, a)| a.is_some());
                matches.sort_by_key(|(e, _, _)| e.at);
                let mut owners = std::collections::BTreeSet::new();
                matches.retain(|(e, _, _)| owners.insert((e.story, e.path.clone())));
            }
            let (note, range, problem) = if ambiguous_id {
                (
                    None,
                    None,
                    Some("Repeated source identity in the same passage; choose a unique match before recovery. No text was replaced.".into()),
                )
            } else if matches.len() == 1 {
                let (e, r, _) = matches.remove(0);
                let problem = r.is_none().then(|| "Passage was edited and has no source anchor; choose its boundaries before recovery.".into());
                (Some(e), r, problem)
            } else {
                (None, None, Some(format!("{} matching notes; no text was replaced.", matches.len())))
            };
            out.push(Candidate { source: saved.id, apparatus: a, note, range, problem });
        }
    }
    Ok(out)
}

fn reserve(d: &mut designcraft_doc::Document) -> Result<u64> {
    if d.next_id >= 1 << 28 {
        return Err(bad("publication.apparatus.recover", "anchor identity space exhausted"));
    }
    Ok(d.alloc() << 24)
}
fn marker(st: &mut Story, at: usize, id: u64, label: &str) {
    let local = st.insert_anchor(at, label);
    if let Some(a) = st.anchors.iter_mut().find(|a| a.id == local) {
        Arc::make_mut(a).id = id;
    }
}
fn recover(s: &mut Session, p: &Value) -> Result<Value> {
    let v = view(s)?;
    let component = ComponentId(optional_id(p, "component")?.unwrap_or(v.component.0));
    let source = optional_id(p, "source")?;
    let proposed = candidates(&v.root, v.edition, component, source)?;
    let report:Vec<_>=proposed.iter().map(|c|json!({"source":c.source,"key":c.apparatus.source_key,"note":c.note.as_ref().map(|e|e.identity.id),"range":c.range,"problem":c.problem})).collect();
    if p["apply"] != true {
        return Ok(json!({"applied":0,"revision":v.root.revision,"candidates":report}));
    }
    mutate(s, |r, v| {
        let mut count = 0;
        // Later ranges first, so inserting markers cannot invalidate an earlier byte address.
        let mut proposed = proposed;
        proposed.sort_by_key(|c| std::cmp::Reverse(c.range.as_ref().map_or(0, |r| r.start)));
        for c in proposed.into_iter().filter(|c| c.problem.is_none()) {
            let (Some(e), Some(range)) = (c.note, c.range) else { continue };
            let mut d = r.project(v.edition, component)?;
            let (start, end) = (reserve(&mut d)?, reserve(&mut d)?);
            let mut a = c.apparatus;
            a.start = start;
            a.end = end;
            // Alternate encodings acquire fresh local identifiers without touching the edited text.
            for reading in &mut a.readings {
                let mut error = None;
                let mut mapping = std::collections::BTreeMap::new();
                reading.text.for_each_encoding_mut(&mut |st| {
                    st.id = StoryId(d.alloc());
                    for anchor in &mut st.anchors {
                        match reserve(&mut d) {
                            Ok(id) => {
                                mapping.insert(anchor.id, id);
                                Arc::make_mut(anchor).id = id;
                            }
                            Err(e) => error = Some(e),
                        }
                    }
                    for object in &mut st.objects {
                        let item = &mut Arc::make_mut(object).item;
                        item.id = designcraft_doc::ItemId(d.alloc());
                        item.layer = d.default_layer();
                    }
                });
                if let Some(e) = error {
                    return Err(e);
                }
                reading.text.for_each_encoding_mut(&mut |st| {
                    for x in &mut st.xrefs {
                        if let Some(id) = mapping.get(&x.target) {
                            Arc::make_mut(x).target = *id;
                        }
                    }
                });
            }
            let st = d
                .story_mut(e.story)
                .and_then(|s| at_path_mut(s, &e.path))
                .ok_or_else(|| bad("publication.apparatus.recover", "note owner missing"))?;
            marker(st, range.end, end, "Recovered apparatus end");
            marker(st, range.start, start, "Recovered apparatus start");
            let note: &mut Footnote = if e.endnote { st.endnote_mut(e.note.id) } else { st.note_mut(e.note.id) }
                .ok_or_else(|| bad("publication.apparatus.recover", "note missing"))?;
            note.apparatus = Some(a);
            r.capture(v.edition, component, &d)?;
            count += 1;
        }
        Ok(json!({"applied":count,"candidates":report}))
    })
}
