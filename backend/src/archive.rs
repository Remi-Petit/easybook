//! Ouverture des archives ZIP (EPUB et CBZ).
//!
//! Les deux formats sont des archives ZIP : un EPUB range des documents XHTML,
//! un CBZ des images. Ouvrir une archive relit son annuaire (une entrée par
//! fichier), ce qui pèse sur un CBZ de plusieurs centaines de planches : les
//! archives sont donc gardées ouvertes dans un petit réservoir
//! (`limits.open_archives`), et une lecture en **emprunte** une.
//!
//! Une archive empruntée n'est à personne d'autre : deux lectures simultanées du
//! même livre en ouvrent chacune une. Un livre remplacé (taille ou date
//! différente) n'est jamais servi depuis une ancienne archive.

use crate::config;
use std::fs::File;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;
use zip::ZipArchive;

struct Slot {
    path: PathBuf,
    size: u64,
    stamp: u64,
    zip: ZipArchive<File>,
}

/// Archives libres, de la moins récemment rendue à la plus récente.
static POOL: Mutex<Vec<Slot>> = Mutex::new(Vec::new());

/// Archive empruntée au réservoir ; rendue en fin de lecture.
pub struct Borrowed {
    slot: Option<Slot>,
}

impl Deref for Borrowed {
    type Target = ZipArchive<File>;

    fn deref(&self) -> &Self::Target {
        &self
            .slot
            .as_ref()
            .expect("archive présente jusqu'au drop")
            .zip
    }
}

impl DerefMut for Borrowed {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self
            .slot
            .as_mut()
            .expect("archive présente jusqu'au drop")
            .zip
    }
}

impl Drop for Borrowed {
    fn drop(&mut self) {
        let Some(slot) = self.slot.take() else {
            return;
        };
        let capacity = config::limits().open_archives;
        if capacity == 0 {
            return;
        }
        let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        pool.push(slot);
        let excess = pool.len().saturating_sub(capacity);
        pool.drain(..excess);
    }
}

/// Ouvre une archive en lecture (ou en emprunte une déjà ouverte).
pub fn open(path: &Path) -> Result<Borrowed, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let size = meta.len();
    let stamp = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_nanos() as u64);

    {
        let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        // Une version périmée de ce livre ne servira plus : on libère son
        // descripteur (le fichier supprimé garderait sinon son espace disque).
        pool.retain(|slot| slot.path != path || (slot.size, slot.stamp) == (size, stamp));
        if let Some(at) = pool
            .iter()
            .rposition(|slot| slot.path == path && (slot.size, slot.stamp) == (size, stamp))
        {
            return Ok(Borrowed {
                slot: Some(pool.remove(at)),
            });
        }
    }

    let file = File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let zip = ZipArchive::new(file).map_err(|e| format!("archive illisible : {e}"))?;
    Ok(Borrowed {
        slot: Some(Slot {
            path: path.to_path_buf(),
            size,
            stamp,
            zip,
        }),
    })
}

/// Lit une entrée par son nom exact.
///
/// `by_name` exige `&mut` : la lecture avance dans le flux de l'archive.
pub fn read(zip: &mut ZipArchive<File>, name: &str) -> Option<Vec<u8>> {
    let mut entry = zip.by_name(name).ok()?;
    if entry.is_dir() {
        return None;
    }
    let mut buffer = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buffer).ok()?;
    Some(buffer)
}

/// Lit une entrée en tolérant la casse et les variantes de chemin.
///
/// Les `href` d'un EPUB sont des URL : ils sont percent-encodés (`Mon%20livre`)
/// et peuvent désigner une entrée écrite avec une autre casse (`images/P1.jpg`
/// pour `Images/p1.jpg`). On essaie le nom tel quel, puis les variantes
/// décodées, puis une recherche insensible à la casse.
pub fn read_named(zip: &mut ZipArchive<File>, name: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = read(zip, name) {
        return Some(bytes);
    }
    let decoded = crate::rewrite::decode(name);
    if decoded != name
        && let Some(bytes) = read(zip, &decoded)
    {
        return Some(bytes);
    }

    // Dernier recours : parcours des noms. Évite de perdre un livre dont l'OPF
    // annonce `Text/chap1.xhtml` alors que l'archive contient `text/Chap1.xhtml`.
    let wanted = decoded.to_ascii_lowercase();
    let actual = zip
        .file_names()
        .filter_map(|name| name.ok())
        .find(|candidate| candidate.to_ascii_lowercase() == wanted)
        .map(|candidate| candidate.into_owned())?;
    read(zip, &actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.zip");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn reads_entry_regardless_of_case() {
        let (_dir, path) = fixture(&[("Images/Cover.PNG", b"png")]);
        let mut zip = open(&path).unwrap();
        assert_eq!(read_named(&mut zip, "images/cover.png").unwrap(), b"png");
    }

    #[test]
    fn reads_percent_encoded_entry_names() {
        let (_dir, path) = fixture(&[("Mon livre/p1.jpg", b"page")]);
        let mut zip = open(&path).unwrap();
        assert_eq!(
            read_named(&mut zip, "Mon%20livre%2Fp1.jpg").unwrap(),
            b"page"
        );
    }

    #[test]
    fn missing_entry_is_none() {
        let (_dir, path) = fixture(&[("a.jpg", b"x")]);
        let mut zip = open(&path).unwrap();
        assert!(read_named(&mut zip, "b.jpg").is_none());
    }

    fn pooled(path: &Path) -> usize {
        POOL.lock()
            .unwrap()
            .iter()
            .filter(|slot| slot.path == path)
            .count()
    }

    #[test]
    fn a_returned_archive_is_kept_and_borrowed_again() {
        let (_dir, path) = fixture(&[("a.jpg", b"x")]);
        drop(open(&path).unwrap());
        assert_eq!(pooled(&path), 1);

        let mut again = open(&path).unwrap();
        assert_eq!(pooled(&path), 0, "empruntée : plus dans le réservoir");
        assert_eq!(read_named(&mut again, "a.jpg").unwrap(), b"x");
    }

    #[test]
    fn a_replaced_book_is_not_served_from_the_old_archive() {
        let (_dir, path) = fixture(&[("a.jpg", b"ancien")]);
        drop(open(&path).unwrap());

        let (_other, replacement) = fixture(&[("a.jpg", b"nouveau, plus long")]);
        std::fs::copy(&replacement, &path).unwrap();

        let mut zip = open(&path).unwrap();
        assert_eq!(
            read_named(&mut zip, "a.jpg").unwrap(),
            b"nouveau, plus long"
        );
        drop(zip);
        assert_eq!(pooled(&path), 1, "l'ancienne archive a été écartée");
    }
}
