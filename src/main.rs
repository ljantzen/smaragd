fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Smaragd",
        native_options,
        Box::new(|cc| Ok(Box::new(smaragd::SmaragdApp::new(cc)))),
    )
}
