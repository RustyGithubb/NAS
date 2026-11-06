//! Sites web : Caddy sur le Mac mini, simulation ailleurs.

mod caddy;
mod mock;

use nas_proto::{Site, SiteAction};

pub trait SitesBackend: Send + Sync {
    fn simulated(&self) -> bool;
    fn list(&self) -> Result<Vec<Site>, String>;
    /// Met en ligne / en maintenance / hors ligne, ou sauvegarde `name` (nom déjà vérifié).
    fn action(&self, name: &str, action: SiteAction) -> Result<(), String>;
}

/// Caddy si l'agent tourne sur le serveur (Linux avec systemd), sinon simulation.
/// `NAS_AGENT_MOCK=1` force la simulation.
pub fn detect() -> Box<dyn SitesBackend> {
    let forced = std::env::var("NAS_AGENT_MOCK").is_ok_and(|v| v == "1");
    if !forced && cfg!(target_os = "linux") && std::path::Path::new("/run/systemd/system").exists() { Box::new(caddy::Caddy::new()) } else { Box::new(mock::Mock::new()) }
}
