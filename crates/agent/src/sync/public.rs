//! Routes publiques `/sync/…`, utilisées par les apps (WorkTable…), joignables depuis Internet.

use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nas_proto::sync::{
    self as proto, ActivityKind, AppStatus, ChangePassword, DeviceHello, Login, PublicApp, Pull, Push, PushResult, Register, SYNC_PROTOCOL_VERSION, Salt, Session,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tiny_http::{Header, Method, Request, Response};

use super::{Sync, db, from_hex, hex, now, random, same, sha256};

/// Paramètres Argon2id que les apps utilisent pour dériver leurs clés du mot de passe.
const KDF_M_KIB: u32 = 64 * 1024;
const KDF_T: u32 = 3;

enum Reply {
    Json(u16, String),
    Bytes(Vec<u8>),
}

fn json<T: Serialize>(code: u16, v: &T) -> Reply {
    Reply::Json(code, serde_json::to_string(v).unwrap_or_default())
}

fn error(code: u16, msg: &str) -> Reply {
    json(code, &serde_json::json!({ "error": msg }))
}

/// Session d'un compte connecté.
struct Who {
    user: u64,
    login: String,
    device: u64,
    device_name: String,
}

/// Traite une demande `/sync/…` (le contrôle de la clé de la console ne s'applique pas ici).
pub fn serve(sync: Option<&Sync>, mut request: Request) {
    let reply = match sync {
        None => error(503, "synchronisation indisponible sur ce serveur"),
        Some(s) => match s.enter() {
            None => error(503, "serveur occupé, réessayez"),
            Some(_slot) => {
                if db::setting(&s.db(), "enabled").as_deref() != Some("1") {
                    error(503, "synchronisation coupée par l'administrateur")
                } else {
                    route(s, &mut request)
                }
            }
        },
    };
    let _ = match reply {
        Reply::Json(code, body) => {
            let h = Header::from_bytes("Content-Type", "application/json").expect("en-tête valide");
            request.respond(Response::from_string(body).with_status_code(code).with_header(h))
        }
        Reply::Bytes(b) => {
            let h = Header::from_bytes("Content-Type", "application/octet-stream").expect("en-tête valide");
            request.respond(Response::from_data(b).with_header(h))
        }
    };
}

/// Adresse du client : celle donnée par Caddy si la demande vient de lui.
fn client_ip(request: &Request) -> String {
    let direct = request.remote_addr().map(|a| a.ip());
    if direct.is_some_and(|ip| ip.is_loopback())
        && let Some(h) = request.headers().iter().find(|h| h.field.equiv("X-Forwarded-For"))
    {
        return h.value.as_str().split(',').next().unwrap_or("").trim().to_string();
    }
    direct.map(|ip| ip.to_string()).unwrap_or_default()
}

/// Corps JSON de la demande (taille limitée).
fn body<T: DeserializeOwned>(request: &mut Request, limit: u64) -> Result<T, Reply> {
    let mut buf = Vec::new();
    request.as_reader().take(limit + 1).read_to_end(&mut buf).map_err(|_| error(400, "lecture impossible"))?;
    if buf.len() as u64 > limit {
        return Err(error(413, "demande trop grosse"));
    }
    serde_json::from_slice(&buf).map_err(|e| error(400, &format!("demande invalide : {e}")))
}

fn route(s: &Sync, request: &mut Request) -> Reply {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let ip = client_ip(request);
    let method = request.method().clone();
    match (&method, parts.as_slice()) {
        (Method::Get, ["sync", "auth", "salt"]) => salt(s, &param(query, "login").unwrap_or_default()),
        (Method::Post, ["sync", "auth", "register"]) => match body::<Register>(request, 64 * 1024) {
            Ok(r) => register(s, r, &ip),
            Err(e) => e,
        },
        (Method::Post, ["sync", "auth", "login"]) => match body::<Login>(request, 64 * 1024) {
            Ok(l) => login(s, l, &ip),
            Err(e) => e,
        },
        _ => {
            let Some(who) = authenticate(s, request, &ip) else { return error(401, "session expirée ou révoquée") };
            match (&method, parts.as_slice()) {
                (Method::Post, ["sync", "auth", "logout"]) => {
                    let _ = db::revoke_device(&s.db(), who.device, Some(who.user));
                    json(200, &proto::Ok { ok: true })
                }
                (Method::Post, ["sync", "auth", "password"]) => match body::<ChangePassword>(request, 64 * 1024) {
                    Ok(c) => password(s, &who, c),
                    Err(e) => e,
                },
                (Method::Get, ["sync", "apps"]) => json(200, &public_apps(s, who.user)),
                (Method::Get, ["sync", "devices"]) => match db::devices(&s.db(), who.user) {
                    Ok(d) => json(200, &d),
                    Err(e) => error(500, &e.to_string()),
                },
                (Method::Post, ["sync", "devices", id, "revoke"]) => match id.parse::<u64>() {
                    Ok(id) => match db::revoke_device(&s.db(), id, Some(who.user)) {
                        Ok(n) => json(200, &proto::Ok { ok: n > 0 }),
                        Err(e) => error(500, &e.to_string()),
                    },
                    Err(_) => error(400, "poste invalide"),
                },
                (_, ["sync", app, rest @ ..]) => {
                    let app = *app;
                    if !proto::valid_app_id(app) || db::access(&s.db(), who.user, app).is_none() {
                        return error(403, "app inconnue, coupée ou non autorisée pour ce compte");
                    }
                    match (&method, rest) {
                        (Method::Post, ["push"]) => match body::<Push>(request, proto::MAX_DOC * 4 / 3 + 64 * 1024) {
                            Ok(p) => push(s, &who, app, p),
                            Err(e) => e,
                        },
                        (Method::Get, ["pull"]) => pull(s, &who, app, param(query, "since").and_then(|v| v.parse().ok()).unwrap_or(0)),
                        (Method::Get, ["blob", doc, version]) => blob(s, &who, app, doc, version),
                        (Method::Get, ["status"]) => {
                            let db = s.db();
                            let a = db::access(&db, who.user, app).unwrap_or_default();
                            json(200, &AppStatus { used: a.used, quota: a.quota, files: a.files, cursor: db::cursor(&db, who.user, app) })
                        }
                        _ => error(404, "route inconnue"),
                    }
                }
                _ => error(404, "route inconnue"),
            }
        }
    }
}

fn param(query: &str, key: &str) -> Option<String> {
    query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).and_then(|(_, v)| urlencoding::decode(v).ok()).map(|v| v.into_owned())
}

fn authenticate(s: &Sync, request: &Request, ip: &str) -> Option<Who> {
    let token = request.headers().iter().find(|h| h.field.equiv("Authorization")).and_then(|h| h.value.as_str().strip_prefix("Bearer "))?;
    let hash = hex(&sha256(token.trim().as_bytes()));
    let (user, login, device, device_name) = db::session(&s.db(), &hash, ip)?;
    Some(Who { user, login, device, device_name })
}

fn salt(s: &Sync, login: &str) -> Reply {
    let login = login.trim().to_lowercase();
    match db::account(&s.db(), &login) {
        Some(a) => json(200, &Salt { salt: a.salt, m_kib: KDF_M_KIB, t: KDF_T, pending: a.auth_hash.is_none() }),
        // Compte inconnu : un sel factice mais stable, pour ne pas révéler quels comptes existent.
        None => json(200, &Salt { salt: hex(&sha256(&[&s.secret[..], b"salt", login.as_bytes()].concat())[..16]), m_kib: KDF_M_KIB, t: KDF_T, pending: false }),
    }
}

/// Trop d'échecs récents depuis cette adresse ?
fn throttled(s: &Sync, ip: &str) -> bool {
    let mut f = s.failures.lock().unwrap_or_else(|e| e.into_inner());
    let t = now();
    let list = f.entry(ip.to_string()).or_default();
    list.retain(|x| t.saturating_sub(*x) < 60);
    list.len() >= super::MAX_FAILURES
}

fn failed(s: &Sync, ip: &str, login: &str, app: &str, user: Option<u64>) {
    s.failures.lock().unwrap_or_else(|e| e.into_inner()).entry(ip.to_string()).or_default().push(now());
    if let Some(u) = user {
        db::login_result(&s.db(), u, false);
    }
    s.log(app, login, ip, ActivityKind::LoginFailed, 1);
    // Ligne lue par fail2ban (voir docs/synchro.md).
    eprintln!("sync : échec de connexion pour « {login} » depuis {ip}");
}

fn check_device(d: &DeviceHello, s: &Sync) -> Result<(), Reply> {
    if d.name.trim().is_empty() || d.name.len() > 64 {
        return Err(error(400, "nom de poste invalide"));
    }
    let app = db::app(&s.db(), &d.app).ok_or_else(|| error(403, "app inconnue"))?;
    if !app.active {
        return Err(error(403, "app coupée par l'administrateur"));
    }
    if d.protocol < app.min_protocol || d.protocol > SYNC_PROTOCOL_VERSION {
        return Err(error(426, "version de l'app incompatible : mettez-la à jour"));
    }
    Ok(())
}

fn open_session(s: &Sync, user: u64, login: &str, d: &DeviceHello, ip: &str, wrapped_key: String) -> Reply {
    let token = hex(&random(32));
    let db = s.db();
    match db::add_device(&db, user, d.name.trim(), &d.app, &hex(&sha256(token.as_bytes())), ip) {
        Ok(device_id) => {
            db::login_result(&db, user, true);
            drop(db);
            s.log(&d.app, login, d.name.trim(), ActivityKind::Login, 1);
            json(200, &Session { token, device_id, login: login.into(), wrapped_key, apps: public_apps(s, user) })
        }
        Err(e) => error(500, &e.to_string()),
    }
}

fn register(s: &Sync, r: Register, ip: &str) -> Reply {
    if throttled(s, ip) {
        return error(429, "trop d'essais, attendez une minute");
    }
    if let Err(e) = check_device(&r.device, s) {
        return e;
    }
    let login = r.login.trim().to_lowercase();
    let account = db::account(&s.db(), &login);
    let valid = account.as_ref().is_some_and(|a| {
        a.active
            && a.auth_hash.is_none()
            && a.invite_expires.is_some_and(|e| e > now())
            && a.invite_hash.as_deref().is_some_and(|h| same(h, &hex(&sha256(r.invite.trim().to_uppercase().as_bytes()))))
    });
    let Some(a) = account.filter(|_| valid) else {
        failed(s, ip, &login, &r.device.app, None);
        return error(403, "code d'invitation invalide ou expiré");
    };
    if db::access(&s.db(), a.id, &r.device.app).is_none() {
        return error(403, "cette app n'est pas autorisée pour ce compte");
    }
    if r.wrapped_key.is_empty() || r.recovery_key.is_empty() || B64.decode(&r.wrapped_key).is_err() || B64.decode(&r.recovery_key).is_err() {
        return error(400, "clés manquantes");
    }
    let Some(hash) = s.hash_auth(&login, &r.auth_key) else { return error(400, "clé d'authentification invalide") };
    if let Err(e) = db::activate_user(&s.db(), a.id, &hash, &r.wrapped_key, &r.recovery_key) {
        return error(500, &e.to_string());
    }
    open_session(s, a.id, &login, &r.device, ip, r.wrapped_key)
}

fn login(s: &Sync, l: Login, ip: &str) -> Reply {
    if throttled(s, ip) {
        return error(429, "trop d'essais, attendez une minute");
    }
    if let Err(e) = check_device(&l.device, s) {
        return e;
    }
    let login = l.login.trim().to_lowercase();
    let account = db::account(&s.db(), &login);
    let hash = s.hash_auth(&login, &l.auth_key);
    let ok = match (&account, &hash) {
        (Some(a), Some(h)) => a.active && a.auth_hash.as_deref().is_some_and(|stored| same(stored, h)),
        _ => false,
    };
    let Some(a) = account.filter(|_| ok) else {
        let user = db::account(&s.db(), &login).map(|a| a.id);
        failed(s, ip, &login, &l.device.app, user);
        return error(401, "identifiant ou mot de passe incorrect");
    };
    if db::access(&s.db(), a.id, &l.device.app).is_none() {
        return error(403, "cette app n'est pas autorisée pour ce compte");
    }
    open_session(s, a.id, &login, &l.device, ip, a.wrapped_key.unwrap_or_default())
}

fn password(s: &Sync, who: &Who, c: ChangePassword) -> Reply {
    let Some(a) = db::account_by_id(&s.db(), who.user) else { return error(404, "compte introuvable") };
    let old_ok = s.hash_auth(&a.login, &c.old_auth_key).zip(a.auth_hash).is_some_and(|(h, stored)| same(&h, &stored));
    if !old_ok {
        return error(403, "ancien mot de passe incorrect");
    }
    if from_hex(&c.new_salt).is_none_or(|v| v.len() < 16) || B64.decode(&c.new_wrapped_key).is_err() {
        return error(400, "paramètres invalides");
    }
    let Some(hash) = s.hash_auth(&a.login, &c.new_auth_key) else { return error(400, "clé invalide") };
    match db::set_password(&s.db(), a.id, &c.new_salt, &hash, &c.new_wrapped_key) {
        Ok(_) => json(200, &proto::Ok { ok: true }),
        Err(e) => error(500, &e.to_string()),
    }
}

fn public_apps(s: &Sync, user: u64) -> Vec<PublicApp> {
    let db = s.db();
    let apps = db::apps(&db).unwrap_or_default();
    db::user_apps(&db, user)
        .unwrap_or_default()
        .into_iter()
        .filter(|x| x.active)
        .filter_map(|x| {
            let a = apps.iter().find(|a| a.id == x.app && a.active)?;
            Some(PublicApp { id: a.id.clone(), name: a.name.clone(), icon: a.icon.clone(), quota: x.quota, used: x.used })
        })
        .collect()
}

fn push(s: &Sync, who: &Who, app: &str, p: Push) -> Reply {
    if !proto::valid_doc_id(&p.doc) {
        return error(400, "document invalide");
    }
    let data = if p.deleted {
        Vec::new()
    } else {
        match B64.decode(&p.data) {
            Ok(d) => d,
            Err(_) => return error(400, "contenu invalide"),
        }
    };
    if data.len() as u64 > proto::MAX_DOC {
        return error(413, "document trop gros");
    }
    if !p.deleted && hex(&sha256(&data)) != p.hash {
        return error(400, "contenu abîmé pendant l'envoi (empreinte différente)");
    }
    let mut db = s.db();
    let current = db::latest(&db, who.user, app, &p.doc);
    let current_v = current.as_ref().map(|m| m.version).unwrap_or(0);
    if current_v != p.base {
        drop(db);
        s.log(app, &who.login, &who.device_name, ActivityKind::Conflict, 1);
        return json(200, &PushResult { ok: false, version: current_v, conflict: current, message: "le serveur a une version plus récente".into() });
    }
    // Rien n'a changé : on ne crée pas de version (envoi rejoué après une coupure, par exemple).
    if let Some(c) = &current
        && c.hash == p.hash
        && c.deleted == p.deleted
    {
        return json(200, &PushResult { ok: true, version: c.version, conflict: None, message: String::new() });
    }
    let access = db::access(&db, who.user, app).unwrap_or_default();
    if db::used(&db, who.user, app) + data.len() as u64 > access.quota {
        return error(507, "quota atteint");
    }
    let version = db::cursor(&db, who.user, app) + 1;
    if !p.deleted {
        let path = s.blob_path(app, who.user, &p.doc, version);
        let tmp = path.with_extension("tmp");
        let written = path.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::write(&tmp, &data)).and_then(|_| std::fs::rename(&tmp, &path));
        if let Err(e) = written {
            return error(500, &format!("écriture impossible : {e}"));
        }
    }
    if let Err(e) = db::add_version(&mut db, who.user, app, &p.doc, &p.hash, data.len() as u64, p.deleted, &who.device_name, version) {
        let _ = std::fs::remove_file(s.blob_path(app, who.user, &p.doc, version));
        return error(500, &e.to_string());
    }
    drop(db);
    s.log(app, &who.login, &who.device_name, ActivityKind::Push, 1);
    json(200, &PushResult { ok: true, version, conflict: None, message: String::new() })
}

fn pull(s: &Sync, who: &Who, app: &str, since: u64) -> Reply {
    let db = s.db();
    match db::changes(&db, who.user, app, since, proto::PULL_LIMIT + 1) {
        Ok(mut docs) => {
            let more = docs.len() > proto::PULL_LIMIT;
            docs.truncate(proto::PULL_LIMIT);
            let cursor = if more { docs.last().map(|d| d.version).unwrap_or(since) } else { db::cursor(&db, who.user, app).max(since) };
            drop(db);
            if !docs.is_empty() {
                s.log(app, &who.login, &who.device_name, ActivityKind::Pull, docs.len() as u32);
            }
            json(200, &Pull { cursor, docs, more })
        }
        Err(e) => error(500, &e.to_string()),
    }
}

fn blob(s: &Sync, who: &Who, app: &str, doc: &str, version: &str) -> Reply {
    let Ok(version) = version.parse::<u64>() else { return error(400, "version invalide") };
    if !proto::valid_doc_id(doc) {
        return error(400, "document invalide");
    }
    match std::fs::read(s.blob_path(app, who.user, doc, version)) {
        Ok(b) => Reply::Bytes(b),
        Err(_) => error(404, "version introuvable (peut-être purgée)"),
    }
}
