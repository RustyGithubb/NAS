//! Sites : sites web hébergés sur le serveur (WordPress, pages fixes). Liste à gauche, détail
//! du site choisi à droite : mise en ligne, maintenance, coupure, sauvegarde.

use eframe::egui::{self, RichText};
use nas_proto::{Site, SiteAction, SiteKind, SiteState};

use super::{header, result_line, row, separator, unavailable};
use crate::liquid::Liquid;
use crate::server::{Link, Server};
use crate::{theme, widgets};

/// Site choisi (gardé d'une visite à l'autre).
#[derive(Default)]
pub struct State {
    selected: Option<String>,
}

/// Renvoie le service dont on veut voir le journal (celui de Caddy, le serveur web).
pub fn show(ui: &mut egui::Ui, st: &mut State, server: &Server, link: &Link, addr: &str) -> Option<String> {
    let (overline, title) = (t!("Hébergement", "Hosting"), "Sites");
    if unavailable(ui, overline, title, link, addr) {
        return None;
    }
    let (sites, pending, last) = {
        let d = server.data();
        (d.sites.clone(), d.sites_pending.clone(), d.last_site_action.clone())
    };
    let sites = sites.unwrap_or_default();
    let online = sites.list.iter().filter(|s| s.state == SiteState::Online).count();
    let mut badge = tf!("{} en ligne sur {}", "{} online out of {}", online, sites.list.len());
    if sites.simulated {
        badge.push_str(t!("  ·  simulé", "  ·  simulated"));
    }
    header(ui, overline, title, &badge, false);
    if st.selected.as_ref().is_none_or(|n| !sites.list.iter().any(|s| &s.name == n)) {
        st.selected = sites.list.first().map(|s| s.name.clone());
    }

    let mut open_logs = None;
    widgets::pane_columns(ui, &[widgets::SIDE_W], |i, ui| {
        if i == 0 {
            widgets::side_label(ui, "Sites");
            let mut hl = Liquid::begin(ui, "sites_list");
            for s in &sites.list {
                let state = s.state;
                let tone = if state == SiteState::Offline { widgets::Tone::Off } else { widgets::Tone::Normal };
                let dot = move |p: &egui::Painter, c: egui::Pos2| {
                    match state {
                        SiteState::Online => p.circle_filled(c, 4.0, theme::TEXT()),
                        SiteState::Maintenance => p.circle_filled(c, 4.0, theme::TODAY()),
                        SiteState::Offline => p.circle_stroke(c, 3.5, egui::Stroke::new(1.2, theme::TEXT_MUTED())),
                    };
                };
                if widgets::nav_row(ui, &s.name, None, st.selected.as_deref() == Some(&s.name), tone, &mut hl, Some(&dot)).clicked() {
                    st.selected = Some(s.name.clone());
                }
            }
            widgets::nav_end(ui, hl);
            if sites.list.is_empty() {
                widgets::empty_state(ui, t!("Aucun site", "No site"), t!("Aucun site déclaré sur le serveur", "No site declared on the server"));
            }
        } else {
            let Some(site) = st.selected.as_ref().and_then(|n| sites.list.iter().find(|s| &s.name == n)) else {
                ui.add_space(40.0);
                widgets::empty_state(ui, t!("Aucun site choisi", "No site selected"), t!("Choisis un site à gauche", "Pick a site on the left"));
                return;
            };
            egui::ScrollArea::vertical().id_salt("site_detail").auto_shrink([false, false]).show(ui, |ui| {
                if let Some(a) = site_card(ui, site, pending.contains(&site.name)) {
                    match a {
                        CardAction::Do(action) => server.site_action(&site.name, action),
                        CardAction::Logs => open_logs = Some("caddy.service".to_string()),
                    }
                }
                if let Some((name, action, result)) = last.as_ref().filter(|l| l.0 == site.name) {
                    ui.add_space(8.0);
                    let done = match action {
                        SiteAction::Set(SiteState::Online) => t!("mis en ligne", "put online"),
                        SiteAction::Set(SiteState::Maintenance) => t!("mis en maintenance", "put in maintenance"),
                        SiteAction::Set(SiteState::Offline) => t!("coupé", "taken offline"),
                        SiteAction::Backup => t!("sauvegardé", "backed up"),
                    };
                    let text = if result.ok { format!("{name} {done}.") } else { result.message.clone() };
                    if result_line(ui, result.ok, &text) {
                        server.data().last_site_action = None;
                    }
                }
            });
        }
    });
    open_logs
}

enum CardAction {
    Do(SiteAction),
    Logs,
}

fn site_card(ui: &mut egui::Ui, s: &Site, busy: bool) -> Option<CardAction> {
    let mut out = None;
    {
        let mut logs = false;
        widgets::pane_title(ui, &s.name, |ui| {
            logs = ui.button(t!("Journal", "Log")).on_hover_text(t!("Journal du serveur web (Caddy)", "Web server log (Caddy)")).clicked();
        });
        if logs {
            out = Some(CardAction::Logs);
        }
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
    }
    out
}

/// « 06/11/2026 21:40 », à l'heure locale.
fn date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default()
}
