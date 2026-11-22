//! Bande dessinée RAR (`.cbr`).
//!
//! Même contenu qu'un CBZ (des images, une par page), dans une archive RAR.
//! Le conteneur réel est détecté par [`super::comic`] : beaucoup de `.cbr`
//! distribués en ligne sont en fait des ZIP renommés, et ils se lisent aussi.
//!
//! Unrar lit **séquentiellement** : lister les pages parcourt les en-têtes,
//! puis extraire une page refait un passage (voir [`super::comic`]).

use std::path::Path;

use crate::formats::{BookFormat, Problem, comic};

/// Format CBR, tel que l'enregistre [`super::module`].
pub struct Cbr;

/// Instance unique du format.
pub static CBR: Cbr = Cbr;

impl BookFormat for Cbr {
    fn id(&self) -> &'static str {
        "cbr"
    }

    fn pages(&self, path: &Path) -> Result<Option<usize>, Problem> {
        Ok(Some(comic::pages(path)?.len()))
    }

    fn cover(&self, path: &Path) -> Option<Vec<u8>> {
        comic::page(path, 0).ok().map(|(_, bytes)| bytes)
    }

    fn page(&self, path: &Path, index: usize) -> Result<(String, Vec<u8>), Problem> {
        comic::page(path, index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_zip_named_cbr_is_still_readable() {
        // Cas réel : les `.cbr` téléchargés sont souvent des ZIP.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("renomme.cbr");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("p1.jpg", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"page").unwrap();
        zip.finish().unwrap();

        assert_eq!(CBR.pages(&path).unwrap(), Some(1));
        assert_eq!(CBR.cover(&path).unwrap(), b"page");
    }
}
