//! Réglages de la console, enregistrés en JSON.
//!
//! En développement (`cargo run`) : `data/prefs.json` à la racine du projet.
//! Version installée : dossier de configuration de l'utilisateur (`%APPDATA%\NAS\` sous Windows).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::i18n::Language;
use crate::theme::ThemeId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeId,
    pub language: Language,
    /// Adresse de l'agent (« IP:port »).
    pub agent_addr: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { theme: ThemeId::default(), language: Language::default(), agent_addr: format!("127.0.0.1:{}", nas_proto::DEFAULT_PORT) }
    }
}

fn path() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/prefs.json"));
    }
    directories::ProjectDirs::from("", "", "NAS").map(|d| d.config_dir().join("prefs.json"))
}

impl Prefs {
    /// Réglages enregistrés, ou ceux par défaut (premier lancement, fichier illisible).
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }
}
