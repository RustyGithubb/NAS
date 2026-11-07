//! Fichiers : emplacements du serveur (médias des sites, partages, sauvegardes). À construire.

use eframe::egui;

use super::header;
use crate::liquid::Liquid;
use crate::widgets;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Place {
    #[default]
    Media,
    Shares,
    Backups,
}

#[derive(Default)]
pub struct State {
    place: Place,
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    header(ui, t!("Stockage", "Storage"), t!("Fichiers", "Files"), t!("Bientôt", "Coming soon"), false);
    let places = [
        (Place::Media, t!("Médias des sites", "Site media"), "/srv/nas/medias"),
        (Place::Shares, t!("Partages", "Shares"), "/srv/nas/partage"),
        (Place::Backups, t!("Sauvegardes", "Backups"), "/srv/nas/sauvegardes"),
    ];
    widgets::pane_columns(ui, &[widgets::SIDE_W], |i, ui| {
        if i == 0 {
            widgets::side_label(ui, t!("Emplacements", "Locations"));
            let mut hl = Liquid::begin(ui, "files_places");
            for (p, label, _) in places {
                if widgets::nav_row(ui, label, None, st.place == p, widgets::Tone::Normal, &mut hl, None).clicked() {
                    st.place = p;
                }
            }
            widgets::nav_end(ui, hl);
        } else {
            let (_, label, path) = places.iter().find(|x| x.0 == st.place).copied().unwrap_or(places[0]);
            widgets::pane_title(ui, label, |ui| {
                ui.label(egui::RichText::new(path).monospace().color(crate::theme::TEXT_MUTED()));
            });
            ui.add_space(40.0);
            widgets::empty_state(
                ui,
                t!("Explorateur de fichiers à venir", "File browser coming soon"),
                t!("Parcourir, envoyer et récupérer les fichiers du serveur", "Browse, upload and download server files"),
            );
        }
    });
}
