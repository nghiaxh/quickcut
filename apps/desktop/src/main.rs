mod app;

use app::Studio;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "QuickCut",
        options,
        Box::new(|cc| Ok(Box::new(Studio::new(cc)))),
    )
}
