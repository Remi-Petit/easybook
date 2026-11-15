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

/// Contenu de `config.yml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Dossier des livres. Relatif au dossier de `config.yml` s'il ne commence
    /// pas par `/`.
    pub books_root: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            books_root: "../books".to_string(),
        }
    }
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
                books_root: "/livres".to_string()
            }
        );
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
