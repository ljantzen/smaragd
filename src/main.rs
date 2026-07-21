fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Tachylite",
        native_options,
        Box::new(|cc| Ok(Box::new(tachylite::TachyliteApp::new(cc)))),
    )
}
