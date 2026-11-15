//! Tests d'intégration de l'API : un vrai dossier de livres, de vrais fichiers,
//! et le routeur complet (celui de la production).
//!
//! Ils vérifient le **contrat** que l'interface consomme : le JSON du catalogue,
//! les fiches de livres, les couvertures de chaque format, les chapitres, les
//! pages et les refus de traversée de chemin.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use easybook::api::{self, AppState};
use http_body_util::BodyExt;
use serde_json::Value;
use std::fs;
use std::time::UNIX_EPOCH;
use tower::ServiceExt;

/// Bibliothèque de test :
/// - `livre.epub` à la racine,
/// - `mangas/T1.cbz` (3 pages) et `mangas/guide.pdf`,
/// - `mangas/sous/T9.cbz` (1 page),
/// - `notes.txt`, qui n'est pas un livre.
fn library() -> (tempfile::TempDir, Router) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("mangas/sous")).unwrap();
    common::epub(root);
    common::cbz(&root.join("mangas"), "T1.cbz", 3);
    common::cbz(&root.join("mangas/sous"), "T9.cbz", 1);
    common::pdf(&root.join("mangas"), "guide.pdf");
    fs::write(root.join("notes.txt"), "pas un livre").unwrap();

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let app = api::routes(AppState::new(root, tx));
    (dir, app)
}

/// Appelle une route et rend le statut, les en-têtes et le corps.
async fn call(app: &Router, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

/// Appelle une route qui répond du JSON.
async fn json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let (status, _, body) = call(app, uri).await;
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn header_is(headers: &HeaderMap, name: header::HeaderName, expected: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

#[tokio::test]
async fn health_answers() {
    let (_dir, app) = library();
    let (status, body) = json(&app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn catalogue_lists_books_and_folders() {
    let (_dir, app) = library();
    let (status, body) = json(&app, "/api/books").await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        body["total"], 4,
        "trois livres dans mangas + un à la racine"
    );

    let books = body["books"].as_array().unwrap();
    assert!(books.iter().all(|book| book["name"] != "notes.txt"));
    let formats: Vec<&str> = books
        .iter()
        .filter_map(|book| book["format"].as_str())
        .collect();
    assert!(formats.contains(&"epub"));
    assert!(formats.contains(&"cbz"));
    assert!(formats.contains(&"pdf"));

    // Le compte d'un dossier est récursif : `mangas` contient aussi `sous/T9`.
    let folders = body["folders"].as_array().unwrap();
    let mangas = folders
        .iter()
        .find(|folder| folder["rel"] == "mangas")
        .expect("dossier mangas");
    assert_eq!(mangas["count"], 3);
    let sous = folders
        .iter()
        .find(|folder| folder["rel"] == "mangas/sous")
        .expect("sous-dossier");
    assert_eq!(sous["count"], 1);
}

#[tokio::test]
async fn catalogue_announces_the_pages_of_each_book() {
    let (_dir, app) = library();
    let (status, body) = json(&app, "/api/books").await;
    assert_eq!(status, StatusCode::OK);

    let pages = |rel: &str| -> Value {
        body["books"]
            .as_array()
            .unwrap()
            .iter()
            .find(|book| book["rel"] == rel)
            .unwrap_or_else(|| panic!("{rel} absent du catalogue"))["pages"]
            .clone()
    };

    assert_eq!(pages("mangas/T1.cbz"), 3, "trois planches");
    assert_eq!(pages("mangas/sous/T9.cbz"), 1);
    assert_eq!(pages("mangas/guide.pdf"), 1);
    assert_eq!(pages("livre.epub"), 2, "deux chapitres");
}

#[tokio::test]
async fn book_details_follow_the_format() {
    let (_dir, app) = library();

    let (status, epub) = json(&app, "/api/book?path=livre.epub").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(epub["format"], "epub");
    assert_eq!(epub["title"], "Livre de test");
    assert_eq!(epub["author"], "Autrice");
    let chapters = epub["chapters"].as_array().unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0]["title"], "Début");
    assert_eq!(chapters[1]["title"], "Suite");
    assert_eq!(epub["pages"], 2, "un EPUB annonce ses chapitres");

    let (status, cbz) = json(&app, "/api/book?path=mangas%2FT1.cbz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cbz["format"], "cbz");
    assert_eq!(cbz["pages"], 3);
    assert!(cbz["chapters"].is_null());

    let (status, pdf) = json(&app, "/api/book?path=mangas%2Fguide.pdf").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pdf["format"], "pdf");
    assert_eq!(pdf["supported"], true);
    assert_eq!(pdf["pages"], 1, "le PDF de test n'a qu'une page");
}

#[tokio::test]
async fn covers_are_jpegs_for_every_format() {
    let (_dir, app) = library();

    // EPUB (image déclarée), CBZ (première page), PDF (image de la page 1).
    for path in ["livre.epub", "mangas%2FT1.cbz", "mangas%2Fguide.pdf"] {
        let (status, headers, body) = call(&app, &format!("/api/cover?path={path}")).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(
            header_is(&headers, header::CONTENT_TYPE, "image/jpeg"),
            "{path} : type MIME"
        );
        assert_eq!(&body[..2], &[0xFF, 0xD8], "{path} : JPEG attendu");
    }
}

#[tokio::test]
async fn cover_cache_follows_the_version_parameter() {
    let (dir, app) = library();
    let modified = fs::metadata(dir.path().join("livre.epub"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Sans version : le navigateur peut garder, mais doit revalider.
    let (_, headers, _) = call(&app, "/api/cover?path=livre.epub").await;
    assert!(header_is(
        &headers,
        header::CACHE_CONTROL,
        "public, max-age=300"
    ));

    // Avec la version du fichier : l'URL ne changera plus de contenu.
    let (_, headers, _) = call(&app, &format!("/api/cover?path=livre.epub&v={modified}")).await;
    assert!(header_is(
        &headers,
        header::CACHE_CONTROL,
        "public, max-age=31536000, immutable"
    ));
}

#[tokio::test]
async fn a_non_book_has_no_cover() {
    let (_dir, app) = library();
    let (status, _, _) = call(&app, "/api/cover?path=notes.txt").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn chapter_html_points_at_the_api_and_assets_are_served() {
    let (_dir, app) = library();

    let (status, headers, body) = call(&app, "/api/chapter?path=livre.epub&index=0").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_is(
        &headers,
        header::CONTENT_TYPE,
        "text/html; charset=utf-8"
    ));
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("<h1>Un</h1>"));
    assert!(
        html.contains("/api/asset?path=livre.epub&inner=OEBPS%2FImages%2Fcover.jpg"),
        "l'image du chapitre doit passer par l'API : {html}"
    );

    let (status, headers, body) = call(
        &app,
        "/api/asset?path=livre.epub&inner=OEBPS%2FImages%2Fcover.jpg",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_is(&headers, header::CONTENT_TYPE, "image/jpeg"));
    assert!(!body.is_empty());

    // Une feuille de style voit ses `url(...)` réécrites vers l'API.
    let (status, _, body) = call(
        &app,
        "/api/asset?path=livre.epub&inner=OEBPS%2FStyles%2Fstyle.css",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let css = String::from_utf8(body).unwrap();
    assert!(
        css.contains("/api/asset?path=livre.epub&inner=OEBPS%2FImages%2Fcover.jpg"),
        "CSS réécrit : {css}"
    );
}

#[tokio::test]
async fn comic_pages_are_served_in_order() {
    let (_dir, app) = library();

    let (status, headers, body) = call(&app, "/api/page?path=mangas%2FT1.cbz&index=1").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_is(&headers, header::CONTENT_TYPE, "image/jpeg"));
    assert!(!body.is_empty());

    let (status, _, _) = call(&app, "/api/page?path=mangas%2FT1.cbz&index=99").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn pdf_is_served_as_is() {
    let (_dir, app) = library();
    let (status, headers, body) = call(&app, "/api/raw?path=mangas%2Fguide.pdf").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_is(&headers, header::CONTENT_TYPE, "application/pdf"));
    assert_eq!(&body[..5], b"%PDF-", "le document est servi tel quel");
}

#[tokio::test]
async fn traversal_is_refused() {
    let (_dir, app) = library();

    for uri in [
        "/api/book?path=..%2Fsecret",
        "/api/raw?path=%2Fetc%2Fpasswd",
        "/api/cover?path=mangas%2F..%2F..%2Fsecret",
    ] {
        let (status, _, _) = call(&app, uri).await;
        assert!(
            matches!(status, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND),
            "{uri} : {status}"
        );
    }

    // Une ressource d'archive ne sort pas de l'archive non plus.
    let (status, _, _) = call(&app, "/api/asset?path=livre.epub&inner=..%2Fsecret").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
