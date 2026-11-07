//! Réglages, organisés comme ceux de WorkTable : Général, Serveur, Apparence, Polices,
//! Animations, À propos.

use eframe::egui::{self, RichText};

use super::{header, row, separator};
use crate::liquid::Liquid;
use crate::prefs::{Animation, Curve, Prefs};
use crate::server::Link;
use crate::{theme, widgets};

/// `key` : clé d'accès de l'agent (enregistrée à part, dans le coffre du système).
pub fn show(ui: &mut egui::Ui, prefs: &mut Prefs, key: &mut String, key_error: Option<&str>, link: &Link) {
    let badge = match link {
        Link::Online(s) => tf!("Connecté à {}", "Connected to {}", s.hostname),
        _ => String::new(),
    };
    header(ui, t!("Application", "Application"), t!("Réglages", "Settings"), &badge, false);
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let width = ui.available_width().min(760.0);
        ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
            general(ui, prefs);
            server(ui, prefs, key, key_error, link);
            themes(ui, prefs);
            fonts(ui, prefs);
            animations(ui, prefs);
            section(ui, t!("À propos", "About"), &[("Version", t!("Console NAS", "NAS console"), Value::Text(env!("CARGO_PKG_VERSION")))]);
        });
    });
}

/// Serveur : adresse de l'agent, clé d'accès, état de la liaison.
fn server(ui: &mut egui::Ui, prefs: &mut Prefs, key: &mut String, key_error: Option<&str>, link: &Link) {
    widgets::card(ui, t!("Serveur", "Server"), 0.0, |ui| {
        row(ui, t!("Adresse de l'agent", "Agent address"), t!("IP:port de la machine qui fait tourner l'agent", "IP:port of the machine running the agent"), |ui| {
            ui.add(egui::TextEdit::singleline(&mut prefs.agent_addr).desired_width(240.0).font(egui::TextStyle::Monospace));
        });
        separator(ui);
        let show_id = egui::Id::unique("nas_show_key");
        let mut show = ui.ctx().data(|d| d.get_temp::<bool>(show_id)).unwrap_or(false);
        row(ui, t!("Clé d'accès", "Access key"), t!("Affichée par l'agent au premier lancement", "Shown by the agent on first launch"), |ui| {
            if ui.selectable_label(show, t!("Afficher", "Show")).clicked() {
                show = !show;
            }
            ui.add(egui::TextEdit::singleline(key).password(!show).desired_width(300.0).font(egui::TextStyle::Monospace));
        });
        ui.ctx().data_mut(|d| d.insert_temp(show_id, show));
        if let Some(e) = key_error {
            ui.label(RichText::new(tf!("Clé non enregistrée dans le coffre du système : {e}", "Key not saved in the system vault: {e}")).color(theme::TODAY()));
        }
        separator(ui);
        let (dot, text) = match link {
            Link::Connecting => (theme::TEXT_MUTED(), t!("Connexion…", "Connecting…").to_string()),
            Link::Online(s) => (theme::TEXT(), tf!("Connecté à {}", "Connected to {}", s.hostname)),
            Link::Offline(e) => (theme::TODAY(), tf!("Hors ligne : {e}", "Offline: {e}")),
            Link::Denied => (theme::TODAY(), t!("Clé d'accès refusée", "Access key refused").to_string()),
        };
        row(ui, t!("État", "Status"), t!("Liaison avec l'agent", "Link with the agent"), |ui| {
            ui.label(RichText::new(text).color(theme::TEXT_MUTED()));
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, dot);
        });
    });
    ui.add_space(12.0);
}

/// Langue de l'interface, onglet au démarrage et mises à jour.
fn general(ui: &mut egui::Ui, prefs: &mut Prefs) {
    use crate::i18n::Language;
    widgets::card(ui, t!("Général", "General"), 0.0, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(t!("Langue", "Language"));
                ui.label(RichText::new(t!("Langue de l'interface", "Language of the interface")).small().color(theme::TEXT_FAINT()));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("nas_language").width(180.0).selected_text(prefs.language.label()).show_ui(ui, |ui| {
                    for l in Language::ALL {
                        ui.selectable_value(&mut prefs.language, l, l.label());
                    }
                });
            });
        });
    });
    ui.add_space(12.0);
}

/// Choix du thème (12 thèmes), chacun montré par un aperçu de ses couleurs.
fn themes(ui: &mut egui::Ui, prefs: &mut Prefs) {
    use crate::theme::{ThemeId, rgb};
    widgets::card(ui, t!("Apparence", "Appearance"), 0.0, |ui| {
        ui.label(
            RichText::new(t!("Thème de couleurs, appliqué immédiatement à toute l'application.", "Colour theme, applied immediately to the whole app."))
                .small()
                .color(theme::TEXT_FAINT()),
        );
        ui.add_space(6.0);
        ui.add_space(4.0);
        let gap = 10.0;
        let cols = 3;
        let w = ((ui.available_width() - gap * (cols - 1) as f32) / cols as f32).floor();
        for line in ThemeId::ALL.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &t in line {
                    let p = t.palette();
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 74.0), egui::Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let sel = prefs.theme == t;
                    let painter = ui.painter();
                    // Aperçu : fond, carte, texte, accent.
                    painter.rect_filled(r, egui::CornerRadius::same(10), rgb(p.bg));
                    let card = egui::Rect::from_min_size(r.min + egui::vec2(10.0, 10.0), egui::vec2(r.width() - 20.0, 30.0));
                    painter.rect(card, egui::CornerRadius::same(6), rgb(p.surface), egui::Stroke::new(1.0, rgb(p.border)), egui::StrokeKind::Inside);
                    painter.rect_filled(egui::Rect::from_min_size(card.min + egui::vec2(8.0, 8.0), egui::vec2(card.width() * 0.45, 5.0)), egui::CornerRadius::same(3), rgb(p.text));
                    painter.rect_filled(
                        egui::Rect::from_min_size(card.min + egui::vec2(8.0, 18.0), egui::vec2(card.width() * 0.3, 4.0)),
                        egui::CornerRadius::same(2),
                        rgb(p.muted),
                    );
                    painter.circle_filled(egui::pos2(card.right() - 12.0, card.center().y), 5.0, rgb(p.accent));
                    painter.circle_filled(egui::pos2(card.right() - 26.0, card.center().y), 5.0, rgb(p.link));
                    painter.text(egui::pos2(r.left() + 10.0, r.bottom() - 15.0), egui::Align2::LEFT_CENTER, t.label(), egui::FontId::proportional(12.5), rgb(p.text));
                    let stroke = if sel {
                        egui::Stroke::new(2.0, rgb(p.accent))
                    } else if resp.hovered() {
                        egui::Stroke::new(1.0, rgb(p.muted))
                    } else {
                        egui::Stroke::new(1.0, rgb(p.border))
                    };
                    painter.rect_stroke(r, egui::CornerRadius::same(10), stroke, egui::StrokeKind::Inside);
                    if resp.clicked() {
                        prefs.theme = t;
                    }
                }
            });
            ui.add_space(gap);
        }
    });
    ui.add_space(12.0);
}

/// Police et taille de chaque groupe de textes, avec aperçu.
fn fonts(ui: &mut egui::Ui, prefs: &mut Prefs) {
    use crate::theme::{FontChoice, FontGroup, FontPrefs};
    widgets::card(ui, t!("Polices", "Fonts"), 0.0, |ui| {
        ui.label(
            RichText::new(t!(
                "Chaque groupe de textes a sa police et sa taille, appliquées dans tous les onglets.",
                "Each group of text has its own font and size, applied in every tab."
            ))
            .small()
            .color(theme::TEXT_FAINT()),
        );
        ui.add_space(8.0);
        let f = &mut prefs.fonts;
        let groups: [(&str, &str, &mut FontGroup, egui::FontId, &str); 4] = [
            (t!("Titres", "Titles"), t!("Titres d'onglets et de cartes", "Tab and card titles"), &mut f.titles, theme::title_font(20.0), t!("Tableau de bord", "Dashboard")),
            (
                t!("Interface", "Interface"),
                t!("Menus, listes, boutons, étiquettes", "Menus, lists, buttons, labels"),
                &mut f.interface,
                egui::FontId::proportional(14.0),
                t!("Services · 5 actifs sur 6", "Services · 5 active out of 6"),
            ),
            (
                t!("Texte", "Text"),
                t!("Descriptions, sous-titres", "Descriptions, subtitles"),
                &mut f.content,
                theme::editor_font(14.5, false),
                t!("Programmes qui tournent en permanence sur le serveur", "Programs running permanently on the server"),
            ),
            ("Code", t!("Journaux, adresses, chasse fixe", "Logs, addresses, monospace"), &mut f.code, egui::FontId::monospace(13.0), "Started OpenBSD Secure Shell server."),
        ];
        for (i, (label, hint, g, preview, sample)) in groups.into_iter().enumerate() {
            if i > 0 {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    ui.label(label);
                    ui.label(RichText::new(hint).small().color(theme::TEXT_FAINT()));
                });
                egui::ComboBox::from_id_salt(("wt_font_group", i)).width(200.0).selected_text(g.font.label()).show_ui(ui, |ui| {
                    for c in FontChoice::ALL {
                        ui.selectable_value(&mut g.font, c, c.label());
                    }
                });
                let mut pct = (g.scale * 100.0).round();
                if ui.add(egui::Slider::new(&mut pct, 80.0..=140.0).suffix(" %").step_by(5.0).fixed_decimals(0)).changed() {
                    g.scale = pct / 100.0;
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(208.0);
                ui.label(RichText::new(sample).font(preview).color(theme::TEXT_MUTED()));
            });
        }
        ui.add_space(6.0);
        if ui.button(t!("Réinitialiser", "Reset")).clicked() {
            *f = FontPrefs::default();
        }
    });
    ui.add_space(12.0);
}

/// Réglages de l'animation liquide (sélection d'onglets, de dossiers, de mails…).
fn animations(ui: &mut egui::Ui, prefs: &mut Prefs) {
    widgets::card(ui, "Animations", 0.0, |ui| {
        let a = &mut prefs.animation;
        let row = |ui: &mut egui::Ui, label: &str, hint: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    ui.label(label);
                    ui.label(RichText::new(hint).small().color(theme::TEXT_FAINT()));
                });
                add(ui);
            });
            ui.add_space(6.0);
        };

        row(ui, t!("Vitesse", "Speed"), t!("Durée d'un déplacement", "Duration of one movement"), &mut |ui| {
            ui.add(egui::Slider::new(&mut a.duration_ms, 100.0..=1500.0).suffix(" ms").step_by(10.0).fixed_decimals(0));
        });
        row(ui, t!("Opacité", "Opacity"), t!("Intensité de la forme liquide", "Strength of the liquid shape"), &mut |ui| {
            let mut pct = a.opacity * 100.0;
            if ui.add(egui::Slider::new(&mut pct, 10.0..=100.0).suffix(" %").step_by(1.0).fixed_decimals(0)).changed() {
                a.opacity = pct / 100.0;
            }
        });
        row(ui, t!("Courbe de mouvement", "Motion curve"), t!("Accélération du déplacement", "Acceleration of the movement"), &mut |ui| {
            let opts: Vec<(Curve, &str)> = Curve::ALL.iter().map(|c| (*c, c.label())).collect();
            widgets::segmented(ui, "curve", &opts, &mut a.curve);
        });

        // Aperçu : la forme passe d'une case à l'autre en continu.
        row(ui, t!("Aperçu", "Preview"), t!("Avec les réglages ci-dessus", "With the settings above"), &mut |ui| preview(ui));

        if ui.button(t!("Réinitialiser", "Reset")).clicked() {
            *a = Animation::default();
        }
    });
    ui.add_space(12.0);
}

fn preview(ui: &mut egui::Ui) {
    let anim = crate::liquid::settings(ui.ctx());
    let period = (anim.duration_ms / 1000.0) as f64 + 0.7;
    let step = (ui.input(|i| i.time) / period) as usize % 4;
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    egui::Frame::new().fill(theme::BG()).stroke(egui::Stroke::new(1.0, theme::BORDER())).corner_radius(egui::CornerRadius::same(255)).inner_margin(egui::Margin::same(4)).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let mut hl = Liquid::begin(ui, "anim_preview");
                for i in 0..4 {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(36.0, 26.0), egui::Sense::hover());
                    if i == step {
                        hl.target(r, i);
                    }
                    let c = theme::blend(theme::TEXT_FAINT(), theme::BG(), hl.coverage(r).clamp(0.0, 1.0));
                    ui.painter().circle_filled(r.center(), 3.0, c);
                }
                hl.end(ui, theme::TEXT(), 13.0);
            });
        },
    );
}

fn section(ui: &mut egui::Ui, title: &str, rows: &[(&str, &str, Value)]) {
    widgets::card(ui, title, 0.0, |ui| {
        for (i, (label, hint, value)) in rows.iter().enumerate() {
            if i > 0 {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(*label);
                    ui.label(RichText::new(*hint).small().color(theme::TEXT_FAINT()));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match value {
                    Value::Text(t) => {
                        ui.label(RichText::new(*t).color(theme::TEXT_MUTED()));
                    }
                });
            });
        }
    });
    ui.add_space(12.0);
}

enum Value {
    Text(&'static str),
}
