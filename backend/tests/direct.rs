//! Connexion directe au fournisseur depuis le formulaire (mode développement).
//!
//! Même faux fournisseur que `oidc.rs`, mais ici le navigateur ne le voit jamais :
//! easybook valide l'e-mail et le mot de passe auprès de lui (`grant_type=password`).

mod common;

use axum::http::StatusCode;
use common::http::{PASSWORD, Reply, Site, site_direct, site_with};
use common::idp::{IDP_PASSWORD, Idp, Person, Quirks};
use easybook::config;
use serde_json::json;

fn site(idp: &Idp, direct: bool, tweak: impl FnOnce(&mut config::Oidc)) -> Site {
    let mut oidc = idp.settings();
    tweak(&mut oidc);
    let settings = config::Auth {
        oidc,
        ..config::Auth::default()
    };
    if direct {
        site_direct(settings)
    } else {
        site_with(settings)
    }
}

fn alice() -> Person {
    Person::new("alice", "alice@example.org")
}

async fn login(site: &Site, email: &str, password: &str) -> Reply {
    site.post(
        "/api/auth/login",
        None,
        json!({ "email": email, "password": password }),
    )
    .await
}

#[tokio::test]
async fn it_is_off_by_default() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = site(&idp, false, |oidc| oidc.auto_create = true);

    let reply = login(&site, "alice@example.org", IDP_PASSWORD).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert!(reply.cookie("easybook_session").is_none());
    assert_eq!(site.accounts.count_users().unwrap(), 1);
    // Le formulaire ne l'annonce pas non plus.
    let session = site.get("/api/auth/session", None).await.json();
    assert_eq!(session["oidc"]["direct"], false);
}

#[tokio::test]
async fn it_does_nothing_without_a_provider() {
    let site = site_direct(config::Auth::default());
    let reply = login(&site, "alice@example.org", IDP_PASSWORD).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert!(site.get("/api/auth/session", None).await.json()["oidc"].is_null());
}

#[tokio::test]
async fn a_provider_account_logs_in_with_its_email_and_password() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = site(&idp, true, |oidc| oidc.auto_create = true);
    assert_eq!(
        site.get("/api/auth/session", None).await.json()["oidc"]["direct"],
        true
    );

    let reply = login(&site, "alice@example.org", IDP_PASSWORD).await;
    assert_eq!(reply.status, StatusCode::OK);
    let user = reply.json();
    assert_eq!(user["email"], "alice@example.org");
    assert_eq!(user["has_password"], false);
    assert_eq!(user["identities"][0]["subject"], "sub-alice");

    let cookie = reply.cookie("easybook_session").unwrap();
    assert_eq!(
        site.get("/api/books", Some(&cookie)).await.status,
        StatusCode::OK
    );
    // La casse de l'adresse ne compte pas, et le même compte est retrouvé.
    let again = login(&site, "ALICE@example.org", IDP_PASSWORD).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.json()["id"], user["id"]);
    assert_eq!(site.accounts.count_users().unwrap(), 2);
}

#[tokio::test]
async fn a_wrong_password_is_refused_and_counted() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = site(&idp, true, |oidc| oidc.auto_create = true);

    for _ in 0..5 {
        let reply = login(&site, "alice@example.org", "faux").await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.error().as_deref(), Some("invalid_credentials"));
    }
    // Le blocage s'applique aussi à cette voie.
    let locked = login(&site, "alice@example.org", IDP_PASSWORD).await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(site.accounts.count_users().unwrap(), 1);
}

#[tokio::test]
async fn the_account_policy_still_applies() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    // Ni création automatique, ni liaison : un compte du fournisseur inconnu est refusé.
    let site = site(&idp, true, |_| {});

    let reply = login(&site, "alice@example.org", IDP_PASSWORD).await;
    assert_eq!(
        (reply.status, reply.error().as_deref()),
        (StatusCode::FORBIDDEN, Some("no_account"))
    );
    assert!(reply.cookie("easybook_session").is_none());
    assert_eq!(site.accounts.count_users().unwrap(), 1);
}

#[tokio::test]
async fn it_reaches_the_local_account_linked_to_the_identity() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = site(&idp, true, |_| {});
    let local = site.add_user(
        "alice-locale",
        easybook::accounts::Role::User,
        Some(PASSWORD),
    );
    site.accounts
        .link_identity(&local.id, &idp.issuer, "sub-alice", None)
        .unwrap();

    // Avec l'adresse et le mot de passe du **fournisseur**, on arrive sur le compte local.
    let reply = login(&site, "alice@example.org", IDP_PASSWORD).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["id"], local.id);
    // Son propre mot de passe local fonctionne toujours, sans interroger le fournisseur.
    drop(idp);
    assert_eq!(
        site.login_reply("alice-locale", PASSWORD).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn link_by_email_applies_here_too() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = site(&idp, true, |oidc| oidc.link_by_email = true);
    let local = site
        .accounts
        .create_user(easybook::accounts::NewUser {
            email: Some("alice@example.org".into()),
            password: Some(PASSWORD.into()),
            ..Default::default()
        })
        .unwrap();

    let reply = login(&site, "alice@example.org", IDP_PASSWORD).await;
    assert_eq!(reply.json()["id"], local.id);
    assert!(
        site.accounts
            .identity(&idp.issuer, "sub-alice")
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn a_disabled_account_is_refused() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = site(&idp, true, |oidc| oidc.auto_create = true);
    let first = login(&site, "alice@example.org", IDP_PASSWORD).await.json();
    let admin = site.admin().await;
    site.request(
        "PATCH",
        &format!("/api/admin/users/{}", first["id"].as_str().unwrap()),
        Some(&admin),
        Some(json!({ "disabled": true })),
        &[],
    )
    .await;

    let reply = login(&site, "alice@example.org", IDP_PASSWORD).await;
    assert_eq!(
        (reply.status, reply.error().as_deref()),
        (StatusCode::FORBIDDEN, Some("account_disabled"))
    );
}

#[tokio::test]
async fn a_bad_token_from_the_provider_opens_nothing() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = site(&idp, true, |oidc| oidc.auto_create = true);

    for (n, quirks) in [
        Quirks {
            audience: Some("autre-client".into()),
            ..Quirks::default()
        },
        Quirks {
            issuer: Some("https://pirate.example".into()),
            ..Quirks::default()
        },
        Quirks {
            expired: true,
            ..Quirks::default()
        },
        Quirks {
            foreign_key: true,
            ..Quirks::default()
        },
        Quirks {
            hs256: true,
            ..Quirks::default()
        },
        Quirks {
            token_error: true,
            ..Quirks::default()
        },
    ]
    .into_iter()
    .enumerate()
    {
        idp.quirks(quirks);
        // Un client différent à chaque essai : le blocage n'est pas l'objet ici.
        let client = format!("10.0.0.{n}");
        let reply = site
            .request(
                "POST",
                "/api/auth/login",
                None,
                Some(json!({ "email": "alice@example.org", "password": IDP_PASSWORD })),
                &[("x-forwarded-for", &client)],
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert!(reply.cookie("easybook_session").is_none());
    }
    assert_eq!(site.accounts.count_users().unwrap(), 1);
}

#[tokio::test]
async fn a_local_password_is_checked_before_the_provider() {
    let idp = Idp::start().await;
    let site = site(&idp, true, |oidc| oidc.auto_create = true);
    // Le fournisseur ne connaît personne : seul le compte local peut répondre.
    assert_eq!(
        site.login_reply("root", PASSWORD).await.status,
        StatusCode::OK
    );
    assert_eq!(
        site.login_reply("root", "faux").await.status,
        StatusCode::UNAUTHORIZED
    );
}
