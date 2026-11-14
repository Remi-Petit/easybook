//! Bande dessinée ZIP (`.cbz`) : une archive d'images, une page par image.
//!
//! Le conteneur réel est détecté par [`super::comic`] (ZIP ou RAR) : un fichier
//! `.cbz` qui contient un RAR se lit aussi — l'extension n'est pas une preuve.

use std::path::Path;

use crate::formats::{BookFormat, comic};

/// Format CBZ, tel que l'enregistre [`super::module`].
pub struct Cbz;

/// Instance unique du format.
pub static CBZ: Cbz = Cbz;

impl BookFormat for Cbz {
    fn id(&self) -> &'static str {
        "cbz"
    }

    fn pages(&self, path: &Path) -> Result<Option<usize>, String> {
        Ok(Some(comic::pages(path)?.len()))
    }

    fn cover(&self, path: &Path) -> Option<Vec<u8>> {
        // Convention du format : la première page est la couverture.
        comic::page(path, 0).ok().map(|(_, bytes)| bytes)
    }

    fn page(&self, path: &Path, index: usize) -> Result<(String, Vec<u8>), String> {
        comic::page(path, index)
    }
}
