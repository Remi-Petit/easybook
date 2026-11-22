//! Petit cache par fichier, pour ce qu'on relirait sinon à chaque requête.
//!
//! Une entrée vaut pour un chemin, une taille et une date : un livre remplacé
//! est un autre livre, et son entrée est refaite. Le nombre d'entrées est
//! borné : au-delà, l'une d'elles est évincée à chaque ajout.

use crate::problem::Problem;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

type Key = (PathBuf, u64, u64);

pub struct FileCache<V> {
    entries: Mutex<Option<HashMap<Key, Arc<V>>>>,
    capacity: usize,
}

impl<V> FileCache<V> {
    pub const fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(None),
            capacity,
        }
    }

    /// L'entrée de ce fichier, calculée par `load` si elle manque. Une erreur de
    /// `load` n'est pas gardée.
    pub fn get_or_load(
        &self,
        path: &Path,
        load: impl FnOnce() -> Result<V, Problem>,
    ) -> Result<Arc<V>, Problem> {
        let meta = std::fs::metadata(path)
            .map_err(|e| Problem::with("io.error", "error", e.to_string()))?;
        let stamp = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_secs());
        let key = (path.to_path_buf(), meta.len(), stamp);

        if let Some(hit) = self
            .entries
            .lock()
            .ok()
            .and_then(|entries| entries.as_ref()?.get(&key).cloned())
        {
            return Ok(hit);
        }

        let value = Arc::new(load()?);
        if let Ok(mut entries) = self.entries.lock() {
            let entries = entries.get_or_insert_with(HashMap::new);
            if entries.len() >= self.capacity
                && let Some(victim) = entries.keys().next().cloned()
            {
                entries.remove(&victim);
            }
            entries.insert(key, Arc::clone(&value));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn loads_once_and_follows_a_replaced_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("livre.bin");
        std::fs::write(&path, b"un").unwrap();

        let cache: FileCache<usize> = FileCache::new(4);
        let loads = Cell::new(0);
        let load = || {
            loads.set(loads.get() + 1);
            Ok(loads.get())
        };
        assert_eq!(*cache.get_or_load(&path, load).unwrap(), 1);
        assert_eq!(*cache.get_or_load(&path, load).unwrap(), 1, "gardée");

        std::fs::write(&path, b"autre taille").unwrap();
        assert_eq!(*cache.get_or_load(&path, load).unwrap(), 2, "refaite");
    }

    #[test]
    fn errors_are_not_kept_and_the_size_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let cache: FileCache<u8> = FileCache::new(2);

        let missing = dir.path().join("absent");
        assert!(cache.get_or_load(&missing, || Ok(0)).is_err());

        let path = dir.path().join("a");
        std::fs::write(&path, b"a").unwrap();
        assert!(
            cache
                .get_or_load(&path, || Err(Problem::new("test.failure")))
                .is_err()
        );
        assert_eq!(*cache.get_or_load(&path, || Ok(7)).unwrap(), 7);

        for name in ["b", "c", "d"] {
            let other = dir.path().join(name);
            std::fs::write(&other, name).unwrap();
            cache.get_or_load(&other, || Ok(1)).unwrap();
        }
        let held = cache.entries.lock().unwrap().as_ref().unwrap().len();
        assert_eq!(held, 2);
    }
}
