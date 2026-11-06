//! Onglets. Fichiers est encore vide.

pub mod dashboard;
pub mod logs;
pub mod services;
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

/// Carte « agent injoignable », commune aux onglets qui ont besoin de l'agent.
pub fn offline(ui: &mut egui::Ui, err: &str, addr: &str) {
    card(ui, t!("Agent injoignable", "Agent unreachable"), |ui| {
        ui.label(egui::RichText::new(tf!("{addr} : {err}.", "{addr}: {err}.")).color(theme::TEXT_MUTED()));
        ui.add_space(4.0);
        ui.label(t!("En développement, lance l'agent sur ce PC dans un autre terminal :", "During development, start the agent on this PC in another terminal:"));
        ui.label(egui::RichText::new("cargo run -p nas-agent").monospace().color(theme::TEXT()));
        ui.label(egui::RichText::new(t!("L'adresse se change dans Réglages › Serveur.", "The address can be changed in Settings › Server.")).color(theme::TEXT_MUTED()));
    });
}

/// Carte « clé d'accès refusée ».
pub fn denied(ui: &mut egui::Ui) {
    card(ui, t!("Clé d'accès refusée", "Access key refused"), |ui| {
        ui.label(t!(
            "L'agent répond, mais la clé d'accès est absente ou fausse. Pour l'afficher, sur la machine de l'agent :",
            "The agent answers, but the access key is missing or wrong. To show it, on the agent's machine:"
        ));
        ui.label(egui::RichText::new("cargo run -p nas-agent -- --show-key").monospace().color(theme::TEXT()));
        ui.label(egui::RichText::new(t!("Puis colle-la dans Réglages › Serveur.", "Then paste it in Settings › Server.")).color(theme::TEXT_MUTED()));
    });
}

/// En-tête et carte d'explication si l'agent n'est pas utilisable. Renvoie `true` dans ce cas.
pub fn unavailable(ui: &mut egui::Ui, title: &str, link: &crate::server::Link, addr: &str) -> bool {
    use crate::server::Link;
    match link {
        Link::Offline(err) => {
            header(ui, title, t!("Hors ligne", "Offline"));
            offline(ui, err, addr);
            true
        }
        Link::Denied => {
            header(ui, title, t!("Accès refusé", "Access refused"));
            denied(ui);
            true
        }
        Link::Connecting | Link::Online(_) => false,
    }
}

/// Pastille d'état d'un service : pleine si actif, creuse si arrêté, accent si en échec.
pub fn state_dot(ui: &mut egui::Ui, state: nas_proto::ServiceState) {
    use nas_proto::ServiceState as S;
    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 18.0), egui::Sense::hover());
    let c = r.center();
    let p = ui.painter();
    match state {
        S::Active => p.circle_filled(c, 4.0, theme::TEXT()),
        S::Inactive => p.circle_stroke(c, 3.5, Stroke::new(1.2, theme::TEXT_MUTED())),
        S::Failed => p.circle_filled(c, 4.0, theme::TODAY()),
        S::Changing => p.circle_stroke(c, 3.5, Stroke::new(1.5, theme::TEXT())),
    };
}

/// Carte « bientôt disponible » d'un onglet pas encore construit.
fn coming(ui: &mut egui::Ui, what: &str) {
    card(ui, t!("Bientôt", "Coming soon"), |ui| {
        ui.label(egui::RichText::new(what).color(theme::TEXT_MUTED()));
    });
}

pub fn files(ui: &mut egui::Ui) {
    header(ui, t!("Fichiers", "Files"), t!("Transfert et synchronisation", "Transfer and sync"));
    coming(ui, t!("Envoi, récupération et synchronisation de fichiers avec le serveur.", "Upload, download and sync files with the server."));
}
