// Rasterized from assets/smaragd-icon.svg by build.rs; keep ICON_SIZE in sync with it.
const ICON_SIZE: u32 = 256;
const ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon_rgba.bin"));

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
