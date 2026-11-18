//! easybook — serveur de bibliothèque.
//!
//! Le serveur ne fait que deux choses : décrire ce que contient `books/`, et
//! servir le contenu des livres. Le dossier est **surveillé** : un fichier
//! ajouté, renommé ou supprimé est poussé aux navigateurs par WebSocket, sans
//! rechargement de page et sans sondage.

use easybook::{api, config, formats, pages, readings, thumbnail, watcher};
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;
use tokio::sync::broadcast;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Réglage de l'allocateur avant la création des threads du runtime.
    thumbnail::tune_allocator();

    // La configuration détermine la taille du runtime : elle est lue avant lui.
    let config = config::Config::load();
    config::install(config.clone());

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(config::limits().max_blocking_threads)
        .build()?
        .block_on(run(config))
}

async fn run(config: config::Config) -> Result<(), Box<dyn std::error::Error>> {
    let names: Vec<&str> = formats::ALL.iter().map(|f| f.id()).collect();
    println!("Formats reconnus : {}", names.join(", "));

    // Racine de la bibliothèque : `BOOKS_ROOT`, sinon `books_root` de
    // `config.yml` (relatif au dossier `backend/`).
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
    // `EASYBOOK_CACHE_DIR` : cache des couvertures hors de `books/` (sinon
    // `books/.easybook/covers`).
    let state = match std::env::var("EASYBOOK_CACHE_DIR") {
        Ok(dir) if !dir.trim().is_empty() => state.with_cache_dir(dir),
        _ => state,
    };

    // Les comptages de pages survivent au redémarrage (sinon chaque PDF est
    // relu en entier au premier scan).
    pages::persist_in(&state.cache_dir());
    // Une lecture terminée rend son cache de fichiers sans attendre qu'il manque
    // de place.
    readings::spawn_sweeper();

    // API + WebSocket sur le pool Tokio.
    let server = tokio::spawn({
        let state = state.clone();
        async move {
            if let Err(e) = api::serve(state).await {
                eprintln!("Erreur serveur : {e}");
            }
        }
    });

    // Préchauffage des couvertures : un thread dédié, déclenché au démarrage
    // puis à chaque changement de la bibliothèque. Les demandes qui arrivent
    // pendant un passage sont fusionnées en un seul passage suivant.
    let (warm_tx, warm_rx) = channel::<()>();
    std::thread::spawn({
        let root = root.clone();
        let cache_dir = state.cache_dir();
        move || {
            while warm_rx.recv().is_ok() {
                while warm_rx.try_recv().is_ok() {}
                let started = std::time::Instant::now();
                let report = thumbnail::warm(&root, &cache_dir);
                if report.made + report.removed > 0 {
                    println!(
                        "Couvertures : {} créée(s), {} déjà en cache, {} sans couverture, {} supprimée(s) ({:.1} s)",
                        report.made,
                        report.kept,
                        report.missing,
                        report.removed,
                        started.elapsed().as_secs_f32()
                    );
                }
            }
        }
    });
    let _ = warm_tx.send(());

    // Watcher : thread dédié (`notify` bloque le thread qui l'appelle).
    std::thread::spawn({
        let state = state.clone();
        let root = root.clone();
        move || {
            if let Err(e) = watch(root, state, warm_tx) {
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
fn watch(
    root: PathBuf,
    state: api::AppState,
    warm: std::sync::mpsc::Sender<()>,
) -> notify::Result<()> {
    let (tx, rx) = channel();

    // Le debouncer regroupe la rafale d'événements d'une même action (copier un
    // dossier produit des dizaines de créations) et n'émet qu'un lot stable.
    // Son thread se réveille à chaque `tick` : à `None` (timeout / 4), c'est 80
    // réveils par seconde, même sans aucun changement.
    let mut debouncer = new_debouncer(
        Duration::from_millis(500),
        Some(Duration::from_millis(250)),
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
        // Un élément caché (le cache des couvertures, `.DS_Store`) n'est pas un
        // livre : on ne le journalise pas.
        let visible = events
            .iter()
            .filter(|event| !event.paths.iter().all(|p| watcher::is_hidden(&root, p)));
        for message in visible.filter_map(watcher::describe_event) {
            if last_message.as_deref() != Some(message.as_str()) {
                println!("{message}");
                last_message = Some(message);
            }
        }

        if watcher::touches_library(&root, &events) {
            api::broadcast_snapshot(&state);
            let _ = warm.send(());
        }
    }

    // Le debouncer est gardé en vie jusqu'ici : sa destruction arrête la
    // surveillance.
    drop(debouncer);
    Ok(())
}
