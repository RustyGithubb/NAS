//! Mises à jour de la console depuis les versions publiées sur GitHub (Réglages).
//!
//! - Au lancement, l'app interroge la dernière version publiée (`releases/latest`) du dépôt
//!   indiqué dans `Cargo.toml` (`repository`). Si elle est plus récente, une pastille apparaît
//!   sur le bouton Réglages.
//! - « Installer » télécharge l'archive Windows produite par `dist`
//!   (`nas-console-x86_64-pc-windows-msvc.zip`), vérifie son empreinte SHA-256 (fichier `.sha256`
//!   publié à côté), en extrait `nas-console.exe` et remplace l'exécutable en cours. La nouvelle
//!   version démarre au prochain lancement (bouton « Redémarrer »).
//! - Les réglages (`%APPDATA%\NAS`) et la clé d'accès (coffre du système) ne sont jamais touchés.
//! - Seule la console est concernée : l'agent du serveur a sa propre mise à jour.
//!
//! Le dépôt doit être **public** : sans jeton, GitHub répond « introuvable » pour un dépôt privé.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

/// Archive publiée pour cette plateforme (seul Windows sait s'installer depuis l'app).
const ASSET: &str = "nas-console-x86_64-pc-windows-msvc.zip";
/// Nom de l'exécutable dans l'archive.
const EXE: &str = "nas-console.exe";

/// Version publiée sur GitHub.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// Page de la version sur GitHub (notes, téléchargements).
    pub page: String,
    archive: Option<String>,
    checksum: Option<String>,
}

#[derive(Clone, Debug)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release),
    /// Nouvelle version installée : elle démarre au prochain lancement.
    Ready(String),
    Failed(String),
}

#[derive(Default)]
struct Shared {
    state: Option<State>,
    checked_at: Option<chrono::DateTime<chrono::Local>>,
}

/// État des mises à jour, partagé avec le fil de vérification / téléchargement.
#[derive(Clone, Default)]
pub struct Updater {
    shared: Arc<Mutex<Shared>>,
}

impl Updater {
    pub fn state(&self) -> State {
        self.shared.lock().map(|s| s.state.clone().unwrap_or(State::Idle)).unwrap_or(State::Idle)
    }

    pub fn checked_at(&self) -> Option<chrono::DateTime<chrono::Local>> {
        self.shared.lock().ok().and_then(|s| s.checked_at)
    }

    /// Pastille sur le bouton Réglages.: une mise à jour attend d'être installée ou appliquée.
    pub fn badge(&self) -> bool {
        matches!(self.state(), State::Available(_) | State::Ready(_))
    }

    fn set(&self, ctx: &egui::Context, state: State) {
        if let Ok(mut s) = self.shared.lock() {
            if matches!(state, State::UpToDate | State::Available(_)) {
                s.checked_at = Some(chrono::Local::now());
            }
            s.state = Some(state);
        }
        ctx.request_repaint();
    }

    fn busy(&self) -> bool {
        matches!(self.state(), State::Checking | State::Downloading(_))
    }

    /// Cherche une version plus récente (en arrière-plan).
    pub fn check(&self, ctx: &egui::Context) {
        if self.busy() || matches!(self.state(), State::Ready(_)) {
            return;
        }
        self.set(ctx, State::Checking);
        let (me, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            let state = match latest() {
                Ok(r) if is_newer(&r.version, env!("CARGO_PKG_VERSION")) => State::Available(r),
                Ok(_) => State::UpToDate,
                Err(e) => State::Failed(e),
            };
            me.set(&ctx, state);
        });
    }

    /// Télécharge et installe la version disponible (en arrière-plan).
    pub fn install(&self, ctx: &egui::Context) {
        let State::Available(release) = self.state() else { return };
        self.set(ctx, State::Downloading(release.clone()));
        let (me, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            let state = match download_and_replace(&release) {
                Ok(()) => State::Ready(release.version.clone()),
                Err(e) => State::Failed(e),
            };
            me.set(&ctx, state);
        });
    }
}

/// L'installation depuis l'app est-elle possible ici ? Sinon, la raison.
pub fn install_blocker() -> Option<&'static str> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Some(t!("Installation automatique disponible sous Windows uniquement.", "Automatic install is only available on Windows."));
    }
    if cfg!(debug_assertions) {
        return Some(t!("En développement (cargo run), l'exécutable n'est pas remplacé.", "In development (cargo run), the executable is not replaced."));
    }
    None
}

/// Relance l'application (nouvel exécutable) puis ferme celle-ci.
pub fn restart(ctx: &egui::Context) {
    if let Ok(exe) = std::env::current_exe() {
        // La nouvelle instance attend que celle-ci soit fermée (instance unique).
        let _ = std::process::Command::new(exe).arg(crate::instance::AFTER_UPDATE).spawn();
    }
    // Fermeture normale : les réglages en attente sont enregistrés (`on_exit`).
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}

// --- Réseau -----------------------------------------------------------------------

/// `propriétaire/dépôt`, tiré de `repository` dans Cargo.toml.
fn repo() -> &'static str {
    env!("CARGO_PKG_REPOSITORY").trim_end_matches('/').trim_end_matches(".git").trim_start_matches("https://github.com/")
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(timeout)).try_proxy_from_env(true).user_agent(concat!("NAS-console/", env!("CARGO_PKG_VERSION"))).build()
}

fn net_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(404, _) => t!(
            "Aucune version publiée trouvée. Le dépôt GitHub doit être public pour les mises à jour.",
            "No published release found. The GitHub repository must be public for updates."
        )
        .into(),
        ureq::Error::Status(403 | 429, _) => t!("GitHub limite les vérifications : réessayez dans une heure.", "GitHub is rate-limiting checks: try again in an hour.").into(),
        ureq::Error::Status(code, _) => tf!("GitHub a répondu {code}.", "GitHub answered {code}."),
        ureq::Error::Transport(t) => tf!("Réseau : {t}", "Network: {t}"),
    }
}

/// Dernière version publiée (hors préversions).
fn latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo());
    let json: serde_json::Value = agent(20).get(&url).set("Accept", "application/vnd.github+json").call().map_err(net_err)?.into_json().map_err(|e| e.to_string())?;
    parse_release(&json).ok_or_else(|| t!("Réponse de GitHub illisible.", "Unreadable answer from GitHub.").into())
}

fn parse_release(json: &serde_json::Value) -> Option<Release> {
    let tag = json["tag_name"].as_str()?;
    let asset = |name: &str| json["assets"].as_array()?.iter().find(|a| a["name"].as_str() == Some(name)).and_then(|a| a["browser_download_url"].as_str()).map(str::to_string);
    Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        page: json["html_url"].as_str().unwrap_or_default().to_string(),
        archive: asset(ASSET),
        checksum: asset(&format!("{ASSET}.sha256")),
    })
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    agent(300).get(url).call().map_err(net_err)?.into_reader().read_to_end(&mut bytes).map_err(|e| tf!("Téléchargement interrompu : {e}", "Download interrupted: {e}"))?;
    Ok(bytes)
}

fn download_and_replace(r: &Release) -> Result<(), String> {
    if let Some(why) = install_blocker() {
        return Err(why.into());
    }
    let url = r.archive.as_deref().ok_or_else(|| tf!("La version {} ne contient pas d'archive Windows.", "Release {} has no Windows archive.", r.version))?;
    // Empreinte obligatoire : sans elle, impossible de vérifier l'archive, on n'installe pas.
    let sum_url = r.checksum.as_deref().ok_or_else(|| {
        tf!(
            "La version {} ne publie pas d'empreinte SHA-256 : installation annulée par sécurité.",
            "Release {} publishes no SHA-256 checksum: installation cancelled for safety.",
            r.version
        )
    })?;
    let zip = download(url)?;
    let expected = String::from_utf8_lossy(&download(sum_url)?).split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if expected.len() != 64 || sha256_hex(&zip) != expected {
        return Err(t!("Archive corrompue (empreinte SHA-256 différente) : rien n'a été modifié.", "Corrupted archive (SHA-256 mismatch): nothing was changed.").into());
    }
    let exe = extract_exe(&zip)?;
    let tmp = std::env::temp_dir().join(format!("nas-console-{}.exe", r.version));
    std::fs::write(&tmp, &exe).map_err(|e| e.to_string())?;
    let result = self_replace::self_replace(&tmp).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            t!(
                "Accès refusé : l'app est installée pour tous les utilisateurs. Relancez l'installeur de la nouvelle version.",
                "Access denied: the app is installed for all users. Run the new version's installer instead."
            )
            .to_string()
        } else {
            tf!("Remplacement impossible : {e}", "Could not replace the app: {e}")
        }
    });
    let _ = std::fs::remove_file(&tmp);
    result
}

/// `nas-console.exe`, à la racine de l'archive ou dans un sous-dossier.
fn extract_exe(zip: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| e.to_string())?;
        let full = f.name().map(|n| n.to_string()).unwrap_or_default();
        let name = full.rsplit(['/', '\\']).next().unwrap_or_default().to_ascii_lowercase();
        if f.is_file() && name == EXE {
            let mut bytes = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            return Ok(bytes);
        }
    }
    Err(tf!("{EXE} absent de l'archive.", "{EXE} missing from the archive."))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// `a` est-elle plus récente que `b` ? (versions `x.y.z`, préfixe `v` toléré)
fn is_newer(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim_start_matches('v').split(['-', '+']).next().unwrap_or_default().split('.').map(|p| p.parse().unwrap_or(0)).collect()
    }
    let (a, b) = (parts(a), parts(b));
    let n = a.len().max(b.len());
    let get = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n).map(|i| get(&a, i).cmp(&get(&b, i))).find(|o| o.is_ne()) == Some(std::cmp::Ordering::Greater)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.2.1", "0.2.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.99"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
    }

    #[test]
    fn repo_from_cargo_toml() {
        assert_eq!(repo().split('/').count(), 2);
    }

    #[test]
    fn reads_release_json() {
        let json = serde_json::json!({
            "tag_name": "v0.3.0",
            "html_url": "https://github.com/o/r/releases/tag/v0.3.0",
            "assets": [
                { "name": ASSET, "browser_download_url": "https://x/a.zip" },
                { "name": format!("{ASSET}.sha256"), "browser_download_url": "https://x/a.zip.sha256" }
            ]
        });
        let r = parse_release(&json).unwrap();
        assert_eq!(r.version, "0.3.0");
        assert_eq!(r.archive.as_deref(), Some("https://x/a.zip"));
        assert_eq!(r.checksum.as_deref(), Some("https://x/a.zip.sha256"));
    }

    #[test]
    fn extracts_exe_from_zip() {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            w.start_file("README.md", opts).unwrap();
            w.write_all(b"doc").unwrap();
            w.start_file("nas-console-x86_64-pc-windows-msvc/nas-console.exe", opts).unwrap();
            w.write_all(b"MZ binaire").unwrap();
            w.finish().unwrap();
        }
        assert_eq!(extract_exe(buf.get_ref()).unwrap(), b"MZ binaire");
    }

    #[test]
    fn hashes_sha256() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
