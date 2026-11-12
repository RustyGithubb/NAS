//! Client de synchronisation avec le NAS, commun à toutes les apps (WorkTable d'abord).
//!
//! L'app garde ses fichiers en local et appelle :
//! - [`Client::register`] (premier login, avec le code d'invitation donné par la console) ou
//!   [`Client::login`] → une [`Session`] (jeton à ranger dans le coffre du système) ;
//! - [`Client::push`] pour envoyer un fichier modifié (chiffré ici) ;
//! - [`Client::pull`] puis [`Client::fetch`] pour recevoir ceux des autres postes.
//!
//! Les règles de fusion en cas de conflit appartiennent à l'app.
//! Bloquant (ureq) : à appeler depuis un fil à part, jamais depuis l'interface.

pub mod crypto;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nas_proto::sync::{self as proto, DeviceHello, DocMeta, Login, Pull, Push, PushResult, Register, SYNC_PROTOCOL_VERSION, Salt};

pub use crypto::{AppKey, MasterKey};

/// Session ouverte sur le serveur.
#[derive(Clone, Debug)]
pub struct Session {
    pub token: String,
    pub device_id: u64,
    pub login: String,
    pub master: MasterKey,
    pub apps: Vec<proto::PublicApp>,
}

/// Erreur lisible (en français, affichable telle quelle).
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Serveur injoignable (hors ligne, adresse fausse…) : réessayer plus tard.
    Offline(String),
    /// Session expirée ou révoquée : se reconnecter.
    SignedOut,
    /// Refus ou problème signalé par le serveur.
    Server(u16, String),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Offline(e) => write!(f, "serveur injoignable ({e})"),
            Error::SignedOut => f.write_str("session expirée ou révoquée : reconnectez-vous"),
            Error::Server(_, m) | Error::Other(m) => f.write_str(m),
        }
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Error::Other(e)
    }
}

/// Client d'une app pour un serveur.
pub struct Client {
    base: String,
    app: String,
    device: String,
    agent: ureq::Agent,
    token: Option<String>,
}

impl Client {
    /// `server` : « https://sync.exemple.fr » (ou « http://IP:7878 » en réseau local / en test).
    pub fn new(server: &str, app: &str, device_name: &str) -> Self {
        let agent = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(8)).timeout(std::time::Duration::from_secs(120)).build();
        let mut base = server.trim().trim_end_matches('/').to_string();
        if !base.starts_with("http://") && !base.starts_with("https://") {
            base = format!("https://{base}");
        }
        Client { base, app: app.into(), device: device_name.into(), agent, token: None }
    }

    /// Délai de connexion plus court (démarrage de l'app : ne pas la faire attendre hors ligne).
    pub fn connect_timeout(mut self, d: std::time::Duration) -> Self {
        self.agent = ureq::AgentBuilder::new().timeout_connect(d).timeout(std::time::Duration::from_secs(120)).build();
        self
    }

    /// Reprend une session enregistrée.
    pub fn with_token(mut self, token: &str) -> Self {
        self.token = Some(token.into());
        self
    }

    fn hello(&self) -> DeviceHello {
        DeviceHello { name: self.device.clone(), app: self.app.clone(), protocol: SYNC_PROTOCOL_VERSION }
    }

    fn req(&self, method: &str, path: &str) -> ureq::Request {
        let r = self.agent.request(method, &format!("{}{path}", self.base));
        match &self.token {
            Some(t) => r.set("Authorization", &format!("Bearer {t}")),
            None => r,
        }
    }

    fn salt(&self, login: &str) -> Result<Salt, Error> {
        json(self.req("GET", &format!("/sync/auth/salt?login={}", enc(login))).call())
    }

    /// Premier login : choisit le mot de passe. Renvoie aussi le **code de récupération**, à
    /// montrer une seule fois (seul moyen de relire les données si le mot de passe est perdu).
    pub fn register(&mut self, login: &str, invite: &str, password: &str) -> Result<(Session, String), Error> {
        let salt = self.salt(login)?;
        if !salt.pending {
            return Err(Error::Other("ce compte est déjà activé : utilisez « Se connecter »".into()));
        }
        let keys = crypto::password_keys(password, &salt.salt, salt.m_kib, salt.t)?;
        let master = MasterKey::generate();
        let recovery = crypto::recovery_code();
        let body = Register {
            login: login.into(),
            invite: invite.into(),
            auth_key: keys.auth_key.clone(),
            wrapped_key: master.wrap(&keys),
            recovery_key: master.wrap_recovery(&recovery),
            device: self.hello(),
        };
        let s: proto::Session = json(self.req("POST", "/sync/auth/register").send_json(body))?;
        self.token = Some(s.token.clone());
        Ok((Session { token: s.token, device_id: s.device_id, login: s.login, master, apps: s.apps }, recovery))
    }

    pub fn login(&mut self, login: &str, password: &str) -> Result<Session, Error> {
        let salt = self.salt(login)?;
        if salt.pending {
            return Err(Error::Other("compte pas encore activé : utilisez le code d'invitation".into()));
        }
        let keys = crypto::password_keys(password, &salt.salt, salt.m_kib, salt.t)?;
        let s: proto::Session = json(self.req("POST", "/sync/auth/login").send_json(Login { login: login.into(), auth_key: keys.auth_key.clone(), device: self.hello() }))?;
        let master = MasterKey::unwrap(&s.wrapped_key, &keys)?;
        self.token = Some(s.token.clone());
        Ok(Session { token: s.token, device_id: s.device_id, login: s.login, master, apps: s.apps })
    }

    pub fn logout(&mut self) -> Result<(), Error> {
        let r: Result<proto::Ok, Error> = json(self.req("POST", "/sync/auth/logout").call());
        self.token = None;
        r.map(|_| ())
    }

    /// Envoie une version d'un fichier. `base` : version reçue ou envoyée la dernière fois
    /// (0 pour un nouveau fichier). En cas de conflit, `PushResult::conflict` dit quoi télécharger
    /// pour fusionner avant de renvoyer.
    pub fn push(&self, key: &AppKey, path: &str, base: u64, content: Option<&[u8]>) -> Result<PushResult, Error> {
        let (data, deleted) = match content {
            Some(c) => (key.encrypt(path, c), false),
            None => (Vec::new(), true),
        };
        if data.len() as u64 > proto::MAX_DOC {
            return Err(Error::Other(format!("{path} : trop gros pour la synchronisation (8 Mo maximum)")));
        }
        let body = Push { doc: key.doc_id(path), base, hash: crypto::sha256_hex(&data), deleted, data: B64.encode(&data) };
        json(self.req("POST", &format!("/sync/{}/push", self.app)).send_json(body))
    }

    /// Documents changés depuis `since` (refaire tant que `more`).
    pub fn pull(&self, since: u64) -> Result<Pull, Error> {
        json(self.req("GET", &format!("/sync/{}/pull?since={since}", self.app)).call())
    }

    /// Télécharge et déchiffre une version : (chemin, contenu).
    pub fn fetch(&self, key: &AppKey, meta: &DocMeta) -> Result<(String, Vec<u8>), Error> {
        let r = self.req("GET", &format!("/sync/{}/blob/{}/{}", self.app, meta.doc, meta.version)).call().map_err(error)?;
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut r.into_reader(), &mut data).map_err(|e| Error::Offline(e.to_string()))?;
        if crypto::sha256_hex(&data) != meta.hash {
            return Err(Error::Other("contenu abîmé pendant le téléchargement".into()));
        }
        Ok(key.decrypt(&data)?)
    }

    pub fn status(&self) -> Result<proto::AppStatus, Error> {
        json(self.req("GET", &format!("/sync/{}/status", self.app)).call())
    }

    pub fn devices(&self) -> Result<Vec<proto::Device>, Error> {
        json(self.req("GET", "/sync/devices").call())
    }

    pub fn revoke_device(&self, id: u64) -> Result<(), Error> {
        json::<proto::Ok>(self.req("POST", &format!("/sync/devices/{id}/revoke")).call()).map(|_| ())
    }

    /// Change le mot de passe (la clé maître ne change pas : rien n'est rechiffré).
    pub fn change_password(&self, login: &str, master: &MasterKey, old: &str, new: &str) -> Result<(), Error> {
        let salt = self.salt(login)?;
        let old_keys = crypto::password_keys(old, &salt.salt, salt.m_kib, salt.t)?;
        let new_salt = crypto::hex(&crypto::random::<16>());
        let new_keys = crypto::password_keys(new, &new_salt, salt.m_kib, salt.t)?;
        let body = proto::ChangePassword { old_auth_key: old_keys.auth_key.clone(), new_salt, new_auth_key: new_keys.auth_key.clone(), new_wrapped_key: master.wrap(&new_keys) };
        json::<proto::Ok>(self.req("POST", "/sync/auth/password").send_json(body)).map(|_| ())
    }
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn error(e: ureq::Error) -> Error {
    match e {
        ureq::Error::Status(401, _) => Error::SignedOut,
        ureq::Error::Status(code, r) => {
            let msg = r.into_json::<serde_json::Value>().ok().and_then(|v| v.get("error").and_then(|m| m.as_str()).map(str::to_string));
            Error::Server(code, msg.unwrap_or_else(|| format!("erreur HTTP {code}")))
        }
        ureq::Error::Transport(t) => Error::Offline(t.to_string()),
    }
}

fn json<T: serde::de::DeserializeOwned>(r: Result<ureq::Response, ureq::Error>) -> Result<T, Error> {
    let r = r.map_err(error)?;
    r.into_json().map_err(|_| Error::Other("réponse illisible du serveur".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parcours complet contre un agent qui tourne :
    /// `NAS_SYNC_TEST=http://127.0.0.1:7878 NAS_SYNC_TEST_INVITE=<code> cargo test -p nas-sync-client -- --ignored`
    /// (compte « test » créé dans la console avec WorkTable autorisée).
    #[test]
    #[ignore]
    fn round_trip_against_agent() {
        let server = std::env::var("NAS_SYNC_TEST").expect("NAS_SYNC_TEST");
        let invite = std::env::var("NAS_SYNC_TEST_INVITE").expect("NAS_SYNC_TEST_INVITE");
        let mut a = Client::new(&server, proto::WORKTABLE, "PC-TEST-A");
        let (s, recovery) = a.register("test", &invite, "mot de passe 1").unwrap();
        let key = s.master.app(proto::WORKTABLE);
        let r = a.push(&key, "notes/Idées.md", 0, Some("# Idées\n- synchro".as_bytes())).unwrap();
        assert!(r.ok);

        // Second poste : login, réception, déchiffrement.
        let mut b = Client::new(&server, proto::WORKTABLE, "PC-TEST-B");
        let s2 = b.login("test", "mot de passe 1").unwrap();
        assert_eq!(s2.master, s.master);
        let pull = b.pull(0).unwrap();
        let (path, content) = b.fetch(&s2.master.app(proto::WORKTABLE), &pull.docs[0]).unwrap();
        assert_eq!(path, "notes/Idées.md");
        assert_eq!(content, "# Idées\n- synchro".as_bytes());

        // Conflit : B envoie sur une base périmée.
        assert!(a.push(&key, "notes/Idées.md", r.version, Some(b"v2")).unwrap().ok);
        let c = b.push(&key, "notes/Idées.md", r.version, Some(b"v2 bis")).unwrap();
        assert!(!c.ok && c.conflict.is_some());

        // Mot de passe changé : l'ancien ne marche plus, la clé maître reste la même.
        a.change_password("test", &s.master, "mot de passe 1", "mot de passe 2").unwrap();
        assert!(Client::new(&server, proto::WORKTABLE, "X").login("test", "mot de passe 1").is_err());
        assert_eq!(Client::new(&server, proto::WORKTABLE, "X").login("test", "mot de passe 2").unwrap().master, s.master);
        assert_eq!(recovery.len(), 29);
    }
}
