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
//! | `GET /api/admin/performance/history?range=` | historique agrégé (3m, 1h, 24h, 7d, 30d) |
//! | `GET /api/admin/storage` | espace disque : bibliothèque, couvertures, bases, code |
//! | `GET /api/admin/library/stats` | chiffres de la bibliothèque : formats, dossiers, gros livres |
//! | `GET /api/chapter?path=&index=` | document d'un chapitre (HTML réécrit) |
//! | `GET /api/asset?path=&inner=` | ressource interne (image, CSS, police) |
//! | `GET /api/page?path=&index=` | page d'une bande dessinée (image) |
//! | `GET /api/raw?path=` | fichier tel quel (PDF, EPUB) |
//! | `POST /api/folder` | crée une catégorie (ou une série dans un dossier) |
//! | `PUT /api/book?folder=&name=` | dépose un livre (corps = le fichier) |
//! | `GET /ws` | avis `{"etag"}` à chaque changement du dossier (recharger `/api/books`) |

use crate::auth::{self, AuthState};
use crate::formats::{self, Format};
use crate::{
    config, library, perf_history, progress, readings, resize, rewrite, storage, thumbnail, write,
};
use axum::body::Bytes;
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
/// un parcours complet du dossier à chaque fois. Le seul travail de fond est le
/// contrôle de cohérence de `main` (voir `SWEEP_INTERVAL`), qui ne relance un
/// scan que si le dossier a réellement changé.
const SNAPSHOT_TTL: Duration = Duration::from_secs(2);

/// Battement de cœur du WebSocket : un `Ping` à cet intervalle, et la connexion est
/// coupée si plus rien n'est revenu depuis `WS_SILENCE`.
///
/// La présence (« en ligne ») ne vaut que si la socket vit encore : sans ce
/// battement, un onglet gelé par le navigateur ou un réseau coupé laisse la session
/// faussement présente, et un `Ping` non envoyé ne permet pas non plus de la
/// réveiller. Le navigateur répond `Pong` tout seul, même quand son JavaScript
/// dort.
const WS_HEARTBEAT: Duration = Duration::from_secs(25);
const WS_SILENCE: Duration = Duration::from_secs(75);

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
    /// Résumé de la bibliothèque, calculé une fois par scan.
    library: storage::Library,
    /// Chiffres de la bibliothèque pour la page des statistiques.
    library_detail: storage::LibraryDetail,
}

impl Snapshot {
    fn new(json: String) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        json.hash(&mut hasher);
        let tag = format!("{:016x}-{:x}", hasher.finish(), json.len());
        // Un seul parcours du catalogue sert les deux vues (résumé + détail).
        let library_detail = storage::library_detail(&json);
        Self {
            library: library_detail.summary(),
            library_detail,
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
    /// Dossier des bases (`accounts.db`, `metrics.db`).
    data_dir: Arc<PathBuf>,
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
    /// Historique des performances ; `None` : enregistrement désactivé.
    history: Option<Arc<perf_history::History>>,
    /// Historique de lecture ; `None` : pas d'identité (authentification
    /// désactivée) ou base indisponible.
    progress: Option<Arc<progress::Progress>>,
    /// Nombre de rediffusions du catalogue (watcher ou contrôle périodique).
    /// Le contrôle périodique de `main` s'en sert pour ne pas rediffuser un
    /// changement que le watcher a déjà traité entre deux de ses passages.
    broadcasts: Arc<std::sync::atomic::AtomicU64>,
}

impl AppState {
    pub fn new(root: impl Into<PathBuf>, ws: broadcast::Sender<Utf8Bytes>) -> Self {
        let root = root.into();
        let data_dir = root.join(".easybook");
        Self {
            cache_dir: Arc::new(root.join(thumbnail::DEFAULT_CACHE_DIR)),
            data_dir: Arc::new(data_dir),
            root: Arc::new(root),
            ws,
            snapshot: Arc::new(RwLock::new(None)),
            snapshot_at: Arc::new(std::sync::Mutex::new(None)),
            refresh: Arc::new(tokio::sync::Mutex::new(())),
            reads: Arc::new(Semaphore::new(config::limits().max_concurrent_reads)),
            resizes: Arc::new(Semaphore::new(config::resize().max_concurrent)),
            auth: None,
            history: None,
            broadcasts: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            progress: None,
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

    /// Attache l'historique des performances (voir [`crate::perf_history`]).
    #[must_use]
    pub fn with_history(mut self, history: Arc<perf_history::History>) -> Self {
        self.history = Some(history);
        self
    }

    pub fn history(&self) -> Option<Arc<perf_history::History>> {
        self.history.clone()
    }

    /// Attache l'historique de lecture (voir [`crate::progress`]).
    #[must_use]
    pub fn with_progress(mut self, progress: Arc<progress::Progress>) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn progress(&self) -> Option<Arc<progress::Progress>> {
        self.progress.clone()
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

    /// Nombre de WebSocket ouverts, c'est-à-dire de clients à prévenir d'un
    /// changement de la bibliothèque.
    pub fn listeners(&self) -> usize {
        self.ws.receiver_count()
    }

    /// Nombre de rediffusions du catalogue depuis le démarrage.
    ///
    /// Une empreinte changée accompagnée d'une rediffusion plus récente a déjà
    /// été poussée par le watcher : le contrôle périodique n'a rien à refaire.
    pub fn broadcasts(&self) -> u64 {
        self.broadcasts.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Dossier du cache disque des couvertures.
    pub fn cache_dir(&self) -> PathBuf {
        self.cache_dir.as_ref().clone()
    }

    /// Dossier des bases, quand `EASYBOOK_DATA_DIR` est monté.
    #[must_use]
    pub fn with_data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Arc::new(dir.into());
        self
    }

    /// Dossier des bases (`accounts.db`, `metrics.db`).
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.as_ref().clone()
    }

    /// Résumé de la bibliothèque, calculé lors du dernier scan du catalogue.
    pub fn library_stats(&self) -> storage::Library {
        self.snapshot
            .read()
            .ok()
            .and_then(|slot| slot.as_ref().map(|snapshot| snapshot.library.clone()))
            .unwrap_or_default()
    }

    /// Chiffres complets de la bibliothèque (page des statistiques).
    pub fn library_detail(&self) -> storage::LibraryDetail {
        self.snapshot
            .read()
            .ok()
            .and_then(|slot| {
                slot.as_ref()
                    .map(|snapshot| snapshot.library_detail.clone())
            })
            .unwrap_or_default()
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

    /// Ce que l'API seule connaît : catalogue en mémoire, sessions, onglets
    /// connectés et coût cumulé du garde de session (voir la page Performance).
    ///
    /// `pub` : l'échantillonneur d'historique (voir `main`) la lit pour
    /// que la mesure enregistrée soit la même que celle affichée en direct.
    pub fn detail(&self) -> crate::perf::Detail {
        let catalog_bytes = self
            .snapshot
            .read()
            .ok()
            .and_then(|slot| slot.as_ref().map(|snapshot| snapshot.text.len() as u64))
            .unwrap_or(0);
        let mut detail = crate::perf::Detail {
            catalog_bytes,
            ..Default::default()
        };
        if let Some(auth) = self.auth.as_ref() {
            let (calls, usec) = auth.guard_stats();
            detail.sessions = auth.accounts.session_count().unwrap_or(0).max(0) as u64;
            detail.sockets = auth.online_total() as u64;
            detail.guard_calls = calls;
            detail.guard_usec = usec;
        }
        detail
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
        .route(
            "/api/admin/performance/history",
            get(get_performance_history),
        )
        .route("/api/admin/storage", get(get_storage))
        .route("/api/admin/library/stats", get(get_library_stats))
        .route("/api/chapter", get(get_chapter))
        .route("/api/asset", get(get_asset))
        .route("/api/page", get(get_page))
        .route("/api/raw", get(get_raw))
        // Historique de lecture : la position du lecteur, et ce qu'en voit un
        // administrateur. Le préfixe `/api/admin/` suffit à exiger le rôle.
        .route(
            "/api/read/progress",
            post(save_progress).delete(forget_progress),
        )
        .route("/api/read/history", get(get_my_reading))
        .route(
            "/api/read/pins",
            get(get_pins).put(pin_item).delete(unpin_item),
        )
        .route(
            "/api/admin/users/{id}/read/history",
            get(get_reading_history),
        )
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
async fn get_performance(State(state): State<AppState>) -> Response {
    let detail = state.detail();
    match tokio::task::spawn_blocking(move || crate::perf::snapshot(detail)).await {
        Ok(snapshot) => ([(header::CACHE_CONTROL, "no-store")], Json(snapshot)).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct PerformanceHistoryQuery {
    range: Option<String>,
}

/// `GET /api/admin/performance/history?range=3m|1h|24h|7d|30d` — historique
/// agrégé. Le serveur choisit la tranche pour rester sous ~800 points ; une
/// même fenêtre est réutilisée 15 s (l'échantillonneur écrit toutes les 10 s).
async fn get_performance_history(
    State(state): State<AppState>,
    Query(query): Query<PerformanceHistoryQuery>,
) -> Response {
    let Some(history) = state.history() else {
        return (StatusCode::NOT_FOUND, "historique désactivé").into_response();
    };
    let range = query
        .range
        .as_deref()
        .and_then(perf_history::Range::parse)
        .unwrap_or(perf_history::Range::Day);
    match tokio::task::spawn_blocking(move || history.series_json(range)).await {
        Ok(Ok(json)) => (
            [
                (header::CACHE_CONTROL, "no-store"),
                (header::CONTENT_TYPE, "application/json"),
            ],
            json,
        )
            .into_response(),
        Ok(Err(e)) => internal_error(e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

/// `GET /api/admin/storage` — espace disque : bibliothèque, couvertures,
/// comptages de pages, bases et code. Lecture à la demande (l'onglet Stockage).
async fn get_storage(State(state): State<AppState>) -> Response {
    let data_dir = state.data_dir();
    let cache_dir = state.cache_dir();
    let library = state.library_stats();
    match tokio::task::spawn_blocking(move || storage::collect(&data_dir, &cache_dir, library))
        .await
    {
        Ok(storage) => ([(header::CACHE_CONTROL, "no-store")], Json(storage)).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

/// `GET /api/admin/library/stats` — chiffres de la bibliothèque : totaux,
/// répartition par format et par dossier, plus gros livres.
///
/// Tout vient du catalogue déjà en mémoire : aucun parcours de dossier.
async fn get_library_stats(State(state): State<AppState>) -> Response {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(state.library_detail()),
    )
        .into_response()
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
    state
        .broadcasts
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    note: Option<formats::Problem>,
    /// Position du lecteur connecté : c'est elle qui fait **reprendre** la
    /// lecture, y compris depuis un autre appareil.
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<Position>,
}

/// La position enregistrée d'un utilisateur dans un livre.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub page: i64,
    pub pages: i64,
}

#[derive(Deserialize)]
struct BookQuery {
    path: String,
}

/// `GET /api/book?path=` — sommaire ou nombre de pages.
async fn get_book(
    State(state): State<AppState>,
    auth::MaybeUser(user): auth::MaybeUser,
    Query(query): Query<BookQuery>,
) -> Response {
    let Some(path) = file_in(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let rel = query.path.clone();
    match state.read(move || detail(&path, &rel)).await {
        Ok(Ok(mut detail)) => {
            // La position du lecteur : c'est elle qui fait reprendre la lecture.
            if let (Some(progress), Some(user)) = (state.progress(), user.as_ref()) {
                let book = query.path.clone();
                let id = user.id.clone();
                let position = tokio::task::spawn_blocking(move || progress.position(&id, &book))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten();
                detail.progress = position.map(|row| Position {
                    page: row.page,
                    pages: row.pages,
                });
            }
            // Réponse propre à un lecteur : jamais de cache intermédiaire.
            ([(header::CACHE_CONTROL, "no-store")], Json(detail)).into_response()
        }
        // Le corps est un diagnostic **traduisible** : l'interface en choisit la
        // langue (voir `formats::Problem`).
        Ok(Err(problem)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(problem)).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

fn detail(path: &Path, rel: &str) -> Result<BookDetail, formats::Problem> {
    let format =
        formats::resolve(path, rel).ok_or_else(|| formats::Problem::new("api.unknown-format"))?;
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
        note: module.problem(),
        progress: None,
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
            None => Err(formats::Problem::new("api.unknown-format")),
        })
        .await
    {
        Ok(Ok(html)) => {
            readings::touch(&book, &query.path, html.len() as u64);
            html_response(html)
        }
        Ok(Err(problem)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(problem)).into_response(),
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

    let format = formats::resolve(&path, &query.path);
    let book = path.clone();

    match state
        .read(move || match format {
            Some(format) => formats::module(format).page(&path, index),
            None => Err(formats::Problem::new("api.unknown-format")),
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
        Ok(Err(problem)) => (StatusCode::NOT_FOUND, Json(problem)).into_response(),
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

    let content_type = formats::resolve(&path, &query.path)
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

    // Le premier `Ping` part après un délai plein : la connexion vient d'être
    // établie, inutile de la sonder tout de suite.
    let later = tokio::time::Instant::now() + WS_HEARTBEAT;
    let mut beat = tokio::time::interval_at(later, WS_HEARTBEAT);
    let mut heard = tokio::time::Instant::now();

    loop {
        tokio::select! {
            // Côté client : seule la fermeture nous intéresse (un `Pong` n'est
            // qu'un signe de vie, il ne réarme que le battement).
            message = stream.next() => match message {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => heard = tokio::time::Instant::now(),
                Some(Err(_)) => break,
            },
            // Plus personne au bout du fil (onglet tué sans fermeture propre,
            // proxy qui a coupé, machine en veille) : on rend la présence exacte
            // en raccrochant nous-mêmes.
            _ = beat.tick() => {
                if heard.elapsed() > WS_SILENCE {
                    break;
                }
                if sink.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
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

/// Origine publique de l'API, telle que le voit le client.
///
/// Derrière le nginx de l'image, `Host` vaut `$host`, qui **perd le port** :
/// les URL réécrites des chapitres EPUB pointeraient alors vers
/// `http://localhost/...` (port 80) au lieu de `http://localhost:5200/...`.
/// `X-Forwarded-Host` (posé par nginx avec `$http_host`) garde l'adresse
/// demandée, port compris ; il prime donc sur `Host` quand il est présent.
fn origin(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            headers
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("127.0.0.1:8091");
    format!("{scheme}://{host}")
}

/// Erreur interne (E/S, sérialisation) → 500 avec le message.
fn internal_error(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ── Historique de lecture ───────────────────────────────────────────────────

/// Corps de `POST /api/read/progress`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgressBody {
    /// Chemin du livre dans la bibliothèque (comme le catalogue).
    path: String,
    /// Index 0-based de la page (BD, PDF) ou du chapitre (EPUB).
    page: i64,
    /// Total connu du lecteur ; `0` s'il ne le sait pas encore.
    #[serde(default)]
    pages: i64,
}

/// `POST /api/read/progress` — enregistre la position du lecteur connecté.
///
/// Le livre doit exister : un chemin inventé n'entre pas en base. Sans identité
/// (authentification désactivée), l'extracteur répond 401 — il n'y a personne à
/// qui rattacher une position, et rien n'est écrit.
async fn save_progress(
    State(state): State<AppState>,
    auth::SessionUser(user): auth::SessionUser,
    Json(body): Json<ProgressBody>,
) -> Response {
    let Some(progress) = state.progress() else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let Some(path) = file_in(&state.root(), &body.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !path.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }

    // Le titre et le format viennent du disque, pas du client : la même règle
    // que le catalogue, et rien à croire sur parole.
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let format = formats::resolve(&path, &body.path)
        .map_or_else(String::new, |format| format.id().to_string());
    let entry = progress::Entry {
        book: body.path,
        title: library::title_of(name),
        format,
        page: body.page,
        pages: body.pages,
    };
    let user_id = user.id.clone();
    let notify = user_id.clone();
    match tokio::task::spawn_blocking(move || progress.save(&user_id, &entry)).await {
        Ok(Ok(())) => {
            // Les administrateurs regardent peut-être la page d'historique.
            if let Some(auth) = state.auth() {
                auth.notify_reading(&notify);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(e)) => internal_error(e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct ProgressQuery {
    /// Chemin du livre dans la bibliothèque (comme le catalogue).
    path: String,
}

/// `DELETE /api/read/progress?path=` — oublie la position du livre connecté.
///
/// C'est « marquer comme non lu » : la position et les sessions du livre
/// partent, la couverture redevient vierge et le temps de lecture cesse de
/// compter ce livre.
async fn forget_progress(
    State(state): State<AppState>,
    auth::SessionUser(user): auth::SessionUser,
    Query(query): Query<ProgressQuery>,
) -> Response {
    let Some(progress) = state.progress() else {
        return StatusCode::NO_CONTENT.into_response();
    };
    // Un chemin inventé n'efface rien : le livre doit exister.
    if file_in(&state.root(), &query.path).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let user_id = user.id.clone();
    let notify = user_id.clone();
    let book = query.path;
    match tokio::task::spawn_blocking(move || progress.forget(&user_id, &book)).await {
        Ok(Ok(_)) => {
            // Les administrateurs regardent peut-être la page d'historique.
            if let Some(auth) = state.auth() {
                auth.notify_reading(&notify);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(e)) => internal_error(e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct PinQuery {
    /// Chemin du livre ou du dossier dans la bibliothèque.
    path: String,
}

/// Titre d'une épingle, tiré du disque : un livre a le titre que lui donne le
/// catalogue, un dossier son nom. `None` si le chemin n'existe pas.
fn pin_title(root: &Path, rel: &str) -> Option<String> {
    if let Some(path) = file_in(root, rel) {
        return Some(library::title_of(path.file_name()?.to_str()?));
    }
    // La racine entière n'est pas une série : on ne l'épingle pas.
    if rel.is_empty() {
        return None;
    }
    folder_in(root, rel)?;
    Some(rel.rsplit('/').next()?.to_string())
}

/// `GET /api/read/pins` — les livres et séries « à regarder plus tard ».
async fn get_pins(
    State(state): State<AppState>,
    auth::SessionUser(user): auth::SessionUser,
) -> Response {
    let Some(progress) = state.progress() else {
        return (StatusCode::NOT_FOUND, "historique désactivé").into_response();
    };
    let id = user.id.clone();
    match tokio::task::spawn_blocking(move || progress.pins(&id)).await {
        Ok(Ok(pins)) => ([(header::CACHE_CONTROL, "no-store")], Json(pins)).into_response(),
        Ok(Err(e)) => internal_error(e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

/// `PUT /api/read/pins?path=` — épingle un livre ou une série.
async fn pin_item(
    State(state): State<AppState>,
    auth::SessionUser(user): auth::SessionUser,
    Query(query): Query<PinQuery>,
) -> Response {
    let Some(progress) = state.progress() else {
        return StatusCode::NO_CONTENT.into_response();
    };
    // Ce qui n'existe pas ne s'épingle pas — et le titre vient du disque.
    let Some(title) = pin_title(&state.root(), &query.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let id = user.id.clone();
    let item = query.path;
    match tokio::task::spawn_blocking(move || progress.pin(&id, &item, &title)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(e)) => internal_error(e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

/// `DELETE /api/read/pins?path=` — détache une épingle.
async fn unpin_item(
    State(state): State<AppState>,
    auth::SessionUser(user): auth::SessionUser,
    Query(query): Query<PinQuery>,
) -> Response {
    let Some(progress) = state.progress() else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let id = user.id.clone();
    let item = query.path;
    match tokio::task::spawn_blocking(move || progress.unpin(&id, &item)).await {
        Ok(Ok(_)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(e)) => internal_error(e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

#[derive(Deserialize)]
struct ReadingHistoryQuery {
    limit: Option<usize>,
    /// Début de la période, en secondes Unix (`0` ou absent : depuis toujours).
    since: Option<i64>,
}

/// `GET /api/read/history?limit=&since=` — les lectures du compte connecté.
///
/// C'est ce que l'accueil affiche (« poursuivre la lecture ») et ce qui donne
/// leur barre de progression aux couvertures. `limit=0` rend **tout**
/// l'historique : la page du compte y calcule ses chiffres (séries terminées),
/// qui doivent porter sur toutes les lectures, pas sur les plus récentes.
/// `since` borne la fenêtre (les KPI du mois, de l'année).
async fn get_my_reading(
    State(state): State<AppState>,
    auth::SessionUser(user): auth::SessionUser,
    Query(query): Query<ReadingHistoryQuery>,
) -> Response {
    let Some(progress) = state.progress() else {
        return (StatusCode::NOT_FOUND, "historique désactivé").into_response();
    };
    let window = progress::Window {
        since: query.since.unwrap_or(0).max(0),
        limit: query.limit.unwrap_or(30),
    };
    let id = user.id.clone();
    match tokio::task::spawn_blocking(move || progress.history(&id, window)).await {
        Ok(Ok(history)) => ([(header::CACHE_CONTROL, "no-store")], Json(history)).into_response(),
        Ok(Err(e)) => internal_error(e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
}

/// `GET /api/admin/users/{id}/read/history?limit=` — l'historique d'un compte.
async fn get_reading_history(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<ReadingHistoryQuery>,
) -> Response {
    let Some(progress) = state.progress() else {
        return (StatusCode::NOT_FOUND, "historique désactivé").into_response();
    };
    // Un identifiant inconnu est une erreur, pas un historique vide.
    if let Some(auth) = state.auth() {
        let accounts = Arc::clone(&auth.accounts);
        let lookup = id.clone();
        let known = tokio::task::spawn_blocking(move || accounts.user(&lookup)).await;
        match known {
            Ok(Ok(Some(_))) => {}
            Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
            Ok(Err(e)) => return internal_error(e).into_response(),
            Err(e) => return internal_error(e).into_response(),
        }
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let window = progress::Window {
        since: query.since.unwrap_or(0).max(0),
        limit,
    };
    let user = id.clone();
    match tokio::task::spawn_blocking(move || progress.history(&user, window)).await {
        Ok(Ok(history)) => ([(header::CACHE_CONTROL, "no-store")], Json(history)).into_response(),
        Ok(Err(e)) => internal_error(e).into_response(),
        Err(e) => internal_error(e).into_response(),
    }
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

    #[test]
    fn origin_keeps_the_forwarded_port() {
        // nginx pose `Host: $host` (sans port) mais `X-Forwarded-Host: $http_host`.
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "localhost".parse().unwrap());
        headers.insert("x-forwarded-host", "localhost:5200".parse().unwrap());
        assert_eq!(origin(&headers), "http://localhost:5200");

        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(origin(&headers), "https://localhost:5200");
    }

    #[test]
    fn origin_falls_back_to_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:8091".parse().unwrap());
        assert_eq!(origin(&headers), "http://127.0.0.1:8091");
    }
}
