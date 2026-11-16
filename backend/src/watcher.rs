//! Interprétation des événements du système de fichiers.
//!
//! Le dossier des livres est surveillé par `notify` (via un debouncer) : ici, on
//! décide ce qui mérite une rediffusion du catalogue. La logique est séparée du
//! watcher lui-même pour être testable sans toucher au disque.

use notify::EventKind;
use notify::event::ModifyKind;
use notify_debouncer_full::DebouncedEvent;
use std::path::Path;

/// `true` si l'événement traduit un **vrai changement** : création,
/// modification, renommage ou suppression.
///
/// Les événements d'**accès** sont écartés : `notify` les émet à chaque
/// ouverture de fichier, et les traiter comme un changement relancerait une
/// diffusion à chaque lecture d'EPUB — une boucle.
pub fn is_content_change(event: &DebouncedEvent) -> bool {
    match &event.kind {
        EventKind::Access(_) | EventKind::Other => false,
        // Métadonnées seules (droits, horodatage) : pas un changement de contenu.
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        _ => true,
    }
}

/// Traduit un événement en message lisible (journal du serveur).
pub fn describe_event(event: &DebouncedEvent) -> Option<String> {
    let path = event
        .paths
        .first()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    match &event.kind {
        EventKind::Create(_) => Some(format!("Ajout : {path}")),
        EventKind::Remove(_) => Some(format!("Suppression : {path}")),
        EventKind::Modify(ModifyKind::Name(_)) => Some(format!("Renommage : {path}")),
        EventKind::Modify(ModifyKind::Metadata(_)) => None,
        EventKind::Modify(_) => Some(format!("Modification : {path}")),
        _ => None,
    }
}

/// `true` si ce lot d'événements change la bibliothèque.
///
/// Un élément **caché** (`.gitkeep`, fichiers d'éditeur) ne compte pas : il
/// n'apparaît pas dans le catalogue, le rediffuser serait du bruit.
pub fn touches_library(root: &Path, events: &[DebouncedEvent]) -> bool {
    events.iter().any(|event| {
        is_content_change(event)
            && event
                .paths
                .iter()
                .any(|path| path.starts_with(root) && !is_hidden(root, path))
    })
}

pub fn is_hidden(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|rel| {
        rel.components()
            .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::Event;
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, RenameMode};
    use std::path::PathBuf;
    use std::time::Instant;

    /// Événement déjà « debouncé », comme celui que rend `notify`.
    fn event(kind: EventKind, path: &str) -> DebouncedEvent {
        let raw = Event::new(kind).add_path(PathBuf::from(path));
        DebouncedEvent::new(raw, Instant::now())
    }

    #[test]
    fn hidden_paths_are_recognised() {
        let root = Path::new("/books");
        assert!(is_hidden(root, Path::new("/books/.gitkeep")));
        assert!(is_hidden(root, Path::new("/books/Manga/.DS_Store")));
        assert!(!is_hidden(root, Path::new("/books/Manga/T01.cbz")));
        // Un point dans le nom n'en fait pas un fichier caché.
        assert!(!is_hidden(root, Path::new("/books/T01.epub")));
    }

    #[test]
    fn reading_a_book_is_not_a_change() {
        // `notify` émet un événement d'accès à chaque ouverture : le traiter
        // comme un changement relancerait une diffusion à chaque page lue.
        assert!(!is_content_change(&event(
            EventKind::Access(AccessKind::Read),
            "/books/T1.cbz"
        )));
        assert!(!is_content_change(&event(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            "/books/T1.cbz"
        )));
        assert!(!is_content_change(&event(EventKind::Other, "/books")));
    }

    #[test]
    fn writes_and_renames_are_changes() {
        assert!(is_content_change(&event(
            EventKind::Create(CreateKind::File),
            "/books/T1.cbz"
        )));
        assert!(is_content_change(&event(
            EventKind::Remove(notify::event::RemoveKind::File),
            "/books/T1.cbz"
        )));
        assert!(is_content_change(&event(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/books/T1.cbz"
        )));
        assert!(is_content_change(&event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
            "/books/T1.cbz"
        )));
    }

    #[test]
    fn events_are_described_in_french() {
        let describe = |kind| describe_event(&event(kind, "/books/T1.cbz"));

        assert_eq!(
            describe(EventKind::Create(CreateKind::File)),
            Some("Ajout : /books/T1.cbz".to_string())
        );
        assert_eq!(
            describe(EventKind::Remove(notify::event::RemoveKind::File)),
            Some("Suppression : /books/T1.cbz".to_string())
        );
        assert_eq!(
            describe(EventKind::Modify(ModifyKind::Name(RenameMode::Both))),
            Some("Renommage : /books/T1.cbz".to_string())
        );
        assert_eq!(
            describe(EventKind::Modify(ModifyKind::Data(DataChange::Any))),
            Some("Modification : /books/T1.cbz".to_string())
        );
        // Rien à annoncer : un accès, des métadonnées, un événement inconnu.
        assert_eq!(
            describe(EventKind::Access(AccessKind::Close(AccessMode::Any))),
            None
        );
        assert_eq!(
            describe(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any))),
            None
        );
        assert_eq!(describe(EventKind::Other), None);
    }

    #[test]
    fn only_events_inside_the_library_count() {
        let root = Path::new("/books");

        assert!(touches_library(
            root,
            &[event(EventKind::Create(CreateKind::File), "/books/T1.cbz")]
        ));
        // Hors de la racine (autre dossier, ou préfixe qui ne correspond pas).
        assert!(!touches_library(
            root,
            &[event(
                EventKind::Create(CreateKind::File),
                "/ailleurs/T1.cbz"
            )]
        ));
        assert!(!touches_library(
            root,
            &[event(
                EventKind::Create(CreateKind::File),
                "/books-autres/T1.cbz"
            )]
        ));
        // Caché : absent du catalogue, rien à rediffuser.
        assert!(!touches_library(
            root,
            &[event(
                EventKind::Create(CreateKind::File),
                "/books/.gitkeep"
            )]
        ));
        // Un lot où **un** événement compte suffit.
        assert!(touches_library(
            root,
            &[
                event(EventKind::Access(AccessKind::Read), "/books/T1.cbz"),
                event(EventKind::Create(CreateKind::File), "/books/T2.cbz"),
            ]
        ));
        // Lot vide : rien à faire.
        assert!(!touches_library(root, &[]));
    }
}
