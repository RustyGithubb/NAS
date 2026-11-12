//! Synchronisation des apps (WorkTable et les suivantes) avec le serveur.
//!
//! Le serveur ne connaît pas les apps : il garde, pour chaque app et chaque compte, des
//! « documents » chiffrés et versionnés. Les noms et le contenu sont chiffrés par l'app
//! (le serveur ne voit qu'un identifiant, une taille et un numéro de version).
//!
//! Routes publiques (jeton de session `Authorization: Bearer …`, sauf `auth/salt|register|login`) :
//! - `GET  /sync/auth/salt?login=` → [`Salt`]
//! - `POST /sync/auth/register` ([`Register`], premier login avec le code d'invitation) → [`Session`]
//! - `POST /sync/auth/login` ([`Login`]) → [`Session`]
//! - `POST /sync/auth/logout` → [`Ok`]
//! - `POST /sync/auth/password` ([`ChangePassword`]) → [`Ok`]
//! - `GET  /sync/apps` → `Vec<`[`PublicApp`]`>` (apps autorisées pour ce compte)
//! - `GET  /sync/devices` → `Vec<`[`Device`]`>` ; `POST /sync/devices/<id>/revoke` → [`Ok`]
//! - `POST /sync/<app>/push` ([`Push`]) → [`PushResult`]
//! - `GET  /sync/<app>/pull?since=<curseur>` → [`Pull`]
//! - `GET  /sync/<app>/blob/<doc>/<version>` → contenu chiffré (octets)
//! - `GET  /sync/<app>/status` → [`AppStatus`]
//!
//! Routes d'administration (console, clé d'accès de l'agent) :
//! - `GET  /api/sync` → [`Overview`]
//! - `POST /api/sync/settings` ([`Settings`]) ; `POST /api/sync/backup`
//! - `POST /api/sync/apps` ([`AppForm`]) ; `POST /api/sync/apps/<app>/<update|enable|disable|delete>`
//! - `POST /api/sync/users` ([`NewUser`]) → [`Invitation`]
//! - `POST /api/sync/users/<id>/<enable|disable|delete>` ; `…/reset` → [`Invitation`]
//! - `POST /api/sync/users/<id>/apps/<app>` ([`Grant`]) ; `…/apps/<app>/remove`
//! - `POST /api/sync/devices/<id>/revoke`
//!
//! Les actions d'administration répondent [`crate::ActionResult`] (sauf mention contraire).

use serde::{Deserialize, Serialize};

/// Version du protocole de synchronisation (indépendante de celle de la console).
pub const SYNC_PROTOCOL_VERSION: u32 = 1;

/// Identifiant de la première app, inscrite d'office.
pub const WORKTABLE: &str = "worktable";

/// Taille maximale d'un document envoyé en une fois (après chiffrement).
pub const MAX_DOC: u64 = 8 * 1024 * 1024;

/// Nombre maximal de documents renvoyés par un `pull`.
pub const PULL_LIMIT: usize = 500;

/// Identifiant d'app : 2 à 32 caractères `a-z`, `0-9`, `-`, sans tiret au début.
pub fn valid_app_id(id: &str) -> bool {
    (2..=32).contains(&id.len()) && !id.starts_with('-') && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Identifiant de compte : 2 à 32 caractères `a-z`, `0-9`, `.`, `_`, `-`.
pub fn valid_login(login: &str) -> bool {
    (2..=32).contains(&login.len()) && login.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

/// Identifiant de document : 64 caractères hexadécimaux (HMAC-SHA256 du chemin, calculé par l'app).
pub fn valid_doc_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

// ───────────────────────────── Routes publiques ─────────────────────────────

/// Paramètres de dérivation de clé d'un compte (Argon2id, fait par l'app).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Salt {
    /// Sel (hexadécimal).
    pub salt: String,
    /// Mémoire Argon2 (Kio), nombre de passes.
    pub m_kib: u32,
    pub t: u32,
    /// Compte créé mais pas encore activé : il faut le code d'invitation (`register`).
    pub pending: bool,
}

/// Premier login : fixe le mot de passe avec le code d'invitation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Register {
    pub login: String,
    pub invite: String,
    /// Clé d'authentification dérivée du mot de passe (hexadécimal, 32 octets).
    pub auth_key: String,
    /// Clé maître du compte, chiffrée par la clé dérivée du mot de passe (base64).
    pub wrapped_key: String,
    /// Clé maître chiffrée par le code de récupération (base64).
    pub recovery_key: String,
    pub device: DeviceHello,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Login {
    pub login: String,
    pub auth_key: String,
    pub device: DeviceHello,
}

/// Ce que le poste dit de lui à la connexion.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceHello {
    /// Nom du poste (« PC-ECOLE »).
    pub name: String,
    /// App qui se connecte.
    pub app: String,
    pub protocol: u32,
}

/// Session ouverte.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    /// Jeton à envoyer dans `Authorization: Bearer …`.
    pub token: String,
    pub device_id: u64,
    pub login: String,
    pub wrapped_key: String,
    pub apps: Vec<PublicApp>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChangePassword {
    pub old_auth_key: String,
    pub new_salt: String,
    pub new_auth_key: String,
    pub new_wrapped_key: String,
}

/// Réponse simple des routes publiques.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ok {
    pub ok: bool,
}

/// App vue par un compte.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PublicApp {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub quota: u64,
    pub used: u64,
}

/// Envoi d'une nouvelle version d'un document.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Push {
    pub doc: String,
    /// Version sur laquelle l'app a travaillé (0 = nouveau document).
    pub base: u64,
    /// SHA-256 du contenu chiffré (hexadécimal).
    pub hash: String,
    /// Suppression : `data` vide.
    pub deleted: bool,
    /// Contenu chiffré (base64).
    pub data: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PushResult {
    pub ok: bool,
    /// Nouvelle version (ou version actuelle du serveur en cas de conflit).
    pub version: u64,
    /// Le serveur a une autre version que `base` : l'app doit fusionner puis renvoyer.
    pub conflict: Option<DocMeta>,
    pub message: String,
}

/// Dernière version connue d'un document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocMeta {
    pub doc: String,
    pub version: u64,
    pub hash: String,
    pub size: u64,
    pub deleted: bool,
    /// Poste qui l'a envoyée.
    pub device: String,
    /// Date (secondes depuis 1970).
    pub time: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Pull {
    /// À renvoyer dans `since` la prochaine fois.
    pub cursor: u64,
    pub docs: Vec<DocMeta>,
    /// Il en reste : refaire un `pull` tout de suite.
    pub more: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppStatus {
    pub used: u64,
    pub quota: u64,
    pub files: u64,
    pub cursor: u64,
}

// ───────────────────────────── Administration ─────────────────────────────

/// Tout ce qu'affiche l'onglet Synchro de la console, en une demande.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Overview {
    pub protocol: u32,
    /// Stockage dans un dossier de test (agent hors Linux ou en développement).
    pub simulated: bool,
    pub settings: Settings,
    /// Dossier des données sur le serveur.
    pub dir: String,
    pub used: u64,
    /// Espace libre sur le disque des données.
    pub disk_free: u64,
    pub apps: Vec<AppInfo>,
    pub users: Vec<UserInfo>,
    /// Dernières synchronisations, de la plus récente à la plus ancienne.
    pub activity: Vec<Activity>,
    /// Problèmes à afficher en rouge.
    pub alerts: Vec<String>,
    /// Dernière sauvegarde réussie (secondes depuis 1970).
    pub last_backup: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Interrupteur général : coupé, toutes les routes `/sync` répondent 503.
    pub enabled: bool,
    /// Adresse publique (`sync.exemple.fr`), affichée aux comptes.
    pub domain: String,
}

/// Une app du répertoire.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Icône : code Twemoji (« 1f5c2 » = 🗂), parmi ceux de la console (`assets/emoji`).
    pub icon: String,
    pub active: bool,
    /// Inscrite d'office par l'agent (ne peut pas être supprimée).
    pub builtin: bool,
    /// Version minimale du protocole acceptée des clients de cette app.
    pub min_protocol: u32,
    /// Quota par compte (octets) donné aux nouveaux accès.
    pub default_quota: u64,
    /// Durée de conservation des anciennes versions (jours).
    pub history_days: u32,
    pub created: u64,
    pub used: u64,
    pub files: u64,
    /// Comptes autorisés.
    pub users: u32,
    pub last_sync: Option<u64>,
}

/// Inscription ou modification d'une app.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppForm {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub default_quota: u64,
    pub history_days: u32,
    pub min_protocol: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: u64,
    pub login: String,
    pub active: bool,
    /// Invitation pas encore utilisée.
    pub pending: bool,
    /// Fin de validité de l'invitation en cours.
    pub invite_expires: Option<u64>,
    pub created: u64,
    pub last_seen: Option<u64>,
    pub apps: Vec<UserApp>,
    pub devices: Vec<Device>,
    /// Échecs de connexion depuis la dernière réussite.
    pub failures: u32,
}

/// Accès d'un compte à une app.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserApp {
    pub app: String,
    pub active: bool,
    pub quota: u64,
    pub used: u64,
    pub files: u64,
    pub last_sync: Option<u64>,
}

/// Poste connecté à un compte.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Device {
    pub id: u64,
    pub name: String,
    /// App avec laquelle il s'est connecté.
    pub app: String,
    pub created: u64,
    pub last_seen: u64,
    pub ip: String,
    /// Fin de validité du jeton.
    pub expires: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NewUser {
    pub login: String,
    /// Apps autorisées dès la création.
    pub apps: Vec<String>,
}

/// Code à donner à la personne pour son premier login.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Invitation {
    pub ok: bool,
    pub message: String,
    pub user_id: u64,
    pub login: String,
    pub code: String,
    pub expires: u64,
}

/// Autorisation (ou modification) de l'accès d'un compte à une app.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Grant {
    pub active: bool,
    /// `None` : quota par défaut de l'app.
    pub quota: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Activity {
    pub time: u64,
    pub app: String,
    pub login: String,
    pub device: String,
    pub kind: ActivityKind,
    /// Documents envoyés ou reçus.
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityKind {
    #[default]
    Push,
    Pull,
    Login,
    LoginFailed,
    Conflict,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert!(valid_app_id("worktable"));
        assert!(valid_app_id("app-2"));
        assert!(!valid_app_id("-x"));
        assert!(!valid_app_id("Work"));
        assert!(!valid_app_id("../x"));
        assert!(valid_login("dorian.n"));
        assert!(!valid_login("d"));
        assert!(!valid_login("a/b"));
        assert!(valid_doc_id(&"a".repeat(64)));
        assert!(!valid_doc_id(&"A".repeat(64)));
        assert!(!valid_doc_id("abc"));
    }
}
