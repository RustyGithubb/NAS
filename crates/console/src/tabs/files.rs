//! Fichiers : explorateur des emplacements du serveur, version simplifiée de l'Explorateur
//! Windows (navigation, liste ou galerie, tri, sélection multiple, copier/couper/coller,
//! renommer, corbeille, favoris, envoi par glisser-déposer, téléchargement) avec la recherche
//! instantanée d'Everything (index tenu par l'agent).
//!
//! Raccourcis : Entrée ouvrir · Retour arrière dossier parent · Alt+←/→ précédent/suivant ·
//! F2 renommer · Suppr corbeille · Ctrl+A tout · Ctrl+C/X/V · F5 actualiser · Ctrl+F rechercher.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, CornerRadius, FontId, Key, Rect, RichText, Sense, Stroke, vec2};
use nas_proto::{Entry, FileOp};

use super::{header, unavailable};
use crate::icons::{self, Emoji};
use crate::liquid::Liquid;
use crate::prefs::{Favorite, Prefs};
use crate::server::{Link, Server};
use crate::tabs::dashboard::bytes;
use crate::{theme, widgets};

/// Délai avant de lancer la recherche pendant la frappe.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
const ROW_H: f32 = 30.0;
const CELL: egui::Vec2 = egui::Vec2::new(132.0, 132.0);
const STATUS_H: f32 = 30.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Sort {
    #[default]
    Name,
    Modified,
    Kind,
    Size,
}

/// Éléments copiés ou coupés, en attente de « Coller ».
#[derive(Clone)]
struct Clip {
    cut: bool,
    root: String,
    paths: Vec<String>,
}

/// État de l'onglet (gardé d'une visite à l'autre).
#[derive(Default)]
pub struct State {
    root: Option<String>,
    path: String,
    back: Vec<(String, String)>,
    fwd: Vec<(String, String)>,
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
    clip: Option<Clip>,
    /// Texte à placer dans le presse-papiers du système (sinon Ctrl+V n'arrive pas jusqu'à egui).
    copy_text: Option<String>,
    requested: Option<(String, String)>,
    roots_asked: bool,
}

/// Élément affiché (dossier courant ou résultat de recherche).
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
    Go(String, String),
    Download(Item),
    Rename(Item),
    Copy(bool),
    Paste,
    Delete,
    Favorite(String, String),
    Unfavorite(usize),
    Reveal(Item),
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
    let (roots, listing, loading, search, last, transfers) = {
        let d = server.data();
        let f = &d.files;
        (f.roots.clone().unwrap_or_default(), f.listing.clone(), f.loading, f.search.clone(), f.last.clone(), f.transfers.clone())
    };
    if st.root.is_none() {
        st.root = roots.first().map(|r| r.name.clone());
    }
    let Some(root) = st.root.clone() else {
        header(ui, overline, title, t!("Chargement…", "Loading…"), false);
        return;
    };
    let want = (root.clone(), st.path.clone());
    if st.requested.as_ref() != Some(&want) {
        server.files_list(&root, &st.path);
        st.requested = Some(want);
    }

    // Recherche instantanée : lancée peu après la dernière frappe.
    let searching = !st.query.trim().is_empty();
    if searching && st.query != st.sent_query && st.query_edited.is_none_or(|t| t.elapsed() >= SEARCH_DEBOUNCE) {
        server.files_search(st.query.trim());
        st.sent_query = st.query.clone();
    } else if searching && st.query != st.sent_query {
        ui.ctx().request_repaint_after(SEARCH_DEBOUNCE);
    }

    // Éléments affichés.
    let mut items: Vec<Item> = if searching {
        match &search {
            Some((_, Ok(r))) => r.hits.iter().map(|h| Item { root: h.root.clone(), path: h.path.clone(), parent: parent_of(&h.path), entry: h.entry.clone() }).collect(),
            _ => Vec::new(),
        }
    } else {
        match &listing {
            Some(Ok(l)) if l.root == root && l.path == st.path => {
                l.entries.iter().map(|e| Item { root: root.clone(), path: join(&st.path, &e.name), parent: st.path.clone(), entry: e.clone() }).collect()
            }
            _ => Vec::new(),
        }
    };
    sort_items(&mut items, st.sort, st.desc);
    if let Some(name) = st.pending_rename.clone()
        && let Some(it) = items.iter().find(|i| i.entry.name == name)
    {
        st.selected = [it.key()].into();
        st.renaming = Some((name.clone(), name));
        st.rename_focus = true;
        st.pending_rename = None;
    }

    let root_info = roots.iter().find(|r| r.name == root);
    let badge = match root_info {
        Some(r) if r.total > 0 => tf!("{} libres sur {}", "{} free of {}", bytes(r.free), bytes(r.total)),
        _ => String::new(),
    };
    header(ui, overline, title, &badge, false);

    let mut todo: Vec<Do> = Vec::new();
    keyboard(ui, st, &items, &mut todo);

    // Fichiers glissés depuis Windows : envoyés dans le dossier affiché.
    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
    if !dropped.is_empty() && !searching {
        server.upload(&root, &st.path, dropped);
    }
    let hovering_files = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());

    widgets::pane_columns(ui, &[widgets::SIDE_W], |i, ui| {
        if i == 0 {
            side(ui, st, prefs, &roots, &root, &search, &mut todo);
        } else {
            let content = ui.max_rect();
            toolbar(ui, st, server, prefs, &root, searching, &mut todo);
            let h = (ui.available_height() - STATUS_H).max(40.0);
            ui.allocate_ui(vec2(ui.available_width(), h), |ui| {
                ui.set_min_size(vec2(ui.available_width(), h));
                if searching {
                    if search.is_none() || st.query != st.sent_query && items.is_empty() {
                        ui.vertical_centered(|ui| ui.spinner());
                    } else if items.is_empty() {
                        ui.add_space(40.0);
                        widgets::empty_state(ui, t!("Aucun résultat", "No result"), t!("Essaie un autre mot, *.png ou ext:jpg", "Try another word, *.png or ext:jpg"));
                    }
                } else if loading && items.is_empty() {
                    ui.vertical_centered(|ui| ui.spinner());
                } else if let Some(Err(e)) = &listing {
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
                    if prefs.files_gallery { gallery(ui, st, &items, searching, &mut todo) } else { list(ui, st, &items, searching, &mut todo) }
                }
            });
            status_bar(ui, st, server, &items, &last, &transfers);
            if hovering_files && !searching {
                let p = ui.painter();
                p.rect_filled(content.expand(8.0), CornerRadius::same(10), theme::overlay(30));
                p.rect_stroke(content.expand(8.0), CornerRadius::same(10), Stroke::new(2.0, theme::TEXT_MUTED()), egui::StrokeKind::Inside);
                p.text(
                    content.center(),
                    Align2::CENTER_CENTER,
                    tf!("Déposer pour envoyer dans « {} »", "Drop to upload into “{}”", if st.path.is_empty() { &root } else { &st.path }),
                    FontId::proportional(18.0),
                    theme::TEXT(),
                );
            }
        }
    });

    for d in todo {
        run(d, st, server, prefs, &root, &items);
    }
    commit_rename(st, server, &items);
    if let Some(text) = st.copy_text.take() {
        ui.ctx().copy_text(text);
    }
}

// --- Colonne de gauche -----------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn side(
    ui: &mut egui::Ui,
    st: &mut State,
    prefs: &Prefs,
    roots: &[nas_proto::Root],
    root: &str,
    search: &Option<(String, Result<nas_proto::SearchResult, String>)>,
    todo: &mut Vec<Do>,
) {
    let resp = ui.add(
        egui::TextEdit::singleline(&mut st.query).id(egui::Id::unique("files_search")).hint_text(t!("Rechercher partout…", "Search everywhere…")).desired_width(f32::INFINITY),
    );
    if resp.changed() {
        st.query_edited = Some(Instant::now());
        st.selected.clear();
    }
    let hint = match search {
        Some((_, Ok(r))) if !st.query.trim().is_empty() => tf!("{} éléments indexés", "{} items indexed", r.indexed),
        _ => "*.png · ext:jpg;png · dossier:".to_string(),
    };
    ui.label(RichText::new(hint).small().color(theme::TEXT_FAINT()));
    ui.add_space(12.0);

    widgets::side_label(ui, t!("Emplacements", "Locations"));
    let mut hl = Liquid::begin(ui, "files_roots");
    for r in roots {
        let e = icons::for_root(&r.name);
        let lead = move |p: &egui::Painter, c: egui::Pos2| icons::paint(p, Rect::from_center_size(c, vec2(18.0, 18.0)), e);
        let active = st.query.trim().is_empty() && r.name == root;
        if widgets::nav_row(ui, &r.name, None, active, widgets::Tone::Normal, &mut hl, Some(&lead)).clicked() {
            st.query.clear();
            todo.push(Do::Go(r.name.clone(), String::new()));
        }
    }
    widgets::nav_end(ui, hl);

    ui.add_space(12.0);
    widgets::side_label(ui, t!("Favoris", "Favorites"));
    if prefs.favorites.is_empty() {
        ui.label(RichText::new(t!("Clic droit sur un dossier › Ajouter aux favoris", "Right-click a folder › Add to favorites")).small().color(theme::TEXT_FAINT()));
    }
    let mut hl = Liquid::begin(ui, "files_favs");
    for (i, f) in prefs.favorites.iter().enumerate() {
        let lead = |p: &egui::Painter, c: egui::Pos2| icons::paint(p, Rect::from_center_size(c, vec2(16.0, 16.0)), Emoji::Star);
        let label = f.path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(&f.root).to_string();
        let active = st.query.trim().is_empty() && f.root == root && f.path == st.path;
        let r = widgets::nav_row(ui, &label, None, active, widgets::Tone::Normal, &mut hl, Some(&lead)).on_hover_text(format!("{} › {}", f.root, f.path));
        if r.clicked() {
            st.query.clear();
            todo.push(Do::Go(f.root.clone(), f.path.clone()));
        }
        r.context_menu(|ui| {
            if ui.button(t!("Retirer des favoris", "Remove from favorites")).clicked() {
                todo.push(Do::Unfavorite(i));
                ui.close();
            }
        });
    }
    widgets::nav_end(ui, hl);
}

// --- Barre d'outils ----------------------------------------------------------------------

fn toolbar(ui: &mut egui::Ui, st: &mut State, server: &Server, prefs: &mut Prefs, root: &str, searching: bool, todo: &mut Vec<Do>) {
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        if searching {
            ui.label(RichText::new(tf!("Résultats pour « {} »", "Results for “{}”", st.query.trim())).size(16.0).color(theme::TEXT()));
        } else {
            if ui.add_enabled(!st.back.is_empty(), egui::Button::new("←")).on_hover_text(t!("Précédent (Alt+←)", "Back (Alt+←)")).clicked() {
                back(st);
            }
            if ui.add_enabled(!st.fwd.is_empty(), egui::Button::new("→")).on_hover_text(t!("Suivant (Alt+→)", "Forward (Alt+→)")).clicked() {
                forward(st);
            }
            if ui.add_enabled(!st.path.is_empty(), egui::Button::new("↑")).on_hover_text(t!("Dossier parent (Retour arrière)", "Parent folder (Backspace)")).clicked() {
                todo.push(Do::Go(root.to_string(), parent_of(&st.path)));
            }
            if ui.button("⟳").on_hover_text(t!("Actualiser (F5)", "Refresh (F5)")).clicked() {
                st.requested = None;
                server.files_roots();
            }
            ui.add_space(6.0);
            // Fil d'Ariane : emplacement › dossier › sous-dossier, chaque niveau cliquable.
            ui.spacing_mut().item_spacing.x = 2.0;
            if ui.add(egui::Button::new(RichText::new(root).size(15.0)).frame(false)).clicked() {
                todo.push(Do::Go(root.to_string(), String::new()));
            }
            let mut acc = String::new();
            for part in st.path.split('/').filter(|p| !p.is_empty()) {
                acc = join(&acc, part);
                ui.label(RichText::new("›").color(theme::TEXT_FAINT()));
                if ui.add(egui::Button::new(RichText::new(part).size(15.0)).frame(false)).clicked() {
                    todo.push(Do::Go(root.to_string(), acc.clone()));
                }
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            widgets::segmented(ui, "files_view", &[(true, t!("Galerie", "Gallery")), (false, t!("Liste", "List"))], &mut prefs.files_gallery);
            if !searching {
                #[cfg(windows)]
                if ui
                    .button(t!("Envoyer…", "Upload…"))
                    .on_hover_text(t!("Envoyer des fichiers de ce PC (ou glisse-les dans la fenêtre)", "Upload files from this PC (or drop them on the window)"))
                    .clicked()
                {
                    let (server, root, dir) = (server.handle_pub(), root.to_string(), st.path.clone());
                    std::thread::spawn(move || {
                        if let Some(files) = rfd::FileDialog::new().pick_files() {
                            server.upload(&root, &dir, files);
                        }
                    });
                }
                if let Some(c) = &st.clip
                    && ui.button(t!("Coller", "Paste")).on_hover_text(tf!("{} élément(s) (Ctrl+V)", "{} item(s) (Ctrl+V)", c.paths.len())).clicked()
                {
                    todo.push(Do::Paste);
                }
                if ui.button(t!("Nouveau dossier", "New folder")).clicked() {
                    new_folder(st, server, root);
                }
            }
        });
    });
    ui.add_space(8.0);
}

// --- Vue en liste ------------------------------------------------------------------------

fn list(ui: &mut egui::Ui, st: &mut State, items: &[Item], searching: bool, todo: &mut Vec<Do>) {
    let w = ui.available_width();
    let (date_w, kind_w, size_w) = (150.0, if searching { 220.0 } else { 140.0 }, 90.0);
    // En-têtes de colonnes : clic = trier (second clic = ordre inverse).
    let (hr, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
    let cols = [
        (Sort::Name, t!("Nom", "Name"), hr.left() + 34.0, Align2::LEFT_CENTER),
        (Sort::Modified, t!("Modifié le", "Modified"), hr.right() - size_w - kind_w - date_w, Align2::LEFT_CENTER),
        (Sort::Kind, if searching { t!("Dossier", "Folder") } else { t!("Type", "Type") }, hr.right() - size_w - kind_w, Align2::LEFT_CENTER),
        (Sort::Size, t!("Taille", "Size"), hr.right() - 8.0, Align2::RIGHT_CENTER),
    ];
    for (i, (sort, label, x, align)) in cols.iter().enumerate() {
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
            let name_rect = Rect::from_min_max(egui::pos2(r.left() + 34.0, r.top()), egui::pos2(cols[1].2 - 12.0, r.bottom()));
            if !name_cell(ui, st, it, name_rect) {
                let font = FontId::proportional(14.0);
                let mut job = egui::text::LayoutJob::simple_singleline(it.entry.name.clone(), font, theme::TEXT());
                job.wrap = egui::text::TextWrapping::truncate_at_width(name_rect.width());
                let g = ui.painter().layout_job(job);
                ui.painter().galley(egui::pos2(name_rect.left(), r.center().y - g.size().y / 2.0), g, theme::TEXT());
            }
            let small = FontId::proportional(12.5);
            let p = ui.painter();
            p.text(egui::pos2(cols[1].2, r.center().y), Align2::LEFT_CENTER, date(it.entry.modified_ms), small.clone(), theme::TEXT_MUTED());
            let third =
                if searching { [it.root.as_str()].into_iter().chain(it.parent.split('/').filter(|p| !p.is_empty())).collect::<Vec<_>>().join(" › ") } else { kind.to_string() };
            let mut job = egui::text::LayoutJob::simple_singleline(third, small.clone(), theme::TEXT_MUTED());
            job.wrap = egui::text::TextWrapping::truncate_at_width(kind_w - 12.0);
            let g = p.layout_job(job);
            p.galley(egui::pos2(cols[2].2, r.center().y - g.size().y / 2.0), g, theme::TEXT_MUTED());
            if !it.entry.is_dir {
                p.text(egui::pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, bytes(it.entry.size), small, theme::TEXT_MUTED());
            }
            interact(ui, st, items, i, &resp, searching, todo);
        }
    });
}

// --- Vue en galerie ----------------------------------------------------------------------

fn gallery(ui: &mut egui::Ui, st: &mut State, items: &[Item], searching: bool, todo: &mut Vec<Do>) {
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
fn name_cell(ui: &mut egui::Ui, st: &mut State, it: &Item, r: Rect) -> bool {
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

fn interact(ui: &mut egui::Ui, st: &mut State, items: &[Item], i: usize, resp: &egui::Response, searching: bool, todo: &mut Vec<Do>) {
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
    if resp.secondary_clicked() && !st.selected.contains(&it.key()) {
        st.selected = [it.key()].into();
        st.anchor = Some(i);
    }
    let many = st.selected.len() > 1;
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        if !many && ui.button(t!("Ouvrir", "Open")).clicked() {
            todo.push(Do::Open(it.clone()));
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
        if ui.button(t!("Copier (Ctrl+C)", "Copy (Ctrl+C)")).clicked() {
            todo.push(Do::Copy(false));
            ui.close();
        }
        if ui.button(t!("Couper (Ctrl+X)", "Cut (Ctrl+X)")).clicked() {
            todo.push(Do::Copy(true));
            ui.close();
        }
        if !many && ui.button(t!("Renommer (F2)", "Rename (F2)")).clicked() {
            todo.push(Do::Rename(it.clone()));
            ui.close();
        }
        if !many && it.entry.is_dir && ui.button(t!("Ajouter aux favoris", "Add to favorites")).clicked() {
            todo.push(Do::Favorite(it.root.clone(), it.path.clone()));
            ui.close();
        }
        ui.separator();
        if ui.button(RichText::new(t!("Mettre à la corbeille (Suppr)", "Move to trash (Del)")).color(theme::DANGER())).clicked() {
            todo.push(Do::Delete);
            ui.close();
        }
    });
}

// --- Clavier -------------------------------------------------------------------------------

fn keyboard(ui: &mut egui::Ui, st: &mut State, items: &[Item], todo: &mut Vec<Do>) {
    let ctx = ui.ctx().clone();
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::F)) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique("files_search")));
    }
    // Pas de raccourcis pendant la saisie (recherche, renommage).
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let selected: Vec<&Item> = items.iter().filter(|i| st.selected.contains(&i.key())).collect();
    ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::COMMAND, Key::A) {
            st.selected = items.iter().map(Item::key).collect();
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
            st.requested = None;
        }
        if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
            if st.selected.is_empty() { st.query.clear() } else { st.selected.clear() }
        }
        if (i.consume_key(egui::Modifiers::NONE, Key::Backspace) || i.consume_key(egui::Modifiers::ALT, Key::ArrowUp))
            && let Some(root) = st.root.clone()
            && !st.path.is_empty()
        {
            todo.push(Do::Go(root, parent_of(&st.path)));
        }
        if i.consume_key(egui::Modifiers::ALT, Key::ArrowLeft) {
            back(st);
        }
        if i.consume_key(egui::Modifiers::ALT, Key::ArrowRight) {
            forward(st);
        }
    });
}

// --- Barre d'état ----------------------------------------------------------------------

fn status_bar(ui: &mut egui::Ui, st: &mut State, server: &Server, items: &[Item], last: &Option<(bool, String)>, transfers: &[crate::server::Transfer]) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), STATUS_H), Sense::hover());
    ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, theme::BORDER()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(vec2(0.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        let sel: Vec<&Item> = items.iter().filter(|i| st.selected.contains(&i.key())).collect();
        let mut text = if items.len() == 1 { t!("1 élément", "1 item").to_string() } else { tf!("{} éléments", "{} items", items.len()) };
        if !sel.is_empty() {
            let size: u64 = sel.iter().map(|i| i.entry.size).sum();
            text.push_str(&tf!("  ·  {} sélectionnés ({})", "  ·  {} selected ({})", sel.len(), bytes(size)));
        }
        if let Some(c) = &st.clip {
            text.push_str(&if c.cut {
                tf!("  ·  {} à déplacer (Ctrl+V)", "  ·  {} to move (Ctrl+V)", c.paths.len())
            } else {
                tf!("  ·  {} à copier (Ctrl+V)", "  ·  {} to copy (Ctrl+V)", c.paths.len())
            });
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

fn run(d: Do, st: &mut State, server: &Server, prefs: &mut Prefs, root: &str, items: &[Item]) {
    let selected: Vec<&Item> = items.iter().filter(|i| st.selected.contains(&i.key())).collect();
    match d {
        Do::Go(r, p) => navigate(st, r, p),
        Do::Open(it) if it.entry.is_dir => {
            st.query.clear();
            navigate(st, it.root.clone(), it.path.clone());
        }
        Do::Open(it) => server.download(&it.root, &it.path, true),
        Do::Download(it) => server.download(&it.root, &it.path, false),
        Do::Reveal(it) => {
            st.query.clear();
            navigate(st, it.root.clone(), it.parent.clone());
            st.selected = [it.key()].into();
        }
        Do::Rename(it) => {
            st.selected = [it.key()].into();
            st.renaming = Some((it.entry.name.clone(), it.entry.name.clone()));
            st.rename_focus = true;
        }
        Do::Copy(cut) => {
            if let Some(first) = selected.first() {
                let clip = Clip { cut, root: first.root.clone(), paths: selected.iter().filter(|i| i.root == first.root).map(|i| i.path.clone()).collect() };
                st.copy_text = Some(clip.paths.iter().map(|p| format!("{} › {p}", clip.root)).collect::<Vec<_>>().join("\n"));
                st.clip = Some(clip);
            }
        }
        Do::Paste => {
            if let Some(c) = st.clip.clone() {
                let op = if c.cut { FileOp::Move } else { FileOp::Copy };
                for p in &c.paths {
                    let msg = if c.cut { tf!("{} déplacé", "{} moved", p) } else { tf!("{} copié", "{} copied", p) };
                    server.files_op(op, &c.root, p, Some(root), Some(&st.path), msg);
                }
                if c.cut {
                    st.clip = None;
                }
            }
        }
        Do::Delete => {
            for it in &selected {
                server.files_op(FileOp::Delete, &it.root, &it.path, None, None, tf!("{} mis à la corbeille", "{} moved to trash", it.entry.name));
            }
            st.selected.clear();
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
    }
}

/// Envoie le renommage validé dans `name_cell`.
fn commit_rename(st: &mut State, server: &Server, items: &[Item]) {
    if std::mem::take(&mut st.commit)
        && let Some((old, new)) = st.renaming.take()
        && let Some(it) = items.iter().find(|i| i.entry.name == old && st.selected.contains(&i.key()))
    {
        server.files_op(FileOp::Rename, &it.root, &it.path, None, Some(&new), tf!("{} renommé en {}", "{} renamed to {}", old, new));
    }
}

fn navigate(st: &mut State, root: String, path: String) {
    let cur = (st.root.clone().unwrap_or_default(), st.path.clone());
    if cur != (root.clone(), path.clone()) {
        st.back.push(cur);
        st.fwd.clear();
    }
    st.root = Some(root);
    st.path = path;
    st.selected.clear();
    st.renaming = None;
    st.anchor = None;
}

fn back(st: &mut State) {
    if let Some((r, p)) = st.back.pop() {
        st.fwd.push((st.root.clone().unwrap_or_default(), st.path.clone()));
        (st.root, st.path) = (Some(r), p);
        st.selected.clear();
    }
}

fn forward(st: &mut State) {
    if let Some((r, p)) = st.fwd.pop() {
        st.back.push((st.root.clone().unwrap_or_default(), st.path.clone()));
        (st.root, st.path) = (Some(r), p);
        st.selected.clear();
    }
}

/// Crée « Nouveau dossier » (ou « Nouveau dossier (2) »…) puis passe en renommage.
fn new_folder(st: &mut State, server: &Server, root: &str) {
    let existing: Vec<String> = server.data().files.listing.as_ref().and_then(|l| l.as_ref().ok()).map(|l| l.entries.iter().map(|e| e.name.clone()).collect()).unwrap_or_default();
    let base = t!("Nouveau dossier", "New folder");
    let name = std::iter::once(base.to_string()).chain((2..).map(|i| format!("{base} ({i})"))).find(|n| !existing.contains(n)).unwrap_or_default();
    server.files_op(FileOp::Mkdir, root, &join(&st.path, &name), None, None, tf!("{} créé", "{} created", name));
    st.pending_rename = Some(name);
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
