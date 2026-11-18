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

/// Réglages de réduction des planches en vigueur.
pub fn resize() -> Resize {
    let mut resize = INSTALLED
        .get()
        .map_or_else(Resize::default, |config| config.resize.clone());
    resize.quality = resize.quality.clamp(1, 100);
    resize.max_concurrent = resize.max_concurrent.max(1);
    resize
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
    fn a_broken_file_falls_back_to_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cassee.yml");
        std::fs::write(&path, "books_root: [").unwrap();
        // Le serveur doit démarrer quand même : on garde les valeurs par défaut.
        assert_eq!(Config::load_from(&path), Config::default());
    }
}
