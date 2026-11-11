//! Fichiers : explorateur des emplacements du serveur, à la manière de l'Explorateur Windows
//! (onglets, barre d'adresse, barre de commandes, volet de navigation, Accueil, Galerie, volet
//! Détails, liste ou galerie, tri, sélection multiple, copier/couper/coller, renommer,
//! corbeille, favoris épinglés, envoi par glisser-déposer, téléchargement) avec la recherche
//! instantanée d'Everything (index tenu par l'agent).
//!
//! Raccourcis : Entrée ouvrir · Retour arrière dossier parent · Alt+←/→ précédent/suivant ·
//! F2 renommer · Suppr corbeille · Ctrl+A tout · Ctrl+C/X/V · F5 actualiser · Ctrl+F rechercher ·
//! Ctrl+T nouvel onglet · Ctrl+W fermer l'onglet · Ctrl+Tab onglet suivant.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, CornerRadius, FontId, Key, Rect, RichText, Sense, Stroke, vec2};
use nas_proto::{Entry, FileOp};

use super::{header, unavailable};
use crate::icons::{self, Emoji};
use crate::liquid::Liquid;
use crate::prefs::{Favorite, Place, Prefs};
use crate::server::{Link, Server};
use crate::tabs::dashboard::bytes;
use crate::{theme, widgets};

/// Délai avant de lancer la recherche pendant la frappe.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
const ROW_H: f32 = 30.0;
const CELL: egui::Vec2 = egui::Vec2::new(132.0, 132.0);
const STATUS_H: f32 = 30.0;
/// Largeur du volet Détails.
const DETAILS_W: f32 = 280.0;
/// Hauteur de la barre d'onglets, de la barre d'adresse et de la barre de commandes.
const CHROME_H: f32 = 132.0;
/// Page Galerie : toutes les images et vidéos (recherche dans l'index de l'agent).
const GALLERY_Q: &str = "ext:jpg;jpeg;png;gif;webp;bmp;heic;tif;tiff;psd;mp4;mov;mkv;webm;avi";

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Sort {
    #[default]
    Name,
    Modified,
    Kind,
    Size,
}

/// Éléments copiés ou coupés, en attente de « Coller » (communs à tous les onglets).
#[derive(Clone)]
struct Clip {
    cut: bool,
    root: String,
    paths: Vec<String>,
}

/// Un onglet d'explorateur.
struct Tab {
    place: Place,
    back: Vec<Place>,
    fwd: Vec<Place>,
    sort: Sort,
    desc: bool,
    /// Éléments sélectionnés : clé = emplacement + chemin complet.
    selected: BTreeSet<(String, String)>,
    anchor: Option<usize>,
    query: String,
    sent_query: String,
    query_edited: Option<Instant>,
    renaming: Option<(String, String)>,
    rename_focus: bool,
    /// Nouveau dossier créé : on passe en renommage dès qu'il apparaît.
    pending_rename: Option<String>,
    /// Renommage validé, à envoyer.
    commit: bool,
    /// Contenu de `place` déjà demandé à l'agent.
    requested: bool,
}

impl Tab {
    fn new(place: Place) -> Tab {
        // Galerie : les plus récents d'abord.
        let (sort, desc) = if place == Place::Gallery { (Sort::Modified, true) } else { (Sort::Name, false) };
        Tab {
            place,
            back: Vec::new(),
            fwd: Vec::new(),
            sort,
            desc,
            selected: BTreeSet::new(),
            anchor: None,
            query: String::new(),
            sent_query: String::new(),
            query_edited: None,
            renaming: None,
            rename_focus: false,
            pending_rename: None,
            commit: false,
            requested: false,
        }
    }

    fn folder(&self) -> Option<(&str, &str)> {
        match &self.place {
            Place::Folder { root, path } => Some((root, path)),
            _ => None,
        }
    }

    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    fn title(&self) -> String {
        if self.searching() {
            return tf!("Recherche : {}", "Search: {}", self.query.trim());
        }
        place_title(&self.place)
    }
}

fn place_title(p: &Place) -> String {
    match p {
        Place::Home => t!("Accueil", "Home").into(),
        Place::Gallery => t!("Galerie", "Gallery").into(),
        Place::Folder { root, path } => path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(root).to_string(),
    }
}

fn place_emoji(p: &Place) -> Emoji {
    match p {
        Place::Home => Emoji::Home,
        Place::Gallery => Emoji::Image,
        Place::Folder { root, path } if path.is_empty() => icons::for_root(root),
        Place::Folder { .. } => Emoji::Folder,
    }
}

/// État de l'onglet Fichiers (gardé d'une visite à l'autre).
pub struct State {
    tabs: Vec<Tab>,
    active: usize,
    clip: Option<Clip>,
    /// Texte à placer dans le presse-papiers du système (sinon Ctrl+V n'arrive pas jusqu'à egui).
    copy_text: Option<String>,
    roots_asked: bool,
    /// Onglets repris des réglages au premier affichage.
    restored: bool,
}

impl Default for State {
    fn default() -> Self {
        State { tabs: vec![Tab::new(Place::Home)], active: 0, clip: None, copy_text: None, roots_asked: false, restored: false }
    }
}

impl State {
    fn tab(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    fn activate(&mut self, i: usize) {
        self.active = i.min(self.tabs.len() - 1);
        // Les données affichées sont celles d'un seul onglet : on les redemande.
        let t = self.tab();
        t.requested = false;
        t.sent_query.clear();
    }
}

/// Élément affiché (dossier courant, résultat de recherche, récent, image de la galerie).
#[derive(Clone)]
struct Item {
    root: String,
    /// Chemin complet de l'élément.
    path: String,
    /// Dossier qui le contient.
    parent: String,
    entry: Entry,
}

impl Item {
    fn key(&self) -> (String, String) {
        (self.root.clone(), self.path.clone())
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") }
}

fn parent_of(path: &str) -> String {
    path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()
}

/// Ce que l'utilisateur a demandé pendant cette image (exécuté à la fin, une fois les données relâchées).
enum Do {
    Open(Item),
    Go(Place),
    Download(Item),
    Rename(Item),
    Copy(bool),
    Paste,
    Delete,
    Favorite(String, String),
    Unfavorite(usize),
    Reveal(Item),
    NewTab(Place),
    CloseTab(usize),
    SelectTab(usize),
    Back,
    Forward,
    Refresh,
    NewFolder,
}

pub fn show(ui: &mut egui::Ui, st: &mut State, server: &Server, link: &Link, addr: &str, prefs: &mut Prefs) {
    let (overline, title) = (t!("Stockage", "Storage"), t!("Fichiers", "Files"));
    if unavailable(ui, overline, title, link, addr) {
        return;
    }
    if !st.roots_asked {
        st.roots_asked = true;
        server.files_roots();
    }
    if !st.restored {
        st.restored = true;
        if !prefs.files_tabs.is_empty() {
            st.tabs = prefs.files_tabs.iter().cloned().map(Tab::new).collect();
        }
        st.activate(prefs.files_active);
    }
    let (roots, listing, loading, search, recent, last, transfers) = {
        let d = server.data();
        let f = &d.files;
        (f.roots.clone().unwrap_or_default(), f.listing.clone(), f.loading, f.search.clone(), f.recent.clone(), f.last.clone(), f.transfers.clone())
    };

    // Contenu de l'onglet au premier plan, demandé une fois.
    let tab = st.tab();
    if !tab.requested {
        tab.requested = true;
        match &tab.place {
            Place::Home => server.files_recent(),
            Place::Gallery => server.files_search(GALLERY_Q),
            Place::Folder { root, path } => server.files_list(root, path),
        }
    }
    // Recherche instantanée : lancée peu après la dernière frappe.
    let searching = tab.searching();
    if searching && tab.query != tab.sent_query && tab.query_edited.is_none_or(|t| t.elapsed() >= SEARCH_DEBOUNCE) {
        server.files_search(tab.query.trim());
        tab.sent_query = tab.query.clone();
    } else if searching && tab.query != tab.sent_query {
        ui.ctx().request_repaint_after(SEARCH_DEBOUNCE);
    }

    let mut items = items_of(tab, &listing, &search, &recent);
    if !(tab.place == Place::Home && !searching) {
        sort_items(&mut items, tab.sort, tab.desc);
    }
    if let Some(name) = tab.pending_rename.clone()
        && let Some(it) = items.iter().find(|i| i.entry.name == name)
    {
        tab.selected = [it.key()].into();
        tab.renaming = Some((name.clone(), name));
        tab.rename_focus = true;
        tab.pending_rename = None;
    }

    let badge = match tab.folder().and_then(|(r, _)| roots.iter().find(|x| x.name == r)) {
        Some(r) if r.total > 0 => tf!("{} libres sur {}", "{} free of {}", bytes(r.free), bytes(r.total)),
        _ => String::new(),
    };
    header(ui, overline, title, &badge, false);

    let mut todo: Vec<Do> = Vec::new();
    keyboard(ui, st, &items, &mut todo);

    // Fichiers glissés depuis Windows : envoyés dans le dossier affiché.
    let tab = &st.tabs[st.active];
    let drop_target = if tab.searching() { None } else { tab.folder().map(|(r, p)| (r.to_string(), p.to_string())) };
    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
    if !dropped.is_empty()
        && let Some((r, p)) = &drop_target
    {
        server.upload(r, p, dropped);
    }
    let hovering_files = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());

    // Onglets, barre d'adresse et barre de commandes, sur toute la largeur.
    let full = ui.available_rect_before_wrap();
    let chrome_rect = Rect::from_min_size(full.min, vec2(full.width(), CHROME_H));
    ui.scope_builder(egui::UiBuilder::new().max_rect(chrome_rect).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
        chrome(ui, st, server, prefs, &roots, &mut todo);
    });

    let gap = ui.spacing().item_spacing.x;
    let mut widths = vec![widgets::SIDE_W];
    if prefs.files_details {
        widths.push((ui.available_width() - widgets::SIDE_W - DETAILS_W - 2.0 * gap).max(200.0));
    }
    widgets::pane_columns(ui, &widths, |i, ui| match i {
        0 => side(ui, st, prefs, &roots, &mut todo),
        1 => {
            let content = ui.max_rect();
            main_view(ui, st, prefs, &roots, &items, &listing, &search, loading, &mut todo);
            status_bar(ui, st, server, &items, &last, &transfers);
            if hovering_files && let Some((r, p)) = &drop_target {
                let p_ = ui.painter();
                p_.rect_filled(content.expand(8.0), CornerRadius::same(10), theme::overlay(30));
                p_.rect_stroke(content.expand(8.0), CornerRadius::same(10), Stroke::new(2.0, theme::TEXT_MUTED()), egui::StrokeKind::Inside);
                p_.text(
                    content.center(),
                    Align2::CENTER_CENTER,
                    tf!("Déposer pour envoyer dans « {} »", "Drop to upload into “{}”", if p.is_empty() { r } else { p }),
                    FontId::proportional(18.0),
                    theme::TEXT(),
                );
            }
        }
        _ => details(ui, &st.tabs[st.active], &items, &roots),
    });

    for d in todo {
        run(d, st, server, prefs, &items);
    }
    commit_rename(st.tab(), server, &items);
    if let Some(text) = st.copy_text.take() {
        ui.ctx().copy_text(text);
    }
    // Onglets ouverts retrouvés au prochain lancement.
    let places: Vec<Place> = st.tabs.iter().map(|t| t.place.clone()).collect();
    if prefs.files_tabs != places {
        prefs.files_tabs = places;
    }
    prefs.files_active = st.active;
}

/// Éléments de l'onglet : résultats de recherche (limités au dossier affiché), contenu du
/// dossier, images de la galerie ou fichiers récents.
fn items_of(
    tab: &Tab,
    listing: &Option<Result<nas_proto::Listing, String>>,
    search: &Option<(String, Result<nas_proto::SearchResult, String>)>,
    recent: &Option<Result<nas_proto::SearchResult, String>>,
) -> Vec<Item> {
    let hit = |h: &nas_proto::Hit| Item { root: h.root.clone(), path: h.path.clone(), parent: parent_of(&h.path), entry: h.entry.clone() };
    let search_for = |q: &str| match search {
        Some((k, Ok(r))) if k == q => r.hits.iter().map(hit).collect(),
        _ => Vec::new(),
    };
    if tab.searching() {
        let all: Vec<Item> = search_for(tab.sent_query.trim());
        return match tab.folder() {
            // « Rechercher dans : dossier » : seulement ce qui est dessous.
            Some((r, p)) => all.into_iter().filter(|i| i.root == r && (p.is_empty() || i.path.starts_with(&format!("{p}/")))).collect(),
            None => all,
        };
    }
    match &tab.place {
        Place::Home => match recent {
            Some(Ok(r)) => r.hits.iter().map(hit).collect(),
            _ => Vec::new(),
        },
        Place::Gallery => search_for(GALLERY_Q),
        Place::Folder { root, path } => match listing {
            Some(Ok(l)) if &l.root == root && &l.path == path => {
                l.entries.iter().map(|e| Item { root: root.clone(), path: join(path, &e.name), parent: path.clone(), entry: e.clone() }).collect()
            }
            _ => Vec::new(),
        },
    }
}

// --- Onglets, barre d'adresse, barre de commandes ---------------------------------------

fn chrome(ui: &mut egui::Ui, st: &mut State, server: &Server, prefs: &mut Prefs, roots: &[nas_proto::Root], todo: &mut Vec<Do>) {
    widgets::pane(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        tab_bar(ui, st, todo);
        address_bar(ui, st, server, roots, todo);
        command_bar(ui, st, server, prefs, todo);
    });
}

/// Onglets façon Explorateur Windows : icône, titre, ×, puis « + ».
fn tab_bar(ui: &mut egui::Ui, st: &State, todo: &mut Vec<Do>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let n = st.tabs.len();
        let w = ((ui.available_width() - 40.0) / n as f32 - 4.0).clamp(90.0, 220.0);
        for (i, t) in st.tabs.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
            let active = i == st.active;
            let p = ui.painter();
            if active {
                p.rect_filled(r, CornerRadius::same(8), theme::SURFACE_HI());
            } else if resp.hovered() {
                p.rect_filled(r, CornerRadius::same(8), theme::overlay(6));
            }
            icons::paint(p, Rect::from_center_size(egui::pos2(r.left() + 16.0, r.center().y), vec2(16.0, 16.0)), place_emoji(&t.place));
            let close = Rect::from_center_size(egui::pos2(r.right() - 14.0, r.center().y), vec2(18.0, 18.0));
            let mut job = egui::text::LayoutJob::simple_singleline(t.title(), FontId::proportional(13.0), if active { theme::TEXT() } else { theme::TEXT_MUTED() });
            job.wrap = egui::text::TextWrapping::truncate_at_width(close.left() - r.left() - 34.0);
            let g = p.layout_job(job);
            p.galley(egui::pos2(r.left() + 30.0, r.center().y - g.size().y / 2.0), g, theme::TEXT());
            // × : sur l'onglet actif ou survolé (il en reste toujours un).
            let close_resp = ui.interact(close, egui::Id::unique(("files_tab_close", i)), Sense::click());
            if n > 1 && (active || resp.hovered() || close_resp.hovered()) {
                if close_resp.hovered() {
                    ui.painter().rect_filled(close, CornerRadius::same(4), theme::overlay(12));
                }
                ui.painter().text(close.center(), Align2::CENTER_CENTER, "×", FontId::proportional(15.0), theme::TEXT_MUTED());
            }
            if n > 1 && close_resp.clicked() {
                todo.push(Do::CloseTab(i));
            } else if resp.clicked() {
                todo.push(Do::SelectTab(i));
            }
            if resp.middle_clicked() && n > 1 {
                todo.push(Do::CloseTab(i));
            }
        }
        if ui
            .add(egui::Button::new(RichText::new("+").size(16.0)).frame(false).min_size(vec2(28.0, 28.0)))
            .on_hover_text(t!("Nouvel onglet (Ctrl+T)", "New tab (Ctrl+T)"))
            .clicked()
        {
            todo.push(Do::NewTab(Place::Home));
        }
    });
}

/// ← → ↑ ⟳, chemin cliquable dans un champ, recherche à droite.
fn address_bar(ui: &mut egui::Ui, st: &mut State, server: &Server, roots: &[nas_proto::Root], todo: &mut Vec<Do>) {
    let tab = st.tab();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let nav = |ui: &mut egui::Ui, enabled: bool, text: &str, tip: &str| {
            ui.add_enabled(enabled, egui::Button::new(RichText::new(text).size(15.0)).frame(false).min_size(vec2(30.0, 30.0))).on_hover_text(tip).clicked()
        };
        if nav(ui, !tab.back.is_empty(), "←", t!("Précédent (Alt+←)", "Back (Alt+←)")) {
            todo.push(Do::Back);
        }
        if nav(ui, !tab.fwd.is_empty(), "→", t!("Suivant (Alt+→)", "Forward (Alt+→)")) {
            todo.push(Do::Forward);
        }
        let up = match &tab.place {
            Place::Folder { root, path } if !path.is_empty() => Some(Place::Folder { root: root.clone(), path: parent_of(path) }),
            Place::Folder { .. } | Place::Gallery => Some(Place::Home),
            Place::Home => None,
        };
        if nav(ui, up.is_some(), "↑", t!("Dossier parent (Retour arrière)", "Parent folder (Backspace)"))
            && let Some(p) = up
        {
            todo.push(Do::Go(p));
        }
        if nav(ui, true, "⟳", t!("Actualiser (F5)", "Refresh (F5)")) {
            todo.push(Do::Refresh);
        }
        ui.add_space(4.0);

        let search_w = 300.0_f32.min(ui.available_width() * 0.35);
        let addr_w = ui.available_width() - search_w - 8.0;
        // Champ d'adresse : icône, puis Accueil › emplacement › dossiers, chaque niveau cliquable.
        ui.allocate_ui_with_layout(vec2(addr_w, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let ar = ui.max_rect();
            ui.set_min_size(ar.size());
            ui.painter().rect_filled(ar, CornerRadius::same(8), theme::BG());
            ui.painter().rect_stroke(ar, CornerRadius::same(8), Stroke::new(1.0, theme::BORDER()), egui::StrokeKind::Inside);
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            let (ir, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            icons::paint(ui.painter(), ir, place_emoji(&tab.place));
            let crumb = |ui: &mut egui::Ui, text: &str| ui.add(egui::Button::new(RichText::new(text).size(14.0)).frame(false)).clicked();
            let sep = |ui: &mut egui::Ui| {
                ui.label(RichText::new("›").color(theme::TEXT_FAINT()));
            };
            sep(ui);
            if crumb(ui, t!("Accueil", "Home")) {
                todo.push(Do::Go(Place::Home));
            }
            match &tab.place {
                Place::Home => {}
                Place::Gallery => {
                    sep(ui);
                    crumb(ui, t!("Galerie", "Gallery"));
                }
                Place::Folder { root, path } => {
                    sep(ui);
                    if crumb(ui, root) {
                        todo.push(Do::Go(Place::Folder { root: root.clone(), path: String::new() }));
                    }
                    let mut acc = String::new();
                    for part in path.split('/').filter(|p| !p.is_empty()) {
                        acc = join(&acc, part);
                        sep(ui);
                        if crumb(ui, part) {
                            todo.push(Do::Go(Place::Folder { root: root.clone(), path: acc.clone() }));
                        }
                    }
                }
            }
        });

        // Recherche : dans le dossier affiché, ou partout depuis l'Accueil et la Galerie.
        let scope = match &tab.place {
            Place::Folder { .. } => tf!("Rechercher dans : {}", "Search in: {}", place_title(&tab.place)),
            _ => t!("Rechercher partout", "Search everywhere").to_string(),
        };
        let resp = ui
            .add(egui::TextEdit::singleline(&mut tab.query).id(egui::Id::unique("files_search")).hint_text(scope).desired_width(search_w).margin(vec2(10.0, 8.0)))
            .on_hover_text(t!("Comme Everything : plusieurs mots, *.png, ext:jpg;png, dossier:, fichier:", "Like Everything: several words, *.png, ext:jpg;png, folder:, file:"));
        if resp.changed() {
            tab.query_edited = Some(Instant::now());
            tab.selected.clear();
        }
        let _ = (server, roots);
    });
}

/// Nouveau ▾ | Couper Copier Coller Renommer Supprimer | Trier ▾ Afficher ▾ … Détails.
fn command_bar(ui: &mut egui::Ui, st: &mut State, server: &Server, prefs: &mut Prefs, todo: &mut Vec<Do>) {
    let has_clip = st.clip.is_some();
    let tab = st.tab();
    let in_folder = tab.folder().is_some() && !tab.searching();
    let n_sel = tab.selected.len();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_enabled_ui(in_folder, |ui| {
            ui.menu_button(RichText::new(t!("⊕  Nouveau  ▾", "⊕  New  ▾")).size(13.5), |ui| {
                ui.set_min_width(200.0);
                if ui.button(t!("📁  Dossier", "📁  Folder")).clicked() {
                    todo.push(Do::NewFolder);
                    ui.close();
                }
                #[cfg(windows)]
                if ui.button(t!("⬆  Envoyer des fichiers…", "⬆  Upload files…")).clicked() {
                    if let Some((root, dir)) = tab.folder() {
                        let (server, root, dir) = (server.handle_pub(), root.to_string(), dir.to_string());
                        std::thread::spawn(move || {
                            if let Some(files) = rfd::FileDialog::new().pick_files() {
                                server.upload(&root, &dir, files);
                            }
                        });
                    }
                    ui.close();
                }
            });
        });
        let _ = server;
        sep_v(ui);
        let icon = |ui: &mut egui::Ui, enabled: bool, glyph: &str, tip: &str| {
            ui.add_enabled(enabled, egui::Button::new(RichText::new(glyph).size(16.0)).frame(false).min_size(vec2(32.0, 30.0))).on_hover_text(tip).clicked()
        };
        if icon(ui, n_sel > 0, "✂", t!("Couper (Ctrl+X)", "Cut (Ctrl+X)")) {
            todo.push(Do::Copy(true));
        }
        if icon(ui, n_sel > 0, "🗐", t!("Copier (Ctrl+C)", "Copy (Ctrl+C)")) {
            todo.push(Do::Copy(false));
        }
        if icon(ui, has_clip && in_folder, "📋", t!("Coller (Ctrl+V)", "Paste (Ctrl+V)")) {
            todo.push(Do::Paste);
        }
        if icon(ui, n_sel == 1, "✏", t!("Renommer (F2)", "Rename (F2)")) {
            todo.push(Do::Rename(Item { root: String::new(), path: String::new(), parent: String::new(), entry: Entry::default() }));
        }
        if icon(ui, n_sel > 0, "🗑", t!("Mettre à la corbeille (Suppr)", "Move to trash (Del)")) {
            todo.push(Do::Delete);
        }
        sep_v(ui);
        ui.menu_button(RichText::new(t!("⇅  Trier  ▾", "⇅  Sort  ▾")).size(13.5), |ui| {
            ui.set_min_width(180.0);
            for (s, label) in [
                (Sort::Name, t!("Nom", "Name")),
                (Sort::Modified, t!("Date de modification", "Date modified")),
                (Sort::Kind, t!("Type", "Type")),
                (Sort::Size, t!("Taille", "Size")),
            ] {
                if ui.radio(tab.sort == s, label).clicked() {
                    tab.sort = s;
                }
            }
            ui.separator();
            if ui.radio(!tab.desc, t!("Croissant", "Ascending")).clicked() {
                tab.desc = false;
            }
            if ui.radio(tab.desc, t!("Décroissant", "Descending")).clicked() {
                tab.desc = true;
            }
        });
        ui.menu_button(RichText::new(t!("☰  Afficher  ▾", "☰  View  ▾")).size(13.5), |ui| {
            ui.set_min_width(180.0);
            if ui.radio(!prefs.files_gallery, t!("Liste", "List")).clicked() {
                prefs.files_gallery = false;
            }
            if ui.radio(prefs.files_gallery, t!("Grandes icônes", "Large icons")).clicked() {
                prefs.files_gallery = true;
            }
            ui.separator();
            ui.checkbox(&mut prefs.files_details, t!("Volet Détails", "Details pane"));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.selectable_label(prefs.files_details, RichText::new(t!("◨  Détails", "◨  Details")).size(13.5)).clicked() {
                prefs.files_details = !prefs.files_details;
            }
        });
    });
}

/// Séparateur vertical de la barre de commandes.
fn sep_v(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 22.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, theme::BORDER()));
    ui.add_space(6.0);
}

// --- Volet de navigation ----------------------------------------------------------------

/// Accueil, Galerie | favoris épinglés | emplacements du serveur.
fn side(ui: &mut egui::Ui, st: &State, prefs: &Prefs, roots: &[nas_proto::Root], todo: &mut Vec<Do>) {
    let tab = &st.tabs[st.active];
    let here = if tab.searching() { None } else { Some(&tab.place) };
    let mut hl = Liquid::begin(ui, "files_nav");
    nav_place(ui, &mut hl, here, todo, t!("Accueil", "Home"), Place::Home, Emoji::Home, false);
    nav_place(ui, &mut hl, here, todo, t!("Galerie", "Gallery"), Place::Gallery, Emoji::Image, false);
    nav_sep(ui);
    if prefs.favorites.is_empty() {
        ui.label(RichText::new(t!("Clic droit sur un dossier › Épingler", "Right-click a folder › Pin")).small().color(theme::TEXT_FAINT()));
        ui.add_space(4.0);
    }
    for (i, f) in prefs.favorites.iter().enumerate() {
        let label = f.path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(&f.root).to_string();
        let place = Place::Folder { root: f.root.clone(), path: f.path.clone() };
        let e = if f.path.is_empty() { icons::for_root(&f.root) } else { Emoji::Folder };
        let r = nav_place(ui, &mut hl, here, todo, &label, place.clone(), e, true).on_hover_text(format!("{} › {}", f.root, f.path));
        r.context_menu(|ui| {
            if ui.button(t!("Ouvrir dans un nouvel onglet", "Open in new tab")).clicked() {
                todo.push(Do::NewTab(place.clone()));
                ui.close();
            }
            if ui.button(t!("Désépingler", "Unpin")).clicked() {
                todo.push(Do::Unfavorite(i));
                ui.close();
            }
        });
    }
    nav_sep(ui);
    for r in roots {
        let place = Place::Folder { root: r.name.clone(), path: String::new() };
        let resp = nav_place(ui, &mut hl, here, todo, &r.name, place.clone(), icons::for_root(&r.name), false);
        resp.context_menu(|ui| {
            if ui.button(t!("Ouvrir dans un nouvel onglet", "Open in new tab")).clicked() {
                todo.push(Do::NewTab(place.clone()));
                ui.close();
            }
            if ui.button(t!("Épingler", "Pin")).clicked() {
                todo.push(Do::Favorite(r.name.clone(), String::new()));
                ui.close();
            }
        });
    }
    widgets::nav_end(ui, hl);
}

/// Ligne du volet de navigation : clic = y aller, clic du milieu = nouvel onglet (comme dans
/// l'Explorateur), épingle à droite pour les favoris.
#[allow(clippy::too_many_arguments)]
fn nav_place(ui: &mut egui::Ui, hl: &mut Liquid, here: Option<&Place>, todo: &mut Vec<Do>, text: &str, place: Place, e: Emoji, pin: bool) -> egui::Response {
    let lead = move |p: &egui::Painter, c: egui::Pos2| icons::paint(p, Rect::from_center_size(c, vec2(18.0, 18.0)), e);
    let r = widgets::nav_row(ui, text, None, here == Some(&place), widgets::Tone::Normal, hl, Some(&lead));
    if pin {
        ui.painter().text(egui::pos2(r.rect.right() - 12.0, r.rect.center().y), Align2::CENTER_CENTER, "📌", FontId::proportional(11.0), theme::TEXT_FAINT());
    }
    if r.clicked() {
        todo.push(Do::Go(place.clone()));
    }
    if r.middle_clicked() {
        todo.push(Do::NewTab(place));
    }
    r
}

fn nav_sep(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, theme::BORDER()));
    ui.add_space(6.0);
}

// --- Contenu ----------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn main_view(
    ui: &mut egui::Ui,
    st: &mut State,
    prefs: &Prefs,
    roots: &[nas_proto::Root],
    items: &[Item],
    listing: &Option<Result<nas_proto::Listing, String>>,
    search: &Option<(String, Result<nas_proto::SearchResult, String>)>,
    loading: bool,
    todo: &mut Vec<Do>,
) {
    let h = (ui.available_height() - STATUS_H).max(40.0);
    let tab = st.tab();
    let searching = tab.searching();
    ui.allocate_ui(vec2(ui.available_width(), h), |ui| {
        ui.set_min_size(vec2(ui.available_width(), h));
        if !searching && tab.place == Place::Home {
            home(ui, tab, prefs, roots, items, todo);
            return;
        }
        let gallery_page = !searching && tab.place == Place::Gallery;
        let waiting = |q: &str| !matches!(search, Some((k, _)) if k == q);
        if searching {
            if waiting(tab.sent_query.trim()) || tab.query != tab.sent_query {
                ui.vertical_centered(|ui| ui.spinner());
            } else if items.is_empty() {
                ui.add_space(40.0);
                widgets::empty_state(ui, t!("Aucun résultat", "No result"), t!("Essaie un autre mot, *.png ou ext:jpg", "Try another word, *.png or ext:jpg"));
            }
        } else if gallery_page {
            if waiting(GALLERY_Q) {
                ui.vertical_centered(|ui| ui.spinner());
            } else if items.is_empty() {
                ui.add_space(40.0);
                widgets::empty_state(
                    ui,
                    t!("Aucune image ni vidéo", "No image or video"),
                    t!("Elles apparaîtront ici dès qu'il y en aura sur le serveur", "They will show up here as soon as the server has some"),
                );
            }
        } else if loading && items.is_empty() {
            ui.vertical_centered(|ui| ui.spinner());
        } else if let Some(Err(e)) = listing {
            ui.add_space(40.0);
            widgets::empty_state(ui, t!("Dossier inaccessible", "Folder unavailable"), e);
        } else if items.is_empty() {
            ui.add_space(40.0);
            widgets::empty_state(
                ui,
                t!("Dossier vide", "Empty folder"),
                t!("Glisse des fichiers ici pour les envoyer sur le serveur", "Drop files here to upload them to the server"),
            );
        }
        if !items.is_empty() {
            // Galerie : toujours en grandes icônes, avec le dossier de chaque élément.
            let wide = searching || gallery_page;
            if prefs.files_gallery || gallery_page { gallery(ui, tab, items, wide, todo) } else { list(ui, tab, items, wide, todo) }
        }
    });
}

/// Page Accueil : Accès rapide (favoris et emplacements en grandes icônes), puis fichiers récents.
fn home(ui: &mut egui::Ui, tab: &mut Tab, prefs: &Prefs, roots: &[nas_proto::Root], items: &[Item], todo: &mut Vec<Do>) {
    egui::ScrollArea::vertical().id_salt("files_home").auto_shrink([false, false]).show(ui, |ui| {
        section_title(ui, t!("Accès rapide", "Quick access"));
        let mut quick: Vec<(String, String, Place, Emoji)> = prefs
            .favorites
            .iter()
            .map(|f| {
                let name = f.path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(&f.root).to_string();
                let e = if f.path.is_empty() { icons::for_root(&f.root) } else { Emoji::Folder };
                (name, format!("📌 {}", f.root), Place::Folder { root: f.root.clone(), path: f.path.clone() }, e)
            })
            .collect();
        for r in roots {
            let sub = if r.total > 0 { tf!("{} libres", "{} free", bytes(r.free)) } else { t!("Serveur", "Server").into() };
            quick.push((r.name.clone(), sub, Place::Folder { root: r.name.clone(), path: String::new() }, icons::for_root(&r.name)));
        }
        let tile = vec2(260.0, 60.0);
        let cols = ((ui.available_width() + 8.0) / (tile.x + 8.0)).floor().max(1.0) as usize;
        for row in quick.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (name, sub, place, e) in row {
                    let (r, resp) = ui.allocate_exact_size(tile, Sense::click());
                    let p = ui.painter();
                    if resp.hovered() {
                        p.rect_filled(r, CornerRadius::same(8), theme::overlay(6));
                    }
                    icons::paint(p, Rect::from_center_size(egui::pos2(r.left() + 30.0, r.center().y), vec2(40.0, 40.0)), *e);
                    p.text(egui::pos2(r.left() + 62.0, r.center().y - 9.0), Align2::LEFT_CENTER, name, FontId::proportional(14.0), theme::TEXT());
                    p.text(egui::pos2(r.left() + 62.0, r.center().y + 10.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.0), theme::TEXT_MUTED());
                    if resp.clicked() || resp.double_clicked() {
                        todo.push(Do::Go(place.clone()));
                    }
                    if resp.middle_clicked() {
                        todo.push(Do::NewTab(place.clone()));
                    }
                }
            });
            ui.add_space(4.0);
        }
        ui.add_space(16.0);
        section_title(ui, t!("Récents", "Recent"));
        if items.is_empty() {
            ui.label(RichText::new(t!("Aucun fichier pour l'instant", "No file yet")).color(theme::TEXT_FAINT()));
        } else {
            let h = (items.len() as f32 * ROW_H + 40.0).min(ui.available_height().max(240.0));
            ui.allocate_ui(vec2(ui.available_width(), h), |ui| list(ui, tab, items, true, todo));
        }
    });
}

/// Titre de section avec chevron, comme « ⌄ Accès rapide ».
fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(format!("⌄  {text}")).size(15.0).strong().color(theme::TEXT()));
    ui.add_space(6.0);
}

// --- Volet Détails ----------------------------------------------------------------------

fn details(ui: &mut egui::Ui, tab: &Tab, items: &[Item], roots: &[nas_proto::Root]) {
    let sel: Vec<&Item> = items.iter().filter(|i| tab.selected.contains(&i.key())).collect();
    let big = |ui: &mut egui::Ui, e: Emoji| {
        ui.add_space(24.0);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 112.0), Sense::hover());
        icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(96.0, 96.0)), e);
        ui.add_space(12.0);
    };
    let title = |ui: &mut egui::Ui, t: &str| {
        ui.add(egui::Label::new(RichText::new(t).size(16.0).strong().color(theme::TEXT())).wrap());
        ui.add_space(10.0);
    };
    let field = |ui: &mut egui::Ui, k: &str, v: &str| {
        ui.label(RichText::new(k).small().color(theme::TEXT_FAINT()));
        ui.add(egui::Label::new(RichText::new(v).color(theme::TEXT_MUTED())).wrap());
        ui.add_space(6.0);
    };
    match sel.as_slice() {
        [one] => {
            let (e, kind) = icons::kind(&one.entry.name, one.entry.is_dir);
            big(ui, e);
            title(ui, &one.entry.name);
            field(ui, t!("Type", "Type"), kind);
            if !one.entry.is_dir {
                field(ui, t!("Taille", "Size"), &bytes(one.entry.size));
            }
            field(ui, t!("Modifié le", "Modified"), &date(one.entry.modified_ms));
            let loc = [one.root.as_str()].into_iter().chain(one.parent.split('/').filter(|p| !p.is_empty())).collect::<Vec<_>>().join(" › ");
            field(ui, t!("Emplacement", "Location"), &loc);
        }
        [] => {
            big(ui, place_emoji(&tab.place));
            title(ui, &tab.title());
            let n = items.len();
            field(ui, t!("Contenu", "Contents"), &if n == 1 { t!("1 élément", "1 item").to_string() } else { tf!("{} éléments", "{} items", n) });
            if let Some((r, _)) = tab.folder()
                && let Some(info) = roots.iter().find(|x| x.name == r)
                && info.total > 0
            {
                field(ui, t!("Espace", "Space"), &tf!("{} libres sur {}", "{} free of {}", bytes(info.free), bytes(info.total)));
            }
            ui.label(RichText::new(t!("Sélectionne un élément pour afficher ses détails.", "Select an item to see its details.")).small().color(theme::TEXT_FAINT()));
        }
        many => {
            big(ui, Emoji::Folder);
            title(ui, &tf!("{} éléments sélectionnés", "{} items selected", many.len()));
            let size: u64 = many.iter().map(|i| i.entry.size).sum();
            field(ui, t!("Taille totale", "Total size"), &bytes(size));
        }
    }
}

// --- Vue en liste ------------------------------------------------------------------------

fn list(ui: &mut egui::Ui, st: &mut Tab, items: &[Item], searching: bool, todo: &mut Vec<Do>) {
    let w = ui.available_width();
    // Colonnes masquées quand la place manque (volet étroit) : d'abord la date, puis le type.
    let date_w = if w >= 640.0 { 150.0 } else { 0.0 };
    let kind_w = if w >= 460.0 { if searching { 220.0 } else { 140.0 } } else { 0.0 };
    let size_w = 90.0;
    // En-têtes de colonnes : clic = trier (second clic = ordre inverse).
    let (hr, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
    let cols = [
        (Sort::Name, t!("Nom", "Name"), hr.left() + 34.0, Align2::LEFT_CENTER),
        (Sort::Modified, t!("Modifié le", "Modified"), hr.right() - size_w - kind_w - date_w, Align2::LEFT_CENTER),
        (Sort::Kind, if searching { t!("Dossier", "Folder") } else { t!("Type", "Type") }, hr.right() - size_w - kind_w, Align2::LEFT_CENTER),
        (Sort::Size, t!("Taille", "Size"), hr.right() - 8.0, Align2::RIGHT_CENTER),
    ];
    for (i, (sort, label, x, align)) in cols.iter().enumerate() {
        if (*sort == Sort::Modified && date_w == 0.0) || (*sort == Sort::Kind && kind_w == 0.0) {
            continue;
        }
        let right = cols.get(i + 1).map(|c| c.2).unwrap_or(hr.right());
        let cell = Rect::from_min_max(egui::pos2(if i == 0 { hr.left() } else { *x - 4.0 }, hr.top()), egui::pos2(right, hr.bottom()));
        let resp = ui.interact(cell, egui::Id::unique(("files_col", i)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let arrow = if st.sort == *sort { if st.desc { " ▼" } else { " ▲" } } else { "" };
        let color = if st.sort == *sort || resp.hovered() { theme::TEXT() } else { theme::TEXT_MUTED() };
        ui.painter().text(egui::pos2(*x, hr.center().y), *align, format!("{label}{arrow}"), FontId::proportional(12.5), color);
        if resp.clicked() {
            if st.sort == *sort { st.desc = !st.desc } else { (st.sort, st.desc) = (*sort, false) }
        }
    }
    ui.painter().hline(hr.x_range(), hr.bottom(), Stroke::new(1.0, theme::BORDER()));
    ui.add_space(4.0);

    egui::ScrollArea::vertical().id_salt("files_list").auto_shrink([false, false]).show_rows(ui, ROW_H, items.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let it = &items[i];
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
            let sel = st.selected.contains(&it.key());
            let p = ui.painter();
            if sel {
                p.rect_filled(r, CornerRadius::same(6), theme::gray(0x2A));
            } else if resp.hovered() {
                p.rect_filled(r, CornerRadius::same(6), theme::overlay(6));
            }
            let (emoji, kind) = icons::kind(&it.entry.name, it.entry.is_dir);
            icons::paint(p, Rect::from_center_size(egui::pos2(r.left() + 18.0, r.center().y), vec2(18.0, 18.0)), emoji);
            let name_right = if date_w > 0.0 {
                cols[1].2
            } else if kind_w > 0.0 {
                cols[2].2
            } else {
                r.right() - size_w
            };
            let name_rect = Rect::from_min_max(egui::pos2(r.left() + 34.0, r.top()), egui::pos2(name_right - 12.0, r.bottom()));
            if !name_cell(ui, st, it, name_rect) {
                let font = FontId::proportional(14.0);
                let mut job = egui::text::LayoutJob::simple_singleline(it.entry.name.clone(), font, theme::TEXT());
                job.wrap = egui::text::TextWrapping::truncate_at_width(name_rect.width());
                let g = ui.painter().layout_job(job);
                ui.painter().galley(egui::pos2(name_rect.left(), r.center().y - g.size().y / 2.0), g, theme::TEXT());
            }
            let small = FontId::proportional(12.5);
            let p = ui.painter();
            if date_w > 0.0 {
                p.text(egui::pos2(cols[1].2, r.center().y), Align2::LEFT_CENTER, date(it.entry.modified_ms), small.clone(), theme::TEXT_MUTED());
            }
            let third =
                if searching { [it.root.as_str()].into_iter().chain(it.parent.split('/').filter(|p| !p.is_empty())).collect::<Vec<_>>().join(" › ") } else { kind.to_string() };
            if kind_w > 0.0 {
                let mut job = egui::text::LayoutJob::simple_singleline(third, small.clone(), theme::TEXT_MUTED());
                job.wrap = egui::text::TextWrapping::truncate_at_width(kind_w - 12.0);
                let g = p.layout_job(job);
                p.galley(egui::pos2(cols[2].2, r.center().y - g.size().y / 2.0), g, theme::TEXT_MUTED());
            }
            if !it.entry.is_dir {
                p.text(egui::pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, bytes(it.entry.size), small, theme::TEXT_MUTED());
            }
            interact(ui, st, items, i, &resp, searching, todo);
        }
    });
}

// --- Vue en galerie ----------------------------------------------------------------------

fn gallery(ui: &mut egui::Ui, st: &mut Tab, items: &[Item], searching: bool, todo: &mut Vec<Do>) {
    let w = ui.available_width();
    let cols = ((w + 8.0) / (CELL.x + 8.0)).floor().max(1.0) as usize;
    let gap = (w - cols as f32 * CELL.x) / (cols as f32 - 1.0).max(1.0);
    let rows = items.len().div_ceil(cols);
    egui::ScrollArea::vertical().id_salt("files_gallery").auto_shrink([false, false]).show_rows(ui, CELL.y + 8.0, rows, |ui, range| {
        for row in range {
            let (line, _) = ui.allocate_exact_size(vec2(w, CELL.y + 8.0), Sense::hover());
            for c in 0..cols {
                let i = row * cols + c;
                let Some(it) = items.get(i) else { break };
                let r = Rect::from_min_size(line.min + vec2(c as f32 * (CELL.x + gap), 0.0), CELL);
                let resp = ui.interact(r, egui::Id::unique(("files_cell", i)), Sense::click());
                let sel = st.selected.contains(&it.key());
                let p = ui.painter();
                if sel {
                    p.rect_filled(r, CornerRadius::same(10), theme::gray(0x2A));
                } else if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(10), theme::overlay(6));
                }
                let (emoji, _) = icons::kind(&it.entry.name, it.entry.is_dir);
                icons::paint(p, Rect::from_center_size(egui::pos2(r.center().x, r.top() + 44.0), vec2(56.0, 56.0)), emoji);
                let name_rect = Rect::from_min_max(egui::pos2(r.left() + 8.0, r.top() + 80.0), egui::pos2(r.right() - 8.0, r.bottom() - 6.0));
                if !name_cell(ui, st, it, name_rect) {
                    // Nom sur deux lignes au plus, centré.
                    let mut job = egui::text::LayoutJob::single_section(it.entry.name.clone(), egui::TextFormat::simple(FontId::proportional(13.0), theme::TEXT()));
                    job.wrap = egui::text::TextWrapping { max_width: name_rect.width(), max_rows: 2, break_anywhere: true, overflow_character: Some('…') };
                    job.halign = egui::Align::Center;
                    let g = ui.painter().layout_job(job);
                    ui.painter().galley(egui::pos2(name_rect.center().x, name_rect.top()), g, theme::TEXT());
                }
                interact(ui, st, items, i, &resp, searching, todo);
            }
        }
    });
}

/// Nom en cours de renommage : champ de saisie. Renvoie `true` si le champ est affiché.
fn name_cell(ui: &mut egui::Ui, st: &mut Tab, it: &Item, r: Rect) -> bool {
    let Some((old, buf)) = st.renaming.as_mut() else { return false };
    if *old != it.entry.name || !st.selected.contains(&it.key()) {
        return false;
    }
    let id = egui::Id::unique("files_rename");
    let edit_rect = Rect::from_center_size(r.center(), vec2(r.width(), 24.0));
    let resp = ui
        .scope_builder(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
            let mut out = egui::TextEdit::singleline(buf).id(id).desired_width(edit_rect.width()).show(ui);
            if st.rename_focus {
                // Comme l'Explorateur : le nom est présélectionné, sans l'extension.
                let n = stem_len(&it.entry.name, it.entry.is_dir);
                out.state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                out.state.store(ui.ctx(), id);
                out.response.request_focus();
                st.rename_focus = false;
            }
            out.response
        })
        .inner;
    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    let esc = ui.input(|i| i.key_pressed(Key::Escape));
    if esc {
        st.renaming = None;
    } else if resp.lost_focus() || enter {
        let (old, new) = st.renaming.take().unwrap_or_default();
        let new = new.trim().to_string();
        if !new.is_empty() && new != old {
            // Validé : l'opération part en fin d'image (voir `commit_rename`).
            st.renaming = Some((old, new));
            st.commit = true;
        }
    }
    true
}

/// Nombre de caractères à présélectionner : le nom sans son extension.
fn stem_len(name: &str, is_dir: bool) -> usize {
    let stem = if is_dir { name } else { name.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(name) };
    stem.chars().count()
}

// --- Clic, double-clic, menu contextuel -------------------------------------------------

fn interact(ui: &mut egui::Ui, st: &mut Tab, items: &[Item], i: usize, resp: &egui::Response, searching: bool, todo: &mut Vec<Do>) {
    let it = &items[i];
    let mods = ui.input(|m| m.modifiers);
    if resp.clicked() {
        if mods.shift
            && let Some(a) = st.anchor
        {
            let (lo, hi) = (a.min(i), a.max(i));
            if !mods.command {
                st.selected.clear();
            }
            st.selected.extend(items[lo..=hi.min(items.len() - 1)].iter().map(Item::key));
        } else if mods.command {
            if !st.selected.remove(&it.key()) {
                st.selected.insert(it.key());
            }
            st.anchor = Some(i);
        } else {
            st.selected = [it.key()].into();
            st.anchor = Some(i);
        }
    }
    if resp.double_clicked() {
        todo.push(Do::Open(it.clone()));
    }
    if resp.middle_clicked() && it.entry.is_dir {
        todo.push(Do::NewTab(Place::Folder { root: it.root.clone(), path: it.path.clone() }));
    }
    if resp.secondary_clicked() && !st.selected.contains(&it.key()) {
        st.selected = [it.key()].into();
        st.anchor = Some(i);
    }
    let many = st.selected.len() > 1;
    // Menu clic droit, sans les raccourcis (ils marchent toujours, voir le haut du fichier).
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        if !many && ui.button(t!("Ouvrir", "Open")).clicked() {
            todo.push(Do::Open(it.clone()));
            ui.close();
        }
        if !many && it.entry.is_dir && ui.button(t!("Ouvrir dans un nouvel onglet", "Open in new tab")).clicked() {
            todo.push(Do::NewTab(Place::Folder { root: it.root.clone(), path: it.path.clone() }));
            ui.close();
        }
        if searching && !many && ui.button(t!("Ouvrir l'emplacement", "Open location")).clicked() {
            todo.push(Do::Reveal(it.clone()));
            ui.close();
        }
        if !it.entry.is_dir && ui.button(t!("Télécharger", "Download")).clicked() {
            for s in items.iter().filter(|x| st.selected.contains(&x.key()) && !x.entry.is_dir) {
                todo.push(Do::Download(s.clone()));
            }
            ui.close();
        }
        ui.separator();
        if ui.button(t!("Copier", "Copy")).clicked() {
            todo.push(Do::Copy(false));
            ui.close();
        }
        if ui.button(t!("Couper", "Cut")).clicked() {
            todo.push(Do::Copy(true));
            ui.close();
        }
        if !many && ui.button(t!("Renommer", "Rename")).clicked() {
            todo.push(Do::Rename(it.clone()));
            ui.close();
        }
        if !many && it.entry.is_dir && ui.button(t!("Épingler", "Pin")).clicked() {
            todo.push(Do::Favorite(it.root.clone(), it.path.clone()));
            ui.close();
        }
        ui.separator();
        if ui.button(RichText::new(t!("Mettre à la corbeille", "Move to trash")).color(theme::DANGER())).clicked() {
            todo.push(Do::Delete);
            ui.close();
        }
    });
}

// --- Clavier -------------------------------------------------------------------------------

fn keyboard(ui: &mut egui::Ui, st: &mut State, items: &[Item], todo: &mut Vec<Do>) {
    let ctx = ui.ctx().clone();
    ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::COMMAND, Key::T) {
            todo.push(Do::NewTab(Place::Home));
        }
        if i.consume_key(egui::Modifiers::COMMAND, Key::W) && st.tabs.len() > 1 {
            todo.push(Do::CloseTab(st.active));
        }
        if i.consume_key(egui::Modifiers::COMMAND, Key::Tab) {
            todo.push(Do::SelectTab((st.active + 1) % st.tabs.len()));
        }
        if i.consume_key(egui::Modifiers::COMMAND, Key::F) {
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("files_search")));
        }
    });
    // Pas de raccourcis pendant la saisie (recherche, renommage).
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let tab = st.tab();
    let selected: Vec<&Item> = items.iter().filter(|i| tab.selected.contains(&i.key())).collect();
    ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::COMMAND, Key::A) {
            tab.selected = items.iter().map(Item::key).collect();
        }
        // egui transforme Ctrl+C/X/V en évènements presse-papiers, pas en touches.
        for e in &i.events {
            match e {
                egui::Event::Copy => todo.push(Do::Copy(false)),
                egui::Event::Cut => todo.push(Do::Copy(true)),
                egui::Event::Paste(_) | egui::Event::PasteImage(_) => todo.push(Do::Paste),
                _ => {}
            }
        }
        if i.consume_key(egui::Modifiers::NONE, Key::Delete) && !selected.is_empty() {
            todo.push(Do::Delete);
        }
        if i.consume_key(egui::Modifiers::NONE, Key::F2)
            && let [one] = selected.as_slice()
        {
            todo.push(Do::Rename((*one).clone()));
        }
        if i.consume_key(egui::Modifiers::NONE, Key::Enter)
            && let [one] = selected.as_slice()
        {
            todo.push(Do::Open((*one).clone()));
        }
        if i.consume_key(egui::Modifiers::NONE, Key::F5) {
            todo.push(Do::Refresh);
        }
        if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
            if tab.selected.is_empty() { tab.query.clear() } else { tab.selected.clear() }
        }
        if (i.consume_key(egui::Modifiers::NONE, Key::Backspace) || i.consume_key(egui::Modifiers::ALT, Key::ArrowUp))
            && let Place::Folder { root, path } = &tab.place
        {
            todo.push(Do::Go(if path.is_empty() { Place::Home } else { Place::Folder { root: root.clone(), path: parent_of(path) } }));
        }
        if i.consume_key(egui::Modifiers::ALT, Key::ArrowLeft) {
            todo.push(Do::Back);
        }
        if i.consume_key(egui::Modifiers::ALT, Key::ArrowRight) {
            todo.push(Do::Forward);
        }
    });
}

// --- Barre d'état ----------------------------------------------------------------------

fn status_bar(ui: &mut egui::Ui, st: &State, server: &Server, items: &[Item], last: &Option<(bool, String)>, transfers: &[crate::server::Transfer]) {
    let tab = &st.tabs[st.active];
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), STATUS_H), Sense::hover());
    ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, theme::BORDER()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(vec2(0.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        let sel: Vec<&Item> = items.iter().filter(|i| tab.selected.contains(&i.key())).collect();
        let mut text = if items.len() == 1 { t!("1 élément", "1 item").to_string() } else { tf!("{} éléments", "{} items", items.len()) };
        if !sel.is_empty() {
            let size: u64 = sel.iter().map(|i| i.entry.size).sum();
            text.push_str(&tf!("  ·  {} sélectionnés ({})", "  ·  {} selected ({})", sel.len(), bytes(size)));
        }
        if let Some(c) = &st.clip {
            text.push_str(&if c.cut { tf!("  ·  {} à déplacer", "  ·  {} to move", c.paths.len()) } else { tf!("  ·  {} à copier", "  ·  {} to copy", c.paths.len()) });
        }
        ui.label(RichText::new(text).small().color(theme::TEXT_MUTED()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let running = transfers.iter().filter(|t| !t.done).count();
            if running > 0 {
                ui.label(RichText::new(tf!("{} transfert(s) en cours", "{} transfer(s) running", running)).small().color(theme::TEXT_MUTED()));
                ui.spinner();
            } else if let Some((ok, msg)) = last {
                if ui.small_button("✕").clicked() {
                    server.data().files.last = None;
                }
                let t = if *ok { RichText::new(format!("✓ {msg}")).color(theme::TEXT_MUTED()) } else { RichText::new(format!("✗ {msg}")).color(theme::DANGER()) };
                ui.add(egui::Label::new(t.small()).truncate());
            }
            if ui.small_button(t!("Ouvrir Téléchargements", "Open Downloads")).on_hover_text(crate::server::download_dir().display().to_string()).clicked() {
                let dir = crate::server::download_dir();
                let _ = std::fs::create_dir_all(&dir);
                crate::server::open_local(&dir);
            }
        });
    });
}

// --- Actions -----------------------------------------------------------------------------

fn run(d: Do, st: &mut State, server: &Server, prefs: &mut Prefs, items: &[Item]) {
    match d {
        Do::NewTab(place) => {
            st.tabs.push(Tab::new(place));
            st.activate(st.tabs.len() - 1);
            return;
        }
        Do::CloseTab(i) => {
            if st.tabs.len() > 1 && i < st.tabs.len() {
                st.tabs.remove(i);
                let next = if st.active > i || st.active == st.tabs.len() { st.active.saturating_sub(1) } else { st.active };
                st.activate(next);
            }
            return;
        }
        Do::SelectTab(i) => {
            if i != st.active {
                st.activate(i);
            }
            return;
        }
        _ => {}
    }
    let State { tabs, active, clip, copy_text, .. } = st;
    let tab = &mut tabs[*active];
    let selected: Vec<&Item> = items.iter().filter(|i| tab.selected.contains(&i.key())).collect();
    match d {
        Do::Go(p) => navigate(tab, p),
        Do::Open(it) if it.entry.is_dir => navigate(tab, Place::Folder { root: it.root.clone(), path: it.path.clone() }),
        Do::Open(it) => server.download(&it.root, &it.path, true),
        Do::Download(it) => server.download(&it.root, &it.path, false),
        Do::Reveal(it) => {
            navigate(tab, Place::Folder { root: it.root.clone(), path: it.parent.clone() });
            tab.selected = [it.key()].into();
        }
        Do::Rename(it) => {
            // Depuis la barre de commandes : l'élément sélectionné.
            let it = if it.path.is_empty() { selected.first().map(|s| (*s).clone()) } else { Some(it) };
            if let Some(it) = it {
                tab.selected = [it.key()].into();
                tab.renaming = Some((it.entry.name.clone(), it.entry.name.clone()));
                tab.rename_focus = true;
            }
        }
        Do::Copy(cut) => {
            if let Some(first) = selected.first() {
                let c = Clip { cut, root: first.root.clone(), paths: selected.iter().filter(|i| i.root == first.root).map(|i| i.path.clone()).collect() };
                *copy_text = Some(c.paths.iter().map(|p| format!("{} › {p}", c.root)).collect::<Vec<_>>().join("\n"));
                *clip = Some(c);
            }
        }
        Do::Paste => {
            if let (Some(c), Some((root, path))) = (clip.clone(), tab.folder().filter(|_| !tab.searching())) {
                let op = if c.cut { FileOp::Move } else { FileOp::Copy };
                for p in &c.paths {
                    let msg = if c.cut { tf!("{} déplacé", "{} moved", p) } else { tf!("{} copié", "{} copied", p) };
                    server.files_op(op, &c.root, p, Some(root), Some(path), msg);
                }
                if c.cut {
                    *clip = None;
                }
            }
        }
        Do::Delete => {
            for it in &selected {
                server.files_op(FileOp::Delete, &it.root, &it.path, None, None, tf!("{} mis à la corbeille", "{} moved to trash", it.entry.name));
            }
            tab.selected.clear();
        }
        Do::Favorite(r, p) => {
            let f = Favorite { root: r, path: p };
            if !prefs.favorites.contains(&f) {
                prefs.favorites.push(f);
            }
        }
        Do::Unfavorite(i) => {
            if i < prefs.favorites.len() {
                prefs.favorites.remove(i);
            }
        }
        Do::Back => {
            if let Some(p) = tab.back.pop() {
                let cur = std::mem::replace(&mut tab.place, p);
                tab.fwd.push(cur);
                reset_view(tab);
            }
        }
        Do::Forward => {
            if let Some(p) = tab.fwd.pop() {
                let cur = std::mem::replace(&mut tab.place, p);
                tab.back.push(cur);
                reset_view(tab);
            }
        }
        Do::Refresh => {
            tab.requested = false;
            tab.sent_query.clear();
            server.files_roots();
        }
        Do::NewFolder => new_folder(tab, server),
        Do::NewTab(_) | Do::CloseTab(_) | Do::SelectTab(_) => {}
    }
}

/// Envoie le renommage validé dans `name_cell`.
fn commit_rename(st: &mut Tab, server: &Server, items: &[Item]) {
    if std::mem::take(&mut st.commit)
        && let Some((old, new)) = st.renaming.take()
        && let Some(it) = items.iter().find(|i| i.entry.name == old && st.selected.contains(&i.key()))
    {
        server.files_op(FileOp::Rename, &it.root, &it.path, None, Some(&new), tf!("{} renommé en {}", "{} renamed to {}", old, new));
    }
}

fn navigate(tab: &mut Tab, place: Place) {
    if tab.place != place {
        let cur = std::mem::replace(&mut tab.place, place);
        tab.back.push(cur);
        tab.fwd.clear();
    }
    tab.query.clear();
    reset_view(tab);
}

/// Nouveau lieu affiché : sélection vidée, contenu redemandé.
fn reset_view(tab: &mut Tab) {
    tab.selected.clear();
    tab.renaming = None;
    tab.anchor = None;
    tab.requested = false;
    if tab.place == Place::Gallery {
        (tab.sort, tab.desc) = (Sort::Modified, true);
    }
}

/// Crée « Nouveau dossier » (ou « Nouveau dossier (2) »…) puis passe en renommage.
fn new_folder(tab: &mut Tab, server: &Server) {
    let Some((root, path)) = tab.folder() else { return };
    let (root, path) = (root.to_string(), path.to_string());
    let existing: Vec<String> = server.data().files.listing.as_ref().and_then(|l| l.as_ref().ok()).map(|l| l.entries.iter().map(|e| e.name.clone()).collect()).unwrap_or_default();
    let base = t!("Nouveau dossier", "New folder");
    let name = std::iter::once(base.to_string()).chain((2..).map(|i| format!("{base} ({i})"))).find(|n| !existing.contains(n)).unwrap_or_default();
    server.files_op(FileOp::Mkdir, &root, &join(&path, &name), None, None, tf!("{} créé", "{} created", name));
    tab.pending_rename = Some(name);
}

/// Tri façon Explorateur : dossiers d'abord, puis la colonne choisie.
fn sort_items(items: &mut [Item], sort: Sort, desc: bool) {
    items.sort_by(|a, b| {
        let dirs = b.entry.is_dir.cmp(&a.entry.is_dir);
        let by = match sort {
            Sort::Name => a.entry.name.to_lowercase().cmp(&b.entry.name.to_lowercase()),
            Sort::Modified => a.entry.modified_ms.cmp(&b.entry.modified_ms),
            Sort::Kind => icons::kind(&a.entry.name, a.entry.is_dir).1.cmp(icons::kind(&b.entry.name, b.entry.is_dir).1).then_with(|| a.path.cmp(&b.path)),
            Sort::Size => a.entry.size.cmp(&b.entry.size),
        };
        dirs.then(if desc { by.reverse() } else { by })
    });
}

/// « 07/11/2026 09:42 », à l'heure locale.
fn date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, is_dir: bool, size: u64) -> Item {
        Item { root: "R".into(), path: name.into(), parent: String::new(), entry: Entry { name: name.into(), is_dir, size, modified_ms: 0 } }
    }

    #[test]
    fn sorting() {
        let mut v = vec![item("b.txt", false, 5), item("Zeta", true, 0), item("a.png", false, 9), item("alpha", true, 0)];
        sort_items(&mut v, Sort::Name, false);
        let names: Vec<&str> = v.iter().map(|i| i.entry.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Zeta", "a.png", "b.txt"]);
        sort_items(&mut v, Sort::Size, true);
        assert_eq!(v[2].entry.name, "a.png");
    }

    #[test]
    fn paths() {
        assert_eq!(join("", "a"), "a");
        assert_eq!(join("a/b", "c"), "a/b/c");
        assert_eq!(parent_of("a/b/c"), "a/b");
        assert_eq!(parent_of("a"), "");
    }
}
