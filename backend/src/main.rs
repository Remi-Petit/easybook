//! easybook — serveur de bibliothèque.
//!
//! Le serveur ne fait que deux choses : décrire ce que contient `books/`, et
//! servir le contenu des livres. Le dossier est **surveillé** : un fichier
//! ajouté, renommé ou supprimé est poussé aux navigateurs par WebSocket, sans
//! rechargement de page et sans sondage.

use easybook::{api, config, formats, watcher};
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let names: Vec<&str> = formats::ALL.iter().map(|f| f.id()).collect();
    println!("Formats reconnus : {}", names.join(", "));

    // Racine de la bibliothèque : `BOOKS_ROOT`, sinon `books_root` de
    // `config.yml` (relatif au dossier `backend/`).
    let config = config::Config::load();
    let resolved = config.resolve_books_root();
    // Canonicalisée quand possible : évite les `..` dans les journaux et fiabilise
    // les tests de préfixe du watcher.
    let root = canonical_root(resolved);

    // Un dossier absent n'empêche pas de démarrer : on le crée, ce qui rend le
    // montage Docker immédiatement utilisable.
    if !root.is_dir() {
        match std::fs::create_dir_all(&root) {
            Ok(()) => println!("Bibliothèque créée : {}", root.display()),
            Err(e) => eprintln!(
                "⚠️  {} est introuvable et n'a pas pu être créé : {e}",
                root.display()
            ),
        }
    }

    println!("Bibliothèque : {}", root.display());

    // Canal broadcast : le catalogue sérialisé (JSON) part à tous les clients WS.
    let (ws_tx, _) = broadcast::channel::<String>(16);
    let state = api::AppState::new(root.clone(), ws_tx);

    // API + WebSocket sur le pool Tokio.
    let server = tokio::spawn({
        let state = state.clone();
        async move {
            if let Err(e) = api::serve(state).await {
                eprintln!("Erreur serveur : {e}");
            }
        }
    });

    // Watcher : thread dédié (`notify` bloque le thread qui l'appelle).
    std::thread::spawn({
        let state = state.clone();
        let root = root.clone();
        move || {
            if let Err(e) = watch(root, state) {
                eprintln!("Erreur watcher : {e}");
            }
        }
    });

    server.await?;
    Ok(())
}

/// Chemin canonique, sans le préfixe « verbatim » de Windows (`\\?\C:\…`).
///
/// `std::fs::canonicalize` rend un chemin verbatim sous Windows, alors que les
/// événements du watcher portent le chemin normal. Comparés tels quels, les
/// préfixes diffèrent et **aucun** changement ne serait reconnu : le temps réel
/// cesserait de fonctionner sous Windows, sans erreur visible.
fn canonical_root(path: PathBuf) -> PathBuf {
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    #[cfg(windows)]
    {
        // Import local : `Path` ne sert que dans ce bloc, et un `use` en tête de
        // fichier serait « inutilisé » sur les autres systèmes (clippy le refuse).
        use std::path::Path;

        if let Some(text) = path.to_str()
            && let Some(stripped) = text.strip_prefix(r"\\?\")
            && Path::new(stripped).is_absolute()
        {
            return PathBuf::from(stripped);
        }
    }
    path
}

/// Surveille la bibliothèque et rediffuse le catalogue à chaque changement.
fn watch(root: PathBuf, state: api::AppState) -> notify::Result<()> {
    let (tx, rx) = channel();

    // Le debouncer regroupe la rafale d'événements d'une même action (copier un
    // dossier produit des dizaines de créations) et n'émet qu'un lot stable.
    let mut debouncer = new_debouncer(
        Duration::from_millis(50),
        None,
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    )?;
    debouncer.watch(&root, RecursiveMode::Recursive)?;

    println!("Je surveille {} (Ctrl+C pour arrêter)", root.display());

    let mut last_message: Option<String> = None;

    loop {
        let events = match rx.recv() {
            Ok(Ok(events)) => events,
            Ok(Err(errors)) => {
                for error in errors {
                    eprintln!("Erreur watcher : {error}");
                }
                continue;
            }
            // Le canal est fermé : plus personne n'écoute, on s'arrête.
            Err(_) => break,
        };

        // Un gros fichier en cours d'écriture produit un lot par bloc : on ne
        // répète pas deux fois de suite la même ligne.
        for message in events.iter().filter_map(watcher::describe_event) {
            if last_message.as_deref() != Some(message.as_str()) {
                println!("{message}");
                last_message = Some(message);
            }
        }

        if watcher::touches_library(&root, &events) {
            api::broadcast_snapshot(&state);
        }
    }

    // Le debouncer est gardé en vie jusqu'ici : sa destruction arrête la
    // surveillance.
    drop(debouncer);
    Ok(())
}
