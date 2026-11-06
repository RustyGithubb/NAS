//! Données échangées entre la console (Windows) et l'agent (Mac mini), en JSON sur HTTP.
//!
//! Routes de l'agent :
//! - `GET  /api/status` → [`Status`]
//! - `GET  /api/services` → [`Services`]
//! - `POST /api/services/<unité>/<start|stop|restart>` → [`ActionResult`]
//! - `GET  /api/logs/<unité>?after=<curseur>` → [`Logs`] (curseur encodé pour l'URL)

use serde::{Deserialize, Serialize};

/// Version du protocole : la console refuse un agent d'une autre version.
pub const PROTOCOL_VERSION: u32 = 2;

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
    pub used: u64,
    pub total: u64,
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
    fn verbs() {
        assert_eq!(Action::from_verb("restart"), Some(Action::Restart));
        assert_eq!(Action::from_verb("kill"), None);
    }
}
