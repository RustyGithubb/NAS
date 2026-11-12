//! NAS — agent : tourne sur le serveur et répond à la console en JSON sur HTTP.
//!
//! Adresse d'écoute : `127.0.0.1:7878` par défaut (accessible seulement depuis la machine
//! elle-même). Pour la changer : variable d'environnement `NAS_AGENT_ADDR`, par exemple
//! `NAS_AGENT_ADDR=0.0.0.0:7878` pour écouter sur tout le réseau.
//! Services et journaux : systemd sur le Mac mini, simulés ailleurs (`NAS_AGENT_MOCK=1` pour forcer).
//! Chaque demande `/api/…` doit porter la clé d'accès (voir `key.rs`) ; `nas-agent --show-key`
//! l'affiche. Les routes `/sync/…` (synchronisation des apps, voir `sync/`) ont leurs propres
//! sessions et sont les seules qu'on peut exposer sur Internet (Caddy) : une demande `/api/…`
//! passée par un serveur intermédiaire (en-tête `X-Forwarded-For`) est refusée.

mod backend;
mod files;
mod key;
mod network;
mod sampler;
mod sites;
mod sync;
mod update;

use std::sync::{Arc, Mutex};

use nas_proto::FileOp;

use nas_proto::{Action, ActionResult, Services, SiteAction, Sites, Status};
use serde::Serialize;

/// Nombre de fichiers récents (page Accueil de l'explorateur).
const RECENT: usize = 20;

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
    let sites = sites::detect();

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

    let updater = update::Updater::new(backend.simulated());
    let network = network::NetMonitor::start(backend.simulated());
    let sync = sync::Sync::open(backend.simulated());
    let ctx = Arc::new(Ctx { key, status, backend, sites, files: files::Files::new(), updater, network, sync });
    // Une demande = un fil : un gros transfert de fichier ne bloque pas le reste.
    for request in server.incoming_requests() {
        let ctx = ctx.clone();
        std::thread::spawn(move || handle(&ctx, request));
    }
}

/// Tout ce dont les demandes ont besoin.
struct Ctx {
    key: String,
    status: Arc<Mutex<Status>>,
    backend: Box<dyn backend::Backend>,
    sites: Box<dyn sites::SitesBackend>,
    files: files::Files,
    updater: update::Updater,
    network: network::NetMonitor,
    /// Synchronisation des apps ; `None` si son dossier est inaccessible.
    sync: Option<Arc<sync::Sync>>,
}

fn handle(ctx: &Ctx, mut request: tiny_http::Request) {
    // Synchronisation : sessions des comptes, pas la clé de la console.
    if request.url().starts_with("/sync/") {
        sync::serve(ctx.sync.as_deref(), request);
        return;
    }
    let forwarded = request.headers().iter().any(|h| h.field.equiv("X-Forwarded-For"));
    let auth = request.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str());
    let (code, body) = if forwarded {
        // Arrivée par Caddy (Internet) : l'administration n'est jamais exposée.
        error(403, "administration accessible seulement en réseau local")
    } else if !key::authorized(auth, &ctx.key) {
        error(401, "clé d'accès refusée")
    } else {
        let url = request.url().to_string();
        let (path, query) = url.split_once('?').unwrap_or((&url, ""));
        match (request.method(), path) {
            // Téléchargement : le fichier part tel quel, pas en JSON.
            (tiny_http::Method::Get, "/api/files/download") => {
                let target = param(query, "root").zip(param(query, "path")).ok_or("paramètres manquants".to_string()).and_then(|(r, p)| ctx.files.resolve(&r, &p));
                match target.and_then(|t| std::fs::File::open(&t).map_err(|e| e.to_string())) {
                    Ok(f) => {
                        let _ = request.respond(tiny_http::Response::from_file(f));
                        return;
                    }
                    Err(e) => error(404, &e),
                }
            }
            (tiny_http::Method::Post, "/api/files/upload") => match param(query, "root").zip(param(query, "path")) {
                Some((r, p)) => match ctx.files.upload(&r, &p, request.as_reader()) {
                    Ok(()) => json(200, &ActionResult { ok: true, message: String::new() }),
                    Err(message) => json(200, &ActionResult { ok: false, message }),
                },
                None => error(400, "paramètres manquants"),
            },
            // Synchronisation : actions avec un corps JSON.
            (method, p) if p == "/api/sync" || p.starts_with("/api/sync/") => {
                let post = *method == tiny_http::Method::Post;
                let mut body = Vec::new();
                if post {
                    use std::io::Read;
                    let _ = request.as_reader().take(1024 * 1024).read_to_end(&mut body);
                }
                let parts: Vec<&str> = p.trim_matches('/').split('/').skip(2).collect();
                sync::admin(ctx.sync.as_deref(), post, &parts, &body)
            }
            _ => route(request.method(), &url, ctx),
        }
    };
    let header = tiny_http::Header::from_bytes("Content-Type", "application/json").expect("en-tête valide");
    let _ = request.respond(tiny_http::Response::from_string(body).with_status_code(code).with_header(header));
}

fn route(method: &tiny_http::Method, url: &str, ctx: &Ctx) -> Reply {
    let (status, backend, sites) = (&ctx.status, ctx.backend.as_ref(), ctx.sites.as_ref());
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
        (Get, ["api", "logs"]) => match backend.logs(None, param(query, "after").as_deref()) {
            Ok(logs) => json(200, &logs),
            Err(e) => error(500, &e),
        },
        (Get, ["api", "logs", unit]) => {
            if !nas_proto::valid_unit(unit) {
                return error(400, "nom de service invalide");
            }
            let after = param(query, "after");
            match backend.logs(Some(unit), after.as_deref()) {
                Ok(logs) => json(200, &logs),
                Err(e) => error(500, &e),
            }
        }
        (Get, ["api", "sites"]) => match sites.list() {
            Ok(list) => json(200, &Sites { simulated: sites.simulated(), list }),
            Err(e) => error(500, &e),
        },
        (Post, ["api", "sites", name, verb]) => {
            let Some(action) = SiteAction::from_verb(verb) else { return error(400, "action inconnue") };
            if !nas_proto::valid_site_name(name) {
                return error(400, "nom de site invalide");
            }
            match sites.action(name, action) {
                Ok(()) => json(200, &ActionResult { ok: true, message: String::new() }),
                Err(message) => json(200, &ActionResult { ok: false, message }),
            }
        }
        (Get, ["api", "network"]) => json(200, &ctx.network.get()),
        (Post, ["api", "network", "speedtest"]) => match ctx.network.speedtest() {
            Ok(()) => json(200, &ActionResult { ok: true, message: String::new() }),
            Err(message) => json(200, &ActionResult { ok: false, message }),
        },
        (Get, ["api", "update"]) => json(200, &ctx.updater.status()),
        (Post, ["api", "update"]) => match ctx.updater.start() {
            Ok(()) => json(200, &ActionResult { ok: true, message: String::new() }),
            Err(message) => json(200, &ActionResult { ok: false, message }),
        },
        (Get, ["api", "files", "roots"]) => json(200, &ctx.files.roots_info()),
        (Get, ["api", "files", "list"]) => match param(query, "root") {
            Some(root) => match ctx.files.list(&root, &param(query, "path").unwrap_or_default()) {
                Ok(l) => json(200, &l),
                Err(e) => error(404, &e),
            },
            None => error(400, "paramètres manquants"),
        },
        (Get, ["api", "files", "recent"]) => json(200, &ctx.files.index.recent(RECENT)),
        (Get, ["api", "files", "search"]) => json(200, &ctx.files.index.search(&param(query, "q").unwrap_or_default())),
        (Post, ["api", "files", verb]) => {
            let Some(op) = FileOp::from_verb(verb) else { return error(400, "action inconnue") };
            let (Some(root), Some(path)) = (param(query, "root"), param(query, "path")) else { return error(400, "paramètres manquants") };
            match ctx.files.op(op, &root, &path, param(query, "to_root").as_deref(), param(query, "to").as_deref()) {
                Ok(()) => json(200, &ActionResult { ok: true, message: String::new() }),
                Err(message) => json(200, &ActionResult { ok: false, message }),
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
