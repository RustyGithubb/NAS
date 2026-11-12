//! Routes `/api/sync/…` : administration depuis la console (clé d'accès de l'agent).

use nas_proto::ActionResult;
use nas_proto::sync::{self as proto, AppForm, Grant, Invitation, NewUser, Overview, SYNC_PROTOCOL_VERSION, Settings};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{INVITE_S, Sync, db, hex, invite_code, now, random, sha256};

type Reply = (u16, String);

fn json<T: Serialize>(code: u16, v: &T) -> Reply {
    (code, serde_json::to_string(v).unwrap_or_default())
}

fn done(r: Result<(), String>) -> Reply {
    match r {
        Ok(()) => json(200, &ActionResult { ok: true, message: String::new() }),
        Err(message) => json(200, &ActionResult { ok: false, message }),
    }
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|e| format!("demande invalide : {e}"))
}

/// Traite `/api/sync/…`. `parts` commence après `api/sync`.
pub fn admin(sync: Option<&Sync>, post: bool, parts: &[&str], body: &[u8]) -> Reply {
    let Some(s) = sync else {
        return (503, serde_json::json!({ "error": "synchronisation indisponible : dossier des données inaccessible (voir le journal de l'agent)" }).to_string());
    };
    match (post, parts) {
        (false, []) => json(200, &overview(s)),
        (true, ["settings"]) => done(parse::<Settings>(body).map(|v| {
            let db = s.db();
            db::set_setting(&db, "enabled", if v.enabled { "1" } else { "0" });
            db::set_setting(&db, "domain", v.domain.trim());
        })),
        (true, ["backup"]) => done(s.backup()),
        (true, ["apps"]) => done(parse::<AppForm>(body).and_then(|f| {
            check_app(&f)?;
            db::add_app(&s.db(), &f).map(|_| ()).map_err(|e| if e.to_string().contains("UNIQUE") { "cet identifiant est déjà pris".to_string() } else { e.to_string() })
        })),
        (true, ["apps", id, verb]) => done(app_action(s, id, verb, body)),
        (true, ["users"]) => json(200, &new_user(s, body)),
        (true, ["users", id, "reset"]) => json(200, &reset(s, id)),
        (true, ["users", id, verb]) => done(user_action(s, id, verb)),
        (true, ["users", id, "apps", app]) => done(id.parse::<u64>().map_err(|_| "compte invalide".to_string()).and_then(|u| {
            let g = parse::<Grant>(body)?;
            db::app(&s.db(), app).ok_or("app inconnue")?;
            db::grant(&s.db(), u, app, g.active, g.quota).map(|_| ()).map_err(|e| e.to_string())
        })),
        (true, ["users", id, "apps", app, "remove"]) => done(id.parse::<u64>().map_err(|_| "compte invalide".to_string()).and_then(|u| {
            db::revoke_app(&s.db(), u, app).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(s.dir.join(app).join(u.to_string()));
            Ok(())
        })),
        (true, ["devices", id, "revoke"]) => {
            done(id.parse::<u64>().map_err(|_| "poste invalide".to_string()).and_then(|d| db::revoke_device(&s.db(), d, None).map(|_| ()).map_err(|e| e.to_string())))
        }
        _ => (404, serde_json::json!({ "error": "route inconnue" }).to_string()),
    }
}

fn check_app(f: &AppForm) -> Result<(), String> {
    if !proto::valid_app_id(&f.id) {
        return Err("identifiant invalide : 2 à 32 caractères a-z, 0-9 ou -".into());
    }
    if f.name.trim().is_empty() {
        return Err("nom manquant".into());
    }
    if f.history_days > 3650 {
        return Err("historique trop long (10 ans maximum)".into());
    }
    Ok(())
}

fn app_action(s: &Sync, id: &str, verb: &str, body: &[u8]) -> Result<(), String> {
    let app = db::app(&s.db(), id).ok_or("app inconnue")?;
    let db = s.db();
    match verb {
        "update" => {
            let mut f = parse::<AppForm>(body)?;
            f.id = app.id;
            check_app(&f)?;
            db::update_app(&db, &f).map(|_| ()).map_err(|e| e.to_string())
        }
        "enable" | "disable" => db::set_app_active(&db, id, verb == "enable").map(|_| ()).map_err(|e| e.to_string()),
        "delete" => {
            if app.builtin {
                return Err(format!("{} est inscrite d'office : on peut la couper, pas la supprimer", app.name));
            }
            db::delete_app(&db, id).map_err(|e| e.to_string())?;
            drop(db);
            let _ = std::fs::remove_dir_all(s.dir.join(id));
            Ok(())
        }
        _ => Err("action inconnue".into()),
    }
}

fn user_id(id: &str) -> Result<u64, String> {
    id.parse().map_err(|_| "compte invalide".into())
}

fn user_action(s: &Sync, id: &str, verb: &str) -> Result<(), String> {
    let id = user_id(id)?;
    let db = s.db();
    match verb {
        "enable" | "disable" => db::set_user_active(&db, id, verb == "enable").map(|_| ()).map_err(|e| e.to_string()),
        "delete" => {
            db::delete_user(&db, id).map_err(|e| e.to_string())?;
            drop(db);
            if let Ok(dirs) = std::fs::read_dir(&s.dir) {
                for d in dirs.flatten() {
                    let _ = std::fs::remove_dir_all(d.path().join(id.to_string()));
                }
            }
            Ok(())
        }
        _ => Err("action inconnue".into()),
    }
}

fn invitation(ok: bool, message: impl Into<String>) -> Invitation {
    Invitation { ok, message: message.into(), ..Default::default() }
}

fn new_user(s: &Sync, body: &[u8]) -> Invitation {
    let n = match parse::<NewUser>(body) {
        Ok(n) => n,
        Err(e) => return invitation(false, e),
    };
    let login = n.login.trim().to_lowercase();
    if !proto::valid_login(&login) {
        return invitation(false, "identifiant invalide : 2 à 32 caractères a-z, 0-9, « . », « _ » ou « - »");
    }
    let code = invite_code();
    let expires = now() + INVITE_S;
    let db = s.db();
    if db::account(&db, &login).is_some() {
        return invitation(false, "ce compte existe déjà");
    }
    let id = match db::add_user(&db, &login, &hex(&random(16)), &hex(&sha256(code.as_bytes())), expires) {
        Ok(id) => id,
        Err(e) => return invitation(false, e.to_string()),
    };
    for app in &n.apps {
        if db::app(&db, app).is_some() {
            let _ = db::grant(&db, id, app, true, None);
        }
    }
    Invitation { ok: true, message: String::new(), user_id: id, login, code, expires }
}

fn reset(s: &Sync, id: &str) -> Invitation {
    let Ok(id) = user_id(id) else { return invitation(false, "compte invalide") };
    let db = s.db();
    let Some(a) = db::account_by_id(&db, id) else { return invitation(false, "compte introuvable") };
    let code = invite_code();
    let expires = now() + INVITE_S;
    match db::reset_user(&db, id, &hex(&random(16)), &hex(&sha256(code.as_bytes())), expires) {
        Ok(_) => Invitation { ok: true, message: String::new(), user_id: id, login: a.login, code, expires },
        Err(e) => invitation(false, e.to_string()),
    }
}

fn overview(s: &Sync) -> Overview {
    let db = s.db();
    let settings = Settings { enabled: db::setting(&db, "enabled").as_deref() == Some("1"), domain: db::setting(&db, "domain").unwrap_or_default() };
    let apps = db::apps(&db).unwrap_or_default();
    let users = db::users(&db).unwrap_or_default();
    let last_backup = db::setting(&db, "last_backup").and_then(|v| v.parse().ok());
    let backup_error = db::setting(&db, "backup_error").unwrap_or_default();
    let used = db::total_used(&db);
    drop(db);
    let disk_free = crate::sampler::space(&s.dir).map(|(_, _, avail)| avail).unwrap_or(0);

    let mut alerts = Vec::new();
    if !backup_error.is_empty() {
        alerts.push(format!("Dernière sauvegarde ratée : {backup_error}"));
    }
    if last_backup.is_none_or(|t: u64| now().saturating_sub(t) > 2 * 86400) && users.iter().any(|u| !u.pending) {
        alerts.push("Aucune sauvegarde depuis plus de 2 jours".into());
    }
    for u in &users {
        if u.failures >= 5 {
            alerts.push(format!("{} : {} échecs de connexion depuis la dernière réussite", u.login, u.failures));
        }
        for a in &u.apps {
            if a.quota > 0 && a.used * 10 >= a.quota * 9 {
                alerts.push(format!("{} : quota {} presque plein", u.login, a.app));
            }
        }
    }
    if disk_free > 0 && disk_free < 20 * 1024 * 1024 * 1024 {
        alerts.push("Moins de 20 Go libres sur le disque du serveur".into());
    }
    let activity = s.activity.lock().map(|a| a.iter().cloned().collect()).unwrap_or_default();
    Overview { protocol: SYNC_PROTOCOL_VERSION, simulated: s.simulated, settings, dir: s.dir.display().to_string(), used, disk_free, apps, users, activity, alerts, last_backup }
}
