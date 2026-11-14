//! Formats de livres reconnus.
//!
//! Le catalogue ne s'appuie que sur l'**extension** : ouvrir chaque fichier
//! pour savoir ce qu'il est coûterait une lecture d'archive par entrée, à chaque
//! scan. Le contenu n'est lu qu'au moment d'ouvrir un livre.

use serde::Serialize;
use std::cmp::Ordering;

/// Formats reconnus, dans l'ordre où ils sont annoncés au démarrage.
pub const ALL: [Format; 4] = [Format::Epub, Format::Cbz, Format::Cbr, Format::Pdf];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Livre numérique (archive ZIP : XHTML + images + CSS).
    Epub,
    /// Bande dessinée (archive ZIP d'images).
    Cbz,
    /// Bande dessinée (archive RAR).
    Cbr,
    /// Document PDF (affiché par le lecteur du navigateur).
    Pdf,
}

impl Format {
    /// Format déduit du nom de fichier, `None` si l'extension est inconnue.
    pub fn from_path(path: &str) -> Option<Self> {
        let name = path.rsplit(['/', '\\']).next()?;
        let (stem, ext) = name.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        match ext.to_ascii_lowercase().as_str() {
            "epub" => Some(Self::Epub),
            "cbz" => Some(Self::Cbz),
            "cbr" => Some(Self::Cbr),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }

    /// Identifiant public (`epub`, `cbz`, `cbr`, `pdf`), celui du JSON.
    pub fn id(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Cbz => "cbz",
            Self::Cbr => "cbr",
            Self::Pdf => "pdf",
        }
    }

    /// `true` si ce serveur sait en afficher le contenu.
    ///
    /// CBR est une archive **RAR** : la décompresser demanderait un décodeur
    /// dédié (ou le binaire `unrar`). Le livre reste listé — il est bien dans le
    /// dossier — et l'interface explique pourquoi il ne s'ouvre pas encore.
    pub fn readable(self) -> bool {
        !matches!(self, Self::Cbr)
    }

    /// Type MIME du fichier lui-même (réponse de `/api/raw`).
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Epub => "application/epub+zip",
            Self::Cbz => "application/vnd.comicbook+zip",
            Self::Cbr => "application/vnd.comicbook-rar",
            Self::Pdf => "application/pdf",
        }
    }
}

/// `true` si le nom désigne une image affichable par un navigateur.
pub fn is_image(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "bmp"
    )
}

/// Type MIME d'une ressource **à l'intérieur** d'un livre (image, CSS, police…).
pub fn content_type(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "xhtml" | "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "ncx" | "xml" | "opf" => "application/xml; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "vtt" => "text/vtt; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Comparaison « naturelle » : `T02` avant `T10`, contrairement à l'ordre
/// lexicographique.
///
/// Sert au catalogue (dossiers, livres) comme aux pages d'une bande dessinée :
/// les planches s'appellent `1.jpg`, `2.jpg`, … `10.jpg`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);

    while i < a.len() && j < b.len() {
        let (da, db) = (a[i].is_ascii_digit(), b[j].is_ascii_digit());

        if da && db {
            // Deux suites de chiffres : comparées par valeur, pas caractère par
            // caractère (`09` et `9` sont le même nombre).
            let (na, end_a) = digits(&a[i..]);
            let (nb, end_b) = digits(&b[j..]);
            let (na, nb) = (trim_zeros(na), trim_zeros(nb));
            let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if ord != Ordering::Equal {
                return ord;
            }
            // `digits` rend une position **relative** à la tranche.
            i += end_a;
            j += end_b;
        } else {
            let ord = a[i].to_ascii_lowercase().cmp(&b[j].to_ascii_lowercase());
            if ord != Ordering::Equal {
                return ord;
            }
            i += 1;
            j += 1;
        }
    }

    (a.len() - i).cmp(&(b.len() - j))
}

/// Suite de chiffres en tête de `bytes`, et l'indice qui la suit.
fn digits(bytes: &[u8]) -> (&[u8], usize) {
    let end = bytes
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(bytes.len());
    (&bytes[..end], end)
}

fn trim_zeros(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| *b != b'0').unwrap_or(bytes.len());
    &bytes[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_extensions_case_insensitively() {
        assert_eq!(Format::from_path("a/B.CBZ"), Some(Format::Cbz));
        assert_eq!(Format::from_path("livre.epub"), Some(Format::Epub));
        assert_eq!(Format::from_path("doc.pdf"), Some(Format::Pdf));
        assert_eq!(Format::from_path("manga.cbr"), Some(Format::Cbr));
        assert_eq!(Format::from_path("note.txt"), None);
        // Un nom sans extension n'est pas un livre.
        assert_eq!(Format::from_path("dossier/.hidden"), None);
    }

    #[test]
    fn natural_order_puts_ten_after_two() {
        let mut names = vec!["T10.cbz", "T2.cbz", "T1.cbz", "T02.cbz"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["T1.cbz", "T2.cbz", "T02.cbz", "T10.cbz"]);
    }

    #[test]
    fn image_detection() {
        assert!(is_image("OEBPS/Images/p1.JPG"));
        assert!(!is_image("OEBPS/style.css"));
        assert!(!is_image("comicinfo.xml"));
    }
}
