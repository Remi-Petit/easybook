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
use std::sync::Arc;
use unrar::{Archive, CursorBeforeHeader, OpenArchive, Process};

use crate::archive;
use crate::formats::filecache::FileCache;
use crate::formats::{Problem, is_image, is_noise, natural_cmp, normalize_entry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Zip,
    Rar,
}

/// Noms des planches, dans l'ordre de lecture.
pub fn pages(path: &Path) -> Result<Vec<String>, Problem> {
    let (mut names, files) = match container(path)? {
        Container::Zip => zip_entries(path)?,
        Container::Rar => rar_entries(path)?,
    };
    // Une archive sans image n'est pas une bande dessinée : la compter « 0 page »
    // en ferait un livre sain, ouvert sur un lecteur vide.
    if names.is_empty() {
        return Err(no_pages(&files));
    }
    // L'ordre de l'archive n'est pas garanti : `p2` avant `p10`.
    names.sort_by(|a, b| natural_cmp(a, b));
    Ok(names)
}

/// Raison d'une archive sans planche, en nommant ce qu'elle contient quand il
/// n'y a qu'un fichier (cas réel : un `.cbr` qui est un RAR de sources et d'un
/// PDF, pas une bande dessinée).
fn no_pages(files: &[String]) -> Problem {
    if let Some(book) = files.iter().find(|name| is_document(name)) {
        return Problem::with("comic.no-pages.book", "name", book.clone());
    }
    match files {
        [] => Problem::new("comic.no-pages.empty"),
        [only] => Problem::with("comic.no-pages.only", "name", only.clone()),
        several => Problem::new("comic.no-pages.some")
            .param("count", several.len().to_string())
            .param("name", several[0].clone()),
    }
}

/// Un livre rangé dans une archive de planches : le dire vaut mieux que lister
/// un fichier de sources au hasard.
fn is_document(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [".pdf", ".epub", ".djvu", ".mobi"]
        .iter()
        .any(|ext| name.ends_with(ext))
}

/// Une planche : son nom d'entrée et son contenu.
pub fn page(path: &Path, index: usize) -> Result<(String, Vec<u8>), Problem> {
    let names = cached_pages(path)?;
    let target = names
        .get(index)
        .ok_or_else(|| {
            Problem::new("comic.page-out-of-range")
                .param("index", index.to_string())
                .param("count", names.len().to_string())
        })?
        .clone();

    match container(path)? {
        Container::Zip => zip_page(path, &target),
        Container::Rar => rar_page(path, &target),
    }
}

/// Listes de planches gardées : lister et trier les milliers d'entrées d'un gros
/// CBZ coûte plus que l'envoi d'une planche, et serait refait à chaque page
/// tournée.
static PAGE_LISTS: FileCache<Vec<String>> = FileCache::new(32);

fn cached_pages(path: &Path) -> Result<Arc<Vec<String>>, Problem> {
    PAGE_LISTS.get_or_load(path, || pages(path))
}

/// Type d'archive, d'après les premiers octets.
fn container(path: &Path) -> Result<Container, Problem> {
    let mut head = [0u8; 8];
    let mut file = std::fs::File::open(path).map_err(read_error)?;
    let read = file.read(&mut head).map_err(read_error)?;
    let head = &head[..read];

    if head.starts_with(b"Rar!\x1a\x07") {
        Ok(Container::Rar)
    } else if head.starts_with(b"PK\x03\x04") {
        Ok(Container::Zip)
    } else {
        // Ni ZIP ni RAR : on nomme ce que les octets disent, confronté à
        // l'extension attendue. C'est la raison de l'anomalie à l'écran.
        let expected = path
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned());
        Err(match (kind(head), expected.as_deref()) {
            (Some(code), Some(ext)) => Problem::with(code, "ext", ext),
            (None, Some(ext)) => Problem::with("comic.content-mismatch", "ext", ext),
            // Sans extension annoncée, on ne nomme pas le type : la phrase
            // « pas une archive .<vide> » n'aurait pas de sens.
            _ => Problem::new("comic.not-archive"),
        })
    }
}

/// Une erreur d'entrée-sortie : le texte du système est un détail technique,
/// pas une phrase à traduire. Le chemin n'est pas répété (l'interface sait de
/// quel livre il s'agit, et un chemin absolu de serveur n'a rien à y faire).
fn read_error(error: std::io::Error) -> Problem {
    Problem::with("io.error", "error", error.to_string())
}

/// Ce qu'est réellement le fichier, d'après ses premiers octets.
///
/// Rend le **code du diagnostic** à montrer : la phrase traduite dépend du type
/// réel, mais c'est toujours la même anomalie (« ce n'est pas une archive »).
/// Répond `None` quand ces octets ne disent rien de connu (fichier vide,
/// conteneur exotique) : on ne devine pas.
fn kind(head: &[u8]) -> Option<&'static str> {
    const KNOWN: [(&[u8], &str); 5] = [
        (b"7z\xbc\xaf\x27\x1c", "comic.not-archive.7z"),
        (b"%PDF", "comic.not-archive.pdf"),
        (b"\x1f\x8b", "comic.not-archive.gzip"),
        (b"BZh", "comic.not-archive.bzip2"),
        (b"\xfd7zXZ\x00", "comic.not-archive.xz"),
    ];

    KNOWN
        .iter()
        .find(|(magic, _)| head.starts_with(magic))
        .map(|(_, code)| *code)
}

// ── ZIP ─────────────────────────────────────────────────────────────────────

/// Contenu d'une archive ZIP : ses planches, puis tous ses fichiers (pour
/// expliquer une archive vide de planches).
fn zip_entries(path: &Path) -> Result<(Vec<String>, Vec<String>), Problem> {
    let zip = archive::open(path)?;
    let files: Vec<String> = zip
        .file_names()
        .filter_map(|name| name.ok())
        .filter(|name| !name.ends_with('/') && !is_noise(name))
        .map(|name| name.into_owned())
        .collect();
    let pages = files
        .iter()
        .filter(|name| is_image(name))
        .cloned()
        .collect();
    Ok((pages, files))
}

fn zip_page(path: &Path, target: &str) -> Result<(String, Vec<u8>), Problem> {
    let mut zip = archive::open(path)?;
    let bytes = archive::read_named(&mut zip, target)
        .ok_or_else(|| Problem::with("comic.page-missing", "name", target))?;
    Ok((target.to_string(), bytes))
}

// ── RAR ─────────────────────────────────────────────────────────────────────

/// Contenu d'une archive RAR : ses planches, puis tous ses fichiers (pour
/// expliquer une archive vide de planches).
fn rar_entries(path: &Path) -> Result<(Vec<String>, Vec<String>), Problem> {
    let mut archive = rar_open(path)?;
    let mut files: Vec<String> = Vec::new();

    while let Some(header) = archive.read_header().map_err(rar_error)? {
        // Les archives créées sous Windows portent des `\` : on normalise.
        let name = normalize_entry(&header.entry().filename.to_string_lossy());
        if header.entry().is_file() && !is_noise(&name) {
            files.push(name);
        }
        archive = header.skip().map_err(rar_error)?;
    }

    let pages = files
        .iter()
        .filter(|name| is_image(name))
        .cloned()
        .collect();
    Ok((pages, files))
}

fn rar_page(path: &Path, target: &str) -> Result<(String, Vec<u8>), Problem> {
    let mut archive = rar_open(path)?;

    while let Some(header) = archive.read_header().map_err(rar_error)? {
        let name = normalize_entry(&header.entry().filename.to_string_lossy());
        if header.entry().is_file() && name == target {
            let (bytes, _) = header.read().map_err(rar_error)?;
            return Ok((target.to_string(), bytes));
        }
        archive = header.skip().map_err(rar_error)?;
    }

    Err(Problem::with("comic.page-missing", "name", target))
}

fn rar_open(path: &Path) -> Result<OpenArchive<Process, CursorBeforeHeader>, Problem> {
    Archive::new(path).open_for_processing().map_err(rar_error)
}

fn rar_error(error: unrar::error::UnrarError) -> Problem {
    Problem::with("comic.rar-error", "error", error.to_string())
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

    #[test]
    fn the_page_list_is_kept_but_follows_a_replaced_file() {
        let (_dir, path) = zip_with(&[("p1.jpg", b"a"), ("p2.jpg", b"b")], "serie.cbz");
        assert_eq!(page(&path, 1).unwrap().1, b"b");
        assert_eq!(
            page(&path, 1).unwrap().1,
            b"b",
            "deuxième appel : liste gardée"
        );

        // Même chemin, autre contenu : une autre taille, donc une autre liste.
        let (_other, replacement) = zip_with(
            &[("p1.jpg", b"x"), ("p2.jpg", b"y"), ("p3.jpg", b"zzz")],
            "serie.cbz",
        );
        std::fs::copy(&replacement, &path).unwrap();
        assert_eq!(page(&path, 2).unwrap().1, b"zzz");
        assert!(page(&path, 3).is_err());
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

    /// Cas réel : un `.cbr` qui est un RAR de sources et d'un PDF. Sans planche,
    /// ce n'est pas un livre lisible — et la raison dit ce qu'il contient.
    #[test]
    fn an_archive_without_pages_is_an_error() {
        let (_dir, seul) = zip_with(&[("notes.txt", b"x")], "seul.cbr");
        assert_eq!(
            pages(&seul).unwrap_err(),
            Problem::with("comic.no-pages.only", "name", "notes.txt")
        );

        let (_dir, plusieurs) =
            zip_with(&[("sketch.ino", b"x"), ("livre.pdf", b"y")], "sources.cbz");
        assert_eq!(
            pages(&plusieurs).unwrap_err(),
            Problem::with("comic.no-pages.book", "name", "livre.pdf")
        );

        let (_dir, sources) = zip_with(&[("a.ino", b"x"), ("b.ino", b"y")], "code.cbz");
        assert_eq!(
            pages(&sources).unwrap_err(),
            Problem::new("comic.no-pages.some")
                .param("count", "2")
                .param("name", "a.ino")
        );

        let (_dir, vide) = zip_with(&[("dossier/", b"")], "vide.cbz");
        assert_eq!(
            pages(&vide).unwrap_err(),
            Problem::new("comic.no-pages.empty")
        );
    }

    #[test]
    fn a_file_that_is_not_an_archive_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tome.cbz");
        // Cas réel : des `.cbz` qui sont en fait des archives 7z.
        std::fs::write(&path, b"7z\xbc\xaf\x27\x1c\x00\x04reste").unwrap();
        assert_eq!(
            pages(&path).unwrap_err(),
            Problem::with("comic.not-archive.7z", "ext", "cbz")
        );

        let renamed = dir.path().join("tome.cbr");
        std::fs::write(&renamed, b"%PDF-1.4reste").unwrap();
        assert_eq!(
            pages(&renamed).unwrap_err(),
            Problem::with("comic.not-archive.pdf", "ext", "cbr")
        );

        // Octets méconnus : on ne prononce pas de type, seulement le désaccord.
        let opaque = dir.path().join("bizarre.cbz");
        std::fs::write(&opaque, b"bonjour").unwrap();
        assert_eq!(
            pages(&opaque).unwrap_err(),
            Problem::with("comic.content-mismatch", "ext", "cbz")
        );
    }

    #[test]
    fn page_out_of_range_is_an_error() {
        let (_dir, path) = zip_with(&[("p1.jpg", b"x")], "c.cbz");
        assert!(page(&path, 4).is_err());
    }
}
