//! Configuration de l'application, lue dans `config.yml`.
//!
//! Source du fichier :
//! 1. variable d'environnement `EASYBOOK_CONFIG=<chemin>` ;
//! 2. sinon `config.yml` dans le dossier courant (lancé depuis `backend/`) ;
//! 3. sinon les valeurs par défaut.
//!
//! Le fichier n'est **pas** relu à chaud : seul le dossier des livres est
//! surveillé (voir `main.rs`). Changer de bibliothèque demande un redémarrage —
//! c'est rare, et ça évite de faire suivre un déplacement de racine au watcher.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Contenu de `config.yml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Dossier des livres. Relatif au dossier de `config.yml` s'il ne commence
    /// pas par `/`.
    pub books_root: String,
    /// Plafonds de ressources (voir [`Limits`]).
    pub limits: Limits,
    /// Réduction des planches à la taille de l'écran (voir [`Resize`]).
    pub resize: Resize,
    /// Historique de la page Performances (voir [`Performance`]).
    pub performance: Performance,
    /// Comptes et connexion (voir [`Auth`]).
    pub auth: Auth,
}

/// Comptes et connexion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Auth {
    /// `false` : aucune connexion demandée, l'API reste ouverte (ancien
    /// comportement).
    pub enabled: bool,
    /// Durée d'une session, en jours.
    pub session_days: u32,
    /// Connexion par un fournisseur OpenID Connect.
    pub oidc: Oidc,
}

impl Default for Auth {
    fn default() -> Self {
        Self {
            enabled: true,
            session_days: 30,
            oidc: Oidc::default(),
        }
    }
}

/// Fournisseur OpenID Connect (Keycloak, Authentik, Google…).
///
/// Actif dès que `issuer` et `client_id` sont renseignés. Chaque clé se pose
/// aussi par variable d'environnement (`EASYBOOK_OIDC_ISSUER`, `…_CLIENT_ID`,
/// `…_CLIENT_SECRET`, `…_DISCOVERY_URL`, `…_REDIRECT_URL`, `…_LABEL`,
/// `…_AUTO_CREATE`, `…_LINK_BY_EMAIL`, `…_SELF_LINK`) : elle gagne sur le fichier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Oidc {
    /// URL de l'émetteur, telle qu'elle figure dans le champ `iss` des jetons.
    pub issuer: String,
    /// Où lire `/.well-known/openid-configuration` quand le serveur ne joint pas
    /// l'émetteur à l'adresse publique (conteneurs) ; vide : l'émetteur.
    pub discovery_url: String,
    pub client_id: String,
    /// Vide pour un client public (PKCE seul).
    pub client_secret: String,
    /// Adresse de retour complète, à déclarer chez le fournisseur ; vide : déduite
    /// de l'adresse par laquelle le navigateur est arrivé.
    pub redirect_url: String,
    pub scopes: Vec<String>,
    /// Texte du bouton de connexion.
    pub label: String,
    /// Un compte OIDC inconnu crée un compte local (rôle `user`). Sinon il est
    /// refusé tant qu'un administrateur ne l'a pas lié à un compte.
    pub auto_create: bool,
    /// Un compte OIDC inconnu est lié au compte local de même e-mail, si le
    /// fournisseur certifie l'adresse (`email_verified`).
    pub link_by_email: bool,
    /// Chacun peut lier son propre compte à une identité OIDC depuis sa page
    /// « Mon compte ». Sinon, seul un administrateur lie.
    pub self_link: bool,
}

impl Default for Oidc {
    fn default() -> Self {
        Self {
            issuer: String::new(),
            discovery_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            redirect_url: String::new(),
            scopes: vec!["openid".into(), "profile".into(), "email".into()],
            label: "Single sign-on".into(),
            auto_create: false,
            link_by_email: false,
            self_link: false,
        }
    }
}

impl Oidc {
    pub fn is_enabled(&self) -> bool {
        !self.issuer.trim().is_empty() && !self.client_id.trim().is_empty()
    }

    /// Reprend les variables d'environnement ; `get` les lit (paramètre : testable).
    #[must_use]
    pub fn with_env(mut self, get: impl Fn(&str) -> Option<String>) -> Self {
        let text = |name: &str| get(name).filter(|value| !value.trim().is_empty());
        let flag = |name: &str| {
            text(name).map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        };
        if let Some(value) = text("EASYBOOK_OIDC_ISSUER") {
            self.issuer = value;
        }
        if let Some(value) = text("EASYBOOK_OIDC_DISCOVERY_URL") {
            self.discovery_url = value;
        }
        if let Some(value) = text("EASYBOOK_OIDC_CLIENT_ID") {
            self.client_id = value;
        }
        if let Some(value) = text("EASYBOOK_OIDC_CLIENT_SECRET") {
            self.client_secret = value;
        }
        if let Some(value) = text("EASYBOOK_OIDC_REDIRECT_URL") {
            self.redirect_url = value;
        }
        if let Some(value) = text("EASYBOOK_OIDC_LABEL") {
            self.label = value;
        }
        if let Some(value) = flag("EASYBOOK_OIDC_AUTO_CREATE") {
            self.auto_create = value;
        }
        if let Some(value) = flag("EASYBOOK_OIDC_LINK_BY_EMAIL") {
            self.link_by_email = value;
        }
        if let Some(value) = flag("EASYBOOK_OIDC_SELF_LINK") {
            self.self_link = value;
        }
        self
    }
}

/// Réduction des planches de bande dessinée (`GET /api/page?w=`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Resize {
    /// Largeurs proposées, en pixels : une demande est arrondie à la plus petite
    /// qui la couvre. Liste vide : aucune réduction.
    pub widths: Vec<u32>,
    /// Qualité JPEG des planches réduites (1 à 100).
    pub quality: u8,
    /// Réductions simultanées : chacune décode la planche entière en pixels.
    pub max_concurrent: usize,
}

impl Default for Resize {
    fn default() -> Self {
        Self {
            widths: vec![800, 1200, 1600, 2400],
            quality: 80,
            max_concurrent: 4,
        }
    }
}

/// Historique de la page Performances.
///
/// La page mesure en direct toutes les `live_secs` ; le serveur enregistre une
/// mesure toutes les `sample_secs` et ne garde que `retention_days` jours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Performance {
    /// `false` : aucune mesure enregistrée (la page reste en direct).
    pub enabled: bool,
    /// Fréquence d'enregistrement côté serveur, en secondes.
    #[serde(deserialize_with = "seconds")]
    pub sample_secs: u64,
    /// Durée de conservation, en jours (plafonnée à 30).
    #[serde(deserialize_with = "days")]
    pub retention_days: u32,
    /// Fréquence de rafraîchissement de la page, en secondes. Une fraction est
    /// acceptée : `0.5` = deux mesures par seconde (2 FPS).
    #[serde(deserialize_with = "live_seconds")]
    pub live_secs: f64,
}

/// Nombre de secondes lu sans faire échouer tout le fichier : une valeur
/// négative, NaN ou démesurée retombe dans les bornes au lieu de rejeter la
/// configuration entière (c'est [`Performance::sanitized`] qui applique le
/// minimum).
fn seconds<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = f64::deserialize(deserializer)?;
    Ok(value.clamp(0.0, u64::MAX as f64).round() as u64)
}

/// Idem pour un nombre de jours, borné au type `u32`.
fn days<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(seconds(deserializer)?.min(u64::from(u32::MAX)) as u32)
}

/// Secondes fractionnaires du direct : `0.5` = 2 mesures par seconde.
/// Une valeur négative ou non finie devient 0, puis [`Performance::sanitized`]
/// applique le plancher.
fn live_seconds<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = f64::deserialize(deserializer)?;
    Ok(if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    })
}

impl Default for Performance {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_secs: 10,
            retention_days: 30,
            // 2 mesures par seconde : la vue « temps réel » reste fluide.
            live_secs: 0.5,
        }
    }
}

impl Performance {
    /// Bornes appliquées à la lecture : 1 s à 1 h d'échantillonnage, 0,25 à
    /// 60 s de rafraîchissement (4 FPS au plus), 1 à 30 jours de rétention.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.sample_secs = self.sample_secs.clamp(1, 3600);
        self.live_secs = self.live_secs.clamp(0.25, 60.0);
        self.retention_days = self.retention_days.clamp(1, 30);
        self
    }
}

/// Plafonds de ressources : sans eux, une rafale de requêtes ouvre autant de
/// lectures d'archive que de connexions, chacune avec sa page en mémoire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// Lectures de livre simultanées (page, ressource, chapitre, fiche). Les
    /// suivantes attendent leur tour.
    pub max_concurrent_reads: usize,
    /// Threads bloquants du runtime Tokio (Tokio en autorise 512 par défaut).
    pub max_blocking_threads: usize,
    /// Archives ZIP gardées ouvertes entre deux requêtes (`0` : aucune).
    pub open_archives: usize,
    /// Comptages de pages gardés en mémoire : au moins le nombre de livres de la
    /// bibliothèque, sinon chaque scan rouvre les livres en trop.
    pub page_counts: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_concurrent_reads: 64,
            max_blocking_threads: 128,
            open_archives: 16,
            page_counts: 50_000,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            books_root: "../books".to_string(),
            limits: Limits::default(),
            resize: Resize::default(),
            performance: Performance::default(),
            auth: Auth::default(),
        }
    }
}

static INSTALLED: OnceLock<Config> = OnceLock::new();

/// Fixe la configuration du process (à appeler une fois, au démarrage).
pub fn install(config: Config) {
    let _ = INSTALLED.set(config);
}

/// Plafonds en vigueur : ceux de `config.yml`, ou les valeurs par défaut si
/// [`install`] n'a pas été appelée (tests).
pub fn limits() -> Limits {
    let mut limits = INSTALLED
        .get()
        .map_or_else(Limits::default, |config| config.limits.clone());
    // Un plafond à zéro bloquerait toute lecture.
    limits.max_concurrent_reads = limits.max_concurrent_reads.max(1);
    limits.max_blocking_threads = limits.max_blocking_threads.max(1);
    limits
}

/// Comptes et connexion en vigueur (l'environnement gagne sur le fichier).
pub fn auth() -> Auth {
    let mut auth = INSTALLED
        .get()
        .map_or_else(Auth::default, |config| config.auth.clone());
    auth.oidc = auth.oidc.with_env(|name| std::env::var(name).ok());
    auth.session_days = auth.session_days.max(1);
    auth
}

/// `true` quand `EASYBOOK_MODE=dev` : outils de développement (connexion directe
/// à Keycloak). Tout autre valeur, ou l'absence de variable : production.
pub fn is_dev(mode: Option<&str>) -> bool {
    mode.is_some_and(|mode| mode.trim().eq_ignore_ascii_case("dev"))
}

/// Mode de l'environnement (`EASYBOOK_MODE`).
pub fn dev_mode() -> bool {
    is_dev(std::env::var("EASYBOOK_MODE").ok().as_deref())
}

/// Réglages de réduction des planches en vigueur.
pub fn resize() -> Resize {
    let mut resize = INSTALLED
        .get()
        .map_or_else(Resize::default, |config| config.resize.clone());
    resize.quality = resize.quality.clamp(1, 100);
    resize.max_concurrent = resize.max_concurrent.max(1);
    resize
}

/// Historique des performances en vigueur (fréquences bornées, 30 jours max).
pub fn performance() -> Performance {
    INSTALLED
        .get()
        .map_or_else(Performance::default, |config| config.performance.clone())
        .sanitized()
}

impl Config {
    /// Chemin du fichier de configuration.
    pub fn config_path() -> PathBuf {
        std::env::var("EASYBOOK_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("config.yml"))
    }

    /// Charge le fichier ; en l'absence de fichier, rend les valeurs par défaut.
    pub fn load() -> Self {
        Self::load_from(&Self::config_path())
    }

    /// Charge **ce** fichier ; en l'absence de fichier, rend les valeurs par
    /// défaut.
    ///
    /// Séparé de [`Config::load`] pour être testable : la seule chose que `load`
    /// ajoute, c'est la lecture de l'environnement.
    ///
    /// Un fichier présent mais illisible **n'empêche pas** le démarrage : le
    /// serveur annonce l'erreur et part sur les défauts (le dossier `books/`
    /// reste ainsi consultable même si le YAML a été cassé à la main).
    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_yaml::from_str(&text) {
            Ok(config) => config,
            Err(e) => {
                eprintln!(
                    "⚠️  {} illisible ({e}) : valeurs par défaut utilisées.",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Dossier des livres réellement utilisé.
    pub fn resolve_books_root(&self) -> PathBuf {
        self.resolve_with(
            std::env::var("BOOKS_ROOT").ok().as_deref(),
            &Self::config_path(),
        )
    }

    /// Racine résolue pour un `BOOKS_ROOT` et un fichier de configuration donnés.
    ///
    /// Priorité : `BOOKS_ROOT` (ce que pose le conteneur), puis `books_root`
    /// relatif au dossier de `config.yml`. Fonction pure : l'environnement est
    /// un **paramètre**, ce qui la rend testable sans le modifier.
    pub fn resolve_with(&self, env_root: Option<&str>, config_path: &Path) -> PathBuf {
        if let Some(root) = env_root.filter(|root| !root.trim().is_empty()) {
            return PathBuf::from(root);
        }

        let raw = PathBuf::from(&self.books_root);
        if raw.is_absolute() {
            return raw;
        }
        let base = config_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_root_is_sibling_of_backend() {
        // `config.yml` vit dans `backend/` : `../books` désigne la racine du dépôt,
        // quel que soit le dossier courant (chemin relatif au fichier de config).
        let config = Config::default();
        assert!(
            config
                .resolve_with(None, Path::new("config.yml"))
                .ends_with("books")
        );
    }

    #[test]
    fn a_relative_root_hangs_from_the_config_file_directory() {
        let config = Config {
            books_root: "../books".to_string(),
            ..Config::default()
        };
        assert_eq!(
            config.resolve_with(None, Path::new("/srv/easybook/backend/config.yml")),
            PathBuf::from("/srv/easybook/backend/../books")
        );
    }

    #[test]
    fn an_absolute_root_is_kept_as_is() {
        let config = Config {
            books_root: "/livres".to_string(),
            ..Config::default()
        };
        assert_eq!(
            config.resolve_with(None, Path::new("/srv/backend/config.yml")),
            PathBuf::from("/livres")
        );
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        let config = Config {
            books_root: "/livres".to_string(),
            ..Config::default()
        };
        // Ce que pose le conteneur : `BOOKS_ROOT=/books`.
        assert_eq!(
            config.resolve_with(Some("/books"), Path::new("/srv/backend/config.yml")),
            PathBuf::from("/books")
        );
        // Une variable vide ou blanche ne compte pas.
        assert_eq!(
            config.resolve_with(Some("  "), Path::new("/srv/backend/config.yml")),
            PathBuf::from("/livres")
        );
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.yml");
        assert_eq!(Config::load_from(&missing), Config::default());
    }

    #[test]
    fn the_file_is_read_when_it_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(&path, "books_root: /livres\n").unwrap();
        assert_eq!(
            Config::load_from(&path),
            Config {
                books_root: "/livres".to_string(),
                ..Config::default()
            }
        );
    }

    #[test]
    fn limits_are_read_and_default_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(&path, "limits:\n  max_concurrent_reads: 4\n").unwrap();

        let limits = Config::load_from(&path).limits;
        assert_eq!(limits.max_concurrent_reads, 4);
        // Une clé absente garde sa valeur par défaut.
        assert_eq!(limits.open_archives, Limits::default().open_archives);
    }

    #[test]
    fn auth_is_on_by_default_and_oidc_off() {
        let auth = Config::default().auth;
        assert!(auth.enabled);
        assert!(!auth.oidc.is_enabled());
        assert!(!auth.oidc.auto_create && !auth.oidc.link_by_email && !auth.oidc.self_link);
    }

    #[test]
    fn auth_is_read_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(
            &path,
            "auth:\n  session_days: 7\n  oidc:\n    issuer: https://idp\n    client_id: eb\n    auto_create: true\n",
        )
        .unwrap();

        let auth = Config::load_from(&path).auth;
        assert_eq!(auth.session_days, 7);
        assert!(auth.enabled);
        assert!(auth.oidc.is_enabled() && auth.oidc.auto_create);
        assert_eq!(auth.oidc.scopes, Oidc::default().scopes);
    }

    #[test]
    fn oidc_environment_wins_over_the_file() {
        let oidc = Oidc {
            issuer: "https://file".into(),
            client_id: "file".into(),
            auto_create: true,
            ..Oidc::default()
        }
        .with_env(|name| match name {
            "EASYBOOK_OIDC_ISSUER" => Some("https://env".into()),
            "EASYBOOK_OIDC_CLIENT_SECRET" => Some("s3cret".into()),
            "EASYBOOK_OIDC_AUTO_CREATE" => Some("false".into()),
            "EASYBOOK_OIDC_LABEL" => Some("  ".into()),
            _ => None,
        });
        assert_eq!(oidc.issuer, "https://env");
        assert_eq!(oidc.client_id, "file");
        assert_eq!(oidc.client_secret, "s3cret");
        assert!(!oidc.auto_create);
        // Une variable blanche ne compte pas.
        assert_eq!(oidc.label, Oidc::default().label);
    }

    #[test]
    fn performance_is_read_from_the_file_and_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(
            &path,
            "performance:\n  enabled: false\n  sample_secs: 5\n  retention_days: 90\n  live_secs: 3\n",
        )
        .unwrap();

        let performance = Config::load_from(&path).performance;
        assert!(!performance.enabled);
        assert_eq!(performance.sample_secs, 5);
        assert_eq!(performance.live_secs, 3.0);
        // La lecture applique le plafond de 30 jours.
        assert_eq!(performance.sanitized().retention_days, 30);
    }

    #[test]
    fn performance_defaults_and_zero_values_are_sanitized() {
        let defaults = Performance::default();
        assert!(defaults.enabled);
        assert_eq!(defaults.sample_secs, 10);
        assert_eq!(defaults.retention_days, 30);
        let zero = Performance {
            sample_secs: 0,
            retention_days: 0,
            live_secs: 0.0,
            ..Performance::default()
        }
        .sanitized();
        assert_eq!(zero.sample_secs, 1);
        assert_eq!(zero.retention_days, 1);
        assert_eq!(zero.live_secs, 0.25);
    }

    #[test]
    fn a_partial_performance_section_keeps_the_other_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(&path, "performance:\n  enabled: false\n").unwrap();

        let performance = Config::load_from(&path).performance;
        assert!(!performance.enabled);
        // Les champs absents gardent les valeurs par défaut, pas zéro.
        assert_eq!(performance.sample_secs, 10);
        assert_eq!(performance.retention_days, 30);
        assert_eq!(performance.live_secs, 0.5);
    }

    #[test]
    fn negative_or_huge_values_do_not_throw_the_file_away() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(
            &path,
            "books_root: /livres\nperformance:\n  sample_secs: -10\n  retention_days: -5\n  live_secs: 1e30\n",
        )
        .unwrap();

        let config = Config::load_from(&path);
        // Le reste du fichier survit à une valeur absurde.
        assert_eq!(config.books_root, "/livres");
        assert_eq!(config.performance.sample_secs, 0);
        assert_eq!(config.performance.retention_days, 0);
        assert_eq!(config.performance.live_secs, 1e30);
        let fixed = config.performance.sanitized();
        assert_eq!(fixed.sample_secs, 1);
        assert_eq!(fixed.retention_days, 1);
        assert_eq!(fixed.live_secs, 60.0);
    }

    #[test]
    fn live_secs_accepts_fps_fractions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(&path, "performance:\n  live_secs: 0.5\n").unwrap();

        let performance = Config::load_from(&path).performance;
        // 0,5 s = 2 mesures par seconde.
        assert_eq!(performance.live_secs, 0.5);
        assert_eq!(performance.sanitized().live_secs, 0.5);
        // Plancher : 4 FPS.
        let too_fast = Performance {
            live_secs: 0.01,
            ..Performance::default()
        }
        .sanitized();
        assert_eq!(too_fast.live_secs, 0.25);
    }

    #[test]
    fn production_is_the_default_mode() {
        assert!(!is_dev(None));
        assert!(!is_dev(Some("")));
        assert!(!is_dev(Some("prod")));
        assert!(!is_dev(Some("development-ish")));
        assert!(is_dev(Some("dev")));
        assert!(is_dev(Some(" DEV ")));
    }

    #[test]
    fn a_broken_file_falls_back_to_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cassee.yml");
        std::fs::write(&path, "books_root: [").unwrap();
        // Le serveur doit démarrer quand même : on garde les valeurs par défaut.
        assert_eq!(Config::load_from(&path), Config::default());
    }
}
