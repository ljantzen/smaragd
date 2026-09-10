// Rasterized from assets/smaragd-icon.svg by build.rs; keep ICON_SIZE in sync with it.
#[cfg(not(target_arch = "wasm32"))]
const ICON_SIZE: u32 = 256;
#[cfg(not(target_arch = "wasm32"))]
const ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon_rgba.bin"));

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_icon(egui::IconData {
            rgba: ICON_RGBA.to_vec(),
            width: ICON_SIZE,
            height: ICON_SIZE,
        }),
        ..Default::default()
    };
    eframe::run_native(
        "Smaragd",
        native_options,
        Box::new(|cc| Ok(Box::new(smaragd::SmaragdApp::new(cc)))),
    )
}

/// The web entry point (Phase 0 spike): mounts into a `<canvas id="the_canvas_id">`
/// on the host page — see `index.html`, which `trunk` serves during
/// `trunk serve`/bundles on `trunk build`. No window icon here (that's a
/// native-viewport concept); the page's own favicon covers that role in a
/// browser tab.
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("failed to find #the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#the_canvas_id was not a canvas");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(smaragd::SmaragdApp::new(cc)))),
            )
            .await;

        if let Err(err) = start_result {
            web_sys::console::error_1(&format!("failed to start smaragd: {err:?}").into());
        }
    });
}
