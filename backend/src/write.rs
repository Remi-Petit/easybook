//! **Écriture** dans la bibliothèque : créer une catégorie, déposer un livre.
//!
//! Le catalogue était jusqu'ici en lecture seule : on rangeait ses livres dans
//! `books/` à la main. Ces fonctions ajoutent le pendant « écriture » en gardant
//! la même règle : le disque est la seule vérité, et un nom de dossier ou de
//! fichier est **validé** avant d'être touché.
//!
//! La validation est ici, pure : l'API ne fait que traduire une erreur en code
//! HTTP (voir [`WriteError::status`]).
//!
//! Rien n'est écrit directement à sa place finale : un dépôt passe par un
//! fichier temporaire **caché** (préfixe `.`, donc ignoré par le scan), renommé
//! une fois complet. Un envoi interrompu ne laisse ainsi jamais un livre
//! tronqué dans le catalogue.

use crate::formats::{self, Format};
use std::path::{Path, PathBuf};

/// Nom maximal d'un dossier ou d'un fichier créé depuis l'interface.
///
/// Les systèmes de fichiers en acceptent plus, mais un nom de 300 caractères ne
/// s'affiche nulle part : c'est presque toujours une erreur de collage.
const MAX_NAME: usize = 120;

/// Ce qui empêche une écriture, et le code HTTP qui va avec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// Nom ou chemin inutilisable (400).
    Invalid(String),
    /// Le dossier de destination n'existe pas (404).
    Missing(String),
    /// Un dossier ou un livre porte déjà ce nom (409).
    Exists(String),
    /// Le disque a refusé (500).
    Failed(String),
}

impl WriteError {
    /// Code HTTP correspondant.
    pub fn status(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        match self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::Missing(_) => StatusCode::NOT_FOUND,
            Self::Exists(_) => StatusCode::CONFLICT,
            Self::Failed(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Message destiné au client.
    pub fn message(&self) -> &str {
        match self {
            Self::Invalid(m) | Self::Missing(m) | Self::Exists(m) | Self::Failed(m) => m,
        }
    }
}

/// Nom de dossier acceptable, ou l'explication.
///
/// Le nom est **un seul segment** : ni séparateur, ni `..`, ni point initial
/// (le scan ignore les entrées cachées — un dossier créé là serait invisible).
pub fn folder_name(raw: &str) -> Result<String, WriteError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(WriteError::Invalid("nom vide".to_string()));
    }
    if name.chars().count() > MAX_NAME {
        return Err(WriteError::Invalid(format!(
            "nom trop long ({MAX_NAME} caractères au plus)"
        )));
    }
    if name.starts_with('.') {
        return Err(WriteError::Invalid(
            "un nom ne peut pas commencer par un point".to_string(),
        ));
    }
    if name.contains(['/', '\\']) || name.contains("..") {
        return Err(WriteError::Invalid(
            "un nom ne peut pas contenir de séparateur ni « .. »".to_string(),
        ));
    }
    // Caractères refusés par Windows, et qui rendraient le nom intapable dans
    // une URL ou un shell.
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(WriteError::Invalid(
            "caractère interdit dans un nom".to_string(),
        ));
    }
    Ok(name.to_string())
}

/// Nom de livre acceptable : un segment, avec une extension de format reconnu.
pub fn book_name(raw: &str) -> Result<(String, Format), WriteError> {
    let name = raw.trim().to_string();
    if name.is_empty() {
        return Err(WriteError::Invalid("nom vide".to_string()));
    }
    if name.chars().count() > MAX_NAME {
        return Err(WriteError::Invalid(format!(
            "nom trop long ({MAX_NAME} caractères au plus)"
        )));
    }
    if name.starts_with('.') {
        return Err(WriteError::Invalid(
            "un nom ne peut pas commencer par un point".to_string(),
        ));
    }
    if name.contains(['/', '\\']) || name.contains("..") {
        return Err(WriteError::Invalid(
            "un nom ne peut pas contenir de séparateur ni « .. »".to_string(),
        ));
    }
    let format = Format::from_path(&name).ok_or_else(|| {
        WriteError::Invalid("format non reconnu (EPUB, CBZ, CBR ou PDF)".to_string())
    })?;
    Ok((name, format))
}

/// Crée un dossier dans `root/parent`, et rend son chemin relatif.
///
/// `parent` est un chemin relatif déjà validé par l'appelant (il vient de
/// [`crate::api::file_in`]-like) ; `""` désigne la racine du catalogue.
pub fn create_folder(root: &Path, parent: &str, raw_name: &str) -> Result<String, WriteError> {
    let name = folder_name(raw_name)?;
    let dir = if parent.is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent)
    };

    if !dir.is_dir() {
        return Err(WriteError::Missing(format!(
            "dossier parent introuvable : {parent}"
        )));
    }

    let path = dir.join(&name);
    if path.exists() {
        return Err(WriteError::Exists(format!("« {name} » existe déjà")));
    }

    // `create_dir` et non `create_dir_all` : si le parent a disparu entre-temps,
    // on veut une erreur, pas une arborescence inventée.
    std::fs::create_dir(&path).map_err(|e| WriteError::Failed(format!("{name} : {e}")))?;

    Ok(relative_of(root, &path))
}

/// Chemin temporaire d'un dépôt, dans le dossier de destination.
///
/// Le préfixe `.` est ce qui le tient hors du catalogue : le scan ignore les
/// entrées cachées, donc un envoi en cours n'apparaît jamais à moitié.
pub fn incoming_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "livre".to_string());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    target.with_file_name(format!(".incoming-{stamp}-{name}"))
}

/// Déplace un dépôt terminé à sa place définitive.
///
/// Refuse d'écraser un livre existant : déposer deux fois le même fichier est
/// une erreur visible, pas un remplacement silencieux.
pub fn commit_incoming(incoming: &Path, target: &Path) -> Result<(), WriteError> {
    if target.exists() {
        let _ = std::fs::remove_file(incoming);
        let name = target
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        return Err(WriteError::Exists(format!("« {name} » existe déjà")));
    }
    std::fs::rename(incoming, target).map_err(|e| {
        let _ = std::fs::remove_file(incoming);
        WriteError::Failed(format!("{} : {e}", target.display()))
    })
}

/// Chemin relatif à `root`, séparateurs `/` (le format du catalogue).
fn relative_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Le nom est-il un fichier caché (donc ignoré par le scan) ?
pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
}

/// Le fichier a-t-il un nom de livre déposable ?
pub fn is_depositable(name: &str) -> bool {
    formats::Format::from_path(name).is_some() && !is_hidden_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_name_is_one_plain_segment() {
        assert_eq!(folder_name("  Mangas  ").unwrap(), "Mangas");
        assert_eq!(folder_name("Bande dessinée").unwrap(), "Bande dessinée");
        assert_eq!(folder_name("Sci-Fi 2000").unwrap(), "Sci-Fi 2000");

        assert!(folder_name("").is_err());
        assert!(folder_name("   ").is_err());
        // Un point initial créerait un dossier invisible : refusé.
        assert!(folder_name(".cache").is_err());
        // Séparateurs et remontée.
        assert!(folder_name("a/b").is_err());
        assert!(folder_name("a\\b").is_err());
        assert!(folder_name("..").is_err());
        assert!(folder_name("a..b").is_err());
        // Caractères que Windows refuse.
        assert!(folder_name("a:b").is_err());
        assert!(folder_name("a?b").is_err());
        assert!(folder_name("a\u{0}b").is_err());
        // Trop long.
        assert!(folder_name(&"a".repeat(MAX_NAME + 1)).is_err());
    }

    #[test]
    fn a_book_name_needs_a_known_extension() {
        assert_eq!(book_name("T01.CBZ").unwrap().1, Format::Cbz);
        assert_eq!(book_name("livre.epub").unwrap().0, "livre.epub");
        assert!(book_name("notes.txt").is_err());
        assert!(book_name("sans-extension").is_err());
        assert!(book_name("mangas/T1.cbz").is_err());
        assert!(book_name(".cache.cbz").is_err());
    }

    #[test]
    fn creating_a_folder_twice_is_a_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let rel = create_folder(root, "", "Mangas").unwrap();
        assert_eq!(rel, "Mangas");
        assert!(root.join("Mangas").is_dir());

        assert!(matches!(
            create_folder(root, "", "Mangas"),
            Err(WriteError::Exists(_))
        ));

        // Sous-dossier.
        let nested = create_folder(root, "Mangas", "Chainsaw Man").unwrap();
        assert_eq!(nested, "Mangas/Chainsaw Man");
    }

    #[test]
    fn creating_inside_a_missing_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            create_folder(dir.path(), "Absent", "Série"),
            Err(WriteError::Missing(_))
        ));
    }

    #[test]
    fn an_upload_lands_only_once_it_is_complete() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("T1.cbz");
        let incoming = incoming_path(&target);

        // Le fichier temporaire porte un nom caché : hors catalogue.
        assert!(is_hidden_name(
            incoming.file_name().unwrap().to_string_lossy().as_ref()
        ));

        std::fs::write(&incoming, b"contenu").unwrap();
        assert!(
            !target.exists(),
            "rien à sa place définitive avant le renommage"
        );

        commit_incoming(&incoming, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"contenu");
        assert!(!incoming.exists(), "le temporaire a disparu");
    }

    #[test]
    fn an_existing_book_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("T1.cbz");
        std::fs::write(&target, b"ancien").unwrap();

        let incoming = incoming_path(&target);
        std::fs::write(&incoming, b"nouveau").unwrap();

        assert!(matches!(
            commit_incoming(&incoming, &target),
            Err(WriteError::Exists(_))
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"ancien");
        assert!(!incoming.exists(), "le temporaire est nettoyé");
    }

    #[test]
    fn only_known_formats_are_depositable() {
        assert!(is_depositable("T1.cbz"));
        assert!(is_depositable("guide.PDF"));
        assert!(!is_depositable("notes.txt"));
        assert!(!is_depositable(".caché.cbz"));
    }
}
