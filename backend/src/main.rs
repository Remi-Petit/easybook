//! easybook — serveur de bibliothèque.
//!
//! Le serveur ne fait que deux choses : décrire ce que contient `books/`, et
//! servir le contenu des livres. Le dossier est **surveillé** : un fichier
//! ajouté, renommé ou supprimé est poussé aux navigateurs par WebSocket, sans
//! rechargement de page. Là où le système de fichiers ne remonte rien (dossier
//! d'un hôte monté sous Docker Desktop), un contrôle périodique de l'empreinte
//! du dossier prend le relais : voir [`sweep`].

use easybook::{
    accounts, api, auth, config, formats, library, pages, perf, perf_history, readings, thumbnail,
    watcher,
};
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::Duration;
use tokio::sync::broadcast;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Une sous-commande éventuelle n'ouvre pas le serveur : elle règle la base
    // des comptes et rend la main.
    if let Some(command) = std::env::args().nth(1) {
        return command_line(&command);
    }

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

const USAGE: &str = "\
usage : easybook [commande]

Sans commande, démarre le serveur (interface, API, bibliothèque surveillée).

Commandes :
  admin-reset   réapplique EASYBOOK_ADMIN_EMAIL / EASYBOOK_ADMIN_PASSWORD à la
                base des comptes, puis quitte
  help          affiche cette aide";

/// Commandes hors serveur (`easybook admin-reset`).
fn command_line(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        "admin-reset" => match reset_admin() {
            Ok(()) => Ok(()),
            Err(message) => fail(&message),
        },
        "help" | "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => fail(&format!("commande inconnue : {other}\n\n{USAGE}")),
    }
}

/// Message d'erreur lisible, sans l'habillage `Error: "…"` que Rust ajoute à
/// une erreur rendue par `main`.
fn fail(message: &str) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("easybook : {message}");
    std::process::exit(1)
}

/// `easybook admin-reset` — rejoue l'administrateur désigné par
/// `EASYBOOK_ADMIN_EMAIL` / `EASYBOOK_ADMIN_PASSWORD`.
///
/// Ces variables ne sont lues qu'au tout premier démarrage (base vide) : cette
/// commande les réapplique à la demande, par exemple après une rotation de mot
/// de passe ou un changement d'adresse. Elle ne démarre pas le serveur, ce qui
/// permet de l'exécuter sur le même volume sans arrêter le conteneur :
///
///     docker compose run --rm --entrypoint easybook easybook admin-reset
fn reset_admin() -> Result<(), String> {
    let config = config::Config::load();
    let dir = accounts_dir(&canonical_root(config.resolve_books_root()));
    let path = dir.join("accounts.db");
    let accounts = accounts::Accounts::open(&path).map_err(|e| e.to_string())?;

    let nonblank = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    };
    let email = nonblank("EASYBOOK_ADMIN_EMAIL")
        .ok_or("EASYBOOK_ADMIN_EMAIL est vide : aucune adresse à administrer")?;
    let password = nonblank("EASYBOOK_ADMIN_PASSWORD")
        .ok_or("EASYBOOK_ADMIN_PASSWORD est vide : rien à réinitialiser")?;

    println!("Comptes   : {}", path.display());
    let outcome = accounts
        .reset_admin(&email, &password)
        .map_err(|e| match e {
            accounts::AccountError::EmailOwnedByUser => format!(
                "{email} appartient déjà à un compte qui n'est pas administrateur : \
                 rien n'a été modifié"
            ),
            other => other.to_string(),
        })?;
    match outcome {
        accounts::ResetOutcome::Created(user) => println!(
            "Administrateur créé : {}",
            user.email.as_deref().unwrap_or_default()
        ),
        accounts::ResetOutcome::Updated(user) => println!(
            "Administrateur mis à jour : {} (sessions révoquées)",
            user.email.as_deref().unwrap_or_default()
        ),
    }
    Ok(())
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
    let (ws_tx, _) = broadcast::channel::<axum::extract::ws::Utf8Bytes>(16);
    let state = api::AppState::new(root.clone(), ws_tx);
    // `EASYBOOK_CACHE_DIR` : cache des couvertures hors de `books/` (sinon
    // `books/.easybook/covers`).
    let state = match std::env::var("EASYBOOK_CACHE_DIR") {
        Ok(dir) if !dir.trim().is_empty() => state.with_cache_dir(dir),
        _ => state,
    };

    let state = state.with_data_dir(accounts_dir(&root));
    let state = enable_accounts(state, &root)?;
    let state = attach_history(state, &root);

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
        let warm = warm_tx.clone();
        move || {
            if let Err(e) = watch(root, state, warm) {
                eprintln!("Erreur watcher : {e}");
            }
        }
    });

    // Contrôle de cohérence : voir `SWEEP_INTERVAL`.
    std::thread::spawn({
        let state = state.clone();
        let root = root.clone();
        move || sweep(root, state, warm_tx)
    });

    server.await?;
    Ok(())
}

/// Intervalle du contrôle de cohérence du dossier des livres.
///
/// Le watcher réagit en quelques centaines de millisecondes — mais seulement là
/// où le système de fichiers remonte des événements. Le dossier d'un hôte
/// monté dans un conteneur Docker Desktop (Windows, macOS) n'en remonte
/// **aucun** : ce contrôle compare alors l'empreinte du dossier (voir
/// [`library::fingerprint`]) pour ne pas laisser passer un ajout ou une
/// suppression faits depuis l'hôte.
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

/// Plafond du contrôle, quand un parcours du dossier devient cher : ce thread
/// ne doit jamais consacrer plus d'un vingtième de son temps à surveiller.
const SWEEP_MAX: Duration = Duration::from_secs(30);

/// Ouvre la base des comptes et exige une connexion (`auth.enabled`).
///
/// La base vit dans `EASYBOOK_DATA_DIR` (à monter en volume : sans lui, les
/// comptes disparaissent avec le conteneur), sinon dans `books/.easybook/`.
/// Au premier démarrage, un administrateur est créé : `EASYBOOK_ADMIN_EMAIL`
/// (défaut `admin@easybook.local`) et `EASYBOOK_ADMIN_PASSWORD` — sans mot de passe imposé, il
/// est tiré au hasard et affiché **une seule fois** ici.
fn enable_accounts(
    state: api::AppState,
    root: &std::path::Path,
) -> Result<api::AppState, Box<dyn std::error::Error>> {
    let settings = config::auth();
    if !settings.enabled {
        println!("⚠️  Authentification désactivée (auth.enabled: false) : l'API est ouverte.");
        return Ok(state);
    }

    let dir = accounts_dir(root);
    let path = dir.join("accounts.db");
    let accounts = accounts::Accounts::open(&path)?;
    println!("Comptes   : {}", path.display());

    let nonblank = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let email =
        nonblank("EASYBOOK_ADMIN_EMAIL").unwrap_or_else(|| "admin@easybook.local".to_string());
    if let Some(boot) = accounts.bootstrap_admin(&email, nonblank("EASYBOOK_ADMIN_PASSWORD"))? {
        match boot.generated_password {
            Some(password) => {
                println!(
                    "Administrateur créé : {}, mot de passe : {password}",
                    boot.user.email.as_deref().unwrap_or_default()
                );
                println!("  (affiché une seule fois : changez-le dès la première connexion)");
            }
            None => println!(
                "Administrateur créé : {}",
                boot.user.email.as_deref().unwrap_or_default()
            ),
        }
    }
    if settings.oidc.is_enabled() {
        println!("OIDC      : {}", settings.oidc.issuer);
    }
    let mut auth = auth::AuthState::new(Arc::new(accounts), settings);
    if config::dev_mode() {
        println!("⚠️  EASYBOOK_MODE=dev : outils de développement actifs.");
        auth = auth.with_direct_login();
        if let Ok(logins) = std::env::var("EASYBOOK_DEV_LOGINS") {
            auth = auth.with_dev_logins(&logins);
            println!("  - connexion rapide : boutons de la page de connexion");
        }
        if auth.direct_login() {
            println!(
                "  - connexion directe à OIDC depuis le formulaire (mot de passe vu par easybook)"
            );
        }
    }
    Ok(state.with_auth(auth))
}

/// Ouvre l'historique des performances et lance son échantillonneur.
///
/// La base vit à côté de celle des comptes (`metrics.db`) : même volume, même
/// sauvegarde. Une erreur d'ouverture ne prive pas la page du direct.
fn attach_history(state: api::AppState, root: &std::path::Path) -> api::AppState {
    let settings = config::performance();
    if !settings.enabled {
        println!("Historique : désactivé (performance.enabled: false)");
        return state;
    }
    let path = accounts_dir(root).join("metrics.db");
    match perf_history::History::open(&path, &settings) {
        Ok(history) => {
            println!(
                "Historique : {} (toutes les {} s, {} jours)",
                path.display(),
                settings.sample_secs,
                settings.retention_days
            );
            let history = Arc::new(history);
            spawn_history(state.clone(), Arc::clone(&history), settings.sample_secs);
            state.with_history(history)
        }
        Err(e) => {
            eprintln!("⚠️  Historique indisponible ({e}) : la page reste en direct.");
            state
        }
    }
}

/// Enregistre une mesure toutes les `sample_secs`, avec les mêmes chiffres que
/// la page (voir `api::AppState::detail`).
fn spawn_history(state: api::AppState, history: Arc<perf_history::History>, sample_secs: u64) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(sample_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut previous: Option<perf::Snapshot> = None;
        loop {
            ticker.tick().await;
            let detail = state.detail();
            let Ok(snapshot) = tokio::task::spawn_blocking(move || perf::snapshot(detail)).await
            else {
                continue;
            };
            if let Some(before) = previous.as_ref()
                && let Some(measurement) = perf_history::Measurement::between(before, &snapshot)
                && let Err(e) = history.record(&measurement)
            {
                eprintln!("Historique : écriture impossible ({e})");
            }
            previous = Some(snapshot);
        }
    });
}

/// Dossier de la base des comptes : `EASYBOOK_DATA_DIR`, sinon `books/.easybook`.
///
/// Partagé par le serveur et par `easybook admin-reset`, qui doivent viser la
/// **même** base dans les deux cas.
fn accounts_dir(root: &std::path::Path) -> PathBuf {
    match std::env::var("EASYBOOK_DATA_DIR") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => root.join(".easybook"),
    }
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

/// Compare régulièrement l'empreinte du dossier et rediffuse s'il a changé.
///
/// Filet de sécurité du watcher : un système de fichiers muet (montage d'un
/// dossier hôte sous Docker Desktop) ne prévient de rien. Le parcours ne fait
/// que lire des métadonnées, et il est sauté tant qu'aucun onglet n'écoute :
/// la prochaine comparaison, au retour d'un client, rattrapera les changements.
fn sweep(root: PathBuf, state: api::AppState, warm: std::sync::mpsc::Sender<()>) {
    let mut known = library::fingerprint(&root);
    let mut broadcasts = state.broadcasts();
    let mut pause = SWEEP_INTERVAL;

    loop {
        std::thread::sleep(pause);

        if state.listeners() == 0 {
            continue;
        }

        let started = std::time::Instant::now();
        let seen = library::fingerprint(&root);
        // Un parcours lent (très grosse bibliothèque, disque réseau) espace les
        // contrôles : vingt fois sa durée, entre une seconde et trente.
        pause = (started.elapsed() * 20).clamp(SWEEP_INTERVAL, SWEEP_MAX);

        let latest = state.broadcasts();
        if seen == known || latest != broadcasts {
            // Rien n'a bougé, ou le watcher a déjà rediffusé ce changement :
            // on se resynchronise sans rien renvoyer.
            known = seen;
            broadcasts = latest;
            continue;
        }

        known = seen;
        println!("Changement détecté hors watcher : catalogue rediffusé");
        api::broadcast_snapshot(&state);
        broadcasts = state.broadcasts();
        let _ = warm.send(());
    }
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
