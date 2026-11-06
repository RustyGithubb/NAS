//! Réglages : thème et langue.

use eframe::egui::{self, CornerRadius, Rect, Sense, Stroke, vec2};

use super::{card, header};
use crate::i18n::Language;
use crate::prefs::Prefs;
use crate::server::Link;
use crate::theme::{self, ThemeId};

pub fn show(ui: &mut egui::Ui, prefs: &mut Prefs, link: &Link) {
    header(ui, t!("Réglages", "Settings"), t!("Serveur, apparence et langue de la console", "Console server, look and language"));
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        card(ui, t!("Serveur", "Server"), |ui| server(ui, prefs, link));
        ui.add_space(12.0);
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

/// Adresse de l'agent et état de la liaison.
fn server(ui: &mut egui::Ui, prefs: &mut Prefs, link: &Link) {
    ui.label(egui::RichText::new(t!("Adresse de l'agent (IP:port)", "Agent address (IP:port)")).color(theme::TEXT_MUTED()));
    ui.add(egui::TextEdit::singleline(&mut prefs.agent_addr).desired_width(260.0).font(egui::TextStyle::Monospace));
    let (dot, text) = match link {
        Link::Connecting => (theme::TEXT_MUTED(), t!("Connexion…", "Connecting…").to_string()),
        Link::Online(s) => (theme::TEXT(), tf!("Connecté à {}", "Connected to {}", s.hostname)),
        Link::Offline(e) => (theme::TODAY(), tf!("Hors ligne : {e}", "Offline: {e}")),
    };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, dot);
        ui.label(text);
    });
}
