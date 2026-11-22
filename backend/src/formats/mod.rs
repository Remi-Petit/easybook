//! Formats de livres : **un module par format**.
//!
//! Le catalogue s'appuie sur l'**extension** pour savoir ce qu'est un fichier,
//! et l'ouvre **une seule fois** pour compter ses pages ([`crate::pages`] garde
//! ce comptage). Le reste du contenu n'est lu qu'au moment où on ouvre le livre.
//!
//! Chaque format implémente [`BookFormat`] et s'enregistre dans [`module`]. Tout
//! ce que l'API sait demander à un livre passe par ce trait — ajouter un format
//! (`.mobi`, `.fb2`…) tient donc en trois gestes :
//! 1. un fichier dans ce dossier, avec son implémentation ;
//! 2. une variante dans [`Format`] (extension, type MIME) ;
//! 3. une ligne dans [`module`] (et dans [`ALL`] pour l'annonce au démarrage).

mod cbr;
mod cbz;
mod comic;
mod epub;
mod filecache;
mod pdf;

use serde::Serialize;
use std::cmp::Ordering;
use std::io::Read;
use std::path::Path;

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
    /// Document PDF (rendu par le navigateur, image extraite pour la couverture).
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
        module(self).id()
    }

    /// Format d'après son identifiant public — l'inverse de [`Format::id`].
    pub fn from_id(id: &str) -> Option<Self> {
        ALL.into_iter().find(|format| format.id() == id)
    }

    /// `true` si ce serveur sait afficher le contenu du livre.
    pub fn readable(self) -> bool {
        module(self).readable()
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

/// Ce qu'un format sait faire.
///
/// Toutes les méthodes ont une valeur par défaut : un format qui ne sait pas
/// faire quelque chose ne l'implémente pas (plutôt que de répondre par une
/// erreur métier). L'API demande, le format répond « je ne sais pas ».
pub trait BookFormat: Send + Sync {
    /// Identifiant public, celui du JSON (`epub`, `cbz`, `cbr`, `pdf`).
    fn id(&self) -> &'static str;

    /// `true` si le contenu peut être affiché par l'interface.
    fn readable(&self) -> bool {
        true
    }

    /// Explication à montrer quand `readable()` est faux, ou précision utile.
    fn note(&self) -> Option<&'static str> {
        None
    }

    /// Titre, auteur et chapitres — les formats qui en ont un.
    fn outline(&self, _path: &Path) -> Result<Option<Outline>, String> {
        Ok(None)
    }

    /// Nombre de pages — les formats paginés (bande dessinée, PDF). Pour un
    /// EPUB, c'est le nombre de chapitres du `spine` : le texte se recompose à
    /// l'écran, il n'a pas de pages fixes.
    fn pages(&self, _path: &Path) -> Result<Option<usize>, String> {
        Ok(None)
    }

    /// Image de couverture **telle qu'elle est dans le livre** (non réduite).
    fn cover(&self, _path: &Path) -> Option<Vec<u8>> {
        None
    }

    /// Ressource interne (document EPUB : image, CSS, police…).
    fn asset(&self, _path: &Path, _inner: &str) -> Option<Vec<u8>> {
        None
    }

    /// Document d'un chapitre, prêt à être affiché (URL réécrites).
    fn chapter(
        &self,
        _path: &Path,
        _index: usize,
        _book_rel: &str,
        _asset_base: &str,
    ) -> Result<String, String> {
        Err(format!("{} : pas de chapitres", self.id()))
    }

    /// Une page illustrée : nom de l'entrée et contenu.
    fn page(&self, _path: &Path, _index: usize) -> Result<(String, Vec<u8>), String> {
        Err(format!("{} : pas de pages", self.id()))
    }
}

/// Registre : le module qui sait ouvrir ce format.
pub fn module(format: Format) -> &'static dyn BookFormat {
    match format {
        Format::Epub => &epub::EPUB,
        Format::Cbz => &cbz::CBZ,
        Format::Cbr => &cbr::CBR,
        Format::Pdf => &pdf::PDF,
    }
}

/// Module d'un chemin, si son extension est reconnue.
pub fn from_path(path: &str) -> Option<&'static dyn BookFormat> {
    Format::from_path(path).map(module)
}

/// Format d'un fichier **existant** : l'extension le déclare, les premiers
/// octets peuvent le corriger.
///
/// Un PDF porte une signature sans ambiguïté (`%PDF`) : quand un fichier
/// annoncé sous une autre extension commence par elle, c'est un PDF, et c'est
/// le module PDF qui doit l'ouvrir. Le cas est réel — des `.cbr` téléchargés en
/// ligne sont parfois des PDF. Les autres écarts de nom restent des anomalies
/// signalées avec leur raison (voir [`comic`]) : on ne devine pas.
///
/// À utiliser pour tout fichier du disque ; `Format::from_path` reste la règle
/// pure, celle du nom (dépôt, validation).
pub fn resolve(path: &Path, rel: &str) -> Option<Format> {
    let declared = Format::from_path(rel)?;
    if declared != Format::Pdf && starts_with(path, b"%PDF") {
        return Some(Format::Pdf);
    }
    Some(declared)
}

/// `true` si le fichier commence par `magic` (faux s'il est absent ou illisible).
fn starts_with(path: &Path, magic: &[u8]) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = vec![0u8; magic.len()];
    file.read_exact(&mut head).is_ok() && head == magic
}

/// Un chapitre du sommaire, dans l'ordre de lecture.
#[derive(Debug, Clone, Serialize)]
pub struct Chapter {
    /// Position dans le sommaire (0 = première entrée).
    pub index: usize,
    /// Libellé de la table des matières, ou un repli (« Chapitre N »).
    pub title: String,
    /// Chemin du document **dans l'archive** (pas une URL).
    pub href: String,
}

/// Sommaire d'un livre.
#[derive(Debug, Clone, Default)]
pub struct Outline {
    pub title: Option<String>,
    pub author: Option<String>,
    pub chapters: Vec<Chapter>,
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

/// Entrée d'archive qui n'est pas du contenu : artefacts du système de fichiers
/// de l'auteur (`__MACOSX/`, `.DS_Store`, dossiers cachés).
pub fn is_noise(name: &str) -> bool {
    name.starts_with("__MACOSX/")
        || name
            .split('/')
            .any(|segment| segment.starts_with('.') && !segment.is_empty())
}

/// Nom normalisé d'une entrée d'archive (séparateurs `/`).
pub fn normalize_entry(name: &str) -> String {
    name.replace('\\', "/")
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
    fn formats_round_trip_through_their_ids() {
        for format in ALL {
            assert_eq!(Format::from_id(format.id()), Some(format));
        }
        assert_eq!(Format::from_id("txt"), None);
    }

    #[test]
    fn a_pdf_under_another_extension_is_opened_as_a_pdf() {
        let dir = tempfile::tempdir().unwrap();

        // Cas réel : un `.cbr` téléchargé qui est en fait un PDF.
        let path = dir.path().join("Wakfu Heroes.cbr");
        std::fs::write(&path, b"%PDF-1.7\nreste").unwrap();
        assert_eq!(resolve(&path, "Wakfu Heroes.cbr"), Some(Format::Pdf));

        // Une vraie bande dessinée garde son format.
        let cbz = dir.path().join("T1.cbz");
        std::fs::write(&cbz, b"PK\x03\x04reste").unwrap();
        assert_eq!(resolve(&cbz, "T1.cbz"), Some(Format::Cbz));

        // Un fichier absent n'est pas deviné : l'extension décide.
        let missing = dir.path().join("absent.cbr");
        assert_eq!(resolve(&missing, "absent.cbr"), Some(Format::Cbr));

        // Une extension inconnue reste inconnue, même avec une signature PDF.
        let odd = dir.path().join("doc.txt");
        std::fs::write(&odd, b"%PDF-1.7\n").unwrap();
        assert_eq!(resolve(&odd, "doc.txt"), None);
    }

    #[test]
    fn every_format_has_a_module() {
        for format in ALL {
            let module = module(format);
            assert_eq!(module.id(), format.id());
            assert!(!format.content_type().is_empty());
        }
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

    #[test]
    fn noise_is_recognised() {
        assert!(is_noise("__MACOSX/._p1.jpg"));
        assert!(is_noise("Manga/.DS_Store"));
        assert!(!is_noise("Manga/p1.jpg"));
        assert!(!is_noise("p1.jpg"));
    }

    #[test]
    fn archive_entries_use_forward_slashes() {
        assert_eq!(normalize_entry("Manga\\T1\\p1.jpg"), "Manga/T1/p1.jpg");
        assert_eq!(normalize_entry("Manga/T1/p1.jpg"), "Manga/T1/p1.jpg");
    }

    #[test]
    fn resources_get_the_right_mime_type() {
        assert_eq!(
            content_type("OEBPS/Text/chap1.xhtml"),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type("OEBPS/Styles/style.CSS"),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type("OEBPS/Images/cover.jpeg"), "image/jpeg");
        assert_eq!(content_type("OEBPS/Images/cover.png"), "image/png");
        assert_eq!(content_type("OEBPS/Fonts/livre.woff2"), "font/woff2");
        assert_eq!(
            content_type("OEBPS/toc.ncx"),
            "application/xml; charset=utf-8"
        );
        // Extensions inconnues et noms sans extension : flux binaire.
        assert_eq!(content_type("OEBPS/notes.xyz"), "application/octet-stream");
        assert_eq!(content_type("OEBPS/LICENSE"), "application/octet-stream");
    }

    #[test]
    fn natural_order_compares_numbers_by_value() {
        use std::cmp::Ordering;

        assert_eq!(natural_cmp("T02", "T2"), Ordering::Equal);
        assert_eq!(natural_cmp("T2", "T10"), Ordering::Less);
        assert_eq!(natural_cmp("T10", "T2"), Ordering::Greater);
        // La casse ne départage pas deux noms identiques.
        assert_eq!(natural_cmp("manga", "MANGA"), Ordering::Equal);
        // Un préfixe plus court passe avant.
        assert_eq!(natural_cmp("T1", "T1 bis"), Ordering::Less);
    }
}
