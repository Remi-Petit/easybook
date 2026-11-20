//! Serveur HTTP et temps réel.
//!
//! Tout `/api` et `/ws` exige une session (voir [`crate::auth`]), sauf si
//! `auth.enabled` est à `false` dans `config.yml` : l'API est alors ouverte et
//! n'écoute que sur la boucle locale par défaut (`EASYBOOK_HOST=0.0.0.0` dans le
//! conteneur).
//!
//! Les routes :
//!
//! | Route | Rôle |
//! |---|---|
//! | `GET /api/health` | état du serveur |
//! | `/api/auth/…`, `/api/admin/users…` | comptes et connexion (voir [`crate::auth`]) |
//! | `GET /api/books` | catalogue complet (JSON) |
//! | `GET /api/book?path=` | sommaire d'un livre |
//! | `GET /api/cover?path=&v=` | couverture (vignette JPEG) |
//! | `GET /api/admin/performance` | mémoire et CPU du conteneur, par process |
//! | `GET /api/chapter?path=&index=` | document d'un chapitre (HTML réécrit) |
//! | `GET /api/asset?path=&inner=` | ressource interne (image, CSS, police) |
//! | `GET /api/page?path=&index=` | page d'une bande dessinée (image) |
//! | `GET /api/raw?path=` | fichier tel quel (PDF, EPUB) |
//! | `POST /api/folder` | crée une catégorie (ou une série dans un dossier) |
//! | `PUT /api/book?folder=&name=` | dépose un livre (corps = le fichier) |
//! | `GET /ws` | avis `{"etag"}` à chaque changement du dossier (recharger `/api/books`) |

use crate::auth::{self, AuthState};
use crate::formats::{self, Format};
use crate::{config, library, readings, resize, rewrite, thumbnail, write};
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, broadcast};
use tower::ServiceExt;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeFile;

/// Durée de validité du catalogue en mémoire.
///
/// Le watcher rediffuse à chaque changement ; ce délai ne sert qu'à absorber
/// une rafale de requêtes (chargement de page, plusieurs onglets) sans refaire
/// un parcours complet du dossier à chaque fois. Ce n'est pas un sondage : rien
/// ne tourne en tâche de fond.
const SNAPSHOT_TTL: Duration = Duration::from_secs(2);

/// Catalogue sérialisé, partagé sans copie entre les requêtes HTTP.
///
/// Les WebSocket ne reçoivent **pas** le catalogue : seulement un avis très court
/// (`notice`) portant son `ETag`. Pousser le catalogue à chaque onglet coûtait
/// plus de 1,8 Mo de RSS par socket pour 6000 livres (tampon d'envoi de chaque
/// connexion) ; le client le recharge par HTTP, où nginx le compresse et où un
/// 304 lui évite tout transfert.
#[derive(Clone)]
struct Snapshot {
    text: Utf8Bytes,
    etag: Arc<str>,
    notice: Utf8Bytes,
}

impl Snapshot {
    fn new(json: String) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        json.hash(&mut hasher);
        let tag = format!("{:016x}-{:x}", hasher.finish(), json.len());
        Self {
            text: Utf8Bytes::from(json),
            notice: Utf8Bytes::from(format!(r#"{{"etag":"{tag}"}}"#)),
            etag: format!("\"{tag}\"").into(),
        }
    }
}

/// État partagé par les handlers.
#[derive(Clone)]
pub struct AppState {
    /// Racine de la bibliothèque.
    root: Arc<PathBuf>,
    /// Dossier du cache disque des couvertures (caché, sous la racine par défaut).
    cache_dir: Arc<PathBuf>,
    /// Diffusion du catalogue aux WebSocket ouverts.
    ws: broadcast::Sender<Utf8Bytes>,
    /// Dernier catalogue sérialisé, et son instant de production.
    snapshot: Arc<RwLock<Option<Snapshot>>>,
    snapshot_at: Arc<std::sync::Mutex<Option<Instant>>>,
    /// Un seul scan à la fois, même si plusieurs requêtes arrivent à froid.
    refresh: Arc<tokio::sync::Mutex<()>>,
    /// Lectures de livre en cours (`limits.max_concurrent_reads`).
    reads: Arc<Semaphore>,
    /// Réductions de planches en cours (`resize.max_concurrent`).
    resizes: Arc<Semaphore>,
    /// Comptes et sessions ; `None` : l'API reste ouverte (voir [`crate::auth`]).
    auth: Option<Arc<AuthState>>,
}

impl AppState {
    pub fn new(root: impl Into<PathBuf>, ws: broadcast::Sender<Utf8Bytes>) -> Self {
        let root = root.into();
        Self {
            cache_dir: Arc::new(root.join(thumbnail::DEFAULT_CACHE_DIR)),
            root: Arc::new(root),
            ws,
            snapshot: Arc::new(RwLock::new(None)),
            snapshot_at: Arc::new(std::sync::Mutex::new(None)),
            refresh: Arc::new(tokio::sync::Mutex::new(())),
            reads: Arc::new(Semaphore::new(config::limits().max_concurrent_reads)),
            resizes: Arc::new(Semaphore::new(config::resize().max_concurrent)),
            auth: None,
        }
    }

    /// Exige une session pour tout `/api` et `/ws`.
    #[must_use]
    pub fn with_auth(mut self, auth: AuthState) -> Self {
        self.auth = Some(Arc::new(auth));
        self
    }

    pub fn auth(&self) -> Option<&Arc<AuthState>> {
        self.auth.as_ref()
    }

    /// Réduit une planche à `width` pixels, dans la limite de
    /// `resize.max_concurrent` : chaque réduction décode l'image entière.
    /// Rend le type de contenu et les octets à servir (l'original si la réduction
    /// n'a pas lieu d'être).
    async fn shrink(
        &self,
        name: String,
        bytes: Vec<u8>,
        width: u32,
        settings: config::Resize,
    ) -> Result<(&'static str, Vec<u8>), tokio::task::JoinError> {
        let original = formats::content_type(&name);
        let permit = Arc::clone(&self.resizes).acquire_owned().await.ok();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            match resize::shrink_page(&bytes, width, &settings) {
                Some(small) => ("image/jpeg", small),
                None => (original, bytes),
            }
        })
        .await
    }

    /// Exécute une lecture de livre sur le pool bloquant, dans la limite de
    /// `limits.max_concurrent_reads` : au-delà, l'appel attend son tour.
    ///
    /// Le permis suit le travail, pas la requête : un client qui se déconnecte
    /// n'arrête pas la lecture déjà partie, elle compte donc jusqu'à sa fin.
    async fn read<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, tokio::task::JoinError> {
        // Le sémaphore n'est jamais fermé : l'erreur ne peut pas arriver.
        let permit = Arc::clone(&self.reads).acquire_owned().await.ok();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
    }

    /// Racine courante de la bibliothèque.
    pub fn root(&self) -> PathBuf {
        self.root.as_ref().clone()
    }

    /// Dossier du cache disque des couvertures.
    pub fn cache_dir(&self) -> PathBuf {
        self.cache_dir.as_ref().clone()
    }

    /// Place le cache disque des couvertures ailleurs que sous la racine.
    #[must_use]
    pub fn with_cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Arc::new(dir.into());
        self
    }

    /// Catalogue encore frais, s'il y en a un.
    fn cached(&self) -> Option<Snapshot> {
        let fresh = self
            .snapshot_at
            .lock()
            .ok()
            .and_then(|at| *at)
            .is_some_and(|at| at.elapsed() < SNAPSHOT_TTL);
        if !fresh {
            return None;
        }
        self.snapshot.read().ok()?.clone()
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
        .route("/api/admin/performance", get(get_performance))
        .route("/api/chapter", get(get_chapter))
        .route("/api/asset", get(get_asset))
        .route("/api/page", get(get_page))
        .route("/api/raw", get(get_raw))
        // Écriture : ce que le catalogue affiche, il doit pouvoir le créer.
        .route("/api/folder", post(create_folder))
        .route("/api/book", put(upload_book))
        .route("/ws", get(ws_library))
        .merge(auth::routes())
        // Sous le CORS : le préambule d'une requête inter-origines ne porte pas de
        // cookie et ne doit pas être refusé.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::guard,
        ))
        // L'interface est servie par un autre process : elle appelle l'API en
        // direct, sans relais (les images et les polices d'un EPUB comprises).
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

/// `GET /api/admin/performance` — mesure instantanée de la mémoire et du CPU.
///
/// L'interface l'interroge à intervalle régulier et calcule les taux CPU à
/// partir de deux mesures successives (les compteurs sont cumulés).
async fn get_performance() -> Response {
    match tokio::task::spawn_blocking(crate::perf::snapshot).await {
        Ok(snapshot) => ([(header::CACHE_CONTROL, "no-store")], Json(snapshot)).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

/// `GET /api/books` — catalogue complet.
///
/// `ETag` : un client dont le catalogue est à jour reçoit un 304, sans corps.
async fn list_books(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(snapshot) = state.cached() {
        return snapshot_response(snapshot, &headers);
    }

    // Le scan est bloquant (un `stat` par fichier) : il part sur le pool prévu
    // pour ça, sans immobiliser un worker de Tokio.
    let _guard = state.refresh.lock().await;
    if let Some(snapshot) = state.cached() {
        return snapshot_response(snapshot, &headers);
    }

    let scanning = state.clone();
    match tokio::task::spawn_blocking(move || store_snapshot(&scanning)).await {
        Ok(snapshot) => snapshot_response(snapshot, &headers),
        Err(e) => internal_error(e).into_response(),
    }
}

fn snapshot_response(snapshot: Snapshot, request: &HeaderMap) -> Response {
    let unchanged = request
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| etag_matches(value, &snapshot.etag));
    if unchanged {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, &*snapshot.etag),
                (header::CACHE_CONTROL, "no-cache"),
            ],
        )
            .into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::ETAG, &*snapshot.etag),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        axum::body::Bytes::from(snapshot.text),
    )
        .into_response()
}

/// Scanne la bibliothèque, mémorise le résultat et le rend.
fn store_snapshot(state: &AppState) -> Snapshot {
    let scan = library::scan(&state.root());
    let snapshot = Snapshot::new(serde_json::to_string(&scan).unwrap_or_else(|_| "{}".to_string()));
    if let Ok(mut slot) = state.snapshot.write() {
        *slot = Some(snapshot.clone());
    }
    if let Ok(mut at) = state.snapshot_at.lock() {
        *at = Some(Instant::now());
    }
    snapshot
}

/// Scanne et prévient les clients WebSocket (appelé par le watcher).
pub fn broadcast_snapshot(state: &AppState) {
    let snapshot = store_snapshot(state);
    // Aucun client à l'écoute n'est pas une erreur : le catalogue reste en
    // cache pour la prochaine requête HTTP.
    let _ = state.ws.send(snapshot.notice);
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
    /// Date de modification du fichier (secondes Unix) : version des pages dans
    /// leur URL, qui peuvent alors être gardées indéfiniment.
    #[serde(skip_serializing_if = "Option::is_none")]
    modified: Option<u64>,
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
    match state.read(move || detail(&path, &rel)).await {
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
    // Le comptage est gardé par fichier (`pages`) : rouvrir une grosse archive
    // pour la trier à chaque fiche coûterait des millisecondes de CPU. Sans
    // comptage, on retente en direct pour obtenir l'erreur.
    let pages = match crate::pages::count(path, format) {
        Some(pages) => Some(pages),
        None => module.pages(path)?,
    };

    Ok(BookDetail {
        rel: rel.to_string(),
        format: format.id(),
        supported: module.readable(),
        title: outline.as_ref().and_then(|outline| outline.title.clone()),
        author: outline.as_ref().and_then(|outline| outline.author.clone()),
        chapters: outline
            .and_then(|outline| (!outline.chapters.is_empty()).then_some(outline.chapters)),
        pages,
        modified: file_stamp(path).map(|(_, secs)| secs),
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
    let cache_dir = state.cache_dir.as_ref().clone();
    // Clé du cache mémoire : la date du fichier, plus la révision demandée par
    // l'interface (une extraction qui change doit produire d'autres images).
    let cache_key = match &query.r {
        Some(revision) if !revision.is_empty() => format!("{version}|{revision}"),
        _ => version.to_string(),
    };
    let cover = tokio::task::spawn_blocking(move || {
        thumbnail::cover(&path, &rel, &cache_key, Some(&cache_dir))
    })
    .await;

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
    let book = path.clone();

    match state
        .read(move || match format {
            Some(format) => formats::module(format).chapter(&path, index, &rel, &origin),
            None => Err("format non reconnu".to_string()),
        })
        .await
    {
        Ok(Ok(html)) => {
            readings::touch(&book, &query.path, html.len() as u64);
            html_response(html)
        }
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
    let book = path.clone();

    let bytes = state
        .read(move || format.and_then(|format| formats::module(format).asset(&path, &entry)))
        .await;

    match bytes {
        Ok(Some(bytes)) => {
            readings::touch(&book, &query.path, bytes.len() as u64);
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
    /// Date de modification annoncée par le catalogue (voir `CoverQuery::v`).
    #[serde(default)]
    v: Option<String>,
    /// Largeur voulue, en pixels (voir [`crate::resize`]) : la planche est servie
    /// réduite à la plus petite largeur proposée qui la couvre.
    #[serde(default)]
    w: Option<u32>,
}

/// Taille et date de modification (secondes Unix) d'un fichier.
fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let secs = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((meta.len(), secs))
}

/// `true` si l'en-tête `If-None-Match` désigne cet `ETag` (liste, `*`, préfixe faible).
fn etag_matches(header: &str, etag: &str) -> bool {
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    })
}

/// `GET /api/page?path=&index=&v=&w=` — page d'une bande dessinée.
///
/// Avec la date du fichier dans l'URL (`v`), la page ne changera plus à cette
/// adresse : elle est servie `immutable`. Sans elle, ou si la date est périmée,
/// le navigateur la revalide par `ETag` sans que l'archive soit rouverte.
///
/// `w` demande une planche réduite (voir [`crate::resize`]) : sans lui, ou si la
/// planche est déjà assez étroite, c'est l'image d'origine, octet pour octet.
async fn get_page(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let index = query.index;
    let settings = config::resize();
    let width = query
        .w
        .and_then(|wanted| resize::snap(wanted, &settings.widths));

    let stamp = file_stamp(&path);
    let announced = stamp.map(|(_, secs)| secs.to_string());
    let cache = if announced.is_some() && query.v == announced {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    let etag = stamp
        .map(|(size, secs)| format!("\"{size:x}-{secs:x}-{index:x}-{:x}\"", width.unwrap_or(0)));
    if let Some(etag) = &etag
        && headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| etag_matches(value, etag))
    {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag.as_str()),
                (header::CACHE_CONTROL, cache),
            ],
        )
            .into_response();
    }

    let format = Format::from_path(&query.path);
    let book = path.clone();

    match state
        .read(move || match format {
            Some(format) => formats::module(format).page(&path, index),
            None => Err("format non reconnu".to_string()),
        })
        .await
    {
        Ok(Ok((name, bytes))) => {
            let (content_type, bytes) = match width {
                Some(width) => match state.shrink(name, bytes, width, settings).await {
                    Ok(shrunk) => shrunk,
                    Err(e) => return internal_error(e).into_response(),
                },
                None => (formats::content_type(&name), bytes),
            };
            readings::touch(&book, &query.path, bytes.len() as u64);
            let mut response = (
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, cache),
                ],
                bytes,
            )
                .into_response();
            if let Some(etag) = etag.and_then(|etag| etag.parse().ok()) {
                response.headers_mut().insert(header::ETAG, etag);
            }
            response
        }
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
/// Sert au lecteur PDF de l'interface (pdf.js) : il lui faut le document, pas une version
/// découpée. Le nom de fichier est proposé au téléchargement (`inline`, donc le
/// lecteur s'ouvre).
///
/// Le fichier est **servi en flux** (`ServeFile`) : la mémoire reste constante
/// quelle que soit sa taille, et les requêtes `Range` sont gérées (pdf.js ne
/// télécharge que les pages qu'il affiche).
async fn get_raw(
    State(state): State<AppState>,
    Query(query): Query<RawQuery>,
    request: Request,
) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let Ok(served) = ServeFile::new(&path).oneshot(request).await;
    let mut response = served.into_response();
    if !response.status().is_success() {
        return response;
    }
    let served = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    readings::touch(&path, &query.path, served);

    let content_type = Format::from_path(&query.path)
        .map(Format::content_type)
        .unwrap_or("application/octet-stream");
    let name = query
        .path
        .rsplit('/')
        .next()
        .unwrap_or("livre")
        .replace('"', "");

    let headers = response.headers_mut();
    if let Ok(value) = content_type.parse() {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = format!("inline; filename=\"{name}\"").parse() {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    headers.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    response
}

/// WebSocket : un avis (`{"etag":"…"}`) à la connexion, puis à chaque changement.
/// Le client recharge `/api/books` quand l'ETag annoncé n'est plus le sien.
async fn ws_library(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    auth::MaybeUser(user): auth::MaybeUser,
    auth::MaybeSession(session): auth::MaybeSession,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state, user, session))
}

async fn handle_ws(
    socket: WebSocket,
    state: AppState,
    user: Option<crate::accounts::User>,
    session: Option<String>,
) {
    let (mut sink, mut stream) = socket.split();

    // Une connexion ouverte vaut une présence ; un administrateur reçoit en plus
    // les avis d'activité des comptes (connexions, sessions, présence).
    let _presence = state
        .auth()
        .zip(user.as_ref())
        .zip(session.as_deref())
        .map(|((auth, user), session)| auth.enter(&user.id, session));
    // Une session fermée par un administrateur coupe ses onglets.
    let mut revoked = state.auth().map(|auth| auth.subscribe_revocations());
    let mut events = state
        .auth()
        .zip(user.as_ref())
        .filter(|(_, user)| user.role == crate::accounts::Role::Admin)
        .map(|(auth, _)| auth.subscribe_events());

    // Abonnement **avant** le premier envoi : un changement survenu pendant le
    // scan initial ne doit pas être perdu.
    let mut rx = state.ws.subscribe();

    // Un seul scan à la fois, comme pour `GET /api/books` : des centaines d'onglets
    // qui se connectent ensemble ne doivent pas lancer autant de scans.
    let initial = {
        let _guard = state.refresh.lock().await;
        match state.cached() {
            Some(snapshot) => snapshot,
            None => snapshot_now(&state).await,
        }
    };

    if sink.send(Message::Text(initial.notice)).await.is_err() {
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
            // La session de ce WebSocket vient d'être fermée.
            closed = async {
                match revoked.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => if let Ok(what) = closed {
                let (user_id, session_id) = (
                    user.as_ref().map_or("", |user| user.id.as_str()),
                    session.as_deref().unwrap_or(""),
                );
                if what.hits(user_id, session_id) {
                    break;
                }
            },
            // Activité d'un compte, réservée aux administrateurs.
            event = async {
                match events.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => match event {
                Ok(json) => {
                    if sink.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                // Des avis perdus : l'interface relit de toute façon l'état à chaque avis.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
            // Changement du dossier, poussé par le watcher.
            envelope = rx.recv() => match envelope {
                Ok(json) => {
                    if sink.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                // En retard sur le canal : un avis frais plutôt que de laisser le
                // client croire à un état périmé.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let fresh = snapshot_now(&state).await;
                    if sink.send(Message::Text(fresh.notice)).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            },
        }
    }
}

async fn snapshot_now(state: &AppState) -> Snapshot {
    let scanning = state.clone();
    tokio::task::spawn_blocking(move || store_snapshot(&scanning))
        .await
        .unwrap_or_else(|_| Snapshot::new("{}".to_string()))
}

/// Un fichier de la bibliothèque existant, ou `None`.
fn file_in(root: &Path, rel: &str) -> Option<PathBuf> {
    let path = safe_join(root, rel)?;
    path.is_file().then_some(path)
}

/// Un dossier de la bibliothèque existant, ou `None` (`""` = la racine).
///
/// Un segment commençant par `.` est refusé : le scan ignore les dossiers
/// cachés, donc écrire dedans créerait un livre que personne ne verrait.
fn folder_in(root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() {
        return root.is_dir().then(|| root.to_path_buf());
    }
    if rel.split('/').any(|segment| segment.starts_with('.')) {
        return None;
    }
    let path = safe_join(root, rel)?;
    path.is_dir().then_some(path)
}

#[derive(Deserialize)]
struct NewFolder {
    /// Dossier parent, relatif à `books/` (`""` pour la racine).
    #[serde(default)]
    parent: String,
    name: String,
}

/// `POST /api/folder` — crée une catégorie (`parent: ""`) ou une série.
///
/// Le dossier créé apparaît tout de suite : le catalogue est rediffusé sans
/// attendre l'événement du watcher (il arrive, mais après une demi-seconde de
/// debounce).
async fn create_folder(State(state): State<AppState>, Json(body): Json<NewFolder>) -> Response {
    let root = state.root();
    if folder_in(&root, &body.parent).is_none() {
        return (
            StatusCode::NOT_FOUND,
            format!("dossier introuvable : {}", body.parent),
        )
            .into_response();
    }
    let name = body.name;
    let rel_parent = body.parent;

    let created = tokio::task::spawn_blocking(move || {
        // Le parent est déjà résolu : `write::create_folder` refait la
        // validation du nom, pas celle du chemin.
        let rel = write::create_folder(&root, &rel_parent, &name)?;
        Ok::<String, write::WriteError>(rel)
    })
    .await;

    match created {
        Ok(Ok(rel)) => {
            refresh_snapshot(&state).await;
            (StatusCode::CREATED, Json(serde_json::json!({ "rel": rel }))).into_response()
        }
        Ok(Err(e)) => (e.status(), e.message().to_string()).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct UploadQuery {
    /// Dossier d'accueil, relatif à `books/` (`""` pour la racine).
    #[serde(default)]
    folder: String,
    /// Nom du fichier (le corps ne porte pas de nom).
    name: String,
}

/// `PUT /api/book?folder=&name=` — dépose un livre.
///
/// Le corps **est** le fichier : `fetch(url, { method: 'PUT', body: file })`
/// l'envoie en flux, sans encodage multipart ni copie en mémoire — une planche
/// de 400 Mo passe donc sans faire gonfler le serveur. L'écriture passe par un
/// fichier temporaire caché (voir [`crate::write`]) : interrompue, elle ne laisse
/// rien dans le catalogue.
async fn upload_book(
    State(state): State<AppState>,
    Query(query): Query<UploadQuery>,
    body: axum::body::Body,
) -> Response {
    let root = state.root();
    let (name, _format) = match write::book_name(&query.name) {
        Ok(valid) => valid,
        Err(e) => return (e.status(), e.message().to_string()).into_response(),
    };
    let Some(folder) = folder_in(&root, &query.folder) else {
        return (
            StatusCode::NOT_FOUND,
            format!("dossier introuvable : {}", query.folder),
        )
            .into_response();
    };

    let target = folder.join(&name);
    if target.exists() {
        return (
            StatusCode::CONFLICT,
            format!("« {name} » existe déjà dans ce dossier"),
        )
            .into_response();
    }

    let incoming = write::incoming_path(&target);
    let written = stream_to(&incoming, body).await;
    let result = match written {
        Ok(true) => tokio::task::spawn_blocking(move || write::commit_incoming(&incoming, &target))
            .await
            .unwrap_or_else(|e| Err(write::WriteError::Failed(e.to_string()))),
        // Fichier vide : le formulaire a envoyé autre chose qu'un livre.
        Ok(false) => {
            let _ = tokio::fs::remove_file(&incoming).await;
            Err(write::WriteError::Invalid(
                "fichier vide : rien n'a été déposé".to_string(),
            ))
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&incoming).await;
            Err(e)
        }
    };

    match result {
        Ok(()) => {
            refresh_snapshot(&state).await;
            let rel = if query.folder.is_empty() {
                name
            } else {
                format!("{}/{}", query.folder, name)
            };
            (StatusCode::CREATED, Json(serde_json::json!({ "rel": rel }))).into_response()
        }
        Err(e) => (e.status(), e.message().to_string()).into_response(),
    }
}

/// Écrit le corps de la requête dans `path` ; `false` s'il est vide.
async fn stream_to(path: &Path, body: axum::body::Body) -> Result<bool, write::WriteError> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|e| write::WriteError::Failed(format!("{} : {e}", path.display())))?;

    let mut received: u64 = 0;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| write::WriteError::Failed(format!("envoi interrompu : {e}")))?;
        received += chunk.len() as u64;
        file.write_all(&chunk)
            .await
            .map_err(|e| write::WriteError::Failed(format!("{} : {e}", path.display())))?;
    }
    file.flush()
        .await
        .map_err(|e| write::WriteError::Failed(format!("{} : {e}", path.display())))?;

    Ok(received > 0)
}

/// Recalcule le catalogue et le rediffuse tout de suite.
async fn refresh_snapshot(state: &AppState) {
    let state = state.clone();
    let _ = tokio::task::spawn_blocking(move || broadcast_snapshot(&state)).await;
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
