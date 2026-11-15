//! Serveur HTTP et temps réel.
//!
//! L'API n'a **aucune authentification** : elle écoute sur la boucle locale par
//! défaut (`EASYBOOK_HOST=0.0.0.0` dans le conteneur). Publier un port, c'est
//! décider d'exposer la bibliothèque — c'est un choix explicite.
//!
//! Les routes :
//!
//! | Route | Rôle |
//! |---|---|
//! | `GET /api/health` | état du serveur |
//! | `GET /api/books` | catalogue complet (JSON) |
//! | `GET /api/book?path=` | sommaire d'un livre |
//! | `GET /api/cover?path=&v=` | couverture (vignette JPEG) |
//! | `GET /api/chapter?path=&index=` | document d'un chapitre (HTML réécrit) |
//! | `GET /api/asset?path=&inner=` | ressource interne (image, CSS, police) |
//! | `GET /api/page?path=&index=` | page d'une bande dessinée (image) |
//! | `GET /api/raw?path=` | fichier tel quel (PDF, EPUB) |
//! | `GET /ws` | catalogue poussé à chaque changement du dossier |

use crate::formats::{self, Format};
use crate::{library, rewrite, thumbnail};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

/// Durée de validité du catalogue en mémoire.
///
/// Le watcher rediffuse à chaque changement ; ce délai ne sert qu'à absorber
/// une rafale de requêtes (chargement de page, plusieurs onglets) sans refaire
/// un parcours complet du dossier à chaque fois. Ce n'est pas un sondage : rien
/// ne tourne en tâche de fond.
const SNAPSHOT_TTL: Duration = Duration::from_secs(2);

/// État partagé par les handlers.
#[derive(Clone)]
pub struct AppState {
    /// Racine de la bibliothèque.
    root: Arc<PathBuf>,
    /// Diffusion du catalogue aux WebSocket ouverts.
    ws: broadcast::Sender<String>,
    /// Dernier catalogue sérialisé, et son instant de production.
    snapshot: Arc<RwLock<String>>,
    snapshot_at: Arc<std::sync::Mutex<Option<Instant>>>,
    /// Un seul scan à la fois, même si plusieurs requêtes arrivent à froid.
    refresh: Arc<tokio::sync::Mutex<()>>,
}

impl AppState {
    pub fn new(root: impl Into<PathBuf>, ws: broadcast::Sender<String>) -> Self {
        Self {
            root: Arc::new(root.into()),
            ws,
            snapshot: Arc::new(RwLock::new(String::new())),
            snapshot_at: Arc::new(std::sync::Mutex::new(None)),
            refresh: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Racine courante de la bibliothèque.
    pub fn root(&self) -> PathBuf {
        self.root.as_ref().clone()
    }

    /// Catalogue encore frais, s'il y en a un.
    fn cached(&self) -> Option<String> {
        let fresh = self
            .snapshot_at
            .lock()
            .ok()
            .and_then(|at| *at)
            .is_some_and(|at| at.elapsed() < SNAPSHOT_TTL);
        if !fresh {
            return None;
        }
        let json = self.snapshot.read().ok()?.clone();
        (!json.is_empty()).then_some(json)
    }
}

/// Démarre le serveur et bloque jusqu'à son arrêt.
pub async fn serve(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    // Nom préfixé : `HOST` est aussi lu par d'autres outils du poste.
    let host = std::env::var("EASYBOOK_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8091".to_string());
    let addr = format!("{host}:{port}").parse::<SocketAddr>()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("API       : http://{addr}/api/books");
    println!("Temps réel: ws://{addr}/ws");
    axum::serve(listener, routes(state)).await?;
    Ok(())
}

/// Table des routes (séparée de [`serve`] pour les tests).
pub fn routes(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/books", get(list_books))
        .route("/api/book", get(get_book))
        .route("/api/cover", get(get_cover))
        .route("/api/chapter", get(get_chapter))
        .route("/api/asset", get(get_asset))
        .route("/api/page", get(get_page))
        .route("/api/raw", get(get_raw))
        .route("/ws", get(ws_library))
        // L'interface est servie par un autre process : elle appelle l'API en
        // direct, sans relais (les images et les polices d'un EPUB comprises).
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

/// `GET /api/books` — catalogue complet.
async fn list_books(State(state): State<AppState>) -> Response {
    if let Some(json) = state.cached() {
        return json_response(json);
    }

    // Le scan est bloquant (un `stat` par fichier) : il part sur le pool prévu
    // pour ça, sans immobiliser un worker de Tokio.
    let _guard = state.refresh.lock().await;
    if let Some(json) = state.cached() {
        return json_response(json);
    }

    let scanning = state.clone();
    match tokio::task::spawn_blocking(move || store_snapshot(&scanning)).await {
        Ok(json) => json_response(json),
        Err(e) => internal_error(e).into_response(),
    }
}

fn json_response(json: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], json).into_response()
}

/// Scanne la bibliothèque, mémorise le résultat et le rend.
fn store_snapshot(state: &AppState) -> String {
    let scan = library::scan(&state.root());
    let json = serde_json::to_string(&scan).unwrap_or_else(|_| "{}".to_string());
    if let Ok(mut slot) = state.snapshot.write() {
        *slot = json.clone();
    }
    if let Ok(mut at) = state.snapshot_at.lock() {
        *at = Some(Instant::now());
    }
    json
}

/// Scanne et diffuse aux clients WebSocket (appelé par le watcher).
pub fn broadcast_snapshot(state: &AppState) {
    let json = store_snapshot(state);
    // Aucun client à l'écoute n'est pas une erreur : le catalogue reste en
    // cache pour la prochaine requête HTTP.
    let _ = state.ws.send(json);
}

/// Fiche d'un livre : de quoi construire le lecteur.
#[derive(Serialize)]
struct BookDetail {
    rel: String,
    format: &'static str,
    /// `false` pour un format que ce serveur ne sait pas encore ouvrir.
    supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chapters: Option<Vec<formats::Chapter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pages: Option<usize>,
    /// Explication, quand le livre ne peut pas être ouvert.
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[derive(Deserialize)]
struct BookQuery {
    path: String,
}

/// `GET /api/book?path=` — sommaire ou nombre de pages.
async fn get_book(State(state): State<AppState>, Query(query): Query<BookQuery>) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let rel = query.path.clone();
    match tokio::task::spawn_blocking(move || detail(&path, &rel)).await {
        Ok(Ok(detail)) => Json(detail).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

fn detail(path: &Path, rel: &str) -> Result<BookDetail, String> {
    let format = Format::from_path(rel).ok_or_else(|| "format non reconnu".to_string())?;
    // Tout ce qui dépend du format passe par son module : l'API ne connaît que
    // le trait (voir `formats::BookFormat`).
    let module = formats::module(format);

    let outline = module.outline(path)?;
    let pages = module.pages(path)?;

    Ok(BookDetail {
        rel: rel.to_string(),
        format: format.id(),
        supported: module.readable(),
        title: outline.as_ref().and_then(|outline| outline.title.clone()),
        author: outline.as_ref().and_then(|outline| outline.author.clone()),
        chapters: outline
            .and_then(|outline| (!outline.chapters.is_empty()).then_some(outline.chapters)),
        pages,
        note: module.note().map(str::to_string),
    })
}

#[derive(Deserialize)]
struct CoverQuery {
    path: String,
    /// Date de modification annoncée par le catalogue (`BookInfo::modified`).
    /// Recopiée dans l'URL, elle rend la réponse « immuable » côté navigateur.
    #[serde(default)]
    v: Option<String>,
    /// Révision de l'extraction (voir `COVER_REVISION` dans l'interface) : elle
    /// n'a de sens que pour le cache, et permet de renouveler les images quand
    /// la façon de les choisir change.
    #[serde(default)]
    r: Option<String>,
}

/// `GET /api/cover?path=&v=` — couverture d'un livre, en vignette JPEG.
///
/// Une vignette, pas l'image d'origine : le catalogue en affiche des dizaines et
/// une planche scannée pèse plusieurs mégaoctets (voir [`crate::thumbnail`]).
async fn get_cover(State(state): State<AppState>, Query(query): Query<CoverQuery>) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // La date du fichier sert de version : elle change dès que le livre est
    // remplacé, et le catalogue l'annonce déjà.
    let version = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs());

    let rel = query.path.clone();
    // Clé du cache mémoire : la date du fichier, plus la révision demandée par
    // l'interface (une extraction qui change doit produire d'autres images).
    let cache_key = match &query.r {
        Some(revision) if !revision.is_empty() => format!("{version}|{revision}"),
        _ => version.to_string(),
    };
    let cover =
        tokio::task::spawn_blocking(move || thumbnail::cover(&path, &rel, &cache_key)).await;

    let announced = version.to_string();
    match cover {
        Ok(Some(bytes)) => {
            // Avec la version dans l'URL, le contenu ne changera plus pour cette
            // adresse : le navigateur peut la garder indéfiniment.
            let cache = if query.v.as_deref() == Some(announced.as_str()) {
                "public, max-age=31536000, immutable"
            } else {
                "public, max-age=300"
            };
            (
                [
                    (header::CONTENT_TYPE, "image/jpeg"),
                    (header::CACHE_CONTROL, cache),
                ],
                bytes,
            )
                .into_response()
        }
        // Pas de couverture pour ce livre (PDF, CBR, archive sans image) : ce
        // n'est pas une erreur, l'interface affiche son repli.
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct ChapterQuery {
    path: String,
    index: usize,
}

/// `GET /api/chapter?path=&index=` — document d'un chapitre, URL réécrites.
async fn get_chapter(
    State(state): State<AppState>,
    Query(query): Query<ChapterQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // Origine publique de l'API, vue par le navigateur : c'est elle qu'on inscrit
    // dans les URL réécrites. La requête arrive ici directement (CORS), l'en-tête
    // `Host` est donc celui de l'API.
    let origin = origin(&headers);
    let rel = query.path.clone();
    let index = query.index;
    let format = Format::from_path(&query.path);

    match tokio::task::spawn_blocking(move || match format {
        Some(format) => formats::module(format).chapter(&path, index, &rel, &origin),
        None => Err("format non reconnu".to_string()),
    })
    .await
    {
        Ok(Ok(html)) => html_response(html),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

fn html_response(html: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response()
}

#[derive(Deserialize)]
struct AssetQuery {
    path: String,
    inner: String,
}

/// `GET /api/asset?path=&inner=` — ressource interne à un livre.
async fn get_asset(
    State(state): State<AppState>,
    Query(query): Query<AssetQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(inner) = clean_inner(&query.inner) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let is_css = inner.to_ascii_lowercase().ends_with(".css");
    let rel = query.path.clone();
    let origin = origin(&headers);
    let entry = inner.clone();
    let format = Format::from_path(&query.path);

    let bytes = tokio::task::spawn_blocking(move || {
        format.and_then(|format| formats::module(format).asset(&path, &entry))
    })
    .await;

    match bytes {
        Ok(Some(bytes)) => {
            let body = if is_css {
                // Les `url(...)` d'une feuille de style sont relatives au
                // fichier CSS, pas au document : on les réécrit ici.
                rewrite::css(
                    &String::from_utf8_lossy(&bytes),
                    rewrite::dir_of(&inner),
                    &rel,
                    &origin,
                )
                .into_bytes()
            } else {
                bytes
            };
            (
                [
                    (header::CONTENT_TYPE, formats::content_type(&inner)),
                    // Une ressource d'archive ne change pas tant que le fichier
                    // n'est pas remplacé ; le catalogue annonce alors un nouveau
                    // livre, donc une nouvelle page.
                    (header::CACHE_CONTROL, "public, max-age=3600"),
                ],
                body,
            )
                .into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct PageQuery {
    path: String,
    index: usize,
}

/// `GET /api/page?path=&index=` — page d'une bande dessinée.
async fn get_page(State(state): State<AppState>, Query(query): Query<PageQuery>) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let index = query.index;
    let format = Format::from_path(&query.path);

    match tokio::task::spawn_blocking(move || match format {
        Some(format) => formats::module(format).page(&path, index),
        None => Err("format non reconnu".to_string()),
    })
    .await
    {
        Ok(Ok((name, bytes))) => (
            [
                (header::CONTENT_TYPE, formats::content_type(&name)),
                (header::CACHE_CONTROL, "public, max-age=3600"),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(e)) => (StatusCode::NOT_FOUND, e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct RawQuery {
    path: String,
}

/// `GET /api/raw?path=` — le fichier tel quel.
///
/// Sert au lecteur PDF du navigateur : il lui faut le document, pas une version
/// découpée. Le nom de fichier est proposé au téléchargement (`inline`, donc le
/// lecteur s'ouvre).
async fn get_raw(State(state): State<AppState>, Query(query): Query<RawQuery>) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = Format::from_path(&query.path)
        .map(Format::content_type)
        .unwrap_or("application/octet-stream");
    let name = query
        .path
        .rsplit('/')
        .next()
        .unwrap_or("livre")
        .replace('"', "");

    let mut headers = HeaderMap::new();
    if let Ok(value) = content_type.parse() {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = format!("inline; filename=\"{name}\"").parse() {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    headers.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());

    (headers, bytes).into_response()
}

/// WebSocket : catalogue initial, puis une poussée à chaque changement.
async fn ws_library(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: AppState) {
    let (mut sink, mut stream) = socket.split();

    // Abonnement **avant** le premier envoi : un changement survenu pendant le
    // scan initial ne doit pas être perdu.
    let mut rx = state.ws.subscribe();

    let initial_state = state.clone();
    let initial = tokio::task::spawn_blocking(move || {
        initial_state
            .cached()
            .unwrap_or_else(|| store_snapshot(&initial_state))
    })
    .await
    .unwrap_or_else(|_| "{}".to_string());

    if sink.send(Message::text(initial)).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            // Côté client : seule la fermeture nous intéresse.
            message = stream.next() => match message {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
            // Changement du dossier, poussé par le watcher.
            envelope = rx.recv() => match envelope {
                Ok(json) => {
                    if sink.send(Message::text(json)).await.is_err() {
                        break;
                    }
                }
                // En retard sur le canal : on renvoie un catalogue complet
                // plutôt que de laisser le client croire à un état périmé.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let fresh = snapshot_now(&state).await;
                    if sink.send(Message::text(fresh)).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            },
        }
    }
}

async fn snapshot_now(state: &AppState) -> String {
    let scanning = state.clone();
    tokio::task::spawn_blocking(move || store_snapshot(&scanning))
        .await
        .unwrap_or_else(|_| "{}".to_string())
}

/// Un fichier de la bibliothèque existant, ou `None`.
fn file_in(root: &Path, rel: &str) -> Option<PathBuf> {
    let path = safe_join(root, rel)?;
    path.is_file().then_some(path)
}

/// Construit un chemin sous `root`, en rejetant toute traversée.
///
/// Deux contrôles : **syntaxique** (pas d'absolu, pas de `..`, pas de préfixe
/// Windows), puis **réel** — le chemin existant est canonicalisé et doit rester
/// sous la racine canonicalisée. Le second attrape ce que le premier ne voit
/// pas : un lien symbolique posé dans la bibliothèque.
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.is_absolute() {
        return None;
    }
    if rel.components().any(|c| {
        matches!(
            c,
            Component::CurDir | Component::ParentDir | Component::Prefix(_)
        )
    }) {
        return None;
    }

    let joined = root.join(rel);
    let root = root.canonicalize().ok()?;

    // On remonte jusqu'au premier composant qui **existe** : la destination
    // peut ne pas exister, et un chemin qui n'existe pas n'est pas un lien.
    let mut current = joined.as_path();
    loop {
        if let Ok(real) = current.canonicalize() {
            return real.starts_with(&root).then_some(joined);
        }
        current = current.parent()?;
    }
}

/// Chemin d'une ressource **dans l'archive** : ni absolu, ni `..`.
fn clean_inner(raw: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            segment => parts.push(segment),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Origine publique de l'API, telle que la voit le client.
fn origin(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1:8091");
    format!("{scheme}://{host}")
}

/// Erreur interne (E/S, sérialisation) → 500 avec le message.
fn internal_error(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_traversal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ok.epub"), b"x").unwrap();

        assert!(safe_join(dir.path(), "ok.epub").is_some());
        assert!(safe_join(dir.path(), "../secret").is_none());
        assert!(safe_join(dir.path(), "/etc/passwd").is_none());
        assert!(safe_join(dir.path(), "a/../../secret").is_none());
    }

    #[test]
    fn cleans_archive_paths() {
        assert_eq!(
            clean_inner("OEBPS/Text/a.xhtml").as_deref(),
            Some("OEBPS/Text/a.xhtml")
        );
        assert_eq!(clean_inner("./a.css").as_deref(), Some("a.css"));
        assert_eq!(clean_inner("../secret"), None);
        assert_eq!(clean_inner(""), None);
    }
}
