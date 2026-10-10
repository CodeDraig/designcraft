//! GPU-independent acceptance check using the application's actual IndexedDB adapter.
//! Run `trunk build recovery-check.html --dist <fresh-output-directory>` and serve that directory.
#[cfg(target_arch = "wasm32")]
#[path = "../src/web/recovery.rs"]
#[allow(dead_code)]
mod recovery;

#[cfg(target_arch = "wasm32")]
fn main() {
    wasm_bindgen_futures::spawn_local(async {
        let result = recovery::check_roundtrip().await;
        if let Some(element) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("result")) {
            element.set_text_content(Some(&match result {
                Ok(report) => format!("PASS\n{report}"),
                Err(error) => format!("FAIL\n{error}"),
            }));
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("Build recovery-check.html with Trunk to run this browser check.");
}
