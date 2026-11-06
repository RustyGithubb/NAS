//! NAS — agent : tourne sur le serveur et répond à la console en JSON sur HTTP.
//!
//! Adresse d'écoute : `127.0.0.1:7878` par défaut (accessible seulement depuis la machine
//! elle-même). Pour la changer : variable d'environnement `NAS_AGENT_ADDR`, par exemple
//! `NAS_AGENT_ADDR=0.0.0.0:7878` pour écouter sur tout le réseau.
//! Services et journaux : systemd sur le Mac mini, simulés ailleurs (`NAS_AGENT_MOCK=1` pour forcer).
//! Chaque demande doit porter la clé d'accès (voir `key.rs`) ; `nas-agent --show-key` l'affiche.

mod backend;
mod key;
mod sampler;

use std::sync::{Arc, Mutex};

use nas_proto::{Action, ActionResult, Services, Status};
use serde::Serialize;

/// Réponse : code HTTP et corps JSON.
type Reply = (u16, String);

fn json<T: Serialize>(code: u16, v: &T) -> Reply {
    (code, serde_json::to_string(v).unwrap_or_default())
}

fn error(code: u16, msg: &str) -> Reply {
    json(code, &serde_json::json!({ "error": msg }))
}

fn main() {
    let (key, new) = match key::load_or_create() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("NAS agent : clé d'accès impossible à créer ou à lire ({e})");
            std::process::exit(1);
        }
    };
    if std::env::args().any(|a| a == "--show-key") {
        println!("{key}");
        return;
    }
    let addr = std::env::var("NAS_AGENT_ADDR").unwrap_or_else(|_| format!("127.0.0.1:{}", nas_proto::DEFAULT_PORT));
    // Dernières mesures, tenues à jour par un fil de mesure en arrière-plan.
    let status = Arc::new(Mutex::new(Status::default()));
    sampler::spawn(status.clone());
    let backend = backend::detect();

    let server = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("NAS agent : impossible d'écouter sur {addr} ({e})");
            std::process::exit(1);
        }
    };
    let mode = if backend.simulated() { "services simulés" } else { "services systemd" };
    println!("NAS agent v{} — à l'écoute sur http://{addr} ({mode})", env!("CARGO_PKG_VERSION"));
    if new {
        println!("Nouvelle clé d'accès (à copier dans la console, Réglages › Serveur) :\n  {key}");
    }
    let file = key::path();
    println!("Clé d'accès : {} (affichable avec --show-key)", file.canonicalize().unwrap_or(file).display());

    for request in server.incoming_requests() {
        let auth = request.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str());
        let (code, body) = if key::authorized(auth, &key) { route(request.method(), request.url(), &status, backend.as_ref()) } else { error(401, "clé d'accès refusée") };
        let header = tiny_http::Header::from_bytes("Content-Type", "application/json").expect("en-tête valide");
        let _ = request.respond(tiny_http::Response::from_string(body).with_status_code(code).with_header(header));
    }
}

fn route(method: &tiny_http::Method, url: &str, status: &Mutex<Status>, backend: &dyn backend::Backend) -> Reply {
    use tiny_http::Method::{Get, Post};
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (method, parts.as_slice()) {
        (Get, ["api", "status"]) => json(200, &status.lock().map(|s| s.clone()).unwrap_or_default()),
        (Get, ["api", "services"]) => match backend.services() {
            Ok(list) => json(200, &Services { simulated: backend.simulated(), list }),
            Err(e) => error(500, &e),
        },
        (Post, ["api", "services", unit, verb]) => {
            let Some(action) = Action::from_verb(verb) else { return error(400, "action inconnue") };
            if !nas_proto::valid_unit(unit) {
                return error(400, "nom de service invalide");
            }
            match backend.action(unit, action) {
                Ok(()) => json(200, &ActionResult { ok: true, message: String::new() }),
                Err(message) => json(200, &ActionResult { ok: false, message }),
            }
        }
        (Get, ["api", "logs", unit]) => {
            if !nas_proto::valid_unit(unit) {
                return error(400, "nom de service invalide");
            }
            let after = param(query, "after");
            match backend.logs(unit, after.as_deref()) {
                Ok(logs) => json(200, &logs),
                Err(e) => error(500, &e),
            }
        }
        _ => error(404, "route inconnue"),
    }
}

/// Valeur décodée d'un paramètre d'URL (`?after=…`).
fn param(query: &str, key: &str) -> Option<String> {
    query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).and_then(|(_, v)| urlencoding::decode(v).ok()).map(|v| v.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params() {
        assert_eq!(param("after=s%3D1%3Bi%3D2&x=1", "after").as_deref(), Some("s=1;i=2"));
        assert_eq!(param("", "after"), None);
    }
}
