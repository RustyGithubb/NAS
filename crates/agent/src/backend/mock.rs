//! Services simulés, pour développer la console sans le Mac mini.
//!
//! Les services « actifs » écrivent de temps en temps une ligne dans leur journal ; démarrer,
//! arrêter ou redémarrer change leur état et l'écrit au journal, comme le ferait systemd.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nas_proto::{Action, LogEntry, Logs, Service, ServiceState};

/// Services d'un serveur maison type, et lignes qu'ils écrivent en fonctionnement.
const CATALOG: [(&str, &str, bool, &[&str]); 9] = [
    ("nas-agent.service", "NAS — agent de la console", true, &["GET /api/status 200", "GET /api/services 200"]),
    ("ssh.service", "OpenBSD Secure Shell server", true, &["Accepted publickey for dorian from 10.8.0.2 port 51234 ssh2", "Connection closed by 10.8.0.2 port 51234"]),
    ("smbd.service", "Samba SMB Daemon", true, &["connect to service partage initially as user dorian", "closed connection to service partage"]),
    ("wg-quick@wg0.service", "WireGuard via wg-quick(8) for wg0", true, &["wg0: peer handshake completed"]),
    ("restic-backup.service", "Sauvegarde restic", false, &["snapshot 4f2a9c1e saved", "Files: 12 new, 3 changed, 18342 unmodified"]),
    ("caddy.service", "Caddy (serveur web)", true, &["exemple.fr: GET / 200", "liens.exemple.fr: GET /favicon.ico 404", "certificate obtained successfully: liens.exemple.fr"]),
    ("php8.4-fpm.service", "The PHP 8.4 FastCGI Process Manager", true, &["pool www: server reached pm.max_children setting (5)"]),
    ("mariadb.service", "MariaDB 11.8 database server", true, &["Aborted connection 42 to db: 'wordpress' (Got timeout reading communication packets)"]),
    ("worktable-sync.service", "Synchronisation WorkTable", true, &["sync: 3 notes envoyées", "sync: aucun changement"]),
];

struct Unit {
    service: Service,
    lines: &'static [&'static str],
    log: Vec<LogEntry>,
    /// Lignes déjà retirées du début du journal (le curseur les compte).
    dropped: usize,
}

/// Journal de tout le système : les lignes de tous les services, dans l'ordre.
#[derive(Default)]
struct Journal {
    log: Vec<LogEntry>,
    dropped: usize,
}

#[derive(Default)]
struct World {
    units: HashMap<String, Unit>,
    system: Journal,
}

pub struct Mock {
    world: Arc<Mutex<World>>,
}

impl Mock {
    pub fn new() -> Self {
        let mut w = World::default();
        let now = super::now_ms();
        for (name, desc, active, lines) in CATALOG {
            let state = if active { ServiceState::Active } else { ServiceState::Inactive };
            let sub = if active { "running" } else { "dead" };
            let mut u = Unit { service: Service { name: name.into(), description: desc.into(), state, sub: sub.into(), enabled: active }, lines, log: Vec::new(), dropped: 0 };
            if active {
                push(&mut u, &mut w.system, now - 60_000, 6, format!("Started {desc}."));
            }
            w.units.insert(name.to_string(), u);
        }
        // Le dernier service est en échec, pour voir à quoi ça ressemble.
        if let Some(u) = w.units.get_mut("restic-backup.service") {
            u.service.state = ServiceState::Failed;
            u.service.sub = "failed".into();
            push(u, &mut w.system, now - 30_000, 3, "Fatal: unable to open repository at /mnt/sauvegarde: no such file or directory".into());
            push(u, &mut w.system, now - 30_000, 4, "restic-backup.service: Failed with result 'exit-code'.".into());
        }
        let world = Arc::new(Mutex::new(w));
        spawn_writer(world.clone());
        Mock { world }
    }
}

/// Écrit une ligne au journal du service et à celui du système.
fn push(u: &mut Unit, system: &mut Journal, time_ms: i64, priority: u8, message: String) {
    let source = u.service.name.trim_end_matches(".service").to_string();
    let e = LogEntry { time_ms, priority, message, source };
    system.log.push(e.clone());
    u.log.push(e);
    // Journaux bornés : les plus anciennes lignes disparaissent.
    if u.log.len() > 2000 {
        u.log.drain(..500);
        u.dropped += 500;
    }
    if system.log.len() > 4000 {
        system.log.drain(..1000);
        system.dropped += 1000;
    }
}

/// Lignes après le curseur (nombre total de lignes écrites depuis le lancement, retirées comprises).
fn read(log: &[LogEntry], dropped: usize, after: Option<&str>) -> Logs {
    let len = log.len();
    let from = match after.and_then(|c| c.parse::<usize>().ok()) {
        Some(c) => c.saturating_sub(dropped).min(len),
        None => len.saturating_sub(nas_proto::LOG_LIMIT),
    };
    let from = from.max(len.saturating_sub(nas_proto::LOG_LIMIT));
    Logs { entries: log[from..].to_vec(), cursor: Some((dropped + len).to_string()) }
}

/// Les services actifs écrivent une ligne toutes les quelques secondes.
fn spawn_writer(world: Arc<Mutex<World>>) {
    std::thread::spawn(move || {
        let mut n = 0usize;
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            n += 1;
            let Ok(mut w) = world.lock() else { return };
            let World { units, system } = &mut *w;
            for (i, u) in units.values_mut().enumerate() {
                if u.service.state == ServiceState::Active && (n + i).is_multiple_of(3) && !u.lines.is_empty() {
                    let line = u.lines[(n / 3 + i) % u.lines.len()];
                    push(u, system, super::now_ms(), 6, line.to_string());
                }
            }
        }
    });
}

impl super::Backend for Mock {
    fn simulated(&self) -> bool {
        true
    }

    fn services(&self) -> Result<Vec<Service>, String> {
        let w = self.world.lock().map_err(|e| e.to_string())?;
        let mut list: Vec<Service> = w.units.values().map(|u| u.service.clone()).collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    fn action(&self, unit: &str, action: Action) -> Result<(), String> {
        let mut w = self.world.lock().map_err(|e| e.to_string())?;
        let World { units, system } = &mut *w;
        let u = units.get_mut(unit).ok_or_else(|| format!("Unit {unit} not found."))?;
        let desc = u.service.description.clone();
        let now = super::now_ms();
        if matches!(action, Action::Stop | Action::Restart) && u.service.state == ServiceState::Active {
            push(u, system, now, 6, format!("Stopping {desc}..."));
            push(u, system, now, 6, format!("Stopped {desc}."));
        }
        if matches!(action, Action::Start | Action::Restart) {
            push(u, system, now, 6, format!("Starting {desc}..."));
            push(u, system, now, 6, format!("Started {desc}."));
            u.service.state = ServiceState::Active;
            u.service.sub = "running".into();
        } else {
            u.service.state = ServiceState::Inactive;
            u.service.sub = "dead".into();
        }
        Ok(())
    }

    fn logs(&self, unit: Option<&str>, after: Option<&str>) -> Result<Logs, String> {
        let w = self.world.lock().map_err(|e| e.to_string())?;
        match unit {
            None => Ok(read(&w.system.log, w.system.dropped, after)),
            Some(unit) => {
                let u = w.units.get(unit).ok_or_else(|| format!("Unit {unit} not found."))?;
                Ok(read(&u.log, u.dropped, after))
            }
        }
    }
}
