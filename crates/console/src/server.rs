//! Liaison avec l'agent. Un fil en arrière-plan demande l'état et la liste des services toutes
//! les 2 secondes ; les actions et la lecture des journaux partent chacune dans leur propre fil.
//! L'interface lit simplement les derniers résultats (elle ne bloque jamais sur le réseau).

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui;
use nas_proto::{Action, ActionResult, LogEntry, Logs, Services, Status};

/// Intervalle entre deux demandes d'état.
const PERIOD: Duration = Duration::from_secs(2);
/// Intervalle entre deux lectures du journal affiché.
const LOG_PERIOD: Duration = Duration::from_secs(1);
/// Délai au-delà duquel l'agent est considéré comme injoignable.
const TIMEOUT: Duration = Duration::from_secs(3);
/// Lignes de journal gardées en mémoire par la console.
const LOG_KEEP: usize = 3000;

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

/// Journal du service affiché dans l'onglet Journaux.
#[derive(Default)]
pub struct LogView {
    pub unit: Option<String>,
    pub entries: VecDeque<LogEntry>,
    cursor: Option<String>,
    in_flight: bool,
    last: Option<Instant>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Shared {
    addr: String,
    link: Option<Link>,
    pub services: Option<Services>,
    /// Services dont une action est en cours.
    pub pending: HashSet<String>,
    /// Résultat de la dernière action : service, action, résultat.
    pub last_action: Option<(String, Action, ActionResult)>,
    pub logs: LogView,
}

pub struct Server {
    shared: Arc<Mutex<Shared>>,
    agent: ureq::Agent,
    ctx: egui::Context,
}

impl Server {
    /// Démarre le fil de liaison vers `addr` (« IP:port »).
    pub fn start(ctx: &egui::Context, addr: &str) -> Self {
        let shared = Arc::new(Mutex::new(Shared { addr: addr.to_string(), ..Default::default() }));
        let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
        let (s, a, c) = (shared.clone(), agent.clone(), ctx.clone());
        std::thread::spawn(move || {
            loop {
                let addr = lock(&s).addr.clone();
                let started = Instant::now();
                let link = fetch_status(&a, &addr);
                let services = matches!(link, Link::Online(_)).then(|| get::<Services>(&a, &addr, "/api/services").ok()).flatten();
                {
                    let mut s = lock(&s);
                    // L'adresse a changé pendant la demande : résultat ignoré.
                    if s.addr == addr {
                        s.link = Some(link);
                        s.services = services;
                    }
                }
                c.request_repaint();
                std::thread::sleep(PERIOD.saturating_sub(started.elapsed()));
            }
        });
        Server { shared, agent, ctx: ctx.clone() }
    }

    /// Accès aux données partagées (liste des services, journal…).
    pub fn data(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    /// Change l'adresse de l'agent (Réglages).
    pub fn set_addr(&self, addr: &str) {
        let mut s = self.data();
        if s.addr != addr {
            *s = Shared { addr: addr.to_string(), ..Default::default() };
        }
    }

    pub fn link(&self) -> Link {
        self.data().link.clone().unwrap_or(Link::Connecting)
    }

    /// Démarre, arrête ou redémarre un service ; le résultat arrive dans `last_action`.
    pub fn action(&self, unit: &str, action: Action) {
        let addr = {
            let mut s = self.data();
            if !s.pending.insert(unit.to_string()) {
                return;
            }
            s.addr.clone()
        };
        let (shared, agent, ctx, unit) = (self.shared.clone(), self.agent.clone(), self.ctx.clone(), unit.to_string());
        std::thread::spawn(move || {
            let path = format!("/api/services/{}/{}", urlencoding::encode(&unit), action.verb());
            let result = match agent.post(&url(&addr, &path)).call() {
                Ok(r) => r.into_json::<ActionResult>().unwrap_or_else(|e| ActionResult { ok: false, message: e.to_string() }),
                Err(e) => ActionResult { ok: false, message: describe(e) },
            };
            // État à jour tout de suite, sans attendre la demande périodique.
            let services = get::<Services>(&agent, &addr, "/api/services").ok();
            let mut s = lock(&shared);
            s.pending.remove(&unit);
            if s.addr == addr {
                if services.is_some() {
                    s.services = services;
                }
                s.last_action = Some((unit, action, result));
            }
            ctx.request_repaint();
        });
    }

    /// À appeler à chaque image par l'onglet Journaux : suit le journal de `unit`.
    pub fn follow_logs(&self, unit: Option<&str>) {
        let addr = {
            let mut s = self.data();
            if s.logs.unit.as_deref() != unit {
                s.logs = LogView { unit: unit.map(str::to_string), ..Default::default() };
            }
            let Some(unit) = s.logs.unit.clone() else { return };
            let due = s.logs.last.is_none_or(|t| t.elapsed() >= LOG_PERIOD);
            if s.logs.in_flight || !due {
                return;
            }
            s.logs.in_flight = true;
            (s.addr.clone(), unit, s.logs.cursor.clone())
        };
        let (addr, unit, cursor) = addr;
        let (shared, agent, ctx) = (self.shared.clone(), self.agent.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut path = format!("/api/logs/{}", urlencoding::encode(&unit));
            if let Some(c) = &cursor {
                path.push_str(&format!("?after={}", urlencoding::encode(c)));
            }
            let result = get::<Logs>(&agent, &addr, &path);
            let mut s = lock(&shared);
            // Autre service choisi entre-temps : résultat ignoré.
            if s.addr != addr || s.logs.unit.as_deref() != Some(&unit) {
                return;
            }
            let view = &mut s.logs;
            view.in_flight = false;
            view.last = Some(Instant::now());
            match result {
                Ok(logs) => {
                    let new = !logs.entries.is_empty();
                    view.entries.extend(logs.entries);
                    while view.entries.len() > LOG_KEEP {
                        view.entries.pop_front();
                    }
                    if logs.cursor.is_some() {
                        view.cursor = logs.cursor;
                    }
                    view.error = None;
                    if new {
                        ctx.request_repaint();
                    }
                }
                Err(e) => {
                    view.error = Some(e);
                    ctx.request_repaint();
                }
            }
        });
        // Prochaine lecture : l'interface doit se redessiner même sans mouvement de souris.
        self.ctx.request_repaint_after(LOG_PERIOD);
    }

    /// Vide le journal affiché (les lignes suivantes continuent d'arriver).
    pub fn clear_logs(&self) {
        self.data().logs.entries.clear();
    }
}

fn lock(s: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    // Un fil qui a paniqué en tenant le verrou ne doit pas bloquer l'interface.
    s.lock().unwrap_or_else(|e| e.into_inner())
}

fn url(addr: &str, path: &str) -> String {
    format!("http://{}{path}", addr.trim())
}

fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let msg = r.into_json::<serde_json::Value>().ok().and_then(|v| v.get("error").and_then(|m| m.as_str()).map(str::to_string));
            msg.unwrap_or_else(|| tf!("erreur HTTP {code}", "HTTP error {code}"))
        }
        ureq::Error::Transport(_) => t!("agent injoignable", "agent unreachable").to_string(),
    }
}

fn get<T: serde::de::DeserializeOwned>(agent: &ureq::Agent, addr: &str, path: &str) -> Result<T, String> {
    match agent.get(&url(addr, path)).call() {
        Ok(r) => r.into_json().map_err(|_| t!("réponse illisible", "unreadable response").to_string()),
        Err(e) => Err(describe(e)),
    }
}

fn fetch_status(agent: &ureq::Agent, addr: &str) -> Link {
    let status: Status = match get(agent, addr, "/api/status") {
        Ok(s) => s,
        Err(e) => return Link::Offline(e),
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
