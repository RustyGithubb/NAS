//! Données échangées entre la console (Windows) et l'agent (Mac mini), en JSON sur HTTP.
//!
//! Routes de l'agent :
//! - `GET /api/status` → [`Status`]

use serde::{Deserialize, Serialize};

/// Version du protocole : la console refuse un agent d'une autre version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Port par défaut de l'agent.
pub const DEFAULT_PORT: u16 = 7878;

/// État de la machine à un instant donné.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub protocol: u32,
    pub agent_version: String,
    /// Nom de la machine sur le réseau.
    pub hostname: String,
    /// Système (« Debian GNU/Linux 13 », « Windows 11 »…).
    pub os: String,
    /// Temps écoulé depuis le démarrage de la machine (secondes).
    pub uptime_s: u64,
    pub cpu: Cpu,
    pub memory: Memory,
    pub disks: Vec<Disk>,
    /// Température la plus haute relevée (°C) ; `None` si le système ne la donne pas.
    pub temperature_c: Option<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cpu {
    /// Modèle du processeur.
    pub model: String,
    /// Utilisation globale (0 à 100).
    pub usage: f32,
    /// Utilisation de chaque cœur (0 à 100).
    pub per_core: Vec<f32>,
}

/// Mémoire vive et mémoire d'échange (swap), en octets.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Memory {
    pub used: u64,
    pub total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
}

/// Disque ou partition, en octets.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Disk {
    /// Point de montage (« / », « C:\ »).
    pub mount: String,
    /// Système de fichiers (« ext4 », « NTFS »).
    pub fs: String,
    pub used: u64,
    pub total: u64,
}
