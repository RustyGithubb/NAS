//! Boucle principale : onglet ouvert, barre latérale, barre de titre, réglages.

use eframe::egui::{self, Margin};

use crate::prefs::Prefs;
use crate::server::Server;
use crate::{sidebar, tabs, theme, titlebar, widgets};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Services,
    Sites,
    Logs,
    Files,
    Settings,
}

impl Tab {
    /// Onglets de la barre principale (Réglages a sa pilule en bas à gauche).
    pub const ALL: [Tab; 5] = [Tab::Dashboard, Tab::Services, Tab::Sites, Tab::Logs, Tab::Files];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Dashboard => t!("Tableau de bord", "Dashboard"),
            Tab::Services => "Services",
            Tab::Sites => "Sites",
            Tab::Logs => t!("Journaux", "Logs"),
            Tab::Files => t!("Fichiers", "Files"),
            Tab::Settings => t!("Réglages", "Settings"),
        }
    }

    /// Identifiant stable (indépendant de la langue) pour les animations.
    pub fn key(self) -> &'static str {
        match self {
            Tab::Dashboard => "dashboard",
            Tab::Services => "services",
            Tab::Sites => "sites",
            Tab::Logs => "logs",
            Tab::Files => "files",
            Tab::Settings => "settings",
        }
    }
}

pub struct NasApp {
    current: Tab,
    prefs: Prefs,
    /// Réglages modifiés, pas encore enregistrés : enregistrés quand l'édition est finie (pas à
    /// chaque image pendant le glisser d'un curseur), comme dans WorkTable.
    prefs_dirty: Option<std::time::Instant>,
    server: Server,
    /// Clé d'accès de l'agent (coffre de mots de passe du système, pas `prefs.json`).
    key: String,
    /// Erreur à l'enregistrement de la clé.
    key_error: Option<String>,
    services: tabs::services::State,
    sites: tabs::sites::State,
    files: tabs::files::State,
    logs: tabs::logs::State,
    /// Position et taille données à la fenêtre de l'explorateur à son ouverture (relues des
    /// réglages une fois, pour ne pas lutter contre l'utilisateur qui la déplace).
    float_rect: Option<[f32; 4]>,
}

impl NasApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let prefs = Prefs::load();
        crate::i18n::set(prefs.language);
        theme::set_theme(ctx, prefs.theme);
        theme::set_fonts(ctx, &prefs.fonts);
        let key = crate::secret::load();
        let server = Server::start(ctx, &prefs.agent_addr, &key);
        NasApp {
            current: Tab::Dashboard,
            prefs,
            prefs_dirty: None,
            server,
            key,
            key_error: None,
            services: Default::default(),
            sites: Default::default(),
            files: Default::default(),
            logs: Default::default(),
            float_rect: None,
        }
    }

    /// Fenêtre volante de l'explorateur : vraie fenêtre Windows, même barre de titre maison,
    /// épingle « toujours au premier plan ». La fermer ramène l'explorateur dans la console.
    fn files_window(&mut self, ctx: &egui::Context, link: &crate::server::Link) {
        let rect = *self.float_rect.get_or_insert_with(|| self.prefs.files_window.unwrap_or([120.0, 120.0, 1100.0, 720.0]));
        let level = if self.prefs.files_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
        let builder = egui::ViewportBuilder::default()
            .with_title(t!("Fichiers — NAS", "Files — NAS"))
            .with_position([rect[0], rect[1]])
            .with_inner_size([rect[2], rect[3]])
            .with_min_inner_size([720.0, 480.0])
            .with_decorations(false)
            .with_window_level(level);
        let addr = self.prefs.agent_addr.clone();
        let (files, server, prefs) = (&mut self.files, &self.server, &mut self.prefs);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("nas_files_window"), builder, |ui, _class| {
            let ctx = ui.ctx().clone();
            let window = ui.max_rect();
            let margin = Margin { left: 16, right: 16, top: (titlebar::HEIGHT + 6.0) as i8, bottom: 16 };
            let mut back = false;
            egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(theme::BG()).inner_margin(margin)).show(ui, |ui| {
                back = tabs::files::show(ui, files, server, link, &addr, prefs, true);
            });
            titlebar::show_float(&ctx, window, t!("Fichiers — NAS", "Files — NAS"), &mut prefs.files_on_top);
            // Position et taille retenues pour la prochaine fois.
            if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
                let now = Some([r.left(), r.top(), r.width(), r.height()]);
                if prefs.files_window != now && !ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
                    prefs.files_window = now;
                }
            }
            if back || ctx.input(|i| i.viewport().close_requested()) {
                prefs.files_floating = false;
            }
        });
    }
}

impl eframe::App for NasApp {
    /// Fermeture : dernier enregistrement des réglages.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.prefs_dirty.take().is_some() {
            self.prefs.save();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let window = ui.max_rect();
        crate::liquid::set_settings(ui.ctx(), self.prefs.animation);
        let before = self.prefs.clone();
        let key_before = self.key.clone();
        let link = self.server.link();

        // L'onglet ouvert occupe toute la fenêtre ; la marge gauche réserve la place de la barre flottante.
        let margin = Margin { left: theme::CONTENT_LEFT_INSET, right: 32, top: 36, bottom: 32 };
        let mut open_logs = None;
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(theme::BG()).inner_margin(margin)).show(ui, |ui| match self.current {
            Tab::Dashboard => tabs::dashboard::show(ui, &link, &self.prefs.agent_addr),
            Tab::Services => open_logs = tabs::services::show(ui, &mut self.services, &self.server, &link, &self.prefs.agent_addr),
            Tab::Sites => open_logs = tabs::sites::show(ui, &mut self.sites, &self.server, &link, &self.prefs.agent_addr),
            Tab::Logs => tabs::logs::show(ui, &mut self.logs, &self.server, &link, &self.prefs.agent_addr),
            Tab::Files if self.prefs.files_floating => {
                // Explorateur détaché : on le signale, avec de quoi le ramener.
                tabs::header(ui, t!("Stockage", "Storage"), t!("Fichiers", "Files"), "", false);
                ui.add_space(80.0);
                widgets::empty_state(ui, t!("Explorateur ouvert dans une fenêtre séparée", "Explorer open in a separate window"), "");
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    if ui.button(t!("Ramener ici", "Bring back here")).clicked() {
                        self.prefs.files_floating = false;
                    }
                });
            }
            Tab::Files => {
                let addr = self.prefs.agent_addr.clone();
                if tabs::files::show(ui, &mut self.files, &self.server, &link, &addr, &mut self.prefs, false) {
                    self.prefs.files_floating = true;
                    self.float_rect = None;
                }
            }
            Tab::Settings => tabs::settings::show(ui, &mut self.prefs, &mut self.key, self.key_error.as_deref(), &link, &self.server),
        });
        // « Journal » cliqué sur un service : ouvre son journal.
        if let Some(unit) = open_logs {
            self.logs.unit = Some(unit);
            self.current = Tab::Logs;
        }

        // Barre latérale et pilule du bas, au premier plan.
        if let Some(t) = sidebar::show(ui.ctx(), window, self.current).or(sidebar::bottom_buttons(ui.ctx(), window, self.current)) {
            self.current = t;
        }
        titlebar::show(ui.ctx(), window);
        if self.prefs.files_floating {
            self.files_window(ui.ctx(), &link);
        }

        // Réglages modifiés : appliqués tout de suite, puis enregistrés.
        if self.prefs != before {
            if self.prefs.theme != before.theme {
                theme::set_theme(ui.ctx(), self.prefs.theme);
            }
            if self.prefs.fonts != before.fonts {
                theme::set_fonts(ui.ctx(), &self.prefs.fonts);
            }
            crate::i18n::set(self.prefs.language);
            self.server.set_target(&self.prefs.agent_addr, &self.key);
            self.prefs_dirty = Some(std::time::Instant::now());
        }
        if let Some(t) = self.prefs_dirty {
            if t.elapsed() >= std::time::Duration::from_millis(500) {
                self.prefs_dirty = None;
                self.prefs.save();
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
        if self.key != key_before {
            self.server.set_target(&self.prefs.agent_addr, &self.key);
            self.key_error = crate::secret::save(self.key.trim()).err();
        }
    }
}
