//! Composants visuels partagés entre les onglets (repris de WorkTable).

use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};

use crate::theme;

/// Tremblement des panneaux pendant que le logo de la barre tourne : amplitude et fréquence
/// croissent avec la vitesse de la roue ; `seed` décale chaque panneau (ils ne tremblent pas en
/// bloc).
pub fn shake_offset(ctx: &egui::Context, seed: f32) -> egui::Vec2 {
    let k = crate::sidebar::spin_intensity();
    if k < 0.04 {
        return egui::Vec2::ZERO;
    }
    // Plus la roue tourne vite, plus c'est fort (progression plus que proportionnelle, plafonnée).
    let amp = (5.0 * k.powf(1.35)).min(16.0);
    let freq = 26.0 + 14.0 * k.min(3.0);
    let t = ctx.input(|i| i.time) as f32;
    let x = (t * freq + seed * 1.7).sin() + 0.5 * (t * freq * 1.9 + seed * 3.1).sin();
    let y = (t * freq * 1.3 + seed * 2.3).cos() + 0.5 * (t * freq * 2.3 + seed * 0.7).sin();
    ctx.request_repaint();
    egui::vec2(x, y) * amp / 1.5
}

/// Affiche `body` décalé de `offset` (effet visuel seulement), sans surcoût quand il est nul.
pub fn shifted<R>(ui: &mut egui::Ui, offset: egui::Vec2, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if offset == egui::Vec2::ZERO { body(ui) } else { ui.with_visual_transform(egui::emath::TSTransform::from_translation(offset), body).inner }
}

/// Titre de section (cartes, colonnes) : police « Titres ».
pub fn section_title(text: &str) -> RichText {
    RichText::new(text).font(theme::title_font(15.0)).color(theme::TEXT())
}

/// Carte de contenu avec bordure fine.
pub fn card<R>(ui: &mut egui::Ui, title: &str, min_height: f32, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let jolt = shake_offset(ui.ctx(), ui.cursor().top() * 0.013 + title.len() as f32);
    shifted(ui, jolt, |ui| card_inner(ui, title, min_height, body))
}

fn card_inner<R>(ui: &mut egui::Ui, title: &str, min_height: f32, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::SURFACE())
        .stroke(Stroke::new(1.0, theme::BORDER()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(min_height);
            if !title.is_empty() {
                ui.label(section_title(title));
                ui.add_space(10.0);
            }
            body(ui)
        })
        .inner
}

/// Sélecteur segmenté : la pastille de sélection glisse (animation liquide) d'une option à l'autre.
pub fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, salt: &str, options: &[(T, &str)], value: &mut T) -> bool {
    let mut changed = false;
    egui::Frame::new().fill(theme::BG()).stroke(egui::Stroke::new(1.0, theme::BORDER())).corner_radius(CornerRadius::same(255)).inner_margin(egui::Margin::same(3)).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut hl = crate::liquid::Liquid::begin(ui, ("segmented", salt));
                for (i, (opt, label)) in options.iter().enumerate() {
                    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), egui::Color32::PLACEHOLDER);
                    let size = egui::vec2(galley.size().x + 24.0, 26.0);
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if *opt == *value {
                        hl.target(rect, i);
                    }
                    let cov = hl.coverage(rect);
                    let base = if resp.hovered() { theme::TEXT() } else { theme::TEXT_MUTED() };
                    let color = theme::blend(base, theme::BG(), cov.clamp(0.0, 1.0));
                    let text_pos = rect.center() - galley.size() / 2.0;
                    ui.painter().galley(text_pos, galley, color);
                    if resp.clicked() && *opt != *value {
                        *value = *opt;
                        changed = true;
                    }
                }
                hl.end(ui, theme::TEXT(), 13.0);
            });
        },
    );
    changed
}
