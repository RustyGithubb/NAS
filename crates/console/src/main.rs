//! NAS — console : pilotage du serveur depuis Windows.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod account_panel;
mod app;
mod drives;
mod icons;
mod instance;
mod liquid;
#[cfg(windows)]
mod ole_drag;
mod overlay;
mod prefs;
mod preview;
mod secret;
mod server;
mod sidebar;
mod tabs;
mod theme;
mod titlebar;
mod update;
mod widgets;

fn main() -> eframe::Result<()> {
    // Déjà ouverte : elle revient au premier plan au lieu d'en ouvrir une deuxième.
    if !instance::single_instance() {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("NAS")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([820.0, 540.0])
            // Pas de barre de titre Windows : l'app dessine la sienne (déplacement, boutons, bords).
            .with_decorations(false)
            // Icône de la fenêtre et de la barre des tâches (logo de la barre latérale).
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo.png")).unwrap_or_default()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "NAS",
        options,
        Box::new(|cc| {
            instance::start(cc);
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::NasApp::new(&cc.egui_ctx)))
        }),
    )
}
