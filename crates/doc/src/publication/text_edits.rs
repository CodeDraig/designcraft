//! Bounded character alignment for carrying edition typography through shared text edits.
//! Most edits reduce to one insertion/deletion after trimming their common prefix/suffix.
//! Disjoint edits use Myers' shortest edit path; ambiguity never silently drops formatting.
use std::ops::Range;

use super::invalid;
use crate::Result;

#[derive(Debug, PartialEq)]
pub struct TextEdit {
    pub before: Range<usize>,
    pub after: Range<usize>,
}

pub fn edits(before: &str, after: &str) -> Result<Vec<TextEdit>> {
    if before == after {
        return Ok(Vec::new());
    }
    let prefix = before.chars().zip(after.chars()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum::<usize>();
    let suffix =
        before[prefix..].chars().rev().zip(after[prefix..].chars().rev()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum::<usize>();
    let a = prefix..before.len() - suffix;
    let b = prefix..after.len() - suffix;
    if a.is_empty() || b.is_empty() {
        return Ok(vec![TextEdit { before: a, after: b }]);
    }
    if a.len().saturating_add(b.len()) > 16 * 1024 * 1024 {
        return Err(invalid("shared text edit exceeds the edition-format alignment size limit (16 MiB); split the edit into smaller operations"));
    }
    let left: Vec<(usize, char)> = before[a.clone()].char_indices().collect();
    let right: Vec<(usize, char)> = after[b.clone()].char_indices().collect();
    // All-new text has no surviving typography to align.
    let alphabet: std::collections::BTreeSet<_> = left.iter().map(|(_, c)| *c).collect();
    if right.iter().all(|(_, c)| !alphabet.contains(c)) {
        return Ok(vec![TextEdit { before: a, after: b }]);
    }
    const LIMIT: usize = 1024;
    const WORK: usize = 16_000_000;
    let max = (left.len() + right.len()).min(LIMIT);
    let offset = max + 1;
    let mut v = vec![0usize; max * 2 + 3];
    let mut trace = Vec::new();
    let mut work = 0usize;
    let mut distance = None;
    'search: for d in 0..=max {
        trace.push(v.clone());
        for step in 0..=d {
            let k = step * 2 + offset - d;
            let mut x = if step == 0 || (step != d && v[k - 1] < v[k + 1]) { v[k + 1] } else { v[k - 1] + 1 };
            let mut y = if k >= offset { x.saturating_sub(k - offset) } else { x + offset - k };
            while x < left.len() && y < right.len() && left[x].1 == right[y].1 {
                x += 1;
                y += 1;
                work += 1;
                if work > WORK {
                    break 'search;
                }
            }
            work += 1;
            v[k] = x;
            if x >= left.len() && y >= right.len() {
                distance = Some(d);
                break 'search;
            }
        }
    }
    let Some(distance) = distance else {
        return Err(invalid("shared text edit exceeds the edition-format alignment limit; split the edit into smaller operations"));
    };
    let (mut x, mut y) = (left.len(), right.len());
    let mut matches = Vec::new();
    for d in (0..=distance).rev() {
        let prior = &trace[d];
        let k = if x >= y { offset + x - y } else { offset - (y - x) };
        let previous_k = if k == offset - d || (k != offset + d && prior[k - 1] < prior[k + 1]) { k + 1 } else { k - 1 };
        let px = prior[previous_k];
        let py = if previous_k >= offset { px.saturating_sub(previous_k - offset) } else { px + offset - previous_k };
        while x > px && y > py {
            x -= 1;
            y -= 1;
            matches.push((x, y));
        }
        x = px;
        y = py;
    }
    matches.reverse();
    let mut out = Vec::new();
    let (mut x, mut y) = (0, 0);
    for (i, j) in matches.into_iter().chain(std::iter::once((left.len(), right.len()))) {
        let ai = left.get(i).map_or(a.end, |(p, _)| a.start + p);
        let bj = right.get(j).map_or(b.end, |(p, _)| b.start + p);
        if i != x || j != y {
            out.push(TextEdit {
                before: left.get(x).map_or(a.end, |(p, _)| a.start + p)..ai,
                after: right.get(y).map_or(b.end, |(p, _)| b.start + p)..bj,
            });
        }
        x = i + 1;
        y = j + 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn edit_ranges_reproduce_new_unicode_text(a in "[a-z Ωŵé\\n]{0,100}", b in "[a-z Ωŵé\\n]{0,100}") {
            let mut actual = a.clone();
            for e in edits(&a,&b).unwrap().into_iter().rev() {
                actual.replace_range(e.before, &b[e.after]);
            }
            prop_assert_eq!(actual, b);
        }
    }

    #[test]
    fn separated_insertions_keep_middle_text() {
        let e = edits("one middle three", "NEW one middle three END").unwrap();
        assert_eq!(e, vec![TextEdit { before: 0..0, after: 0..4 }, TextEdit { before: 16..16, after: 20..24 }]);
    }
}
