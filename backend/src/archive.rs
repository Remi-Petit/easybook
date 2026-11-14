//! Ouverture des archives ZIP (EPUB et CBZ).
//!
//! Les deux formats sont des archives ZIP : un EPUB range des documents XHTML,
//! un CBZ des images. L'archive est ouverte **par requête** — le système de
//! fichiers est chaud, et un cache de descripteurs compliquerait la gestion des
//! fichiers remplacés pendant la lecture.

use crate::formats;
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

/// Une entrée est-elle « technique » (cachée par le système, artefact d'un
/// ordinateur) plutôt que du contenu ?
fn is_noise(name: &str) -> bool {
    name.starts_with("__MACOSX/")
        || name
            .split('/')
            .any(|segment| segment.starts_with('.') && !segment.is_empty())
}

/// Noms des images d'une bande dessinée, **dans l'ordre de lecture**.
///
/// L'ordre vient d'une comparaison naturelle (`p2.jpg` avant `p10.jpg`) et non
/// de l'ordre de l'archive : les outils de création ne le garantissent pas.
pub fn images(path: &Path) -> Result<Vec<String>, String> {
    let zip = open(path)?;
    let mut names: Vec<String> = zip
        .file_names()
        .filter_map(|name| name.ok())
        .filter(|name| !is_noise(name) && formats::is_image(name))
        .map(|name| name.into_owned())
        .collect();
    names.sort_by(|a, b| formats::natural_cmp(a, b));
    Ok(names)
}

/// Nombre de pages d'une bande dessinée.
pub fn page_count(path: &Path) -> Result<usize, String> {
    Ok(images(path)?.len())
}

/// Une page : son nom dans l'archive et son contenu.
pub fn page(path: &Path, index: usize) -> Result<(String, Vec<u8>), String> {
    let names = images(path)?;
    let name = names
        .get(index)
        .ok_or_else(|| format!("page {index} hors limites ({} pages)", names.len()))?
        .clone();

    let mut zip = open(path)?;
    let bytes = read_named(&mut zip, &name)
        .ok_or_else(|| format!("page {name} introuvable dans l'archive"))?;
    Ok((name, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn fixture(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.cbz");
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
    fn pages_are_sorted_naturally_and_noise_is_ignored() {
        let (_dir, path) = fixture(&[
            ("p10.jpg", b"10"),
            ("p2.jpg", b"2"),
            ("__MACOSX/._p2.jpg", b"x"),
            ("comicinfo.xml", b"<x/>"),
        ]);
        let names = images(&path).unwrap();
        assert_eq!(names, vec!["p2.jpg", "p10.jpg"]);
        assert_eq!(page_count(&path).unwrap(), 2);
    }

    #[test]
    fn reads_entry_regardless_of_case() {
        let (_dir, path) = fixture(&[("Images/Cover.PNG", b"png")]);
        let mut zip = open(&path).unwrap();
        assert_eq!(read_named(&mut zip, "images/cover.png").unwrap(), b"png");
    }
}
