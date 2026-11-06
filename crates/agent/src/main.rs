//! NAS — agent : tourne sur le serveur et répond à la console en JSON sur HTTP.
//!
//! Adresse d'écoute : `127.0.0.1:7878` par défaut (accessible seulement depuis la machine
//! elle-même). Pour la changer : variable d'environnement `NAS_AGENT_ADDR`, par exemple
//! `NAS_AGENT_ADDR=0.0.0.0:7878` pour écouter sur tout le réseau.

mod sampler;

use std::sync::{Arc, Mutex};

use nas_proto::Status;

fn main() {
    let addr = std::env::var("NAS_AGENT_ADDR").unwrap_or_else(|_| format!("127.0.0.1:{}", nas_proto::DEFAULT_PORT));
    // Dernières mesures, tenues à jour par un fil de mesure en arrière-plan.
    let status = Arc::new(Mutex::new(Status::default()));
    sampler::spawn(status.clone());

    let server = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("NAS agent : impossible d'écouter sur {addr} ({e})");
            std::process::exit(1);
        }
    };
    println!("NAS agent v{} — à l'écoute sur http://{addr}", env!("CARGO_PKG_VERSION"));

    for request in server.incoming_requests() {
        let (code, body) = match (request.method(), request.url()) {
            (tiny_http::Method::Get, "/api/status") => {
                let s = status.lock().map(|s| s.clone()).unwrap_or_default();
                (200, serde_json::to_string(&s).unwrap_or_default())
            }
            _ => (404, r#"{"error":"route inconnue"}"#.to_string()),
        };
        let json = tiny_http::Header::from_bytes("Content-Type", "application/json").expect("en-tête valide");
        let _ = request.respond(tiny_http::Response::from_string(body).with_status_code(code).with_header(json));
    }
}
