//! Tableau de bord : état du serveur reçu de l'agent. Quatre tuiles, le processeur sur toute
//! la largeur (cœurs en grille), puis le stockage jusqu'en bas.

use eframe::egui::{self, CornerRadius, Margin, Rect, Sense, Stroke, vec2};

use super::{header, unavailable};
use crate::server::Link;
use crate::{theme, widgets};

/// Seuil au-delà duquel une jauge passe à la couleur d'accent (%).
const HIGH: f32 = 85.0;

pub fn show(ui: &mut egui::Ui, link: &Link, addr: &str) {
    let overline = t!("Tableau de bord", "Dashboard");
    if unavailable(ui, overline, t!("Serveur", "Server"), link, addr) {
        return;
    }
    let Link::Online(s) = link else { return };
    let hot = s.cpu.usage >= HIGH || pct(s.memory.used, s.memory.total) >= HIGH;
    header(ui, overline, &s.hostname, &s.os, hot);

    // Une seule grille, un seul espacement : le même écart entre les tuiles, entre les tuiles et
    // les panneaux, et entre les panneaux (celui des colonnes de tous les onglets).
    let gap = ui.spacing().item_spacing.x;
    let full = ui.available_rect_before_wrap();
    let tile_w = (full.width() - 3.0 * gap) / 4.0;
    let mem = pct(s.memory.used, s.memory.total);
    let temp = s.temperature_c.map(|t| format!("{t:.0} °C")).unwrap_or_else(|| "—".into());
    let temp_note = if s.temperature_c.is_some() { t!("la plus haute", "highest") } else { t!("non disponible", "not available") };
    let tiles: [(&str, &str, String, String, Option<f32>); 4] = [
        ("cpu", t!("Processeur", "CPU"), format!("{:.0} %", s.cpu.usage), tf!("{} cœurs", "{} cores", s.cpu.per_core.len()), Some(s.cpu.usage)),
        ("mem", t!("Mémoire", "Memory"), format!("{mem:.0} %"), format!("{} / {}", bytes(s.memory.used), bytes(s.memory.total)), Some(mem)),
        ("temp", t!("Température", "Temperature"), temp, temp_note.to_string(), None),
        ("up", t!("En marche depuis", "Up for"), uptime(s.uptime_s), tf!("agent v{}", "agent v{}", s.agent_version), None),
    ];
    for (i, (key, label, value, note, gauge)) in tiles.iter().enumerate() {
        let r = Rect::from_min_size(full.min + vec2(i as f32 * (tile_w + gap), 0.0), vec2(tile_w, TILE_H));
        ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)), |ui| tile(ui, key, label, value, note, *gauge));
    }

    // Processeur sur toute la largeur, cœurs en grille ; stockage en dessous, jusqu'en bas.
    let inner_w = full.width() - 2.0 * PANE_PAD;
    let fit = ((inner_w + CELL_GAP) / (CORE_CELL_MIN + CELL_GAP)).floor().clamp(1.0, 8.0) as usize;
    let n = s.cpu.per_core.len().max(1);
    // Lignes aussi pleines que possible : 16 cœurs sur 4 colonnes → 4 lignes complètes.
    let core_rows = n.div_ceil(fit);
    let cols = n.div_ceil(core_rows);
    let swap_rows = usize::from(s.memory.swap_total > 0);
    let cpu_h = PANE_PAD * 2.0 + TITLE_H + (core_rows + swap_rows) as f32 * ROW_H;
    let cpu_rect = Rect::from_min_size(full.min + vec2(0.0, TILE_H + gap), vec2(full.width(), cpu_h));
    let disk_rect = Rect::from_min_max(egui::pos2(full.left(), cpu_rect.bottom() + gap), full.max);

    in_rect(ui, cpu_rect, |ui| {
        widgets::pane(ui, |ui| {
            // Modèle à droite du titre, en petit.
            widgets::pane_title(ui, t!("Processeur", "CPU"), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&s.cpu.model).small().color(theme::TEXT_FAINT())).truncate());
            });
            let top = ui.cursor().min;
            let w = ui.available_width();
            let cell_w = (w - (cols as f32 - 1.0) * CELL_GAP) / cols as f32;
            for (i, u) in s.cpu.per_core.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let cell = Rect::from_min_size(top + vec2(c as f32 * (cell_w + CELL_GAP), r as f32 * ROW_H), vec2(cell_w, ROW_H));
                gauge_row(ui, cell, ("core", i), &tf!("Cœur {}", "Core {}", i + 1), 72.0, *u, &format!("{u:.0} %"), 44.0);
            }
            if swap_rows > 0 {
                let cell = Rect::from_min_size(top + vec2(0.0, core_rows as f32 * ROW_H), vec2(w, ROW_H));
                let swap = pct(s.memory.swap_used, s.memory.swap_total);
                gauge_row(ui, cell, "swap", t!("Mémoire d'échange", "Swap"), 150.0, swap, &format!("{} / {}", bytes(s.memory.swap_used), bytes(s.memory.swap_total)), 150.0);
            }
            ui.allocate_space(vec2(w, (core_rows + swap_rows) as f32 * ROW_H));
        });
    });

    in_rect(ui, disk_rect, |ui| {
        widgets::pane(ui, |ui| {
            widgets::pane_title(ui, t!("Stockage", "Storage"), |ui| {
                ui.label(egui::RichText::new(tf!("{} disques", "{} disks", s.disks.len())).color(theme::TEXT_FAINT()));
            });
            egui::ScrollArea::vertical().id_salt("dash_disks").auto_shrink([false, false]).show(ui, |ui| {
                if s.disks.is_empty() {
                    widgets::empty_state(ui, t!("Aucun disque détecté", "No disk detected"), "");
                }
                let w = ui.available_width();
                for (i, d) in s.disks.iter().enumerate() {
                    let (cell, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
                    let label = format!("{}  ({})", d.mount, d.fs);
                    let text = tf!("{} / {}  ·  {} libres", "{} / {}  ·  {} free", bytes(d.used), bytes(d.total), bytes(d.total.saturating_sub(d.used)));
                    gauge_row(ui, cell, ("disk", i), &label, 220.0, pct(d.used, d.total), &text, 240.0);
                }
            });
        });
    });
    ui.allocate_rect(full, Sense::hover());
}

/// Marge intérieure des panneaux (celle de `widgets::pane`).
const PANE_PAD: f32 = 16.0;
/// Ligne de titre d'un panneau (`widgets::pane_title`), espace compris.
const TITLE_H: f32 = 44.0;
/// Hauteur d'une ligne de jauge.
const ROW_H: f32 = 30.0;
/// Largeur minimale d'une case « cœur » dans la grille, et écart entre les cases.
const CORE_CELL_MIN: f32 = 220.0;
const CELL_GAP: f32 = 24.0;

/// Dispose `body` dans `r`, en haut à gauche.
fn in_rect(ui: &mut egui::Ui, r: Rect, body: impl FnOnce(&mut egui::Ui)) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)), body);
}

/// Hauteur des tuiles.
const TILE_H: f32 = 112.0;

/// Tuile : intitulé discret, grande valeur, note, et jauge facultative (0 à 100).
fn tile(ui: &mut egui::Ui, key: &str, label: &str, value: &str, note: &str, gauge: Option<f32>) {
    let jolt = crate::widgets::shake_offset(ui.ctx(), ui.max_rect().left() * 0.011);
    crate::widgets::shifted(ui, jolt, |ui| tile_inner(ui, key, label, value, note, gauge));
}

fn tile_inner(ui: &mut egui::Ui, key: &str, label: &str, value: &str, note: &str, gauge: Option<f32>) {
    egui::Frame::new().fill(theme::SURFACE()).stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(egui::RichText::new(label).small().color(theme::TEXT_MUTED()));
        ui.label(egui::RichText::new(value).font(theme::title_font(26.0)).color(theme::TEXT()));
        ui.label(egui::RichText::new(note).color(theme::TEXT_MUTED()));
        ui.add_space(8.0);
        // Hauteur identique pour toutes les tuiles, avec ou sans jauge.
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        if let Some(v) = gauge {
            bar(ui, key, r, v);
        }
    });
}

/// Ligne de jauge dans `cell` : intitulé à gauche (`label_w`), valeur à droite (`text_w`), et la
/// jauge prend toute la largeur restante.
#[allow(clippy::too_many_arguments)]
fn gauge_row(ui: &egui::Ui, cell: Rect, key: impl std::hash::Hash + std::fmt::Debug, label: &str, label_w: f32, value: f32, text: &str, text_w: f32) {
    let font = egui::FontId::proportional(13.0);
    let l = Rect::from_min_size(cell.min, vec2(label_w, cell.height()));
    // Intitulé trop long (point de montage) : coupé au bord de sa colonne.
    ui.painter().with_clip_rect(l.with_max_x(l.right() - 8.0)).text(l.left_center(), egui::Align2::LEFT_CENTER, label, font.clone(), theme::TEXT());
    ui.painter().text(cell.right_center(), egui::Align2::RIGHT_CENTER, text, font, theme::TEXT_MUTED());
    let g = Rect::from_min_max(egui::pos2(l.right(), cell.center().y - 3.0), egui::pos2((cell.right() - text_w - 12.0).max(l.right() + 20.0), cell.center().y + 3.0));
    bar(ui, key, g, value);
}

/// Jauge arrondie ; la valeur glisse en douceur d'une mesure à l'autre.
fn bar(ui: &egui::Ui, key: impl std::hash::Hash + std::fmt::Debug, r: Rect, value: f32) {
    let v = ui.ctx().animate_value_with_time(egui::Id::unique(("nas_bar", key)), value.clamp(0.0, 100.0), 0.6);
    let p = ui.painter();
    let round = CornerRadius::same((r.height() / 2.0) as u8);
    p.rect_filled(r, round, theme::SURFACE_HI());
    let fill = if v >= HIGH { theme::TODAY() } else { theme::TEXT() };
    let w = (r.width() * v / 100.0).max(if v > 0.0 { r.height() } else { 0.0 });
    p.rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), round, fill);
}

fn pct(used: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { used as f32 * 100.0 / total as f32 }
}

/// Taille lisible : « 3,2 Go » / « 3.2 GB ».
fn bytes(n: u64) -> String {
    const UNITS_FR: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    const UNITS_EN: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let units = t!(UNITS_FR, UNITS_EN);
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let s = if i == 0 || v >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") };
    let s = if crate::i18n::en() { s } else { s.replace('.', ",") };
    format!("{s} {}", units[i])
}

/// Durée lisible : « 2 j 4 h », « 3 h 12 min », « 5 min ».
fn uptime(s: u64) -> String {
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3600, (s % 3600) / 60);
    if d > 0 {
        tf!("{d} j {h} h", "{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        crate::i18n::set(crate::i18n::Language::French);
        assert_eq!(bytes(8 * 1024 * 1024 * 1024), "8,0 Go");
        assert_eq!(bytes(512), "512 o");
        assert_eq!(uptime(2 * 86_400 + 4 * 3600 + 60), "2 j 4 h");
        assert_eq!(uptime(3 * 3600 + 12 * 60), "3 h 12 min");
        assert_eq!(pct(1, 4), 25.0);
        assert_eq!(pct(1, 0), 0.0);
    }
}
