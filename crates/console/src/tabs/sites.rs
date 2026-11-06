//! Sites : sites web hébergés sur le serveur (WordPress, pages fixes), avec mise en ligne,
//! maintenance, coupure et sauvegarde.

use eframe::egui::{self, RichText};
use nas_proto::{Site, SiteAction, SiteKind, SiteState};

use super::{card, header, unavailable};
use crate::server::{Link, Server};
use crate::{theme, widgets};

/// Renvoie le service dont on veut voir le journal (celui de Caddy, le serveur web).
pub fn show(ui: &mut egui::Ui, server: &Server, link: &Link, addr: &str) -> Option<String> {
    let title = "Sites";
    if unavailable(ui, title, link, addr) {
        return None;
    }
    let (sites, pending, last) = {
        let d = server.data();
        (d.sites.clone(), d.sites_pending.clone(), d.last_site_action.clone())
    };
    let Some(sites) = sites else {
        header(ui, title, t!("Chargement…", "Loading…"));
        return None;
    };
    let online = sites.list.iter().filter(|s| s.state == SiteState::Online).count();
    let mut sub = tf!("{} en ligne sur {}", "{} online out of {}", online, sites.list.len());
    if sites.simulated {
        sub.push_str(t!("  ·  données simulées", "  ·  simulated data"));
    }
    header(ui, title, &sub);

    // Résultat de la dernière action (ligne de hauteur fixe : la liste ne bouge pas).
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        if let Some((name, action, result)) = &last {
            let done = match action {
                SiteAction::Set(SiteState::Online) => t!("mis en ligne", "put online"),
                SiteAction::Set(SiteState::Maintenance) => t!("mis en maintenance", "put in maintenance"),
                SiteAction::Set(SiteState::Offline) => t!("coupé", "taken offline"),
                SiteAction::Backup => t!("sauvegardé", "backed up"),
            };
            let text = if result.ok {
                RichText::new(format!("✓ {name} {done}.")).color(theme::TEXT_MUTED())
            } else {
                RichText::new(tf!("✗ {name} : {}", "✗ {name}: {}", result.message)).color(theme::TODAY())
            };
            ui.add(egui::Label::new(text).truncate());
            if ui.small_button("✕").on_hover_text(t!("Masquer", "Hide")).clicked() {
                server.data().last_site_action = None;
            }
        }
    });
    ui.add_space(6.0);

    let mut open_logs = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let width = ui.available_width().min(860.0);
        ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
            if sites.list.is_empty() {
                card(ui, t!("Aucun site", "No site"), |ui| {
                    ui.label(RichText::new(t!("Aucun site n'est encore déclaré sur le serveur.", "No site is declared on the server yet.")).color(theme::TEXT_MUTED()));
                });
            }
            for site in &sites.list {
                if let Some(a) = site_card(ui, site, pending.contains(&site.name)) {
                    match a {
                        CardAction::Do(action) => server.site_action(&site.name, action),
                        CardAction::Logs => open_logs = Some("caddy.service".to_string()),
                    }
                }
                ui.add_space(12.0);
            }
        });
    });
    open_logs
}

enum CardAction {
    Do(SiteAction),
    Logs,
}

fn site_card(ui: &mut egui::Ui, s: &Site, busy: bool) -> Option<CardAction> {
    let mut out = None;
    card(ui, &s.name, |ui| {
        // Adresse et type.
        ui.horizontal(|ui| {
            ui.hyperlink_to(RichText::new(&s.domain).color(theme::TEXT()), format!("https://{}", s.domain));
            let kind = match s.kind {
                SiteKind::WordPress => "WordPress",
                SiteKind::Static => t!("Pages fixes", "Static pages"),
            };
            ui.label(RichText::new(format!("·  {kind}")).color(theme::TEXT_MUTED()));
        });
        ui.add_space(10.0);

        // État : sélecteur segmenté (la pastille glisse d'un état à l'autre).
        row(ui, t!("État", "Status"), t!("Visible par tous, page d'attente, ou coupé", "Visible to all, holding page, or off"), |ui| {
            if busy {
                ui.spinner();
                return;
            }
            let opts = [(SiteState::Offline, t!("Coupé", "Off")), (SiteState::Maintenance, "Maintenance"), (SiteState::Online, t!("En ligne", "Online"))];
            let mut state = s.state;
            if widgets::segmented(ui, &format!("site_state_{}", s.name), &opts, &mut state) {
                out = Some(CardAction::Do(SiteAction::Set(state)));
            }
        });
        separator(ui);

        let cert = match s.cert_expires_ms {
            Some(ms) => {
                let days = (ms - chrono::Utc::now().timestamp_millis()) / 86_400_000;
                let text = tf!("valide encore {} jours", "valid for {} more days", days);
                // Caddy renouvelle seul vers 30 jours ; en dessous de 14, quelque chose bloque.
                RichText::new(text).color(if days < 14 { theme::TODAY() } else { theme::TEXT_MUTED() })
            }
            None => RichText::new(t!("pas encore obtenu (à la première mise en ligne)", "not obtained yet (on first going online)")).color(theme::TEXT_MUTED()),
        };
        row(ui, t!("Certificat HTTPS", "HTTPS certificate"), t!("Renouvelé automatiquement par Caddy", "Renewed automatically by Caddy"), |ui| {
            ui.label(cert);
        });
        separator(ui);

        row(ui, t!("Médias", "Media"), t!("Images et fichiers du site, aussi dans l'onglet Fichiers", "Site images and files, also in the Files tab"), |ui| {
            ui.label(RichText::new(&s.media_dir).monospace().color(theme::TEXT_MUTED()));
        });
        separator(ui);

        let last = s.last_backup_ms.map(date).unwrap_or_else(|| t!("jamais", "never").to_string());
        row(ui, t!("Sauvegarde", "Backup"), &tf!("Dernière : {last}", "Last: {last}"), |ui| {
            if ui.add_enabled(!busy, egui::Button::new(t!("Sauvegarder maintenant", "Back up now"))).clicked() {
                out = Some(CardAction::Do(SiteAction::Backup));
            }
        });
        separator(ui);

        row(ui, t!("Journal", "Log"), t!("Journal du serveur web (Caddy)", "Web server log (Caddy)"), |ui| {
            if ui.button(t!("Ouvrir", "Open")).clicked() {
                out = Some(CardAction::Logs);
            }
        });
    });
    out
}

/// Ligne : intitulé et aide à gauche, contrôle à droite (comme les Réglages).
fn row(ui: &mut egui::Ui, label: &str, hint: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(label);
            ui.label(RichText::new(hint).small().color(theme::TEXT_FAINT()));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

fn separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}

/// « 06/11/2026 21:40 », à l'heure locale.
fn date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default()
}
