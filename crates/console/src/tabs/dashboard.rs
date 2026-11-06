//! Tableau de bord : état du serveur reçu de l'agent.

use eframe::egui::{self, CornerRadius, Margin, Rect, Sense, Stroke, vec2};
use nas_proto::Status;

use super::{card, header, offline};
use crate::server::Link;
use crate::theme;

/// Seuil au-delà duquel une jauge passe à la couleur d'accent (%).
const HIGH: f32 = 85.0;

pub fn show(ui: &mut egui::Ui, link: &Link, addr: &str) {
    let title = t!("Tableau de bord", "Dashboard");
    match link {
        Link::Connecting => {
            header(ui, title, &tf!("Connexion à {addr}…", "Connecting to {addr}…"));
        }
        Link::Offline(err) => {
            header(ui, title, t!("Hors ligne", "Offline"));
            offline(ui, err, addr);
        }
        Link::Online(s) => {
            header(ui, title, &format!("● {}  ·  {}", s.hostname, s.os));
            online(ui, s);
        }
    }
}

fn online(ui: &mut egui::Ui, s: &Status) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // Quatre tuiles : processeur, mémoire, température, durée de fonctionnement.
        let mem = pct(s.memory.used, s.memory.total);
        ui.columns(4, |c| {
            tile(&mut c[0], "cpu", t!("Processeur", "CPU"), &format!("{:.0} %", s.cpu.usage), &tf!("{} cœurs", "{} cores", s.cpu.per_core.len()), Some(s.cpu.usage));
            tile(&mut c[1], "mem", t!("Mémoire", "Memory"), &format!("{:.0} %", mem), &format!("{} / {}", bytes(s.memory.used), bytes(s.memory.total)), Some(mem));
            let temp = s.temperature_c.map(|t| format!("{t:.0} °C")).unwrap_or_else(|| "—".into());
            let temp_note = if s.temperature_c.is_some() { t!("la plus haute", "highest") } else { t!("non disponible", "not available") };
            tile(&mut c[2], "temp", t!("Température", "Temperature"), &temp, temp_note, None);
            tile(&mut c[3], "up", t!("En marche depuis", "Up for"), &uptime(s.uptime_s), &tf!("agent v{}", "agent v{}", s.agent_version), None);
        });
        ui.add_space(12.0);

        card(ui, t!("Processeur", "CPU"), |ui| {
            ui.label(egui::RichText::new(&s.cpu.model).color(theme::TEXT_MUTED()));
            ui.add_space(4.0);
            for (i, u) in s.cpu.per_core.iter().enumerate() {
                row(ui, ("core", i), &tf!("Cœur {}", "Core {}", i + 1), *u, &format!("{u:.0} %"));
            }
            if s.memory.swap_total > 0 {
                ui.add_space(4.0);
                let swap = pct(s.memory.swap_used, s.memory.swap_total);
                row(ui, "swap", t!("Mémoire d'échange", "Swap"), swap, &format!("{} / {}", bytes(s.memory.swap_used), bytes(s.memory.swap_total)));
            }
        });
        ui.add_space(12.0);

        card(ui, t!("Disques", "Disks"), |ui| {
            if s.disks.is_empty() {
                ui.label(egui::RichText::new(t!("Aucun disque détecté.", "No disk detected.")).color(theme::TEXT_MUTED()));
            }
            for (i, d) in s.disks.iter().enumerate() {
                let label = format!("{}  ({})", d.mount, d.fs);
                row(ui, ("disk", i), &label, pct(d.used, d.total), &format!("{} / {}", bytes(d.used), bytes(d.total)));
            }
        });
    });
}

/// Tuile : intitulé discret, grande valeur, note, et jauge facultative (0 à 100).
fn tile(ui: &mut egui::Ui, key: &str, label: &str, value: &str, note: &str, gauge: Option<f32>) {
    egui::Frame::new().fill(theme::SURFACE()).stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
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

/// Ligne : intitulé à gauche, jauge au milieu, valeur à droite.
fn row(ui: &mut egui::Ui, key: impl std::hash::Hash + std::fmt::Debug, label: &str, value: f32, text: &str) {
    ui.horizontal(|ui| {
        let (l, _) = ui.allocate_exact_size(vec2(170.0, 20.0), Sense::hover());
        // Intitulé trop long (point de montage) : coupé au bord de sa colonne.
        ui.painter().with_clip_rect(l.shrink2(vec2(0.0, -4.0)).with_max_x(l.right() - 8.0)).text(
            l.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            theme::TEXT(),
        );
        let right_w = 150.0;
        let (g, _) = ui.allocate_exact_size(vec2((ui.available_width() - right_w - 8.0).max(40.0), 20.0), Sense::hover());
        bar(ui, key, Rect::from_center_size(g.center(), vec2(g.width(), 6.0)), value);
        let (t, _) = ui.allocate_exact_size(vec2(right_w, 20.0), Sense::hover());
        ui.painter().text(t.right_center(), egui::Align2::RIGHT_CENTER, text, egui::FontId::proportional(13.0), theme::TEXT_MUTED());
    });
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
