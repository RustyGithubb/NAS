//! Liaison avec l'agent. Un fil en arrière-plan demande l'état et la liste des services toutes
//! les 2 secondes ; les actions et la lecture des journaux partent chacune dans leur propre fil.
//! L'interface lit simplement les derniers résultats (elle ne bloque jamais sur le réseau).

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui;
use nas_proto::{Action, ActionResult, FileOp, Listing, LogEntry, Logs, Root, SearchResult, Services, SiteAction, Sites, Status, UpdateState, UpdateStatus};

/// Intervalle entre deux demandes d'état.
const PERIOD: Duration = Duration::from_secs(2);
/// Intervalle entre deux lectures du journal affiché.
const LOG_PERIOD: Duration = Duration::from_secs(1);
/// Délai au-delà duquel l'agent est considéré comme injoignable.
const TIMEOUT: Duration = Duration::from_secs(3);
/// Lignes de journal gardées en mémoire par la console.
const LOG_KEEP: usize = 3000;

/// État de la liaison.
#[derive(Clone)]
pub enum Link {
    /// Première tentative en cours.
    Connecting,
    /// Agent joignable : dernier état reçu.
    Online(Box<Status>),
    /// Agent injoignable ou réponse illisible.
    Offline(String),
    /// Agent joignable, mais la clé d'accès est absente ou fausse.
    Denied,
}

/// Journal affiché dans l'onglet Journaux.
#[derive(Default)]
pub struct LogView {
    /// Service suivi ; `None` : tout le système.
    pub unit: Option<String>,
    pub entries: VecDeque<LogEntry>,
    cursor: Option<String>,
    in_flight: bool,
    last: Option<Instant>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Shared {
    addr: String,
    key: String,
    link: Option<Link>,
    pub services: Option<Services>,
    pub sites: Option<Sites>,
    /// Sites dont une action est en cours.
    pub sites_pending: HashSet<String>,
    /// Résultat de la dernière action sur un site.
    pub last_site_action: Option<(String, SiteAction, ActionResult)>,
    /// Services dont une action est en cours.
    pub pending: HashSet<String>,
    /// Résultat de la dernière action : service, action, résultat.
    pub last_action: Option<(String, Action, ActionResult)>,
    pub logs: LogView,
    pub files: FilesData,
    /// Mise à jour de l'agent (Réglages) : dernier état connu, ou erreur.
    pub update: Option<Result<UpdateStatus, String>>,
    update_polled: Option<Instant>,
    update_in_flight: bool,
}

/// Onglet Fichiers : emplacements, dossier affiché, recherche, transferts.
#[derive(Default)]
pub struct FilesData {
    pub roots: Option<Vec<Root>>,
    /// Dossier demandé (emplacement, chemin) et son contenu.
    pub listing_for: Option<(String, String)>,
    pub listing: Option<Result<Listing, String>>,
    pub loading: bool,
    /// Dernière recherche : requête et résultat.
    pub search: Option<(String, Result<SearchResult, String>)>,
    /// Fichiers modifiés récemment (page Accueil).
    pub recent: Option<Result<SearchResult, String>>,
    /// Résultat de la dernière opération (réussie ?, message).
    pub last: Option<(bool, String)>,
    pub transfers: Vec<Transfer>,
    /// Augmente à chaque fin de transfert ou de copie : les dossiers de ce PC affichés sont relus.
    pub local_gen: u64,
    /// Un transfert vers le serveur s'est terminé : le dossier affiché est redemandé.
    pub changed: bool,
    next_transfer: u64,
}

/// Sens d'un transfert.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Du serveur vers ce PC.
    Download,
    /// De ce PC vers le serveur.
    Upload,
    /// D'un dossier de ce PC à un autre.
    Local,
}

/// Envoi, téléchargement ou copie sur ce PC, avec sa progression (onglet Transferts).
#[derive(Clone)]
pub struct Transfer {
    pub id: u64,
    pub name: String,
    pub kind: Kind,
    pub done: bool,
    pub error: Option<String>,
    /// Octets transférés, et total (0 tant qu'il n'est pas connu).
    pub bytes: u64,
    pub total: u64,
    pub started: Instant,
    pub finished: Option<Instant>,
    /// Sur ce PC : destination d'un téléchargement ou d'une copie, source d'un envoi.
    pub local: std::path::PathBuf,
    /// Sur le serveur : emplacement et chemin (élément téléchargé, ou dossier de destination d'un envoi).
    pub remote: (String, String),
}

/// Met à jour la progression d'un transfert depuis son fil (au plus 10 fois par seconde).
struct Tracker {
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    id: u64,
    pending: u64,
    last: Instant,
}

impl Tracker {
    fn with(&self, f: impl FnOnce(&mut Transfer)) {
        let mut s = lock(&self.shared);
        if let Some(t) = s.files.transfers.iter_mut().find(|t| t.id == self.id) {
            f(t);
        }
    }

    fn total(&self, total: u64) {
        self.with(|t| t.total = total);
    }

    fn add(&mut self, n: u64) {
        self.pending += n;
        if self.last.elapsed() >= Duration::from_millis(100) {
            let p = std::mem::take(&mut self.pending);
            self.with(|t| t.bytes += p);
            self.last = Instant::now();
            self.ctx.request_repaint();
        }
    }

    fn finish(mut self, r: Result<(), String>) {
        let p = std::mem::take(&mut self.pending);
        self.with(|t| {
            t.bytes += p;
            if r.is_ok() && t.total > 0 {
                t.bytes = t.total;
            }
            t.done = true;
            t.finished = Some(Instant::now());
            t.error = r.err();
        });
        let mut s = lock(&self.shared);
        // Le contenu affiché a changé : dossier du serveur redemandé, dossiers du PC relus.
        s.files.local_gen += 1;
        s.files.changed = true;
        drop(s);
        self.ctx.request_repaint();
    }
}

/// Lecture qui compte les octets envoyés (progression d'un envoi).
struct Counting<'a, R> {
    inner: R,
    tracker: &'a mut Tracker,
}

impl<R: std::io::Read> std::io::Read for Counting<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.tracker.add(n as u64);
        Ok(n)
    }
}

/// Copie `from` vers `to` par morceaux en comptant les octets.
fn copy_counted(from: &mut dyn std::io::Read, to: &mut dyn std::io::Write, tracker: &mut Tracker) -> Result<(), String> {
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = from.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        tracker.add(n as u64);
    }
}

/// Fichiers et dossiers d'un dossier de ce PC, récursivement : dossiers (chemins relatifs, dans
/// l'ordre de création) et fichiers (chemin complet, chemin relatif, taille).
type LocalTree = (Vec<String>, Vec<(std::path::PathBuf, String, u64)>);

fn local_tree(root: &std::path::Path) -> LocalTree {
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    fn walk(dir: &std::path::Path, rel: &str, dirs: &mut Vec<String>, files: &mut Vec<(std::path::PathBuf, String, u64)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                dirs.push(r.clone());
                walk(&e.path(), &r, dirs, files);
            } else if md.is_file() {
                files.push((e.path(), r, md.len()));
            }
        }
    }
    walk(root, "", &mut dirs, &mut files);
    (dirs, files)
}

/// Copie un fichier ou un dossier de ce PC (déplacement : renommage si possible).
fn local_copy_item(src: &std::path::Path, dest: &std::path::Path, mv: bool, tracker: &mut Tracker) -> Result<(), String> {
    if mv && std::fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    if src.is_dir() {
        let (dirs, files) = local_tree(src);
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        for d in dirs {
            std::fs::create_dir_all(dest.join(&d)).map_err(|e| e.to_string())?;
        }
        for (path, rel, _) in files {
            let mut from = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            let mut to = std::fs::File::create(dest.join(&rel)).map_err(|e| e.to_string())?;
            copy_counted(&mut from, &mut to, tracker)?;
        }
        if mv {
            std::fs::remove_dir_all(src).map_err(|e| e.to_string())?;
        }
    } else {
        let mut from = std::fs::File::open(src).map_err(|e| e.to_string())?;
        let mut to = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        copy_counted(&mut from, &mut to, tracker)?;
        if mv {
            std::fs::remove_file(src).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Taille d'un fichier ou d'un dossier de ce PC.
fn local_size(p: &std::path::Path) -> u64 {
    if p.is_dir() { local_tree(p).1.iter().map(|f| f.2).sum() } else { p.metadata().map(|m| m.len()).unwrap_or(0) }
}

pub struct Server {
    shared: Arc<Mutex<Shared>>,
    agent: ureq::Agent,
    /// Pour les transferts de fichiers : pas de limite de durée totale, seulement d'inactivité.
    transfer_agent: ureq::Agent,
    ctx: egui::Context,
}

impl Server {
    /// Démarre le fil de liaison vers `addr` (« IP:port »).
    pub fn start(ctx: &egui::Context, addr: &str, key: &str) -> Self {
        let shared = Arc::new(Mutex::new(Shared { addr: addr.to_string(), key: key.to_string(), ..Default::default() }));
        let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
        let (s, a, c) = (shared.clone(), agent.clone(), ctx.clone());
        std::thread::spawn(move || {
            loop {
                let to = Target::of(&lock(&s));
                let started = Instant::now();
                let link = fetch_status(&a, &to);
                let online = matches!(link, Link::Online(_));
                let services = online.then(|| get::<Services>(&a, &to, "/api/services").ok()).flatten();
                let sites = online.then(|| get::<Sites>(&a, &to, "/api/sites").ok()).flatten();
                {
                    let mut s = lock(&s);
                    // Adresse ou clé changée pendant la demande : résultat ignoré.
                    if to.current(&s) {
                        s.link = Some(link);
                        s.services = services;
                        s.sites = sites;
                    }
                }
                c.request_repaint();
                std::thread::sleep(PERIOD.saturating_sub(started.elapsed()));
            }
        });
        let transfer_agent = ureq::AgentBuilder::new().timeout_connect(TIMEOUT).timeout_read(Duration::from_secs(60)).timeout_write(Duration::from_secs(60)).build();
        Server { shared, agent, transfer_agent, ctx: ctx.clone() }
    }

    /// Accès aux données partagées (liste des services, journal…).
    pub fn data(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    /// Change l'adresse de l'agent ou la clé d'accès (Réglages) : on repart de zéro.
    pub fn set_target(&self, addr: &str, key: &str) {
        let mut s = self.data();
        if s.addr != addr || s.key != key {
            *s = Shared { addr: addr.to_string(), key: key.to_string(), ..Default::default() };
        }
    }

    pub fn link(&self) -> Link {
        self.data().link.clone().unwrap_or(Link::Connecting)
    }

    /// Démarre, arrête ou redémarre un service ; le résultat arrive dans `last_action`.
    pub fn action(&self, unit: &str, action: Action) {
        let to = {
            let mut s = self.data();
            if !s.pending.insert(unit.to_string()) {
                return;
            }
            Target::of(&s)
        };
        let (shared, agent, ctx, unit) = (self.shared.clone(), self.agent.clone(), self.ctx.clone(), unit.to_string());
        std::thread::spawn(move || {
            let path = format!("/api/services/{}/{}", urlencoding::encode(&unit), action.verb());
            let result = match to.request(&agent, "POST", &path).call() {
                Ok(r) => r.into_json::<ActionResult>().unwrap_or_else(|e| ActionResult { ok: false, message: e.to_string() }),
                Err(e) => ActionResult { ok: false, message: describe(e) },
            };
            // État à jour tout de suite, sans attendre la demande périodique.
            let services = get::<Services>(&agent, &to, "/api/services").ok();
            let mut s = lock(&shared);
            s.pending.remove(&unit);
            if to.current(&s) {
                if services.is_some() {
                    s.services = services;
                }
                s.last_action = Some((unit, action, result));
            }
            ctx.request_repaint();
        });
    }

    /// Met un site en ligne / en maintenance / hors ligne, ou le sauvegarde ; le résultat arrive
    /// dans `last_site_action`.
    pub fn site_action(&self, name: &str, action: SiteAction) {
        let to = {
            let mut s = self.data();
            if !s.sites_pending.insert(name.to_string()) {
                return;
            }
            Target::of(&s)
        };
        let (shared, agent, ctx, name) = (self.shared.clone(), self.agent.clone(), self.ctx.clone(), name.to_string());
        std::thread::spawn(move || {
            let path = format!("/api/sites/{}/{}", urlencoding::encode(&name), action.verb());
            let result = match to.request(&agent, "POST", &path).call() {
                Ok(r) => r.into_json::<ActionResult>().unwrap_or_else(|e| ActionResult { ok: false, message: e.to_string() }),
                Err(e) => ActionResult { ok: false, message: describe(e) },
            };
            let sites = get::<Sites>(&agent, &to, "/api/sites").ok();
            let mut s = lock(&shared);
            s.sites_pending.remove(&name);
            if to.current(&s) {
                if sites.is_some() {
                    s.sites = sites;
                }
                s.last_site_action = Some((name, action, result));
            }
            ctx.request_repaint();
        });
    }

    /// À appeler à chaque image par l'onglet Journaux : suit le journal de `unit` (du système entier si `None`).
    pub fn follow_logs(&self, unit: Option<&str>) {
        let job = {
            let mut s = self.data();
            if s.logs.unit.as_deref() != unit {
                s.logs = LogView { unit: unit.map(str::to_string), ..Default::default() };
            }
            let unit = s.logs.unit.clone();
            let due = s.logs.last.is_none_or(|t| t.elapsed() >= LOG_PERIOD);
            if s.logs.in_flight || !due {
                return;
            }
            s.logs.in_flight = true;
            (Target::of(&s), unit, s.logs.cursor.clone())
        };
        let (to, unit, cursor) = job;
        let (shared, agent, ctx) = (self.shared.clone(), self.agent.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            // Sans service : journal de tout le système.
            let mut path = match &unit {
                Some(u) => format!("/api/logs/{}", urlencoding::encode(u)),
                None => "/api/logs".to_string(),
            };
            if let Some(c) = &cursor {
                path.push_str(&format!("?after={}", urlencoding::encode(c)));
            }
            let result = get::<Logs>(&agent, &to, &path);
            let mut s = lock(&shared);
            // Autre service, adresse ou clé choisis entre-temps : résultat ignoré.
            if !to.current(&s) || s.logs.unit != unit {
                return;
            }
            let view = &mut s.logs;
            view.in_flight = false;
            view.last = Some(Instant::now());
            match result {
                Ok(logs) => {
                    let new = !logs.entries.is_empty();
                    view.entries.extend(logs.entries);
                    while view.entries.len() > LOG_KEEP {
                        view.entries.pop_front();
                    }
                    if logs.cursor.is_some() {
                        view.cursor = logs.cursor;
                    }
                    view.error = None;
                    if new {
                        ctx.request_repaint();
                    }
                }
                Err(e) => {
                    view.error = Some(e);
                    ctx.request_repaint();
                }
            }
        });
        // Prochaine lecture : l'interface doit se redessiner même sans mouvement de souris.
        self.ctx.request_repaint_after(LOG_PERIOD);
    }

    // --- Fichiers ------------------------------------------------------------------------

    /// Lance `job` dans un fil, avec l'agent visé à cet instant ; `done` range le résultat
    /// (seulement si l'adresse et la clé n'ont pas changé entre-temps).
    fn spawn_files<T: Send + 'static>(&self, job: impl FnOnce(&ureq::Agent, &ureq::Agent, &Target) -> T + Send + 'static, done: impl FnOnce(&mut FilesData, T) + Send + 'static) {
        let to = Target::of(&self.data());
        let (shared, agent, transfer, ctx) = (self.shared.clone(), self.agent.clone(), self.transfer_agent.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let out = job(&agent, &transfer, &to);
            let mut s = lock(&shared);
            if to.current(&s) {
                done(&mut s.files, out);
            }
            ctx.request_repaint();
        });
    }

    pub fn files_roots(&self) {
        self.spawn_files(|a, _, to| get::<Vec<Root>>(a, to, "/api/files/roots"), |f, r| f.roots = r.ok());
    }

    /// Demande le contenu d'un dossier.
    pub fn files_list(&self, root: &str, path: &str) {
        {
            let mut s = self.data();
            s.files.listing_for = Some((root.to_string(), path.to_string()));
            s.files.loading = true;
        }
        let key = (root.to_string(), path.to_string());
        let url = format!("/api/files/list?root={}&path={}", urlencoding::encode(root), urlencoding::encode(path));
        self.spawn_files(
            move |a, _, to| get::<Listing>(a, to, &url),
            move |f, r| {
                // Un autre dossier a été demandé entre-temps : résultat ignoré.
                if f.listing_for.as_ref() == Some(&key) {
                    f.listing = Some(r);
                    f.loading = false;
                }
            },
        );
    }

    /// Recherche dans l'index de l'agent (instantané, comme Everything).
    pub fn files_search(&self, q: &str) {
        let q = q.to_string();
        let url = format!("/api/files/search?q={}", urlencoding::encode(&q));
        self.spawn_files(move |a, _, to| get::<SearchResult>(a, to, &url), move |f, r| f.search = Some((q, r)));
    }

    /// Fichiers modifiés récemment, pour la page Accueil.
    pub fn files_recent(&self) {
        self.spawn_files(|a, _, to| get::<SearchResult>(a, to, "/api/files/recent"), |f, r| f.recent = Some(r));
    }

    /// Opération sur un élément, puis rafraîchit le dossier affiché.
    pub fn files_op(&self, op: FileOp, root: &str, path: &str, to_root: Option<&str>, to: Option<&str>, done_msg: String) {
        let mut url = format!("/api/files/{}?root={}&path={}", op.verb(), urlencoding::encode(root), urlencoding::encode(path));
        if let Some(r) = to_root {
            url.push_str(&format!("&to_root={}", urlencoding::encode(r)));
        }
        if let Some(t) = to {
            url.push_str(&format!("&to={}", urlencoding::encode(t)));
        }
        let me = self.handle();
        self.spawn_files(
            move |a, _, to| match to.request(a, "POST", &url).call() {
                Ok(r) => r.into_json::<ActionResult>().map_err(|e| e.to_string()),
                Err(e) => Err(describe(e)),
            },
            move |f, r| {
                f.last = Some(match r {
                    Ok(res) if res.ok => (true, done_msg),
                    Ok(res) => (false, res.message),
                    Err(e) => (false, e),
                });
                me.refresh_files(f);
            },
        );
    }

    /// Nouveau transfert affiché dans l'onglet Transferts, et son suivi.
    fn track(&self, name: &str, kind: Kind, local: std::path::PathBuf, remote: (String, String)) -> Tracker {
        let mut s = self.data();
        s.files.next_transfer += 1;
        let id = s.files.next_transfer;
        s.files.transfers.push(Transfer { id, name: name.to_string(), kind, done: false, error: None, bytes: 0, total: 0, started: Instant::now(), finished: None, local, remote });
        Tracker { shared: self.shared.clone(), ctx: self.ctx.clone(), id, pending: 0, last: Instant::now() }
    }

    /// Ouvre un fichier du serveur : téléchargé dans un dossier temporaire, puis ouvert avec le
    /// programme par défaut.
    pub fn download(&self, root: &str, path: &str, open: bool) {
        let dir = std::env::temp_dir().join("NAS");
        let _ = std::fs::create_dir_all(&dir);
        self.download_to(root, path, false, dir, open);
    }

    /// Télécharge un fichier ou un dossier (avec tout son contenu) du serveur dans `dest_dir`.
    pub fn download_to(&self, root: &str, path: &str, is_dir: bool, dest_dir: std::path::PathBuf, open: bool) {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let target = free_local_name(&dest_dir, &name);
        let mut tracker = self.track(&name, Kind::Download, target.clone(), (root.to_string(), path.to_string()));
        let (agent, transfer, to, root, path) = (self.agent.clone(), self.transfer_agent.clone(), Target::of(&self.data()), root.to_string(), path.to_string());
        std::thread::spawn(move || {
            let get_file = |remote: &str, local: &std::path::Path, tracker: &mut Tracker| -> Result<(), String> {
                let url = format!("/api/files/download?root={}&path={}", urlencoding::encode(&root), urlencoding::encode(remote));
                let resp = to.request(&transfer, "GET", &url).call().map_err(describe)?;
                let mut file = std::fs::File::create(local).map_err(|e| e.to_string())?;
                copy_counted(&mut resp.into_reader(), &mut file, tracker)
            };
            let r = (|| -> Result<(), String> {
                if !is_dir {
                    let size = get::<Listing>(&agent, &to, &format!("/api/files/list?root={}&path={}", urlencoding::encode(&root), urlencoding::encode(&parent(&path))))
                        .ok()
                        .and_then(|l| l.entries.into_iter().find(|e| e.name == name).map(|e| e.size))
                        .unwrap_or(0);
                    tracker.total(size);
                    return get_file(&path, &target, &mut tracker);
                }
                // Dossier : on parcourt d'abord tout le contenu (pour connaître la taille totale).
                let (mut dirs, mut files) = (vec![String::new()], Vec::new());
                let mut i = 0;
                while i < dirs.len() {
                    let rel = dirs[i].clone();
                    let full = if rel.is_empty() { path.clone() } else { format!("{path}/{rel}") };
                    let l = get::<Listing>(&agent, &to, &format!("/api/files/list?root={}&path={}", urlencoding::encode(&root), urlencoding::encode(&full)))?;
                    for e in l.entries {
                        let r = if rel.is_empty() { e.name.clone() } else { format!("{rel}/{}", e.name) };
                        if e.is_dir { dirs.push(r) } else { files.push((r, e.size)) }
                    }
                    i += 1;
                }
                tracker.total(files.iter().map(|f| f.1).sum());
                for d in &dirs {
                    std::fs::create_dir_all(target.join(d)).map_err(|e| e.to_string())?;
                }
                for (rel, _) in files {
                    get_file(&format!("{path}/{rel}"), &target.join(&rel), &mut tracker)?;
                }
                Ok(())
            })();
            if r.is_ok() && open {
                open_local(&target);
            }
            tracker.finish(r);
        });
    }

    /// Envoie des fichiers ou des dossiers (avec tout leur contenu) de ce PC dans le dossier
    /// `dir` de l'emplacement `root`.
    pub fn upload(&self, root: &str, dir: &str, local: Vec<std::path::PathBuf>) {
        for src in local {
            let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let mut tracker = self.track(&name, Kind::Upload, src.clone(), (root.to_string(), dir.to_string()));
            let (agent, transfer, to, root, dir) = (self.agent.clone(), self.transfer_agent.clone(), Target::of(&self.data()), root.to_string(), dir.to_string());
            std::thread::spawn(move || {
                let join = |a: &str, b: &str| if a.is_empty() { b.to_string() } else { format!("{a}/{b}") };
                let put = |file: &std::path::Path, remote: &str, tracker: &mut Tracker| -> Result<(), String> {
                    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
                    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                    let url = format!("/api/files/upload?root={}&path={}", urlencoding::encode(&root), urlencoding::encode(remote));
                    let r = to.request(&transfer, "POST", &url).set("Content-Length", &len.to_string()).send(Counting { inner: f, tracker }).map_err(describe)?;
                    let res: ActionResult = r.into_json().map_err(|e| e.to_string())?;
                    if res.ok { Ok(()) } else { Err(res.message) }
                };
                // Dossier déjà présent sur le serveur : on le complète (pas une erreur).
                let mkdir = |remote: &str| -> Result<(), String> {
                    let url = format!("/api/files/mkdir?root={}&path={}", urlencoding::encode(&root), urlencoding::encode(remote));
                    let res: ActionResult = to.request(&agent, "POST", &url).call().map_err(describe)?.into_json().map_err(|e| e.to_string())?;
                    if res.ok || res.message.to_lowercase().contains("exist") { Ok(()) } else { Err(res.message) }
                };
                let r = (|| -> Result<(), String> {
                    let base = join(&dir, &name);
                    if src.is_dir() {
                        let (dirs, files) = local_tree(&src);
                        tracker.total(files.iter().map(|f| f.2).sum());
                        mkdir(&base)?;
                        for d in dirs {
                            mkdir(&join(&base, &d))?;
                        }
                        for (path, rel, _) in files {
                            put(&path, &join(&base, &rel), &mut tracker)?;
                        }
                        Ok(())
                    } else {
                        tracker.total(src.metadata().map(|m| m.len()).unwrap_or(0));
                        put(&src, &base, &mut tracker)
                    }
                })();
                tracker.finish(r);
            });
        }
    }

    /// Copie (ou déplace) des fichiers et dossiers de ce PC dans `dest_dir`.
    pub fn local_copy(&self, srcs: Vec<std::path::PathBuf>, dest_dir: std::path::PathBuf, mv: bool) {
        for src in srcs {
            let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // Déplacé dans son propre dossier : rien à faire (copié : « nom (2) »).
            if name.is_empty() || mv && src.parent() == Some(dest_dir.as_path()) {
                continue;
            }
            let dest = free_local_name(&dest_dir, &name);
            let mut tracker = self.track(&name, Kind::Local, dest.clone(), Default::default());
            std::thread::spawn(move || {
                tracker.total(local_size(&src));
                let r = if dest.starts_with(&src) {
                    Err(t!("impossible de copier un dossier dans lui-même", "cannot copy a folder into itself").to_string())
                } else {
                    local_copy_item(&src, &dest, mv, &mut tracker)
                };
                tracker.finish(r);
            });
        }
    }

    /// Retire de l'onglet Transferts ceux qui sont terminés.
    pub fn clear_transfers(&self) {
        self.data().files.transfers.retain(|t| !t.done);
    }

    /// Un transfert vers le serveur s'est terminé depuis la dernière fois ?
    pub fn take_changed(&self) -> bool {
        std::mem::take(&mut self.data().files.changed)
    }

    /// Copie pour lancer des demandes depuis un autre fil (fenêtre « Ouvrir » de Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn handle_pub(&self) -> Server {
        self.handle()
    }

    /// Copie légère pour relancer des demandes depuis un fil.
    fn handle(&self) -> Server {
        Server { shared: self.shared.clone(), agent: self.agent.clone(), transfer_agent: self.transfer_agent.clone(), ctx: self.ctx.clone() }
    }

    /// Redemande le dossier affiché et les emplacements (après une modification). Appelée avec
    /// le verrou déjà pris : les demandes partent dans un autre fil.
    fn refresh_files(&self, f: &mut FilesData) {
        let Some((root, path)) = f.listing_for.clone() else { return };
        let me = self.handle();
        std::thread::spawn(move || {
            me.files_list(&root, &path);
            me.files_roots();
        });
    }

    /// À appeler à chaque image par les Réglages : redemande l'état des mises à jour de l'agent,
    /// toutes les 2 s pendant une recherche (ou si l'agent redémarre), sinon toutes les 30 s.
    pub fn poll_update(&self) {
        let to = {
            let mut s = self.data();
            let busy = matches!(&s.update, Some(Ok(u)) if u.state == UpdateState::Running) || matches!(s.update, Some(Err(_)));
            let every = if busy { Duration::from_secs(2) } else { Duration::from_secs(30) };
            self.ctx.request_repaint_after(every);
            if s.update_in_flight || s.update_polled.is_some_and(|t| t.elapsed() < every) {
                return;
            }
            s.update_in_flight = true;
            s.update_polled = Some(Instant::now());
            Target::of(&s)
        };
        let (shared, agent, ctx) = (self.shared.clone(), self.agent.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = get::<UpdateStatus>(&agent, &to, "/api/update");
            let mut s = lock(&shared);
            s.update_in_flight = false;
            if to.current(&s) {
                s.update = Some(r);
            }
            ctx.request_repaint();
        });
    }

    /// Lance une recherche de mise à jour de l'agent ; l'avancement arrive par `poll_update`.
    pub fn check_update(&self) {
        let to = Target::of(&self.data());
        let (shared, agent, ctx) = (self.shared.clone(), self.agent.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = match to.request(&agent, "POST", "/api/update").call() {
                Ok(r) => r.into_json::<ActionResult>().unwrap_or_else(|e| ActionResult { ok: false, message: e.to_string() }),
                Err(e) => ActionResult { ok: false, message: describe(e) },
            };
            let mut s = lock(&shared);
            if to.current(&s) {
                s.update = Some(if r.ok {
                    let prev = s.update.take().and_then(Result::ok).unwrap_or_default();
                    Ok(UpdateStatus { state: UpdateState::Running, message: t!("Recherche de mise à jour…", "Looking for an update…").into(), ..prev })
                } else {
                    Err(r.message)
                });
                s.update_polled = Some(Instant::now());
            }
            ctx.request_repaint();
        });
    }

    /// Vide le journal affiché (les lignes suivantes continuent d'arriver).
    pub fn clear_logs(&self) {
        self.data().logs.entries.clear();
    }
}

/// Dossier Téléchargements de ce PC.
pub fn download_dir() -> std::path::PathBuf {
    directories::UserDirs::new().and_then(|u| u.download_dir().map(|d| d.to_path_buf())).unwrap_or_else(std::env::temp_dir)
}

fn parent(path: &str) -> String {
    path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()
}

/// Nom libre dans `dir` : « photo.jpg », sinon « photo (2).jpg »…
pub fn free_local_name(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (2..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// Ouvre un fichier ou un dossier de ce PC avec le programme par défaut.
pub fn open_local(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Ouvre le dossier d'un fichier de ce PC, le fichier sélectionné.
pub fn reveal_local(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(not(windows))]
    open_local(path.parent().unwrap_or(path));
}

fn lock(s: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    // Un fil qui a paniqué en tenant le verrou ne doit pas bloquer l'interface.
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Agent visé par une demande : adresse et clé au moment où elle part.
struct Target {
    addr: String,
    key: String,
}

impl Target {
    fn of(s: &Shared) -> Self {
        Target { addr: s.addr.clone(), key: s.key.clone() }
    }

    /// Toujours l'agent choisi dans les réglages ?
    fn current(&self, s: &Shared) -> bool {
        s.addr == self.addr && s.key == self.key
    }

    /// Demande HTTP vers l'agent, avec la clé d'accès.
    fn request(&self, agent: &ureq::Agent, method: &str, path: &str) -> ureq::Request {
        agent.request(method, &format!("http://{}{path}", self.addr.trim())).set("Authorization", &format!("Bearer {}", self.key.trim()))
    }
}

fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(401, _) => t!("clé d'accès refusée", "access key refused").to_string(),
        ureq::Error::Status(code, r) => {
            let msg = r.into_json::<serde_json::Value>().ok().and_then(|v| v.get("error").and_then(|m| m.as_str()).map(str::to_string));
            msg.unwrap_or_else(|| tf!("erreur HTTP {code}", "HTTP error {code}"))
        }
        ureq::Error::Transport(_) => t!("agent injoignable", "agent unreachable").to_string(),
    }
}

fn get<T: serde::de::DeserializeOwned>(agent: &ureq::Agent, to: &Target, path: &str) -> Result<T, String> {
    match to.request(agent, "GET", path).call() {
        Ok(r) => r.into_json().map_err(|_| t!("réponse illisible", "unreadable response").to_string()),
        Err(e) => Err(describe(e)),
    }
}

fn fetch_status(agent: &ureq::Agent, to: &Target) -> Link {
    let status: Status = match to.request(agent, "GET", "/api/status").call() {
        Ok(r) => match r.into_json() {
            Ok(s) => s,
            Err(_) => return Link::Offline(t!("réponse illisible", "unreadable response").to_string()),
        },
        Err(ureq::Error::Status(401, _)) => return Link::Denied,
        Err(e) => return Link::Offline(describe(e)),
    };
    if status.protocol != nas_proto::PROTOCOL_VERSION {
        return Link::Offline(tf!(
            "version incompatible (agent : {}, console : {})",
            "incompatible version (agent: {}, console: {})",
            status.protocol,
            nas_proto::PROTOCOL_VERSION
        ));
    }
    Link::Online(Box::new(status))
}
