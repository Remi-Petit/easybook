//! Planches d'une archive : **ZIP** (`.cbz`) ou **RAR** (`.cbr`).
//!
//! En pratique les deux se rencontrent sous la mauvaise extension : beaucoup de
//! `.cbr` distribués en ligne sont des ZIP renommés. On regarde donc les
//! **octets** du fichier — pas son nom — pour choisir le lecteur d'archive.
//!
//! Un RAR se lit séquentiellement (unrar n'offre pas d'accès aléatoire) : on
//! parcourt les en-têtes pour lister les pages, puis on refait un passage pour
//! extraire celle qu'on veut.

use std::io::Read;
use std::path::Path;
use unrar::{Archive, CursorBeforeHeader, OpenArchive, Process};

use crate::archive;
use crate::formats::{is_image, is_noise, natural_cmp, normalize_entry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Zip,
    Rar,
}

/// Noms des planches, dans l'ordre de lecture.
pub fn pages(path: &Path) -> Result<Vec<String>, String> {
    let mut names = match container(path)? {
        Container::Zip => zip_pages(path)?,
        Container::Rar => rar_pages(path)?,
    };
    // L'ordre de l'archive n'est pas garanti : `p2` avant `p10`.
    names.sort_by(|a, b| natural_cmp(a, b));
    Ok(names)
}

/// Une planche : son nom d'entrée et son contenu.
pub fn page(path: &Path, index: usize) -> Result<(String, Vec<u8>), String> {
    let names = pages(path)?;
    let target = names
        .get(index)
        .ok_or_else(|| format!("page {index} hors limites ({} pages)", names.len()))?
        .clone();

    match container(path)? {
        Container::Zip => zip_page(path, &target),
        Container::Rar => rar_page(path, &target),
    }
}

/// Type d'archive, d'après les premiers octets.
fn container(path: &Path) -> Result<Container, String> {
    let mut head = [0u8; 8];
    let mut file = std::fs::File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let read = file
        .read(&mut head)
        .map_err(|e| format!("{} : {e}", path.display()))?;
    let head = &head[..read];

    if head.starts_with(b"Rar!\x1a\x07") {
        Ok(Container::Rar)
    } else if head.starts_with(b"PK\x03\x04") {
        Ok(Container::Zip)
    } else {
        Err(format!("{} : archive ni ZIP ni RAR", path.display()))
    }
}

// ── ZIP ─────────────────────────────────────────────────────────────────────

fn zip_pages(path: &Path) -> Result<Vec<String>, String> {
    let zip = archive::open(path)?;
    Ok(zip
        .file_names()
        .filter_map(|name| name.ok())
        .filter(|name| !is_noise(name) && is_image(name))
        .map(|name| name.into_owned())
        .collect())
}

fn zip_page(path: &Path, target: &str) -> Result<(String, Vec<u8>), String> {
    let mut zip = archive::open(path)?;
    let bytes = archive::read_named(&mut zip, target)
        .ok_or_else(|| format!("page {target} introuvable dans l'archive"))?;
    Ok((target.to_string(), bytes))
}

// ── RAR ─────────────────────────────────────────────────────────────────────

fn rar_pages(path: &Path) -> Result<Vec<String>, String> {
    let mut archive = rar_open(path)?;
    let mut names: Vec<String> = Vec::new();

    while let Some(header) = archive.read_header().map_err(rar_error)? {
        // Les archives créées sous Windows portent des `\` : on normalise.
        let name = normalize_entry(&header.entry().filename.to_string_lossy());
        if header.entry().is_file() && !is_noise(&name) && is_image(&name) {
            names.push(name);
        }
        archive = header.skip().map_err(rar_error)?;
    }

    Ok(names)
}

fn rar_page(path: &Path, target: &str) -> Result<(String, Vec<u8>), String> {
    let mut archive = rar_open(path)?;

    while let Some(header) = archive.read_header().map_err(rar_error)? {
        let name = normalize_entry(&header.entry().filename.to_string_lossy());
        if header.entry().is_file() && name == target {
            let (bytes, _) = header.read().map_err(rar_error)?;
            return Ok((target.to_string(), bytes));
        }
        archive = header.skip().map_err(rar_error)?;
    }

    Err(format!("page {target} introuvable dans l'archive"))
}

fn rar_open(path: &Path) -> Result<OpenArchive<Process, CursorBeforeHeader>, String> {
    Archive::new(path).open_for_processing().map_err(rar_error)
}

fn rar_error(error: unrar::error::UnrarError) -> String {
    format!("archive RAR : {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// Archive ZIP de test.
    pub(crate) fn zip_with(
        entries: &[(&str, &[u8])],
        name: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (entry, bytes) in entries {
            zip.start_file(*entry, SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn zip_pages_are_sorted_naturally_and_noise_is_ignored() {
        let (_dir, path) = zip_with(
            &[
                ("p10.jpg", b"10"),
                ("p2.jpg", b"2"),
                ("__MACOSX/._p2.jpg", b"x"),
                ("comicinfo.xml", b"<x/>"),
            ],
            "test.cbz",
        );
        assert_eq!(pages(&path).unwrap(), vec!["p2.jpg", "p10.jpg"]);
        assert_eq!(page(&path, 1).unwrap().1, b"10");
    }

    /// Cas réel : un `.cbr` qui est en fait un ZIP.
    #[test]
    fn a_zip_named_cbr_is_read_as_a_zip() {
        let (_dir, path) = zip_with(&[("p1.jpg", b"zip")], "renomme.cbr");
        assert_eq!(pages(&path).unwrap(), vec!["p1.jpg"]);
        assert_eq!(page(&path, 0).unwrap().1, b"zip");
    }

    /// Les vrais RAR ne sont pas écrits par ce crate : on vérifie au moins que
    /// l'en-tête RAR est reconnu comme tel (et échoue proprement s'il est tronqué).
    #[test]
    fn rar_signature_selects_the_rar_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("faux.cbr");
        std::fs::write(&path, b"Rar!\x1a\x07\x00tronque").unwrap();
        assert_eq!(container(&path).unwrap(), Container::Rar);
        assert!(
            pages(&path).is_err(),
            "archive incomplète : erreur, pas panique"
        );
    }

    #[test]
    fn a_random_file_is_not_a_comic_archive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("texte.cbz");
        std::fs::write(&path, b"bonjour").unwrap();
        assert!(pages(&path).is_err());
    }

    #[test]
    fn page_out_of_range_is_an_error() {
        let (_dir, path) = zip_with(&[("p1.jpg", b"x")], "c.cbz");
        assert!(page(&path, 4).is_err());
    }
}
