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

fn is_hidden(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|rel| {
        rel.components()
            .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_paths_are_recognised() {
        let root = Path::new("/books");
        assert!(is_hidden(root, Path::new("/books/.gitkeep")));
        assert!(is_hidden(root, Path::new("/books/Manga/.DS_Store")));
        assert!(!is_hidden(root, Path::new("/books/Manga/T01.cbz")));
        // Un point dans le nom n'en fait pas un fichier caché.
        assert!(!is_hidden(root, Path::new("/books/T01.epub")));
    }
}
