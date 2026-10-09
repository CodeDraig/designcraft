//! Numbered-list styles and numeral formatting.

use super::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NumberStyle {
    #[default]
    Arabic,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
    /// 01, 02, …
    ArabicLeadingZero,
    /// *, †, ‡, §, ¶, ‖, then doubled (**, ††, …): footnotes.
    Symbols,
}

impl NumberStyle {
    pub fn format(self, n: u32) -> String {
        match self {
            NumberStyle::Arabic => n.to_string(),
            NumberStyle::ArabicLeadingZero => format!("{n:02}"),
            NumberStyle::UpperRoman => roman(n),
            NumberStyle::LowerRoman => roman(n).to_lowercase(),
            NumberStyle::UpperLetters => letters(n),
            NumberStyle::LowerLetters => letters(n).to_lowercase(),
            NumberStyle::Symbols => {
                const SYM: [char; 6] = ['*', '†', '‡', '§', '¶', '‖'];
                let n = n.max(1) as usize - 1;
                std::iter::repeat_n(SYM[n % SYM.len()], n / SYM.len() + 1).collect()
            }
        }
    }
}

fn roman(mut n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let t = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// A, B, …, Z, AA, BB, … (InDesign's letter numbering repeats the letter).
fn letters(n: u32) -> String {
    if n == 0 {
        return String::new();
    }
    let i = (n - 1) % 26;
    let reps = (n - 1) / 26 + 1;
    std::iter::repeat_n(char::from(b'A' + i as u8), reps as usize).collect()
}
