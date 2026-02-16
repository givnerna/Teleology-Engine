fn main() -> eframe::Result<()> {
    eframe::run_native(
        "Teleology — Visual Editor",
        eframe::NativeOptions::default(),
        Box::new(|cc| Ok(Box::new(teleology_editor::EditorApp::new(cc)))),
    )
}
