//! Services et journaux : systemd sur le Mac mini, simulation ailleurs (Windows, Linux sans systemd).

mod mock;
mod systemd;

use nas_proto::{Action, Logs, Service};

pub trait Backend: Send + Sync {
    /// Données simulées ?
    fn simulated(&self) -> bool;
    fn services(&self) -> Result<Vec<Service>, String>;
    /// Démarre, arrête ou redémarre `unit` (nom déjà vérifié par `valid_unit`).
    fn action(&self, unit: &str, action: Action) -> Result<(), String>;
    /// Dernières lignes du journal de `unit`, ou celles qui suivent le curseur `after`.
    fn logs(&self, unit: &str, after: Option<&str>) -> Result<Logs, String>;
}

/// systemd s'il est là (Linux démarré avec systemd), sinon simulation.
/// `NAS_AGENT_MOCK=1` force la simulation.
pub fn detect() -> Box<dyn Backend> {
    let forced = std::env::var("NAS_AGENT_MOCK").is_ok_and(|v| v == "1");
    if !forced && cfg!(target_os = "linux") && std::path::Path::new("/run/systemd/system").exists() { Box::new(systemd::Systemd) } else { Box::new(mock::Mock::new()) }
}

/// Date actuelle en millisecondes depuis 1970.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
