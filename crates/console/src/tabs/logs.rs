//! Journaux : services à gauche, journal du service choisi à droite, mis à jour en direct.

use eframe::egui::{self, RichText};
use nas_proto::LogEntry;

use super::{header, paint_state, unavailable};
use crate::liquid::Liquid;
use crate::server::{Link, Server};
use crate::widgets;

/// Service affiché, suivi en direct, filtres (gardés d'une visite à l'autre).
pub struct State {
    pub unit: Option<String>,
    follow: bool,
    query: String,
    search: String,
}

impl Default for State {
    fn default() -> Self {
        State { unit: None, follow: true, query: String::new(), search: String::new() }
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State, server: &Server, link: &Link, addr: &str) {
    let (overline, title) = (t!("Serveur", "Server"), t!("Journaux", "Logs"));
    if unavailable(ui, overline, title, link, addr) {
        return;
    }
    let services = server.data().services.as_ref().map(|s| s.list.clone()).unwrap_or_default();
    // Premier passage : journal du premier service, comme la boîte de réception de WorkTable.
    if st.unit.is_none() {
        st.unit = services.first().map(|s| s.name.clone());
    }
    header(ui, overline, title, st.unit.as_deref().unwrap_or(""), false);
    server.follow_logs(st.unit.as_deref());

    widgets::pane_columns_with(
        ui,
        &[widgets::SIDE_W],
        |i| i == 0,
        |i, ui| {
            if i == 0 {
                ui.add(egui::TextEdit::singleline(&mut st.search).hint_text(t!("Rechercher un service…", "Search services…")).desired_width(f32::INFINITY));
                ui.add_space(14.0);
                widgets::side_label(ui, "Services");
                let q = st.search.to_lowercase();
                egui::ScrollArea::vertical().id_salt("logs_units").auto_shrink([false, false]).show(ui, |ui| {
                    let mut hl = Liquid::begin(ui, "logs_units");
                    for s in services.iter().filter(|s| q.is_empty() || s.name.to_lowercase().contains(&q)) {
                        let state = s.state;
                        let dot = move |p: &egui::Painter, c: egui::Pos2| paint_state(p, c, state);
                        if widgets::nav_row(ui, &s.name, None, st.unit.as_deref() == Some(&s.name), super::service_tone(s.state), &mut hl, Some(&dot)).clicked() {
                            st.unit = Some(s.name.clone());
                        }
                    }
                    widgets::nav_end(ui, hl);
                });
            } else {
                log_pane(ui, st, server);
            }
        },
    );
}

/// Couleurs du Terminal Windows (jeu « Campbell ») : le panneau du journal en reprend la DA,
/// quel que soit le thème de l'app.
mod term {
    use eframe::egui::Color32;
    /// Fond du terminal.
    pub const BG: Color32 = Color32::from_rgb(0x0C, 0x0C, 0x0C);
    /// Barre des onglets (au-dessus du terminal).
    pub const TAB_BAR: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
    /// Contour de la fenêtre.
    pub const BORDER: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
    /// Texte normal.
    pub const FG: Color32 = Color32::from_rgb(0xCC, 0xCC, 0xCC);
    /// Gris vif (« bright black ») : heures, débogage.
    pub const DIM: Color32 = Color32::from_rgb(0x76, 0x76, 0x76);
    pub const RED: Color32 = Color32::from_rgb(0xE7, 0x48, 0x56);
    pub const YELLOW: Color32 = Color32::from_rgb(0xF9, 0xF1, 0xA5);
    pub const BLUE: Color32 = Color32::from_rgb(0x3B, 0x78, 0xFF);
    /// Champs de la barre d'onglets.
    pub const FIELD: Color32 = Color32::from_rgb(0x2D, 0x2D, 0x2D);
}

/// Hauteur de la barre d'onglets du terminal.
const TAB_BAR_H: f32 = 44.0;

/// Panneau du journal façon Terminal Windows : barre d'onglets (le service ouvert, filtre,
/// suivi, effacer) puis le terminal, qui occupe tout le panneau jusqu'aux bords.
fn log_pane(ui: &mut egui::Ui, st: &mut State, server: &Server) {
    use egui::{CornerRadius, Rect, Stroke, vec2};
    // Colonne sans panneau : le terminal occupe tout le rectangle, jusqu'aux bords.
    let outer = ui.max_rect();
    let content = outer;
    let bar = Rect::from_min_max(outer.min, egui::pos2(outer.right(), outer.top() + TAB_BAR_H));
    let body = Rect::from_min_max(egui::pos2(outer.left(), bar.bottom()), outer.max);
    let p = ui.painter();
    p.rect_filled(outer, CornerRadius::same(12), term::BG);
    p.rect_filled(bar, CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, term::TAB_BAR);

    // Onglet actif : même fond que le terminal, il s'y raccorde.
    let title = st.unit.clone().unwrap_or_else(|| t!("Aucun service", "No service").to_string());
    let font = egui::FontId::proportional(13.0);
    let g = p.layout_no_wrap(title.clone(), font, term::FG);
    let tab = Rect::from_min_size(egui::pos2(bar.left() + 10.0, bar.bottom() - 34.0), vec2(g.size().x + 64.0, 34.0));
    p.rect_filled(tab, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, term::BG);
    prompt_icon(p, egui::pos2(tab.left() + 20.0, tab.center().y), term::FG);
    p.galley(egui::pos2(tab.left() + 36.0, tab.center().y - g.size().y / 2.0), g, term::FG);

    // Commandes à droite de la barre, aux couleurs du terminal.
    let mut clear = false;
    let controls = Rect::from_min_max(egui::pos2(tab.right() + 12.0, bar.top()), egui::pos2(bar.right() - 10.0, bar.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(controls).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
        let v = &mut ui.style_mut().visuals;
        v.override_text_color = Some(term::FG);
        v.extreme_bg_color = term::FIELD;
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
            w.bg_fill = term::FIELD;
            w.weak_bg_fill = term::FIELD;
            w.bg_stroke = Stroke::NONE;
        }
        v.widgets.hovered.weak_bg_fill = term::BORDER;
        v.selection.bg_fill = term::BLUE;
        clear = ui
            .button(t!("Effacer", "Clear"))
            .on_hover_text(t!("Vide l'affichage (le journal du serveur n'est pas modifié)", "Clears the view (the server log is untouched)"))
            .clicked();
        ui.checkbox(&mut st.follow, t!("Suivre en direct", "Follow live"));
        ui.add(egui::TextEdit::singleline(&mut st.query).hint_text(egui::RichText::new(t!("Filtrer les lignes…", "Filter lines…")).color(term::DIM)).desired_width(200.0));
    });
    if clear {
        server.clear_logs();
    }

    // Terminal : toute la zone sous la barre, avec une petite marge intérieure.
    let text_rect = body.shrink2(vec2(14.0, 10.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
        ui.style_mut().visuals.override_text_color = Some(term::FG);
        terminal(ui, st, server);
    });
    ui.painter().rect_stroke(outer, CornerRadius::same(12), Stroke::new(1.0, term::BORDER), egui::StrokeKind::Inside);
    ui.allocate_rect(content, egui::Sense::hover());
}

/// Lignes du journal, puis le curseur clignotant.
fn terminal(ui: &mut egui::Ui, st: &State, server: &Server) {
    if st.unit.is_none() {
        ui.label(RichText::new(t!("Choisis un service à gauche pour afficher son journal.", "Pick a service on the left to show its log.")).monospace().color(term::DIM));
        return;
    }
    let data = server.data();
    let q = st.query.to_lowercase();
    let lines: Vec<&LogEntry> = data.logs.entries.iter().filter(|e| q.is_empty() || e.message.to_lowercase().contains(&q)).collect();
    let error = data.logs.error.clone();
    let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
    // Curseur : visible une demi-seconde sur deux.
    let now = ui.input(|i| i.time);
    let cursor_on = (now * 2.0) as i64 % 2 == 0;
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    let total = lines.len() + 1 + usize::from(error.is_some());
    egui::ScrollArea::both().id_salt("logs_lines").auto_shrink(false).stick_to_bottom(st.follow).show_rows(ui, row_h, total, |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            if let Some(e) = lines.get(i) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(time(e.time_ms)).monospace().color(term::DIM));
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new(&e.message).monospace().color(color(e.priority))).wrap_mode(egui::TextWrapMode::Extend));
                });
            } else if i == lines.len() && error.is_some() {
                ui.label(RichText::new(error.as_deref().unwrap_or_default()).monospace().color(term::RED));
            } else {
                // Dernière ligne : invite et curseur, comme une console prête.
                ui.horizontal(|ui| {
                    if lines.is_empty() && error.is_none() {
                        ui.label(RichText::new(t!("En attente de lignes… ", "Waiting for lines… ")).monospace().color(term::DIM));
                    }
                    let (r, _) = ui.allocate_exact_size(egui::vec2(row_h * 0.55, row_h), egui::Sense::hover());
                    if cursor_on {
                        ui.painter().rect_filled(r.shrink2(egui::vec2(0.0, 1.0)), 0.0, term::FG);
                    }
                });
            }
        }
    });
}

/// Icône « >_ » de l'onglet.
fn prompt_icon(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let s = egui::Stroke::new(1.6, color);
    p.add(egui::Shape::line(vec![c + egui::vec2(-6.0, -4.0), c + egui::vec2(-2.0, 0.0), c + egui::vec2(-6.0, 4.0)], s));
    p.line_segment([c + egui::vec2(0.0, 4.5), c + egui::vec2(6.0, 4.5)], s);
}

/// Couleur selon la gravité (couleurs du Terminal Windows) : erreurs en rouge, avertissements
/// en jaune, débogage en gris.
fn color(priority: u8) -> egui::Color32 {
    match priority {
        0..=3 => term::RED,
        4 => term::YELLOW,
        7 => term::DIM,
        _ => term::FG,
    }
}

/// « 06/11 17:42:03 », à l'heure locale.
fn time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M:%S").to_string()).unwrap_or_default()
}
