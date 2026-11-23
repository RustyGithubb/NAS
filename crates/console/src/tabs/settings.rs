//! Réglages, organisés comme ceux de WorkTable : Général, Apparence, Polices, Animations,
//! À propos. Le serveur et la clé d'accès sont dans le panneau Compte.

use eframe::egui::{self, RichText};

use crate::liquid::Liquid;
use crate::overlay::{Backdrop, overlay_frame};
use crate::prefs::{Animation, Curve, Prefs};
use crate::{theme, widgets};

/// Panneau Réglages (clic sur la pilule ⚙️) : même boîte que le panneau Compte, sur fond
/// flouté ; fermé par Échap ou un clic à côté (la pilule comprise).
#[derive(Default)]
pub struct SettingsPanel {
    open: bool,
    bg: Backdrop,
    /// Remettre le défilement en haut (ouverture).
    scroll_top: bool,
    /// Faire passer le fond devant les pilules (ouverture).
    raise: bool,
}

impl SettingsPanel {
    /// Ouvert et pas en train de disparaître.
    pub fn is_open(&self) -> bool {
        self.open && !self.bg.is_closing()
    }

    pub fn toggle(&mut self, ctx: &egui::Context) {
        if self.is_open() {
            self.bg.close(ctx);
        } else {
            self.open = true;
            self.scroll_top = true;
            self.raise = true;
            self.bg.start(ctx);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, prefs: &mut Prefs, update: &crate::update::Updater) {
        if !self.open {
            return;
        }
        // Niveau Foreground : les listes déroulantes (Langue, Polices) s'ouvrent au-dessus du
        // panneau, pas dessous.
        let (k, mut close) = self.bg.show_in(ctx, "settings_backdrop", egui::Order::Foreground);
        let closing = self.bg.is_closing();
        // Liste ouverte : Échap et le clic à côté sont pour elle.
        if egui::Popup::is_any_open(ctx) {
            close = false;
        } else if !closing && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }

        let screen = ctx.content_rect();
        // Même écart à gauche et à droite : 16 % de la largeur de la fenêtre (comme le haut et le
        // bas, 16 % de la hauteur). Même haut que la recherche Ctrl+Espace de WorkTable.
        let w = screen.width() * (1.0 - 2.0 * 0.16);
        let top = screen.top() + screen.height() * 0.16 - (1.0 - k) * 12.0;
        // Pendant la disparition, la boîte ne réagit plus aux clics.
        egui::Area::new(egui::Id::unique("settings_box")).order(egui::Order::Foreground).fixed_pos(egui::pos2(screen.center().x - w / 2.0, top)).interactable(!closing).show(
            ctx,
            |ui| {
                ui.multiply_opacity(k);
                overlay_frame().show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    // En-tête fixe.
                    ui.label(RichText::new(t!("Application", "Application")).small().color(theme::TEXT_FAINT()));
                    ui.label(RichText::new(t!("Réglages", "Settings")).font(theme::title_font(18.0)).color(theme::TEXT()));
                    // Étiré vers le bas : même écart sous le panneau qu'au-dessus, quelle que soit la
                    // hauteur de la fenêtre (14 px : marge du cadre).
                    let bottom = screen.bottom() - screen.height() * 0.16;
                    let h = (bottom - 14.0 - ui.cursor().top()).max(120.0);
                    let mut scroll = egui::ScrollArea::vertical().min_scrolled_height(h).max_height(h).auto_shrink([false, false]);
                    // Réouverture : de nouveau en haut.
                    if std::mem::take(&mut self.scroll_top) {
                        scroll = scroll.vertical_scroll_offset(0.0);
                    }
                    scroll.show(ui, |ui| {
                        general(ui, prefs, update);
                        themes(ui, prefs);
                        fonts(ui, prefs);
                        animations(ui, prefs);
                        section(ui, t!("À propos", "About"), &[("Version", t!("Console NAS", "NAS console"), Value::Text(env!("CARGO_PKG_VERSION")))]);
                    });
                });
            },
        );
        let backdrop = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("settings_backdrop"));
        // À l'ouverture, le fond passe devant les pilules (même niveau) ; ensuite les listes
        // déroulantes ouvertes plus tard restent devant lui.
        if std::mem::take(&mut self.raise) {
            ctx.move_to_top(backdrop);
        }
        // La boîte reste juste au-dessus de son fond, même après un clic sur le fond.
        ctx.set_sublayer(backdrop, egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("settings_box")));
        if close {
            self.bg.close(ctx);
        }
        // Fin de la disparition : le panneau est retiré.
        if self.bg.is_closed() {
            self.open = false;
            self.bg.clear();
        }
    }
}

/// Section des Réglages : filet `BORDER` (8 px avant et après), titre en gras, contenu.
fn group<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(8.0);
    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), egui::Stroke::new(1.0, theme::BORDER()));
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong().color(theme::TEXT()));
    ui.add_space(4.0);
    body(ui)
}

/// Langue de l'interface et mises à jour de la console.
fn general(ui: &mut egui::Ui, prefs: &mut Prefs, update: &crate::update::Updater) {
    use crate::i18n::Language;
    group(ui, t!("Général", "General"), |ui| {
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
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        updates(ui, update);
    });
}

/// Mises à jour de la console : vérifier sur GitHub, installer, redémarrer (comme WorkTable).
fn updates(ui: &mut egui::Ui, update: &crate::update::Updater) {
    use crate::update::{self, State};
    let current = env!("CARGO_PKG_VERSION");
    let state = update.state();
    let checked = update.checked_at().map(|t| tf!(" (vérifié à {})", " (checked at {})", t.format("%H:%M"))).unwrap_or_default();
    let (status, accent) = match &state {
        State::Idle => (tf!("Version {current}", "Version {current}"), false),
        State::Checking => (t!("Recherche d'une nouvelle version…", "Looking for a new version…").to_string(), false),
        State::UpToDate => (tf!("Version {current} — à jour{checked}", "Version {current} — up to date{checked}"), false),
        State::Available(r) => (tf!("Version {} disponible (actuelle : {current})", "Version {} available (current: {current})", r.version), true),
        State::Downloading(r) => (tf!("Téléchargement de la version {}…", "Downloading version {}…", r.version), false),
        State::Ready(v) => (tf!("Version {v} installée — redémarrez pour l'utiliser.", "Version {v} installed — restart to use it."), true),
        State::Failed(e) => (e.clone(), false),
    };
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(t!("Mises à jour de la console", "Console updates"));
            let color = if accent { theme::TODAY() } else { theme::TEXT_FAINT() };
            ui.horizontal(|ui| {
                if matches!(state, State::Checking | State::Downloading(_)) {
                    ui.add(egui::Spinner::new().size(11.0));
                }
                ui.add(egui::Label::new(RichText::new(status).small().color(color)).wrap());
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match &state {
            State::Idle | State::UpToDate | State::Failed(_) => {
                if ui.button(t!("Vérifier", "Check")).on_hover_text(t!("Chercher une nouvelle version sur GitHub", "Look for a new version on GitHub")).clicked() {
                    update.check(ui.ctx());
                }
            }
            State::Available(r) => {
                match update::install_blocker() {
                    None => {
                        if ui.button(tf!("Installer la {}", "Install {}", r.version)).clicked() {
                            update.install(ui.ctx());
                        }
                    }
                    Some(why) => {
                        ui.add_enabled(false, egui::Button::new(t!("Installer", "Install"))).on_disabled_hover_text(why);
                    }
                }
                if !r.page.is_empty() && ui.button(t!("Notes de version", "Release notes")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&r.page));
                }
            }
            State::Ready(_) => {
                if ui.button(t!("Redémarrer", "Restart")).clicked() {
                    update::restart(ui.ctx());
                }
            }
            State::Checking | State::Downloading(_) => {}
        });
    });
}

/// Choix du thème (12 thèmes), chacun montré par un aperçu de ses couleurs.
fn themes(ui: &mut egui::Ui, prefs: &mut Prefs) {
    use crate::theme::{ThemeId, rgb};
    group(ui, t!("Apparence", "Appearance"), |ui| {
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
}

/// Police et taille de chaque groupe de textes, avec aperçu.
fn fonts(ui: &mut egui::Ui, prefs: &mut Prefs) {
    use crate::theme::{FontChoice, FontGroup, FontPrefs};
    group(ui, t!("Polices", "Fonts"), |ui| {
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
}

/// Réglages de l'animation liquide (sélection d'onglets, de dossiers, de mails…).
fn animations(ui: &mut egui::Ui, prefs: &mut Prefs) {
    group(ui, "Animations", |ui| {
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
    group(ui, title, |ui| {
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
}

enum Value {
    Text(&'static str),
}
