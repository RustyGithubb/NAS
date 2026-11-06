//! Réglages de la console, enregistrés en JSON.
//!
//! En développement (`cargo run`) : `data/prefs.json` à la racine du projet.
//! Version installée : dossier de configuration de l'utilisateur (`%APPDATA%\NAS\` sous Windows).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::i18n::Language;
use crate::theme::{FontPrefs, ThemeId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeId,
    pub language: Language,
    /// Adresse de l'agent (« IP:port »).
    pub agent_addr: String,
    /// Animation liquide (sélection d'onglets, filtres…).
    pub animation: Animation,
    /// Police et taille de chaque groupe de textes.
    pub fonts: FontPrefs,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeId::default(),
            language: Language::default(),
            agent_addr: format!("127.0.0.1:{}", nas_proto::DEFAULT_PORT),
            animation: Animation::default(),
            fonts: FontPrefs::default(),
        }
    }
}

fn path() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/prefs.json"));
    }
    directories::ProjectDirs::from("", "", "NAS").map(|d| d.config_dir().join("prefs.json"))
}

// --- Animation (reprise de WorkTable) ----------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize, Default)]
pub enum Curve {
    Linear,
    EaseIn,
    EaseOut,
    #[default]
    EaseInOut,
    /// Léger dépassement puis retour (effet « goutte »).
    Elastic,
}

impl Curve {
    pub const ALL: [Curve; 5] = [Curve::Linear, Curve::EaseIn, Curve::EaseOut, Curve::EaseInOut, Curve::Elastic];

    pub fn label(self) -> &'static str {
        match self {
            Curve::Linear => t!("Linéaire", "Linear"),
            Curve::EaseIn => "Ease in",
            Curve::EaseOut => "Ease out",
            Curve::EaseInOut => "Ease in-out",
            Curve::Elastic => t!("Élastique", "Elastic"),
        }
    }

    /// Progression `t` ∈ [0, 1] → valeur animée.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            Curve::EaseIn => t * t * t,
            Curve::EaseOut => 1.0 - (1.0 - t).powi(3),
            Curve::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Curve::Elastic => {
                // « back out » : dépasse légèrement la cible puis revient.
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Animation {
    /// Durée d'un déplacement, en millisecondes.
    pub duration_ms: f32,
    /// Opacité du surlignage liquide (0 à 1).
    pub opacity: f32,
    pub curve: Curve,
}

impl Default for Animation {
    fn default() -> Self {
        Animation { duration_ms: 450.0, opacity: 1.0, curve: Curve::EaseInOut }
    }
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
