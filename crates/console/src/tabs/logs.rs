//! Journaux : ce qu'écrit un service, mis à jour en direct.

use eframe::egui::{self, CornerRadius, Margin, Stroke};
use nas_proto::LogEntry;

use super::{header, unavailable};
use crate::server::{Link, Server};
use crate::theme;

/// Service affiché, suivi en direct, filtre (gardés d'une visite à l'autre).
pub struct State {
    pub unit: Option<String>,
    follow: bool,
    query: String,
}

impl Default for State {
    fn default() -> Self {
        State { unit: None, follow: true, query: String::new() }
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut State, server: &Server, link: &Link, addr: &str) {
    let title = t!("Journaux", "Logs");
    if unavailable(ui, title, link, addr) {
        return;
    }
    header(ui, title, t!("Ce que les services écrivent, en direct", "What services write, live"));
    server.follow_logs(state.unit.as_deref());

    let names: Vec<String> = server.data().services.as_ref().map(|s| s.list.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
    ui.horizontal(|ui| {
        let current = state.unit.clone().unwrap_or_else(|| t!("Choisir un service", "Choose a service").to_string());
        egui::ComboBox::from_id_salt("nas_logs_unit").selected_text(current).width(260.0).height(400.0).show_ui(ui, |ui| {
            for n in &names {
                ui.selectable_value(&mut state.unit, Some(n.clone()), n);
            }
        });
        ui.add_space(8.0);
        ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(t!("Filtrer les lignes…", "Filter lines…")).desired_width(220.0));
        ui.checkbox(&mut state.follow, t!("Suivre en direct", "Follow live"));
        if ui
            .button(t!("Effacer", "Clear"))
            .on_hover_text(t!("Vide l'affichage (le journal du serveur n'est pas modifié)", "Clears the view (the server log is untouched)"))
            .clicked()
        {
            server.clear_logs();
        }
    });
    ui.add_space(10.0);

    let Some(unit) = state.unit.clone() else {
        ui.label(egui::RichText::new(t!("Choisis un service pour afficher son journal.", "Choose a service to show its log.")).color(theme::TEXT_MUTED()));
        return;
    };

    let data = server.data();
    if let Some(err) = &data.logs.error {
        ui.label(egui::RichText::new(tf!("{unit} : {err}", "{unit}: {err}")).color(theme::TODAY()));
    }
    let q = state.query.to_lowercase();
    let lines: Vec<&LogEntry> = data.logs.entries.iter().filter(|e| q.is_empty() || e.message.to_lowercase().contains(&q)).collect();

    egui::Frame::new().fill(theme::SURFACE()).stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(12)).show(ui, |ui| {
        let font = egui::TextStyle::Monospace;
        let row_h = ui.text_style_height(&font);
        egui::ScrollArea::both().auto_shrink(false).stick_to_bottom(state.follow).show_rows(ui, row_h, lines.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for e in &lines[range] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(time(e.time_ms)).monospace().color(theme::TEXT_MUTED()));
                    ui.add_space(6.0);
                    let label = egui::Label::new(egui::RichText::new(&e.message).monospace().color(color(e.priority)));
                    ui.add(label.wrap_mode(egui::TextWrapMode::Extend));
                });
            }
        });
        if lines.is_empty() {
            ui.label(egui::RichText::new(t!("Aucune ligne pour l'instant.", "No lines yet.")).color(theme::TEXT_MUTED()));
        }
    });
}

/// Couleur selon la gravité : erreurs en accent, avertissements à mi-chemin, débogage discret.
fn color(priority: u8) -> egui::Color32 {
    match priority {
        0..=3 => theme::TODAY(),
        4 => theme::blend(theme::TEXT(), theme::TODAY(), 0.5),
        7 => theme::TEXT_MUTED(),
        _ => theme::TEXT(),
    }
}

/// « 06/11 17:42:03 », à l'heure locale.
fn time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M:%S").to_string()).unwrap_or_default()
}
