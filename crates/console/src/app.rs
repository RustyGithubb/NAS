//! Boucle principale : onglet ouvert, barre latérale, barre de titre, réglages.

use eframe::egui::{self, Margin};

use crate::prefs::Prefs;
use crate::server::Server;
use crate::{sidebar, tabs, theme, titlebar};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Services,
    Logs,
    Files,
    Settings,
}

impl Tab {
    /// Onglets de la barre principale (Réglages a sa pilule en bas à gauche).
    pub const ALL: [Tab; 4] = [Tab::Dashboard, Tab::Services, Tab::Logs, Tab::Files];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Dashboard => t!("Tableau de bord", "Dashboard"),
            Tab::Services => "Services",
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
            Tab::Logs => "logs",
            Tab::Files => "files",
            Tab::Settings => "settings",
        }
    }
}

pub struct NasApp {
    current: Tab,
    prefs: Prefs,
    server: Server,
}

impl NasApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let prefs = Prefs::load();
        crate::i18n::set(prefs.language);
        theme::set_theme(ctx, prefs.theme);
        let server = Server::start(ctx, &prefs.agent_addr);
        NasApp { current: Tab::Dashboard, prefs, server }
    }
}

impl eframe::App for NasApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let window = ui.max_rect();
        let before = self.prefs.clone();
        let link = self.server.link();

        // L'onglet ouvert occupe toute la fenêtre ; la marge gauche réserve la place de la barre flottante.
        let margin = Margin { left: theme::CONTENT_LEFT_INSET, right: 32, top: 36, bottom: 32 };
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(theme::BG()).inner_margin(margin)).show(ui, |ui| match self.current {
            Tab::Dashboard => tabs::dashboard::show(ui, &link, &self.prefs.agent_addr),
            Tab::Services => tabs::services(ui),
            Tab::Logs => tabs::logs(ui),
            Tab::Files => tabs::files(ui),
            Tab::Settings => tabs::settings::show(ui, &mut self.prefs, &link),
        });

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
            crate::i18n::set(self.prefs.language);
            self.server.set_addr(&self.prefs.agent_addr);
            self.prefs.save();
        }
    }
}
