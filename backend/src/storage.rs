//! Espace disque : bibliothèque, couvertures, comptages de pages, bases.
//!
//! L'onglet « Stockage » de la page Performances lit ces chiffres à la demande.
//! La bibliothèque est résumée depuis le catalogue déjà sérialisé (aucun scan
//! supplémentaire) ; le reste vient de `stat` et de deux parcours de dossiers
//! (couvertures et build de l'interface).
//!
//! Rien n'est stocké : ces valeurs ne sont pas dans l'historique.

use crate::config;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Debug, Default, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub books: u64,
    /// Livres dont le nombre de pages a pu être compté.
    pub with_pages: u64,
    /// Somme des pages connues (chapitres pour un EPUB).
    pub pages: u64,
    /// Taille des fichiers de livres.
    pub bytes: u64,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Covers {
    pub disk_files: u64,
    pub disk_bytes: u64,
    pub memory_entries: u64,
    pub memory_bytes: u64,
    pub memory_budget: u64,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Pages {
    pub file_bytes: u64,
    /// Comptages en mémoire (chargés depuis `pages.json` + nouveaux).
    pub entries: u64,
    /// Plafond du cache (`limits.page_counts`).
    pub capacity: u64,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Database {
    pub name: String,
    pub bytes: u64,
    pub wal_bytes: u64,
    pub shm_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct App {
    /// Binaire de l'API en cours d'exécution.
    pub binary_bytes: u64,
    /// Build de l'interface trouvé à côté (adapter-node), si présent.
    pub ui_build_bytes: Option<u64>,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub library: Library,
    pub covers: Covers,
    pub pages: Pages,
    pub databases: Vec<Database>,
    pub app: App,
    /// Bases + couvertures + comptages ; la bibliothèque et le code sont à part.
    pub total_bytes: u64,
}

/// Résume le catalogue sérialisé (`{ books: [{ size, pages }] }`).
pub fn library_stats(catalog_json: &str) -> Library {
    let Ok(catalog) = serde_json::from_str::<serde_json::Value>(catalog_json) else {
        return Library::default();
    };
    let Some(books) = catalog.get("books").and_then(|books| books.as_array()) else {
        return Library::default();
    };
    let mut library = Library {
        books: books.len() as u64,
        ..Library::default()
    };
    for book in books {
        library.bytes += book.get("size").and_then(|size| size.as_u64()).unwrap_or(0);
        if let Some(pages) = book.get("pages").and_then(|pages| pages.as_u64()) {
            library.with_pages += 1;
            library.pages += pages;
        }
    }
    library
}

/// Rassemble les chiffres du disque. `library` vient du catalogue en mémoire.
pub fn collect(data_dir: &Path, cache_dir: &Path, library: Library) -> Storage {
    let covers = covers(cache_dir);
    let pages = pages(cache_dir);
    let databases = vec![
        database(data_dir, "accounts.db"),
        database(data_dir, "metrics.db"),
    ];
    let database_bytes = databases
        .iter()
        .map(|database| database.total_bytes)
        .sum::<u64>();
    let total_bytes = database_bytes + covers.disk_bytes + pages.file_bytes;
    Storage {
        library,
        covers,
        pages,
        databases,
        app: app(),
        total_bytes,
    }
}

fn covers(cache_dir: &Path) -> Covers {
    let (disk_files, disk_bytes) = dir_stats(cache_dir, is_jpeg);
    let (entries, bytes) = crate::thumbnail::cache_stats();
    Covers {
        disk_files,
        disk_bytes,
        memory_entries: entries as u64,
        memory_bytes: bytes as u64,
        memory_budget: crate::thumbnail::CACHE_BYTES as u64,
    }
}

fn pages(cache_dir: &Path) -> Pages {
    Pages {
        file_bytes: std::fs::metadata(cache_dir.join("pages.json")).map_or(0, |meta| meta.len()),
        entries: crate::pages::cache_entries() as u64,
        capacity: config::limits().page_counts as u64,
    }
}

fn database(data_dir: &Path, name: &str) -> Database {
    let size = |suffix: &str| {
        std::fs::metadata(data_dir.join(format!("{name}{suffix}"))).map_or(0, |meta| meta.len())
    };
    let bytes = size("");
    let wal_bytes = size("-wal");
    let shm_bytes = size("-shm");
    Database {
        name: name.to_string(),
        bytes,
        wal_bytes,
        shm_bytes,
        total_bytes: bytes + wal_bytes + shm_bytes,
    }
}

fn app() -> App {
    let binary_bytes = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::metadata(path).ok())
        .map_or(0, |meta| meta.len());
    App {
        binary_bytes,
        ui_build_bytes: ui_build_dir().map(|dir| dir_stats(&dir, |_| true).1),
    }
}

/// Build de l'interface (adapter-node) : `/app/build` dans l'image, à côté du
/// dépôt en développement. Un fichier `index.js` évite de confondre un dossier
/// `build` quelconque avec celui de l'application.
fn ui_build_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    [
        PathBuf::from("/app/build"),
        cwd.join("build"),
        cwd.join("../frontend/build"),
        cwd.join("frontend/build"),
    ]
    .into_iter()
    .find(|dir| dir.join("index.js").is_file())
}

fn is_jpeg(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("jpg")
}

/// (fichiers, octets) d'un dossier, récursivement, sans suivre les liens.
fn dir_stats(dir: &Path, keep: impl Fn(&Path) -> bool) -> (u64, u64) {
    fn walk(dir: &Path, keep: &dyn Fn(&Path) -> bool, files: &mut u64, bytes: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                walk(&path, keep, files, bytes);
            } else if kind.is_file() && keep(&path) {
                *files += 1;
                *bytes += entry.metadata().map_or(0, |meta| meta.len());
            }
        }
    }
    let mut files = 0;
    let mut bytes = 0;
    walk(dir, &keep, &mut files, &mut bytes);
    (files, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_the_catalog() {
        let library = library_stats(
            r#"{"books":[{"size":100,"pages":10},{"size":50,"pages":null},{"size":25}]}"#,
        );
        assert_eq!(library.books, 3);
        assert_eq!(library.with_pages, 1);
        assert_eq!(library.pages, 10);
        assert_eq!(library.bytes, 175);
    }

    #[test]
    fn a_broken_catalog_gives_zeros() {
        assert_eq!(library_stats("{").books, 0);
        assert_eq!(library_stats(r#"{"folders":[]}"#).books, 0);
    }

    #[test]
    fn counts_files_and_bytes_without_following_the_filter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sous")).unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"abc").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"de").unwrap();
        std::fs::write(dir.path().join("sous/c.jpg"), b"f").unwrap();
        assert_eq!(dir_stats(dir.path(), is_jpeg), (2, 4));
        assert_eq!(dir_stats(dir.path(), |_| true), (3, 6));
    }
}
