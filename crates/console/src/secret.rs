//! Clé d'accès de l'agent, rangée dans le coffre de mots de passe du système
//! (Gestionnaire d'identification sous Windows), jamais en clair dans les réglages.

const SERVICE: &str = "NAS";
const USER: &str = "agent-key";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, USER).map_err(|e| e.to_string())
}

/// Clé enregistrée (vide s'il n'y en a pas).
pub fn load() -> String {
    entry().and_then(|e| e.get_password().map_err(|e| e.to_string())).unwrap_or_default()
}

/// Enregistre la clé (ou l'efface si elle est vide).
pub fn save(key: &str) -> Result<(), String> {
    let e = entry()?;
    if key.is_empty() {
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    } else {
        e.set_password(key).map_err(|e| e.to_string())
    }
}
