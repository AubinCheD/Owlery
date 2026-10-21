// Pas de console sous Windows en version publiée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use eframe::egui;

fn main() -> eframe::Result {
    let demo = std::env::var_os("OWLERY_DEMO").is_some_and(|v| v == "1");
    // La démonstration n'écrit jamais dans l'état réel.
    let dir = if demo { owlery::state::data_dir().join("demo") } else { owlery::state::data_dir() };
    let loaded = owlery::state::load(&dir);
    let viewport = egui::ViewportBuilder::default()
        .with_title("Owlery")
        .with_inner_size([1100.0, 700.0])
        .with_min_inner_size([640.0, 360.0]);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Owlery",
        options,
        Box::new(move |cc| {
            Ok(Box::new(owlery::app::App::new(cc, dir, loaded, Arc::new(owlery::secrets::OsStore), demo)))
        }),
    )
}
