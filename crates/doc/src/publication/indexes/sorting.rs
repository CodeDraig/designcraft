use super::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Atom {
    Text(String),
    Number(i128),
}

fn natural(text: &str) -> Vec<Atom> {
    let mut parts = Vec::new();
    let mut run = String::new();
    let mut digit = false;
    let flush = |run: &mut String, digit: bool, parts: &mut Vec<Atom>| {
        if run.is_empty() {
            return;
        }
        parts.push(if digit {
            run.parse::<i128>().map(Atom::Number).unwrap_or_else(|_| Atom::Text(run.clone()))
        } else {
            Atom::Text(run.to_lowercase())
        });
        run.clear();
    };
    for c in text.chars() {
        let next = c.is_ascii_digit();
        if next != digit {
            flush(&mut run, digit, &mut parts);
            digit = next;
        }
        run.push(c);
    }
    flush(&mut run, digit, &mut parts);
    parts
}

impl Definition {
    pub(super) fn sort_key(&self, t: &Topic) -> Result<Vec<Atom>> {
        let key = match (&self.sorting, &t.sort) {
            (SortMode::Alphabetical, SortKey::Alphabetical { key }) => {
                vec![Atom::Text(if key.is_empty() { &t.display } else { key }.trim().to_lowercase())]
            }
            (SortMode::Numeric, SortKey::Numeric { value }) => vec![Atom::Number(i128::from(*value))],
            (SortMode::Biblical, SortKey::Biblical { book, chapter, verse, end_chapter, end_verse }) => {
                let canonical = self.book_aliases.get(book).unwrap_or(book);
                let order = self
                    .book_order
                    .iter()
                    .position(|b| b == canonical)
                    .ok_or_else(|| invalid(format!("book {book} is absent from this index's configured order")))?;
                let end = (*end_chapter.as_ref().unwrap_or(chapter), *end_verse.as_ref().unwrap_or(verse));
                if *chapter == 0 || *verse == 0 || end < (*chapter, *verse) || end.1 == 0 {
                    return Err(invalid("biblical chapter/verse ranges must be positive and ordered"));
                }
                vec![
                    Atom::Number(order as i128),
                    Atom::Number(i128::from(*chapter)),
                    Atom::Number(i128::from(*verse)),
                    Atom::Number(i128::from(end.0)),
                    Atom::Number(i128::from(end.1)),
                ]
            }
            (SortMode::Archival, SortKey::Archival { repository, collection, shelfmark, folio, side }) => {
                if repository.len() > 1024 || collection.len() > 1024 || shelfmark.len() > 1024 {
                    return Err(invalid("archival key strings exceed 1024 bytes"));
                }
                let mut key = vec![Atom::Text(repository.to_lowercase()), Atom::Text(collection.to_lowercase())];
                key.extend(natural(shelfmark));
                key.push(Atom::Number(i128::from(*folio)));
                key.push(Atom::Number(if *side == Side::Recto { 0 } else { 1 }));
                key
            }
            _ => return Err(invalid("topic sort key must match the index's sorting mode")),
        };
        if let SortKey::Alphabetical { key } = &t.sort
            && key.len() > 4096
        {
            return Err(invalid("alphabetical sort key exceeds 4096 bytes"));
        }
        Ok(key)
    }
}
