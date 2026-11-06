//! Liaison avec l'agent : un fil en arrière-plan lui demande son état toutes les 2 secondes.
//! L'interface lit simplement le dernier résultat (elle ne bloque jamais sur le réseau).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use nas_proto::Status;

/// Intervalle entre deux demandes.
const PERIOD: Duration = Duration::from_secs(2);
/// Délai au-delà duquel l'agent est considéré comme injoignable.
const TIMEOUT: Duration = Duration::from_secs(2);

/// État de la liaison.
#[derive(Clone)]
pub enum Link {
    /// Première tentative en cours.
    Connecting,
    /// Agent joignable : dernier état reçu.
    Online(Box<Status>),
    /// Agent injoignable ou réponse illisible.
    Offline(String),
}

struct Shared {
    addr: String,
    link: Link,
}

pub struct Server {
    shared: Arc<Mutex<Shared>>,
}

impl Server {
    /// Démarre le fil de liaison vers `addr` (« IP:port »).
    pub fn start(ctx: &egui::Context, addr: &str) -> Self {
        let shared = Arc::new(Mutex::new(Shared { addr: addr.to_string(), link: Link::Connecting }));
        let (s, ctx) = (shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
            loop {
                let addr = s.lock().map(|s| s.addr.clone()).unwrap_or_default();
                let started = Instant::now();
                let link = fetch(&agent, &addr);
                if let Ok(mut s) = s.lock() {
                    // L'adresse a changé pendant la demande : résultat ignoré.
                    if s.addr == addr {
                        s.link = link;
                    }
                }
                ctx.request_repaint();
                std::thread::sleep(PERIOD.saturating_sub(started.elapsed()));
            }
        });
        Server { shared }
    }

    /// Change l'adresse de l'agent (Réglages).
    pub fn set_addr(&self, addr: &str) {
        if let Ok(mut s) = self.shared.lock()
            && s.addr != addr
        {
            s.addr = addr.to_string();
            s.link = Link::Connecting;
        }
    }

    pub fn link(&self) -> Link {
        self.shared.lock().map(|s| s.link.clone()).unwrap_or(Link::Connecting)
    }
}

fn fetch(agent: &ureq::Agent, addr: &str) -> Link {
    let url = format!("http://{}/api/status", addr.trim());
    let status: Status = match agent.get(&url).call() {
        Ok(r) => match r.into_json() {
            Ok(s) => s,
            Err(_) => return Link::Offline(t!("réponse illisible", "unreadable response").to_string()),
        },
        Err(ureq::Error::Status(code, _)) => return Link::Offline(tf!("erreur HTTP {code}", "HTTP error {code}")),
        Err(_) => return Link::Offline(t!("agent injoignable", "agent unreachable").to_string()),
    };
    if status.protocol != nas_proto::PROTOCOL_VERSION {
        return Link::Offline(tf!(
            "version incompatible (agent : {}, console : {})",
            "incompatible version (agent: {}, console: {})",
            status.protocol,
            nas_proto::PROTOCOL_VERSION
        ));
    }
    Link::Online(Box::new(status))
}
