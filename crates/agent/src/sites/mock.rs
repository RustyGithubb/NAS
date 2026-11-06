//! Sites simulés, pour développer la console sans le Mac mini.

use std::sync::Mutex;

use nas_proto::{Site, SiteAction, SiteKind, SiteState};

pub struct Mock {
    sites: Mutex<Vec<Site>>,
}

impl Mock {
    pub fn new() -> Self {
        let now = crate::backend::now_ms();
        let day = 86_400_000;
        Mock {
            sites: Mutex::new(vec![
                Site {
                    name: "liens".into(),
                    domain: "liens.exemple.fr".into(),
                    kind: SiteKind::Static,
                    state: SiteState::Online,
                    root: "/srv/nas/sites/liens".into(),
                    media_dir: "/srv/nas/medias/liens".into(),
                    cert_expires_ms: Some(now + 74 * day),
                    last_backup_ms: Some(now - 2 * day),
                },
                Site {
                    name: "portfolio".into(),
                    domain: "exemple.fr".into(),
                    kind: SiteKind::WordPress,
                    state: SiteState::Offline,
                    root: "/srv/nas/sites/portfolio".into(),
                    media_dir: "/srv/nas/medias/portfolio".into(),
                    cert_expires_ms: None,
                    last_backup_ms: None,
                },
            ]),
        }
    }
}

impl super::SitesBackend for Mock {
    fn simulated(&self) -> bool {
        true
    }

    fn list(&self) -> Result<Vec<Site>, String> {
        Ok(self.sites.lock().map_err(|e| e.to_string())?.clone())
    }

    fn action(&self, name: &str, action: SiteAction) -> Result<(), String> {
        let mut sites = self.sites.lock().map_err(|e| e.to_string())?;
        let site = sites.iter_mut().find(|s| s.name == name).ok_or_else(|| format!("site {name} inconnu"))?;
        match action {
            SiteAction::Set(state) => {
                // Premier passage en ligne : le certificat HTTPS est obtenu à ce moment-là.
                if state != SiteState::Offline && site.cert_expires_ms.is_none() {
                    site.cert_expires_ms = Some(crate::backend::now_ms() + 90 * 86_400_000);
                }
                site.state = state;
            }
            SiteAction::Backup => {
                std::thread::sleep(std::time::Duration::from_millis(800));
                site.last_backup_ms = Some(crate::backend::now_ms());
            }
        }
        Ok(())
    }
}
