//! Synchronisation des apps (WorkTable et les suivantes) : comptes, répertoire d'apps, documents
//! chiffrés et versionnés. Voir `nas_proto::sync` pour les routes.
//!
//! - Routes publiques `/sync/…` : joignables depuis Internet par Caddy, protégées par un jeton
//!   de session propre à chaque poste (et non par la clé de la console).
//! - Routes `/api/sync/…` : administration depuis la console (clé d'accès de l'agent).
//!
//! Données : `NAS_SYNC_DIR` (défaut `/srv/nas/sync`, `data/sync` en développement) :
//! `sync.db` (SQLite : apps, comptes, postes, index des documents) et un fichier par version de
//! document, `<app>/<compte>/<2 premiers caractères>/<document>.<version>`. Tout est chiffré par
//! les apps : le serveur ne voit ni les noms ni le contenu.
//! Sauvegardes : `NAS_SYNC_BACKUP_DIR` (défaut `/srv/nas/sauvegardes/sync`).

mod admin;
mod db;
mod public;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nas_proto::sync::{Activity, ActivityKind};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

pub use admin::admin;
pub use public::serve;

/// Demandes `/sync` traitées en même temps au maximum (un fil par demande : on protège le Mac).
const MAX_INFLIGHT: usize = 8;
/// Lignes d'activité gardées en mémoire.
const ACTIVITY: usize = 60;
/// Durée de validité d'une session (prolongée à chaque utilisation).
const SESSION_S: u64 = 90 * 24 * 3600;
/// Durée de validité d'un code d'invitation.
const INVITE_S: u64 = 48 * 3600;
/// Échecs de connexion tolérés par adresse et par minute.
const MAX_FAILURES: usize = 5;
/// Sauvegardes de la base gardées.
const BACKUPS: usize = 7;

pub struct Sync {
    db: Mutex<Connection>,
    dir: PathBuf,
    backup_dir: PathBuf,
    simulated: bool,
    inflight: AtomicUsize,
    activity: Mutex<VecDeque<Activity>>,
    /// Échecs de connexion récents par adresse IP (dates en secondes).
    failures: Mutex<HashMap<String, Vec<u64>>>,
    /// Secret du serveur (sels factices des comptes inconnus, sel du hachage des clés).
    secret: Vec<u8>,
}

impl Sync {
    /// Ouvre (ou crée) la base et inscrit WorkTable. `None` si le dossier est inaccessible : la
    /// synchronisation est alors indisponible mais le reste de l'agent fonctionne.
    pub fn open(simulated: bool) -> Option<Arc<Sync>> {
        let dev = cfg!(debug_assertions);
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::var_os("NAS_SYNC_DIR").map(PathBuf::from).unwrap_or_else(|| if dev { base.join("sync") } else { PathBuf::from("/srv/nas/sync") });
        let backup_dir = std::env::var_os("NAS_SYNC_BACKUP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| if dev { base.join("fichiers/sauvegardes/sync") } else { PathBuf::from("/srv/nas/sauvegardes/sync") });
        if let Err(e) = create_private_dir(&dir) {
            eprintln!("sync : dossier {} inaccessible ({e}) — synchronisation désactivée", dir.display());
            return None;
        }
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        let conn = match db::open(&dir.join("sync.db")) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("sync : base impossible à ouvrir ({e}) — synchronisation désactivée");
                return None;
            }
        };
        let secret = db::secret(&conn);
        let sync = Arc::new(Sync {
            db: Mutex::new(conn),
            dir,
            backup_dir,
            simulated: simulated || dev,
            inflight: AtomicUsize::new(0),
            activity: Mutex::new(VecDeque::new()),
            failures: Mutex::new(HashMap::new()),
            secret,
        });
        let s = sync.clone();
        std::thread::spawn(move || s.housekeeping());
        Some(sync)
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Fichier d'une version de document.
    fn blob_path(&self, app: &str, user: u64, doc: &str, version: u64) -> PathBuf {
        self.dir.join(app).join(user.to_string()).join(&doc[..2]).join(format!("{doc}.{version}"))
    }

    fn log(&self, app: &str, login: &str, device: &str, kind: ActivityKind, count: u32) {
        let mut a = self.activity.lock().unwrap_or_else(|e| e.into_inner());
        let time = now();
        // Envois et réceptions rapprochés d'un même poste : une seule ligne.
        if let Some(last) = a.front_mut()
            && last.kind == kind
            && last.app == app
            && last.login == login
            && last.device == device
            && matches!(kind, ActivityKind::Push | ActivityKind::Pull)
            && time.saturating_sub(last.time) < 30
        {
            last.count += count;
            last.time = time;
            return;
        }
        a.push_front(Activity { time, app: app.into(), login: login.into(), device: device.into(), kind, count });
        a.truncate(ACTIVITY);
    }

    /// Tâches de fond : purge des anciennes versions (toutes les heures), sauvegarde la nuit.
    fn housekeeping(&self) {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
            let t = now();
            if t % 3600 < 60 {
                self.purge();
            }
            // 2 h UTC (3 h ou 4 h en France), une fois par jour.
            let last = db::setting(&self.db(), "last_backup").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            if (t / 3600) % 24 == 2
                && t.saturating_sub(last) > 20 * 3600
                && let Err(e) = self.backup()
            {
                eprintln!("sync : sauvegarde ratée ({e})");
                db::set_setting(&self.db(), "backup_error", &e);
            }
        }
    }

    /// Supprime les versions remplacées depuis plus longtemps que la durée d'historique de l'app.
    pub fn purge(&self) {
        let old = db::purgeable(&self.db(), now());
        for (app, user, doc, version) in &old {
            let _ = std::fs::remove_file(self.blob_path(app, *user, doc, *version));
        }
        if !old.is_empty() {
            db::forget_versions(&self.db(), &old);
        }
    }

    /// Copie de la base (sept dernières gardées) et miroir des fichiers de versions (jamais
    /// modifiés une fois écrits : seuls les nouveaux sont copiés).
    pub fn backup(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.backup_dir).map_err(|e| e.to_string())?;
        let t = now();
        let name = format!("sync-{t}.db");
        let target = self.backup_dir.join(&name);
        self.db().execute("VACUUM INTO ?1", [target.to_string_lossy()]).map_err(|e| e.to_string())?;
        // Rotation des copies de la base.
        let mut dbs: Vec<PathBuf> = std::fs::read_dir(&self.backup_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("sync-") && n.ends_with(".db")))
            .collect();
        dbs.sort();
        while dbs.len() > BACKUPS {
            let _ = std::fs::remove_file(dbs.remove(0));
        }
        mirror(&self.dir, &self.backup_dir.join("versions"))?;
        let db = self.db();
        db::set_setting(&db, "last_backup", &t.to_string());
        db::set_setting(&db, "backup_error", "");
        Ok(())
    }

    /// Réserve une place parmi les demandes en cours ; `None` si le serveur est saturé.
    fn enter(&self) -> Option<Inflight<'_>> {
        if self.inflight.fetch_add(1, Ordering::SeqCst) >= MAX_INFLIGHT {
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Inflight(&self.inflight))
    }

    /// Hachage d'une clé d'authentification (déjà dérivée du mot de passe par l'app, donc longue et
    /// aléatoire) : Argon2id léger, le Mac mini est lent.
    fn hash_auth(&self, login: &str, auth_key: &str) -> Option<String> {
        let key = from_hex(auth_key).filter(|k| k.len() == 32)?;
        let salt = sha256(&[&self.secret[..], b"auth", login.as_bytes()].concat());
        let params = argon2::Params::new(8 * 1024, 2, 1, Some(32)).ok()?;
        let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let mut out = [0u8; 32];
        a.hash_password_into(&key, &salt[..16], &mut out).ok()?;
        Some(hex(&out))
    }
}

/// Place réservée parmi les demandes en cours, rendue à la fin.
struct Inflight<'a>(&'a AtomicUsize);

impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// Octets aléatoires du système.
fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("générateur aléatoire du système");
    b
}

/// Comparaison en temps constant (ne dit pas à quel caractère deux secrets diffèrent).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Code d'invitation lisible : `ABCD-EFGH-JKLM` (sans 0/O ni 1/I).
fn invite_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let r = random(12);
    let s: String = r.iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect();
    format!("{}-{}-{}", &s[..4], &s[4..8], &s[8..])
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Copie dans `to` les fichiers de `from` qui n'y sont pas encore (hors base SQLite).
fn mirror(from: &Path, to: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("sync.db") {
            continue;
        }
        let target = to.join(&name);
        if path.is_dir() {
            mirror(&path, &target)?;
        } else if !target.exists() {
            std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
            std::fs::copy(&path, &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(from_hex(&hex(&[0, 15, 255])), Some(vec![0, 15, 255]));
        assert_eq!(from_hex("zz"), None);
        assert_eq!(from_hex("abc"), None);
    }

    #[test]
    fn invite_format() {
        let c = invite_code();
        assert_eq!(c.len(), 14);
        assert_eq!(c.matches('-').count(), 2);
    }

    #[test]
    fn constant_time_eq() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }
}
