//! NAS — console : pilotage du serveur depuis Windows.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod app;
mod liquid;
mod prefs;
mod secret;
mod server;
mod sidebar;
mod tabs;
mod theme;
mod titlebar;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("NAS")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([820.0, 540.0])
            // Pas de barre de titre Windows : l'app dessine la sienne (déplacement, boutons, bords).
            .with_decorations(false),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "NAS",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::NasApp::new(&cc.egui_ctx)))
        }),
    )
}
