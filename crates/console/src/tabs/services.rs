//! Services : liste des unités systemd, avec démarrer, arrêter, redémarrer.

use eframe::egui::{self, Stroke};
use nas_proto::{Action, Service, ServiceState};

use super::{card, header, state_dot, unavailable};
use crate::server::{Link, Server};
use crate::theme;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Active,
    Stopped,
    Failed,
}

impl Filter {
    const ALL: [Filter; 4] = [Filter::All, Filter::Active, Filter::Stopped, Filter::Failed];

    fn label(self) -> &'static str {
        match self {
            Filter::All => t!("Tous", "All"),
            Filter::Active => t!("Actifs", "Active"),
            Filter::Stopped => t!("Arrêtés", "Stopped"),
            Filter::Failed => t!("En échec", "Failed"),
        }
    }

    fn keeps(self, s: &Service) -> bool {
        match self {
            Filter::All => true,
            Filter::Active => matches!(s.state, ServiceState::Active | ServiceState::Changing),
            Filter::Stopped => s.state == ServiceState::Inactive,
            Filter::Failed => s.state == ServiceState::Failed,
        }
    }
}

/// Recherche et filtre (gardés d'une visite à l'autre).
#[derive(Default)]
pub struct State {
    query: String,
    filter: Filter,
}

/// Renvoie le service dont on veut voir le journal.
pub fn show(ui: &mut egui::Ui, state: &mut State, server: &Server, link: &Link, addr: &str) -> Option<String> {
    let title = "Services";
    if unavailable(ui, title, link, addr) {
        return None;
    }
    // Copie des données utiles, pour ne pas garder le verrou pendant le dessin.
    let (services, pending, last) = {
        let d = server.data();
        (d.services.clone(), d.pending.clone(), d.last_action.clone())
    };
    let Some(services) = services else {
        header(ui, title, t!("Chargement…", "Loading…"));
        return None;
    };
    let active = services.list.iter().filter(|s| s.state == ServiceState::Active).count();
    let mut sub = tf!("{} actifs sur {}", "{} active out of {}", active, services.list.len());
    if services.simulated {
        sub.push_str(t!("  ·  données simulées", "  ·  simulated data"));
    }
    header(ui, title, &sub);

    // Recherche et filtres ; à droite, résultat de la dernière action (sur la même ligne, pour
    // ne pas décaler la liste sous le pointeur au moment où il apparaît).
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(t!("Rechercher un service…", "Search services…")).desired_width(260.0));
        ui.add_space(8.0);
        for f in Filter::ALL {
            if ui.selectable_label(state.filter == f, f.label()).clicked() {
                state.filter = f;
            }
        }
        if let Some((unit, action, result)) = last {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").on_hover_text(t!("Masquer", "Hide")).clicked() {
                    server.data().last_action = None;
                }
                let done = match action {
                    Action::Start => t!("démarré", "started"),
                    Action::Stop => t!("arrêté", "stopped"),
                    Action::Restart => t!("redémarré", "restarted"),
                };
                let text = if result.ok {
                    egui::RichText::new(format!("✓ {unit} {done}.")).color(theme::TEXT_MUTED())
                } else {
                    egui::RichText::new(tf!("✗ {unit} : {}", "✗ {unit}: {}", result.message)).color(theme::TODAY())
                };
                ui.add(egui::Label::new(text).truncate());
            });
        }
    });
    ui.add_space(10.0);

    let q = state.query.to_lowercase();
    let shown: Vec<&Service> =
        services.list.iter().filter(|s| state.filter.keeps(s) && (q.is_empty() || s.name.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q))).collect();

    let mut open_logs = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        card(ui, &tf!("{} service(s)", "{} service(s)", shown.len()), |ui| {
            if shown.is_empty() {
                ui.label(egui::RichText::new(t!("Aucun service ne correspond.", "No matching service.")).color(theme::TEXT_MUTED()));
            }
            for (i, s) in shown.iter().enumerate() {
                if i > 0 {
                    let w = ui.available_width();
                    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
                    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, theme::BORDER()));
                }
                if let Some(a) = row(ui, s, pending.contains(&s.name)) {
                    match a {
                        RowAction::Do(action) => server.action(&s.name, action),
                        RowAction::Logs => open_logs = Some(s.name.clone()),
                    }
                }
            }
        });
    });
    open_logs
}

enum RowAction {
    Do(Action),
    Logs,
}

/// Une ligne : pastille, nom et description, état détaillé, boutons à droite.
fn row(ui: &mut egui::Ui, s: &Service, busy: bool) -> Option<RowAction> {
    let mut out = None;
    ui.horizontal(|ui| {
        state_dot(ui, s.state);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(&s.name).color(theme::TEXT()));
            ui.label(egui::RichText::new(&s.description).font(theme::editor_font(12.0, false)).color(theme::TEXT_MUTED()));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t!("Journal", "Log")).clicked() {
                out = Some(RowAction::Logs);
            }
            if busy {
                ui.spinner();
                return;
            }
            let running = matches!(s.state, ServiceState::Active | ServiceState::Changing);
            if running {
                if ui.button(t!("Redémarrer", "Restart")).clicked() {
                    out = Some(RowAction::Do(Action::Restart));
                }
                if ui.button(t!("Arrêter", "Stop")).clicked() {
                    out = Some(RowAction::Do(Action::Stop));
                }
            } else if ui.button(t!("Démarrer", "Start")).clicked() {
                out = Some(RowAction::Do(Action::Start));
            }
            ui.label(egui::RichText::new(&s.sub).small().color(if s.state == ServiceState::Failed { theme::TODAY() } else { theme::TEXT_MUTED() }));
        });
    });
    out
}
