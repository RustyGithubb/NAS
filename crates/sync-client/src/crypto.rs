//! Chiffrement de bout en bout, fait sur le poste : le serveur ne voit jamais le mot de passe,
//! les noms de fichiers ni leur contenu.
//!
//! - Mot de passe + sel du compte → Argon2id → 64 octets : clé d'authentification (envoyée au
//!   serveur, qui la re-hache) et clé d'emballage (jamais envoyée).
//! - Clé maître du compte : aléatoire, gardée par le serveur **emballée** (chiffrée) par la clé
//!   d'emballage, et une seconde fois par le code de récupération.
//! - Clé de chaque app : dérivée de la clé maître (HKDF) ; une app ne lit pas les données d'une autre.
//! - Contenu : AES-256-GCM (nonce aléatoire de 12 octets en tête). Noms : HMAC-SHA256 du chemin.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// Clés tirées du mot de passe.
pub struct PasswordKeys {
    /// Envoyée au serveur (hexadécimal).
    pub auth_key: String,
    /// Emballe la clé maître ; ne quitte jamais le poste.
    wrap_key: [u8; 32],
}

/// Clé maître d'un compte.
#[derive(Clone, PartialEq, Eq)]
pub struct MasterKey(pub [u8; 32]);

/// Clés d'une app : contenu et noms.
#[derive(Clone)]
pub struct AppKey {
    content: [u8; 32],
    names: [u8; 32],
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(…)")
    }
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("générateur aléatoire du système");
    b
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Dérive les clés du mot de passe (paramètres donnés par le serveur : `Salt`).
pub fn password_keys(password: &str, salt_hex: &str, m_kib: u32, t: u32) -> Result<PasswordKeys, String> {
    let salt = from_hex(salt_hex).filter(|s| s.len() >= 8).ok_or("sel invalide")?;
    let params = argon2::Params::new(m_kib.max(8 * 1024), t.max(1), 1, Some(64)).map_err(|e| e.to_string())?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 64];
    a.hash_password_into(password.as_bytes(), &salt, &mut out).map_err(|e| e.to_string())?;
    let mut wrap_key = [0u8; 32];
    wrap_key.copy_from_slice(&out[32..]);
    Ok(PasswordKeys { auth_key: hex(&out[..32]), wrap_key })
}

impl MasterKey {
    pub fn generate() -> Self {
        MasterKey(random())
    }

    /// Emballée par la clé du mot de passe (base64, pour le serveur).
    pub fn wrap(&self, keys: &PasswordKeys) -> String {
        B64.encode(seal(&keys.wrap_key, &self.0))
    }

    pub fn unwrap(wrapped: &str, keys: &PasswordKeys) -> Result<Self, String> {
        Self::open_with(&keys.wrap_key, wrapped)
    }

    /// Emballée par le code de récupération.
    pub fn wrap_recovery(&self, code: &str) -> String {
        B64.encode(seal(&recovery_key(code), &self.0))
    }

    pub fn unwrap_recovery(wrapped: &str, code: &str) -> Result<Self, String> {
        Self::open_with(&recovery_key(code), wrapped)
    }

    fn open_with(key: &[u8; 32], wrapped: &str) -> Result<Self, String> {
        let data = B64.decode(wrapped).map_err(|_| "clé emballée illisible".to_string())?;
        let plain = open(key, &data).map_err(|_| "mot de passe ou code incorrect".to_string())?;
        let k: [u8; 32] = plain.try_into().map_err(|_| "clé maître invalide".to_string())?;
        Ok(MasterKey(k))
    }

    /// Clés d'une app.
    pub fn app(&self, app: &str) -> AppKey {
        let hk = hkdf::Hkdf::<Sha256>::new(Some(b"nas-sync v1"), &self.0);
        let (mut content, mut names) = ([0u8; 32], [0u8; 32]);
        hk.expand(format!("app:{app}:content").as_bytes(), &mut content).expect("longueur valide");
        hk.expand(format!("app:{app}:names").as_bytes(), &mut names).expect("longueur valide");
        AppKey { content, names }
    }
}

impl AppKey {
    /// Identifiant (opaque pour le serveur) d'un chemin de fichier de l'app.
    pub fn doc_id(&self, path: &str) -> String {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.names).expect("clé HMAC");
        mac.update(path.as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    /// Chiffre un document. Le chemin est chiffré avec le contenu (pour le retrouver au pull).
    pub fn encrypt(&self, path: &str, content: &[u8]) -> Vec<u8> {
        let mut plain = Vec::with_capacity(4 + path.len() + content.len());
        plain.extend_from_slice(&(path.len() as u32).to_le_bytes());
        plain.extend_from_slice(path.as_bytes());
        plain.extend_from_slice(content);
        seal(&self.content, &plain)
    }

    /// Déchiffre un document : (chemin, contenu).
    pub fn decrypt(&self, data: &[u8]) -> Result<(String, Vec<u8>), String> {
        let plain = open(&self.content, data).map_err(|_| "document illisible (clé différente ou contenu abîmé)".to_string())?;
        let len = u32::from_le_bytes(plain.get(..4).ok_or("document tronqué")?.try_into().expect("4 octets")) as usize;
        let path = plain.get(4..4 + len).ok_or("document tronqué")?;
        let path = String::from_utf8(path.to_vec()).map_err(|_| "chemin invalide")?;
        Ok((path, plain[4 + len..].to_vec()))
    }
}

/// Code de récupération lisible : 6 groupes de 4 (≈ 120 bits).
pub fn recovery_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let r: [u8; 24] = random();
    let s: Vec<String> = r.chunks(4).map(|c| c.iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect()).collect();
    s.join("-")
}

fn recovery_key(code: &str) -> [u8; 32] {
    let clean: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
    let hk = hkdf::Hkdf::<Sha256>::new(Some(b"nas-sync recovery v1"), clean.as_bytes());
    let mut k = [0u8; 32];
    hk.expand(b"wrap", &mut k).expect("longueur valide");
    k
}

fn seal(key: &[u8; 32], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(key).expect("clé de 32 octets");
    let nonce: [u8; 12] = random();
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(Nonce::from_slice(&nonce), plain).expect("chiffrement"));
    out
}

fn open(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, ()> {
    if data.len() < 12 + 16 {
        return Err(());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ())?;
    cipher.decrypt(Nonce::from_slice(&data[..12]), &data[12..]).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(pw: &str) -> PasswordKeys {
        // Paramètres réduits : tests rapides.
        password_keys(pw, "00112233445566778899aabbccddeeff", 8 * 1024, 1).unwrap()
    }

    #[test]
    fn wrap_and_unwrap() {
        let k = keys("secret");
        let m = MasterKey::generate();
        let w = m.wrap(&k);
        assert_eq!(MasterKey::unwrap(&w, &k).unwrap(), m);
        assert!(MasterKey::unwrap(&w, &keys("autre")).is_err());
    }

    #[test]
    fn recovery() {
        let m = MasterKey::generate();
        let code = recovery_code();
        assert_eq!(code.len(), 29);
        let w = m.wrap_recovery(&code);
        // Tolère minuscules et espaces à la saisie.
        assert_eq!(MasterKey::unwrap_recovery(&w, &code.to_lowercase().replace('-', " ")).unwrap(), m);
        assert!(MasterKey::unwrap_recovery(&w, "AAAA-BBBB").is_err());
    }

    #[test]
    fn documents() {
        let m = MasterKey::generate();
        let a = m.app("worktable");
        let data = a.encrypt("notes/Idées.md", b"# Bonjour");
        assert_eq!(a.decrypt(&data).unwrap(), ("notes/Idées.md".to_string(), b"# Bonjour".to_vec()));
        // Une autre app ne lit pas les documents de WorkTable, et ses identifiants diffèrent.
        assert!(m.app("photos").decrypt(&data).is_err());
        assert_ne!(a.doc_id("x"), m.app("photos").doc_id("x"));
        assert!(nas_proto::sync::valid_doc_id(&a.doc_id("notes/Idées.md")));
        // Même chemin → même identifiant ; contenu chiffré différent à chaque fois.
        assert_eq!(a.doc_id("a"), a.doc_id("a"));
        assert_ne!(a.encrypt("a", b"x"), a.encrypt("a", b"x"));
    }

    #[test]
    fn auth_key_is_stable() {
        assert_eq!(keys("pw").auth_key, keys("pw").auth_key);
        assert_ne!(keys("pw").auth_key, keys("pw2").auth_key);
        assert_eq!(keys("pw").auth_key.len(), 64);
    }
}
