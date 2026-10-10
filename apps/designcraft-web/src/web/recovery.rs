//! Atomic browser recovery in IndexedDB. A browser run writes only its own keys; recovery opens
//! copies, preserving snapshots from earlier runs and other tabs until the user clears site data.
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use designcraft_ui_egui::{DeliveredFiles, Inbox, RecoverySaveFn, StatusInbox};
use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, IdbDatabase, IdbRequest, IdbTransaction, IdbTransactionMode};

const STORE: &str = "snapshots";

fn error(value: JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}

async fn request(request: &IdbRequest) -> Result<JsValue, String> {
    let mut success = None;
    let mut failure = None;
    let promise = Promise::new(&mut |resolve, reject| {
        let r = request.clone();
        let reject_result = reject.clone();
        success = Some(Closure::<dyn FnMut(Event)>::new(move |_| match r.result() {
            Ok(value) => {
                let _ = resolve.call1(&JsValue::NULL, &value);
            }
            Err(e) => {
                let _ = reject_result.call1(&JsValue::NULL, &e);
            }
        }));
        let r = request.clone();
        failure = Some(Closure::<dyn FnMut(Event)>::new(move |_| {
            let message = r.error().ok().flatten().map_or_else(|| "browser storage request failed".into(), |e| e.message());
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&message));
        }));
        if let Some(handler) = &success {
            request.set_onsuccess(Some(handler.as_ref().unchecked_ref()));
        }
        if let Some(handler) = &failure {
            request.set_onerror(Some(handler.as_ref().unchecked_ref()));
        }
    });
    let result = JsFuture::from(promise).await.map_err(error);
    request.set_onsuccess(None);
    request.set_onerror(None);
    drop((success, failure));
    result
}

async fn open() -> Result<IdbDatabase, String> {
    let factory = web_sys::window().ok_or("no browser window")?.indexed_db().map_err(error)?.ok_or("IndexedDB is unavailable")?;
    let pending = factory.open_with_u32("designcraft-recovery", 1).map_err(error)?;
    let r = pending.clone();
    let upgrade = Closure::<dyn FnMut(Event)>::new(move |_| {
        let result = r.result().and_then(|value| value.dyn_into::<IdbDatabase>()).and_then(|db| db.create_object_store(STORE));
        if result.is_err()
            && let Some(tx) = r.transaction()
        {
            let _ = tx.abort();
        }
    });
    pending.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
    let result = request(&pending).await;
    pending.set_onupgradeneeded(None);
    result?.dyn_into::<IdbDatabase>().map_err(error)
}

struct Completion {
    transaction: IdbTransaction,
    promise: Promise,
    _complete: Option<Closure<dyn FnMut(Event)>>,
    _failure: Option<Closure<dyn FnMut(Event)>>,
}

impl Completion {
    fn new(transaction: &IdbTransaction) -> Self {
        let mut complete = None;
        let mut failure = None;
        let promise = Promise::new(&mut |resolve, reject| {
            complete = Some(Closure::<dyn FnMut(Event)>::new(move |_| {
                let _ = resolve.call0(&JsValue::NULL);
            }));
            let tx = transaction.clone();
            failure = Some(Closure::<dyn FnMut(Event)>::new(move |_| {
                let message = tx.error().map_or_else(|| "browser storage transaction aborted".into(), |e| e.message());
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&message));
            }));
            if let Some(handler) = &complete {
                transaction.set_oncomplete(Some(handler.as_ref().unchecked_ref()));
            }
            if let Some(handler) = &failure {
                transaction.set_onerror(Some(handler.as_ref().unchecked_ref()));
                transaction.set_onabort(Some(handler.as_ref().unchecked_ref()));
            }
        });
        Self { transaction: transaction.clone(), promise, _complete: complete, _failure: failure }
    }

    async fn wait(self) -> Result<(), String> {
        JsFuture::from(self.promise.clone()).await.map_err(error)?;
        Ok(())
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        self.transaction.set_oncomplete(None);
        self.transaction.set_onerror(None);
        self.transaction.set_onabort(None);
    }
}

async fn write(
    snapshots: Vec<designcraft_engine::recovery::RecoverySnapshot>,
    prefix: &str,
    previous: &BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    // Prepare all data before creating the transaction: yielding between puts could otherwise
    // let IndexedDB commit a partial batch.
    let mut records = Vec::new();
    let mut current = BTreeSet::new();
    for snapshot in snapshots {
        let key = format!("{prefix}{}", snapshot.uid);
        let value = Object::new();
        Reflect::set(&value, &JsValue::from_str("key"), &JsValue::from_str(&key)).map_err(error)?;
        Reflect::set(&value, &JsValue::from_str("name"), &JsValue::from_str(&snapshot.title)).map_err(error)?;
        Reflect::set(&value, &JsValue::from_str("bytes"), &Uint8Array::from(snapshot.bytes.as_slice())).map_err(error)?;
        current.insert(key.clone());
        records.push((key, value));
    }
    let db = open().await?;
    let tx = db.transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite).map_err(error)?;
    let done = Completion::new(&tx);
    let result = (|| {
        let store = tx.object_store(STORE).map_err(error)?;
        for (key, value) in records {
            store.put_with_key(&value, &JsValue::from_str(&key)).map_err(error)?;
        }
        // These belong to this run and were explicitly saved or closed in the editor.
        for key in previous.difference(&current) {
            store.delete(&JsValue::from_str(key)).map_err(error)?;
        }
        Ok::<(), String>(())
    })();
    if result.is_err() {
        let _ = tx.abort();
    }
    let committed = done.wait().await;
    db.close();
    result?;
    committed?;
    Ok(current)
}

pub(super) fn writer(prefix: String, status: StatusInbox, ctx: egui::Context) -> RecoverySaveFn {
    let busy = Rc::new(Cell::new(false));
    let previous = Rc::new(RefCell::new(BTreeSet::new()));
    Box::new(move |snapshots| {
        if busy.replace(true) {
            return;
        }
        let busy = busy.clone();
        let previous = previous.clone();
        let keys = previous.borrow().clone();
        let prefix = prefix.clone();
        let status = status.clone();
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match write(snapshots, &prefix, &keys).await {
                Ok(keys) => *previous.borrow_mut() = keys,
                Err(e) => status.lock().unwrap_or_else(|e| e.into_inner()).push(format!("Browser recovery was not saved: {e}")),
            }
            busy.set(false);
            ctx.request_repaint();
        });
    })
}

pub(super) fn recover(prefix: String, inbox: Inbox, status: StatusInbox, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        match read(&prefix).await {
            Ok(files) if files.is_empty() => {
                status.lock().unwrap_or_else(|e| e.into_inner()).push("No previous browser recovery snapshots were found.".into())
            }
            Ok(files) => inbox.lock().unwrap_or_else(|e| e.into_inner()).push(DeliveredFiles { purpose: "recovery".into(), files }),
            Err(e) => status.lock().unwrap_or_else(|e| e.into_inner()).push(format!("Could not read browser recovery: {e}")),
        }
        ctx.request_repaint();
    });
}

async fn read(prefix: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let db = open().await?;
    let tx = db.transaction_with_str(STORE).map_err(error)?;
    let done = Completion::new(&tx);
    let pending = tx.object_store(STORE).map_err(error)?.get_all().map_err(error)?;
    let values = request(&pending).await;
    let committed = done.wait().await;
    db.close();
    committed?;
    let mut files = Vec::new();
    for record in Array::from(&values?) {
        let key = Reflect::get(&record, &JsValue::from_str("key")).map_err(error)?.as_string().ok_or("invalid recovery identity")?;
        if key.starts_with(prefix) {
            continue;
        }
        let name = Reflect::get(&record, &JsValue::from_str("name")).map_err(error)?.as_string().ok_or("invalid recovery title")?;
        let bytes = Reflect::get(&record, &JsValue::from_str("bytes")).map_err(error)?.dyn_into::<Uint8Array>().map_err(error)?;
        if bytes.length() > 1024 * 1024 * 1024 {
            return Err("recovery archive exceeds 1 GiB".into());
        }
        files.push((name, bytes.to_vec()));
    }
    Ok(files)
}

/// This check is compiled only for the explicit browser-checks target. It retains its own
/// archives as evidence and never removes snapshots belonging to the application or other tabs.
#[cfg(feature = "browser-checks")]
#[allow(dead_code)] // Called by the opt-in check binary, not the normal editor binary.
pub(super) async fn check_roundtrip() -> Result<String, String> {
    use designcraft_engine::{Session, cmd::base64_encode, recovery::snapshots};
    use serde_json::json;

    let suffix = format!("{}-{}", js_sys::Date::now(), js_sys::Math::random());
    let title = format!("Recovery acceptance {suffix}");
    let prefix = format!("check-{suffix}:");
    let mut session = Session::new();
    session.execute("publication.new", &json!({"title":title})).map_err(|e| e.to_string())?;
    session.execute("publication.addComponent", &json!({"title":"Second chapter"})).map_err(|e| e.to_string())?;
    session.execute("publication.edition.create", &json!({"name":"Reading"})).map_err(|e| e.to_string())?;
    let original = snapshots(&session).map_err(|e| e.to_string())?;
    let expected = original.first().ok_or("missing dirty publication")?.bytes.clone();
    let keys = write(original, &prefix, &BTreeSet::new()).await?;
    if read(&prefix).await?.iter().any(|(name, _)| name == &title) {
        return Err("recovery offered the current browser run's snapshot".into());
    }
    let files = read("a-different-browser-run:").await?;
    let (_, bytes) = files.iter().find(|(name, _)| name == &title).ok_or("committed archive missing on subsequent connection")?;
    if bytes != &expected {
        return Err("IndexedDB changed the native archive bytes".into());
    }
    let mut restored = Session::new();
    restored.execute("file.recoverBytes", &json!({"base64":base64_encode(bytes)})).map_err(|e| e.to_string())?;
    let st = restored.doc().map_err(|e| e.to_string())?;
    let view = st.doc.publication.as_ref().ok_or("publication root lost")?;
    if view.root.components.len() != 2 || view.root.editions.len() != 2 || !st.is_dirty() || st.path.is_some() {
        return Err("recovery lost chapters, editions or unsaved state".into());
    }
    let other = format!("check-other-{suffix}:");
    write(snapshots(&restored).map_err(|e| e.to_string())?, &other, &BTreeSet::new()).await?;
    write(Vec::new(), &prefix, &keys).await?;
    let count = read("a-different-browser-run:").await?.iter().filter(|(name, _)| name == &title).count();
    if count != 1 {
        return Err("closing one run removed another run's archive".into());
    }
    Ok(format!(
        "Two chapters and two editions restored as an unsaved publication.\n{} archive bytes preserved exactly.\nCurrent-run exclusion and other-run retention verified.\nRetained evidence prefix: {other}",
        expected.len()
    ))
}
