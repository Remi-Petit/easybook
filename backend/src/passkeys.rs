//! Connexion par passkey (WebAuthn).
//!
//! Une passkey est une clé privée gardée par l'appareil (ou par son trousseau
//! synchronisé) et déverrouillée par la biométrie ou le code de l'appareil.
//! Elle est **liée au domaine** du site (`rp_id`) : c'est ce qui empêche le
//! hameçonnage, et c'est pourquoi `rp_id` ne doit jamais changer.
//!
//! Ce module ne garde que les **cérémonies en cours** (le défi et son état),
//! cinq minutes au plus, en mémoire. Le résultat vérifié (`Passkey`) est
//! enregistré dans la base des comptes par `Accounts`.
//!
//! Choix de sûreté :
//! - vérification faite par `webauthn-rs`, jamais à la main ;
//! - `userVerification` **requise** (code ou biométrie à chaque connexion) ;
//! - attestation `none` (aucun certificat d'appareil exigé : une passkey
//!   synchronisée n'en a pas) ;
//! - défi à usage unique, lié au navigateur par un cookie `HttpOnly`
//!   `SameSite=Strict` ;
//! - la connexion demande l'adresse e-mail, pour que la cérémonie ne dépende
//!   pas d'un identifiant de credential révélé à un visiteur anonyme.

use crate::accounts::User;
use crate::config;
use axum::http::HeaderMap;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, CredentialID, Passkey, PasskeyAuthentication, PasskeyRegistration,
    PublicKeyCredential, RegisterPublicKeyCredential, RequestChallengeResponse, Url, Webauthn,
    WebauthnBuilder,
};

/// Durée de vie d'une cérémonie : le temps de poser un doigt, sans plus.
pub const CEREMONY_TTL: Duration = Duration::from_secs(300);
/// Cérémonies simultanées gardées (tous comptes confondus) : au-delà, la plus
/// ancienne est oubliée plutôt que de laisser la mémoire croître.
const PENDING_MAX: usize = 256;

/// Ce qui empêche une cérémonie d'aboutir.
#[derive(Debug)]
pub enum PasskeyError {
    /// Aucun domaine utilisable (adresse IP, configuration vide) : le
    /// navigateur ne peut pas créer de passkey.
    Unavailable,
    /// Défi absent, expiré ou déjà consommé.
    Expired,
    /// Le défi appartient à un autre compte que la session.
    OtherUser,
    /// Signature, origine ou drapeaux invalides.
    Rejected(String),
    /// Erreur inattendue (sérialisation, lecture de la base…).
    Internal(String),
}

impl From<webauthn_rs::prelude::WebauthnError> for PasskeyError {
    fn from(e: webauthn_rs::prelude::WebauthnError) -> Self {
        Self::Rejected(e.to_string())
    }
}

struct Pending {
    webauthn: Webauthn,
    state: Ceremony,
    user_id: String,
    /// Nom donné par la personne à la passkey (création seulement).
    name: String,
    created: Instant,
}

enum Ceremony {
    Register(PasskeyRegistration),
    Authenticate(PasskeyAuthentication),
}

/// Une cérémonie sortie du magasin : à vérifier avec *son* `Webauthn` (celui
/// de l'origine où elle a commencé, pas celle de la requête de retour).
pub struct RegistrationPending {
    pub user_id: String,
    pub name: String,
    webauthn: Webauthn,
    state: PasskeyRegistration,
}

pub struct AuthenticationPending {
    pub user_id: String,
    webauthn: Webauthn,
    state: PasskeyAuthentication,
}

impl RegistrationPending {
    /// Vérifie la réponse du navigateur et rend la passkey à ranger.
    pub fn finish(self, credential: &RegisterPublicKeyCredential) -> Result<Passkey, PasskeyError> {
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &self.state)?;
        Ok(passkey)
    }
}

impl AuthenticationPending {
    /// Vérifie la signature. L'appelant a déjà chargé la passkey de ce compte
    /// portant ce credential ; la vérification refait foi.
    pub fn finish(
        &self,
        credential: &PublicKeyCredential,
    ) -> Result<webauthn_rs::prelude::AuthenticationResult, PasskeyError> {
        let result = self
            .webauthn
            .finish_passkey_authentication(credential, &self.state)?;
        // Ceinture et bretelles : `userVerification` est déjà exigée dans le
        // défi, mais une passkey ne vaut que si l'appareil a vérifié la personne.
        if !result.user_verified() {
            return Err(PasskeyError::Rejected("user verification missing".into()));
        }
        Ok(result)
    }
}

/// Les cérémonies en cours et les réglages du site.
pub struct Passkeys {
    settings: config::WebAuthn,
    pending: Mutex<HashMap<String, Pending>>,
}

impl Passkeys {
    #[must_use]
    pub fn new(settings: config::WebAuthn) -> Self {
        Self {
            settings,
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.settings.enabled
    }

    /// Le navigateur peut-il créer ou utiliser une passkey ici ? Faux sur une
    /// adresse IP (WebAuthn l'interdit) ou quand le domaine ne se déduit pas.
    pub fn available(&self, headers: &HeaderMap) -> bool {
        self.webauthn(headers).is_some()
    }

    /// `rp_id` et `Webauthn` pour cette requête. Le domaine vient de la
    /// configuration, sinon du `Host` (sans le port).
    fn webauthn(&self, headers: &HeaderMap) -> Option<(String, Webauthn)> {
        if !self.settings.enabled {
            return None;
        }
        let rp_id = self.rp_id(headers)?;
        let origin = Url::parse(&self.origin(headers)).ok()?;
        let webauthn = WebauthnBuilder::new(&rp_id, &origin)
            .ok()?
            .rp_name(&self.settings.rp_name)
            .timeout(CEREMONY_TTL)
            .build()
            .ok()?;
        Some((rp_id, webauthn))
    }

    fn rp_id(&self, headers: &HeaderMap) -> Option<String> {
        let configured = self.settings.rp_id.trim();
        if !configured.is_empty() {
            return valid_rp_id(configured).then(|| configured.to_string());
        }
        let host = host_name(headers)?;
        valid_rp_id(host).then(|| host.to_string())
    }

    fn origin(&self, headers: &HeaderMap) -> String {
        let configured = self.settings.origin.trim();
        if !configured.is_empty() {
            return configured.to_string();
        }
        let scheme = if is_https(headers) { "https" } else { "http" };
        let host = headers
            .get("x-forwarded-host")
            .or_else(|| headers.get(axum::http::header::HOST))
            .and_then(|value| value.to_str().ok())
            .unwrap_or("localhost");
        format!("{scheme}://{host}")
    }

    /// Commence la création d'une passkey pour ce compte.
    pub fn begin_registration(
        &self,
        headers: &HeaderMap,
        user: &User,
        name: &str,
        exclude: Option<Vec<CredentialID>>,
    ) -> Result<(CreationChallengeResponse, String), PasskeyError> {
        let (_, webauthn) = self.webauthn(headers).ok_or(PasskeyError::Unavailable)?;
        let id = Uuid::parse_str(&user.id)
            .map_err(|e| PasskeyError::Internal(format!("identifiant de compte : {e}")))?;
        let user_name = user.email.clone().unwrap_or_else(|| user.username.clone());
        let display = user
            .display_name
            .clone()
            .unwrap_or_else(|| user.username.clone());
        let (options, state) = webauthn.start_passkey_registration(
            id,
            &user_name,
            &display,
            exclude.filter(|list| !list.is_empty()),
        )?;
        let token = self.store(Pending {
            webauthn,
            state: Ceremony::Register(state),
            user_id: user.id.clone(),
            name: name.trim().to_string(),
            created: Instant::now(),
        });
        Ok((options, token))
    }

    /// Commence une connexion pour ce compte, avec ses passkeys comme seules
    /// clés acceptées.
    pub fn begin_authentication(
        &self,
        headers: &HeaderMap,
        user_id: &str,
        creds: &[Passkey],
    ) -> Result<(RequestChallengeResponse, String), PasskeyError> {
        let (_, webauthn) = self.webauthn(headers).ok_or(PasskeyError::Unavailable)?;
        let (options, state) = webauthn.start_passkey_authentication(creds)?;
        let token = self.store(Pending {
            webauthn,
            state: Ceremony::Authenticate(state),
            user_id: user_id.to_string(),
            name: String::new(),
            created: Instant::now(),
        });
        Ok((options, token))
    }

    /// Retire la cérémonie de création ; `None` si le défi a expiré.
    pub fn take_registration(
        &self,
        token: Option<&str>,
        user_id: &str,
    ) -> Result<RegistrationPending, PasskeyError> {
        let pending = self.take(token)?;
        if pending.user_id != user_id {
            return Err(PasskeyError::OtherUser);
        }
        match pending.state {
            Ceremony::Register(state) => Ok(RegistrationPending {
                user_id: pending.user_id,
                name: pending.name,
                webauthn: pending.webauthn,
                state,
            }),
            Ceremony::Authenticate(_) => Err(PasskeyError::Expired),
        }
    }

    /// Retire la cérémonie de connexion ; `None` si le défi a expiré.
    pub fn take_authentication(
        &self,
        token: Option<&str>,
    ) -> Result<AuthenticationPending, PasskeyError> {
        let pending = self.take(token)?;
        match pending.state {
            Ceremony::Authenticate(state) => Ok(AuthenticationPending {
                user_id: pending.user_id,
                webauthn: pending.webauthn,
                state,
            }),
            Ceremony::Register(_) => Err(PasskeyError::Expired),
        }
    }

    fn store(&self, pending: Pending) -> String {
        let token = crate::accounts::random_token(32);
        let mut map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, entry| entry.created.elapsed() < CEREMONY_TTL);
        if map.len() >= PENDING_MAX
            && let Some(oldest) = map
                .iter()
                .min_by_key(|(_, entry)| entry.created)
                .map(|(token, _)| token.clone())
        {
            map.remove(&oldest);
        }
        map.insert(token.clone(), pending);
        token
    }

    fn take(&self, token: Option<&str>) -> Result<Pending, PasskeyError> {
        let Some(token) = token.filter(|token| !token.is_empty()) else {
            return Err(PasskeyError::Expired);
        };
        let mut map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let Some(pending) = map.remove(token) else {
            return Err(PasskeyError::Expired);
        };
        if pending.created.elapsed() >= CEREMONY_TTL {
            return Err(PasskeyError::Expired);
        }
        Ok(pending)
    }
}

/// Une adresse IP ne peut pas être un `rp_id` WebAuthn : il faut un nom.
fn valid_rp_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.contains(['/', ':', ' '])
        && value != "localhost."
        && value.parse::<std::net::IpAddr>().is_err()
}

/// Le nom d'hôte, sans le port ni les crochets d'une adresse IPv6.
fn host_name(headers: &HeaderMap) -> Option<&str> {
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(axum::http::header::HOST))
        .and_then(|value| value.to_str().ok())?
        .trim();
    if host.starts_with('[') {
        return host
            .split_once(']')
            .map(|(inside, _)| &host[..inside.len() + 1]);
    }
    Some(host.rsplit_once(':').map_or(host, |(name, _)| name))
}

fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|proto| proto.eq_ignore_ascii_case("https"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(host: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(
            axum::http::header::HOST,
            HeaderValue::from_str(host).unwrap(),
        );
        map
    }

    fn settings() -> config::WebAuthn {
        config::WebAuthn::default()
    }

    fn user() -> User {
        User {
            id: Uuid::new_v4().to_string(),
            username: "claire".into(),
            display_name: Some("Claire".into()),
            email: Some("claire@example.org".into()),
            role: crate::accounts::Role::User,
            disabled: false,
            has_password: true,
            created_at: 0,
            last_login_at: None,
            must_change_password: false,
        }
    }

    #[test]
    fn the_domain_comes_from_the_host_without_its_port() {
        let passkeys = Passkeys::new(settings());
        assert_eq!(
            passkeys.rp_id(&headers("easybook.example.org:5200")),
            Some("easybook.example.org".to_string())
        );
        assert_eq!(
            passkeys.origin(&headers("easybook.example.org:5200")),
            "http://easybook.example.org:5200"
        );
        let mut https = headers("easybook.example.org");
        https.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert_eq!(passkeys.origin(&https), "https://easybook.example.org");
    }

    #[test]
    fn an_ip_address_cannot_be_a_relying_party() {
        let passkeys = Passkeys::new(settings());
        assert!(!passkeys.available(&headers("127.0.0.1:5200")));
        assert!(!passkeys.available(&headers("[::1]:5200")));
        // `localhost` est le seul nom local accepté par WebAuthn.
        assert!(passkeys.available(&headers("localhost:5200")));
    }

    #[test]
    fn the_configuration_wins_over_the_request() {
        let passkeys = Passkeys::new(config::WebAuthn {
            rp_id: "easybook.example.org".into(),
            origin: "https://easybook.example.org".into(),
            ..settings()
        });
        assert_eq!(
            passkeys.rp_id(&headers("127.0.0.1:5200")),
            Some("easybook.example.org".to_string())
        );
        assert_eq!(
            passkeys.origin(&headers("127.0.0.1:5200")),
            "https://easybook.example.org"
        );
    }

    #[test]
    fn a_disabled_site_offers_nothing() {
        let passkeys = Passkeys::new(config::WebAuthn {
            enabled: false,
            ..settings()
        });
        assert!(!passkeys.available(&headers("easybook.example.org")));
    }

    #[test]
    fn a_ceremony_starts_with_its_options_and_a_single_use_token() {
        let passkeys = Passkeys::new(settings());
        let user = user();
        let (options, token) = passkeys
            .begin_registration(&headers("localhost:5200"), &user, "Portable", None)
            .unwrap();
        // Le défi est bien celui du navigateur, et la sélection d'authentificateur
        // (vérification de la personne) accompagne la création.
        assert!(!options.public_key.challenge.is_empty());
        assert!(options.public_key.authenticator_selection.is_some());
        let pending = passkeys
            .take_registration(Some(&token), &user.id)
            .expect("cérémonie en attente");
        assert_eq!(pending.name, "Portable");
        // Le jeton ne sert qu'une fois.
        assert!(passkeys.take_registration(Some(&token), &user.id).is_err());
    }

    #[test]
    fn a_ceremony_belongs_to_one_account_only() {
        let passkeys = Passkeys::new(settings());
        let user = user();
        let (_, token) = passkeys
            .begin_registration(&headers("localhost:5200"), &user, "Portable", None)
            .unwrap();
        assert!(matches!(
            passkeys.take_registration(Some(&token), "autre"),
            Err(PasskeyError::OtherUser)
        ));
    }

    #[test]
    fn an_absent_or_unknown_token_is_expired() {
        let passkeys = Passkeys::new(settings());
        assert!(matches!(
            passkeys.take_authentication(None),
            Err(PasskeyError::Expired)
        ));
        assert!(matches!(
            passkeys.take_authentication(Some("inconnu")),
            Err(PasskeyError::Expired)
        ));
    }
}
