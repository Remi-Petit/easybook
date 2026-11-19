//! Client OpenID Connect : flux « authorization code » avec PKCE.
//!
//! Écrit à la main plutôt que tiré d'une bibliothèque : le flux tient en trois
//! appels (découverte, échange du code, clés de signature) et deux contraintes
//! propres aux conteneurs doivent rester sous contrôle — l'émetteur annoncé aux
//! navigateurs n'est pas toujours l'adresse que le serveur sait joindre
//! (`discovery_url`), et la validation d'un jeton ne doit jamais accepter un
//! algorithme symétrique.

use crate::accounts::random_token;
use crate::config;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Durée de validité d'une connexion en cours (de la redirection au retour).
pub const FLOW_TTL: Duration = Duration::from_secs(600);
const METADATA_TTL: Duration = Duration::from_secs(3600);
/// Délai minimal entre deux relectures des clés, pour qu'un jeton à `kid`
/// inconnu ne déclenche pas une requête sortante à chaque tentative.
const KEYS_REFETCH: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct OidcError(pub String);

impl std::fmt::Display for OidcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OidcError {}

type Result<T> = std::result::Result<T, OidcError>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(OidcError(message.into()))
}

#[derive(Debug, Deserialize)]
struct Metadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}

/// Ce que le fournisseur certifie sur la personne qui vient de s'authentifier.
#[derive(Debug, Clone, PartialEq)]
pub struct Claims {
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub username: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdToken {
    sub: String,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<Value>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

pub struct Provider {
    cfg: config::Oidc,
    http: reqwest::Client,
    metadata: RwLock<Option<(Instant, Arc<Metadata>)>>,
    keys: RwLock<Option<(Instant, Arc<Vec<Value>>)>>,
}

impl Provider {
    pub fn new(cfg: config::Oidc) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            // Un fournisseur qui redirige un point d'accès serveur à serveur est
            // suspect : on ne suit pas.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self {
            cfg,
            http,
            metadata: RwLock::new(None),
            keys: RwLock::new(None),
        }
    }

    pub fn config(&self) -> &config::Oidc {
        &self.cfg
    }

    pub fn issuer(&self) -> &str {
        &self.cfg.issuer
    }

    async fn metadata(&self) -> Result<Arc<Metadata>> {
        if let Some((at, meta)) = self.metadata.read().await.as_ref()
            && at.elapsed() < METADATA_TTL
        {
            return Ok(Arc::clone(meta));
        }
        let base = if self.cfg.discovery_url.trim().is_empty() {
            &self.cfg.issuer
        } else {
            &self.cfg.discovery_url
        };
        let url = format!(
            "{}/.well-known/openid-configuration",
            base.trim_end_matches('/')
        );
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| OidcError(format!("découverte {url} : {e}")))?;
        if !response.status().is_success() {
            return fail(format!("découverte {url} : HTTP {}", response.status()));
        }
        let meta: Metadata = response
            .json()
            .await
            .map_err(|e| OidcError(format!("découverte {url} illisible : {e}")))?;
        // Sans cette vérification, un serveur tiers pourrait se faire passer pour
        // l'émetteur configuré (OpenID Connect Discovery, §4.3).
        if meta.issuer.trim_end_matches('/') != self.cfg.issuer.trim_end_matches('/') {
            return fail(format!(
                "l'émetteur annoncé ({}) n'est pas celui configuré ({})",
                meta.issuer, self.cfg.issuer
            ));
        }
        let meta = Arc::new(meta);
        *self.metadata.write().await = Some((Instant::now(), Arc::clone(&meta)));
        Ok(meta)
    }

    /// Adresse du fournisseur où envoyer le navigateur.
    pub async fn authorization_url(
        &self,
        redirect_uri: &str,
        state: &str,
        nonce: &str,
        challenge: &str,
    ) -> Result<String> {
        let meta = self.metadata().await?;
        let mut url = reqwest::Url::parse(&meta.authorization_endpoint)
            .map_err(|e| OidcError(format!("authorization_endpoint invalide : {e}")))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &self.cfg.scopes.join(" "))
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    /// Échange le code contre un jeton d'identité, et le valide.
    pub async fn exchange(
        &self,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
        nonce: &str,
    ) -> Result<Claims> {
        let id_token = self
            .token_request(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
            ])
            .await?;
        self.verify_id_token(&id_token, Some(nonce)).await
    }

    /// Connexion **directe** (`grant_type=password`) : l'e-mail et le mot de passe
    /// du fournisseur sont vérifiés par lui, sans passer par sa page. Réservé au
    /// développement : easybook voit alors le mot de passe, ce que le flux avec
    /// redirection évite.
    pub async fn password_grant(&self, email: &str, password: &str) -> Result<Claims> {
        let id_token = self
            .token_request(&[
                ("grant_type", "password"),
                ("username", email),
                ("password", password),
                ("scope", "openid profile email"),
            ])
            .await?;
        // Pas de redirection, donc pas de `nonce` à comparer.
        self.verify_id_token(&id_token, None).await
    }

    /// Appelle le point d'accès jeton avec les identifiants du client.
    async fn token_request(&self, fields: &[(&str, &str)]) -> Result<String> {
        let meta = self.metadata().await?;
        let mut form = fields.to_vec();
        form.push(("client_id", self.cfg.client_id.as_str()));
        if !self.cfg.client_secret.is_empty() {
            form.push(("client_secret", self.cfg.client_secret.as_str()));
        }
        let response = self
            .http
            .post(&meta.token_endpoint)
            .form(&form)
            .send()
            .await
            .map_err(|e| OidcError(format!("point d'accès jeton : {e}")))?;
        if !response.status().is_success() {
            return fail(format!("point d'accès jeton : HTTP {}", response.status()));
        }
        let tokens: TokenResponse = response
            .json()
            .await
            .map_err(|e| OidcError(format!("réponse du point d'accès jeton : {e}")))?;
        Ok(tokens.id_token)
    }

    async fn verify_id_token(&self, token: &str, nonce: Option<&str>) -> Result<Claims> {
        let header =
            decode_header(token).map_err(|e| OidcError(format!("jeton illisible : {e}")))?;
        let algorithm = header.alg;
        // Les algorithmes symétriques sont refusés : la « clé » serait le secret
        // du client, que le fournisseur partage avec nous mais pas avec l'attaquant,
        // et accepter `HS256` ouvrirait la confusion de clé.
        if !matches!(
            algorithm,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::PS384
                | Algorithm::PS512
                | Algorithm::ES256
                | Algorithm::ES384
        ) {
            return fail(format!("algorithme de signature refusé : {algorithm:?}"));
        }

        let key = self.signing_key(header.kid.as_deref(), algorithm).await?;
        let mut validation = Validation::new(algorithm);
        validation.set_audience(&[&self.cfg.client_id]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 60;
        // `iss` est comparé plus bas : certains fournisseurs y ajoutent une barre
        // finale que la comparaison stricte de la bibliothèque refuserait.

        let data = decode::<serde_json::Map<String, Value>>(token, &key, &validation)
            .map_err(|e| OidcError(format!("jeton invalide : {e}")))?;
        let issuer = data
            .claims
            .get("iss")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if issuer.trim_end_matches('/') != self.cfg.issuer.trim_end_matches('/') {
            return fail(format!("émetteur du jeton inattendu : {issuer}"));
        }
        let claims: IdToken = serde_json::from_value(Value::Object(data.claims))
            .map_err(|e| OidcError(format!("jeton incomplet : {e}")))?;
        if let Some(nonce) = nonce
            && claims.nonce.as_deref() != Some(nonce)
        {
            return fail("nonce du jeton différent de celui de la demande");
        }
        if claims.sub.is_empty() {
            return fail("jeton sans sujet");
        }
        let verified = match &claims.email_verified {
            Some(Value::Bool(flag)) => *flag,
            Some(Value::String(text)) => text == "true",
            _ => false,
        };
        Ok(Claims {
            issuer: self.cfg.issuer.clone(),
            subject: claims.sub,
            email: claims.email.filter(|email| !email.trim().is_empty()),
            email_verified: verified,
            username: claims.preferred_username,
            name: claims.name,
        })
    }

    /// Clé de vérification pour ce `kid`, en relisant les clés du fournisseur si
    /// elle est inconnue (rotation).
    async fn signing_key(&self, kid: Option<&str>, algorithm: Algorithm) -> Result<DecodingKey> {
        if let Some(key) = pick_key(&self.cached_keys().await, kid, algorithm) {
            return Ok(key);
        }
        let fresh = self.fetch_keys().await?;
        pick_key(&fresh, kid, algorithm)
            .ok_or_else(|| OidcError("clé de signature inconnue".into()))
    }

    async fn cached_keys(&self) -> Arc<Vec<Value>> {
        match self.keys.read().await.as_ref() {
            Some((_, keys)) => Arc::clone(keys),
            None => Arc::new(Vec::new()),
        }
    }

    async fn fetch_keys(&self) -> Result<Arc<Vec<Value>>> {
        if let Some((at, keys)) = self.keys.read().await.as_ref()
            && at.elapsed() < KEYS_REFETCH
        {
            return Ok(Arc::clone(keys));
        }
        let meta = self.metadata().await?;
        let response = self
            .http
            .get(&meta.jwks_uri)
            .send()
            .await
            .map_err(|e| OidcError(format!("clés {} : {e}", meta.jwks_uri)))?;
        if !response.status().is_success() {
            return fail(format!(
                "clés {} : HTTP {}",
                meta.jwks_uri,
                response.status()
            ));
        }
        let set: Value = response
            .json()
            .await
            .map_err(|e| OidcError(format!("clés illisibles : {e}")))?;
        let keys = Arc::new(
            set.get("keys")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        *self.keys.write().await = Some((Instant::now(), Arc::clone(&keys)));
        Ok(keys)
    }
}

/// Choisit une clé de signature dans un JWKS lu à la main : Keycloak y mêle des
/// clés de chiffrement (`use: enc`) que les analyseurs stricts ne savent pas lire.
fn pick_key(keys: &[Value], kid: Option<&str>, algorithm: Algorithm) -> Option<DecodingKey> {
    let wanted_type = match algorithm {
        Algorithm::ES256 | Algorithm::ES384 => "EC",
        _ => "RSA",
    };
    let text =
        |key: &Value, field: &str| key.get(field).and_then(Value::as_str).map(str::to_string);
    let mut candidates = keys.iter().filter(|key| {
        text(key, "kty").as_deref() == Some(wanted_type)
            && text(key, "use").is_none_or(|usage| usage == "sig")
    });
    let key = match kid {
        Some(kid) => candidates.find(|key| text(key, "kid").as_deref() == Some(kid)),
        // Sans `kid` dans le jeton, une seule clé possible : sinon c'est ambigu.
        None => {
            let first = candidates.next()?;
            candidates.next().is_none().then_some(first)
        }
    }?;
    if wanted_type == "EC" {
        DecodingKey::from_ec_components(&text(key, "x")?, &text(key, "y")?).ok()
    } else {
        DecodingKey::from_rsa_components(&text(key, "n")?, &text(key, "e")?).ok()
    }
}

/// Couple PKCE : (vérificateur secret, empreinte envoyée au fournisseur).
pub fn pkce_pair() -> (String, String) {
    let verifier = random_token(32);
    let challenge = pkce_challenge(&verifier);
    (verifier, challenge)
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Destination après connexion : un chemin **de ce site** uniquement.
///
/// Sans ce filtre, `?next=https://pirate.example` ferait de la page de connexion
/// un tremplin de hameçonnage (redirection ouverte).
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(path)
            if path.starts_with('/')
                && !path.starts_with("//")
                && !path.contains('\\')
                && !path.chars().any(char::is_control) =>
        {
            path.to_string()
        }
        _ => "/".to_string(),
    }
}

/// But d'une connexion OIDC en cours.
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    /// Ouvrir une session.
    Login,
    /// Lier l'identité au compte déjà connecté.
    LinkSelf { user_id: String },
    /// Lier l'identité au compte visé par une invitation d'administrateur.
    LinkInvite { token: String },
}

#[derive(Debug, Clone)]
pub struct Pending {
    pub nonce: String,
    pub verifier: String,
    pub redirect_uri: String,
    pub next: String,
    pub intent: Intent,
    expires: Instant,
}

impl Pending {
    pub fn new(verifier: String, redirect_uri: String, next: String, intent: Intent) -> Self {
        Self {
            nonce: random_token(24),
            verifier,
            redirect_uri,
            next,
            intent,
            expires: Instant::now() + FLOW_TTL,
        }
    }
}

/// Connexions en cours, par `state`. En mémoire : un redémarrage ne perd que les
/// connexions commencées depuis moins de dix minutes.
#[derive(Default)]
pub struct Flows(Mutex<HashMap<String, Pending>>);

impl Flows {
    pub fn insert(&self, state: String, pending: Pending) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        map.retain(|_, flow| flow.expires > now);
        map.insert(state, pending);
    }

    /// Reprend (et retire) une connexion : un `state` ne sert qu'une fois.
    pub fn take(&self, state: &str) -> Option<Pending> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(state)
            .filter(|flow| flow.expires > Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_stays_on_this_site() {
        assert_eq!(
            safe_next(Some("/read/livre.epub?a=1")),
            "/read/livre.epub?a=1"
        );
        assert_eq!(safe_next(None), "/");
        assert_eq!(safe_next(Some("")), "/");
        assert_eq!(safe_next(Some("https://pirate.example")), "/");
        assert_eq!(safe_next(Some("//pirate.example")), "/");
        assert_eq!(safe_next(Some("/\\pirate.example")), "/");
        assert_eq!(safe_next(Some("/ok\r\nSet-Cookie: x=y")), "/");
        assert_eq!(safe_next(Some("javascript:alert(1)")), "/");
    }

    #[test]
    fn pkce_matches_the_rfc_vector() {
        // RFC 7636, annexe B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (verifier, challenge) = pkce_pair();
        assert!(verifier.len() >= 43);
        assert_eq!(pkce_challenge(&verifier), challenge);
    }

    #[test]
    fn a_flow_is_used_once_and_expires() {
        let flows = Flows::default();
        let pending = |expired: bool| {
            let mut pending =
                Pending::new("v".into(), "http://x/cb".into(), "/".into(), Intent::Login);
            if expired {
                pending.expires = Instant::now() - Duration::from_secs(1);
            }
            pending
        };
        flows.insert("a".into(), pending(false));
        flows.insert("b".into(), pending(true));

        assert!(flows.take("a").is_some());
        assert!(flows.take("a").is_none());
        assert!(flows.take("b").is_none());
        assert!(flows.take("inconnu").is_none());
    }

    #[test]
    fn keys_are_picked_by_kid_and_use() {
        let keys: Vec<Value> = serde_json::from_str(
            r#"[
              {"kty":"RSA","kid":"enc","use":"enc","n":"AQAB","e":"AQAB"},
              {"kty":"RSA","kid":"sig","use":"sig","n":"AQAB","e":"AQAB"},
              {"kty":"EC","kid":"ec","crv":"P-256","x":"AA","y":"AA"}
            ]"#,
        )
        .unwrap();
        assert!(pick_key(&keys, Some("sig"), Algorithm::RS256).is_some());
        // Une clé de chiffrement ne vérifie pas de signature.
        assert!(pick_key(&keys, Some("enc"), Algorithm::RS256).is_none());
        assert!(pick_key(&keys, Some("absente"), Algorithm::RS256).is_none());
        // Sans `kid` : accepté seulement s'il n'y a qu'une clé de signature.
        assert!(pick_key(&keys, None, Algorithm::RS256).is_some());
        let two: Vec<Value> = vec![keys[1].clone(), keys[1].clone()];
        assert!(pick_key(&two, None, Algorithm::RS256).is_none());
    }
}
