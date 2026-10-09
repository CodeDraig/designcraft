//! Hyperlinks and Bookmarks panels (Window › Interactive).

mod hyperlinks;
pub use hyperlinks::hyperlinks;
mod bookmarks;
pub use bookmarks::bookmarks;
mod articles;
pub use articles::articles;
mod structure;
pub use structure::tags;
mod states;
pub use states::states;
mod buttons;
pub use buttons::buttons;
mod liquid;
pub use liquid::liquid;
mod media;
pub use media::media;
mod transitions;
pub use transitions::transitions;
mod track_changes;
pub use track_changes::track_changes;

use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;
