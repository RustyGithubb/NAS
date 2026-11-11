//! Fichiers : explorateur des emplacements du serveur, à la manière de l'Explorateur Windows
//! (onglets, barre d'adresse, barre de commandes, volet de navigation, Accueil, Galerie, volet
//! Détails, liste ou galerie, tri, sélection multiple, copier/couper/coller, renommer,
//! corbeille, favoris épinglés, envoi par glisser-déposer, téléchargement) avec la recherche
//! instantanée d'Everything (index tenu par l'agent). « Ce PC » montre aussi les dossiers et les
//! disques de cet ordinateur ; glisser des éléments d'un dossier à un autre (autre onglet, volet
//! de gauche) les télécharge, les envoie ou les copie (Maj : déplace). Onglet Transferts :
//! progression des envois et des téléchargements. Sous Windows, glisser des éléments hors de la
//! fenêtre les dépose dans l'Explorateur ou sur le Bureau (voir `ole_drag`).
//!
//! Raccourcis : Entrée ouvrir · Retour arrière dossier parent · Alt+←/→ précédent/suivant ·
//! F2 renommer · Suppr corbeille · Ctrl+A tout · Ctrl+C/X/V · F5 actualiser · Ctrl+F rechercher ·
//! Ctrl+T nouvel onglet · Ctrl+W fermer l'onglet · Ctrl+Tab onglet suivant.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, CornerRadius, FontId, Key, Rect, RichText, Sense, Stroke, vec2};
use nas_proto::{Entry, FileOp};

use super::{header, unavailable};
use crate::icons::{self, Emoji};
use crate::liquid::Liquid;
use crate::prefs::{Favorite, Place, Prefs};
use crate::preview::{Preview, PreviewKind};
use crate::server::{Clip, Kind, Link, Server, Transfer};
use crate::tabs::dashboard::bytes;
use crate::{theme, widgets};

/// Délai avant de lancer la recherche pendant la frappe.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
const ROW_H: f32 = 30.0;
const CELL: egui::Vec2 = egui::Vec2::new(132.0, 132.0);
const STATUS_H: f32 = 30.0;
/// Hauteur d'une ligne du volet de navigation.
const NAV_H: f32 = 26.0;
/// Hauteur maximale de l'aperçu dans le volet Détails.
const PREVIEW_H: f32 = 260.0;
/// Largeur du volet Détails.
const DETAILS_W: f32 = 280.0;
/// Hauteur de la barre d'onglets, de la barre d'adresse et de la barre de commandes.
const CHROME_H: f32 = 132.0;
/// Page Galerie : toutes les images et vidéos (recherche dans l'index de l'agent).
/// Emplacement des éléments de ce PC (leur chemin est alors le chemin complet sur ce PC).
const LOCAL: &str = "\u{1}pc";
/// Survol d'un onglet pendant un glisser : il passe au premier plan.
const HOVER_SWITCH: Duration = Duration::from_millis(600);
const GALLERY_Q: &str = "ext:jpg;jpeg;png;gif;webp;bmp;heic;tif;tiff;psd;mp4;mov;mkv;webm;avi";

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Sort {
    #[default]
    Name,
    Modified,
    Kind,
    Size,
}

/// Éléments en cours de glisser-déposer.
struct DragItems {
    root: String,
    items: Vec<(String, bool)>,
}

/// Numéro unique de chaque onglet (identifiants egui distincts d'une fenêtre à l'autre).
static NEXT_TAB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Un onglet d'explorateur.
pub struct Tab {
    uid: u64,
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
    /// Dossier de ce PC lu : chemin, génération (voir `FilesData::local_gen`), contenu.
    local: Option<(String, u64, Result<Vec<Item>, String>)>,
    /// Dernière modification du serveur prise en compte (voir `FilesData::remote_gen`).
    remote_gen: u64,
}

impl Tab {
    fn new(place: Place) -> Tab {
        // Galerie : les plus récents d'abord.
        let (sort, desc) = if place == Place::Gallery { (Sort::Modified, true) } else { (Sort::Name, false) };
        Tab {
            uid: NEXT_TAB.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
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
            local: None,
            remote_gen: 0,
        }
    }

    fn folder(&self) -> Option<(&str, &str)> {
        match &self.place {
            Place::Folder { root, path } => Some((root, path)),
            _ => None,
        }
    }

    /// Dossier où coller ou déposer : (emplacement, chemin), `LOCAL` pour ce PC.
    fn dest(&self) -> Option<(String, String)> {
        if self.searching() {
            return None;
        }
        match &self.place {
            Place::Folder { root, path } => Some((root.clone(), path.clone())),
            Place::Local { path } if !path.is_empty() => Some((LOCAL.to_string(), path.clone())),
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
        Place::Local { path } if path.is_empty() => t!("Ce PC", "This PC").into(),
        Place::Local { path } => local_name(path),
        Place::Transfers => t!("Transferts", "Transfers").into(),
    }
}

/// Dernier élément d'un chemin de ce PC (« C: » pour un disque).
fn local_name(path: &str) -> String {
    match Path::new(path).file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => {
            let s = path.trim_end_matches(['\\', '/']);
            if s.is_empty() { path.to_string() } else { s.to_string() }
        }
    }
}

fn local_parent(path: &str) -> String {
    Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

fn place_emoji(p: &Place) -> Emoji {
    match p {
        Place::Home => Emoji::Home,
        Place::Gallery => Emoji::Image,
        Place::Folder { root, path } if path.is_empty() => icons::for_root(root),
        Place::Folder { .. } => Emoji::Folder,
        Place::Local { path } if path.is_empty() => Emoji::Computer,
        Place::Local { path } if Path::new(path).parent().is_none() => Emoji::Disc,
        Place::Local { .. } => Emoji::Folder,
        Place::Transfers => Emoji::Archive,
    }
}

/// Lieu d'un favori (dossier du serveur ou de ce PC).
fn fav_place(f: &Favorite) -> Place {
    if f.root == LOCAL { Place::Local { path: f.path.clone() } } else { Place::Folder { root: f.root.clone(), path: f.path.clone() } }
}

fn fav_label(f: &Favorite) -> String {
    if f.root == LOCAL { local_name(&f.path) } else { f.path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(&f.root).to_string() }
}

/// État de l'onglet Fichiers (gardé d'une visite à l'autre).
pub struct State {
    /// Numéro de cet explorateur (identifiants egui propres à sa fenêtre).
    uid: u64,
    tabs: Vec<Tab>,
    active: usize,
    /// Explorateur de la console (ou sa fenêtre volante) : onglets enregistrés dans les
    /// réglages. Sinon : fenêtre d'un onglet détaché.
    main: bool,
    /// Texte à placer dans le presse-papiers du système (sinon Ctrl+V n'arrive pas jusqu'à egui).
    copy_text: Option<String>,
    roots_asked: bool,
    /// Onglets repris des réglages au premier affichage.
    restored: bool,
    /// Affiché dans la fenêtre volante (sinon dans la console).
    floating: bool,
    /// Bouton « Détacher » / « Ramener » cliqué.
    toggle_float: bool,
    /// Onglet survolé pendant un glisser, et depuis quand.
    hover_tab: Option<(usize, Instant)>,
    /// Fichiers glissés hors de la console : s'ils reviennent y être déposés, on les ignore.
    outgoing: Vec<PathBuf>,
    /// Onglet en cours de glisser par son intitulé : numéro et dernière position du pointeur.
    tab_drag: Option<(usize, egui::Pos2)>,
    /// Onglet détaché pendant cette image, et position (écran) où l'ouvrir.
    detached: Option<(Tab, egui::Pos2)>,
    /// Aperçu affiché : clé et image prête à dessiner.
    preview_tex: Option<((String, String, i64), egui::TextureHandle)>,
    /// Dernier aperçu demandé (pas redemandé à chaque image).
    preview_asked: Option<(String, String, i64)>,
    /// Vignettes de la vue en grandes icônes.
    thumbs: Thumbs,
    /// Disques de ce PC, sous « Ce PC ».
    drives: Drives,
}

/// Disques de ce PC, relus dans un autre fil (un lecteur réseau peut tarder à répondre).
#[derive(Default)]
struct Drives {
    list: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
    read: Option<Instant>,
}

impl Drives {
    fn get(&mut self) -> Vec<(String, String)> {
        if self.read.is_none_or(|t| t.elapsed() >= Duration::from_secs(3)) {
            self.read = Some(Instant::now());
            let list = self.list.clone();
            std::thread::spawn(move || {
                let now = crate::drives::list();
                if let Ok(mut l) = list.lock() {
                    *l = now;
                }
            });
        }
        self.list.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

/// Vignettes : textures prêtes (`None` : image illisible) et demandes en cours.
#[derive(Default)]
struct Thumbs {
    ready: std::collections::HashMap<(String, String, i64), Option<egui::TextureHandle>>,
    asked: std::collections::HashMap<(String, String, i64), Instant>,
}

/// Vignettes demandées en même temps, au plus.
const THUMBS_AT_ONCE: usize = 6;

impl Thumbs {
    /// Vignette de l'image, si elle est prête ; sinon la demande (quelques-unes à la fois).
    fn get(&mut self, ui: &egui::Ui, server: &Server, it: &Item) -> Option<egui::TextureHandle> {
        let key = (it.root.clone(), it.path.clone(), it.entry.modified_ms);
        if let Some(t) = self.ready.get(&key) {
            return t.clone();
        }
        if let Some(img) = server.data().files.thumbs.remove(&key) {
            self.asked.remove(&key);
            if self.ready.len() > 400 {
                self.ready.clear();
            }
            let tex = img.map(|img| ui.ctx().load_texture(format!("files_thumb_{}", key.1), img, egui::TextureOptions::LINEAR));
            self.ready.insert(key, tex.clone());
            return tex;
        }
        // Demande sans réponse (agent changé entre-temps) : oubliée au bout de 20 s.
        self.asked.retain(|_, t| t.elapsed() < Duration::from_secs(20));
        if !self.asked.contains_key(&key) && self.asked.len() < THUMBS_AT_ONCE {
            server.files_thumb(&it.root, &it.path, it.entry.modified_ms, it.local());
            self.asked.insert(key, Instant::now());
        }
        ui.ctx().request_repaint_after(Duration::from_millis(200));
        None
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            uid: NEXT_TAB.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            tabs: vec![Tab::new(Place::Home)],
            active: 0,
            main: true,
            copy_text: None,
            roots_asked: false,
            restored: false,
            floating: false,
            toggle_float: false,
            hover_tab: None,
            outgoing: Vec::new(),
            tab_drag: None,
            detached: None,
            preview_tex: None,
            preview_asked: None,
            thumbs: Thumbs::default(),
            drives: Drives::default(),
        }
    }
}

/// Ce que l'explorateur demande à la fenêtre qui l'affiche.
#[derive(Default)]
pub struct Out {
    /// Détacher tout l'explorateur dans sa fenêtre (ou l'y ramener).
    pub toggle_float: bool,
    /// Un onglet détaché : explorateur de sa nouvelle fenêtre, et position (écran) du coin.
    pub detach: Option<(State, egui::Pos2)>,
}

impl State {
    /// Explorateur d'une fenêtre ne contenant que cet onglet.
    fn single(tab: Tab) -> State {
        State { tabs: vec![tab], active: 0, main: false, restored: true, ..Default::default() }
    }

    /// Reprend les onglets d'une fenêtre fermée.
    pub fn adopt(&mut self, other: State) {
        self.tabs.extend(other.tabs);
        self.activate(self.tabs.len() - 1);
    }

    /// Onglets déjà repris des réglages ?
    pub fn restored(&self) -> bool {
        self.restored
    }

    /// Lieux des onglets (enregistrés dans les réglages).
    pub fn places(&self) -> Vec<Place> {
        self.tabs.iter().map(|t| t.place.clone()).collect()
    }

    /// Titre de la fenêtre d'un onglet détaché.
    pub fn title(&self) -> String {
        self.tabs[self.active].title()
    }

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

    fn local(&self) -> bool {
        self.root == LOCAL
    }

    /// Dossier personnel ou disque de « Ce PC » : ni renommé, ni supprimé, ni déplacé.
    fn fixed(&self) -> bool {
        self.local() && self.parent.is_empty()
    }

    fn emoji(&self) -> Emoji {
        if self.fixed() && Path::new(&self.path).parent().is_none() { Emoji::Disc } else { icons::kind(&self.entry.name, self.entry.is_dir).0 }
    }

    /// Dossier qui le contient, pour l'affichage.
    fn location(&self) -> String {
        if self.local() {
            return if self.parent.is_empty() { t!("Ce PC", "This PC").into() } else { self.parent.clone() };
        }
        [self.root.as_str()].into_iter().chain(self.parent.split('/').filter(|p| !p.is_empty())).collect::<Vec<_>>().join(" › ")
    }

    /// Lieu de ce dossier.
    fn place(&self) -> Place {
        if self.local() { Place::Local { path: self.path.clone() } } else { Place::Folder { root: self.root.clone(), path: self.path.clone() } }
    }
}

/// Contenu d'un dossier de ce PC (vide = « Ce PC »), sans les fichiers cachés.
fn read_local(path: &str) -> Result<Vec<Item>, String> {
    if path.is_empty() {
        return Ok(this_pc());
    }
    let rd = std::fs::read_dir(path).map_err(|e| e.to_string())?;
    Ok(rd
        .flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            if hidden(&name, &md) {
                return None;
            }
            let modified_ms = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0);
            let entry = Entry { name, is_dir: md.is_dir(), size: if md.is_dir() { 0 } else { md.len() }, modified_ms };
            Some(Item { root: LOCAL.into(), path: e.path().to_string_lossy().into_owned(), parent: path.to_string(), entry })
        })
        .collect())
}

#[cfg(windows)]
fn hidden(_: &str, md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // Attributs « caché » (2) et « système » (4) de Windows.
    md.file_attributes() & 0x6 != 0
}

#[cfg(not(windows))]
fn hidden(name: &str, _: &std::fs::Metadata) -> bool {
    name.starts_with('.')
}

/// « Ce PC » : dossiers personnels, puis disques.
fn this_pc() -> Vec<Item> {
    let dir = |name: &str, p: &Path| Item {
        root: LOCAL.into(),
        path: p.to_string_lossy().into_owned(),
        parent: String::new(),
        entry: Entry { name: name.to_string(), is_dir: true, size: 0, modified_ms: 0 },
    };
    let mut v = Vec::new();
    if let Some(u) = directories::UserDirs::new() {
        for (n, p) in [
            (t!("Bureau", "Desktop"), u.desktop_dir()),
            (t!("Documents", "Documents"), u.document_dir()),
            (t!("Téléchargements", "Downloads"), u.download_dir()),
            (t!("Images", "Pictures"), u.picture_dir()),
            (t!("Vidéos", "Videos"), u.video_dir()),
            (t!("Musique", "Music"), u.audio_dir()),
        ] {
            if let Some(p) = p.filter(|p| p.is_dir()) {
                v.push(dir(n, p));
            }
        }
    }
    for (name, root) in crate::drives::list() {
        v.push(dir(&name, Path::new(&root)));
    }
    v
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
    Download(Vec<Item>),
    /// Éléments déposés dans un dossier (emplacement, chemin), avec les touches tenues.
    Drop(std::sync::Arc<DragItems>, String, String, egui::Modifiers),
    /// Onglet détaché, lâché à cette position (dans la fenêtre).
    Detach(usize, egui::Pos2),
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
    ToggleFloat,
}

/// `floating` : dans une fenêtre à part (pas d'en-tête de page).
pub fn show(ui: &mut egui::Ui, st: &mut State, server: &Server, link: &Link, addr: &str, prefs: &mut Prefs, floating: bool) -> Out {
    st.floating = floating;
    let (overline, title) = (t!("Stockage", "Storage"), t!("Fichiers", "Files"));
    if unavailable(ui, overline, title, link, addr) {
        return Out::default();
    }
    if !st.roots_asked {
        st.roots_asked = true;
        server.files_roots();
    }
    if !st.restored && st.main {
        st.restored = true;
        if !prefs.files_tabs.is_empty() {
            st.tabs = prefs.files_tabs.iter().cloned().map(Tab::new).collect();
        }
        st.activate(prefs.files_active);
    }
    // Contenu de l'onglet au premier plan : demandé une fois, puis après chaque modification
    // sur le serveur (faite d'ici ou d'une autre fenêtre).
    let (remote_gen, local_gen) = {
        let d = server.data();
        (d.files.remote_gen, d.files.local_gen)
    };
    let tab = st.tab();
    if tab.remote_gen != remote_gen {
        tab.remote_gen = remote_gen;
        if !matches!(tab.place, Place::Local { .. } | Place::Transfers) {
            tab.requested = false;
        }
    }
    if !tab.requested {
        tab.requested = true;
        match &tab.place {
            Place::Home => server.files_recent(),
            Place::Gallery => server.files_search(GALLERY_Q),
            Place::Folder { root, path } => server.files_list(root, path),
            Place::Local { .. } => tab.local = None,
            Place::Transfers => {}
        }
    }
    // Dossier de ce PC : relu à l'arrivée, à l'actualisation et après une copie.
    if let Place::Local { path } = &tab.place
        && tab.local.as_ref().is_none_or(|(p, g, _)| p != path || *g != local_gen)
    {
        tab.local = Some((path.clone(), local_gen, read_local(path)));
    }
    // Recherche instantanée : lancée peu après la dernière frappe (sur ce PC : filtre du dossier).
    let searching = tab.searching();
    let remote = !matches!(tab.place, Place::Local { .. } | Place::Transfers);
    if searching && !remote {
        tab.sent_query = tab.query.clone();
    } else if searching && tab.query != tab.sent_query && tab.query_edited.is_none_or(|t| t.elapsed() >= SEARCH_DEBOUNCE) {
        server.files_search(tab.query.trim());
        tab.sent_query = tab.query.clone();
    } else if searching && tab.query != tab.sent_query {
        ui.ctx().request_repaint_after(SEARCH_DEBOUNCE);
    }

    // Données de cet onglet seulement (d'autres fenêtres peuvent afficher d'autres dossiers).
    let (roots, listing, loading, search, recent, last, transfers) = {
        let d = server.data();
        let f = &d.files;
        let key = tab.folder().map(|(r, p)| (r.to_string(), p.to_string()));
        let listing = key.as_ref().and_then(|k| f.listings.get(k).cloned());
        let loading = key.as_ref().is_some_and(|k| f.loading.contains(k));
        let q = if searching { tab.sent_query.trim().to_string() } else { GALLERY_Q.to_string() };
        let search = f.searches.get(&q).cloned().map(|r| (q, r));
        (f.roots.clone().unwrap_or_default(), listing, loading, search, f.recent.clone(), f.last.clone(), f.transfers.clone())
    };

    let mut items = items_of(tab, &listing, &search, &recent);
    let unsorted = tab.place == Place::Home || tab.place == (Place::Local { path: String::new() });
    if !(unsorted && !searching) {
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
    if !floating {
        header(ui, overline, title, &badge, false);
    }

    let mut todo: Vec<Do> = Vec::new();
    keyboard(ui, st, &items, &mut todo);

    // Fichiers glissés depuis Windows : envoyés dans le dossier affiché.
    let tab = &st.tabs[st.active];
    let drop_target = tab.dest();
    let dropped: Vec<PathBuf> =
        ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty() && !st.outgoing.contains(p)).collect());
    if !dropped.is_empty()
        && let Some((r, p)) = &drop_target
    {
        if r == LOCAL { server.local_copy(dropped, PathBuf::from(p), false) } else { server.upload(r, p, dropped) }
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
        0 => side(ui, st, prefs, &roots, transfers.iter().filter(|t| !t.done).count(), &mut todo),
        1 => {
            let content = ui.max_rect();
            main_view(ui, st, server, prefs, &roots, &items, &listing, &search, loading, &transfers, &mut todo);
            status_bar(ui, st, server, &items, &last, &transfers, &mut todo);
            if hovering_files && let Some((r, p)) = &drop_target {
                let p_ = ui.painter();
                p_.rect_filled(content.expand(8.0), CornerRadius::same(10), theme::overlay(30));
                p_.rect_stroke(content.expand(8.0), CornerRadius::same(10), Stroke::new(2.0, theme::TEXT_MUTED()), egui::StrokeKind::Inside);
                p_.text(
                    content.center(),
                    Align2::CENTER_CENTER,
                    tf!(
                        "Déposer pour copier dans « {} »",
                        "Drop to copy into “{}”",
                        if r == LOCAL {
                            local_name(p)
                        } else if p.is_empty() {
                            r.clone()
                        } else {
                            p.clone()
                        }
                    ),
                    FontId::proportional(18.0),
                    theme::TEXT(),
                );
            }
        }
        _ => details(ui, st, server, &items, &roots),
    });

    drag_ghost(ui.ctx());
    #[cfg(windows)]
    drag_out(ui.ctx(), st, server);
    for d in todo {
        run(d, st, server, prefs, &items);
    }
    commit_rename(st.tab(), server, &items);
    if let Some(text) = st.copy_text.take() {
        ui.ctx().copy_text(text);
    }
    // Onglets ouverts retrouvés au prochain lancement (ceux des fenêtres détachées sont
    // ajoutés par la console).
    if st.main {
        let places = st.places();
        if prefs.files_tabs != places {
            prefs.files_tabs = places;
        }
        prefs.files_active = st.active;
    }
    // Onglet détaché : sa fenêtre s'ouvre là où il a été lâché.
    let detach = st.detached.take().map(|(tab, pos)| {
        let origin = ui.ctx().input(|i| i.viewport().inner_rect.map(|r| r.min)).unwrap_or_default();
        (State::single(tab), origin + pos.to_vec2())
    });
    Out { toggle_float: std::mem::take(&mut st.toggle_float), detach }
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
    let local = || match &tab.local {
        Some((_, _, Ok(v))) => v.clone(),
        _ => Vec::new(),
    };
    if tab.searching() && matches!(tab.place, Place::Local { .. }) {
        let q = tab.query.trim().to_lowercase();
        return local().into_iter().filter(|i| i.entry.name.to_lowercase().contains(&q)).collect();
    }
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
        Place::Local { .. } => local(),
        Place::Transfers => Vec::new(),
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
fn tab_bar(ui: &mut egui::Ui, st: &mut State, todo: &mut Vec<Do>) {
    let dragging = egui::DragAndDrop::has_payload_of_type::<DragItems>(ui.ctx());
    let mut hovered_tab = None;
    let last_drag = st.tab_drag;
    let mut tab_drag = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let n = st.tabs.len();
        let w = ((ui.available_width() - 40.0) / n as f32 - 4.0).clamp(90.0, 220.0);
        for (i, t) in st.tabs.iter().enumerate() {
            let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
            let resp = ui.interact(r, egui::Id::unique(("files_tab", t.uid)), Sense::click_and_drag());
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
            let close_resp = ui.interact(close, egui::Id::unique(("files_tab_close", t.uid)), Sense::click());
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
            // Intitulé tiré hors de la barre d'onglets : l'onglet part dans sa propre fenêtre.
            let away = |p: egui::Pos2| (p.y - r.center().y).abs() > 45.0;
            if n > 1 && resp.dragged() {
                let pos = resp.interact_pointer_pos().or(last_drag.map(|d| d.1));
                if let Some(pos) = pos {
                    tab_drag = Some((i, pos));
                    if away(pos) {
                        ghost(ui.ctx(), pos, &tf!("Ouvrir « {} » dans une fenêtre", "Open “{}” in a window", t.title()));
                    }
                }
            }
            if n > 1
                && resp.drag_stopped()
                && let Some(pos) = resp.interact_pointer_pos().filter(|p| ui.ctx().input(|i| i.viewport_rect().contains(*p))).or(last_drag.map(|d| d.1))
                && away(pos)
            {
                todo.push(Do::Detach(i, pos));
            }
            // Glisser : déposé sur l'onglet = dans son dossier ; survolé un instant = au premier plan.
            if dragging && resp.contains_pointer() {
                hovered_tab = Some(i);
                if t.dest().is_some() {
                    ui.painter().rect_stroke(r, CornerRadius::same(8), Stroke::new(1.5, theme::TEXT_MUTED()), egui::StrokeKind::Inside);
                }
                if i != st.active && st.hover_tab.is_some_and(|(h, since)| h == i && since.elapsed() >= HOVER_SWITCH) {
                    todo.push(Do::SelectTab(i));
                }
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
            if let Some(items) = resp.dnd_release_payload::<DragItems>()
                && let Some((root, path)) = t.dest()
            {
                todo.push(Do::Drop(items, root, path, ui.input(|i| i.modifiers)));
            }
        }
        if ui
            .add(egui::Button::new(RichText::new("+").size(16.0)).frame(false).min_size(vec2(28.0, 28.0)))
            .on_hover_text(t!("Nouvel onglet (Ctrl+T)", "New tab (Ctrl+T)"))
            .clicked()
        {
            todo.push(Do::NewTab(Place::Home));
        }
        // À droite : détacher l'explorateur dans sa fenêtre, ou l'y ramener.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !st.main {
                return;
            }
            let tip = if st.floating { t!("Ramener dans la console", "Bring back into the console") } else { t!("Détacher dans une fenêtre", "Detach into a window") };
            if ui.add(egui::Button::new(RichText::new("⧉").size(16.0)).frame(false).min_size(vec2(30.0, 28.0))).on_hover_text(tip).clicked() {
                todo.push(Do::ToggleFloat);
            }
        });
    });
    st.tab_drag = tab_drag;
    st.hover_tab = match (hovered_tab, st.hover_tab) {
        (Some(i), Some((h, since))) if h == i => Some((h, since)),
        (Some(i), _) => Some((i, Instant::now())),
        (None, _) => None,
    };
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
            Place::Local { path } if !path.is_empty() => Some(Place::Local { path: local_parent(path) }),
            Place::Folder { .. } | Place::Gallery | Place::Local { .. } | Place::Transfers => Some(Place::Home),
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
                Place::Local { path } => {
                    sep(ui);
                    if crumb(ui, t!("Ce PC", "This PC")) {
                        todo.push(Do::Go(Place::Local { path: String::new() }));
                    }
                    let levels: Vec<&Path> = Path::new(path).ancestors().filter(|a| !a.as_os_str().is_empty()).collect();
                    for a in levels.into_iter().rev() {
                        let a = a.to_string_lossy().into_owned();
                        sep(ui);
                        if crumb(ui, &local_name(&a)) {
                            todo.push(Do::Go(Place::Local { path: a }));
                        }
                    }
                }
                Place::Transfers => {
                    sep(ui);
                    crumb(ui, t!("Transferts", "Transfers"));
                }
            }
        });

        // Recherche : dans le dossier affiché, ou partout depuis l'Accueil et la Galerie.
        let scope = match &tab.place {
            Place::Folder { .. } | Place::Local { .. } => tf!("Rechercher dans : {}", "Search in: {}", place_title(&tab.place)),
            _ => t!("Rechercher partout", "Search everywhere").to_string(),
        };
        let resp = ui
            .add(egui::TextEdit::singleline(&mut tab.query).id(egui::Id::unique(("files_search", tab.uid))).hint_text(scope).desired_width(search_w).margin(vec2(10.0, 8.0)))
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
    let tab = st.tab();
    let in_folder = tab.dest().is_some();
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
                if tab.folder().is_some() && ui.button(t!("⬆  Envoyer des fichiers…", "⬆  Upload files…")).clicked() {
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
fn side(ui: &mut egui::Ui, st: &mut State, prefs: &Prefs, roots: &[nas_proto::Root], running: usize, todo: &mut Vec<Do>) {
    let drives = st.drives.get();
    let tab = &st.tabs[st.active];
    let here = if tab.searching() { None } else { Some(&tab.place) };
    // Lignes serrées, comme le volet de navigation de l'Explorateur.
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut hl = Liquid::begin(ui, ("files_nav", st.uid));
    nav_place(ui, &mut hl, here, todo, t!("Accueil", "Home"), Place::Home, Emoji::Home, false);
    nav_place(ui, &mut hl, here, todo, t!("Galerie", "Gallery"), Place::Gallery, Emoji::Image, false);
    nav_sep(ui);
    if prefs.favorites.is_empty() {
        ui.label(RichText::new(t!("Clic droit sur un dossier › Épingler", "Right-click a folder › Pin")).small().color(theme::TEXT_FAINT()));
        ui.add_space(4.0);
    }
    for (i, f) in prefs.favorites.iter().enumerate() {
        let place = fav_place(f);
        let e = if f.path.is_empty() { icons::for_root(&f.root) } else { Emoji::Folder };
        let tip = if f.root == LOCAL { f.path.clone() } else { format!("{} › {}", f.root, f.path) };
        let r = nav_place(ui, &mut hl, here, todo, &fav_label(f), place.clone(), e, true).on_hover_text(tip);
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
    nav_sep(ui);
    nav_place(ui, &mut hl, here, todo, t!("Ce PC", "This PC"), Place::Local { path: String::new() }, Emoji::Computer, false);
    // Disques, en retrait sous « Ce PC » (liste relue en arrière-plan toutes les 3 s).
    for (name, root) in drives {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(16.0);
            nav_place(ui, &mut hl, here, todo, &name, Place::Local { path: root.clone() }, Emoji::Disc, false).on_hover_text(&root);
        });
    }
    // Transferts : nombre en cours à droite.
    let lead = |p: &egui::Painter, c: egui::Pos2| icons::paint(p, Rect::from_center_size(c, vec2(18.0, 18.0)), Emoji::Archive);
    let r = widgets::nav_row_h(
        ui,
        t!("Transferts", "Transfers"),
        (running > 0).then_some(running),
        here == Some(&Place::Transfers),
        widgets::Tone::Normal,
        &mut hl,
        Some(&lead),
        NAV_H,
    );
    if r.clicked() {
        todo.push(Do::Go(Place::Transfers));
    }
    widgets::nav_end(ui, hl);
}

/// Ligne du volet de navigation : clic = y aller, clic du milieu = nouvel onglet (comme dans
/// l'Explorateur), épingle à droite pour les favoris.
#[allow(clippy::too_many_arguments)]
fn nav_place(ui: &mut egui::Ui, hl: &mut Liquid, here: Option<&Place>, todo: &mut Vec<Do>, text: &str, place: Place, e: Emoji, pin: bool) -> egui::Response {
    let lead = move |p: &egui::Painter, c: egui::Pos2| icons::paint(p, Rect::from_center_size(c, vec2(18.0, 18.0)), e);
    let r = widgets::nav_row_h(ui, text, None, here == Some(&place), widgets::Tone::Normal, hl, Some(&lead), NAV_H);
    if pin {
        ui.painter().text(egui::pos2(r.rect.right() - 12.0, r.rect.center().y), Align2::CENTER_CENTER, "📌", FontId::proportional(11.0), theme::TEXT_FAINT());
    }
    if r.clicked() {
        todo.push(Do::Go(place.clone()));
    }
    if r.middle_clicked() {
        todo.push(Do::NewTab(place.clone()));
    }
    // Dossier du volet de gauche : on peut y déposer des éléments glissés.
    let dest = match &place {
        Place::Folder { root, path } => Some((root.clone(), path.clone())),
        Place::Local { path } if !path.is_empty() => Some((LOCAL.to_string(), path.clone())),
        _ => None,
    };
    if let Some((root, path)) = dest {
        if r.dnd_hover_payload::<DragItems>().is_some() {
            ui.painter().rect_stroke(r.rect, CornerRadius::same(8), Stroke::new(1.5, theme::TEXT_MUTED()), egui::StrokeKind::Inside);
        }
        if let Some(items) = r.dnd_release_payload::<DragItems>() {
            todo.push(Do::Drop(items, root, path, ui.input(|i| i.modifiers)));
        }
    }
    r
}

fn nav_sep(ui: &mut egui::Ui) {
    ui.add_space(10.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, theme::BORDER()));
    ui.add_space(10.0);
}

// --- Contenu ----------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn main_view(
    ui: &mut egui::Ui,
    st: &mut State,
    server: &Server,
    prefs: &Prefs,
    roots: &[nas_proto::Root],
    items: &[Item],
    listing: &Option<Result<nas_proto::Listing, String>>,
    search: &Option<(String, Result<nas_proto::SearchResult, String>)>,
    loading: bool,
    transfers: &[Transfer],
    todo: &mut Vec<Do>,
) {
    // Contenu + barre d'état = hauteur du panneau, au pixel près (même bas que les autres panneaux).
    let h = (ui.available_height() - STATUS_H - ui.spacing().item_spacing.y).max(40.0);
    let State { tabs, active, thumbs, .. } = st;
    let tab = &mut tabs[*active];
    let searching = tab.searching();
    ui.allocate_ui(vec2(ui.available_width(), h), |ui| {
        ui.set_min_size(vec2(ui.available_width(), h));
        if !searching && tab.place == Place::Home {
            home(ui, tab, prefs, roots, items, todo);
            return;
        }
        if tab.place == Place::Transfers {
            transfers_page(ui, server, transfers, todo);
            return;
        }
        let view = ui.max_rect();
        let gallery_page = !searching && tab.place == Place::Gallery;
        let is_local = matches!(tab.place, Place::Local { .. });
        let waiting = |q: &str| !matches!(search, Some((k, _)) if k == q);
        let error = match (&tab.place, &tab.local) {
            (Place::Local { .. }, Some((_, _, Err(e)))) => Some(e.clone()),
            (Place::Folder { .. }, _) => listing.as_ref().and_then(|l| l.as_ref().err().cloned()),
            _ => None,
        };
        if searching && is_local {
            if items.is_empty() {
                ui.add_space(40.0);
                widgets::empty_state(ui, t!("Aucun résultat", "No result"), t!("Aucun nom de ce dossier ne contient ce texte", "No name in this folder contains this text"));
            }
        } else if searching {
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
        } else if loading && !is_local && items.is_empty() {
            ui.vertical_centered(|ui| ui.spinner());
        } else if let Some(e) = &error {
            ui.add_space(40.0);
            widgets::empty_state(ui, t!("Dossier inaccessible", "Folder unavailable"), e);
        } else if items.is_empty() {
            ui.add_space(40.0);
            let hint = if is_local {
                t!("Glisse des fichiers ici pour les copier dans ce dossier", "Drop files here to copy them into this folder")
            } else {
                t!("Glisse des fichiers ici pour les envoyer sur le serveur", "Drop files here to upload them to the server")
            };
            widgets::empty_state(ui, t!("Dossier vide", "Empty folder"), hint);
        }
        if !items.is_empty() {
            // Galerie : toujours en grandes icônes, avec le dossier de chaque élément.
            let wide = searching || gallery_page;
            if prefs.files_gallery || gallery_page { gallery(ui, tab, thumbs, server, items, wide, todo) } else { list(ui, tab, items, wide, todo) }
        }
        // Déposé ailleurs que sur un dossier : dans le dossier affiché.
        if let Some((root, path)) = tab.dest() {
            let r = ui.interact(view, egui::Id::unique(("files_view_drop", tab.uid)), Sense::hover());
            if r.dnd_hover_payload::<DragItems>().is_some() {
                ui.painter().rect_stroke(view, CornerRadius::same(8), Stroke::new(1.5, theme::TEXT_FAINT()), egui::StrokeKind::Inside);
            }
            if let Some(items) = r.dnd_release_payload::<DragItems>() {
                todo.push(Do::Drop(items, root, path, ui.input(|i| i.modifiers)));
            }
        }
    });
}

/// Page Accueil : Accès rapide (favoris et emplacements en grandes icônes), puis fichiers récents.
fn home(ui: &mut egui::Ui, tab: &mut Tab, prefs: &Prefs, roots: &[nas_proto::Root], items: &[Item], todo: &mut Vec<Do>) {
    egui::ScrollArea::vertical().id_salt(("files_home", tab.uid)).auto_shrink([false, false]).show(ui, |ui| {
        section_title(ui, t!("Accès rapide", "Quick access"));
        let mut quick: Vec<(String, String, Place, Emoji)> = prefs
            .favorites
            .iter()
            .map(|f| {
                let e = if f.path.is_empty() { icons::for_root(&f.root) } else { Emoji::Folder };
                let sub = if f.root == LOCAL { t!("📌 Ce PC", "📌 This PC").to_string() } else { format!("📌 {}", f.root) };
                (fav_label(f), sub, fav_place(f), e)
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

fn details(ui: &mut egui::Ui, st: &mut State, server: &Server, items: &[Item], roots: &[nas_proto::Root]) {
    // Contenu plus haut que le volet (grand aperçu) : il défile, le bas reste aligné.
    let h = ui.available_height();
    egui::ScrollArea::vertical().id_salt(("files_details", st.uid)).max_height(h).auto_shrink([false, false]).show(ui, |ui| details_body(ui, st, server, items, roots));
}

fn details_body(ui: &mut egui::Ui, st: &mut State, server: &Server, items: &[Item], roots: &[nas_proto::Root]) {
    let State { tabs, active, preview_tex, preview_asked, .. } = st;
    let tab = &tabs[*active];
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
            let kind = icons::kind(&one.entry.name, one.entry.is_dir).1;
            // Aperçu à la place de l'icône quand le fichier peut être lu.
            let shown = match crate::preview::kind_of(&one.entry.name, one.entry.size).filter(|_| !one.entry.is_dir) {
                Some(k) => preview(ui, server, preview_tex, preview_asked, one, k),
                None => false,
            };
            if !shown {
                big(ui, one.emoji());
            }
            title(ui, &one.entry.name);
            field(ui, t!("Type", "Type"), kind);
            if !one.entry.is_dir {
                field(ui, t!("Taille", "Size"), &bytes(one.entry.size));
            }
            field(ui, t!("Modifié le", "Modified"), &date(one.entry.modified_ms));
            field(ui, t!("Emplacement", "Location"), &one.location());
        }
        [] => {
            big(ui, place_emoji(&tab.place));
            title(ui, &tab.title());
            let n = items.len();
            if tab.place == Place::Transfers {
                return;
            }
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

/// Aperçu du fichier : demandé une fois, puis dessiné. Renvoie `false` s'il n'y en a pas
/// (illisible) : l'icône est alors affichée.
fn preview(
    ui: &mut egui::Ui,
    server: &Server,
    tex: &mut Option<((String, String, i64), egui::TextureHandle)>,
    asked: &mut Option<(String, String, i64)>,
    it: &Item,
    kind: PreviewKind,
) -> bool {
    let key = (it.root.clone(), it.path.clone(), it.entry.modified_ms);
    ui.add_space(12.0);
    let w = ui.available_width();
    if let Some((k, t)) = tex.as_ref()
        && *k == key
    {
        let size = t.size_vec2();
        let scale = (w / size.x).min(PREVIEW_H / size.y).min(2.0);
        let (r, _) = ui.allocate_exact_size(vec2(w, size.y * scale), Sense::hover());
        ui.painter().image(t.id(), Rect::from_center_size(r.center(), size * scale), Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        ui.add_space(12.0);
        return true;
    }
    enum Got {
        Image(egui::ColorImage),
        Text(String, bool),
        Failed,
    }
    let got = server.data().files.previews.get(&key).map(|r| match r {
        Ok(Preview::Image(img)) => Got::Image(img.clone()),
        Ok(Preview::Text(t, cut)) => Got::Text(t.clone(), *cut),
        Err(_) => Got::Failed,
    });
    match got {
        None => {
            if asked.as_ref() != Some(&key) {
                *asked = Some(key);
                server.files_preview(&it.root, &it.path, it.entry.modified_ms, it.local(), kind);
            }
            let (r, _) = ui.allocate_exact_size(vec2(w, 112.0), Sense::hover());
            ui.put(Rect::from_center_size(r.center(), vec2(24.0, 24.0)), egui::Spinner::new());
            ui.add_space(12.0);
            true
        }
        Some(Got::Failed) => false,
        Some(Got::Image(img)) => {
            *tex = Some((key, ui.ctx().load_texture("files_preview", img, egui::TextureOptions::LINEAR)));
            ui.ctx().request_repaint();
            true
        }
        Some(Got::Text(text, cut)) => {
            egui::Frame::new().fill(theme::BG()).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::both().id_salt(("files_preview", &key.0, &key.1)).max_height(PREVIEW_H).auto_shrink([false, true]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(text).monospace().size(11.5).color(theme::TEXT_MUTED())).extend());
                });
            });
            if cut {
                ui.label(RichText::new(t!("Début du fichier (64 Ko)", "Start of the file (64 KB)")).small().color(theme::TEXT_FAINT()));
            }
            ui.add_space(12.0);
            true
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
        let resp = ui.interact(cell, egui::Id::unique(("files_col", st.uid, i)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let arrow = if st.sort == *sort { if st.desc { " ▼" } else { " ▲" } } else { "" };
        let color = if st.sort == *sort || resp.hovered() { theme::TEXT() } else { theme::TEXT_MUTED() };
        ui.painter().text(egui::pos2(*x, hr.center().y), *align, format!("{label}{arrow}"), FontId::proportional(12.5), color);
        if resp.clicked() {
            if st.sort == *sort { st.desc = !st.desc } else { (st.sort, st.desc) = (*sort, false) }
        }
    }
    ui.painter().hline(hr.x_range(), hr.bottom(), Stroke::new(1.0, theme::BORDER()));
    ui.add_space(4.0);

    egui::ScrollArea::vertical().id_salt(("files_list", st.uid)).auto_shrink([false, false]).show_rows(ui, ROW_H, items.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let it = &items[i];
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
            let sel = st.selected.contains(&it.key());
            let p = ui.painter();
            if sel {
                p.rect_filled(r, CornerRadius::same(6), theme::gray(0x2A));
            } else if resp.hovered() {
                p.rect_filled(r, CornerRadius::same(6), theme::overlay(6));
            }
            let kind = icons::kind(&it.entry.name, it.entry.is_dir).1;
            icons::paint(p, Rect::from_center_size(egui::pos2(r.left() + 18.0, r.center().y), vec2(18.0, 18.0)), it.emoji());
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
            let third = if searching { it.location() } else { kind.to_string() };
            if kind_w > 0.0 {
                let mut job = egui::text::LayoutJob::simple_singleline(third, small.clone(), theme::TEXT_MUTED());
                job.wrap = egui::text::TextWrapping::truncate_at_width(kind_w - 12.0);
                let g = p.layout_job(job);
                p.galley(egui::pos2(cols[2].2, r.center().y - g.size().y / 2.0), g, theme::TEXT_MUTED());
            }
            if !it.entry.is_dir {
                p.text(egui::pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, bytes(it.entry.size), small, theme::TEXT_MUTED());
            }
            interact(ui, st, items, i, &resp, r, searching, todo);
        }
    });
}

// --- Vue en galerie ----------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn gallery(ui: &mut egui::Ui, st: &mut Tab, thumbs: &mut Thumbs, server: &Server, items: &[Item], searching: bool, todo: &mut Vec<Do>) {
    let w = ui.available_width();
    let cols = ((w + 8.0) / (CELL.x + 8.0)).floor().max(1.0) as usize;
    let gap = (w - cols as f32 * CELL.x) / (cols as f32 - 1.0).max(1.0);
    let rows = items.len().div_ceil(cols);
    egui::ScrollArea::vertical().id_salt(("files_gallery", st.uid)).auto_shrink([false, false]).show_rows(ui, CELL.y + 8.0, rows, |ui, range| {
        for row in range {
            let (line, _) = ui.allocate_exact_size(vec2(w, CELL.y + 8.0), Sense::hover());
            for c in 0..cols {
                let i = row * cols + c;
                let Some(it) = items.get(i) else { break };
                let r = Rect::from_min_size(line.min + vec2(c as f32 * (CELL.x + gap), 0.0), CELL);
                let resp = ui.interact(r, egui::Id::unique(("files_cell", st.uid, i)), Sense::click_and_drag());
                let sel = st.selected.contains(&it.key());
                let p = ui.painter();
                if sel {
                    p.rect_filled(r, CornerRadius::same(10), theme::gray(0x2A));
                } else if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(10), theme::overlay(6));
                }
                // Image : sa vignette à la place de l'icône (dès qu'elle est arrivée).
                let thumb = if !it.entry.is_dir && crate::preview::kind_of(&it.entry.name, it.entry.size) == Some(PreviewKind::Image) { thumbs.get(ui, server, it) } else { None };
                let p = ui.painter();
                match thumb {
                    Some(t) => {
                        let area = vec2(CELL.x - 16.0, 72.0);
                        let size = t.size_vec2();
                        let scale = (area.x / size.x).min(area.y / size.y);
                        let at = Rect::from_center_size(egui::pos2(r.center().x, r.top() + 44.0), size * scale);
                        p.image(t.id(), at, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                    }
                    None => icons::paint(p, Rect::from_center_size(egui::pos2(r.center().x, r.top() + 44.0), vec2(56.0, 56.0)), it.emoji()),
                }
                let name_rect = Rect::from_min_max(egui::pos2(r.left() + 8.0, r.top() + 80.0), egui::pos2(r.right() - 8.0, r.bottom() - 6.0));
                if !name_cell(ui, st, it, name_rect) {
                    // Nom sur deux lignes au plus, centré.
                    let mut job = egui::text::LayoutJob::single_section(it.entry.name.clone(), egui::TextFormat::simple(FontId::proportional(13.0), theme::TEXT()));
                    job.wrap = egui::text::TextWrapping { max_width: name_rect.width(), max_rows: 2, break_anywhere: true, overflow_character: Some('…') };
                    job.halign = egui::Align::Center;
                    let g = ui.painter().layout_job(job);
                    ui.painter().galley(egui::pos2(name_rect.center().x, name_rect.top()), g, theme::TEXT());
                }
                interact(ui, st, items, i, &resp, r, searching, todo);
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
    let id = egui::Id::unique(("files_rename", st.uid));
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

#[allow(clippy::too_many_arguments)]
fn interact(ui: &mut egui::Ui, st: &mut Tab, items: &[Item], i: usize, resp: &egui::Response, rect: Rect, searching: bool, todo: &mut Vec<Do>) {
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
        todo.push(Do::NewTab(it.place()));
    }
    // Glisser : la sélection (ou cet élément s'il n'en fait pas partie), sauf un disque.
    if resp.drag_started() && !(it.fixed() && Path::new(&it.path).parent().is_none()) {
        if !st.selected.contains(&it.key()) {
            st.selected = [it.key()].into();
            st.anchor = Some(i);
        }
        let picked = items.iter().filter(|x| x.root == it.root && st.selected.contains(&x.key())).map(|x| (x.path.clone(), x.entry.is_dir)).collect();
        egui::DragAndDrop::set_payload(ui.ctx(), DragItems { root: it.root.clone(), items: picked });
    }
    // Dossier : on peut y déposer (sauf s'il fait partie de ce qui est glissé).
    if it.entry.is_dir
        && let Some(p) = resp.dnd_hover_payload::<DragItems>()
        && !(p.root == it.root && p.items.iter().any(|(x, _)| *x == it.path))
    {
        ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.5, theme::TEXT()), egui::StrokeKind::Inside);
        if let Some(items) = resp.dnd_release_payload::<DragItems>() {
            let root = if it.local() { LOCAL.to_string() } else { it.root.clone() };
            todo.push(Do::Drop(items, root, it.path.clone(), ui.input(|i| i.modifiers)));
        }
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
            todo.push(Do::NewTab(it.place()));
            ui.close();
        }
        if searching && !many && ui.button(t!("Ouvrir l'emplacement", "Open location")).clicked() {
            todo.push(Do::Reveal(it.clone()));
            ui.close();
        }
        if !it.local() && ui.button(t!("Télécharger…", "Download…")).clicked() {
            todo.push(Do::Download(items.iter().filter(|x| st.selected.contains(&x.key()) && !x.local()).cloned().collect()));
            ui.close();
        }
        ui.separator();
        if ui.button(t!("Copier", "Copy")).clicked() {
            todo.push(Do::Copy(false));
            ui.close();
        }
        let fixed = it.fixed();
        if !fixed && ui.button(t!("Couper", "Cut")).clicked() {
            todo.push(Do::Copy(true));
            ui.close();
        }
        if !many && !fixed && ui.button(t!("Renommer", "Rename")).clicked() {
            todo.push(Do::Rename(it.clone()));
            ui.close();
        }
        if !many && it.entry.is_dir && ui.button(t!("Épingler", "Pin")).clicked() {
            todo.push(Do::Favorite(it.root.clone(), it.path.clone()));
            ui.close();
        }
        if !fixed {
            ui.separator();
            if ui.button(RichText::new(t!("Mettre à la corbeille", "Move to trash")).color(theme::DANGER())).clicked() {
                todo.push(Do::Delete);
                ui.close();
            }
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
            let uid = st.tabs[st.active].uid;
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique(("files_search", uid))));
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
        if i.consume_key(egui::Modifiers::NONE, Key::Backspace) || i.consume_key(egui::Modifiers::ALT, Key::ArrowUp) {
            match &tab.place {
                Place::Folder { root, path } if !path.is_empty() => todo.push(Do::Go(Place::Folder { root: root.clone(), path: parent_of(path) })),
                Place::Local { path } if !path.is_empty() => todo.push(Do::Go(Place::Local { path: local_parent(path) })),
                Place::Home => {}
                _ => todo.push(Do::Go(Place::Home)),
            }
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

fn status_bar(ui: &mut egui::Ui, st: &State, server: &Server, items: &[Item], last: &Option<(bool, String)>, transfers: &[Transfer], todo: &mut Vec<Do>) {
    let tab = &st.tabs[st.active];
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), STATUS_H), Sense::hover());
    ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, theme::BORDER()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(vec2(0.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        let sel: Vec<&Item> = items.iter().filter(|i| tab.selected.contains(&i.key())).collect();
        let mut text = match (&tab.place, items.len()) {
            (Place::Transfers, _) => String::new(),
            (_, 1) => t!("1 élément", "1 item").to_string(),
            (_, n) => tf!("{} éléments", "{} items", n),
        };
        if !sel.is_empty() {
            let size: u64 = sel.iter().map(|i| i.entry.size).sum();
            text.push_str(&tf!("  ·  {} sélectionnés ({})", "  ·  {} selected ({})", sel.len(), bytes(size)));
        }
        let clip = server.data().files.clip.clone();
        if let Some(c) = &clip {
            text.push_str(&if c.cut { tf!("  ·  {} à déplacer", "  ·  {} to move", c.items.len()) } else { tf!("  ·  {} à copier", "  ·  {} to copy", c.items.len()) });
        }
        ui.label(RichText::new(text).small().color(theme::TEXT_MUTED()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let running = transfers.iter().filter(|t| !t.done).count();
            if running > 0 {
                let link = egui::Button::new(RichText::new(tf!("{} transfert(s) en cours", "{} transfer(s) running", running)).small().color(theme::LINK())).frame(false);
                if ui.add(link).on_hover_text(t!("Voir les transferts", "See transfers")).clicked() {
                    todo.push(Do::Go(Place::Transfers));
                }
                ui.spinner();
            } else if let Some((ok, msg)) = last {
                if ui.small_button("✕").clicked() {
                    server.data().files.last = None;
                }
                let t = if *ok { RichText::new(format!("✓ {msg}")).color(theme::TEXT_MUTED()) } else { RichText::new(format!("✗ {msg}")).color(theme::DANGER()) };
                ui.add(egui::Label::new(t.small()).truncate());
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
        Do::ToggleFloat => {
            st.toggle_float = true;
            return;
        }
        Do::Detach(i, pos) => {
            if st.tabs.len() > 1 && i < st.tabs.len() {
                let tab = st.tabs.remove(i);
                let next = if st.active > i || st.active == st.tabs.len() { st.active.saturating_sub(1) } else { st.active };
                st.activate(next);
                st.detached = Some((tab, pos - vec2(220.0, 24.0)));
            }
            return;
        }
        _ => {}
    }
    let State { tabs, active, copy_text, .. } = st;
    let tab = &mut tabs[*active];
    let selected: Vec<&Item> = items.iter().filter(|i| tab.selected.contains(&i.key())).collect();
    match d {
        Do::Go(p) => navigate(tab, p),
        Do::Open(it) if it.entry.is_dir => navigate(tab, it.place()),
        Do::Open(it) if it.local() => crate::server::open_local(Path::new(&it.path)),
        Do::Open(it) => server.download(&it.root, &it.path, true),
        Do::Download(list) => {
            // Windows : on choisit le dossier ; ailleurs : Téléchargements.
            let server = server.handle_pub();
            std::thread::spawn(move || {
                #[cfg(windows)]
                let dest = rfd::FileDialog::new().set_directory(crate::server::download_dir()).pick_folder();
                #[cfg(not(windows))]
                let dest = Some(crate::server::download_dir());
                if let Some(dest) = dest {
                    for it in list {
                        server.download_to(&it.root, &it.path, it.entry.is_dir, dest.clone(), false);
                    }
                }
            });
        }
        Do::Drop(d, root, path, mods) => {
            // Sur le serveur : déplacé (Ctrl : copié) ; sur ce PC : copié (Maj : déplacé) ;
            // de l'un à l'autre : téléchargé ou envoyé.
            let mv = match (d.root == LOCAL, root == LOCAL) {
                (false, false) => !mods.command,
                (true, true) => mods.shift,
                _ => false,
            };
            send(server, &d.root, &d.items, &root, &path, mv, true)
        }
        Do::Reveal(it) => {
            let place = if it.local() { Place::Local { path: it.parent.clone() } } else { Place::Folder { root: it.root.clone(), path: it.parent.clone() } };
            navigate(tab, place);
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
                let picked = selected.iter().filter(|i| i.root == first.root && !(cut && i.fixed()));
                let c = Clip { cut, root: first.root.clone(), items: picked.map(|i| (i.path.clone(), i.entry.is_dir)).collect() };
                let shown = |p: &String| if c.root == LOCAL { p.clone() } else { format!("{} › {p}", c.root) };
                *copy_text = Some(c.items.iter().map(|(p, _)| shown(p)).collect::<Vec<_>>().join("\n"));
                server.data().files.clip = Some(c);
            }
        }
        Do::Paste => {
            let clip = server.data().files.clip.clone();
            if let (Some(c), Some((root, path))) = (clip, tab.dest()) {
                send(server, &c.root, &c.items, &root, &path, c.cut, false);
                if c.cut {
                    server.data().files.clip = None;
                }
            }
        }
        Do::Delete => {
            // Ce PC : vers la Corbeille de Windows.
            let local: Vec<&&Item> = selected.iter().filter(|i| i.local() && !i.fixed()).collect();
            if !local.is_empty() {
                let r = trash::delete_all(local.iter().map(|i| &i.path)).map_err(|e| e.to_string());
                let msg = if local.len() == 1 {
                    tf!("{} mis à la corbeille", "{} moved to trash", local[0].entry.name)
                } else {
                    tf!("{} éléments mis à la corbeille", "{} items moved to trash", local.len())
                };
                local_done(server, r, msg);
            }
            for it in selected.iter().filter(|i| !i.local()) {
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
            tab.local = None;
            tab.sent_query.clear();
            server.files_roots();
        }
        Do::NewFolder => new_folder(tab, server),
        Do::NewTab(_) | Do::CloseTab(_) | Do::SelectTab(_) | Do::ToggleFloat | Do::Detach(..) => {}
    }
}

/// Envoie le renommage validé dans `name_cell`.
fn commit_rename(st: &mut Tab, server: &Server, items: &[Item]) {
    if std::mem::take(&mut st.commit)
        && let Some((old, new)) = st.renaming.take()
        && let Some(it) = items.iter().find(|i| i.entry.name == old && st.selected.contains(&i.key()))
    {
        if it.local() {
            let to = Path::new(&it.parent).join(&new);
            let r = if to.exists() { Err(t!("ce nom existe déjà", "this name already exists").to_string()) } else { std::fs::rename(&it.path, &to).map_err(|e| e.to_string()) };
            local_done(server, r, tf!("{} renommé en {}", "{} renamed to {}", old, new));
            return;
        }
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
    let base = t!("Nouveau dossier", "New folder");
    if let Place::Local { path } = &tab.place
        && !path.is_empty()
    {
        let dir = crate::server::free_local_name(Path::new(path), base);
        let name = local_name(&dir.to_string_lossy());
        let r = std::fs::create_dir(&dir).map_err(|e| e.to_string());
        local_done(server, r, tf!("{} créé", "{} created", name));
        tab.pending_rename = Some(name);
        return;
    }
    let Some((root, path)) = tab.folder() else { return };
    let (root, path) = (root.to_string(), path.to_string());
    let key = (root.clone(), path.clone());
    let existing: Vec<String> =
        server.data().files.listings.get(&key).and_then(|l| l.as_ref().ok()).map(|l| l.entries.iter().map(|e| e.name.clone()).collect()).unwrap_or_default();
    let name = std::iter::once(base.to_string()).chain((2..).map(|i| format!("{base} ({i})"))).find(|n| !existing.contains(n)).unwrap_or_default();
    server.files_op(FileOp::Mkdir, &root, &join(&path, &name), None, None, tf!("{} créé", "{} created", name));
    tab.pending_rename = Some(name);
}

/// Opération faite sur ce PC : message dans la barre d'état, dossiers affichés relus.
fn local_done(server: &Server, r: Result<(), String>, ok_msg: String) {
    let mut d = server.data();
    d.files.last = Some(match r {
        Ok(()) => (true, ok_msg),
        Err(e) => (false, e),
    });
    d.files.local_gen += 1;
}

/// Copie (ou déplace) des éléments vers un dossier : selon les deux côtés, copie sur le serveur,
/// téléchargement, envoi ou copie sur ce PC. `drag` : glissé-déposé (rien à faire dans son propre
/// dossier ; un collage y fait une copie).
fn send(server: &Server, src: &str, items: &[(String, bool)], dest_root: &str, dest: &str, mv: bool, drag: bool) {
    let from_pc = src == LOCAL;
    let skip = |p: &str| {
        if src != dest_root {
            return false;
        }
        // Un dossier dans lui-même, ou déposé là où il est déjà.
        let (inside, parent) = if from_pc { (Path::new(dest).starts_with(p), local_parent(p)) } else { (dest == p || dest.starts_with(&format!("{p}/")), parent_of(p)) };
        inside || (drag || mv) && parent == dest
    };
    let items: Vec<&(String, bool)> = items.iter().filter(|(p, _)| !skip(p)).collect();
    match (from_pc, dest_root == LOCAL) {
        (false, false) => {
            let op = if mv { FileOp::Move } else { FileOp::Copy };
            for (p, _) in items {
                let msg = if mv { tf!("{} déplacé", "{} moved", p) } else { tf!("{} copié", "{} copied", p) };
                server.files_op(op, src, p, Some(dest_root), Some(dest), msg);
            }
        }
        (false, true) => {
            for (p, is_dir) in items {
                server.download_to(src, p, *is_dir, PathBuf::from(dest), false);
            }
        }
        (true, false) => server.upload(dest_root, dest, items.iter().map(|(p, _)| PathBuf::from(p)).collect()),
        (true, true) => server.local_copy(items.iter().map(|(p, _)| PathBuf::from(p)).collect(), PathBuf::from(dest), mv),
    }
}

/// Nom de ce qui est glissé, près du pointeur.
fn drag_ghost(ctx: &egui::Context) {
    let (Some(d), Some(pos)) = (egui::DragAndDrop::payload::<DragItems>(ctx), ctx.pointer_interact_pos()) else { return };
    let text = match d.items.as_slice() {
        [(p, _)] => {
            if d.root == LOCAL {
                local_name(p)
            } else {
                p.rsplit('/').next().unwrap_or(p).to_string()
            }
        }
        many => tf!("{} éléments", "{} items", many.len()),
    };
    ghost(ctx, pos, &text);
}

/// Étiquette qui suit le pointeur pendant un glisser.
fn ghost(ctx: &egui::Context, pos: egui::Pos2, text: &str) {
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("files_drag_ghost")));
    let g = p.layout_no_wrap(text.to_string(), FontId::proportional(13.0), theme::TEXT());
    let r = Rect::from_min_size(pos + vec2(16.0, 12.0), g.size() + vec2(20.0, 12.0));
    p.rect_filled(r, CornerRadius::same(6), theme::SURFACE_HI());
    p.rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, theme::BORDER()), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(10.0, 6.0), g, theme::TEXT());
}

/// Pointeur sorti de la fenêtre pendant un glisser : on passe la main au glisser-déposer de
/// Windows. Les éléments du serveur sont téléchargés tout de suite dans un dossier temporaire ;
/// l'Explorateur les copie à l'endroit du dépôt (en attendant la fin du téléchargement).
#[cfg(windows)]
fn drag_out(ctx: &egui::Context, st: &mut State, server: &Server) {
    let Some(d) = egui::DragAndDrop::payload::<DragItems>(ctx) else { return };
    let outside = ctx.input(|i| i.pointer.primary_down() && i.pointer.latest_pos().is_none_or(|p| !i.viewport_rect().contains(p)));
    if !outside {
        return;
    }
    egui::DragAndDrop::clear_payload(ctx);
    ctx.stop_dragging();
    let (paths, ids): (Vec<PathBuf>, Vec<u64>) = if d.root == LOCAL {
        (d.items.iter().map(|(p, _)| PathBuf::from(p)).collect(), Vec::new())
    } else {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_millis()).unwrap_or(0);
        let dir = std::env::temp_dir().join("NAS").join(format!("glisser-{stamp}"));
        let _ = std::fs::create_dir_all(&dir);
        d.items.iter().map(|(p, is_dir)| server.download_to(&d.root, p, *is_dir, dir.clone(), false)).unzip()
    };
    st.outgoing = paths.clone();
    let server = server.handle_pub();
    crate::ole_drag::drag_out(paths, move || {
        loop {
            if let Some(ok) = server.transfers_done(&ids) {
                return ok;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
}

// --- Transferts --------------------------------------------------------------------------

/// Envois, téléchargements et copies : progression, vitesse, temps restant, résultat.
fn transfers_page(ui: &mut egui::Ui, server: &Server, transfers: &[Transfer], todo: &mut Vec<Do>) {
    ui.horizontal(|ui| {
        let running = transfers.iter().filter(|t| !t.done).count();
        let text = if running > 0 { tf!("{} en cours", "{} running", running) } else { tf!("{} transfert(s)", "{} transfer(s)", transfers.len()) };
        ui.label(RichText::new(text).color(theme::TEXT_MUTED()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(transfers.iter().any(|t| t.done), egui::Button::new(t!("Effacer les terminés", "Clear finished"))).clicked() {
                server.clear_transfers();
            }
        });
    });
    ui.add_space(8.0);
    if transfers.is_empty() {
        ui.add_space(40.0);
        widgets::empty_state(
            ui,
            t!("Aucun transfert", "No transfer"),
            t!("Glisse des fichiers d'un dossier du serveur vers un dossier de ce PC (ou l'inverse)", "Drag files from a server folder to a folder of this PC (or back)"),
        );
        return;
    }
    let remote = |(root, path): &(String, String)| [root.as_str()].into_iter().chain(path.split('/').filter(|p| !p.is_empty())).collect::<Vec<_>>().join(" › ");
    egui::ScrollArea::vertical().id_salt("files_transfers").auto_shrink([false, false]).show(ui, |ui| {
        for t in transfers.iter().rev() {
            egui::Frame::new().fill(theme::overlay(4)).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (ir, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                    icons::paint(ui.painter(), ir, icons::kind(&t.name, t.local.is_dir()).0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&t.name).strong().color(theme::TEXT()));
                        let parent = t.local.parent().map(|p| p.display().to_string()).unwrap_or_default();
                        let route = match t.kind {
                            Kind::Download => tf!("Téléchargement · {} → {}", "Download · {} → {}", remote(&t.remote), parent),
                            Kind::Upload => tf!("Envoi · {} → {}", "Upload · {} → {}", parent, remote(&t.remote)),
                            Kind::Local => tf!("Copie sur ce PC · → {}", "Copy on this PC · → {}", parent),
                        };
                        ui.add(egui::Label::new(RichText::new(route).small().color(theme::TEXT_MUTED())).truncate());
                        let secs = t.finished.unwrap_or_else(Instant::now).duration_since(t.started).as_secs_f64();
                        let frac = if t.total > 0 {
                            t.bytes as f32 / t.total as f32
                        } else if t.done {
                            1.0
                        } else {
                            0.0
                        };
                        if !t.done {
                            ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_height(6.0));
                        }
                        let status = match (&t.error, t.done) {
                            (Some(e), _) => RichText::new(tf!("✗ {}", "✗ {}", e)).color(theme::DANGER()),
                            (None, true) => RichText::new(tf!("✓ Terminé · {} en {}", "✓ Done · {} in {}", bytes(t.total.max(t.bytes)), duration(secs))).color(theme::TEXT_MUTED()),
                            (None, false) => {
                                let speed = if secs > 0.3 { t.bytes as f64 / secs } else { 0.0 };
                                let mut s = if t.total > 0 { tf!("{} sur {}", "{} of {}", bytes(t.bytes), bytes(t.total)) } else { bytes(t.bytes) };
                                if speed > 0.0 {
                                    s.push_str(&format!(" · {}/s", bytes(speed as u64)));
                                    if t.total > t.bytes {
                                        s.push_str(&tf!(" · {} restantes", " · {} left", duration((t.total - t.bytes) as f64 / speed)));
                                    }
                                }
                                RichText::new(s).color(theme::TEXT_MUTED())
                            }
                        };
                        ui.label(status.small());
                        if t.done && t.error.is_none() {
                            ui.horizontal(|ui| match t.kind {
                                Kind::Download | Kind::Local => {
                                    if ui.small_button(t!("Ouvrir", "Open")).clicked() {
                                        crate::server::open_local(&t.local);
                                    }
                                    if ui.small_button(t!("Afficher dans le dossier", "Show in folder")).clicked() {
                                        crate::server::reveal_local(&t.local);
                                    }
                                }
                                Kind::Upload => {
                                    if ui.small_button(t!("Afficher sur le serveur", "Show on the server")).clicked() {
                                        todo.push(Do::Go(Place::Folder { root: t.remote.0.clone(), path: t.remote.1.clone() }));
                                    }
                                }
                            });
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
    });
}

/// « 8 s », « 2 min 05 s », « 1 h 03 min ».
fn duration(secs: f64) -> String {
    let s = secs.round().max(0.0) as u64;
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min {:02} s", s / 60, s % 60),
        _ => format!("{} h {:02} min", s / 3600, s / 60 % 60),
    }
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
    fn durations() {
        assert_eq!(duration(8.2), "8 s");
        assert_eq!(duration(125.0), "2 min 05 s");
        assert_eq!(duration(3780.0), "1 h 03 min");
    }

    #[test]
    fn local_names() {
        assert_eq!(local_name("/home/me/Documents"), "Documents");
        assert_eq!(local_name("/"), "/");
    }

    #[test]
    fn paths() {
        assert_eq!(join("", "a"), "a");
        assert_eq!(join("a/b", "c"), "a/b/c");
        assert_eq!(parent_of("a/b/c"), "a/b");
        assert_eq!(parent_of("a"), "");
    }
}
