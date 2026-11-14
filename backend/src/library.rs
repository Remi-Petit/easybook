//! Scan récursif du dossier des livres.
//!
//! Le résultat est **plat** : une liste de livres et une liste de dossiers, avec
//! des chemins relatifs en `/`. Le frontend reconstitue la hiérarchie à partir
//! des `rel` — le JSON ne dépend donc pas de la profondeur de l'arborescence, et
//! un livre rangé trois niveaux plus bas n'a rien de particulier.

use crate::formats::{self, Format};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Un livre trouvé dans le dossier.
#[derive(Debug, Clone, Serialize)]
pub struct BookInfo {
    /// Chemin relatif à la racine, en `/` (ex : `Chainsaw Man/T01.cbz`).
    pub rel: String,
    /// Nom du fichier seul.
    pub name: String,
    /// Dossier de premier niveau (`""` pour un livre à la racine).
    pub folder: String,
    /// Format reconnu (`epub`, `cbz`, `cbr`, `pdf`).
    pub format: &'static str,
    /// `false` pour un format que ce serveur ne sait pas encore ouvrir (CBR).
    pub readable: bool,
    /// Taille en octets.
    pub size: u64,
    /// Dernière modification, en secondes Unix.
    pub modified: Option<u64>,
    /// Titre affiché : le nom sans extension.
    pub title: String,
}

/// Un dossier du catalogue.
#[derive(Debug, Clone, Serialize)]
pub struct FolderInfo {
    /// Chemin relatif à la racine.
    pub rel: String,
    /// Nom seul (dernier segment).
    pub name: String,
    /// Nombre de livres contenus, **récursivement**.
    pub count: usize,
    /// Dernière modification du dossier, en secondes Unix.
    pub modified: Option<u64>,
}

/// Contenu complet du dossier des livres.
#[derive(Debug, Default, Serialize)]
pub struct LibraryScan {
    /// Tous les livres, à plat, triés naturellement (`T2` avant `T10`).
    pub books: Vec<BookInfo>,
    /// Tous les dossiers, à plat, triés naturellement.
    pub folders: Vec<FolderInfo>,
    /// Nombre de livres.
    pub total: usize,
}

/// Parcourt `root` et décrit tout ce qu'il contient.
///
/// Un dossier ou un fichier **caché** (nom commençant par `.`) est ignoré :
/// c'est ainsi qu'on tient hors du catalogue `.gitkeep`, les fichiers des
/// éditeurs et nos propres artefacts. Les liens symboliques sont ignorés aussi
/// — un lien vers un parent ferait boucler le parcours.
pub fn scan(root: &Path) -> LibraryScan {
    let mut books: Vec<BookInfo> = Vec::new();
    let mut folders: Vec<FolderInfo> = Vec::new();
    walk(root, root, &mut books, &mut folders);

    books.sort_by(|a, b| formats::natural_cmp(&a.rel, &b.rel));
    folders.sort_by(|a, b| formats::natural_cmp(&a.rel, &b.rel));

    // Comptage récursif : chaque livre incrémente tous ses dossiers parents.
    let mut counts: HashMap<String, usize> = HashMap::new();
    for book in &books {
        let mut prefix = String::new();
        // Le dernier segment est le nom du fichier : seuls les dossiers comptent.
        let segments: Vec<&str> = book.rel.split('/').collect();
        for segment in &segments[..segments.len().saturating_sub(1)] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            *counts.entry(prefix.clone()).or_default() += 1;
        }
    }
    for folder in &mut folders {
        folder.count = counts.get(&folder.rel).copied().unwrap_or(0);
    }

    LibraryScan {
        total: books.len(),
        books,
        folders,
    }
}

fn walk(root: &Path, dir: &Path, books: &mut Vec<BookInfo>, folders: &mut Vec<FolderInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        // `file_type` ne suit pas les liens : c'est voulu (pas de boucle).
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }

        let path = entry.path();
        let rel = relative(root, &path);

        if kind.is_dir() {
            folders.push(FolderInfo {
                rel,
                name,
                count: 0,
                modified: modified(&path),
            });
            walk(root, &path, books, folders);
            continue;
        }

        if !kind.is_file() {
            continue;
        }
        let Some(format) = Format::from_path(&rel) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };

        let folder = match rel.split_once('/') {
            Some((first, _)) => first.to_string(),
            None => String::new(),
        };

        books.push(BookInfo {
            folder,
            title: title_of(&name),
            name,
            format: format.id(),
            readable: format.readable(),
            size: meta.len(),
            modified: modified(&path),
            rel,
        });
    }
}

/// Chemin relatif à `root`, séparateurs `/`.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Date de modification, en secondes Unix.
fn modified(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Titre affiché : le nom du fichier sans son extension.
fn title_of(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_recursively_and_counts_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Manga/Serie")).unwrap();
        fs::write(root.join("racine.epub"), b"x").unwrap();
        fs::write(root.join("Manga/T1.cbz"), b"x").unwrap();
        fs::write(root.join("Manga/Serie/T2.cbz"), b"x").unwrap();
        // Ignorés : mauvaise extension et fichier caché.
        fs::write(root.join("Manga/notes.txt"), b"x").unwrap();
        fs::write(root.join(".DS_Store"), b"x").unwrap();

        let scan = scan(root);
        assert_eq!(scan.total, 3);
        assert_eq!(
            scan.books
                .iter()
                .map(|b| b.rel.as_str())
                .collect::<Vec<_>>(),
            vec!["Manga/Serie/T2.cbz", "Manga/T1.cbz", "racine.epub"]
        );
        // `folder` : premier segment pour un livre rangé, vide à la racine.
        assert_eq!(scan.books[0].folder, "Manga");
        assert_eq!(scan.books[2].folder, "");
        assert_eq!(scan.books[2].title, "racine");

        let counts: HashMap<_, _> = scan
            .folders
            .iter()
            .map(|f| (f.rel.as_str(), f.count))
            .collect();
        assert_eq!(counts["Manga"], 2);
        assert_eq!(counts["Manga/Serie"], 1);
    }

    #[test]
    fn missing_root_is_an_empty_library() {
        let scan = scan(Path::new("ce-dossier-n-existe-pas"));
        assert_eq!(scan.total, 0);
        assert!(scan.books.is_empty());
    }
}
