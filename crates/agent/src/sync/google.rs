//! Vérification des jetons d'identité Google (`id_token` OpenID, signé RS256).
//!
//! Les clés publiques de Google (JWKS) sont téléchargées avec `curl` et gardées en mémoire
//! 6 heures ; une clé inconnue déclenche un nouveau téléchargement (au plus toutes les 5 min).
//! Rien n'est envoyé à Google : seule la liste publique des clés est lue.

use std::process::Command;
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::signature::Verifier;
use rsa::{BigUint, RsaPublicKey};
use serde_json::Value;
use sha2::Sha256;

use super::now;

const JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const KEEP_S: u64 = 6 * 3600;
const RETRY_S: u64 = 5 * 60;
/// Décalage d'horloge toléré.
const SKEW_S: u64 = 60;

/// Identité prouvée par le jeton.
pub struct Identity {
    /// Identifiant Google stable du compte.
    pub sub: String,
    /// E-mail vérifié, en minuscules.
    pub email: String,
}

struct Keys {
    fetched: u64,
    list: Vec<(String, RsaPublicKey)>,
}

static KEYS: Mutex<Keys> = Mutex::new(Keys { fetched: 0, list: Vec::new() });

/// Vérifie le jeton : signature, émetteur, destinataire (`client_ids`), expiration, e-mail vérifié.
pub fn verify(id_token: &str, client_ids: &[String]) -> Result<Identity, String> {
    if client_ids.is_empty() {
        return Err("connexion Google non configurée sur ce serveur".into());
    }
    let mut parts = id_token.trim().split('.');
    let (Some(h), Some(p), Some(sig), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err("jeton Google invalide".into());
    };
    let decode = |s: &str| -> Result<Value, String> {
        let raw = B64URL.decode(s).map_err(|_| "jeton Google invalide")?;
        serde_json::from_slice(&raw).map_err(|_| "jeton Google invalide".to_string())
    };
    let header = decode(h)?;
    if header["alg"] != "RS256" {
        return Err("jeton Google invalide".into());
    }
    let kid = header["kid"].as_str().ok_or("jeton Google invalide")?;
    let key = key(kid)?;
    let sig = B64URL.decode(sig).ok().and_then(|b| Signature::try_from(b.as_slice()).ok()).ok_or("jeton Google invalide")?;
    VerifyingKey::<Sha256>::new(key).verify(format!("{h}.{p}").as_bytes(), &sig).map_err(|_| "signature du jeton Google invalide")?;

    let claims = decode(p)?;
    let iss = claims["iss"].as_str().unwrap_or("");
    if iss != "accounts.google.com" && iss != "https://accounts.google.com" {
        return Err("jeton Google invalide".into());
    }
    if !claims["aud"].as_str().is_some_and(|aud| client_ids.iter().any(|c| c == aud)) {
        return Err("ce jeton Google n'est pas destiné à une app de ce serveur".into());
    }
    if claims["exp"].as_u64().is_none_or(|exp| exp + SKEW_S < now()) {
        return Err("jeton Google expiré".into());
    }
    let verified = claims["email_verified"].as_bool().unwrap_or(claims["email_verified"] == "true");
    let email = claims["email"].as_str().unwrap_or("").trim().to_lowercase();
    let sub = claims["sub"].as_str().unwrap_or("").to_string();
    if !verified || email.is_empty() || sub.is_empty() {
        return Err("e-mail Google non vérifié".into());
    }
    Ok(Identity { sub, email })
}

/// Clé publique `kid` (téléchargée si absente ou trop vieille).
fn key(kid: &str) -> Result<RsaPublicKey, String> {
    let mut k = KEYS.lock().unwrap_or_else(|e| e.into_inner());
    let t = now();
    let find = |k: &Keys| k.list.iter().find(|(id, _)| id == kid).map(|(_, key)| key.clone());
    let stale = t.saturating_sub(k.fetched) > KEEP_S;
    if !stale && let Some(key) = find(&k) {
        return Ok(key);
    }
    if stale || t.saturating_sub(k.fetched) > RETRY_S {
        // Daté même en cas d'échec : Google injoignable, on ne réessaie qu'après RETRY_S
        // (sinon chaque connexion attendrait curl, verrou tenu).
        k.fetched = t;
        k.list = fetch()?;
    }
    find(&k).ok_or_else(|| "clé Google inconnue".into())
}

fn fetch() -> Result<Vec<(String, RsaPublicKey)>, String> {
    let out = Command::new("curl").args(["-fsS", "--max-time", "10", JWKS_URL]).output().map_err(|e| format!("curl introuvable ({e})"))?;
    if !out.status.success() {
        return Err("clés Google injoignables (accès Internet du serveur ?)".into());
    }
    let jwks: Value = serde_json::from_slice(&out.stdout).map_err(|_| "clés Google illisibles")?;
    let list = jwks["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|k| k["kty"] == "RSA")
        .filter_map(|k| {
            let n = BigUint::from_bytes_be(&B64URL.decode(k["n"].as_str()?).ok()?);
            let e = BigUint::from_bytes_be(&B64URL.decode(k["e"].as_str()?).ok()?);
            Some((k["kid"].as_str()?.to_string(), RsaPublicKey::new(n, e).ok()?))
        })
        .collect::<Vec<_>>();
    if list.is_empty() {
        return Err("clés Google illisibles".into());
    }
    Ok(list)
}
