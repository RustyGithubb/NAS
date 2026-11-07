//! Boucle principale : onglet ouvert, barre latérale, barre de titre, réglages.

use eframe::egui::{self, Margin};

use crate::prefs::Prefs;
use crate::server::Server;
use crate::{sidebar, tabs, theme, titlebar};

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
        }
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
            Tab::Files => tabs::files::show(ui, &mut self.files),
            Tab::Settings => tabs::settings::show(ui, &mut self.prefs, &mut self.key, self.key_error.as_deref(), &link),
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
