//! Connexion et liaison OIDC, contre un faux fournisseur sur un vrai port local.
//!
//! Le « navigateur » est joué à la main : on suit les redirections une à une,
//! en renvoyant les cookies, comme le ferait un vrai navigateur.

mod common;

use axum::http::StatusCode;
use common::http::{PASSWORD, Reply, Site, site_with};
use common::idp::{Idp, Person, Quirks, path_and_query};
use easybook::accounts::Role;
use easybook::config;
use serde_json::json;

fn oidc_site(idp: &Idp, tweak: impl FnOnce(&mut config::Oidc)) -> Site {
    let mut oidc = idp.settings();
    tweak(&mut oidc);
    site_with(config::Auth {
        oidc,
        ..config::Auth::default()
    })
}

fn alice() -> Person {
    Person::new("alice", "alice@example.org")
}

fn jar(cookies: &[Option<&str>]) -> Option<String> {
    let joined: Vec<&str> = cookies.iter().flatten().copied().collect();
    (!joined.is_empty()).then(|| joined.join("; "))
}

/// Premier temps : le navigateur demande la connexion. Rend l'adresse du
/// fournisseur où il est envoyé, et le cookie qui lie le retour à ce navigateur.
async fn begin(site: &Site, uri: &str, session: Option<&str>) -> (String, String) {
    let reply = site.get(uri, session).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "{uri} : {:?}",
        reply.location()
    );
    let location = reply.location().unwrap();
    let flow = reply
        .cookie("easybook_oidc")
        .expect("cookie de la connexion en cours");
    (location, flow)
}

/// Second temps : le navigateur revient du fournisseur après s'y être connecté.
async fn finish(
    site: &Site,
    idp: &Idp,
    authorize_url: &str,
    sub: &str,
    flow: &str,
    session: Option<&str>,
) -> Reply {
    let back = idp.authenticate(authorize_url, sub).await;
    site.get(
        &path_and_query(&back),
        jar(&[Some(flow), session]).as_deref(),
    )
    .await
}

/// Une connexion complète, du clic au retour.
async fn sign_in(site: &Site, idp: &Idp, sub: &str) -> Reply {
    let (url, flow) = begin(site, "/api/auth/oidc/login", None).await;
    finish(site, idp, &url, sub, &flow, None).await
}

fn param(url: &str, name: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

#[tokio::test]
async fn the_login_sends_the_browser_to_the_provider_with_pkce() {
    let idp = Idp::start().await;
    let site = oidc_site(&idp, |_| {});
    let (url, flow) = begin(&site, "/api/auth/oidc/login", None).await;

    assert!(
        url.starts_with(&format!("{}/authorize?", idp.issuer)),
        "{url}"
    );
    assert_eq!(param(&url, "response_type").as_deref(), Some("code"));
    assert_eq!(param(&url, "client_id").as_deref(), Some("easybook"));
    assert_eq!(
        param(&url, "code_challenge_method").as_deref(),
        Some("S256")
    );
    assert_eq!(
        param(&url, "scope").as_deref(),
        Some("openid profile email")
    );
    assert_eq!(
        param(&url, "redirect_uri").as_deref(),
        Some("http://localhost/api/auth/oidc/callback")
    );
    let state = param(&url, "state").unwrap();
    assert!(state.len() >= 24);
    assert!(param(&url, "nonce").unwrap().len() >= 24);
    // Le secret du client et le vérificateur PKCE ne passent jamais par le navigateur.
    assert!(param(&url, "code_verifier").is_none());
    assert!(!url.contains("secret-de-test"));

    // Le cookie de la connexion en cours porte le `state`, et lui seul.
    assert_eq!(flow, format!("easybook_oidc={state}"));
    let reply = site.get("/api/auth/oidc/login", None).await;
    let line = reply.set_cookie_line("easybook_oidc").unwrap();
    assert!(
        line.contains("HttpOnly") && line.contains("SameSite=Lax"),
        "{line}"
    );
    assert!(line.contains("Path=/api/auth/oidc"), "{line}");
}

#[tokio::test]
async fn the_session_announces_the_provider() {
    let idp = Idp::start().await;
    let site = oidc_site(&idp, |oidc| oidc.self_link = true);
    let body = site.get("/api/auth/session", None).await.json();
    assert_eq!(body["oidc"]["label"], "Faux IdP");
    assert_eq!(body["oidc"]["self_link"], true);
    // Rien de secret dans ce que voit un visiteur anonyme.
    assert!(!body.to_string().contains("secret-de-test"));
}

#[tokio::test]
async fn the_redirect_address_follows_the_public_host_or_the_configuration() {
    let idp = Idp::start().await;
    let site = oidc_site(&idp, |_| {});
    let reply = site
        .request(
            "GET",
            "/api/auth/oidc/login",
            None,
            None,
            &[
                ("host", "127.0.0.1:8091"),
                ("x-forwarded-host", "livres.example"),
                ("x-forwarded-proto", "https"),
            ],
        )
        .await;
    let url = reply.location().unwrap();
    assert_eq!(
        param(&url, "redirect_uri").as_deref(),
        Some("https://livres.example/api/auth/oidc/callback")
    );
    // Derrière HTTPS, les cookies de la connexion sont `Secure` aussi.
    assert!(
        reply
            .set_cookie_line("easybook_oidc")
            .unwrap()
            .contains("; Secure")
    );

    let fixed = oidc_site(&idp, |oidc| {
        oidc.redirect_url = "https://fixe.example/retour".into()
    });
    let (url, _) = begin(&fixed, "/api/auth/oidc/login", None).await;
    assert_eq!(
        param(&url, "redirect_uri").as_deref(),
        Some("https://fixe.example/retour")
    );
}

#[tokio::test]
async fn an_unknown_identity_is_refused_unless_the_admin_allows_creation() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |_| {});

    let reply = sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.location().as_deref(), Some("/login?error=no_account"));
    assert!(reply.cookie("easybook_session").is_none());
    // Rien n'a été créé, rien n'a été lié.
    assert_eq!(site.accounts.count_users().unwrap(), 1);
    assert!(
        site.accounts
            .identity(&idp.issuer, "sub-alice")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn auto_create_opens_a_session_for_a_new_passwordless_account() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);

    let reply = sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(reply.location().as_deref(), Some("/"));
    let cookie = reply.cookie("easybook_session").unwrap();
    // Le cookie de connexion en cours est effacé.
    assert!(
        reply
            .set_cookie_line("easybook_oidc")
            .unwrap()
            .contains("Max-Age=0")
    );

    let session = site.get("/api/auth/session", Some(&cookie)).await.json();
    let user = &session["user"];
    assert_eq!(user["username"], "alice");
    assert_eq!(user["display_name"], "ALICE");
    assert_eq!(user["email"], "alice@example.org");
    assert_eq!(user["role"], "user");
    assert_eq!(user["has_password"], false);
    assert_eq!(user["identities"][0]["subject"], "sub-alice");
    assert_eq!(user["identities"][0]["issuer"], idp.issuer);
    assert_eq!(
        site.get("/api/books", Some(&cookie)).await.status,
        StatusCode::OK
    );
    // Un compte créé ainsi n'est jamais administrateur.
    assert_eq!(
        site.get("/api/admin/users", Some(&cookie)).await.status,
        StatusCode::FORBIDDEN
    );

    // Revenir retrouve le même compte, sans doublon.
    let again = sign_in(&site, &idp, "sub-alice").await;
    let cookie2 = again.cookie("easybook_session").unwrap();
    let id = |cookie: &str| {
        let token = cookie.trim_start_matches("easybook_session=");
        site.accounts.session_user(token).unwrap().unwrap().id
    };
    assert_eq!(id(&cookie), id(&cookie2));
    assert_eq!(site.accounts.count_users().unwrap(), 2);
}

#[tokio::test]
async fn the_username_of_an_auto_created_account_never_collides() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);
    site.add_user("alice", Role::User, Some(PASSWORD));

    let reply = sign_in(&site, &idp, "sub-alice").await;
    let token = reply.cookie("easybook_session").unwrap();
    let user = site
        .accounts
        .session_user(token.trim_start_matches("easybook_session="))
        .unwrap()
        .unwrap();
    assert_eq!(user.username, "alice-2");
    // Le compte local homonyme n'est pas touché : pas de prise de contrôle par le nom.
    let local = site.accounts.user_by_username("alice").unwrap().unwrap();
    assert!(local.has_password);
    assert_ne!(local.id, user.id);
}

#[tokio::test]
async fn the_destination_after_login_stays_on_this_site() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);

    for (next, expected) in [
        ("/read/livre.epub", "/read/livre.epub"),
        ("https://pirate.example", "/"),
        ("//pirate.example", "/"),
    ] {
        let uri = format!(
            "/api/auth/oidc/login?next={}",
            reqwest::Url::parse_with_params("http://x", [("n", next)])
                .unwrap()
                .query()
                .unwrap()
                .trim_start_matches("n=")
        );
        let (url, flow) = begin(&site, &uri, None).await;
        let reply = finish(&site, &idp, &url, "sub-alice", &flow, None).await;
        assert_eq!(reply.location().as_deref(), Some(expected), "{next}");
    }
}

#[tokio::test]
async fn link_by_email_attaches_the_identity_to_the_local_account() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.link_by_email = true);
    let local = site
        .accounts
        .create_user(easybook::accounts::NewUser {
            username: "alice-locale".into(),
            email: Some("Alice@example.org".into()),
            password: Some(PASSWORD.into()),
            ..Default::default()
        })
        .unwrap();

    let reply = sign_in(&site, &idp, "sub-alice").await;
    let cookie = reply.cookie("easybook_session").unwrap();
    let session = site.get("/api/auth/session", Some(&cookie)).await.json();
    assert_eq!(session["user"]["id"], local.id);
    assert_eq!(session["user"]["identities"].as_array().unwrap().len(), 1);
    // Le compte local garde son mot de passe : les deux façons de se connecter coexistent.
    assert_eq!(
        site.login_reply("alice@example.org", PASSWORD).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn link_by_email_ignores_an_address_the_provider_does_not_certify() {
    let idp = Idp::start().await;
    idp.person(
        "sub-alice",
        Person {
            email_verified: false,
            ..alice()
        },
    );
    let site = oidc_site(&idp, |oidc| oidc.link_by_email = true);
    site.accounts
        .create_user(easybook::accounts::NewUser {
            username: "alice-locale".into(),
            email: Some("alice@example.org".into()),
            ..Default::default()
        })
        .unwrap();

    let reply = sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(reply.location().as_deref(), Some("/login?error=no_account"));
    assert!(
        site.accounts
            .identity(&idp.issuer, "sub-alice")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn an_admin_invitation_links_a_local_account_to_an_oidc_one() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |_| {});
    let admin = site.admin().await;
    let local = site.add_user("alice-locale", Role::User, Some(PASSWORD));

    let invite = site
        .post(
            &format!("/api/admin/users/{}/link-token", local.id),
            Some(&admin),
            json!({}),
        )
        .await;
    assert_eq!(invite.status, StatusCode::CREATED);
    let invite = invite.json();
    let url = invite["url"].as_str().unwrap().to_string();
    assert!(url.starts_with("/api/auth/oidc/login?link="), "{url}");
    assert_eq!(invite["expires_in"], 900);

    // La personne invitée n'a pas de session : elle ouvre le lien, s'authentifie
    // chez le fournisseur, et arrive connectée **au compte local**.
    let (authorize, flow) = begin(&site, &url, None).await;
    let reply = finish(&site, &idp, &authorize, "sub-alice", &flow, None).await;
    assert_eq!(reply.location().as_deref(), Some("/"));
    let cookie = reply.cookie("easybook_session").unwrap();
    let session = site.get("/api/auth/session", Some(&cookie)).await.json();
    assert_eq!(session["user"]["id"], local.id);
    assert_eq!(session["user"]["username"], "alice-locale");
    assert_eq!(session["user"]["identities"][0]["subject"], "sub-alice");
    assert_eq!(site.accounts.count_users().unwrap(), 2);

    // Le compte local et le compte OIDC ne font plus qu'un : la prochaine fois, le
    // bouton suffit — et le mot de passe marche toujours.
    let again = sign_in(&site, &idp, "sub-alice").await;
    let cookie2 = again.cookie("easybook_session").unwrap();
    let session2 = site.get("/api/auth/session", Some(&cookie2)).await.json();
    assert_eq!(session2["user"]["id"], local.id);
    assert_eq!(
        site.login_reply("alice-locale", PASSWORD).await.status,
        StatusCode::OK
    );

    // L'invitation était à usage unique.
    let replay = site.get(&url, None).await;
    assert_eq!(
        replay.location().as_deref(),
        Some("/login?error=invalid_link")
    );
}

#[tokio::test]
async fn an_invitation_is_not_valid_for_anyone_else() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |_| {});

    let reply = site.get("/api/auth/oidc/login?link=invention", None).await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=invalid_link")
    );
    assert!(reply.set_cookie_line("easybook_oidc").is_none());

    let local = site.add_user("alice-locale", Role::User, Some(PASSWORD));
    let stale = site.accounts.create_link_token(&local.id, -1).unwrap();
    let reply = site
        .get(&format!("/api/auth/oidc/login?link={stale}"), None)
        .await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=invalid_link")
    );
}

#[tokio::test]
async fn an_invitation_cannot_take_an_identity_already_linked_elsewhere() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |_| {});
    let owner = site.add_user("proprietaire", Role::User, Some(PASSWORD));
    site.accounts
        .link_identity(&owner.id, &idp.issuer, "sub-alice", None)
        .unwrap();
    let target = site.add_user("cible", Role::User, Some(PASSWORD));
    let token = site.accounts.create_link_token(&target.id, 600).unwrap();

    let (authorize, flow) = begin(&site, &format!("/api/auth/oidc/login?link={token}"), None).await;
    let reply = finish(&site, &idp, &authorize, "sub-alice", &flow, None).await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=identity_taken")
    );
    assert!(reply.cookie("easybook_session").is_none());
    assert_eq!(site.accounts.identities_of(&target.id).unwrap().len(), 0);
    // L'invitation n'est pas brûlée par un échec.
    assert!(site.accounts.link_token_user(&token).unwrap().is_some());
}

#[tokio::test]
async fn an_admin_can_link_and_unlink_by_subject() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |_| {});
    let admin = site.admin().await;
    let local = site.add_user("alice-locale", Role::User, Some(PASSWORD));

    let linked = site
        .post(
            &format!("/api/admin/users/{}/identities", local.id),
            Some(&admin),
            json!({ "subject": "sub-alice" }),
        )
        .await;
    assert_eq!(linked.status, StatusCode::CREATED);
    // L'émetteur par défaut est celui du fournisseur configuré.
    assert_eq!(linked.json()["issuer"], idp.issuer);
    let identity_id = linked.json()["id"].as_str().unwrap().to_string();

    let reply = sign_in(&site, &idp, "sub-alice").await;
    let cookie = reply.cookie("easybook_session").unwrap();
    assert_eq!(
        site.get("/api/auth/session", Some(&cookie)).await.json()["user"]["id"],
        local.id
    );

    // Lier deux fois : sans effet ; lier chez quelqu'un d'autre : refusé.
    let other = site.add_user("autre", Role::User, Some(PASSWORD));
    let steal = site
        .post(
            &format!("/api/admin/users/{}/identities", other.id),
            Some(&admin),
            json!({ "subject": "sub-alice" }),
        )
        .await;
    assert_eq!(
        (steal.status, steal.error().as_deref()),
        (StatusCode::CONFLICT, Some("identity_taken"))
    );
    let blank = site
        .post(
            &format!("/api/admin/users/{}/identities", other.id),
            Some(&admin),
            json!({ "subject": "  " }),
        )
        .await;
    assert_eq!(blank.status, StatusCode::BAD_REQUEST);

    let unlink = site
        .request(
            "DELETE",
            &format!("/api/admin/users/{}/identities/{identity_id}", local.id),
            Some(&admin),
            None,
            &[],
        )
        .await;
    assert_eq!(unlink.status, StatusCode::NO_CONTENT);
    let after = sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(after.location().as_deref(), Some("/login?error=no_account"));
}

#[tokio::test]
async fn the_only_way_to_log_in_cannot_be_unlinked() {
    let idp = Idp::start().await;
    let site = oidc_site(&idp, |_| {});
    let admin = site.admin().await;
    let carol = site.add_user("carol", Role::User, None);
    let identity = site
        .accounts
        .link_identity(&carol.id, &idp.issuer, "sub-carol", None)
        .unwrap();

    let reply = site
        .request(
            "DELETE",
            &format!("/api/admin/users/{}/identities/{}", carol.id, identity.id),
            Some(&admin),
            None,
            &[],
        )
        .await;
    assert_eq!(
        (reply.status, reply.error().as_deref()),
        (StatusCode::CONFLICT, Some("last_credential"))
    );

    // Le titulaire non plus, de son côté.
    let cookie = site.session_for(&carol);
    let own = site
        .request(
            "DELETE",
            &format!("/api/auth/identities/{}", identity.id),
            Some(&cookie),
            None,
            &[],
        )
        .await;
    assert_eq!(own.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_user_unlinks_only_their_own_identity() {
    let idp = Idp::start().await;
    let site = oidc_site(&idp, |_| {});
    let alice = site.add_user("alice", Role::User, Some(PASSWORD));
    let bob = site.add_user("bob", Role::User, Some(PASSWORD));
    let mine = site
        .accounts
        .link_identity(&alice.id, &idp.issuer, "s-a", None)
        .unwrap();
    let theirs = site
        .accounts
        .link_identity(&bob.id, &idp.issuer, "s-b", None)
        .unwrap();
    let cookie = site.session_for(&alice);

    let steal = site
        .request(
            "DELETE",
            &format!("/api/auth/identities/{}", theirs.id),
            Some(&cookie),
            None,
            &[],
        )
        .await;
    assert_eq!(steal.status, StatusCode::NOT_FOUND);
    let own = site
        .request(
            "DELETE",
            &format!("/api/auth/identities/{}", mine.id),
            Some(&cookie),
            None,
            &[],
        )
        .await;
    assert_eq!(own.status, StatusCode::NO_CONTENT);
    assert!(
        site.accounts
            .identity(&idp.issuer, "s-b")
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn linking_ones_own_account_is_up_to_the_admin() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());

    // Par défaut : réservé aux administrateurs.
    let site = oidc_site(&idp, |_| {});
    let alice_user = site.add_user("alice-locale", Role::User, Some(PASSWORD));
    let cookie = site.session_for(&alice_user);
    let denied = site.get("/api/auth/oidc/link", Some(&cookie)).await;
    assert_eq!(
        (denied.status, denied.error().as_deref()),
        (StatusCode::FORBIDDEN, Some("self_link_disabled"))
    );
    assert_eq!(
        site.get("/api/auth/oidc/link", None).await.status,
        StatusCode::UNAUTHORIZED
    );

    // L'administrateur l'a autorisé : chacun lie son propre compte.
    let site = oidc_site(&idp, |oidc| oidc.self_link = true);
    let alice_user = site.add_user("alice-locale", Role::User, Some(PASSWORD));
    let cookie = site.session_for(&alice_user);
    let (url, flow) = begin(&site, "/api/auth/oidc/link", Some(&cookie)).await;
    let reply = finish(&site, &idp, &url, "sub-alice", &flow, Some(&cookie)).await;
    assert_eq!(reply.location().as_deref(), Some("/account?linked=1"));
    // La session en cours est conservée : pas de nouveau cookie.
    assert!(reply.cookie("easybook_session").is_none());
    let session = site.get("/api/auth/session", Some(&cookie)).await.json();
    assert_eq!(session["user"]["identities"][0]["subject"], "sub-alice");
    assert_eq!(session["oidc"]["self_link"], true);
}

#[tokio::test]
async fn an_admin_may_link_their_own_account_even_when_self_link_is_off() {
    let idp = Idp::start().await;
    idp.person("sub-root", Person::new("root", "root@example.org"));
    let site = oidc_site(&idp, |_| {});
    let admin = site.admin().await;

    let (url, flow) = begin(&site, "/api/auth/oidc/link", Some(&admin)).await;
    let reply = finish(&site, &idp, &url, "sub-root", &flow, Some(&admin)).await;
    assert_eq!(reply.location().as_deref(), Some("/account?linked=1"));
    let root = site.accounts.user_by_username("root").unwrap().unwrap();
    assert_eq!(site.accounts.identities_of(&root.id).unwrap().len(), 1);
}

#[tokio::test]
async fn self_link_fails_when_the_session_changed_in_between() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.self_link = true);
    let alice_user = site.add_user("alice-locale", Role::User, Some(PASSWORD));
    let bob = site.add_user("bob", Role::User, Some(PASSWORD));
    let alice_cookie = site.session_for(&alice_user);
    let bob_cookie = site.session_for(&bob);

    // Alice lance la liaison ; le retour arrive dans le navigateur de Bob.
    let (url, flow) = begin(&site, "/api/auth/oidc/link", Some(&alice_cookie)).await;
    let reply = finish(&site, &idp, &url, "sub-alice", &flow, Some(&bob_cookie)).await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/account?error=session_mismatch")
    );
    assert!(
        site.accounts
            .identity(&idp.issuer, "sub-alice")
            .unwrap()
            .is_none()
    );

    // Et sans session du tout.
    let (url, flow) = begin(&site, "/api/auth/oidc/link", Some(&alice_cookie)).await;
    let reply = finish(&site, &idp, &url, "sub-alice", &flow, None).await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/account?error=session_mismatch")
    );
}

#[tokio::test]
async fn self_link_refuses_an_identity_that_belongs_to_someone_else() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.self_link = true);
    let owner = site.add_user("proprietaire", Role::User, Some(PASSWORD));
    site.accounts
        .link_identity(&owner.id, &idp.issuer, "sub-alice", None)
        .unwrap();
    let me = site.add_user("moi", Role::User, Some(PASSWORD));
    let cookie = site.session_for(&me);

    let (url, flow) = begin(&site, "/api/auth/oidc/link", Some(&cookie)).await;
    let reply = finish(&site, &idp, &url, "sub-alice", &flow, Some(&cookie)).await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/account?error=identity_taken")
    );
    assert_eq!(site.accounts.identities_of(&me.id).unwrap().len(), 0);
}

#[tokio::test]
async fn a_disabled_account_is_refused_through_oidc_too() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |_| {});
    let local = site.add_user("alice-locale", Role::User, Some(PASSWORD));
    site.accounts
        .link_identity(&local.id, &idp.issuer, "sub-alice", None)
        .unwrap();
    let admin = site.admin().await;
    site.request(
        "PATCH",
        &format!("/api/admin/users/{}", local.id),
        Some(&admin),
        Some(json!({ "disabled": true })),
        &[],
    )
    .await;

    let reply = sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=account_disabled")
    );
    assert!(reply.cookie("easybook_session").is_none());
}

#[tokio::test]
async fn the_return_must_come_from_the_browser_that_started_the_login() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);

    let (url, flow) = begin(&site, "/api/auth/oidc/login", None).await;
    let back = path_and_query(&idp.authenticate(&url, "sub-alice").await);

    // Sans le cookie : un lien de retour piégé, ouvert par une autre personne.
    let no_cookie = site.get(&back, None).await;
    assert_eq!(
        no_cookie.location().as_deref(),
        Some("/login?error=oidc_state")
    );
    // Avec le cookie d'une autre connexion.
    let (_, other_flow) = begin(&site, "/api/auth/oidc/login", None).await;
    let wrong_cookie = site.get(&back, Some(&other_flow)).await;
    assert_eq!(
        wrong_cookie.location().as_deref(),
        Some("/login?error=oidc_state")
    );
    assert!(wrong_cookie.cookie("easybook_session").is_none());
    assert_eq!(site.accounts.count_users().unwrap(), 1);

    // Le bon navigateur passe — une seule fois.
    let ok = site.get(&back, Some(&flow)).await;
    assert!(ok.cookie("easybook_session").is_some());
    let replay = site.get(&back, Some(&flow)).await;
    assert_eq!(
        replay.location().as_deref(),
        Some("/login?error=oidc_state")
    );
    assert!(replay.cookie("easybook_session").is_none());
}

#[tokio::test]
async fn a_forged_callback_is_refused() {
    let idp = Idp::start().await;
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);
    for uri in [
        "/api/auth/oidc/callback",
        "/api/auth/oidc/callback?code=x",
        "/api/auth/oidc/callback?code=x&state=inconnu",
    ] {
        let reply = site.get(uri, Some("easybook_oidc=inconnu")).await;
        assert_eq!(
            reply.location().as_deref(),
            Some("/login?error=oidc_state"),
            "{uri}"
        );
    }
    assert_eq!(site.accounts.count_users().unwrap(), 1);
}

#[tokio::test]
async fn a_refusal_at_the_provider_comes_back_as_denied() {
    let idp = Idp::start().await;
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);
    let (url, flow) = begin(&site, "/api/auth/oidc/login", None).await;
    let state = param(&url, "state").unwrap();

    let reply = site
        .get(
            &format!("/api/auth/oidc/callback?error=access_denied&state={state}"),
            Some(&flow),
        )
        .await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=oidc_denied")
    );
}

#[tokio::test]
async fn a_bad_id_token_never_opens_a_session() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);

    let faults = [
        (
            "autre client (aud)",
            Quirks {
                audience: Some("autre-client".into()),
                ..Quirks::default()
            },
        ),
        (
            "autre émetteur (iss)",
            Quirks {
                issuer: Some("https://pirate.example".into()),
                ..Quirks::default()
            },
        ),
        (
            "nonce rejoué",
            Quirks {
                nonce: Some("un-autre-nonce".into()),
                ..Quirks::default()
            },
        ),
        (
            "jeton échu",
            Quirks {
                expired: true,
                ..Quirks::default()
            },
        ),
        (
            "clé inconnue",
            Quirks {
                foreign_key: true,
                ..Quirks::default()
            },
        ),
        (
            "HS256 avec le secret du client",
            Quirks {
                hs256: true,
                ..Quirks::default()
            },
        ),
        (
            "point d'accès jeton en panne",
            Quirks {
                token_error: true,
                ..Quirks::default()
            },
        ),
    ];
    for (name, quirks) in faults {
        idp.quirks(quirks);
        let reply = sign_in(&site, &idp, "sub-alice").await;
        assert_eq!(
            reply.location().as_deref(),
            Some("/login?error=oidc_failed"),
            "{name}"
        );
        assert!(reply.cookie("easybook_session").is_none(), "{name}");
    }
    // Aucune de ces fautes n'a créé de compte.
    assert_eq!(site.accounts.count_users().unwrap(), 1);

    // Contrôle : sans faute, la même connexion passe.
    idp.quirks(Quirks::default());
    assert!(
        sign_in(&site, &idp, "sub-alice")
            .await
            .cookie("easybook_session")
            .is_some()
    );
}

#[tokio::test]
async fn a_provider_announcing_another_issuer_is_not_trusted() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    idp.quirks(Quirks {
        discovery_issuer: Some("https://pirate.example".into()),
        ..Quirks::default()
    });
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);

    // Dès le départ : le navigateur n'est même pas envoyé chez ce fournisseur.
    let reply = site.get("/api/auth/oidc/login", None).await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=oidc_failed")
    );
    assert!(reply.set_cookie_line("easybook_oidc").is_none());
}

#[tokio::test]
async fn a_wrong_client_secret_is_a_failure_not_a_login() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| {
        oidc.auto_create = true;
        oidc.client_secret = "mauvais".into();
    });
    let reply = sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=oidc_failed")
    );
}

#[tokio::test]
async fn an_unreachable_provider_fails_cleanly() {
    let idp = Idp::start().await;
    let site = oidc_site(&idp, |oidc| oidc.issuer = "http://127.0.0.1:9".into());
    let reply = site.get("/api/auth/oidc/login", None).await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=oidc_failed")
    );
}

#[tokio::test]
async fn discovery_may_use_an_internal_address() {
    // Conteneurs : le fournisseur annonce l'adresse publique, mais easybook le
    // joint par une autre (`discovery_url`). L'émetteur attendu reste le public.
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| {
        oidc.auto_create = true;
        oidc.discovery_url = format!("{}/", idp.issuer);
    });
    let reply = sign_in(&site, &idp, "sub-alice").await;
    assert!(reply.cookie("easybook_session").is_some());
}

#[tokio::test]
async fn the_requested_scopes_come_from_the_configuration() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| {
        oidc.auto_create = true;
        oidc.scopes = vec!["openid".into(), "email".into()];
    });
    sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(idp.last_scopes(), ["openid", "email"]);
}

#[tokio::test]
async fn auto_create_refuses_an_address_the_provider_does_not_certify() {
    let idp = Idp::start().await;
    idp.person(
        "sub-alice",
        Person {
            email_verified: false,
            ..alice()
        },
    );
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);

    let reply = sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=email_required")
    );
    assert_eq!(site.accounts.count_users().unwrap(), 1);
}

#[tokio::test]
async fn auto_create_does_not_take_over_a_local_address() {
    let idp = Idp::start().await;
    idp.person("sub-alice", alice());
    let site = oidc_site(&idp, |oidc| oidc.auto_create = true);
    site.accounts
        .create_user(easybook::accounts::NewUser {
            email: Some("alice@example.org".into()),
            password: Some(PASSWORD.into()),
            ..Default::default()
        })
        .unwrap();

    let reply = sign_in(&site, &idp, "sub-alice").await;
    assert_eq!(
        reply.location().as_deref(),
        Some("/login?error=email_taken")
    );
    assert!(reply.cookie("easybook_session").is_none());
    assert_eq!(site.accounts.count_users().unwrap(), 2);
}
