//! Données échangées entre la console (Windows) et l'agent (Mac mini), en JSON sur HTTP.
//!
//! Routes de l'agent :
//! - `GET  /api/status` → [`Status`]
//! - `GET  /api/services` → [`Services`]
//! - `POST /api/services/<unité>/<start|stop|restart>` → [`ActionResult`]
//! - `GET  /api/logs/<unité>?after=<curseur>` → [`Logs`] (curseur encodé pour l'URL)
//! - `GET  /api/sites` → [`Sites`]
//! - `POST /api/sites/<nom>/<online|maintenance|offline|backup>` → [`ActionResult`]
//! - `GET  /api/files/roots` → `Vec<`[`Root`]`>`
//! - `GET  /api/files/list?root=&path=` → [`Listing`]
//! - `GET  /api/files/search?q=` → [`SearchResult`]
//! - `POST /api/files/<mkdir|rename|delete|copy|move>?…` → [`ActionResult`] (voir [`FileOp`])
//! - `GET  /api/files/download?root=&path=` → contenu du fichier
//! - `POST /api/files/upload?root=&path=` (corps = contenu) → [`ActionResult`]
//!
//! Chemins de fichiers : toujours relatifs à un emplacement (`root`), séparés par `/`.

use serde::{Deserialize, Serialize};

/// Version du protocole : la console refuse un agent d'une autre version.
pub const PROTOCOL_VERSION: u32 = 5;

/// Port par défaut de l'agent.
pub const DEFAULT_PORT: u16 = 7878;

/// Nombre maximal de lignes de journal renvoyées par demande.
pub const LOG_LIMIT: usize = 300;

/// État de la machine à un instant donné.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub protocol: u32,
    pub agent_version: String,
    /// Nom de la machine sur le réseau.
    pub hostname: String,
    /// Système (« Debian GNU/Linux 13 », « Windows 11 »…).
    pub os: String,
    /// Temps écoulé depuis le démarrage de la machine (secondes).
    pub uptime_s: u64,
    pub cpu: Cpu,
    pub memory: Memory,
    pub disks: Vec<Disk>,
    /// Température la plus haute relevée (°C) ; `None` si le système ne la donne pas.
    pub temperature_c: Option<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cpu {
    /// Modèle du processeur.
    pub model: String,
    /// Utilisation globale (0 à 100).
    pub usage: f32,
    /// Utilisation de chaque cœur (0 à 100).
    pub per_core: Vec<f32>,
}

/// Mémoire vive et mémoire d'échange (swap), en octets.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Memory {
    pub used: u64,
    pub total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
}

/// Disque ou partition, en octets.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Disk {
    /// Point de montage (« / », « C:\ »).
    pub mount: String,
    /// Système de fichiers (« ext4 », « NTFS »).
    pub fs: String,
    /// Espace réellement occupé par les fichiers (comme la colonne « Utilisé » de `df`).
    pub used: u64,
    pub total: u64,
    /// Espace encore utilisable. Sous Linux, plus petit que `total - used` : une petite part
    /// du disque est réservée à l'administrateur.
    pub available: u64,
}

/// Liste des services (unités systemd).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Services {
    /// Données simulées (agent lancé hors Linux, ou sans systemd).
    pub simulated: bool,
    pub list: Vec<Service>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Service {
    /// Nom de l'unité (« ssh.service »).
    pub name: String,
    pub description: String,
    pub state: ServiceState,
    /// Détail donné par systemd (« running », « exited », « dead »…).
    pub sub: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    Active,
    #[default]
    Inactive,
    Failed,
    /// En train de démarrer ou de s'arrêter.
    Changing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Action {
    /// Mot utilisé dans l'URL et par `systemctl`.
    pub fn verb(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
        }
    }

    pub fn from_verb(v: &str) -> Option<Action> {
        [Action::Start, Action::Stop, Action::Restart].into_iter().find(|a| a.verb() == v)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    /// Message d'erreur de systemd (vide si tout s'est bien passé).
    pub message: String,
}

/// Lignes de journal d'un service.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Logs {
    pub entries: Vec<LogEntry>,
    /// À renvoyer dans `after` pour n'obtenir que les lignes suivantes.
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LogEntry {
    /// Date (millisecondes depuis 1970).
    pub time_ms: i64,
    /// Gravité syslog : 0 = urgence … 3 = erreur, 4 = avertissement … 7 = débogage.
    pub priority: u8,
    pub message: String,
}

/// Sites web hébergés.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Sites {
    /// Données simulées (agent lancé hors du serveur).
    pub simulated: bool,
    pub list: Vec<Site>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Site {
    /// Nom court, sert d'identifiant (« portfolio »).
    pub name: String,
    /// Adresse publique (« dorian-noirel.fr »).
    pub domain: String,
    pub kind: SiteKind,
    pub state: SiteState,
    /// Dossier du site sur le serveur.
    pub root: String,
    /// Dossier des fichiers du site (images, vidéos…), visible aussi dans l'onglet Fichiers.
    pub media_dir: String,
    /// Fin de validité du certificat HTTPS (millisecondes depuis 1970), si connue.
    pub cert_expires_ms: Option<i64>,
    /// Dernière sauvegarde (millisecondes depuis 1970).
    pub last_backup_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteKind {
    /// Pages HTML fixes (page de liens, vitrine…).
    #[default]
    Static,
    /// WordPress (PHP + base de données).
    WordPress,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteState {
    /// Visible par tous.
    Online,
    /// Page « site en maintenance » à la place du site.
    Maintenance,
    /// Plus servi du tout.
    #[default]
    Offline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteAction {
    Set(SiteState),
    /// Sauvegarde immédiate (fichiers + base de données).
    Backup,
}

impl SiteAction {
    pub const ALL: [SiteAction; 4] = [SiteAction::Set(SiteState::Online), SiteAction::Set(SiteState::Maintenance), SiteAction::Set(SiteState::Offline), SiteAction::Backup];

    /// Mot utilisé dans l'URL.
    pub fn verb(self) -> &'static str {
        match self {
            SiteAction::Set(SiteState::Online) => "online",
            SiteAction::Set(SiteState::Maintenance) => "maintenance",
            SiteAction::Set(SiteState::Offline) => "offline",
            SiteAction::Backup => "backup",
        }
    }

    pub fn from_verb(v: &str) -> Option<SiteAction> {
        Self::ALL.into_iter().find(|a| a.verb() == v)
    }
}

/// Emplacement de stockage du serveur (médias des sites, partages, sauvegardes…).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Root {
    /// Nom affiché, sert d'identifiant (« Médias »).
    pub name: String,
    /// Dossier réel sur le serveur (« /srv/nas/medias »), pour information.
    pub path: String,
    /// Espace libre et total du disque qui le contient (octets).
    pub free: u64,
    pub total: u64,
}

/// Élément d'un dossier.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    /// Taille en octets (0 pour un dossier).
    pub size: u64,
    /// Dernière modification (millisecondes depuis 1970).
    pub modified_ms: i64,
}

/// Contenu d'un dossier.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Listing {
    pub root: String,
    pub path: String,
    pub entries: Vec<Entry>,
}

/// Résultat de recherche : un élément et son emplacement.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Hit {
    pub root: String,
    /// Chemin complet de l'élément (dossier parent + nom).
    pub path: String,
    pub entry: Entry,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchResult {
    pub hits: Vec<Hit>,
    /// Il y avait plus de résultats que ceux renvoyés.
    pub truncated: bool,
    /// Nombre d'éléments indexés (tous emplacements confondus).
    pub indexed: usize,
}

/// Opération sur les fichiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileOp {
    /// Nouveau dossier `path` (paramètre `path` = chemin complet du dossier à créer).
    Mkdir,
    /// Renommer `path` en `to` (nouveau nom, sans dossier).
    Rename,
    /// Mettre `path` à la corbeille de son emplacement.
    Delete,
    /// Copier `path` dans le dossier `to` de l'emplacement `to_root`.
    Copy,
    /// Déplacer `path` dans le dossier `to` de l'emplacement `to_root`.
    Move,
}

impl FileOp {
    pub const ALL: [FileOp; 5] = [FileOp::Mkdir, FileOp::Rename, FileOp::Delete, FileOp::Copy, FileOp::Move];

    pub fn verb(self) -> &'static str {
        match self {
            FileOp::Mkdir => "mkdir",
            FileOp::Rename => "rename",
            FileOp::Delete => "delete",
            FileOp::Copy => "copy",
            FileOp::Move => "move",
        }
    }

    pub fn from_verb(v: &str) -> Option<FileOp> {
        Self::ALL.into_iter().find(|o| o.verb() == v)
    }
}

/// Dossier de corbeille de chaque emplacement (masqué dans les listes).
pub const TRASH_DIR: &str = ".corbeille";

/// Chemin relatif sans danger : pas de `..`, pas de chemin absolu, pas de lecteur Windows,
/// pas de caractère nul. Le chemin vide désigne la racine de l'emplacement.
pub fn safe_rel_path(path: &str) -> bool {
    !path.contains('\0') && !path.starts_with('/') && !path.contains(':') && path.split(['/', '\\']).all(|part| part != ".." && part != ".")
}

/// Nom de fichier acceptable (un seul élément, pas de séparateur).
pub fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && name != "." && name != ".." && !name.contains(['/', '\\', '\0', ':'])
}

/// Nom de site acceptable : lettres minuscules, chiffres et tirets (il sert de nom de fichier).
pub fn valid_site_name(name: &str) -> bool {
    (1..=40).contains(&name.len()) && !name.starts_with('-') && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Nom d'unité acceptable : protège les commandes `systemctl` / `journalctl` (pas d'option
/// déguisée en nom, pas de caractère inattendu).
pub fn valid_unit(name: &str) -> bool {
    (1..=128).contains(&name.len()) && !name.starts_with('-') && name.ends_with(".service") && name.chars().all(|c| c.is_ascii_alphanumeric() || "@._:-\\".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units() {
        assert!(valid_unit("ssh.service"));
        assert!(valid_unit("wg-quick@wg0.service"));
        assert!(!valid_unit("--help.service"));
        assert!(!valid_unit("ssh"));
        assert!(!valid_unit("a b.service"));
        assert!(!valid_unit("x;rm.service"));
    }

    #[test]
    fn sites() {
        assert!(valid_site_name("portfolio"));
        assert!(valid_site_name("liens-2026"));
        assert!(!valid_site_name("../etc"));
        assert!(!valid_site_name("Portfolio"));
        assert!(!valid_site_name(""));
        for a in SiteAction::ALL {
            assert_eq!(SiteAction::from_verb(a.verb()), Some(a));
        }
    }

    #[test]
    fn paths() {
        assert!(safe_rel_path(""));
        assert!(safe_rel_path("photos/2026/plage.jpg"));
        assert!(!safe_rel_path("../etc/passwd"));
        assert!(!safe_rel_path("a/../../b"));
        assert!(!safe_rel_path("/etc"));
        assert!(!safe_rel_path("C:/Windows"));
        assert!(!safe_rel_path("a\\..\\b"));
        assert!(safe_name("rapport final.pdf"));
        assert!(!safe_name("a/b"));
        assert!(!safe_name(".."));
        for o in FileOp::ALL {
            assert_eq!(FileOp::from_verb(o.verb()), Some(o));
        }
    }

    #[test]
    fn verbs() {
        assert_eq!(Action::from_verb("restart"), Some(Action::Restart));
        assert_eq!(Action::from_verb("kill"), None);
    }
}
