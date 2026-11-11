//! Onglets. Tous organisés comme ceux de WorkTable : en-tête (sur-titre, titre, info), puis
//! des panneaux pleine hauteur côte à côte (colonne de gauche de 260 px, puis le reste).

pub mod dashboard;
pub mod files;
pub mod logs;
pub mod services;
pub mod settings;
pub mod sites;

use eframe::egui::{self, RichText, Stroke};

use crate::server::Link;
use crate::{theme, widgets};

/// En-tête d'onglet : sur-titre, titre, et information à côté (en rouge si `alert`).
pub fn header(ui: &mut egui::Ui, overline: &str, title: &str, badge: &str, alert: bool) {
    widgets::page_header_with(ui, overline, title, |ui| {
        if !badge.is_empty() {
            widgets::header_badge(ui, badge, alert);
        }
    });
}

/// Si l'agent n'est pas utilisable : en-tête, puis un panneau pleine page qui explique quoi
/// faire. Renvoie `true` dans ce cas (l'onglet s'arrête là).
pub fn unavailable(ui: &mut egui::Ui, overline: &str, title: &str, link: &Link, addr: &str) -> bool {
    match link {
        Link::Offline(err) => {
            header(ui, overline, title, t!("Hors ligne", "Offline"), true);
            widgets::pane(ui, |ui| {
                ui.add_space(40.0);
                widgets::empty_state(ui, t!("Agent injoignable", "Agent unreachable"), &tf!("{addr} : {err}", "{addr}: {err}"));
                hint(
                    ui,
                    t!("En développement, lance l'agent sur ce PC dans un autre terminal :", "During development, start the agent on this PC in another terminal:"),
                    "cargo run -p nas-agent",
                );
            });
            true
        }
        Link::Denied => {
            header(ui, overline, title, t!("Accès refusé", "Access refused"), true);
            widgets::pane(ui, |ui| {
                ui.add_space(40.0);
                widgets::empty_state(
                    ui,
                    t!("Clé d'accès refusée", "Access key refused"),
                    t!("L'agent répond, mais la clé est absente ou fausse", "The agent answers, but the key is missing or wrong"),
                );
                hint(
                    ui,
                    t!(
                        "Pour l'afficher, sur la machine de l'agent (puis la coller dans Réglages › Serveur) :",
                        "To show it, on the agent's machine (then paste it in Settings › Server):"
                    ),
                    "cargo run -p nas-agent -- --show-key",
                );
            });
            true
        }
        Link::Connecting => {
            header(ui, overline, title, t!("Connexion…", "Connecting…"), false);
            widgets::pane(ui, |ui| {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.spinner());
            });
            true
        }
        Link::Online(_) => false,
    }
}

fn hint(ui: &mut egui::Ui, text: &str, cmd: &str) {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).color(theme::TEXT_MUTED()));
        ui.add_space(4.0);
        ui.label(RichText::new(cmd).monospace().color(theme::TEXT()));
    });
}

/// Pastille d'état d'un service : pleine si actif, creuse si arrêté, rouge si problème (en
/// échec : pleine ; arrêté alors qu'il devrait tourner : creuse).
pub fn paint_state(p: &egui::Painter, c: egui::Pos2, s: &nas_proto::Service) {
    use nas_proto::ServiceState as S;
    match s.state {
        S::Active => p.circle_filled(c, 4.0, theme::TEXT()),
        S::Inactive if s.enabled => p.circle_stroke(c, 3.5, Stroke::new(1.6, theme::DANGER())),
        S::Inactive => p.circle_stroke(c, 3.5, Stroke::new(1.2, theme::TEXT_MUTED())),
        S::Failed => p.circle_filled(c, 4.0, theme::DANGER()),
        S::Changing => p.circle_stroke(c, 3.5, Stroke::new(1.5, theme::TEXT())),
    };
}

/// Ton d'un service dans les listes : problème → alerte (rouge), arrêté normalement (tâche
/// ponctuelle terminée) → éteint.
pub fn service_tone(s: &nas_proto::Service) -> widgets::Tone {
    if s.problem() {
        widgets::Tone::Alert
    } else if s.state == nas_proto::ServiceState::Inactive {
        widgets::Tone::Off
    } else {
        widgets::Tone::Normal
    }
}

/// Pastille d'état dans une mise en page.
pub fn state_dot(ui: &mut egui::Ui, s: &nas_proto::Service) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 18.0), egui::Sense::hover());
    paint_state(ui.painter(), r.center(), s);
}

/// Ligne de détail : intitulé et aide à gauche, contrôle à droite (comme les Réglages).
pub fn row(ui: &mut egui::Ui, label: &str, hint: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(label);
            if !hint.is_empty() {
                ui.label(RichText::new(hint).small().color(theme::TEXT_FAINT()));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

pub fn separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}

/// Résultat de la dernière action, en une ligne (✓ en gris, ✗ en rouge) avec ✕ pour masquer.
/// Renvoie `true` si on a cliqué sur ✕.
pub fn result_line(ui: &mut egui::Ui, ok: bool, text: &str) -> bool {
    let mut hide = false;
    ui.horizontal(|ui| {
        let t = if ok { RichText::new(format!("✓ {text}")).color(theme::TEXT_MUTED()) } else { RichText::new(format!("✗ {text}")).color(theme::DANGER()) };
        ui.add(egui::Label::new(t).truncate());
        hide = ui.small_button("✕").on_hover_text(t!("Masquer", "Hide")).clicked();
    });
    hide
}
