//! Clé d'accès : la console doit la présenter à chaque demande
//! (en-tête `Authorization: Bearer <clé>`), sinon l'agent refuse.
//!
//! Créée au premier lancement (64 caractères hexadécimaux tirés au hasard) et gardée dans un
//! fichier lisible seulement par son propriétaire :
//! - en développement (`cargo run`) : `data/agent-key` à la racine du projet ;
//! - sous Linux : `/etc/nas-agent/key` ;
//! - ailleurs : `agent-key` à côté de l'exécutable.
//!
//! `NAS_AGENT_KEY_FILE` choisit un autre fichier. Pour changer de clé : supprimer le fichier et
//! relancer l'agent.

use std::path::PathBuf;

pub fn path() -> PathBuf {
    if let Ok(p) = std::env::var("NAS_AGENT_KEY_FILE") {
        return PathBuf::from(p);
    }
    if cfg!(debug_assertions) {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/agent-key");
    }
    if cfg!(target_os = "linux") {
        return PathBuf::from("/etc/nas-agent/key");
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("agent-key"))).unwrap_or_else(|| PathBuf::from("agent-key"))
}

/// Clé existante, ou nouvelle clé créée et enregistrée. `bool` : nouvelle clé.
pub fn load_or_create() -> Result<(String, bool), String> {
    let p = path();
    if let Ok(k) = std::fs::read_to_string(&p) {
        let k = k.trim().to_string();
        if k.len() >= 32 {
            return Ok((k, false));
        }
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("tirage au hasard impossible : {e}"))?;
    let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    }
    write_private(&p, &key).map_err(|e| format!("{} : {e}", p.display()))?;
    Ok((key, true))
}

/// Écrit le fichier avec les droits 600 sous Linux (lecture et écriture pour le propriétaire seul).
fn write_private(p: &std::path::Path, key: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(p)?.write_all(format!("{key}\n").as_bytes())
}

/// L'en-tête `Authorization` reçu porte-t-il la bonne clé ?
/// Comparaison en temps constant : la durée ne trahit pas combien de caractères sont justes.
pub fn authorized(header: Option<&str>, key: &str) -> bool {
    let Some(given) = header.and_then(|h| h.strip_prefix("Bearer ")) else { return false };
    let (a, b) = (given.trim().as_bytes(), key.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_key() {
        let key = "ab12cd34";
        assert!(authorized(Some("Bearer ab12cd34"), key));
        assert!(!authorized(Some("Bearer ab12cd35"), key));
        assert!(!authorized(Some("Bearer ab12"), key));
        assert!(!authorized(Some("ab12cd34"), key));
        assert!(!authorized(None, key));
    }

    #[test]
    fn creates_then_reuses() {
        let dir = std::env::temp_dir().join(format!("nas-key-test-{}", std::process::id()));
        let file = dir.join("key");
        // SAFETY : seul test qui touche à cette variable.
        unsafe { std::env::set_var("NAS_AGENT_KEY_FILE", &file) };
        let (k1, new1) = load_or_create().unwrap();
        let (k2, new2) = load_or_create().unwrap();
        assert!(new1 && !new2);
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
