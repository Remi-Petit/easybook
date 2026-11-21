//! Site de test avec comptes : le routeur complet, une base SQLite en mémoire et
//! un administrateur prêt à servir.

#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use easybook::accounts::{Accounts, NewUser, Role, User};
use easybook::api::{self, AppState};
use easybook::auth::AuthState;
use easybook::config;
use easybook::progress;
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

pub const PASSWORD: &str = "motdepasse1";

pub struct Site {
    pub app: Router,
    pub accounts: Arc<Accounts>,
    pub dir: tempfile::TempDir,
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    /// La ligne `Set-Cookie` complète d'un cookie.
    pub fn set_cookie_line(&self, name: &str) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|line| line.starts_with(&format!("{name}=")))
            .map(str::to_string)
    }

    /// Le cookie posé, prêt à être renvoyé (`nom=valeur`) ; `None` s'il est effacé.
    pub fn cookie(&self, name: &str) -> Option<String> {
        let line = self.set_cookie_line(name)?;
        let pair = line.split(';').next()?.to_string();
        (!pair.ends_with('=')).then_some(pair)
    }

    pub fn location(&self) -> Option<String> {
        self.headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    }

    pub fn error(&self) -> Option<String> {
        self.json()["error"].as_str().map(str::to_string)
    }
}

/// Site avec les réglages par défaut (comptes locaux seulement).
pub fn site() -> Site {
    site_with(config::Auth::default())
}

pub fn site_with(settings: config::Auth) -> Site {
    build(settings, false, None)
}

/// Comme [`site_with`], avec la connexion directe au fournisseur (mode dev).
pub fn site_direct(settings: config::Auth) -> Site {
    build(settings, true, None)
}

/// Mode dev complet : connexion directe et boutons de connexion rapide.
pub fn site_dev(settings: config::Auth, logins: &str) -> Site {
    build(settings, true, Some(logins))
}

fn build(settings: config::Auth, direct: bool, logins: Option<&str>) -> Site {
    let dir = tempfile::tempdir().unwrap();
    let accounts = Arc::new(Accounts::open_in_memory().unwrap());
    accounts
        .create_user(NewUser {
            username: "root".into(),
            email: Some("root@local.test".into()),
            password: Some(PASSWORD.into()),
            role: Some(Role::Admin),
            ..NewUser::default()
        })
        .unwrap();

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut auth = AuthState::new(Arc::clone(&accounts), settings);
    if direct {
        auth = auth.with_direct_login();
    }
    if let Some(logins) = logins {
        auth = auth.with_dev_logins(logins);
    }
    let state = AppState::new(dir.path(), tx)
        .with_auth(auth)
        // Comme en production : des comptes, donc un historique de lecture.
        .with_progress(Arc::new(progress::Progress::open_in_memory().unwrap()));
    Site {
        app: api::routes(state),
        accounts,
        dir,
    }
}

impl Site {
    pub async fn request(
        &self,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
        extra: &[(&str, &str)],
    ) -> Reply {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        let body = match body {
            Some(json) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let (parts, body) = response.into_parts();
        Reply {
            status: parts.status,
            headers: parts.headers,
            body: body.collect().await.unwrap().to_bytes().to_vec(),
        }
    }

    pub async fn get(&self, uri: &str, cookie: Option<&str>) -> Reply {
        self.request("GET", uri, cookie, None, &[]).await
    }

    pub async fn post(&self, uri: &str, cookie: Option<&str>, body: Value) -> Reply {
        self.request("POST", uri, cookie, Some(body), &[]).await
    }

    /// Connexion. `who` est une adresse, ou un nom de compte de test (`alice` →
    /// `alice@local.test`).
    pub async fn login_reply(&self, who: &str, password: &str) -> Reply {
        self.post(
            "/api/auth/login",
            None,
            serde_json::json!({ "email": email_of(who), "password": password }),
        )
        .await
    }

    /// Ouvre une session et rend son cookie (`nom=valeur`).
    pub async fn login(&self, username: &str, password: &str) -> String {
        let reply = self.login_reply(username, password).await;
        assert_eq!(reply.status, StatusCode::OK, "connexion de {username}");
        reply.cookie("easybook_session").expect("cookie de session")
    }

    pub async fn admin(&self) -> String {
        self.login("root", PASSWORD).await
    }

    /// Un compte local, créé directement dans la base.
    pub fn add_user(&self, name: &str, role: Role, password: Option<&str>) -> User {
        self.accounts
            .create_user(NewUser {
                username: name.into(),
                email: Some(email_of(name)),
                password: password.map(str::to_string),
                role: Some(role),
                ..NewUser::default()
            })
            .unwrap()
    }

    /// Un compte avec session, sans passer par la connexion.
    pub fn session_for(&self, user: &User) -> String {
        let token = self.accounts.create_session(&user.id, 3600).unwrap();
        format!("easybook_session={token}")
    }
}

/// Adresse d'un compte de test : `alice` → `alice@local.test`.
pub fn email_of(who: &str) -> String {
    if who.contains('@') {
        who.to_string()
    } else {
        format!("{who}@local.test")
    }
}
