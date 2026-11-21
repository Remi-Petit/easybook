//! Historique de lecture : la position d'un lecteur, et ce qu'en voit un
//! administrateur.
//!
//! Le site de test a des comptes **et** un historique (voir `common::http`), comme
//! la production : sans compte, il n'y aurait personne à qui rattacher une
//! position.

mod common;

use axum::http::StatusCode;
use common::http::{PASSWORD, site};
use easybook::accounts::Role;
use serde_json::json;
/// Corps d'une position, pour alléger les appels.
fn position(path: &str, page: i64, pages: i64) -> serde_json::Value {
    json!({ "path": path, "page": page, "pages": pages })
}

#[tokio::test]
async fn a_position_is_recorded_and_readable_by_an_admin() {
    let site = site();
    let alice = site.add_user("alice", Role::User, Some(PASSWORD));
    std::fs::write(site.dir.path().join("T1.cbz"), b"x").unwrap();
    let reader = site.session_for(&alice);

    let reply = site
        .post(
            "/api/read/progress",
            Some(&reader),
            position("T1.cbz", 12, 100),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "position enregistrée");

    // La position suivante met à jour la même ligne, pas une nouvelle.
    let reply = site
        .post(
            "/api/read/progress",
            Some(&reader),
            position("T1.cbz", 20, 100),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);

    let admin = site.admin().await;
    let uri = format!("/api/admin/users/{}/read/history", alice.id);
    let reply = site.get(&uri, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let history = reply.json();

    assert_eq!(history["summary"]["books"], 1);
    assert_eq!(history["summary"]["finished"], 0);
    assert_eq!(history["summary"]["pages"], 21);
    let recent = history["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["book"], "T1.cbz");
    // Titre et format viennent du disque, pas du corps de la requête.
    assert_eq!(recent[0]["title"], "T1");
    assert_eq!(recent[0]["format"], "cbz");
    assert_eq!(recent[0]["page"], 20);
    assert_eq!(recent[0]["pages"], 100);
    // Deux écritures rapprochées : une seule session, qui a suivi.
    let sessions = history["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["endPage"], 20);
}

#[tokio::test]
async fn a_finished_book_is_counted() {
    let site = site();
    let alice = site.add_user("alice", Role::User, Some(PASSWORD));
    std::fs::write(site.dir.path().join("T1.cbz"), b"x").unwrap();
    let reader = site.session_for(&alice);

    // Page 99 d'un livre de 100 pages : la dernière (index 0-based).
    site.post(
        "/api/read/progress",
        Some(&reader),
        position("T1.cbz", 99, 100),
    )
    .await;

    let admin = site.admin().await;
    let uri = format!("/api/admin/users/{}/read/history", alice.id);
    let history = site.get(&uri, Some(&admin)).await.json();
    assert_eq!(history["summary"]["finished"], 1);
}

#[tokio::test]
async fn only_the_owner_writes_and_only_admins_read() {
    let site = site();
    let alice = site.add_user("alice", Role::User, Some(PASSWORD));
    let bob = site.add_user("bob", Role::User, Some(PASSWORD));
    std::fs::write(site.dir.path().join("T1.cbz"), b"x").unwrap();

    // Sans session : refusé, rien n'est écrit.
    let reply = site
        .post("/api/read/progress", None, position("T1.cbz", 1, 10))
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);

    // Un livre absent de la bibliothèque est refusé.
    let reader = site.session_for(&alice);
    let reply = site
        .post(
            "/api/read/progress",
            Some(&reader),
            position("ghost.cbz", 1, 10),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    // Chacun ne voit que ses lignes.
    site.post(
        "/api/read/progress",
        Some(&reader),
        position("T1.cbz", 3, 10),
    )
    .await;
    let admin = site.admin().await;
    let bob_history = site
        .get(
            &format!("/api/admin/users/{}/read/history", bob.id),
            Some(&admin),
        )
        .await
        .json();
    assert_eq!(bob_history["summary"]["books"], 0);
    assert_eq!(bob_history["sessions"].as_array().unwrap().len(), 0);

    // Un simple utilisateur n'ouvre pas la page d'administration.
    let reply = site
        .get(
            &format!("/api/admin/users/{}/read/history", alice.id),
            Some(&reader),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_unknown_account_is_not_found() {
    let site = site();
    let admin = site.admin().await;
    let reply = site
        .get("/api/admin/users/inconnu/read/history", Some(&admin))
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_reader_sees_their_own_history() {
    let site = site();
    let alice = site.add_user("alice", Role::User, Some(PASSWORD));
    std::fs::write(site.dir.path().join("T1.cbz"), b"x").unwrap();
    let reader = site.session_for(&alice);

    site.post(
        "/api/read/progress",
        Some(&reader),
        position("T1.cbz", 7, 40),
    )
    .await;

    let reply = site.get("/api/read/history", Some(&reader)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let history = reply.json();
    assert_eq!(history["summary"]["books"], 1);
    assert_eq!(history["recent"][0]["book"], "T1.cbz");
    assert_eq!(history["recent"][0]["page"], 7);

    // Sans session, l'historique personnel n'existe pas.
    let reply = site.get("/api/read/history", None).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_book_detail_carries_the_position_of_the_reader() {
    let site = site();
    let alice = site.add_user("alice", Role::User, Some(PASSWORD));
    let bob = site.add_user("bob", Role::User, Some(PASSWORD));
    // Une vraie archive : la fiche d'un livre est lue par son module de format.
    common::cbz(site.dir.path(), "T1.cbz", 12);

    let reader = site.session_for(&alice);
    site.post(
        "/api/read/progress",
        Some(&reader),
        position("T1.cbz", 5, 12),
    )
    .await;

    // Le lecteur retrouve sa position dans la fiche du livre.
    let reply = site.get("/api/book?path=T1.cbz", Some(&reader)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let detail = reply.json();
    assert_eq!(detail["progress"]["page"], 5);
    assert_eq!(detail["progress"]["pages"], 12);

    // Un autre compte ne l'hérite pas.
    let other = site
        .get("/api/book?path=T1.cbz", Some(&site.session_for(&bob)))
        .await;
    assert!(other.json().get("progress").is_none());
}
