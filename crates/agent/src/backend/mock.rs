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

pub struct Mock {
    units: Arc<Mutex<HashMap<String, Unit>>>,
}

impl Mock {
    pub fn new() -> Self {
        let mut units = HashMap::new();
        let now = super::now_ms();
        for (name, desc, active, lines) in CATALOG {
            let state = if active { ServiceState::Active } else { ServiceState::Inactive };
            let sub = if active { "running" } else { "dead" };
            let mut u = Unit { service: Service { name: name.into(), description: desc.into(), state, sub: sub.into(), enabled: active }, lines, log: Vec::new(), dropped: 0 };
            if active {
                push(&mut u, now - 60_000, 6, format!("Started {desc}."));
            }
            units.insert(name.to_string(), u);
        }
        // Le dernier service est en échec, pour voir à quoi ça ressemble.
        if let Some(u) = units.get_mut("restic-backup.service") {
            u.service.state = ServiceState::Failed;
            u.service.sub = "failed".into();
            push(u, now - 30_000, 3, "Fatal: unable to open repository at /mnt/sauvegarde: no such file or directory".into());
            push(u, now - 30_000, 4, "restic-backup.service: Failed with result 'exit-code'.".into());
        }
        let units = Arc::new(Mutex::new(units));
        spawn_writer(units.clone());
        Mock { units }
    }
}

fn push(u: &mut Unit, time_ms: i64, priority: u8, message: String) {
    u.log.push(LogEntry { time_ms, priority, message });
    // Journal borné : les plus anciennes lignes disparaissent.
    if u.log.len() > 2000 {
        u.log.drain(..500);
        u.dropped += 500;
    }
}

/// Les services actifs écrivent une ligne toutes les quelques secondes.
fn spawn_writer(units: Arc<Mutex<HashMap<String, Unit>>>) {
    std::thread::spawn(move || {
        let mut n = 0usize;
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            n += 1;
            let Ok(mut units) = units.lock() else { return };
            for (i, u) in units.values_mut().enumerate() {
                if u.service.state == ServiceState::Active && (n + i).is_multiple_of(3) && !u.lines.is_empty() {
                    let line = u.lines[(n / 3 + i) % u.lines.len()];
                    push(u, super::now_ms(), 6, line.to_string());
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
        let units = self.units.lock().map_err(|e| e.to_string())?;
        let mut list: Vec<Service> = units.values().map(|u| u.service.clone()).collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    fn action(&self, unit: &str, action: Action) -> Result<(), String> {
        let mut units = self.units.lock().map_err(|e| e.to_string())?;
        let u = units.get_mut(unit).ok_or_else(|| format!("Unit {unit} not found."))?;
        let desc = u.service.description.clone();
        let now = super::now_ms();
        if matches!(action, Action::Stop | Action::Restart) && u.service.state == ServiceState::Active {
            push(u, now, 6, format!("Stopping {desc}..."));
            push(u, now, 6, format!("Stopped {desc}."));
        }
        if matches!(action, Action::Start | Action::Restart) {
            push(u, now, 6, format!("Starting {desc}..."));
            push(u, now, 6, format!("Started {desc}."));
            u.service.state = ServiceState::Active;
            u.service.sub = "running".into();
        } else {
            u.service.state = ServiceState::Inactive;
            u.service.sub = "dead".into();
        }
        Ok(())
    }

    fn logs(&self, unit: &str, after: Option<&str>) -> Result<Logs, String> {
        let units = self.units.lock().map_err(|e| e.to_string())?;
        let u = units.get(unit).ok_or_else(|| format!("Unit {unit} not found."))?;
        // Curseur : nombre total de lignes écrites depuis le lancement (retirées comprises).
        let len = u.log.len();
        let from = match after.and_then(|c| c.parse::<usize>().ok()) {
            Some(c) => c.saturating_sub(u.dropped).min(len),
            None => len.saturating_sub(nas_proto::LOG_LIMIT),
        };
        let from = from.max(len.saturating_sub(nas_proto::LOG_LIMIT));
        Ok(Logs { entries: u.log[from..].to_vec(), cursor: Some((u.dropped + len).to_string()) })
    }
}
