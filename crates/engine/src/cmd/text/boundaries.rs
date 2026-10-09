//! UTF-8 character and word navigation.

pub fn next_char(s: &str, i: usize) -> usize {
    s[i.min(s.len())..].chars().next().map(|c| i + c.len_utf8()).unwrap_or(s.len())
}
pub fn prev_char(s: &str, i: usize) -> usize {
    s[..i.min(s.len())].chars().next_back().map(|c| i - c.len_utf8()).unwrap_or(0)
}
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’'
}
pub fn next_word(s: &str, i: usize) -> usize {
    let mut j = i;
    let mut seen = false;
    for (k, c) in s[i..].char_indices() {
        if is_word(c) {
            seen = true;
        } else if seen {
            return i + k;
        }
        j = i + k + c.len_utf8();
    }
    j
}
pub fn prev_word(s: &str, i: usize) -> usize {
    let mut seen = false;
    for (k, c) in s[..i].char_indices().rev() {
        if is_word(c) {
            seen = true;
        } else if seen {
            return k + c.len_utf8();
        }
    }
    0
}
pub(super) fn word_bounds(s: &str, i: usize) -> (usize, usize) {
    let a = s[..i.min(s.len())].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map(|(k, _)| k).unwrap_or(i);
    let b = s[i.min(s.len())..].char_indices().find(|(_, c)| !is_word(*c)).map(|(k, _)| i + k).unwrap_or(s.len());
    (a, b)
}
