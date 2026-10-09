//! Output ink aliases and process-color conversion settings.

use serde::{Deserialize, Serialize};

/// Ink Manager settings for output.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InkManager {
    /// All Spots to Process.
    pub all_to_process: bool,
    /// Spot inks converted to process one by one.
    pub to_process: Vec<String>,
    /// Ink Alias: (spot ink, the ink it prints on).
    pub aliases: Vec<(String, String)>,
}

impl InkManager {
    pub(super) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The ink a spot swatch prints on (after aliases; at most a few hops) and whether it's
    /// converted to process.
    pub fn resolve<'a>(&'a self, ink: &'a str) -> (&'a str, bool) {
        let mut n = ink;
        for _ in 0..8 {
            match self.aliases.iter().find(|(a, _)| a == n) {
                Some((_, to)) if to != n => n = to,
                _ => break,
            }
        }
        (n, self.all_to_process || self.to_process.iter().any(|x| x == n))
    }
}
