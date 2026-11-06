//! Vrais sites, servis par Caddy (serveur web qui obtient et renouvelle seul les certificats
//! HTTPS). Installation et mise en place : `docs/heberger-un-site.md`.
//!
//! - Liste des sites : `/etc/nas-agent/sites.json` (`NAS_AGENT_SITES` pour un autre fichier),
//!   tenu à jour par l'agent (état, dernière sauvegarde).
//! - Chaque site en ligne ou en maintenance a son fichier `/etc/caddy/sites/<nom>.caddy`,
//!   importé par le Caddyfile principal ; « hors ligne » = fichier retiré. Après chaque
//!   changement : `systemctl reload caddy` (Caddy relit sa configuration sans couper les autres sites).
//! - Sauvegardes : `/srv/nas/sauvegardes/sites/<nom>/<date>/` (fichiers + médias, et base de
//!   données pour WordPress).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use nas_proto::{Site, SiteAction, SiteKind, SiteState};
use serde::{Deserialize, Serialize};

const CADDY_SITES: &str = "/etc/caddy/sites";
const CADDY_CERTS: &str = "/var/lib/caddy/.local/share/caddy/certificates";
const MAINTENANCE_DIR: &str = "/var/lib/nas-agent/maintenance";
const BACKUPS: &str = "/srv/nas/sauvegardes/sites";
/// Socket de PHP-FPM (paquet `php-fpm` de Debian).
const PHP_SOCKET: &str = "unix//run/php/php-fpm.sock";

/// Un site tel qu'enregistré dans `sites.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SiteConfig {
    pub name: String,
    pub domain: String,
    pub kind: SiteKind,
    #[serde(default)]
    pub state: SiteState,
    pub root: String,
    pub media_dir: String,
    /// Base de données MariaDB (WordPress).
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub last_backup_ms: Option<i64>,
}

pub struct Caddy {
    file: PathBuf,
    /// Un seul changement à la fois (fichier de configuration partagé).
    lock: Mutex<()>,
}

impl Caddy {
    pub fn new() -> Self {
        let file = std::env::var("NAS_AGENT_SITES").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/etc/nas-agent/sites.json"));
        Caddy { file, lock: Mutex::new(()) }
    }

    fn load(&self) -> Result<Vec<SiteConfig>, String> {
        match std::fs::read_to_string(&self.file) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{} : {e}", self.file.display())),
            // Pas encore de fichier : aucun site.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(format!("{} : {e}", self.file.display())),
        }
    }

    fn save(&self, sites: &[SiteConfig]) -> Result<(), String> {
        let s = serde_json::to_string_pretty(sites).map_err(|e| e.to_string())?;
        std::fs::write(&self.file, s).map_err(|e| format!("{} : {e}", self.file.display()))
    }
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd} : {e}"))?;
    if out.status.success() { Ok(String::from_utf8_lossy(&out.stdout).into_owned()) } else { Err(format!("{cmd} : {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

/// Configuration Caddy d'un site pour un état donné (`None` = hors ligne, pas de fichier).
pub fn caddyfile(s: &SiteConfig, state: SiteState) -> Option<String> {
    let log = format!("\tlog {{\n\t\toutput file /var/log/caddy/{}.log\n\t}}\n", s.name);
    let body = match (state, s.kind) {
        (SiteState::Offline, _) => return None,
        (SiteState::Maintenance, _) => {
            format!("\troot * {MAINTENANCE_DIR}\n\trewrite * /index.html\n\theader Retry-After 3600\n\tfile_server {{\n\t\tstatus 503\n\t}}\n")
        }
        (SiteState::Online, SiteKind::Static) => format!("\troot * {}\n\tencode zstd gzip\n\tfile_server\n", s.root),
        (SiteState::Online, SiteKind::WordPress) => format!(
            "\troot * {}\n\tencode zstd gzip\n\
             \t# Fichiers sensibles de WordPress : jamais servis.\n\
             \t@blocked path /wp-config.php /xmlrpc.php /wp-content/debug.log\n\
             \trespond @blocked 403\n\
             \tphp_fastcgi {PHP_SOCKET}\n\tfile_server\n",
            s.root
        ),
    };
    Some(format!("# Généré par l'agent NAS : ne pas modifier à la main.\n{} {{\n{body}{log}}}\n", s.domain))
}

/// Page affichée pendant la maintenance.
const MAINTENANCE_HTML: &str = "<!doctype html><html lang=\"fr\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">\
<title>Maintenance</title><body style=\"font-family:system-ui;background:#111;color:#eee;display:grid;place-items:center;height:100vh;margin:0\">\
<div style=\"text-align:center\"><h1>Site en maintenance</h1><p>Retour très bientôt.</p></div></body></html>\n";

/// Fin de validité du certificat HTTPS du domaine (lu avec `openssl`), en millisecondes.
fn cert_expiry(domain: &str) -> Option<i64> {
    let dirs = std::fs::read_dir(CADDY_CERTS).ok()?;
    for issuer in dirs.flatten() {
        let crt = issuer.path().join(domain).join(format!("{domain}.crt"));
        if crt.exists() {
            let out = run("openssl", &["x509", "-enddate", "-noout", "-in", &crt.to_string_lossy()]).ok()?;
            let date = out.trim().strip_prefix("notAfter=")?;
            let secs: i64 = run("date", &["-d", date, "+%s"]).ok()?.trim().parse().ok()?;
            return Some(secs * 1000);
        }
    }
    None
}

impl super::SitesBackend for Caddy {
    fn simulated(&self) -> bool {
        false
    }

    fn list(&self) -> Result<Vec<Site>, String> {
        Ok(self
            .load()?
            .into_iter()
            .map(|c| Site {
                cert_expires_ms: cert_expiry(&c.domain),
                name: c.name,
                domain: c.domain,
                kind: c.kind,
                state: c.state,
                root: c.root,
                media_dir: c.media_dir,
                last_backup_ms: c.last_backup_ms,
            })
            .collect())
    }

    fn action(&self, name: &str, action: SiteAction) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        let mut sites = self.load()?;
        let site = sites.iter_mut().find(|s| s.name == name).ok_or_else(|| format!("site {name} inconnu"))?;
        match action {
            SiteAction::Set(state) => {
                let file = Path::new(CADDY_SITES).join(format!("{name}.caddy"));
                match caddyfile(site, state) {
                    Some(conf) => {
                        if state == SiteState::Maintenance {
                            std::fs::create_dir_all(MAINTENANCE_DIR).map_err(|e| e.to_string())?;
                            std::fs::write(Path::new(MAINTENANCE_DIR).join("index.html"), MAINTENANCE_HTML).map_err(|e| e.to_string())?;
                        }
                        std::fs::create_dir_all(CADDY_SITES).map_err(|e| e.to_string())?;
                        std::fs::write(&file, conf).map_err(|e| format!("{} : {e}", file.display()))?;
                    }
                    None => {
                        let _ = std::fs::remove_file(&file);
                    }
                }
                run("systemctl", &["reload", "caddy"])?;
                site.state = state;
            }
            SiteAction::Backup => {
                let stamp = run("date", &["+%Y%m%d-%H%M%S"])?.trim().to_string();
                let dir = Path::new(BACKUPS).join(name).join(&stamp);
                std::fs::create_dir_all(&dir).map_err(|e| format!("{} : {e}", dir.display()))?;
                let d = dir.to_string_lossy().into_owned();
                run("tar", &["-czf", &format!("{d}/site.tar.gz"), "-C", &site.root, "."])?;
                run("tar", &["-czf", &format!("{d}/medias.tar.gz"), "-C", &site.media_dir, "."])?;
                if let Some(db) = &site.database {
                    let dump = run("mysqldump", &["--single-transaction", "--", db])?;
                    std::fs::write(dir.join("base.sql"), dump).map_err(|e| e.to_string())?;
                }
                site.last_backup_ms = Some(crate::backend::now_ms());
            }
        }
        self.save(&sites)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(kind: SiteKind) -> SiteConfig {
        SiteConfig {
            name: "portfolio".into(),
            domain: "exemple.fr".into(),
            kind,
            state: SiteState::Offline,
            root: "/srv/nas/sites/portfolio".into(),
            media_dir: "/srv/nas/medias/portfolio".into(),
            database: Some("wordpress".into()),
            last_backup_ms: None,
        }
    }

    #[test]
    fn caddy_config() {
        let wp = site(SiteKind::WordPress);
        assert!(caddyfile(&wp, SiteState::Offline).is_none());
        let on = caddyfile(&wp, SiteState::Online).unwrap();
        assert!(on.starts_with("# Généré") && on.contains("exemple.fr {"));
        assert!(on.contains("php_fastcgi") && on.contains("respond @blocked 403"));
        assert!(on.contains("/var/log/caddy/portfolio.log"));
        let m = caddyfile(&wp, SiteState::Maintenance).unwrap();
        assert!(m.contains("status 503") && !m.contains("php_fastcgi"));
        let st = caddyfile(&site(SiteKind::Static), SiteState::Online).unwrap();
        assert!(st.contains("file_server") && !st.contains("php_fastcgi"));
        // Accolades équilibrées.
        for c in [on, m, st] {
            assert_eq!(c.matches('{').count(), c.matches('}').count());
        }
    }

    #[test]
    fn config_file_roundtrip() {
        let json = r#"[{"name":"liens","domain":"liens.exemple.fr","kind":"Static","root":"/srv/a","media_dir":"/srv/b"}]"#;
        let v: Vec<SiteConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(v[0].state, SiteState::Offline);
        assert!(v[0].database.is_none());
    }
}
