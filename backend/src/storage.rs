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

#[derive(Serialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub books: u64,
    /// Livres dont le nombre de pages a pu être compté.
    pub with_pages: u64,
    /// Somme des pages connues (chapitres pour un EPUB).
    pub pages: u64,
    /// Taille des fichiers de livres.
    pub bytes: u64,
    /// Les plus gros livres, du plus lourd au plus léger.
    pub largest: Vec<LargestBook>,
}

/// Un livre du résumé : de quoi nommer ce qui pèse le plus lourd.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LargestBook {
    /// Chemin relatif à la racine (`Chainsaw Man/T01.cbz`).
    pub rel: String,
    pub size: u64,
}

/// Nombre de livres détaillés dans le résumé.
const LARGEST: usize = 5;

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
    /// Bases + couvertures + comptages : ce que l'application se garde.
    pub data_bytes: u64,
    /// Binaire de l'API + build de l'interface : le programme lui-même.
    pub code_bytes: u64,
    /// `data_bytes` + `code_bytes` : l'application seule, hors livres.
    pub app_bytes: u64,
    /// `app_bytes` + livres : tout ce que l'installation occupe.
    pub total_bytes: u64,
}

/// Résume le catalogue sérialisé (`{ books: [{ rel, size, pages }] }`).
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
        let size = book.get("size").and_then(|size| size.as_u64()).unwrap_or(0);
        library.bytes += size;
        if let Some(pages) = book.get("pages").and_then(|pages| pages.as_u64()) {
            library.with_pages += 1;
            library.pages += pages;
        }
        if let Some(rel) = book.get("rel").and_then(|rel| rel.as_str()) {
            library.largest.push(LargestBook {
                rel: rel.to_string(),
                size,
            });
        }
    }
    library
        .largest
        .sort_by_key(|book| std::cmp::Reverse(book.size));
    library.largest.truncate(LARGEST);
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
    let app = app();
    let code_bytes = app.binary_bytes + app.ui_build_bytes.unwrap_or(0);
    let data_bytes = database_bytes + covers.disk_bytes + pages.file_bytes;
    let app_bytes = data_bytes + code_bytes;
    Storage {
        total_bytes: app_bytes + library.bytes,
        library,
        covers,
        pages,
        databases,
        app,
        data_bytes,
        code_bytes,
        app_bytes,
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
            r#"{"books":[
                {"rel":"a/T02.cbz","size":100,"pages":10},
                {"rel":"a/T01.cbz","size":50,"pages":null},
                {"rel":"b/T03.cbz","size":25}
            ]}"#,
        );
        assert_eq!(library.books, 3);
        assert_eq!(library.with_pages, 1);
        assert_eq!(library.pages, 10);
        assert_eq!(library.bytes, 175);
        // Du plus lourd au plus léger : la bibliothèque dit ce qui pèse.
        assert_eq!(
            library.largest,
            vec![
                LargestBook {
                    rel: "a/T02.cbz".into(),
                    size: 100
                },
                LargestBook {
                    rel: "a/T01.cbz".into(),
                    size: 50
                },
                LargestBook {
                    rel: "b/T03.cbz".into(),
                    size: 25
                }
            ]
        );
    }

    #[test]
    fn keeps_only_the_five_biggest_books() {
        let books: Vec<String> = (0..8)
            .map(|index| format!(r#"{{"rel":"{index}.cbz","size":{}}}"#, index + 1))
            .collect();
        let library = library_stats(&format!(r#"{{"books":[{}]}}"#, books.join(",")));
        assert_eq!(library.largest.len(), 5);
        assert_eq!(library.largest[0].rel, "7.cbz");
        // Somme de tous les livres, pas seulement des plus gros.
        assert_eq!(library.bytes, (1..=8).sum::<u64>());
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
