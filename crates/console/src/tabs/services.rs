//! Services : unités systemd. Trois panneaux comme la messagerie de WorkTable : filtres à
//! gauche, liste au milieu, détail et actions du service choisi à droite.

use eframe::egui::{self, RichText};
use nas_proto::{Action, Service, ServiceState};

use super::{header, paint_state, result_line, row, separator, unavailable};
use crate::liquid::Liquid;
use crate::server::{Link, Server};
use crate::{theme, widgets};

/// Largeur de la liste (colonne du milieu).
const LIST_W: f32 = 420.0;

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
            Filter::All => t!("Tous les services", "All services"),
            Filter::Active => t!("Actifs", "Active"),
            Filter::Stopped => t!("Arrêtés", "Stopped"),
            Filter::Failed => t!("Problèmes", "Problems"),
        }
    }

    fn keeps(self, s: &Service) -> bool {
        match self {
            Filter::All => true,
            Filter::Active => matches!(s.state, ServiceState::Active | ServiceState::Changing),
            Filter::Stopped => s.state == ServiceState::Inactive,
            Filter::Failed => s.problem(),
        }
    }
}

/// Recherche, filtre et service choisi (gardés d'une visite à l'autre).
#[derive(Default)]
pub struct State {
    query: String,
    filter: Filter,
    selected: Option<String>,
}

/// Renvoie le service dont on veut voir le journal.
pub fn show(ui: &mut egui::Ui, st: &mut State, server: &Server, link: &Link, addr: &str) -> Option<String> {
    let (overline, title) = (t!("Serveur", "Server"), "Services");
    if unavailable(ui, overline, title, link, addr) {
        return None;
    }
    let (services, pending, last) = {
        let d = server.data();
        (d.services.clone(), d.pending.clone(), d.last_action.clone())
    };
    let services = services.unwrap_or_default();
    let failed = services.list.iter().filter(|s| s.problem()).count();
    let active = services.list.iter().filter(|s| s.state == ServiceState::Active).count();
    let mut badge = if failed > 0 {
        if failed == 1 { t!("1 problème", "1 problem").to_string() } else { tf!("{failed} problèmes", "{failed} problems") }
    } else {
        tf!("{} actifs sur {}", "{} active out of {}", active, services.list.len())
    };
    if services.simulated {
        badge.push_str(t!("  ·  simulé", "  ·  simulated"));
    }
    header(ui, overline, title, &badge, failed > 0);

    let q = st.query.to_lowercase();
    let shown: Vec<&Service> =
        services.list.iter().filter(|s| st.filter.keeps(s) && (q.is_empty() || s.name.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q))).collect();
    if st.selected.as_ref().is_none_or(|n| !services.list.iter().any(|s| &s.name == n)) {
        st.selected = shown.first().map(|s| s.name.clone());
    }

    let mut out = None;
    widgets::pane_columns(ui, &[widgets::SIDE_W, LIST_W], |i, ui| match i {
        0 => {
            ui.add(egui::TextEdit::singleline(&mut st.query).hint_text(t!("Rechercher…", "Search…")).desired_width(f32::INFINITY));
            ui.add_space(14.0);
            widgets::side_label(ui, t!("Filtres", "Filters"));
            let mut hl = Liquid::begin(ui, "services_filters");
            for f in Filter::ALL {
                let n = services.list.iter().filter(|s| f.keeps(s)).count();
                // « Problèmes » ressort tant qu'il en reste un.
                let tone = if f == Filter::Failed && n > 0 { widgets::Tone::Alert } else { widgets::Tone::Normal };
                if widgets::nav_row(ui, f.label(), Some(n), st.filter == f, tone, &mut hl, None).clicked() {
                    st.filter = f;
                }
            }
            widgets::nav_end(ui, hl);
        }
        1 => {
            widgets::pane_title(ui, st.filter.label(), |ui| {
                ui.label(RichText::new(shown.len().to_string()).color(theme::TEXT_FAINT()));
            });
            egui::ScrollArea::vertical().id_salt("services_list").auto_shrink([false, false]).show(ui, |ui| {
                if shown.is_empty() {
                    widgets::empty_state(ui, t!("Aucun service", "No service"), t!("Rien ne correspond à ce filtre", "Nothing matches this filter"));
                }
                for s in &shown {
                    let sel = st.selected.as_deref() == Some(&s.name);
                    let tone = super::service_tone(s);
                    let resp = widgets::list_item(ui, sel, tone, |ui| {
                        ui.horizontal(|ui| {
                            super::state_dot(ui, s);
                            ui.label(RichText::new(&s.name).color(tone.text(sel)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(&s.sub).small().color(if s.problem() { theme::DANGER() } else { theme::TEXT_FAINT() }));
                            });
                        });
                        ui.add(egui::Label::new(RichText::new(&s.description).font(theme::editor_font(12.5, false)).color(theme::TEXT_FAINT())).truncate());
                    });
                    if resp.clicked() {
                        st.selected = Some(s.name.clone());
                    }
                }
            });
        }
        _ => {
            let Some(s) = st.selected.as_ref().and_then(|n| services.list.iter().find(|s| &s.name == n)) else {
                ui.add_space(40.0);
                widgets::empty_state(ui, t!("Aucun service choisi", "No service selected"), t!("Choisis un service dans la liste", "Pick a service in the list"));
                return;
            };
            if let Some(a) = detail(ui, s, pending.contains(&s.name)) {
                match a {
                    Detail::Do(action) => server.action(&s.name, action),
                    Detail::Logs => out = Some(s.name.clone()),
                }
            }
            if let Some((unit, action, result)) = last.as_ref().filter(|l| l.0 == s.name) {
                ui.add_space(8.0);
                let done = match action {
                    Action::Start => t!("démarré", "started"),
                    Action::Stop => t!("arrêté", "stopped"),
                    Action::Restart => t!("redémarré", "restarted"),
                };
                let text = if result.ok { format!("{unit} {done}.") } else { result.message.clone() };
                if result_line(ui, result.ok, &text) {
                    server.data().last_action = None;
                }
            }
        }
    });
    out
}

enum Detail {
    Do(Action),
    Logs,
}

/// Détail du service choisi : nom, description, état, actions.
fn detail(ui: &mut egui::Ui, s: &Service, busy: bool) -> Option<Detail> {
    let mut out = None;
    widgets::pane_title(ui, &s.name, |ui| {
        if ui.button(t!("Journal", "Log")).clicked() {
            out = Some(Detail::Logs);
        }
    });
    ui.label(RichText::new(&s.description).font(theme::editor_font(14.0, false)).color(theme::TEXT_MUTED()));
    ui.add_space(14.0);
    let state = match s.state {
        ServiceState::Active => t!("Actif", "Active"),
        ServiceState::Inactive if s.enabled => t!("Arrêté (devrait tourner)", "Stopped (should be running)"),
        ServiceState::Inactive => t!("Arrêté", "Stopped"),
        ServiceState::Failed => t!("En échec", "Failed"),
        ServiceState::Changing => t!("En cours de changement", "Changing"),
    };
    row(ui, t!("État", "Status"), &tf!("Détail systemd : {}", "systemd detail: {}", s.sub), |ui| {
        ui.label(RichText::new(state).color(if s.problem() { theme::DANGER() } else { theme::TEXT() }));
        let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
        paint_state(ui.painter(), r.center(), s);
    });
    separator(ui);
    // Actions sur leur propre ligne : la colonne peut être étroite.
    ui.label(t!("Actions", "Actions"));
    ui.label(RichText::new(t!("Démarrer, arrêter ou redémarrer le service", "Start, stop or restart the service")).small().color(theme::TEXT_FAINT()));
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if busy {
            ui.spinner();
            return;
        }
        if matches!(s.state, ServiceState::Active | ServiceState::Changing) {
            if ui.button(t!("Redémarrer", "Restart")).clicked() {
                out = Some(Detail::Do(Action::Restart));
            }
            if ui.button(t!("Arrêter", "Stop")).clicked() {
                out = Some(Detail::Do(Action::Stop));
            }
        } else if ui.button(t!("Démarrer", "Start")).clicked() {
            out = Some(Detail::Do(Action::Start));
        }
    });
    out
}
