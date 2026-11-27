//! Panneau Compte (clic sur la pastille Compte, comme WorkTable) : paramètres du serveur et de
//! l'accès à l'agent. Même boîte que la recherche Ctrl+Espace de WorkTable, sur fond flouté ;
//! fermé par Échap ou un clic à côté.

use eframe::egui::{self, Key, Modifiers, Pos2, RichText, Sense, Stroke, vec2};

use crate::overlay::{Backdrop, overlay_frame};
use crate::prefs::Prefs;
use crate::server::{Link, Server};
use crate::tabs::row;
use crate::{theme, widgets};
use nas_proto::UpdateState;

/// Largeur des intitulés des champs protégés.
const LABEL_W: f32 = 130.0;
/// Rayon de l'image du serveur dans l'en-tête (même taille que le compte dans WorkTable).
const AVATAR_R: f32 = 48.0;

#[derive(Default)]
pub struct AccountPanel {
    open: bool,
    bg: Backdrop,
    /// Remettre le défilement en haut (ouverture).
    scroll_top: bool,
    /// Saisies des champs protégés, appliquées seulement à la validation.
    addr: String,
    key: String,
    /// Dernier nom déclaré par l'agent (gardé quand il ne répond plus).
    host: String,
}

/// Image ronde du serveur : initiales du nom déclaré par l'agent (« mac-mini » → « MM »).
pub fn paint_identity(p: &egui::Painter, c: Pos2, r: f32, name: &str) {
    p.circle_filled(c, r, theme::SURFACE_HI());
    let initials: String = name.split(['.', '_', '-', ' ']).filter(|w| !w.is_empty()).take(2).filter_map(|w| w.chars().next()).flat_map(char::to_uppercase).collect();
    let initials = if initials.is_empty() { "?".to_string() } else { initials };
    p.text(c, egui::Align2::CENTER_CENTER, initials, theme::editor_font((r * 0.7).max(9.0), true), theme::TEXT());
}

/// Pastille d'état de la liaison : pleine si connecté, creuse pendant la connexion, rouge sinon.
pub fn paint_dot(p: &egui::Painter, c: Pos2, link: &Link) {
    match link {
        Link::Online(_) => p.circle_filled(c, 4.5, theme::TEXT()),
        Link::Connecting => p.circle_stroke(c, 4.0, Stroke::new(1.5, theme::TEXT_MUTED())),
        _ => p.circle_filled(c, 4.5, theme::DANGER()),
    };
}

/// État de la liaison, en clair.
fn link_text(link: &Link) -> String {
    match link {
        Link::Connecting => t!("Connexion…", "Connecting…").to_string(),
        Link::Online(s) => tf!("Connecté à {}", "Connected to {}", s.hostname),
        Link::Offline(e) => tf!("Hors ligne : {e}", "Offline: {e}"),
        Link::Denied => t!("Clé d'accès refusée", "Access key refused").to_string(),
        Link::Mismatch(a, c) => tf!("Version incompatible (agent : {a}, console : {c})", "Incompatible version (agent: {a}, console: {c})"),
    }
}

fn rule(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, theme::BORDER()));
    ui.add_space(8.0);
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).strong().color(theme::TEXT()));
    ui.add_space(4.0);
}

/// Mises à jour de l'agent : vérifiées chaque nuit sur GitHub par le serveur, ou à la demande.
fn update_row(ui: &mut egui::Ui, srv: &Server) {
    srv.poll_update();
    let update = srv.data().update.clone();
    let running = matches!(&update, Some(Ok(u)) if u.state == UpdateState::Running);
    row(ui, t!("Mises à jour de l'agent", "Agent updates"), t!("Vérifiées chaque nuit sur GitHub", "Checked every night on GitHub"), |ui| {
        if ui
            .add_enabled(!running, egui::Button::new(t!("Rechercher", "Check now")))
            .on_hover_text(t!("Chercher une nouvelle version sur GitHub et l'installer", "Look for a new version on GitHub and install it"))
            .clicked()
        {
            srv.check_update();
        }
        match &update {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => {
                ui.add(egui::Label::new(RichText::new(e).color(theme::DANGER())).truncate());
            }
            Some(Ok(u)) => {
                let mut text = u.message.clone();
                // Version publiée (« 0.1.0 » → « v0.1.0 ») ; commit d'un ancien agent compilé sur le serveur.
                if !u.commit.is_empty() && u.state != UpdateState::Running {
                    let v = if u.commit.contains('.') { format!("v{}", u.commit) } else { u.commit.clone() };
                    text.push_str(&format!("  ·  {v}"));
                }
                if u.checked_ms > 0
                    && u.state != UpdateState::Running
                    && let Some(d) = chrono::DateTime::from_timestamp_millis(u.checked_ms)
                {
                    text.push_str(&format!("  ·  {}", d.with_timezone(&chrono::Local).format("%d/%m %H:%M")));
                }
                let color = match u.state {
                    UpdateState::Failed => theme::DANGER(),
                    UpdateState::Updated => theme::TEXT(),
                    _ => theme::TEXT_MUTED(),
                };
                ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
                if running {
                    ui.spinner();
                }
            }
        }
    });
}

impl AccountPanel {
    /// Ouvert et pas en train de disparaître.
    pub fn is_open(&self) -> bool {
        self.open && !self.bg.is_closing()
    }

    /// Nom déclaré par l'agent (le dernier connu s'il ne répond plus).
    pub fn name(&mut self, link: &Link) -> &str {
        if let Link::Online(s) = link
            && self.host != s.hostname
        {
            self.host = s.hostname.clone();
        }
        &self.host
    }

    pub fn toggle(&mut self, ctx: &egui::Context, prefs: &Prefs, key: &str) {
        if self.is_open() {
            self.bg.close(ctx);
        } else {
            self.open = true;
            self.scroll_top = true;
            self.addr = prefs.agent_addr.clone();
            self.key = key.to_string();
            // Édition abandonnée à la fermeture : les champs rouvrent en lecture seule.
            widgets::reset_locked_field(ctx, "agent_addr");
            widgets::reset_locked_field(ctx, "agent_key");
            self.bg.start(ctx);
        }
    }

    /// `key` : clé d'accès de l'agent (enregistrée à part, dans le coffre du système).
    pub fn show(&mut self, ctx: &egui::Context, prefs: &mut Prefs, key: &mut String, key_error: Option<&str>, link: &Link, srv: &Server) {
        if !self.open {
            return;
        }
        let (k, mut close) = self.bg.show(ctx, "account_backdrop");
        let closing = self.bg.is_closing();
        if !closing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            close = true;
        }
        let name = self.name(link).to_string();

        let screen = ctx.content_rect();
        // Même largeur et même haut que la recherche Ctrl+Espace de WorkTable.
        let w = 640.0_f32.min(screen.width() - 40.0);
        let top = screen.top() + screen.height() * 0.16 - (1.0 - k) * 12.0;
        // Pendant la disparition, la boîte ne réagit plus aux clics.
        egui::Area::new(egui::Id::unique("account_box")).order(egui::Order::Tooltip).fixed_pos(Pos2::new(screen.center().x - w / 2.0, top)).interactable(!closing).show(
            ctx,
            |ui| {
                ui.multiply_opacity(k);
                overlay_frame().show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    // En-tête centré, comme WorkTable : image du serveur en grand (pastille d'état
                    // de la liaison en bas à droite), nom déclaré par l'agent, état en dessous.
                    ui.vertical_centered(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(2.0 * AVATAR_R, 2.0 * AVATAR_R), Sense::hover());
                        paint_identity(ui.painter(), r.center(), AVATAR_R, &name);
                        let dot = r.center() + vec2(AVATAR_R, AVATAR_R) * std::f32::consts::FRAC_1_SQRT_2;
                        ui.painter().circle_filled(dot, 9.0, theme::SURFACE());
                        paint_dot(ui.painter(), dot, link);
                        ui.add_space(10.0);
                        let title = if name.is_empty() { t!("Serveur inconnu", "Unknown server").to_string() } else { name.clone() };
                        ui.label(RichText::new(title).font(theme::title_font(26.0)).color(theme::TEXT()));
                        ui.label(RichText::new(link_text(link)).color(theme::TEXT_MUTED()));
                    });
                    rule(ui);
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
                        section(ui, t!("Serveur", "Server"));
                        ui.label(
                            RichText::new(t!("IP:port de la machine qui fait tourner l'agent", "IP:port of the machine running the agent")).small().color(theme::TEXT_FAINT()),
                        );
                        ui.add_space(6.0);
                        if widgets::locked_field(ui, "agent_addr", t!("Adresse de l'agent", "Agent address"), LABEL_W, &mut self.addr, "127.0.0.1:7878", false) {
                            self.addr = self.addr.trim().to_string();
                            prefs.agent_addr = self.addr.clone();
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add_sized([LABEL_W, 18.0], egui::Label::new(RichText::new(t!("État", "Status")).color(theme::TEXT_MUTED())));
                            let (r, _) = ui.allocate_exact_size(vec2(12.0, 18.0), Sense::hover());
                            paint_dot(ui.painter(), r.center(), link);
                            ui.add(egui::Label::new(RichText::new(link_text(link)).color(theme::TEXT_MUTED())).truncate());
                        });

                        rule(ui);
                        section(ui, t!("Clé d'accès", "Access key"));
                        ui.label(RichText::new(t!("Affichée par l'agent au premier lancement", "Shown by the agent on first launch")).small().color(theme::TEXT_FAINT()));
                        ui.add_space(6.0);
                        if widgets::locked_field(ui, "agent_key", t!("Clé", "Key"), LABEL_W, &mut self.key, "", true) {
                            self.key = self.key.trim().to_string();
                            *key = self.key.clone();
                        }
                        if let Some(e) = key_error {
                            ui.label(
                                RichText::new(tf!("Clé non enregistrée dans le coffre du système : {e}", "Key not saved in the system vault: {e}")).small().color(theme::DANGER()),
                            );
                        }

                        // Agent d'une autre version : on peut quand même le mettre à jour d'ici.
                        if matches!(link, Link::Online(_) | Link::Mismatch(..)) {
                            rule(ui);
                            section(ui, t!("Agent", "Agent"));
                            update_row(ui, srv);
                        }
                    });
                });
            },
        );
        // Le fond cliqué passe au premier plan de son niveau : la boîte reste au-dessus.
        ctx.move_to_top(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("account_box")));
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
