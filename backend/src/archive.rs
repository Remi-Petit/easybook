//! Ouverture des archives ZIP (EPUB et CBZ).
//!
//! Les deux formats sont des archives ZIP : un EPUB range des documents XHTML,
//! un CBZ des images. L'archive est ouverte **par requête** — le système de
//! fichiers est chaud, et un cache de descripteurs compliquerait la gestion des
//! fichiers remplacés pendant la lecture.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

/// Ouvre une archive en lecture.
pub fn open(path: &Path) -> Result<ZipArchive<File>, String> {
    let file = File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    ZipArchive::new(file).map_err(|e| format!("archive illisible : {e}"))
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
}
