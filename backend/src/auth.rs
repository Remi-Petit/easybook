//! Authentification : sessions par cookie, comptes locaux, OIDC, administration.
//!
//! Tout `/api` (et `/ws`) exige une session, sauf les routes de connexion. Le
//! cookie de session est `HttpOnly` (le JavaScript de la page ne le lit pas) et
//! `SameSite=Lax` ; en plus, une requête qui modifie quelque chose et dont
//! l'en-tête `Origin` désigne un autre site est refusée.
//!
//! | Route | Accès |
//! |---|---|
//! | `GET /api/auth/session` | public — qui suis-je, et comment se connecter |
//! | `GET /api/auth/check` | connecté — 204 ; contrôle d'accès du cache nginx |
//! | `POST /api/auth/login`, `POST /api/auth/logout` | public |
//! | `POST /api/auth/password` | connecté |
//! | `GET /api/auth/oidc/login` | public — `?next=`, `?link=<invitation>` |
//! | `GET /api/auth/oidc/link` | connecté — lie son propre compte |
//! | `GET /api/auth/oidc/callback` | public — retour du fournisseur |
//! | `DELETE /api/auth/identities/{id}` | connecté — délie sa propre identité |
//! | `/api/admin/users…` | administrateur |

// `Response` comme erreur : c'est ce que les routes axum renvoient telle quelle.
#![allow(clippy::result_large_err)]

use crate::accounts::{
    AccountError, Accounts, Identity, NewUser, Role, User, UserPatch, random_token,
};
use crate::api::AppState;
use crate::config;
use crate::oidc::{Claims, Flows, Intent, Pending, Provider, pkce_pair, safe_next};
use axum::body::Body;
use axum::extract::{FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SESSION_COOKIE: &str = "easybook_session";
/// Retient le `state` de la connexion OIDC en cours : lie le retour du
/// fournisseur au navigateur qui l'a demandé (sans lui, on pourrait faire
/// ouvrir à quelqu'un la session d'un autre).
const FLOW_COOKIE: &str = "easybook_oidc";

/// Essais ratés avant blocage temporaire, par identifiant et par client.
const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(300);
/// Validité d'une invitation de liaison.
const LINK_TOKEN_TTL: i64 = 900;

/// Ce que l'API sait de l'authentification : les comptes, le fournisseur OIDC
/// éventuel, et les connexions en cours.
pub struct AuthState {
    pub accounts: Arc<Accounts>,
    provider: Option<Provider>,
    flows: Flows,
    throttle: Throttle,
    session_ttl: i64,
    /// Connexion directe au fournisseur depuis le formulaire (développement).
    direct: bool,
    /// Comptes à un clic de la page de connexion : (e-mail, mot de passe). Le mot
    /// de passe ne quitte jamais le serveur. Développement seulement.
    dev_logins: Vec<(String, String)>,
}

impl AuthState {
    pub fn new(accounts: Arc<Accounts>, settings: config::Auth) -> Self {
        let provider = settings
            .oidc
            .is_enabled()
            .then(|| Provider::new(settings.oidc));
        Self {
            accounts,
            provider,
            flows: Flows::default(),
            throttle: Throttle::default(),
            session_ttl: i64::from(settings.session_days) * 86_400,
            direct: false,
            dev_logins: Vec::new(),
        }
    }

    /// Boutons de connexion rapide : `email:mot_de_passe,email:mot_de_passe`.
    /// **Développement seulement** (voir `config::dev_mode`).
    #[must_use]
    pub fn with_dev_logins(mut self, raw: &str) -> Self {
        self.dev_logins = parse_dev_logins(raw);
        self
    }

    /// Accepte, au formulaire de connexion, l'e-mail et le mot de passe d'un
    /// compte du fournisseur (`grant_type=password`). **Développement
    /// seulement** ; sans effet si OIDC n'est pas configuré.
    #[must_use]
    pub fn with_direct_login(mut self) -> Self {
        self.direct = self.provider.is_some();
        self
    }

    pub fn direct_login(&self) -> bool {
        self.direct
    }

    pub fn oidc(&self) -> Option<&Provider> {
        self.provider.as_ref()
    }
}

/// `a@x.org:mdp1,b@x.org:mdp2` → `[(a@x.org, mdp1), (b@x.org, mdp2)]`. Une entrée
/// sans `:` ou à e-mail vide est ignorée ; le mot de passe peut contenir `:`.
pub fn parse_dev_logins(raw: &str) -> Vec<(String, String)> {
    raw.split(',')
        .filter_map(|entry| {
            let (email, password) = entry.trim().split_once(':')?;
            let email = email.trim();
            (!email.is_empty() && !password.is_empty())
                .then(|| (email.to_string(), password.to_string()))
        })
        .collect()
}

/// L'utilisateur de la requête (posé par [`guard`]).
#[derive(Clone)]
pub struct CurrentUser(pub User);

/// Extracteur : exige une session valide.
pub struct SessionUser(pub User);

impl<S: Send + Sync> FromRequestParts<S> for SessionUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Response> {
        parts
            .extensions
            .get::<CurrentUser>()
            .map(|current| SessionUser(current.0.clone()))
            .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "unauthenticated"))
    }
}

/// Extracteur : la session, si elle existe.
pub struct MaybeUser(pub Option<User>);

impl<S: Send + Sync> FromRequestParts<S> for MaybeUser {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(MaybeUser(
            parts.extensions.get::<CurrentUser>().map(|c| c.0.clone()),
        ))
    }
}

// ── Garde ────────────────────────────────────────────────────────────────

/// Routes accessibles sans session.
fn is_public(path: &str) -> bool {
    matches!(
        path,
        "/api/health"
            | "/api/auth/session"
            | "/api/auth/login"
            | "/api/auth/dev-login"
            | "/api/auth/logout"
            | "/api/auth/oidc/login"
            | "/api/auth/oidc/callback"
    )
}

/// Exige une session pour tout `/api` et `/ws`, et un administrateur pour
/// `/api/admin/…`. Sans comptes configurés (`auth.enabled: false`), ne fait rien.
pub async fn guard(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let Some(auth) = state.auth() else {
        return next.run(request).await;
    };
    // Le préambule CORS n'embarque jamais de cookie : il passe toujours.
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    if !same_origin(request.method(), request.headers()) {
        return error(StatusCode::FORBIDDEN, "cross_origin");
    }

    let token = cookie_value(request.headers(), SESSION_COOKIE);
    let user = match token {
        Some(token) => auth.accounts.session_user(&token).ok().flatten(),
        None => None,
    };

    let path = request.uri().path().to_string();
    match user {
        Some(user) => {
            if path.starts_with("/api/admin/") && user.role != Role::Admin {
                return error(StatusCode::FORBIDDEN, "forbidden");
            }
            request.extensions_mut().insert(CurrentUser(user));
        }
        None if !is_public(&path) => return error(StatusCode::UNAUTHORIZED, "unauthenticated"),
        None => {}
    }
    next.run(request).await
}

/// Une requête qui modifie l'état doit venir de ce site. Seuls les **noms
/// d'hôte** sont comparés : nginx transmet `Host` sans le port.
fn same_origin(method: &Method, headers: &HeaderMap) -> bool {
    if matches!(*method, Method::GET | Method::HEAD) {
        return true;
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        // Pas d'`Origin` : client qui n'est pas un navigateur, ou même origine.
        return true;
    };
    let origin_host = origin
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or_default())
        .map(host_only);
    let own = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .map(host_only);
    matches!((origin_host, own), (Some(a), Some(b)) if !a.is_empty() && a.eq_ignore_ascii_case(b))
}

/// `exemple.org:5200` → `exemple.org` ; `[::1]:80` → `[::1]`.
fn host_only(host: &str) -> &str {
    let host = host.trim();
    if host.starts_with('[') {
        return host
            .split_once(']')
            .map_or(host, |(inside, _)| &host[..inside.len() + 1]);
    }
    host.rsplit_once(':').map_or(host, |(name, _)| name)
}

// ── Cookies et réponses ──────────────────────────────────────────────────

pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_string())
        })
}

fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|proto| proto.eq_ignore_ascii_case("https"))
}

fn cookie(name: &str, value: &str, max_age: i64, path: &str, secure: bool) -> String {
    format!(
        "{name}={value}; Path={path}; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(serde_json::json!({ "error": code }))).into_response()
}

fn account_error(e: AccountError) -> Response {
    let status = match e {
        AccountError::InvalidUsername | AccountError::InvalidEmail | AccountError::WeakPassword => {
            StatusCode::BAD_REQUEST
        }
        AccountError::UsernameTaken
        | AccountError::EmailTaken
        | AccountError::IdentityTaken
        | AccountError::LastAdmin
        | AccountError::LastCredential => StatusCode::CONFLICT,
        AccountError::NotFound => StatusCode::NOT_FOUND,
        AccountError::Internal(ref detail) => {
            eprintln!("comptes : {detail}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    error(status, e.code())
}

fn internal(e: impl std::fmt::Display) -> Response {
    eprintln!("auth : {e}");
    error(StatusCode::INTERNAL_SERVER_ERROR, "internal")
}

fn redirect(to: &str, cookies: Vec<String>) -> Response {
    let location = HeaderValue::from_str(to).unwrap_or_else(|_| HeaderValue::from_static("/"));
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    response.headers_mut().insert(header::LOCATION, location);
    for value in cookies {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// Adresse publique du site, telle que le navigateur l'a utilisée.
fn public_origin(headers: &HeaderMap) -> String {
    let scheme = if is_https(headers) { "https" } else { "http" };
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{scheme}://{host}")
}

/// Exécute un accès aux comptes sur le pool bloquant.
async fn db<T: Send + 'static>(
    auth: &Arc<AuthState>,
    work: impl FnOnce(&Accounts) -> T + Send + 'static,
) -> Result<T, Response> {
    let accounts = Arc::clone(&auth.accounts);
    tokio::task::spawn_blocking(move || work(&accounts))
        .await
        .map_err(internal)
}

fn auth_of(state: &AppState) -> Result<Arc<AuthState>, Response> {
    state
        .auth()
        .cloned()
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "auth_disabled"))
}

// ── Limitation des essais ────────────────────────────────────────────────

#[derive(Default)]
struct Throttle(Mutex<HashMap<String, (u32, Instant)>>);

impl Throttle {
    /// Secondes à attendre, si cette clé est bloquée.
    fn blocked(&self, key: &str) -> Option<u64> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let (count, since) = *map.get(key)?;
        let elapsed = since.elapsed();
        if elapsed >= LOCKOUT {
            map.remove(key);
            return None;
        }
        (count >= MAX_FAILURES).then(|| (LOCKOUT - elapsed).as_secs() + 1)
    }

    fn failure(&self, key: &str) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        map.retain(|_, (_, since)| since.elapsed() < LOCKOUT);
        let entry = map.entry(key.to_string()).or_insert((0, now));
        entry.0 += 1;
        // La fenêtre court à partir du dernier échec.
        entry.1 = now;
    }

    fn success(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}

/// Le client, tel que nginx l'a vu : la **dernière** adresse de
/// `X-Forwarded-For`, la seule qu'un client ne peut pas écrire lui-même.
fn client_key(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|list| list.rsplit(',').next())
        .map_or_else(|| "-".to_string(), |ip| ip.trim().to_string())
}

// ── Routes ───────────────────────────────────────────────────────────────

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/session", get(session))
        .route("/api/auth/check", get(check))
        .route("/api/auth/login", post(login))
        .route("/api/auth/dev-login", post(dev_login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(change_password))
        .route("/api/auth/password/skip", post(skip_password_change))
        .route("/api/auth/oidc/login", get(oidc_login))
        .route("/api/auth/oidc/link", get(oidc_link))
        .route("/api/auth/oidc/callback", get(oidc_callback))
        .route("/api/auth/identities/{id}", delete(unlink_own_identity))
        .route("/api/admin/users", get(list_users).post(create_user))
        .route(
            "/api/admin/users/{id}",
            axum::routing::patch(update_user).delete(delete_user),
        )
        .route("/api/admin/users/{id}/password", put(admin_set_password))
        .route("/api/admin/users/{id}/link-token", post(create_link_token))
        .route(
            "/api/admin/users/{id}/identities",
            post(admin_link_identity),
        )
        .route(
            "/api/admin/users/{id}/identities/{identity}",
            delete(admin_unlink_identity),
        )
}

#[derive(Serialize)]
struct UserView {
    #[serde(flatten)]
    user: User,
    identities: Vec<Identity>,
}

fn view(accounts: &Accounts, user: User) -> Result<UserView, AccountError> {
    let identities = accounts.identities_of(&user.id)?;
    Ok(UserView { user, identities })
}

/// `GET /api/auth/session` — toujours 200 : l'interface s'en sert pour décider
/// d'afficher la page de connexion.
async fn session(State(state): State<AppState>, MaybeUser(user): MaybeUser) -> Response {
    let Some(auth) = state.auth().cloned() else {
        return Json(serde_json::json!({ "enabled": false, "user": null, "oidc": null }))
            .into_response();
    };
    let user = match user {
        Some(user) => match db(&auth, move |accounts| view(accounts, user)).await {
            Ok(Ok(view)) => Some(view),
            Ok(Err(e)) => return account_error(e),
            Err(response) => return response,
        },
        None => None,
    };
    let oidc = auth.oidc().map(|provider| {
        serde_json::json!({
            "label": provider.config().label,
            "self_link": provider.config().self_link,
            "direct": auth.direct,
        })
    });
    let dev_emails: Vec<&str> = auth.dev_logins.iter().map(|(e, _)| e.as_str()).collect();
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "enabled": true, "user": user, "oidc": oidc, "dev_logins": dev_emails })),
    )
        .into_response()
}

/// `GET /api/auth/check` — 204 pour une session valide (la garde répond 401
/// sinon). Sert à nginx (`auth_request`) devant le cache des planches.
async fn check() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

fn session_cookies(headers: &HeaderMap, token: &str, ttl: i64) -> String {
    cookie(SESSION_COOKIE, token, ttl, "/", is_https(headers))
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    sign_in(&auth, &headers, body.email, body.password).await
}

#[derive(Deserialize)]
struct DevLoginBody {
    email: String,
}

/// `POST /api/auth/dev-login` — connexion à un clic pour un compte de la liste de
/// développement (`EASYBOOK_DEV_LOGINS`). Inexistante en production.
async fn dev_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DevLoginBody>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    let password = auth
        .dev_logins
        .iter()
        .find(|(email, _)| email.eq_ignore_ascii_case(body.email.trim()))
        .map(|(_, password)| password.clone())
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "not_found"))?;
    sign_in(&auth, &headers, body.email, password).await
}

async fn sign_in(
    auth: &Arc<AuthState>,
    headers: &HeaderMap,
    email: String,
    password: String,
) -> Result<Response, Response> {
    let key = format!("{}|{}", email.trim().to_lowercase(), client_key(headers));
    if let Some(wait) = auth.throttle.blocked(&key) {
        let mut response = error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
        if let Ok(value) = HeaderValue::from_str(&wait.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        return Err(response);
    }

    let ttl = auth.session_ttl;
    let (local_email, local_password) = (email.clone(), password.clone());
    let mut outcome = db(
        auth,
        move |accounts| -> Result<Option<(User, String)>, AccountError> {
            let Some(user) = accounts.authenticate(&local_email, &local_password)? else {
                return Ok(None);
            };
            let token = accounts.create_session(&user.id, ttl)?;
            accounts.touch_login(&user.id)?;
            Ok(Some((user, token)))
        },
    )
    .await?
    .map_err(account_error)?;

    // Un compte local d'abord ; sinon, en développement, le fournisseur lui-même.
    if outcome.is_none() {
        outcome = direct_login(auth, &email, &password).await?;
    }

    let Some((user, token)) = outcome else {
        auth.throttle.failure(&key);
        return Err(error(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    };
    auth.throttle.success(&key);

    let cookie = session_cookies(headers, &token, ttl);
    let user = db(auth, move |accounts| view(accounts, user))
        .await?
        .map_err(account_error)?;
    Ok(([(header::SET_COOKIE, cookie)], Json(user)).into_response())
}

/// Connexion directe au fournisseur (`grant_type=password`), si elle est permise.
///
/// Le compte easybook se décide comme pour un retour de redirection : identité
/// déjà liée, sinon `link_by_email`, sinon `auto_create`, sinon refus (`403`).
async fn direct_login(
    auth: &Arc<AuthState>,
    email: &str,
    password: &str,
) -> Result<Option<(User, String)>, Response> {
    let Some(provider) = auth.oidc().filter(|_| auth.direct) else {
        return Ok(None);
    };
    let claims = match provider.password_grant(email, password).await {
        Ok(claims) => claims,
        Err(e) => {
            eprintln!("OIDC (connexion directe) : {e}");
            return Ok(None);
        }
    };

    let (settings, ttl) = (provider.config().clone(), auth.session_ttl);
    let resolved = db(auth, move |accounts| {
        let resolved = resolve(accounts, &settings, &Intent::Login, None, &claims, ttl)?;
        let session = resolved.session.ok_or("internal")?;
        let user = accounts
            .session_user(&session)
            .map_err(|_| "internal")?
            .ok_or("internal")?;
        Ok::<_, &'static str>((user, session))
    })
    .await?;
    match resolved {
        Ok(found) => Ok(Some(found)),
        Err(code) => Err(error(StatusCode::FORBIDDEN, code)),
    }
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    if let Some(token) = cookie_value(&headers, SESSION_COOKIE) {
        db(&auth, move |accounts| accounts.delete_session(&token))
            .await?
            .map_err(account_error)?;
    }
    let expired = cookie(SESSION_COOKIE, "", 0, "/", is_https(&headers));
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, expired)]).into_response())
}

#[derive(Deserialize)]
struct PasswordBody {
    #[serde(default)]
    current: String,
    new: String,
}

/// `POST /api/auth/password` — change **son** mot de passe. Toutes les autres
/// sessions du compte sont fermées ; celle-ci est renouvelée.
async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    SessionUser(user): SessionUser,
    Json(body): Json<PasswordBody>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    let ttl = auth.session_ttl;
    let key = format!("pw|{}", user.id);
    if let Some(wait) = auth.throttle.blocked(&key) {
        let mut response = error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
        if let Ok(value) = HeaderValue::from_str(&wait.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        return Err(response);
    }

    let id = user.id.clone();
    let result = db(
        &auth,
        move |accounts| -> Result<Option<String>, AccountError> {
            // Un compte qui n'a pas encore de mot de passe (OIDC seul) peut en poser un ;
            // un mot de passe temporaire se change sans le redonner, la personne vient de le saisir.
            if user.has_password
                && !user.must_change_password
                && !accounts.check_user_password(&id, &body.current)?
            {
                return Ok(None);
            }
            accounts.set_password(&id, &body.new)?;
            Ok(Some(accounts.create_session(&id, ttl)?))
        },
    )
    .await?
    .map_err(account_error)?;

    let Some(token) = result else {
        auth.throttle.failure(&key);
        return Err(error(StatusCode::FORBIDDEN, "wrong_password"));
    };
    auth.throttle.success(&key);
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session_cookies(&headers, &token, ttl))],
    )
        .into_response())
}

async fn skip_password_change(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    db(&auth, move |accounts| {
        accounts.clear_must_change_password(&user.id)
    })
    .await?
    .map_err(account_error)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn unlink_own_identity(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Path(identity): Path<String>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    db(&auth, move |accounts| {
        accounts.unlink_identity(&user.id, &identity)
    })
    .await?
    .map_err(account_error)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ── OIDC ─────────────────────────────────────────────────────────────────

fn provider_of(auth: &AuthState) -> Result<&Provider, Response> {
    auth.oidc()
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "oidc_disabled"))
}

fn redirect_uri(provider: &Provider, headers: &HeaderMap) -> String {
    let configured = provider.config().redirect_url.trim();
    if configured.is_empty() {
        format!("{}/api/auth/oidc/callback", public_origin(headers))
    } else {
        configured.to_string()
    }
}

/// Où renvoyer le navigateur quand la connexion échoue.
fn failure_page(intent: &Intent, code: &str) -> String {
    match intent {
        Intent::LinkSelf { .. } => format!("/account?error={code}"),
        _ => format!("/login?error={code}"),
    }
}

/// Commence une connexion OIDC : mémorise le contexte et envoie le navigateur
/// chez le fournisseur.
async fn begin(auth: &AuthState, headers: &HeaderMap, next: String, intent: Intent) -> Response {
    let provider = match provider_of(auth) {
        Ok(provider) => provider,
        Err(response) => return response,
    };
    let redirect_uri = redirect_uri(provider, headers);
    let state = random_token(24);
    let (verifier, challenge) = pkce_pair();
    let pending = Pending::new(verifier, redirect_uri.clone(), next, intent.clone());
    let nonce = pending.nonce.clone();

    match provider
        .authorization_url(&redirect_uri, &state, &nonce, &challenge)
        .await
    {
        Ok(url) => {
            auth.flows.insert(state.clone(), pending);
            let flow_cookie = cookie(
                FLOW_COOKIE,
                &state,
                crate::oidc::FLOW_TTL.as_secs() as i64,
                "/api/auth/oidc",
                is_https(headers),
            );
            redirect(&url, vec![flow_cookie])
        }
        Err(e) => {
            eprintln!("OIDC : {e}");
            redirect(&failure_page(&intent, "oidc_failed"), Vec::new())
        }
    }
}

#[derive(Deserialize)]
struct LoginQuery {
    next: Option<String>,
    link: Option<String>,
}

/// `GET /api/auth/oidc/login` — connexion par le fournisseur. Avec `link=`, une
/// invitation d'administrateur : l'identité qui s'authentifie est liée au compte
/// visé par l'invitation.
async fn oidc_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LoginQuery>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    provider_of(&auth)?;
    let next = safe_next(query.next.as_deref());
    let intent = match query.link {
        Some(token) => {
            let probe = token.clone();
            let known = db(&auth, move |accounts| accounts.link_token_user(&probe))
                .await?
                .map_err(account_error)?;
            if known.is_none() {
                return Ok(redirect("/login?error=invalid_link", Vec::new()));
            }
            Intent::LinkInvite { token }
        }
        None => Intent::Login,
    };
    Ok(begin(&auth, &headers, next, intent).await)
}

/// `GET /api/auth/oidc/link` — lie **son** compte à une identité du fournisseur.
/// Réservé aux administrateurs, sauf si `auth.oidc.self_link` est activé.
async fn oidc_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    SessionUser(user): SessionUser,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    let provider = provider_of(&auth)?;
    if user.role != Role::Admin && !provider.config().self_link {
        return Err(error(StatusCode::FORBIDDEN, "self_link_disabled"));
    }
    Ok(begin(
        &auth,
        &headers,
        "/account".into(),
        Intent::LinkSelf { user_id: user.id },
    )
    .await)
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// `GET /api/auth/oidc/callback` — retour du fournisseur.
async fn oidc_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    MaybeUser(current): MaybeUser,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    let provider = provider_of(&auth)?;
    let secure = is_https(&headers);
    let clear_flow = cookie(FLOW_COOKIE, "", 0, "/api/auth/oidc", secure);

    // Le `state` doit être connu, non échu, et revenir avec le navigateur qui l'a
    // demandé.
    let bound = cookie_value(&headers, FLOW_COOKIE);
    let pending = query
        .state
        .as_deref()
        .filter(|state| bound.as_deref() == Some(*state))
        .and_then(|state| auth.flows.take(state));
    let Some(pending) = pending else {
        return Ok(redirect("/login?error=oidc_state", vec![clear_flow]));
    };
    let fail = |code: &str| {
        redirect(
            &failure_page(&pending.intent, code),
            vec![clear_flow.clone()],
        )
    };

    if query.error.is_some() {
        return Ok(fail("oidc_denied"));
    }
    let Some(code) = query.code.as_deref() else {
        return Ok(fail("oidc_failed"));
    };
    let claims = match provider
        .exchange(
            code,
            &pending.redirect_uri,
            &pending.verifier,
            &pending.nonce,
        )
        .await
    {
        Ok(claims) => claims,
        Err(e) => {
            eprintln!("OIDC : {e}");
            return Ok(fail("oidc_failed"));
        }
    };

    let settings = provider.config().clone();
    let (intent, ttl) = (pending.intent.clone(), auth.session_ttl);
    let outcome = db(&auth, move |accounts| {
        resolve(accounts, &settings, &intent, current.as_ref(), &claims, ttl)
    })
    .await?;

    Ok(match outcome {
        Ok(Resolved { session }) => {
            let mut cookies = vec![clear_flow];
            if let Some(token) = session {
                cookies.push(session_cookies(&headers, &token, ttl));
            }
            let target = match pending.intent {
                Intent::LinkSelf { .. } => format!("{}?linked=1", pending.next),
                _ => pending.next.clone(),
            };
            redirect(&target, cookies)
        }
        Err(code) => fail(code),
    })
}

/// Issue d'une connexion OIDC réussie côté fournisseur.
struct Resolved {
    /// Jeton de la session à ouvrir ; `None` quand on reste dans la session
    /// courante (liaison de son propre compte).
    session: Option<String>,
}

/// Décide ce que devient une identité authentifiée. Rend un code d'erreur que
/// l'interface sait traduire.
fn resolve(
    accounts: &Accounts,
    settings: &config::Oidc,
    intent: &Intent,
    current: Option<&User>,
    claims: &Claims,
    ttl: i64,
) -> Result<Resolved, &'static str> {
    let internal = |e: AccountError| -> &'static str {
        eprintln!("OIDC : {e}");
        "internal"
    };
    let open = |user: &User| -> Result<Resolved, &'static str> {
        if user.disabled {
            return Err("account_disabled");
        }
        let token = accounts.create_session(&user.id, ttl).map_err(internal)?;
        accounts.touch_login(&user.id).map_err(internal)?;
        Ok(Resolved {
            session: Some(token),
        })
    };
    let link = |user_id: &str| {
        accounts
            .link_identity(
                user_id,
                &claims.issuer,
                &claims.subject,
                claims.email.as_deref(),
            )
            .map_err(|e| match e {
                AccountError::IdentityTaken => "identity_taken",
                other => internal(other),
            })
    };

    match intent {
        Intent::LinkSelf { user_id } => {
            if current.map(|user| &user.id) != Some(user_id) {
                return Err("session_mismatch");
            }
            link(user_id)?;
            Ok(Resolved { session: None })
        }
        Intent::LinkInvite { token } => {
            let user_id = accounts
                .link_token_user(token)
                .map_err(internal)?
                .ok_or("invalid_link")?;
            link(&user_id)?;
            // Consommée **après** la liaison : un échec ne brûle pas l'invitation.
            accounts.consume_link_token(token).map_err(internal)?;
            let user = accounts
                .user(&user_id)
                .map_err(internal)?
                .ok_or("invalid_link")?;
            open(&user)
        }
        Intent::Login => {
            if let Some(identity) = accounts
                .identity(&claims.issuer, &claims.subject)
                .map_err(internal)?
            {
                let user = accounts
                    .user(&identity.user_id)
                    .map_err(internal)?
                    .ok_or("no_account")?;
                return open(&user);
            }
            if settings.link_by_email
                && claims.email_verified
                && let Some(email) = &claims.email
                && let Some(user) = accounts.user_by_email(email).map_err(internal)?
            {
                link(&user.id)?;
                return open(&user);
            }
            if settings.auto_create {
                // Le compte créé porte l'adresse du fournisseur : elle doit être
                // certifiée, sinon n'importe qui pourrait s'en prévaloir.
                let email = claims
                    .email
                    .as_ref()
                    .filter(|_| claims.email_verified)
                    .ok_or("email_required")?;
                let base = claims
                    .username
                    .as_deref()
                    .or_else(|| claims.email.as_deref().and_then(|e| e.split('@').next()))
                    .unwrap_or("user");
                let username = accounts.available_username(base).map_err(internal)?;
                let user = accounts
                    .create_user(NewUser {
                        username,
                        display_name: claims.name.clone(),
                        email: Some(email.clone()),
                        password: None,
                        role: Some(Role::User),
                        must_change_password: false,
                    })
                    .map_err(|e| match e {
                        AccountError::EmailTaken => "email_taken",
                        other => internal(other),
                    })?;
                link(&user.id)?;
                return open(&user);
            }
            Err("no_account")
        }
    }
}

// ── Administration des comptes ───────────────────────────────────────────

async fn list_users(State(state): State<AppState>) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    let users = db(&auth, |accounts| -> Result<Vec<UserView>, AccountError> {
        accounts
            .list_users()?
            .into_iter()
            .map(|user| view(accounts, user))
            .collect()
    })
    .await?
    .map_err(account_error)?;
    Ok(Json(users).into_response())
}

#[derive(Deserialize)]
struct NewUserBody {
    email: String,
    /// Affichage seulement ; tiré de l'adresse s'il est vide.
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    role: Option<Role>,
    /// Le mot de passe est temporaire : l'utilisateur est invité à le changer.
    #[serde(default)]
    temporary: bool,
}

async fn create_user(
    State(state): State<AppState>,
    Json(body): Json<NewUserBody>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    let created = db(&auth, move |accounts| -> Result<UserView, AccountError> {
        let user = accounts.create_user(NewUser {
            username: body.username,
            display_name: body.display_name,
            email: Some(body.email),
            // Un compte créé ici a toujours un mot de passe : les comptes OIDC se
            // créent à la première connexion (`auto_create`), pas par ce formulaire.
            password: Some(body.password.unwrap_or_default()),
            role: body.role,
            must_change_password: body.temporary,
        })?;
        view(accounts, user)
    })
    .await?
    .map_err(account_error)?;
    Ok((StatusCode::CREATED, Json(created)).into_response())
}

async fn update_user(
    State(state): State<AppState>,
    SessionUser(actor): SessionUser,
    Path(id): Path<String>,
    Json(patch): Json<UserPatch>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    // Sinon un administrateur pourrait se retirer ses droits, ou se verrouiller dehors.
    if actor.id == id {
        if patch.role == Some(Role::User) {
            return Err(error(StatusCode::CONFLICT, "cannot_demote_self"));
        }
        if patch.disabled == Some(true) {
            return Err(error(StatusCode::CONFLICT, "cannot_disable_self"));
        }
    }
    let updated = db(&auth, move |accounts| -> Result<UserView, AccountError> {
        let user = accounts.update_user(&id, &patch)?;
        view(accounts, user)
    })
    .await?
    .map_err(account_error)?;
    Ok(Json(updated).into_response())
}

async fn delete_user(
    State(state): State<AppState>,
    SessionUser(actor): SessionUser,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    if actor.id == id {
        return Err(error(StatusCode::CONFLICT, "cannot_delete_self"));
    }
    db(&auth, move |accounts| accounts.delete_user(&id))
        .await?
        .map_err(account_error)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
struct SetPasswordBody {
    password: String,
    #[serde(default)]
    temporary: bool,
}

async fn admin_set_password(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetPasswordBody>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    db(&auth, move |accounts| {
        if body.temporary {
            accounts.set_temporary_password(&id, &body.password)
        } else {
            accounts.set_password(&id, &body.password)
        }
    })
    .await?
    .map_err(account_error)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/admin/users/{id}/link-token` — invitation de liaison : l'adresse
/// à remettre à la personne qui doit lier son compte OIDC à celui-ci.
async fn create_link_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    provider_of(&auth)?;
    let token = db(&auth, move |accounts| {
        accounts.create_link_token(&id, LINK_TOKEN_TTL)
    })
    .await?
    .map_err(account_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "token": token,
            "url": format!("/api/auth/oidc/login?link={token}"),
            "expires_in": LINK_TOKEN_TTL,
        })),
    )
        .into_response())
}

#[derive(Deserialize)]
struct LinkBody {
    /// Défaut : l'émetteur configuré.
    #[serde(default)]
    issuer: Option<String>,
    subject: String,
    #[serde(default)]
    email: Option<String>,
}

/// `POST /api/admin/users/{id}/identities` — lie un compte du fournisseur dont
/// l'administrateur connaît l'identifiant (`sub`).
async fn admin_link_identity(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<LinkBody>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    let provider = provider_of(&auth)?;
    let issuer = body
        .issuer
        .filter(|issuer| !issuer.trim().is_empty())
        .unwrap_or_else(|| provider.issuer().to_string());
    let subject = body.subject.trim().to_string();
    if subject.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_subject"));
    }
    let identity = db(&auth, move |accounts| {
        accounts.link_identity(&id, &issuer, &subject, body.email.as_deref())
    })
    .await?
    .map_err(account_error)?;
    Ok((StatusCode::CREATED, Json(identity)).into_response())
}

async fn admin_unlink_identity(
    State(state): State<AppState>,
    Path((id, identity)): Path<(String, String)>,
) -> Result<Response, Response> {
    let auth = auth_of(&state)?;
    db(&auth, move |accounts| {
        accounts.unlink_identity(&id, &identity)
    })
    .await?
    .map_err(account_error)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn cookies_are_read_by_name() {
        let map = headers(&[("cookie", "a=1; easybook_session=tok; b=2")]);
        assert_eq!(cookie_value(&map, SESSION_COOKIE).as_deref(), Some("tok"));
        assert_eq!(cookie_value(&map, "a").as_deref(), Some("1"));
        assert_eq!(cookie_value(&map, "absent"), None);
        // Un nom qui n'est que le suffixe d'un autre ne correspond pas.
        let map = headers(&[("cookie", "x_easybook_session=pirate")]);
        assert_eq!(cookie_value(&map, SESSION_COOKIE), None);
    }

    #[test]
    fn the_session_cookie_is_httponly_lax_and_secure_behind_https() {
        let plain = cookie(SESSION_COOKIE, "t", 60, "/", false);
        assert!(plain.contains("HttpOnly") && plain.contains("SameSite=Lax"));
        assert!(!plain.contains("Secure"));
        assert!(cookie(SESSION_COOKIE, "t", 60, "/", true).ends_with("; Secure"));
    }

    #[test]
    fn writes_must_come_from_this_site() {
        let post = Method::POST;
        let own = ("host", "livres.example");
        // Même site, avec ou sans port.
        assert!(same_origin(
            &post,
            &headers(&[own, ("origin", "https://livres.example")])
        ));
        assert!(same_origin(
            &post,
            &headers(&[("host", "localhost"), ("origin", "http://localhost:5200")])
        ));
        // nginx transmet l'hôte public dans `X-Forwarded-Host`.
        assert!(same_origin(
            &post,
            &headers(&[
                ("host", "127.0.0.1:8091"),
                ("x-forwarded-host", "localhost:5200"),
                ("origin", "http://localhost:5200")
            ])
        ));
        // Autre site, `Origin: null`, origine mal formée.
        assert!(!same_origin(
            &post,
            &headers(&[own, ("origin", "https://pirate.example")])
        ));
        assert!(!same_origin(&post, &headers(&[own, ("origin", "null")])));
        assert!(!same_origin(
            &post,
            &headers(&[own, ("origin", "https://livres.example.pirate.example")])
        ));
        // Sans `Origin` (curl, autre serveur) et lectures : acceptées.
        assert!(same_origin(&post, &headers(&[own])));
        assert!(same_origin(
            &Method::GET,
            &headers(&[own, ("origin", "https://pirate.example")])
        ));
    }

    #[test]
    fn dev_logins_are_parsed_leniently() {
        assert_eq!(
            parse_dev_logins(" a@x.org:mdp1 , b@x.org:a:b,sans-deux-points,:vide,c@x.org: "),
            [
                ("a@x.org".to_string(), "mdp1".to_string()),
                ("b@x.org".to_string(), "a:b".to_string()),
            ]
        );
        assert!(parse_dev_logins("").is_empty());
    }

    #[test]
    fn hosts_lose_their_port() {
        assert_eq!(host_only("localhost:5200"), "localhost");
        assert_eq!(host_only("livres.example"), "livres.example");
        assert_eq!(host_only("[::1]:8080"), "[::1]");
    }

    #[test]
    fn the_throttle_blocks_after_repeated_failures_and_resets() {
        let throttle = Throttle::default();
        for _ in 0..MAX_FAILURES - 1 {
            throttle.failure("alice|1.2.3.4");
        }
        assert!(throttle.blocked("alice|1.2.3.4").is_none());
        throttle.failure("alice|1.2.3.4");
        assert!(throttle.blocked("alice|1.2.3.4").is_some());
        // Un autre client, un autre compte : indemnes.
        assert!(throttle.blocked("alice|5.6.7.8").is_none());
        assert!(throttle.blocked("bob|1.2.3.4").is_none());
        throttle.success("alice|1.2.3.4");
        assert!(throttle.blocked("alice|1.2.3.4").is_none());
    }

    #[test]
    fn the_client_is_the_last_forwarded_address() {
        // L'adresse écrite par le client vient en premier, celle de nginx en dernier.
        let map = headers(&[("x-forwarded-for", "9.9.9.9, 10.0.0.7")]);
        assert_eq!(client_key(&map), "10.0.0.7");
        assert_eq!(client_key(&HeaderMap::new()), "-");
    }

    fn claims(subject: &str) -> Claims {
        Claims {
            issuer: "https://idp".into(),
            subject: subject.into(),
            email: Some("alice@example.org".into()),
            email_verified: true,
            username: Some("alice".into()),
            name: Some("Alice".into()),
        }
    }

    fn setup() -> (Accounts, config::Oidc) {
        (
            Accounts::open_in_memory().unwrap(),
            config::Oidc {
                issuer: "https://idp".into(),
                client_id: "easybook".into(),
                ..config::Oidc::default()
            },
        )
    }

    fn local(accounts: &Accounts, name: &str, email: Option<&str>) -> User {
        accounts
            .create_user(NewUser {
                username: name.into(),
                email: Some(email.map_or_else(|| format!("{name}@local.test"), str::to_string)),
                password: Some("motdepasse1".into()),
                ..NewUser::default()
            })
            .unwrap()
    }

    #[test]
    fn an_unknown_identity_is_refused_by_default() {
        let (accounts, settings) = setup();
        local(&accounts, "alice", Some("alice@example.org"));
        let outcome = resolve(
            &accounts,
            &settings,
            &Intent::Login,
            None,
            &claims("s1"),
            60,
        );
        assert_eq!(outcome.err(), Some("no_account"));
        assert_eq!(accounts.count_users().unwrap(), 1);
    }

    #[test]
    fn a_linked_identity_logs_in() {
        let (accounts, settings) = setup();
        let alice = local(&accounts, "alice", None);
        accounts
            .link_identity(&alice.id, "https://idp", "s1", None)
            .unwrap();

        let resolved = resolve(
            &accounts,
            &settings,
            &Intent::Login,
            None,
            &claims("s1"),
            60,
        )
        .ok()
        .unwrap();
        let user = accounts
            .session_user(&resolved.session.unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(user.id, alice.id);
        assert!(user.last_login_at.is_some());
    }

    #[test]
    fn a_disabled_account_cannot_log_in_through_oidc() {
        let (accounts, settings) = setup();
        let alice = local(&accounts, "alice", None);
        accounts
            .link_identity(&alice.id, "https://idp", "s1", None)
            .unwrap();
        accounts
            .update_user(
                &alice.id,
                &UserPatch {
                    disabled: Some(true),
                    ..UserPatch::default()
                },
            )
            .unwrap();
        let outcome = resolve(
            &accounts,
            &settings,
            &Intent::Login,
            None,
            &claims("s1"),
            60,
        );
        assert_eq!(outcome.err(), Some("account_disabled"));
    }

    #[test]
    fn auto_create_makes_a_passwordless_user_with_a_free_username() {
        let (accounts, mut settings) = setup();
        settings.auto_create = true;
        local(&accounts, "alice", None);

        let resolved = resolve(
            &accounts,
            &settings,
            &Intent::Login,
            None,
            &claims("s9"),
            60,
        )
        .ok()
        .unwrap();
        let user = accounts
            .session_user(&resolved.session.unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(user.username, "alice-2");
        assert_eq!(user.role, Role::User);
        assert!(!user.has_password);
        assert_eq!(user.display_name.as_deref(), Some("Alice"));
        assert!(accounts.identity("https://idp", "s9").unwrap().is_some());

        // Le même compte OIDC retrouve le même compte local.
        let again = resolve(
            &accounts,
            &settings,
            &Intent::Login,
            None,
            &claims("s9"),
            60,
        )
        .ok()
        .unwrap();
        assert_eq!(
            accounts
                .session_user(&again.session.unwrap())
                .unwrap()
                .unwrap()
                .id,
            user.id
        );
        assert_eq!(accounts.count_users().unwrap(), 2);
    }

    #[test]
    fn link_by_email_needs_a_verified_and_unambiguous_address() {
        let (accounts, mut settings) = setup();
        settings.link_by_email = true;
        let alice = local(&accounts, "alice", Some("alice@example.org"));

        let mut unverified = claims("s1");
        unverified.email_verified = false;
        assert_eq!(
            resolve(&accounts, &settings, &Intent::Login, None, &unverified, 60).err(),
            Some("no_account")
        );

        let resolved = resolve(
            &accounts,
            &settings,
            &Intent::Login,
            None,
            &claims("s1"),
            60,
        )
        .ok()
        .unwrap();
        assert_eq!(
            accounts
                .session_user(&resolved.session.unwrap())
                .unwrap()
                .unwrap()
                .id,
            alice.id
        );
        // L'identité est désormais liée : plus besoin de l'adresse.
        assert_eq!(
            accounts
                .identity("https://idp", "s1")
                .unwrap()
                .unwrap()
                .user_id,
            alice.id
        );
    }

    #[test]
    fn auto_create_needs_a_certified_email() {
        let (accounts, mut settings) = setup();
        settings.auto_create = true;

        let mut unverified = claims("s1");
        unverified.email_verified = false;
        let mut missing = claims("s2");
        missing.email = None;
        for refused in [unverified, missing] {
            assert_eq!(
                resolve(&accounts, &settings, &Intent::Login, None, &refused, 60).err(),
                Some("email_required")
            );
        }
        assert_eq!(accounts.count_users().unwrap(), 0);
    }

    #[test]
    fn auto_create_never_takes_over_an_existing_address() {
        let (accounts, mut settings) = setup();
        settings.auto_create = true;
        let local_alice = local(&accounts, "alice-locale", Some("alice@example.org"));

        let outcome = resolve(
            &accounts,
            &settings,
            &Intent::Login,
            None,
            &claims("s1"),
            60,
        );
        assert_eq!(outcome.err(), Some("email_taken"));
        // Sans `link_by_email`, le compte local n'est pas lié pour autant.
        assert!(accounts.identities_of(&local_alice.id).unwrap().is_empty());
        assert_eq!(accounts.count_users().unwrap(), 1);
    }

    #[test]
    fn an_invitation_links_then_logs_in_once() {
        let (accounts, settings) = setup();
        let alice = local(&accounts, "alice", None);
        let token = accounts.create_link_token(&alice.id, 600).unwrap();
        let intent = Intent::LinkInvite {
            token: token.clone(),
        };

        let resolved = resolve(&accounts, &settings, &intent, None, &claims("s1"), 60)
            .ok()
            .unwrap();
        assert_eq!(
            accounts
                .session_user(&resolved.session.unwrap())
                .unwrap()
                .unwrap()
                .id,
            alice.id
        );
        assert!(accounts.identity("https://idp", "s1").unwrap().is_some());

        // L'invitation est consommée : la rejouer échoue.
        assert_eq!(
            resolve(&accounts, &settings, &intent, None, &claims("s2"), 60).err(),
            Some("invalid_link")
        );
        assert!(accounts.identity("https://idp", "s2").unwrap().is_none());
    }

    #[test]
    fn an_invitation_cannot_steal_an_identity_of_another_account() {
        let (accounts, settings) = setup();
        let alice = local(&accounts, "alice", None);
        let bob = local(&accounts, "bob", None);
        accounts
            .link_identity(&bob.id, "https://idp", "s1", None)
            .unwrap();
        let token = accounts.create_link_token(&alice.id, 600).unwrap();

        let outcome = resolve(
            &accounts,
            &settings,
            &Intent::LinkInvite {
                token: token.clone(),
            },
            None,
            &claims("s1"),
            60,
        );
        assert_eq!(outcome.err(), Some("identity_taken"));
        // Échec : l'invitation reste utilisable.
        assert_eq!(
            accounts.link_token_user(&token).unwrap().as_deref(),
            Some(alice.id.as_str())
        );
    }

    #[test]
    fn linking_oneself_requires_the_same_session() {
        let (accounts, settings) = setup();
        let alice = local(&accounts, "alice", None);
        let bob = local(&accounts, "bob", None);
        let intent = Intent::LinkSelf {
            user_id: alice.id.clone(),
        };

        assert_eq!(
            resolve(&accounts, &settings, &intent, None, &claims("s1"), 60).err(),
            Some("session_mismatch")
        );
        assert_eq!(
            resolve(&accounts, &settings, &intent, Some(&bob), &claims("s1"), 60).err(),
            Some("session_mismatch")
        );

        let resolved = resolve(
            &accounts,
            &settings,
            &intent,
            Some(&alice),
            &claims("s1"),
            60,
        )
        .ok()
        .unwrap();
        // On reste dans la session courante.
        assert!(resolved.session.is_none());
        assert_eq!(
            accounts
                .identity("https://idp", "s1")
                .unwrap()
                .unwrap()
                .user_id,
            alice.id
        );
    }
}
