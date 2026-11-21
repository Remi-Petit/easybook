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

/// Appelle une route avec une méthode, un corps JSON ou binaire.
async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, body)
}

/// `POST /api/folder` avec un corps JSON.
async fn create_folder(app: &Router, parent: &str, name: &str) -> (StatusCode, Value) {
    let payload = serde_json::json!({ "parent": parent, "name": name });
    let (status, body) = send(
        app,
        "POST",
        "/api/folder",
        Some("application/json"),
        payload.to_string().into_bytes(),
    )
    .await;
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
async fn performance_describes_this_process() {
    let (_dir, app) = library();
    let (status, body) = json(&app, "/api/admin/performance").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["cpus"].as_u64().unwrap() >= 1);
    assert!(body["atMs"].as_u64().unwrap() > 0);
    if body["supported"] == true {
        let me = u64::from(std::process::id());
        let processes = body["processes"].as_array().unwrap();
        let mine = processes
            .iter()
            .find(|p| p["pid"] == me)
            .expect("process courant");
        assert!(mine["rss"].as_u64().unwrap() > 0);
    }
}

#[tokio::test]
async fn performance_history_is_absent_by_default() {
    let (_dir, app) = library();
    let (status, _) = json(&app, "/api/admin/performance/history?range=1h").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn performance_history_aggregates_stored_measurements() {
    use easybook::{config, perf_history};

    let dir = tempfile::tempdir().unwrap();
    common::epub(dir.path());
    let settings = config::Performance {
        sample_secs: 60,
        ..Default::default()
    };
    let history = std::sync::Arc::new(
        perf_history::History::open(&dir.path().join("metrics.db"), &settings).unwrap(),
    );
    // Deux mesures dans la même minute, il y a deux minutes.
    let base = perf_history::now_ms() / 60_000 * 60_000 - 120_000;
    for (index, mem) in [(0_i64, 100_i64), (1, 140)] {
        history
            .record(&perf_history::Measurement {
                at_ms: base + index * 10_000,
                mem_total: mem,
                mem_anon: mem / 2,
                mem_file: mem / 4,
                mem_kernel: 1,
                cpu_pct: 5.0,
                heap_live: 1_000,
                sessions: 2,
                sockets: 3,
                groups: vec![perf_history::GroupMeasurement {
                    name: "easybook".into(),
                    rss: mem,
                    cpu_pct: 5.0,
                }],
            })
            .unwrap();
    }
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let app = api::routes(AppState::new(dir.path(), tx).with_history(history));

    let (status, body) = json(&app, "/api/admin/performance/history?range=1h").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["range"], "1h");
    assert_eq!(body["stepMs"], 60_000);
    let points = body["points"].as_array().unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points[0]["memMax"], 140);
    assert_eq!(points[0]["groups"]["easybook"]["rssMax"], 140);

    // Fenêtre inconnue : la journée par défaut.
    let (status, body) = json(&app, "/api/admin/performance/history?range=nope").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["range"], "24h");
}

#[tokio::test]
async fn library_stats_describe_the_catalog() {
    let (_dir, app) = library();
    // Le résumé est calculé avec le catalogue, pas à la première lecture.
    let (status, _) = json(&app, "/api/books").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = json(&app, "/api/admin/library/stats").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["totals"]["books"], 4);
    // `mangas` et `mangas/sous` : les dossiers du catalogue, tous niveaux.
    assert_eq!(body["totals"]["folders"], 2);
    assert_eq!(body["totals"]["unreadable"], 0);
    assert_eq!(body["formats"].as_array().unwrap().len(), 3);
    assert_eq!(body["largest"].as_array().unwrap().len(), 4);
    let folders: Vec<&str> = body["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    // Regroupement par dossier parent : la racine compte pour un.
    assert!(folders.contains(&"mangas"));
    assert!(folders.contains(&"mangas/sous"));
    assert!(folders.contains(&""));
}

#[tokio::test]
async fn storage_summarizes_the_library_and_the_data() {
    let (_dir, app) = library();
    // Le résumé de la bibliothèque est calculé en même temps que le catalogue.
    let (status, _) = json(&app, "/api/books").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = json(&app, "/api/admin/storage").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["library"]["books"], 4);
    assert!(body["library"]["bytes"].as_u64().unwrap() > 0);
    assert_eq!(body["databases"].as_array().unwrap().len(), 2);
    assert!(body["app"]["binaryBytes"].as_u64().unwrap() > 0);
    // Les parts s'additionnent : l'application seule, puis les livres.
    let number = |field: &str| body[field].as_u64().unwrap();
    assert_eq!(
        number("appBytes"),
        number("dataBytes") + number("codeBytes")
    );
    assert_eq!(
        number("totalBytes"),
        number("appBytes") + body["library"]["bytes"].as_u64().unwrap()
    );
    assert!(!body["library"]["largest"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn performance_lists_the_books_being_read() {
    let (_dir, app) = library();
    let (status, _, _) = call(&app, "/api/page?path=mangas%2FT1.cbz&index=0").await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = json(&app, "/api/admin/performance").await;
    let readings = &body["readings"];
    assert_eq!(readings["idleLimitSecs"], 60);
    assert!(
        readings["active"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["rel"] == "mangas/T1.cbz" && r["requests"].as_u64().unwrap() >= 1),
        "la lecture en cours doit être listée : {readings}"
    );
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

/// Appelle une route avec `If-None-Match`.
async fn call_if_none_match(app: &Router, uri: &str, etag: &str) -> (StatusCode, HeaderMap) {
    let request = Request::builder()
        .uri(uri)
        .header(header::IF_NONE_MATCH, etag)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    (response.status(), response.headers().clone())
}

#[tokio::test]
async fn a_page_is_immutable_with_the_file_date_and_revalidated_by_etag() {
    let (dir, app) = library();
    let modified = fs::metadata(dir.path().join("mangas/T1.cbz"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let versioned = format!("/api/page?path=mangas%2FT1.cbz&index=1&v={modified}");
    let (status, headers, _) = call(&app, &versioned).await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_is(
        &headers,
        header::CACHE_CONTROL,
        "public, max-age=31536000, immutable"
    ));

    // Une date périmée n'est pas immuable.
    let (_, stale, _) = call(&app, "/api/page?path=mangas%2FT1.cbz&index=1&v=1").await;
    assert!(header_is(
        &stale,
        header::CACHE_CONTROL,
        "public, max-age=3600"
    ));

    // L'ETag rend un 304 sans corps, y compris sous sa forme faible (nginx).
    let etag = headers[header::ETAG].to_str().unwrap().to_string();
    for sent in [etag.clone(), format!("W/{etag}")] {
        let (status, headers) = call_if_none_match(&app, &versioned, &sent).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert!(headers.contains_key(header::ETAG));
    }

    // Une autre page n'a pas le même ETag.
    let (status, _) = call_if_none_match(
        &app,
        &format!("/api/page?path=mangas%2FT1.cbz&index=2&v={modified}"),
        &etag,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_page_can_be_asked_at_the_screen_width() {
    let dir = tempfile::tempdir().unwrap();
    let wide = common::jpeg(1600, 2000);
    let mut zip = zip::ZipWriter::new(fs::File::create(dir.path().join("large.cbz")).unwrap());
    zip.start_file("p1.jpg", zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(&mut zip, &wide).unwrap();
    zip.finish().unwrap();
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let app = api::routes(AppState::new(dir.path(), tx));

    // Sans `w` : l'image d'origine, octet pour octet.
    let (_, _, original) = call(&app, "/api/page?path=large.cbz&index=0").await;
    assert_eq!(original, wide);

    // 700 est arrondi à la largeur proposée au-dessus (800) ; le ratio est gardé.
    let (status, headers, small) = call(&app, "/api/page?path=large.cbz&index=0&w=700").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_is(&headers, header::CONTENT_TYPE, "image/jpeg"));
    assert!(small.len() < original.len());
    let decoded = image::load_from_memory(&small).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (800, 1000));

    // Plus large que la planche : rien à réduire, c'est l'original.
    let (_, _, same) = call(&app, "/api/page?path=large.cbz&index=0&w=2400").await;
    assert_eq!(same, original);

    // La taille fait partie de l'ETag : on ne confond pas deux variantes.
    let (_, full, _) = call(&app, "/api/page?path=large.cbz&index=0").await;
    assert_ne!(full[header::ETAG], headers[header::ETAG]);
}

#[tokio::test]
async fn the_catalogue_is_revalidated_by_etag() {
    let (dir, app) = library();

    let (status, headers, body) = call(&app, "/api/books").await;
    assert_eq!(status, StatusCode::OK);
    let etag = headers[header::ETAG].to_str().unwrap().to_string();
    assert!(!body.is_empty());

    let (status, _) = call_if_none_match(&app, "/api/books", &etag).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);

    // Un livre ajouté change le catalogue, donc l'ETag (le cache du catalogue
    // dure 2 s : on passe par l'API d'écriture, qui le rafraîchit tout de suite).
    common::cbz(&dir.path().join("mangas"), "T2.cbz", 1);
    let (_, _) = create_folder(&app, "", "Nouveau").await;
    let (status, _) = call_if_none_match(&app, "/api/books", &etag).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn pdf_is_served_as_is() {
    let (_dir, app) = library();
    let (status, headers, body) = call(&app, "/api/raw?path=mangas%2Fguide.pdf").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_is(&headers, header::CONTENT_TYPE, "application/pdf"));
    assert_eq!(&body[..5], b"%PDF-", "le document est servi tel quel");
}

/// Le lecteur PDF du navigateur demande des tranches : le serveur les honore.
#[tokio::test]
async fn raw_files_support_range_requests() {
    let (_dir, app) = library();
    let request = Request::builder()
        .uri("/api/raw?path=mangas%2Fguide.pdf")
        .header(header::RANGE, "bytes=0-4")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert!(header_is(
        response.headers(),
        header::CONTENT_TYPE,
        "application/pdf"
    ));
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"%PDF-");
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

/// Ce qui n'existe pas, ou qui n'est pas un livre : 404 ; ce qui existe mais ne
/// peut pas être lu : 422 avec le message du format.
#[tokio::test]
async fn missing_and_unreadable_books_are_answered_precisely() {
    let (_dir, app) = library();

    // Fichier absent du disque.
    let (status, body) = json(&app, "/api/book?path=mangas%2Fabsent.cbz").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.is_null());

    // Présent, mais pas un format de livre.
    let (status, _, body) = call(&app, "/api/book?path=notes.txt").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        String::from_utf8_lossy(&body).contains("format non reconnu"),
        "message du format attendu"
    );

    // Un PDF n'a pas de planches : /api/page n'a rien à servir.
    let (status, _, _) = call(&app, "/api/page?path=mangas%2Fguide.pdf&index=0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Un EPUB non plus (ce sont des chapitres, pas des pages).
    let (status, _, _) = call(&app, "/api/page?path=notes.txt&index=0").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Chapitre hors du sommaire.
    let (status, _, _) = call(&app, "/api/chapter?path=livre.epub&index=42").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Ressource absente de l'archive.
    let (status, _, _) = call(
        &app,
        "/api/asset?path=livre.epub&inner=OEBPS%2FStyles%2Fabsent.css",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Fichier qui n'est pas un livre : aucune ressource interne à servir.
    let (status, _, _) = call(&app, "/api/asset?path=notes.txt&inner=note.css").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Le fichier brut est proposé au lecteur du navigateur avec son nom d'origine.
#[tokio::test]
async fn raw_files_keep_their_type_and_name() {
    let (_dir, app) = library();
    let (status, headers, body) = call(&app, "/api/raw?path=livre.epub").await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_is(
        &headers,
        header::CONTENT_TYPE,
        "application/epub+zip"
    ));
    assert_eq!(
        headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some("inline; filename=\"livre.epub\"")
    );
    assert!(!body.is_empty());
}

/// Créer une catégorie depuis l'interface : le dossier apparaît dans le
/// catalogue, et une seconde création du même nom est un conflit.
#[tokio::test]
async fn a_category_can_be_created_from_the_api() {
    let (dir, app) = library();
    let root = dir.path();

    let (status, body) = create_folder(&app, "", "  Beaux livres  ").await;
    assert_eq!(status, StatusCode::CREATED, "création acceptée");
    assert_eq!(body["rel"], "Beaux livres", "le nom est rogné");
    assert!(root.join("Beaux livres").is_dir());

    // Le catalogue est rediffusé tout de suite, sans attendre le watcher.
    let (_, catalogue) = json(&app, "/api/books").await;
    let folders = catalogue["folders"].as_array().unwrap();
    assert!(
        folders.iter().any(|folder| folder["rel"] == "Beaux livres"),
        "le nouveau dossier doit être dans le catalogue"
    );

    // Une série dans une catégorie existante.
    let (status, body) = create_folder(&app, "mangas", "Série").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["rel"], "mangas/Série");
    assert!(root.join("mangas/Série").is_dir());

    // Doublon.
    let (status, _) = create_folder(&app, "", "Beaux livres").await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Nom refusé, parent inconnu.
    let (status, _) = create_folder(&app, "", "../evade").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = create_folder(&app, "Absent", "Série").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Déposer un livre : le corps **est** le fichier, il finit dans le catalogue.
#[tokio::test]
async fn a_book_can_be_dropped_onto_the_api() {
    let (dir, app) = library();
    let root = dir.path();
    let payload = common::cbz_bytes("Déposé.cbz", 2);

    let (status, body) = send(
        &app,
        "PUT",
        "/api/book?folder=mangas&name=D%C3%A9pos%C3%A9.cbz",
        Some("application/octet-stream"),
        payload.clone(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(body_json(&body)["rel"], "mangas/Déposé.cbz");

    let written = root.join("mangas/Déposé.cbz");
    assert_eq!(fs::read(&written).unwrap(), payload, "contenu intact");

    // Aucun fichier temporaire ne reste dans le dossier.
    let leftovers: Vec<String> = fs::read_dir(root.join("mangas"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "restes : {leftovers:?}");

    // Le catalogue le voit, avec son nombre de pages.
    let (_, catalogue) = json(&app, "/api/books").await;
    let book = catalogue["books"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["rel"] == "mangas/Déposé.cbz")
        .expect("livre déposé dans le catalogue");
    assert_eq!(book["pages"], 2);

    // Même nom, deuxième dépôt : conflit, et l'ancien contenu est conservé.
    let (status, _) = send(
        &app,
        "PUT",
        "/api/book?folder=mangas&name=D%C3%A9pos%C3%A9.cbz",
        None,
        common::cbz_bytes("x", 1),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(fs::read(&written).unwrap(), payload);

    // Corps vide, format inconnu, dossier inconnu.
    let (status, _) = send(
        &app,
        "PUT",
        "/api/book?folder=mangas&name=vide.cbz",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        "PUT",
        "/api/book?folder=mangas&name=notes.txt",
        None,
        b"pas un livre".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        "PUT",
        "/api/book?folder=Absent&name=T3.cbz",
        None,
        payload.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Le JSON d'une réponse de création.
fn body_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}
