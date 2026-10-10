//! The browser shell: web `Services`, drag-and-drop, and the eframe web runner.

mod files;
mod recovery;
use files::services;

use designcraft_engine::Session;
use designcraft_ui_egui::{DeliveredFiles, DesignApp, Inbox};
use wasm_bindgen::JsCast as _;

const DOC_EXTS: &[&str] = &["designcraft", "idml", "tei", "xml"];
const IMAGE_EXTS: &[&str] = &[
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "tif",
    "tiff",
    "bmp",
    "psd",
    "svg",
    "pdf",
    "ai",
    "txt",
    "docx",
    "rtf",
    "md",
    "xlsx",
    "tei",
    "xml",
    "idml",
    "designcraft",
];
const CANVAS_ID: &str = "designcraft_canvas";
const LOADING_ID: &str = "designcraft_loading";

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"{CANVAS_ID}\">");
            return;
        };
        let mut options = eframe::WebOptions::default();
        if query().contains("webgl")
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
        {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(move |cc| {
                    if let Some(rs) = &cc.wgpu_render_state {
                        log::info!("designcraft-web: wgpu backend {:?}", rs.adapter.get_info().backend);
                    }
                    let inbox: Inbox = Inbox::default();
                    let mut app = DesignApp::new(Session::new(), services(inbox.clone(), cc.egui_ctx.clone()));
                    if query().contains("sample") {
                        let _ = app.run("file.newSample", serde_json::json!({}));
                    }
                    Ok(Box::new(WebShell { app, inbox }))
                }),
            )
            .await;
        if let Some(el) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => el.remove(),
                Err(e) => el.set_inner_html(&format!("<p>DesignCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>")),
            }
        }
    });
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

/// Wraps the app to read dropped files asynchronously (browsers can't read them synchronously)
/// and feed them through the inbox.
struct WebShell {
    app: DesignApp,
    inbox: Inbox,
}

impl eframe::App for WebShell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        if !dropped.is_empty() {
            let inbox = self.inbox.clone();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let mut files = Vec::new();
                for f in dropped {
                    let name = f.path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped".into());
                    match f.bytes_async().await {
                        Ok(bytes) => files.push((name, bytes)),
                        Err(e) => log::error!("dropped file: {e}"),
                    }
                }
                inbox.lock().unwrap_or_else(|e| e.into_inner()).push(DeliveredFiles { purpose: "drop".into(), files });
                ctx.request_repaint();
            });
        }
        self.app.logic(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}
