//! Faux fournisseur OpenID Connect, sur un vrai port local.
//!
//! Il parle le protocole pour de bon (découverte, `authorize`, `token` avec
//! vérification de PKCE et du secret du client, `jwks`) et signe ses jetons en
//! RS256 : c'est le code d'easybook qui est exercé, pas un double.
//! [`Quirks`] permet de lui faire commettre les fautes qu'un client doit refuser.
//!
//! Les clés sont des clés de test jetables, sans aucune valeur.

#![allow(dead_code)]

use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use easybook::config;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const IDP_KEY: &str = include_str!("keys/idp.pem");
const OTHER_KEY: &str = include_str!("keys/other.pem");
/// Module public de `keys/idp.pem` (base64 URL).
const IDP_MODULUS: &str = "1t8L_11Wl56b-D88vr0EXnE4ZtJ8iR98SpxinkmpYzSIkdv55L3xhmQKQppQF7pe6l3F0SCD_bHc1MYlzKVejDlNn8MN_g2IUlHFqZbWlEpFT7SmnPtWa2z2R58GIwAtFGMjqNtjqChzCH9yWQdwelArv5LcU0jkDdAWjCml72VizmVTjbhKfZdE5v8Bfsaf2rLrw38lPZcXqPsSNY8MlnNjJhqooZJI3mIsvoL_C1KEE12HJeqIenCAeEtAeh-4tDglr68OeojtoLhtvlhWkEoX5ryMtj0BQANM5eZdeLnhU4uvhhIzp6k9oh9ToBKEPZIjQpR9pAw3QAR3FQVm-w";
const KID: &str = "test-key";

#[derive(Clone, Default)]
pub struct Person {
    pub email: Option<String>,
    pub email_verified: bool,
    pub username: Option<String>,
    pub name: Option<String>,
}

impl Person {
    pub fn new(username: &str, email: &str) -> Self {
        Self {
            email: Some(email.into()),
            email_verified: true,
            username: Some(username.into()),
            name: Some(username.to_uppercase()),
        }
    }
}

/// Fautes que le fournisseur peut commettre.
#[derive(Clone, Default)]
pub struct Quirks {
    /// `aud` d'un autre client.
    pub audience: Option<String>,
    /// `iss` d'un autre émetteur.
    pub issuer: Option<String>,
    /// `nonce` différent de celui de la demande.
    pub nonce: Option<String>,
    /// Jeton échu depuis une heure.
    pub expired: bool,
    /// Signé par une clé que le JWKS ne publie pas.
    pub foreign_key: bool,
    /// Signé en HS256 avec le secret du client (confusion d'algorithme).
    pub hs256: bool,
    /// Découverte qui annonce un autre émetteur que celui joint.
    pub discovery_issuer: Option<String>,
    /// Le point d'accès jeton répond 500.
    pub token_error: bool,
}

struct Grant {
    sub: String,
    nonce: String,
    challenge: String,
    redirect_uri: String,
    scope: String,
}

struct Shared {
    base: String,
    client_id: String,
    client_secret: String,
    people: Mutex<HashMap<String, Person>>,
    codes: Mutex<HashMap<String, Grant>>,
    quirks: Mutex<Quirks>,
    /// Dernière valeur de `scope` demandée.
    scopes: Mutex<Vec<String>>,
}

pub struct Idp {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    shared: Arc<Shared>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Idp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Idp {
    pub async fn start() -> Idp {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shared = Arc::new(Shared {
            base: base.clone(),
            client_id: "easybook".into(),
            client_secret: "secret-de-test".into(),
            people: Mutex::default(),
            codes: Mutex::default(),
            quirks: Mutex::default(),
            scopes: Mutex::default(),
        });
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .route("/jwks", get(jwks))
            .with_state(Arc::clone(&shared));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Idp {
            issuer: base,
            client_id: shared.client_id.clone(),
            client_secret: shared.client_secret.clone(),
            shared,
            task,
        }
    }

    /// Réglages à donner à easybook pour parler à ce fournisseur.
    pub fn settings(&self) -> config::Oidc {
        config::Oidc {
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            label: "Faux IdP".into(),
            ..config::Oidc::default()
        }
    }

    pub fn person(&self, sub: &str, person: Person) {
        self.shared
            .people
            .lock()
            .unwrap()
            .insert(sub.into(), person);
    }

    pub fn quirks(&self, quirks: Quirks) {
        *self.shared.quirks.lock().unwrap() = quirks;
    }

    pub fn last_scopes(&self) -> Vec<String> {
        self.shared.scopes.lock().unwrap().clone()
    }

    /// Joue le navigateur chez le fournisseur : s'authentifie en tant que `sub`
    /// sur l'adresse d'autorisation, et rend l'adresse de retour (avec `code`).
    pub async fn authenticate(&self, authorize_url: &str, sub: &str) -> String {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let url = format!("{authorize_url}&login_hint={sub}");
        let response = client.get(&url).send().await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::SEE_OTHER,
            "authorize : {url}"
        );
        response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }
}

/// `http://hôte/chemin?requête` → `/chemin?requête`.
pub fn path_and_query(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.find('/')
        .map_or("/".into(), |at| rest[at..].to_string())
}

async fn discovery(State(s): State<Arc<Shared>>) -> Response {
    let issuer = s
        .quirks
        .lock()
        .unwrap()
        .discovery_issuer
        .clone()
        .unwrap_or_else(|| s.base.clone());
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{}/authorize", s.base),
        "token_endpoint": format!("{}/token", s.base),
        "jwks_uri": format!("{}/jwks", s.base),
    }))
    .into_response()
}

async fn jwks() -> Response {
    Json(json!({ "keys": [
        // Clé de chiffrement : Keycloak en publie à côté des clés de signature.
        { "kty": "RSA", "kid": "enc-key", "use": "enc", "alg": "RSA-OAEP", "n": IDP_MODULUS, "e": "AQAB" },
        { "kty": "RSA", "kid": KID, "use": "sig", "alg": "RS256", "n": IDP_MODULUS, "e": "AQAB" },
    ]}))
    .into_response()
}

async fn authorize(
    State(s): State<Arc<Shared>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let get = |name: &str| q.get(name).cloned().unwrap_or_default();
    if get("client_id") != s.client_id
        || get("response_type") != "code"
        || get("code_challenge_method") != "S256"
        || get("code_challenge").is_empty()
        || get("state").is_empty()
        || get("nonce").is_empty()
    {
        return (StatusCode::BAD_REQUEST, "demande d'autorisation invalide").into_response();
    }
    let sub = get("login_hint");
    if !s.people.lock().unwrap().contains_key(&sub) {
        return (StatusCode::BAD_REQUEST, "utilisateur inconnu").into_response();
    }
    let scope = get("scope");
    *s.scopes.lock().unwrap() = scope.split(' ').map(str::to_string).collect();

    let code = format!("code-{}", s.codes.lock().unwrap().len() + 1);
    s.codes.lock().unwrap().insert(
        code.clone(),
        Grant {
            sub,
            nonce: get("nonce"),
            challenge: get("code_challenge"),
            redirect_uri: get("redirect_uri"),
            scope,
        },
    );
    let back = format!("{}?code={code}&state={}", get("redirect_uri"), get("state"));
    (StatusCode::SEE_OTHER, [(header::LOCATION, back)]).into_response()
}

async fn token(State(s): State<Arc<Shared>>, Form(f): Form<HashMap<String, String>>) -> Response {
    let quirks = s.quirks.lock().unwrap().clone();
    if quirks.token_error {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let get = |name: &str| f.get(name).cloned().unwrap_or_default();
    let reject = |why: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant", "why": why })),
        )
            .into_response()
    };

    if get("grant_type") != "authorization_code"
        || get("client_id") != s.client_id
        || get("client_secret") != s.client_secret
    {
        return reject("client");
    }
    // Un code ne sert qu'une fois.
    let Some(grant) = s.codes.lock().unwrap().remove(&get("code")) else {
        return reject("code");
    };
    let verifier = get("code_verifier");
    if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != grant.challenge {
        return reject("pkce");
    }
    if get("redirect_uri") != grant.redirect_uri || !grant.scope.contains("openid") {
        return reject("redirect_uri");
    }

    let person = s.people.lock().unwrap().get(&grant.sub).cloned().unwrap();
    let now = easybook::accounts::now();
    let claims = json!({
        "iss": quirks.issuer.clone().unwrap_or_else(|| s.base.clone()),
        "aud": quirks.audience.clone().unwrap_or_else(|| s.client_id.clone()),
        "sub": grant.sub,
        "iat": now,
        "exp": if quirks.expired { now - 3600 } else { now + 300 },
        "nonce": quirks.nonce.clone().unwrap_or(grant.nonce),
        "email": person.email,
        "email_verified": person.email_verified,
        "preferred_username": person.username,
        "name": person.name,
    });
    let (algorithm, key) = if quirks.hs256 {
        (
            Algorithm::HS256,
            EncodingKey::from_secret(s.client_secret.as_bytes()),
        )
    } else if quirks.foreign_key {
        (
            Algorithm::RS256,
            EncodingKey::from_rsa_pem(OTHER_KEY.as_bytes()).unwrap(),
        )
    } else {
        (
            Algorithm::RS256,
            EncodingKey::from_rsa_pem(IDP_KEY.as_bytes()).unwrap(),
        )
    };
    let mut header = Header::new(algorithm);
    header.kid = Some(KID.into());
    let id_token = jsonwebtoken::encode(&header, &claims, &key).unwrap();
    Json(json!({ "access_token": "inutile", "token_type": "Bearer", "id_token": id_token }))
        .into_response()
}
