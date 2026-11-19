//! Connexion contre un **vrai** Keycloak : celui du `docker-compose.yml` de test.
//!
//! ```text
//! docker compose up -d keycloak          # realm `easybook` importé au démarrage
//! cargo test --test keycloak -- --ignored
//! ```
//!
//! Ignorés par défaut : ils demandent le conteneur. `EASYBOOK_TEST_KEYCLOAK`
//! change son adresse (défaut `http://localhost:8180`). Les utilisateurs du
//! realm sont dans `docker/keycloak/easybook-realm.json` (mot de passe = nom).
//!
//! Le faux fournisseur de `oidc.rs` couvre les cas limites ; ceux-ci prouvent
//! que le client fonctionne avec une implémentation réelle (jetons signés par
//! Keycloak, clés de chiffrement dans le JWKS, UUID comme `sub`…).

mod common;

use axum::http::StatusCode;
use common::http::{Reply, Site, site_with};
use common::idp::path_and_query;
use easybook::accounts::{NewUser, Role};
use easybook::config;
use reqwest::redirect::Policy;
use serde_json::json;

fn realm() -> String {
    let base = std::env::var("EASYBOOK_TEST_KEYCLOAK")
        .unwrap_or_else(|_| "http://localhost:8180".to_string());
    format!("{}/realms/easybook", base.trim_end_matches('/'))
}

fn keycloak_site(tweak: impl FnOnce(&mut config::Oidc)) -> Site {
    let mut oidc = config::Oidc {
        issuer: realm(),
        client_id: "easybook".into(),
        client_secret: "easybook-test-secret".into(),
        ..config::Oidc::default()
    };
    tweak(&mut oidc);
    site_with(config::Auth {
        oidc,
        ..config::Auth::default()
    })
}

/// Le navigateur chez Keycloak : charge la page de connexion, remplit le
/// formulaire, et rend l'adresse de retour — `None` si Keycloak refuse.
async fn keycloak_login(authorize_url: &str, username: &str, password: &str) -> Option<String> {
    let browser = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(Policy::none())
        .build()
        .unwrap();

    let page = browser
        .get(authorize_url)
        .send()
        .await
        .expect("Keycloak joignable");
    assert_eq!(
        page.status(),
        StatusCode::OK,
        "page de connexion : {authorize_url}"
    );
    let html = page.text().await.unwrap();
    let form = regex::Regex::new(r#"id="kc-form-login"[^>]*action="([^"]+)""#)
        .unwrap()
        .captures(&html)
        .unwrap_or_else(|| panic!("formulaire de connexion introuvable :\n{html}"))[1]
        .replace("&amp;", "&");

    let answer = browser
        .post(&form)
        .form(&[
            ("username", username),
            ("password", password),
            ("credentialId", ""),
        ])
        .send()
        .await
        .unwrap();
    if answer.status() != StatusCode::FOUND {
        return None;
    }
    Some(answer.headers()["location"].to_str().unwrap().to_string())
}

async fn start(site: &Site, uri: &str) -> (String, String) {
    let reply = site.get(uri, None).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "{:?}",
        reply.location()
    );
    (
        reply.location().unwrap(),
        reply.cookie("easybook_oidc").unwrap(),
    )
}

async fn sign_in(site: &Site, uri: &str, username: &str, password: &str) -> Reply {
    let (authorize, flow) = start(site, uri).await;
    let back = keycloak_login(&authorize, username, password)
        .await
        .expect("Keycloak a accepté la connexion");
    site.get(&path_and_query(&back), Some(&flow)).await
}

#[tokio::test]
#[ignore = "demande `docker compose up -d keycloak`"]
async fn keycloak_creates_and_recognizes_an_account() {
    let site = keycloak_site(|oidc| oidc.auto_create = true);

    let reply = sign_in(&site, "/api/auth/oidc/login", "alice", "alice").await;
    assert_eq!(reply.location().as_deref(), Some("/"));
    let cookie = reply.cookie("easybook_session").unwrap();
    let session = site.get("/api/auth/session", Some(&cookie)).await.json();
    let user = &session["user"];
    assert_eq!(user["username"], "alice");
    assert_eq!(user["email"], "alice@example.org");
    assert_eq!(user["has_password"], false);
    // Pour Keycloak, `sub` est un UUID : jamais un identifiant séquentiel.
    let subject = user["identities"][0]["subject"].as_str().unwrap();
    assert!(uuid::Uuid::parse_str(subject).is_ok(), "{subject}");
    assert_eq!(user["identities"][0]["issuer"], realm());

    // Une seconde connexion retrouve le même compte.
    let again = sign_in(&site, "/api/auth/oidc/login", "alice", "alice").await;
    let cookie2 = again.cookie("easybook_session").unwrap();
    let session2 = site.get("/api/auth/session", Some(&cookie2)).await.json();
    assert_eq!(session2["user"]["id"], user["id"]);
    assert_eq!(site.accounts.count_users().unwrap(), 2);
}

#[tokio::test]
#[ignore = "demande `docker compose up -d keycloak`"]
async fn keycloak_refuses_a_wrong_password_and_easybook_a_stranger() {
    let site = keycloak_site(|_| {});
    let (authorize, _) = start(&site, "/api/auth/oidc/login").await;
    assert!(
        keycloak_login(&authorize, "alice", "pas-le-bon")
            .await
            .is_none()
    );

    // Bien authentifié chez Keycloak, mais inconnu d'easybook : refusé.
    let reply = sign_in(&site, "/api/auth/oidc/login", "bob", "bob").await;
    assert_eq!(reply.location().as_deref(), Some("/login?error=no_account"));
    assert!(reply.cookie("easybook_session").is_none());
    assert_eq!(site.accounts.count_users().unwrap(), 1);
}

#[tokio::test]
#[ignore = "demande `docker compose up -d keycloak`"]
async fn keycloak_account_is_linked_by_an_admin_invitation() {
    let site = keycloak_site(|_| {});
    let admin = site.admin().await;
    let local = site.add_user("robert", Role::User, Some("motdepasse1"));

    let invite = site
        .post(
            &format!("/api/admin/users/{}/link-token", local.id),
            Some(&admin),
            json!({}),
        )
        .await
        .json();
    let reply = sign_in(&site, invite["url"].as_str().unwrap(), "bob", "bob").await;
    let cookie = reply.cookie("easybook_session").unwrap();
    let session = site.get("/api/auth/session", Some(&cookie)).await.json();
    // Connecté au compte **local** de l'administrateur, pas à un nouveau.
    assert_eq!(session["user"]["id"], local.id);
    assert_eq!(session["user"]["username"], "robert");
    assert_eq!(session["user"]["identities"].as_array().unwrap().len(), 1);
    assert_eq!(
        site.login_reply("robert", "motdepasse1").await.status,
        StatusCode::OK
    );
    assert_eq!(site.accounts.count_users().unwrap(), 2);
}

#[tokio::test]
#[ignore = "demande `docker compose up -d keycloak`"]
async fn keycloak_links_by_certified_email_only() {
    let site = keycloak_site(|oidc| oidc.link_by_email = true);
    let by_email = |name: &str, email: &str| {
        site.accounts
            .create_user(NewUser {
                username: name.into(),
                email: Some(email.into()),
                ..NewUser::default()
            })
            .unwrap()
    };
    let bob = by_email("robert", "bob@example.org");
    by_email("caroline", "carol@example.org");

    // bob : adresse vérifiée chez Keycloak → lié au compte de même adresse.
    let reply = sign_in(&site, "/api/auth/oidc/login", "bob", "bob").await;
    let cookie = reply.cookie("easybook_session").unwrap();
    let session = site.get("/api/auth/session", Some(&cookie)).await.json();
    assert_eq!(session["user"]["id"], bob.id);

    // carol : adresse non vérifiée → refusée, rien de lié.
    let reply = sign_in(&site, "/api/auth/oidc/login", "carol", "carol").await;
    assert_eq!(reply.location().as_deref(), Some("/login?error=no_account"));
    assert_eq!(site.accounts.count_users().unwrap(), 3);
}
