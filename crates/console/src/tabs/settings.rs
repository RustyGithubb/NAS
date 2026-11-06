//! Réglages : thème et langue.

use eframe::egui::{self, CornerRadius, Rect, Sense, Stroke, vec2};

use super::{card, header};
use crate::i18n::Language;
use crate::prefs::Prefs;
use crate::theme::{self, ThemeId};

pub fn show(ui: &mut egui::Ui, prefs: &mut Prefs) {
    header(ui, t!("Réglages", "Settings"), t!("Apparence et langue de la console", "Console look and language"));
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        card(ui, t!("Thème", "Theme"), |ui| themes(ui, prefs));
        ui.add_space(12.0);
        card(ui, t!("Langue", "Language"), |ui| {
            ui.horizontal_wrapped(|ui| {
                for lang in Language::ALL {
                    if ui.selectable_label(prefs.language == lang, lang.label()).clicked() {
                        prefs.language = lang;
                    }
                }
            });
        });
    });
}

/// Grille des thèmes : chaque vignette montre le fond, une surface et l'accent du thème.
fn themes(ui: &mut egui::Ui, prefs: &mut Prefs) {
    const W: f32 = 176.0;
    const H: f32 = 64.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for id in ThemeId::ALL {
            let (rect, resp) = ui.allocate_exact_size(vec2(W, H), Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let pal = id.palette();
            let p = ui.painter();
            let selected = prefs.theme == id;
            p.rect_filled(rect, CornerRadius::same(10), theme::rgb(pal.bg));
            // Fausse fenêtre miniature : une carte et une pastille d'accent.
            let inner = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(46.0, H - 20.0));
            p.rect(inner, CornerRadius::same(6), theme::rgb(pal.surface), Stroke::new(1.0, theme::rgb(pal.border)), egui::StrokeKind::Inside);
            p.circle_filled(inner.center(), 5.0, theme::rgb(pal.accent));
            p.text(
                rect.left_center() + vec2(66.0, 0.0),
                egui::Align2::LEFT_CENTER,
                id.label(),
                egui::FontId::proportional(13.0),
                theme::rgb(if selected { pal.text } else { pal.muted }),
            );
            let border = if selected {
                Stroke::new(2.0, theme::TEXT())
            } else if resp.hovered() {
                Stroke::new(1.0, theme::TEXT_MUTED())
            } else {
                Stroke::new(1.0, theme::BORDER())
            };
            p.rect_stroke(rect, CornerRadius::same(10), border, egui::StrokeKind::Inside);
            if resp.clicked() {
                prefs.theme = id;
            }
        }
    });
}
