//! Fichiers : emplacements de stockage du serveur, parcourus et modifiés depuis la console.
//!
//! Emplacements : `NAS_AGENT_ROOTS="Médias=/srv/nas/medias;Partage=/srv/nas/partage"`.
//! Par défaut : `/srv/nas/{medias,partage,sauvegardes}` sous Linux ; en développement, un bac à
//! sable `data/fichiers/…` rempli d'exemples au premier lancement.
//!
//! Sécurité : tout chemin reçu est relatif à un emplacement ; `..`, chemins absolus et liens
//! qui sortent de l'emplacement sont refusés (vérifié après résolution des liens).

mod index;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nas_proto::{Entry, FileOp, Listing, Root, TRASH_DIR};

pub use index::Index;

/// Emplacement configuré : nom affiché et dossier réel.
#[derive(Clone)]
pub struct RootDir {
    pub name: String,
    pub dir: PathBuf,
}

pub struct Files {
    pub roots: Vec<RootDir>,
    pub index: Arc<Index>,
}

impl Files {
    pub fn new() -> Self {
        let mut roots = configured_roots();
        for r in &mut roots {
            let _ = fs::create_dir_all(&r.dir);
            // Chemin réel (sans « .. ») : affiché tel quel dans la console.
            if let Ok(c) = fs::canonicalize(&r.dir) {
                r.dir = c;
            }
        }
        if cfg!(debug_assertions) && std::env::var("NAS_AGENT_ROOTS").is_err() {
            seed_demo(&roots);
        }
        let index = Index::spawn(roots.clone());
        Files { roots, index }
    }

    fn root(&self, name: &str) -> Result<&RootDir, String> {
        self.roots.iter().find(|r| r.name == name).ok_or_else(|| format!("emplacement inconnu : {name}"))
    }

    /// Chemin réel d'un élément existant, vérifié (reste dans son emplacement).
    pub fn resolve(&self, root: &str, rel: &str) -> Result<PathBuf, String> {
        let r = self.root(root)?;
        if !nas_proto::safe_rel_path(rel) {
            return Err("chemin refusé".into());
        }
        let base = fs::canonicalize(&r.dir).map_err(|e| e.to_string())?;
        let full = fs::canonicalize(base.join(rel)).map_err(|_| format!("introuvable : {rel}"))?;
        if !full.starts_with(&base) {
            return Err("chemin refusé".into());
        }
        Ok(full)
    }

    /// Chemin réel d'un nouvel élément `name` dans le dossier existant `dir`.
    fn resolve_new(&self, root: &str, dir: &str, name: &str) -> Result<PathBuf, String> {
        if !nas_proto::safe_name(name) {
            return Err(format!("nom refusé : {name}"));
        }
        let parent = self.resolve(root, dir)?;
        if !parent.is_dir() {
            return Err(format!("pas un dossier : {dir}"));
        }
        Ok(parent.join(name))
    }

    pub fn roots_info(&self) -> Vec<Root> {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        self.roots
            .iter()
            .map(|r| {
                let path = fs::canonicalize(&r.dir).unwrap_or_else(|_| r.dir.clone());
                // Disque qui contient l'emplacement : le point de montage le plus long qui le préfixe.
                let disk = disks.list().iter().filter(|d| path.starts_with(d.mount_point())).max_by_key(|d| d.mount_point().as_os_str().len());
                Root {
                    name: r.name.clone(),
                    path: r.dir.to_string_lossy().into_owned(),
                    free: disk.map(|d| d.available_space()).unwrap_or(0),
                    total: disk.map(|d| d.total_space()).unwrap_or(0),
                }
            })
            .collect()
    }

    pub fn list(&self, root: &str, rel: &str) -> Result<Listing, String> {
        let dir = self.resolve(root, rel)?;
        let mut entries = Vec::new();
        for e in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // Fichiers cachés (commençant par un point) et corbeille : masqués.
            if name.starts_with('.') {
                continue;
            }
            if let Some(entry) = entry_of(&e.path(), name) {
                entries.push(entry);
            }
        }
        Ok(Listing { root: root.to_string(), path: rel.to_string(), entries })
    }

    /// Opération demandée par la console. `to_root` / `to` : destination (copie, déplacement)
    /// ou nouveau nom (renommage).
    pub fn op(&self, op: FileOp, root: &str, path: &str, to_root: Option<&str>, to: Option<&str>) -> Result<(), String> {
        let res = match op {
            FileOp::Mkdir => {
                let (dir, name) = split_parent(path);
                let target = self.resolve_new(root, dir, name)?;
                fs::create_dir(&target).map_err(|e| e.to_string())
            }
            FileOp::Rename => {
                let src = self.resolve_existing_item(root, path)?;
                let (dir, _) = split_parent(path);
                let target = self.resolve_new(root, dir, to.ok_or("nouveau nom manquant")?)?;
                if target.exists() {
                    return Err("un élément porte déjà ce nom".into());
                }
                fs::rename(&src, &target).map_err(|e| e.to_string())
            }
            FileOp::Delete => {
                let src = self.resolve_existing_item(root, path)?;
                let trash = fs::canonicalize(&self.root(root)?.dir).map_err(|e| e.to_string())?.join(TRASH_DIR);
                fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
                let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let target = free_name(&trash, &name);
                move_path(&src, &target)
            }
            FileOp::Copy | FileOp::Move => {
                let src = self.resolve_existing_item(root, path)?;
                let dest_root = to_root.unwrap_or(root);
                let dest_dir = to.unwrap_or("");
                let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let parent = self.resolve(dest_root, dest_dir)?;
                if parent.starts_with(&src) {
                    return Err("impossible de copier un dossier dans lui-même".into());
                }
                let target = free_name(&parent, &name);
                if op == FileOp::Copy { copy_path(&src, &target) } else { move_path(&src, &target) }
            }
        };
        self.index.touch();
        res
    }

    /// Élément existant qui n'est pas la racine de l'emplacement.
    fn resolve_existing_item(&self, root: &str, path: &str) -> Result<PathBuf, String> {
        if path.trim_matches('/').is_empty() {
            return Err("impossible sur la racine d'un emplacement".into());
        }
        self.resolve(root, path)
    }

    /// Chemin où écrire un fichier envoyé (`path` = dossier + nom). Écrit d'abord dans un
    /// fichier temporaire, renommé à la fin : un envoi coupé ne laisse pas de fichier tronqué.
    pub fn upload(&self, root: &str, path: &str, body: &mut dyn std::io::Read) -> Result<(), String> {
        let (dir, name) = split_parent(path);
        let target = self.resolve_new(root, dir, name)?;
        let tmp = target.with_file_name(format!(".{name}.envoi"));
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        if let Err(e) = std::io::copy(body, &mut f) {
            let _ = fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        drop(f);
        let target = if target.exists() { free_name(target.parent().unwrap_or(Path::new(".")), name) } else { target };
        fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        self.index.touch();
        Ok(())
    }
}

/// Emplacements configurés (variable d'environnement, sinon valeurs par défaut).
fn configured_roots() -> Vec<RootDir> {
    if let Ok(v) = std::env::var("NAS_AGENT_ROOTS") {
        return v.split(';').filter_map(|kv| kv.split_once('=')).map(|(n, p)| RootDir { name: n.trim().to_string(), dir: PathBuf::from(p.trim()) }).collect();
    }
    let base = if cfg!(debug_assertions) { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fichiers") } else { PathBuf::from("/srv/nas") };
    [("Médias", "medias"), ("Partage", "partage"), ("Sauvegardes", "sauvegardes")].into_iter().map(|(n, d)| RootDir { name: n.into(), dir: base.join(d) }).collect()
}

/// Exemples du bac à sable de développement (une seule fois).
fn seed_demo(roots: &[RootDir]) {
    let marker = roots[0].dir.join(".exemples");
    if marker.exists() {
        return;
    }
    let files: &[(&str, &str)] = &[
        ("portfolio/images/accueil.png", "png"),
        ("portfolio/images/weeping-desert-01.jpg", "jpg"),
        ("portfolio/images/weeping-desert-02.jpg", "jpg"),
        ("portfolio/videos/bande-annonce.mp4", "mp4"),
        ("portfolio/cv-dorian.pdf", "pdf"),
        ("liens/index.html", "<!doctype html><title>Liens</title>"),
        ("liens/style.css", "body{}"),
    ];
    for (path, content) in files {
        let p = roots[0].dir.join(path);
        let _ = fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
        let _ = fs::write(p, content.repeat(200));
    }
    if let Some(share) = roots.get(1) {
        for (path, content) in [
            ("Projets/GMTK 2026/build.zip", "zip"),
            ("Projets/GMTK 2026/notes.md", "# Notes"),
            ("Projets/Weeping Desert/Personnage.fbx", "fbx"),
            ("Projets/Weeping Desert/Desert.blend", "blend"),
            ("Projets/Weeping Desert/planning.xlsx", "xlsx"),
            ("Musique/theme-principal.wav", "wav"),
            ("Documents/facture-ovh.pdf", "pdf"),
            ("Documents/presentation.pptx", "pptx"),
            ("Documents/lisez-moi.txt", "Bonjour"),
            ("outils/script.ps1", "Write-Host"),
        ] {
            let p = share.dir.join(path);
            let _ = fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
            let _ = fs::write(p, content.repeat(500));
        }
    }
    let _ = fs::write(marker, "");
}

fn entry_of(path: &Path, name: String) -> Option<Entry> {
    let meta = fs::metadata(path).ok()?;
    let modified_ms = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0);
    Some(Entry { name, is_dir: meta.is_dir(), size: if meta.is_dir() { 0 } else { meta.len() }, modified_ms })
}

/// « a/b/c.txt » → (« a/b », « c.txt »).
fn split_parent(path: &str) -> (&str, &str) {
    let path = path.trim_matches('/');
    match path.rsplit_once('/') {
        Some((d, n)) => (d, n),
        None => ("", path),
    }
}

/// Nom libre dans `dir` : « photo.jpg », sinon « photo (2).jpg », « photo (3).jpg »…
fn free_name(dir: &Path, name: &str) -> PathBuf {
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

fn copy_path(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for e in fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
            copy_path(&e.path(), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(src, dst).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// Déplacement ; entre deux disques, `rename` échoue : copie puis suppression.
fn move_path(src: &Path, dst: &Path) -> Result<(), String> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_path(src, dst)?;
    if src.is_dir() { fs::remove_dir_all(src) } else { fs::remove_file(src) }.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> Files {
        let dir = std::env::temp_dir().join(format!("nas-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("a/photo.jpg"), "x").unwrap();
        let roots = vec![RootDir { name: "R".into(), dir: dir.clone() }];
        Files { index: Index::spawn(roots.clone()), roots }
    }

    #[test]
    fn refuses_escape() {
        let f = sandbox("escape");
        assert!(f.resolve("R", "../").is_err());
        assert!(f.resolve("R", "/etc").is_err());
        assert!(f.resolve("Autre", "").is_err());
        assert!(f.resolve("R", "a/photo.jpg").is_ok());
        assert!(f.op(FileOp::Delete, "R", "", None, None).is_err());
    }

    #[test]
    fn operations() {
        let f = sandbox("ops");
        f.op(FileOp::Mkdir, "R", "a/nouveau", None, None).unwrap();
        f.op(FileOp::Copy, "R", "a/photo.jpg", None, Some("a/b")).unwrap();
        f.op(FileOp::Copy, "R", "a/photo.jpg", None, Some("a")).unwrap();
        let names: Vec<String> = f.list("R", "a").unwrap().entries.into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"photo (2).jpg".to_string()) && names.contains(&"nouveau".to_string()));
        f.op(FileOp::Rename, "R", "a/photo (2).jpg", None, Some("copie.jpg")).unwrap();
        f.op(FileOp::Move, "R", "a/copie.jpg", None, Some("a/nouveau")).unwrap();
        assert_eq!(f.list("R", "a/nouveau").unwrap().entries.len(), 1);
        f.op(FileOp::Delete, "R", "a/nouveau", None, None).unwrap();
        assert!(f.list("R", "a").unwrap().entries.iter().all(|e| e.name != "nouveau"));
        // La corbeille est masquée mais contient l'élément.
        assert!(f.roots[0].dir.join(TRASH_DIR).join("nouveau").exists());
        assert!(f.op(FileOp::Copy, "R", "a", None, Some("a/b")).is_err());
        f.upload("R", "a/b/envoi.txt", &mut "bonjour".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(f.roots[0].dir.join("a/b/envoi.txt")).unwrap(), "bonjour");
    }
}
