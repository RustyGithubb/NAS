//! Onglets. Services, Journaux et Fichiers sont encore vides.

pub mod dashboard;
pub mod settings;

use eframe::egui::{self, CornerRadius, Margin, Stroke};

use crate::theme;

/// En-tête d'onglet : grand titre et sous-titre discret.
pub fn header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(egui::RichText::new(title).font(theme::title_font(28.0)).color(theme::TEXT()));
    ui.add_space(-4.0);
    ui.label(egui::RichText::new(subtitle).color(theme::TEXT_MUTED()));
    ui.add_space(16.0);
}

/// Carte : surface légèrement plus claire que le fond, bord fin, coins arrondis.
pub fn card<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::SURFACE())
        .stroke(Stroke::new(1.0, theme::BORDER()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).font(theme::title_font(15.0)).color(theme::TEXT()));
            ui.add_space(6.0);
            body(ui)
        })
        .inner
}

/// Carte « bientôt disponible » d'un onglet pas encore construit.
fn coming(ui: &mut egui::Ui, what: &str) {
    card(ui, t!("Bientôt", "Coming soon"), |ui| {
        ui.label(egui::RichText::new(what).color(theme::TEXT_MUTED()));
    });
}

pub fn services(ui: &mut egui::Ui) {
    header(ui, "Services", t!("Programmes qui tournent en permanence sur le serveur", "Programs running permanently on the server"));
    coming(ui, t!("Liste des services, avec démarrer, arrêter et redémarrer.", "Service list, with start, stop and restart."));
}

pub fn logs(ui: &mut egui::Ui) {
    header(ui, t!("Journaux", "Logs"), t!("Ce que les services écrivent, en direct", "What services write, live"));
    coming(ui, t!("Journal de chaque service, mis à jour en continu.", "Each service's log, updated continuously."));
}

pub fn files(ui: &mut egui::Ui) {
    header(ui, t!("Fichiers", "Files"), t!("Transfert et synchronisation", "Transfer and sync"));
    coming(ui, t!("Envoi, récupération et synchronisation de fichiers avec le serveur.", "Upload, download and sync files with the server."));
}
