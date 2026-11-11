//! Index de recherche façon Everything : la liste de tous les éléments des emplacements est
//! gardée en mémoire et refaite en arrière-plan (toutes les 5 minutes, et peu après chaque
//! modification faite depuis la console). Chercher ne lit donc jamais le disque : résultat immédiat.
//!
//! Syntaxe : mots séparés par des espaces (tous doivent apparaître dans le nom, sans tenir compte
//! des majuscules), jokers `*` et `?` (`*.png`, `rapport-202?`), `ext:jpg;png`, `dossier:` (dossiers
//! seulement), `fichier:` (fichiers seulement).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use nas_proto::{Entry, Hit, SearchResult};

use super::RootDir;

/// Nombre maximal d'éléments indexés (au-delà, l'index s'arrête).
const MAX_ITEMS: usize = 500_000;
/// Nombre maximal de résultats renvoyés.
const MAX_HITS: usize = 500;
const REBUILD_EVERY: Duration = Duration::from_secs(300);

struct Item {
    root: usize,
    /// Chemin relatif complet.
    path: String,
    name_lower: String,
    entry: Entry,
}

pub struct Index {
    roots: Vec<RootDir>,
    items: RwLock<Vec<Item>>,
    dirty: AtomicBool,
}

impl Index {
    pub fn spawn(roots: Vec<RootDir>) -> Arc<Index> {
        let index = Arc::new(Index { roots, items: RwLock::new(Vec::new()), dirty: AtomicBool::new(true) });
        let i = index.clone();
        std::thread::spawn(move || {
            let mut last = Instant::now();
            loop {
                if i.dirty.swap(false, Ordering::Relaxed) || last.elapsed() >= REBUILD_EVERY {
                    i.rebuild();
                    last = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        index
    }

    /// Une modification a eu lieu : l'index sera refait sous peu.
    pub fn touch(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn rebuild(&self) {
        let mut items = Vec::new();
        for (i, r) in self.roots.iter().enumerate() {
            walk(&r.dir, "", i, &mut items);
        }
        if let Ok(mut w) = self.items.write() {
            *w = items;
        }
    }

    pub fn search(&self, query: &str) -> SearchResult {
        let q = Query::parse(query);
        let items = self.items.read().ok();
        let Some(items) = items else { return SearchResult::default() };
        let mut hits = Vec::new();
        let mut truncated = false;
        if !q.is_empty() {
            for it in items.iter().filter(|it| q.matches(it)) {
                if hits.len() == MAX_HITS {
                    truncated = true;
                    break;
                }
                hits.push(Hit { root: self.roots[it.root].name.clone(), path: it.path.clone(), entry: it.entry.clone() });
            }
        }
        SearchResult { hits, truncated, indexed: items.len() }
    }
}

fn walk(dir: &Path, rel: &str, root: usize, out: &mut Vec<Item>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= MAX_ITEMS {
            return;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        // Les liens symboliques ne sont pas suivis (boucles, sorties de l'emplacement).
        if ft.is_symlink() {
            continue;
        }
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let Some(entry) = super::entry_of(&e.path(), name.clone()) else { continue };
        let is_dir = entry.is_dir;
        out.push(Item { root, path: path.clone(), name_lower: name.to_lowercase(), entry });
        if is_dir {
            walk(&e.path(), &path, root, out);
        }
    }
}

/// Requête analysée.
#[derive(Default)]
struct Query {
    words: Vec<String>,
    globs: Vec<String>,
    exts: Vec<String>,
    dirs_only: bool,
    files_only: bool,
}

impl Query {
    fn parse(q: &str) -> Query {
        let mut out = Query::default();
        for tok in q.split_whitespace() {
            let low = tok.to_lowercase();
            if let Some(e) = low.strip_prefix("ext:") {
                out.exts.extend(e.split(';').filter(|s| !s.is_empty()).map(|s| s.trim_start_matches('.').to_string()));
            } else if low == "dossier:" || low == "folder:" {
                out.dirs_only = true;
            } else if low == "fichier:" || low == "file:" {
                out.files_only = true;
            } else if low.contains(['*', '?']) {
                out.globs.push(low);
            } else {
                out.words.push(low);
            }
        }
        out
    }

    fn is_empty(&self) -> bool {
        self.words.is_empty() && self.globs.is_empty() && self.exts.is_empty() && !self.dirs_only && !self.files_only
    }

    fn matches(&self, it: &Item) -> bool {
        if (self.dirs_only && !it.entry.is_dir) || (self.files_only && it.entry.is_dir) {
            return false;
        }
        if !self.exts.is_empty() {
            let ext = it.name_lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
            if it.entry.is_dir || !self.exts.iter().any(|e| e == ext) {
                return false;
            }
        }
        self.words.iter().all(|w| it.name_lower.contains(w.as_str())) && self.globs.iter().all(|g| glob(g.as_bytes(), it.name_lower.as_bytes()))
    }
}

/// Correspondance avec jokers : `*` = n'importe quelle suite, `?` = un caractère.
fn glob(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, is_dir: bool) -> Item {
        Item { root: 0, path: name.into(), name_lower: name.to_lowercase(), entry: Entry { name: name.into(), is_dir, ..Default::default() } }
    }

    #[test]
    fn globs() {
        assert!(glob(b"*.png", b"accueil.png"));
        assert!(!glob(b"*.png", b"accueil.jpg"));
        assert!(glob(b"rapport-202?", b"rapport-2026"));
        assert!(glob(b"*desert*", b"weeping-desert-01.jpg"));
    }

    #[test]
    fn queries() {
        let q = Query::parse("Desert ext:jpg;png");
        assert!(q.matches(&item("Weeping-Desert-01.JPG", false)));
        assert!(!q.matches(&item("desert.blend", false)));
        assert!(!q.matches(&item("Desert", true)));
        let q = Query::parse("dossier: projets");
        assert!(q.matches(&item("Projets", true)) && !q.matches(&item("projets.txt", false)));
        assert!(Query::parse("  ").is_empty());
    }
}
