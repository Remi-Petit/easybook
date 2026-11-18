//! Nombre de pages d'un livre : comptage à la demande, avec cache mémoire.
//!
//! Le catalogue annonce ce nombre, donc chaque livre doit être ouvert au moins
//! une fois — une lecture d'en-têtes d'archive, ou du document pour un PDF. Le
//! cache évite de le refaire à chaque scan : un scan a lieu à chaque changement
//! du dossier, pas seulement au démarrage.
//!
//! La clé contient le chemin, la taille et la date du fichier : un livre
//! remplacé est un autre livre, et son comptage est refait.

use crate::formats::{self, Format};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// Nombre de comptages gardés. Au-delà, une entrée est évincée à chaque ajout :
/// vider tout le cache obligerait à rouvrir chaque livre au scan suivant.
const CAPACITY: usize = 4096;

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
    let key = format!("{}|{}|{stamp}", path.display(), meta.len());

    if let Some(cached) = cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).copied())
    {
        return cached;
    }

    let value = formats::module(format).pages(path).ok().flatten();
    if let Ok(mut cache) = cache().lock() {
        if cache.len() >= CAPACITY
            && let Some(victim) = cache.keys().next().cloned()
        {
            cache.remove(&victim);
        }
        cache.insert(key, value);
    }
    value
}

/// Nombre de comptages en cache.
pub fn cache_entries() -> usize {
    cache().lock().map_or(0, |cache| cache.len())
}

fn cache() -> &'static Mutex<HashMap<String, Option<usize>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<usize>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
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
    fn a_missing_file_has_no_count() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count(&dir.path().join("absent.pdf"), Format::Pdf), None);
    }
}
