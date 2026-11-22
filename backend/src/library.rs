//! Scan récursif du dossier des livres.
//!
//! Le résultat est **plat** : une liste de livres et une liste de dossiers, avec
//! des chemins relatifs en `/`. Le frontend reconstitue la hiérarchie à partir
//! des `rel` — le JSON ne dépend donc pas de la profondeur de l'arborescence, et
//! un livre rangé trois niveaux plus bas n'a rien de particulier.

use crate::formats::{self, Format};
use crate::pages;
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
    /// Nombre de pages (nombre de chapitres pour un EPUB), `None` si le livre
    /// n'a pas pu être ouvert.
    pub pages: Option<usize>,
    /// Pourquoi le livre ne s'ouvre pas, ou précision sur son format : le
    /// message du format, **sans** le chemin (le catalogue l'affiche à côté).
    /// Absent quand le livre s'ouvre normalement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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

    pages::save();
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
        // Une seule lecture des métadonnées par livre : taille et date servent
        // au catalogue et à la clé du cache des pages.
        let stamp = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs());

        let folder = match rel.split_once('/') {
            Some((first, _)) => first.to_string(),
            None => String::new(),
        };

        let counted = pages::open_stat(&path, format, meta.len(), stamp.unwrap_or(0));
        books.push(BookInfo {
            folder,
            title: title_of(&name),
            pages: counted.pages,
            note: counted
                .problem
                .or_else(|| formats::module(format).note().map(str::to_string)),
            name,
            format: format.id(),
            readable: format.readable(),
            size: meta.len(),
            modified: stamp,
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
pub fn title_of(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

/// Une ligne par entrée visible : chemin relatif, est-ce un dossier, taille,
/// date de modification en secondes.
type Stamp = (String, bool, u64, u64);

/// Empreinte bon marché de ce que le catalogue verrait.
///
/// Elle sert de filet de sécurité au watcher : certains montages ne remontent
/// **aucun** événement système (le dossier d'un hôte monté sous Docker Desktop,
/// par exemple). Comparer cette empreinte à intervalle régulier détecte alors
/// les ajouts, suppressions et remplacements. Seules les métadonnées sont lues,
/// jamais le contenu des livres.
pub fn fingerprint(root: &Path) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut entries: Vec<Stamp> = Vec::new();
    collect(root, root, &mut entries);
    // L'ordre de `read_dir` n'est pas garanti : on trie pour une empreinte stable.
    entries.sort();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

fn collect(root: &Path, dir: &Path, entries: &mut Vec<Stamp>) {
    let Ok(list) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in list.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Mêmes règles que `walk` : un caché ou un lien n'apparaît pas dans le
        // catalogue, il ne doit pas faire bouger l'empreinte.
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }

        let path = entry.path();
        let meta = entry.metadata().ok();
        let size = meta.as_ref().map_or(0, |meta| meta.len());
        // Secondes, comme le reste du scan : la sous-seconde d'un fichier qui
        // vient d'être écrit peut encore bouger, sans que le catalogue change.
        let when = meta
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_secs());
        entries.push((relative(root, &path), kind.is_dir(), size, when));

        if kind.is_dir() {
            collect(root, &path, entries);
        }
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

    #[test]
    fn fingerprint_follows_additions_and_removals() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Manga")).unwrap();
        fs::write(root.join("Manga/T1.cbz"), b"x").unwrap();

        let empty = fingerprint(root);
        assert_eq!(fingerprint(root), empty, "l'empreinte est stable");

        // Un livre déposé à la racine change l'empreinte.
        fs::write(root.join("racine.epub"), b"x").unwrap();
        let added = fingerprint(root);
        assert_ne!(added, empty);

        // Un caché ne change rien : il n'est pas dans le catalogue.
        fs::write(root.join(".DS_Store"), b"x").unwrap();
        assert_eq!(fingerprint(root), added);

        // La suppression du livre se voit aussi.
        fs::remove_file(root.join("racine.epub")).unwrap();
        assert_eq!(fingerprint(root), empty);
    }
}
