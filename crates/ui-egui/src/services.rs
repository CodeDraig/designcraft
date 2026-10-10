//! Host file services and asynchronous file delivery.

pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;
pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;
pub type PickFn = Box<dyn FnMut(&str) -> Option<String>>;
pub type OpenAsyncFn = Box<dyn FnMut(&str)>;
pub type DownloadFn = Box<dyn FnMut(&str, &[u8])>;
pub type RecoverySaveFn = Box<dyn FnMut(Vec<designcraft_engine::recovery::RecoverySnapshot>)>;
pub type StatusInbox = std::sync::Arc<std::sync::Mutex<Vec<String>>>;
/// One asynchronous file selection. Preserve Open/Place intent and group dropped resources.
#[derive(Clone, Debug)]
pub struct DeliveredFiles {
    pub purpose: String,
    pub files: Vec<(String, Vec<u8>)>,
}
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<DeliveredFiles>>>;

/// Platform services injected by the host (desktop or web).
#[derive(Default)]
pub struct Services {
    /// Open-file dialog for a purpose (`open`, `place`) → path.
    pub pick_open: Option<PickFn>,
    /// Save dialog with a suggested name → path.
    pub pick_save: Option<PickFn>,
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
    /// Asynchronous open dialog for a purpose (`open`, `place`); the chosen file arrives later
    /// through [`Services::inbox`]. Used when `pick_open` is unset (web).
    pub open_async: Option<OpenAsyncFn>,
    /// Hand bytes to the user as a named file (browser download). When set, Save uses it
    /// instead of writing to a path.
    pub download: Option<DownloadFn>,
    /// File selections delivered asynchronously and drained every frame.
    pub inbox: Option<Inbox>,
    /// Browser host persistence. Receives all currently dirty documents in one atomic batch.
    pub save_recovery: Option<RecoverySaveFn>,
    /// Asynchronous host errors (for example browser storage quota exhaustion).
    pub status_inbox: Option<StatusInbox>,
}
