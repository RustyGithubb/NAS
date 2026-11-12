//! Base SQLite de la synchronisation : schéma et requêtes.

use std::path::Path;

use nas_proto::sync::{AppForm, AppInfo, Device, DocMeta, UserApp, UserInfo};
use rusqlite::{Connection, OptionalExtension, params};

use super::now;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS apps (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL, icon TEXT NOT NULL,
    active INTEGER NOT NULL, builtin INTEGER NOT NULL, min_protocol INTEGER NOT NULL,
    default_quota INTEGER NOT NULL, history_days INTEGER NOT NULL, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY, login TEXT NOT NULL UNIQUE, active INTEGER NOT NULL, created INTEGER NOT NULL,
    salt TEXT NOT NULL, auth_hash TEXT, wrapped_key TEXT, recovery_key TEXT,
    invite_hash TEXT, invite_expires INTEGER, last_seen INTEGER, failures INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS access (
    user INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    app TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    active INTEGER NOT NULL, quota INTEGER NOT NULL, cursor INTEGER NOT NULL DEFAULT 0, last_sync INTEGER,
    PRIMARY KEY (user, app));
CREATE TABLE IF NOT EXISTS devices (
    id INTEGER PRIMARY KEY, user INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL, app TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE,
    created INTEGER NOT NULL, last_seen INTEGER NOT NULL, ip TEXT NOT NULL, expires INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS docs (
    user INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    app TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    doc TEXT NOT NULL, version INTEGER NOT NULL, hash TEXT NOT NULL, size INTEGER NOT NULL,
    deleted INTEGER NOT NULL, device TEXT NOT NULL, time INTEGER NOT NULL,
    PRIMARY KEY (user, app, doc, version));
CREATE INDEX IF NOT EXISTS docs_cursor ON docs (user, app, version);
";

/// 20 Go.
pub const DEFAULT_QUOTA: u64 = 20 * 1024 * 1024 * 1024;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.execute_batch(SCHEMA)?;
    // WorkTable, première app, inscrite d'office.
    c.execute(
        "INSERT OR IGNORE INTO apps VALUES (?1, 'WorkTable', 'Organisation de travail : notes, tâches, calendrier, mail, Blueprints', '1f5c2', 1, 1, 1, ?2, 30, ?3)",
        params![nas_proto::sync::WORKTABLE, DEFAULT_QUOTA as i64, now() as i64],
    )?;
    if setting(&c, "enabled").is_none() {
        set_setting(&c, "enabled", "1");
    }
    Ok(c)
}

pub fn setting(c: &Connection, key: &str) -> Option<String> {
    c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
}

pub fn set_setting(c: &Connection, key: &str, value: &str) {
    let _ = c.execute("INSERT INTO settings VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", [key, value]);
}

/// Secret du serveur, tiré au hasard à la création de la base.
pub fn secret(c: &Connection) -> Vec<u8> {
    if let Some(s) = setting(c, "secret").and_then(|s| super::from_hex(&s)) {
        return s;
    }
    let s = super::random(32);
    set_setting(c, "secret", &super::hex(&s));
    s
}

// ───────────────────────────── Apps ─────────────────────────────

pub fn apps(c: &Connection) -> rusqlite::Result<Vec<AppInfo>> {
    let mut st = c.prepare(
        "SELECT a.id, a.name, a.description, a.icon, a.active, a.builtin, a.min_protocol, a.default_quota, a.history_days, a.created,
                (SELECT COALESCE(SUM(size), 0) FROM docs d WHERE d.app = a.id),
                (SELECT COUNT(*) FROM access x WHERE x.app = a.id),
                (SELECT MAX(last_sync) FROM access x WHERE x.app = a.id)
         FROM apps a ORDER BY a.builtin DESC, a.name",
    )?;
    let rows = st.query_map([], |r| {
        Ok(AppInfo {
            id: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
            icon: r.get(3)?,
            active: r.get(4)?,
            builtin: r.get(5)?,
            min_protocol: r.get(6)?,
            default_quota: r.get::<_, i64>(7)? as u64,
            history_days: r.get(8)?,
            created: r.get::<_, i64>(9)? as u64,
            used: r.get::<_, i64>(10)? as u64,
            files: 0,
            users: r.get(11)?,
            last_sync: r.get::<_, Option<i64>>(12)?.map(|v| v as u64),
        })
    })?;
    let mut list = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    for a in &mut list {
        a.files = live_files(c, None, &a.id)?;
    }
    Ok(list)
}

pub fn app(c: &Connection, id: &str) -> Option<AppInfo> {
    apps(c).ok()?.into_iter().find(|a| a.id == id)
}

pub fn add_app(c: &Connection, f: &AppForm) -> rusqlite::Result<usize> {
    c.execute(
        "INSERT INTO apps VALUES (?1, ?2, ?3, ?4, 1, 0, ?5, ?6, ?7, ?8)",
        params![f.id, f.name, f.description, f.icon, f.min_protocol.max(1), f.default_quota as i64, f.history_days, now() as i64],
    )
}

pub fn update_app(c: &Connection, f: &AppForm) -> rusqlite::Result<usize> {
    c.execute(
        "UPDATE apps SET name = ?2, description = ?3, icon = ?4, min_protocol = ?5, default_quota = ?6, history_days = ?7 WHERE id = ?1",
        params![f.id, f.name, f.description, f.icon, f.min_protocol.max(1), f.default_quota as i64, f.history_days],
    )
}

pub fn set_app_active(c: &Connection, id: &str, active: bool) -> rusqlite::Result<usize> {
    c.execute("UPDATE apps SET active = ?2 WHERE id = ?1", params![id, active])
}

pub fn delete_app(c: &Connection, id: &str) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM apps WHERE id = ?1 AND builtin = 0", [id])
}

// ───────────────────────────── Comptes ─────────────────────────────

/// Compte vu par le serveur (ce qui sert à l'authentification).
pub struct Account {
    pub id: u64,
    pub login: String,
    pub active: bool,
    pub salt: String,
    pub auth_hash: Option<String>,
    pub wrapped_key: Option<String>,
    pub invite_hash: Option<String>,
    pub invite_expires: Option<u64>,
}

pub fn account(c: &Connection, login: &str) -> Option<Account> {
    c.query_row("SELECT id, login, active, salt, auth_hash, wrapped_key, invite_hash, invite_expires FROM users WHERE login = ?1", [login], |r| {
        Ok(Account {
            id: r.get::<_, i64>(0)? as u64,
            login: r.get(1)?,
            active: r.get(2)?,
            salt: r.get(3)?,
            auth_hash: r.get(4)?,
            wrapped_key: r.get(5)?,
            invite_hash: r.get(6)?,
            invite_expires: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        })
    })
    .optional()
    .ok()
    .flatten()
}

pub fn account_by_id(c: &Connection, id: u64) -> Option<Account> {
    let login: String = c.query_row("SELECT login FROM users WHERE id = ?1", [id as i64], |r| r.get(0)).optional().ok().flatten()?;
    account(c, &login)
}

/// Crée un compte en attente d'invitation. Renvoie son identifiant.
pub fn add_user(c: &Connection, login: &str, salt: &str, invite_hash: &str, expires: u64) -> rusqlite::Result<u64> {
    c.execute(
        "INSERT INTO users (login, active, created, salt, invite_hash, invite_expires) VALUES (?1, 1, ?2, ?3, ?4, ?5)",
        params![login, now() as i64, salt, invite_hash, expires as i64],
    )?;
    Ok(c.last_insert_rowid() as u64)
}

/// Nouvelle invitation : le mot de passe et les postes sont oubliés (les données restent, mais
/// ne sont lisibles qu'avec le code de récupération).
pub fn reset_user(c: &Connection, id: u64, salt: &str, invite_hash: &str, expires: u64) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM devices WHERE user = ?1", [id as i64])?;
    c.execute(
        "UPDATE users SET salt = ?2, auth_hash = NULL, wrapped_key = NULL, invite_hash = ?3, invite_expires = ?4, failures = 0 WHERE id = ?1",
        params![id as i64, salt, invite_hash, expires as i64],
    )
}

pub fn activate_user(c: &Connection, id: u64, auth_hash: &str, wrapped: &str, recovery: &str) -> rusqlite::Result<usize> {
    c.execute(
        "UPDATE users SET auth_hash = ?2, wrapped_key = ?3, recovery_key = ?4, invite_hash = NULL, invite_expires = NULL, failures = 0, last_seen = ?5 WHERE id = ?1",
        params![id as i64, auth_hash, wrapped, recovery, now() as i64],
    )
}

pub fn set_password(c: &Connection, id: u64, salt: &str, auth_hash: &str, wrapped: &str) -> rusqlite::Result<usize> {
    c.execute("UPDATE users SET salt = ?2, auth_hash = ?3, wrapped_key = ?4 WHERE id = ?1", params![id as i64, salt, auth_hash, wrapped])
}

pub fn set_user_active(c: &Connection, id: u64, active: bool) -> rusqlite::Result<usize> {
    c.execute("UPDATE users SET active = ?2 WHERE id = ?1", params![id as i64, active])
}

pub fn delete_user(c: &Connection, id: u64) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM users WHERE id = ?1", [id as i64])
}

pub fn login_result(c: &Connection, id: u64, ok: bool) {
    let _ = if ok {
        c.execute("UPDATE users SET failures = 0, last_seen = ?2 WHERE id = ?1", params![id as i64, now() as i64])
    } else {
        c.execute("UPDATE users SET failures = failures + 1 WHERE id = ?1", [id as i64])
    };
}

pub fn users(c: &Connection) -> rusqlite::Result<Vec<UserInfo>> {
    let mut st = c.prepare("SELECT id, login, active, auth_hash IS NULL, invite_expires, created, last_seen, failures FROM users ORDER BY login")?;
    let rows = st.query_map([], |r| {
        Ok(UserInfo {
            id: r.get::<_, i64>(0)? as u64,
            login: r.get(1)?,
            active: r.get(2)?,
            pending: r.get(3)?,
            invite_expires: r.get::<_, Option<i64>>(4)?.map(|v| v as u64),
            created: r.get::<_, i64>(5)? as u64,
            last_seen: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
            failures: r.get(7)?,
            apps: Vec::new(),
            devices: Vec::new(),
        })
    })?;
    let mut list = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    for u in &mut list {
        u.apps = user_apps(c, u.id)?;
        u.devices = devices(c, u.id)?;
    }
    Ok(list)
}

pub fn user_apps(c: &Connection, user: u64) -> rusqlite::Result<Vec<UserApp>> {
    let mut st = c.prepare(
        "SELECT x.app, x.active, x.quota, x.last_sync, (SELECT COALESCE(SUM(size), 0) FROM docs d WHERE d.user = x.user AND d.app = x.app)
         FROM access x WHERE x.user = ?1 ORDER BY x.app",
    )?;
    let rows = st.query_map([user as i64], |r| {
        Ok(UserApp {
            app: r.get(0)?,
            active: r.get(1)?,
            quota: r.get::<_, i64>(2)? as u64,
            last_sync: r.get::<_, Option<i64>>(3)?.map(|v| v as u64),
            used: r.get::<_, i64>(4)? as u64,
            files: 0,
        })
    })?;
    let mut list = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    for a in &mut list {
        a.files = live_files(c, Some(user), &a.app)?;
    }
    Ok(list)
}

/// Accès actif d'un compte à une app active.
pub fn access(c: &Connection, user: u64, app: &str) -> Option<UserApp> {
    let ok: Option<bool> = c
        .query_row("SELECT x.active AND a.active FROM access x JOIN apps a ON a.id = x.app WHERE x.user = ?1 AND x.app = ?2", params![user as i64, app], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    if ok != Some(true) {
        return None;
    }
    user_apps(c, user).ok()?.into_iter().find(|a| a.app == app)
}

pub fn grant(c: &Connection, user: u64, app: &str, active: bool, quota: Option<u64>) -> rusqlite::Result<usize> {
    let quota = match quota {
        Some(q) => q as i64,
        None => c.query_row("SELECT default_quota FROM apps WHERE id = ?1", [app], |r| r.get::<_, i64>(0))?,
    };
    c.execute(
        "INSERT INTO access (user, app, active, quota) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(user, app) DO UPDATE SET active = ?3, quota = ?4",
        params![user as i64, app, active, quota],
    )
}

/// Retire l'accès et efface les données du compte pour cette app (renvoie les fichiers à supprimer).
pub fn revoke_app(c: &Connection, user: u64, app: &str) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM docs WHERE user = ?1 AND app = ?2", params![user as i64, app])?;
    c.execute("DELETE FROM access WHERE user = ?1 AND app = ?2", params![user as i64, app])
}

// ───────────────────────────── Postes ─────────────────────────────

pub fn devices(c: &Connection, user: u64) -> rusqlite::Result<Vec<Device>> {
    let mut st = c.prepare("SELECT id, name, app, created, last_seen, ip, expires FROM devices WHERE user = ?1 ORDER BY last_seen DESC")?;
    let rows = st.query_map([user as i64], |r| {
        Ok(Device {
            id: r.get::<_, i64>(0)? as u64,
            name: r.get(1)?,
            app: r.get(2)?,
            created: r.get::<_, i64>(3)? as u64,
            last_seen: r.get::<_, i64>(4)? as u64,
            ip: r.get(5)?,
            expires: r.get::<_, i64>(6)? as u64,
        })
    })?;
    rows.collect()
}

pub fn add_device(c: &Connection, user: u64, name: &str, app: &str, token_hash: &str, ip: &str) -> rusqlite::Result<u64> {
    let t = now();
    c.execute(
        "INSERT INTO devices (user, name, app, token_hash, created, last_seen, ip, expires) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)",
        params![user as i64, name, app, token_hash, t as i64, ip, (t + super::SESSION_S) as i64],
    )?;
    Ok(c.last_insert_rowid() as u64)
}

/// Session d'un jeton valide : (compte, login, poste, nom du poste). Prolonge la session.
pub fn session(c: &Connection, token_hash: &str, ip: &str) -> Option<(u64, String, u64, String)> {
    let t = now();
    let s = c
        .query_row(
            "SELECT u.id, u.login, d.id, d.name FROM devices d JOIN users u ON u.id = d.user WHERE d.token_hash = ?1 AND d.expires > ?2 AND u.active = 1",
            params![token_hash, t as i64],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?, r.get::<_, i64>(2)? as u64, r.get::<_, String>(3)?)),
        )
        .optional()
        .ok()
        .flatten()?;
    let _ = c.execute("UPDATE devices SET last_seen = ?2, ip = ?3, expires = ?4 WHERE id = ?1", params![s.2 as i64, t as i64, ip, (t + super::SESSION_S) as i64]);
    let _ = c.execute("UPDATE users SET last_seen = ?2 WHERE id = ?1", params![s.0 as i64, t as i64]);
    Some(s)
}

/// Révoque un poste (de ce compte seulement si `user` est donné).
pub fn revoke_device(c: &Connection, id: u64, user: Option<u64>) -> rusqlite::Result<usize> {
    match user {
        Some(u) => c.execute("DELETE FROM devices WHERE id = ?1 AND user = ?2", params![id as i64, u as i64]),
        None => c.execute("DELETE FROM devices WHERE id = ?1", [id as i64]),
    }
}

// ───────────────────────────── Documents ─────────────────────────────

fn meta(r: &rusqlite::Row) -> rusqlite::Result<DocMeta> {
    Ok(DocMeta {
        doc: r.get(0)?,
        version: r.get::<_, i64>(1)? as u64,
        hash: r.get(2)?,
        size: r.get::<_, i64>(3)? as u64,
        deleted: r.get(4)?,
        device: r.get(5)?,
        time: r.get::<_, i64>(6)? as u64,
    })
}

/// Dernière version d'un document.
pub fn latest(c: &Connection, user: u64, app: &str, doc: &str) -> Option<DocMeta> {
    c.query_row(
        "SELECT doc, version, hash, size, deleted, device, time FROM docs WHERE user = ?1 AND app = ?2 AND doc = ?3 ORDER BY version DESC LIMIT 1",
        params![user as i64, app, doc],
        meta,
    )
    .optional()
    .ok()
    .flatten()
}

/// Enregistre une nouvelle version (le fichier est déjà écrit).
#[allow(clippy::too_many_arguments)]
pub fn add_version(c: &mut Connection, user: u64, app: &str, doc: &str, hash: &str, size: u64, deleted: bool, device: &str, version: u64) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    let t = now() as i64;
    tx.execute("INSERT INTO docs VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", params![user as i64, app, doc, version as i64, hash, size as i64, deleted, device, t])?;
    tx.execute("UPDATE access SET cursor = ?3, last_sync = ?4 WHERE user = ?1 AND app = ?2", params![user as i64, app, version as i64, t])?;
    tx.commit()
}

/// Curseur actuel (dernier numéro de version attribué) d'un compte pour une app.
pub fn cursor(c: &Connection, user: u64, app: &str) -> u64 {
    c.query_row("SELECT cursor FROM access WHERE user = ?1 AND app = ?2", params![user as i64, app], |r| r.get::<_, i64>(0)).map(|v| v as u64).unwrap_or(0)
}

/// Dernières versions des documents changés depuis `since`.
pub fn changes(c: &Connection, user: u64, app: &str, since: u64, limit: usize) -> rusqlite::Result<Vec<DocMeta>> {
    let mut st = c.prepare(
        "SELECT doc, version, hash, size, deleted, device, time FROM docs d
         WHERE user = ?1 AND app = ?2 AND version > ?3
           AND version = (SELECT MAX(version) FROM docs e WHERE e.user = d.user AND e.app = d.app AND e.doc = d.doc)
         ORDER BY version LIMIT ?4",
    )?;
    let rows = st.query_map(params![user as i64, app, since as i64, limit as i64], meta)?;
    rows.collect()
}

/// Documents existants (dernière version non supprimée).
fn live_files(c: &Connection, user: Option<u64>, app: &str) -> rusqlite::Result<u64> {
    let sql = "SELECT COUNT(*) FROM docs d WHERE app = ?1 AND (?2 IS NULL OR user = ?2) AND deleted = 0
               AND version = (SELECT MAX(version) FROM docs e WHERE e.user = d.user AND e.app = d.app AND e.doc = d.doc)";
    c.query_row(sql, params![app, user.map(|u| u as i64)], |r| r.get::<_, i64>(0)).map(|v| v as u64)
}

/// Espace occupé (toutes versions) par un compte dans une app.
pub fn used(c: &Connection, user: u64, app: &str) -> u64 {
    c.query_row("SELECT COALESCE(SUM(size), 0) FROM docs WHERE user = ?1 AND app = ?2", params![user as i64, app], |r| r.get::<_, i64>(0)).map(|v| v as u64).unwrap_or(0)
}

pub fn total_used(c: &Connection) -> u64 {
    c.query_row("SELECT COALESCE(SUM(size), 0) FROM docs", [], |r| r.get::<_, i64>(0)).map(|v| v as u64).unwrap_or(0)
}

/// Versions remplacées depuis plus longtemps que la durée d'historique de leur app.
pub fn purgeable(c: &Connection, now: u64) -> Vec<(String, u64, String, u64)> {
    let sql = "SELECT d.app, d.user, d.doc, d.version FROM docs d JOIN apps a ON a.id = d.app
               WHERE d.version < (SELECT MAX(version) FROM docs e WHERE e.user = d.user AND e.app = d.app AND e.doc = d.doc)
                 AND (SELECT MIN(time) FROM docs e WHERE e.user = d.user AND e.app = d.app AND e.doc = d.doc AND e.version > d.version) < ?1 - a.history_days * 86400
               LIMIT 5000";
    let Ok(mut st) = c.prepare(sql) else { return Vec::new() };
    st.query_map([now as i64], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64, r.get(2)?, r.get::<_, i64>(3)? as u64))).map(|rows| rows.flatten().collect()).unwrap_or_default()
}

pub fn forget_versions(c: &Connection, list: &[(String, u64, String, u64)]) {
    for (app, user, doc, version) in list {
        let _ = c.execute("DELETE FROM docs WHERE app = ?1 AND user = ?2 AND doc = ?3 AND version = ?4", params![app, *user as i64, doc, *version as i64]);
    }
}
