//! Mise à jour de l'agent depuis GitHub. Le travail est fait par le service systemd
//! `nas-agent-update` (script `deploy/nas-agent-update.sh`, lancé chaque nuit par un timer) : il
//! compile la nouvelle version, l'installe et redémarre l'agent. L'agent se contente de le
//! lancer à la demande de la console et de lire le résultat qu'il écrit.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nas_proto::{UpdateState, UpdateStatus};

/// Résultat écrit par le script.
const STATUS_FILE: &str = "/var/lib/nas-agent/update.json";
const UNIT: &str = "nas-agent-update.service";
const UNIT_FILE: &str = "/etc/systemd/system/nas-agent-update.service";

pub struct Updater {
    /// Hors du Mac mini (Windows, `NAS_AGENT_MOCK=1`) : recherche simulée.
    mock: Option<Arc<Mutex<UpdateStatus>>>,
}

impl Updater {
    pub fn new(simulated: bool) -> Self {
        let mock = simulated.then(|| Arc::new(Mutex::new(UpdateStatus { message: "Mise à jour simulée".into(), ..Default::default() })));
        Updater { mock }
    }

    pub fn status(&self) -> UpdateStatus {
        if let Some(m) = &self.mock {
            return m.lock().map(|s| s.clone()).unwrap_or_default();
        }
        if !Path::new(UNIT_FILE).exists() {
            return UpdateStatus { state: UpdateState::NotInstalled, message: "Mise à jour automatique non installée (sudo sh deploy/install.sh)".into(), ..Default::default() };
        }
        let mut st: UpdateStatus = std::fs::read_to_string(STATUS_FILE)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| UpdateStatus { message: "Aucune recherche pour l'instant".into(), ..Default::default() });
        // Le service tourne encore : la recherche n'est pas finie, quoi qu'en dise le fichier.
        if running() {
            st.state = UpdateState::Running;
        }
        st
    }

    /// Lance une recherche (sans attendre la fin : la compilation peut prendre des minutes).
    pub fn start(&self) -> Result<(), String> {
        if let Some(m) = &self.mock {
            let m = m.clone();
            if let Ok(mut s) = m.lock() {
                s.state = UpdateState::Running;
                s.message = "Recherche de mise à jour…".into();
            }
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(4));
                if let Ok(mut s) = m.lock() {
                    *s = UpdateStatus { state: UpdateState::UpToDate, message: "Déjà à jour (simulé)".into(), commit: "simule".into(), checked_ms: now_ms() };
                }
            });
            return Ok(());
        }
        if !Path::new(UNIT_FILE).exists() {
            return Err("mise à jour automatique non installée (sudo sh deploy/install.sh)".into());
        }
        let out = Command::new("systemctl").args(["start", "--no-block", UNIT]).output().map_err(|e| format!("systemctl : {e}"))?;
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
    }
}

fn running() -> bool {
    Command::new("systemctl")
        .args(["is-active", UNIT])
        .output()
        .map(|o| matches!(String::from_utf8_lossy(&o.stdout).trim(), "active" | "activating" | "reloading"))
        .unwrap_or(false)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
