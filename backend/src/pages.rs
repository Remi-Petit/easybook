//! Nombre de pages d'un livre : comptage à la demande, avec cache mémoire et disque.
//!
//! Le catalogue annonce ce nombre, donc chaque livre doit être ouvert au moins
//! une fois — une lecture d'en-têtes d'archive, ou du document pour un PDF. Le
//! cache évite de le refaire à chaque scan : un scan a lieu à chaque changement
//! du dossier, pas seulement au démarrage.
//!
//! Une entrée vaut pour un chemin, un format, une taille et une date de
//! fichier : un livre remplacé est un autre livre, et son comptage est refait.
//! Le format en fait partie parce qu'un même chemin peut être tenté deux fois
//! (un PDF nommé `.cbr` l'est d'abord comme bande dessinée, voir
//! [`formats::resolve`]).
//!
//! Les comptages sont aussi écrits dans [`FILE`] (voir [`persist_in`]) : sans
//! cela, chaque démarrage rouvrirait tous les livres, et lopdf lit un PDF en
//! entier pour en compter les pages.

use crate::formats::{self, Format};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// Nom du fichier de persistance, dans le dossier du cache disque.
const FILE: &str = "pages.json";

/// Nombre de comptages gardés (`limits.page_counts`). Au-delà, une entrée est
/// évincée à chaque ajout : une bibliothèque plus grande que ce nombre rouvre
/// les livres excédentaires à **chaque** scan.
fn capacity() -> usize {
    crate::config::limits().page_counts
}

/// Nombre de pages d'un livre, `None` s'il ne peut pas être ouvert.
///
/// Pour un EPUB, c'est le nombre de **chapitres** (`spine`) : le texte se
/// recompose à l'écran, un EPUB n'a pas de pages fixes (voir
/// [`crate::formats::epub`]).
pub fn count(path: &Path, format: Format) -> Option<usize> {
    let meta = std::fs::metadata(path).ok()?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs());
    count_stat(path, format, meta.len(), stamp)
}

/// Ce qu'un comptage a donné : le nombre de pages s'il est connu, et sinon la
/// **raison** de l'échec telle que le format l'a dite (sans le chemin, que
/// l'appelant connaît déjà).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Count {
    pub pages: Option<usize>,
    pub problem: Option<String>,
}

/// Comme [`count`], pour un appelant qui a déjà lu la taille et la date du
/// fichier (le scan : un `stat` de moins par livre).
pub fn count_stat(path: &Path, format: Format, size: u64, stamp: u64) -> Option<usize> {
    open_stat(path, format, size, stamp).pages
}

/// Comptage **et** raison de l'échec, quand le livre ne peut pas être ouvert.
pub fn open_stat(path: &Path, format: Format, size: u64, stamp: u64) -> Count {
    let key = path.display().to_string();

    if let Some(cached) = cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key, format, size, stamp).cloned())
    {
        return cached;
    }

    let count = match formats::module(format).pages(path) {
        Ok(pages) => Count {
            pages,
            problem: None,
        },
        Err(error) => Count {
            pages: None,
            problem: Some(explain(&error, path)),
        },
    };
    crate::readings::release_if_idle(path);
    if let Ok(mut cache) = cache().lock() {
        cache.put(
            key,
            Entry {
                format,
                size,
                stamp,
                count: count.clone(),
            },
        );
    }
    count
}

/// Le message du format, sans le chemin que l'appelant affiche déjà.
fn explain(error: &str, path: &Path) -> String {
    let prefix = format!("{} : ", path.display());
    error.strip_prefix(&prefix).unwrap_or(error).to_string()
}

/// Nombre de comptages en cache.
pub fn cache_entries() -> usize {
    cache().lock().map_or(0, |cache| cache.entries.len())
}

/// Active la persistance : charge les comptages de `dir` et les y réécrit
/// ([`save`]). À appeler avant le premier scan.
pub fn persist_in(dir: &Path) {
    if let Ok(mut cache) = cache().lock() {
        cache.load(dir.join(FILE));
    }
}

/// Écrit les comptages sur disque s'il y en a de nouveaux depuis la dernière écriture.
pub fn save() {
    if let Ok(mut cache) = cache().lock() {
        cache.save();
    }
}

#[derive(Clone)]
struct Entry {
    /// Module qui a produit ce comptage : le même chemin peut en essayer deux
    /// (un PDF nommé `.cbr`).
    format: Format,
    size: u64,
    stamp: u64,
    /// Un livre illisible (`pages` à `None`) est gardé en mémoire seulement,
    /// pour ne pas le rouvrir à chaque scan ; un redémarrage retente.
    count: Count,
}

/// Une ligne de [`FILE`].
#[derive(Serialize, Deserialize)]
struct Stored {
    path: String,
    /// Identifiant du format ; absent des fichiers écrits avant son ajout, on
    /// le déduit alors de l'extension.
    #[serde(default)]
    format: String,
    size: u64,
    stamp: u64,
    pages: usize,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
    file: Option<PathBuf>,
    dirty: bool,
}

impl Cache {
    fn get(&self, path: &str, format: Format, size: u64, stamp: u64) -> Option<&Count> {
        let entry = self.entries.get(path)?;
        (entry.format == format && entry.size == size && entry.stamp == stamp)
            .then_some(&entry.count)
    }

    fn put(&mut self, path: String, entry: Entry) {
        if self.entries.len() >= capacity()
            && !self.entries.contains_key(&path)
            && let Some(victim) = self.entries.keys().next().cloned()
        {
            self.entries.remove(&victim);
        }
        self.dirty |= entry.count.pages.is_some();
        self.entries.insert(path, entry);
    }

    /// Un fichier absent, illisible ou périmé n'est pas une erreur : on recompte.
    fn load(&mut self, file: PathBuf) {
        let stored: Vec<Stored> = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        for line in stored.into_iter().take(capacity()) {
            // Les livres disparus ne sont pas gardés : le fichier ne grossit pas.
            if !Path::new(&line.path).is_file() {
                continue;
            }
            let Some(format) =
                Format::from_id(&line.format).or_else(|| Format::from_path(&line.path))
            else {
                continue;
            };
            self.entries.entry(line.path).or_insert(Entry {
                format,
                size: line.size,
                stamp: line.stamp,
                count: Count {
                    pages: Some(line.pages),
                    problem: None,
                },
            });
        }
        self.file = Some(file);
    }

    fn save(&mut self) {
        let Some(file) = self.file.as_ref().filter(|_| self.dirty) else {
            return;
        };
        // Pas de nouvel essai à chaque scan si l'écriture échoue (dossier en
        // lecture seule) : les comptages restent servis depuis la mémoire.
        self.dirty = false;
        let stored: Vec<Stored> = self
            .entries
            .iter()
            .filter_map(|(path, entry)| {
                Some(Stored {
                    path: path.clone(),
                    format: entry.format.id().to_string(),
                    size: entry.size,
                    stamp: entry.stamp,
                    pages: entry.count.pages?,
                })
            })
            .collect();
        let Ok(json) = serde_json::to_vec(&stored) else {
            return;
        };
        if let Some(dir) = file.parent()
            && std::fs::create_dir_all(dir).is_err()
        {
            return;
        }
        // Écriture dans un fichier temporaire puis renommage : un arrêt en cours
        // d'écriture ne laisse jamais un fichier à moitié écrit.
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_err() || std::fs::rename(&tmp, file).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document, Object, dictionary};

    /// PDF minimal d'une page, écrit sur disque.
    fn pdf_with_one_page(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("une-page.pdf");
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.save(&path).unwrap();
        path
    }

    #[test]
    fn counts_the_pages_of_a_real_book() {
        let dir = tempfile::tempdir().unwrap();
        let path = pdf_with_one_page(dir.path());
        assert_eq!(count(&path, Format::Pdf), Some(1));
    }

    #[test]
    fn an_unreadable_book_has_no_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("casse.cbz");
        std::fs::write(&path, b"pas une archive").unwrap();
        assert_eq!(count(&path, Format::Cbz), None);
    }

    #[test]
    fn an_unreadable_book_carries_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sept.cbz");
        std::fs::write(&path, b"7z\xbc\xaf\x27\x1creste").unwrap();
        let size = std::fs::metadata(&path).unwrap().len();

        let counted = open_stat(&path, Format::Cbz, size, 1);
        assert_eq!(counted.pages, None);
        let problem = counted.problem.expect("une raison");
        assert_eq!(
            problem,
            "le fichier est une archive 7z, pas une archive .cbz"
        );
        assert!(
            !problem.contains(&path.display().to_string()),
            "le chemin est déjà connu de l'appelant : {problem}"
        );
    }

    fn entry(pages: Option<usize>) -> Entry {
        Entry {
            format: Format::Pdf,
            size: 10,
            stamp: 20,
            count: Count {
                pages,
                problem: None,
            },
        }
    }

    #[test]
    fn counts_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("livre.pdf");
        std::fs::write(&book, b"x").unwrap();
        let book = book.display().to_string();
        let file = dir.path().join("cache").join(FILE);

        let mut first = Cache::default();
        first.load(file.clone());
        first.put(book.clone(), entry(Some(42)));
        first.save();

        let mut second = Cache::default();
        second.load(file);
        assert_eq!(
            second
                .get(&book, Format::Pdf, 10, 20)
                .map(|count| count.pages),
            Some(Some(42))
        );
        assert_eq!(
            second.get(&book, Format::Cbr, 10, 20),
            None,
            "format changé"
        );
        assert_eq!(
            second.get(&book, Format::Pdf, 11, 20),
            None,
            "taille changée"
        );
        assert_eq!(second.get(&book, Format::Pdf, 10, 21), None, "date changée");
    }

    #[test]
    fn unreadable_and_vanished_books_are_not_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let kept = dir.path().join("ok.pdf");
        std::fs::write(&kept, b"x").unwrap();
        let kept = kept.display().to_string();
        let gone = dir.path().join("disparu.pdf").display().to_string();
        let broken = dir.path().join("casse.pdf");
        std::fs::write(&broken, b"x").unwrap();
        let broken = broken.display().to_string();
        let file = dir.path().join(FILE);

        let mut first = Cache::default();
        first.load(file.clone());
        first.put(kept.clone(), entry(Some(1)));
        first.put(gone.clone(), entry(Some(2)));
        first.put(broken.clone(), entry(None));
        first.save();

        let mut second = Cache::default();
        second.load(file);
        assert_eq!(
            second
                .get(&kept, Format::Pdf, 10, 20)
                .map(|count| count.pages),
            Some(Some(1))
        );
        assert_eq!(second.get(&gone, Format::Pdf, 10, 20), None);
        assert_eq!(second.get(&broken, Format::Pdf, 10, 20), None);
    }

    #[test]
    fn a_corrupt_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        std::fs::write(&file, b"{pas du json").unwrap();

        let mut cache = Cache::default();
        cache.load(file);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn nothing_is_written_without_news() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);

        let mut cache = Cache::default();
        cache.load(file.clone());
        cache.save();
        assert!(!file.exists());
    }

    #[test]
    fn a_missing_file_has_no_count() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count(&dir.path().join("absent.pdf"), Format::Pdf), None);
    }
}
