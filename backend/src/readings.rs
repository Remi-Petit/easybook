//! Lectures en cours : quels livres sont ouverts, et ce qu'ils occupent en mémoire.
//!
//! Une lecture commence à la première requête de contenu d'un livre (chapitre,
//! page, ressource, fichier brut) et s'arrête quand il ne demande plus rien
//! depuis [`IDLE`]. À ce moment, le cache de fichiers que le noyau garde pour ce
//! livre est rendu ([`release`]) : sans cela, il resterait en mémoire tant que le
//! système n'en manque pas, c'est-à-dire indéfiniment sur une machine qui a de la
//! place.
//!
//! La mémoire d'une lecture est celle du livre dans le cache de fichiers
//! (`mincore`), toutes lectures confondues : le noyau ne sait pas la distinguer
//! par lecteur. Hors Linux (glibc), elle est inconnue et rien n'est rendu.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Silence après lequel une lecture est considérée comme terminée.
pub const IDLE: Duration = Duration::from_secs(60);
/// Période de vérification des lectures terminées.
const SWEEP: Duration = Duration::from_secs(5);
/// Silence après lequel les pages du cache média (nginx) sont rendues : les
/// fichiers restent sur disque, seule leur copie mémoire part.
const MEDIA_IDLE: Duration = Duration::from_secs(600);
/// Un tour sur douze (5 s × 12) : parcourir l'arborescence est plus coûteux que
/// vérifier deux ou trois livres.
const MEDIA_EVERY: u32 = 12;
/// Cache média de nginx (voir `docker/nginx.conf`, `proxy_cache_path`).
const MEDIA_DIR: &str = "/tmp/nginx-cache";

static RELEASED_BYTES: AtomicU64 = AtomicU64::new(0);
static RELEASED_BOOKS: AtomicU64 = AtomicU64::new(0);
static RELEASED_MEDIA: AtomicU64 = AtomicU64::new(0);

struct Reading {
    rel: String,
    started: Instant,
    last: Instant,
    requests: u64,
    served: u64,
}

#[derive(Default)]
struct Registry {
    live: HashMap<PathBuf, Reading>,
}

impl Registry {
    fn touch(&mut self, path: &Path, rel: &str, bytes: u64, now: Instant) {
        let reading = self
            .live
            .entry(path.to_path_buf())
            .or_insert_with(|| Reading {
                rel: rel.to_string(),
                started: now,
                last: now,
                requests: 0,
                served: 0,
            });
        reading.last = now;
        reading.requests += 1;
        reading.served += bytes;
    }

    /// Retire les lectures silencieuses depuis [`IDLE`] et rend leurs chemins.
    fn expire(&mut self, now: Instant) -> Vec<PathBuf> {
        let done: Vec<PathBuf> = self
            .live
            .iter()
            .filter(|(_, reading)| now.duration_since(reading.last) >= IDLE)
            .map(|(path, _)| path.clone())
            .collect();
        for path in &done {
            self.live.remove(path);
        }
        done
    }

    fn is_active(&self, path: &Path, now: Instant) -> bool {
        self.live
            .get(path)
            .is_some_and(|reading| now.duration_since(reading.last) < IDLE)
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

/// Note qu'une requête de contenu vient de servir `bytes` octets de ce livre.
pub fn touch(path: &Path, rel: &str, bytes: u64) {
    if let Ok(mut registry) = registry().lock() {
        registry.touch(path, rel, bytes, Instant::now());
    }
}

/// Rend le cache des livres dont la lecture est terminée.
pub fn sweep() {
    let done = registry()
        .lock()
        .map(|mut registry| registry.expire(Instant::now()))
        .unwrap_or_default();
    for path in done {
        release(&path);
    }
}

/// Lance la vérification périodique des lectures terminées.
pub fn spawn_sweeper() {
    let _ = std::thread::Builder::new()
        .name("readings".into())
        .spawn(|| {
            let mut round = 0_u32;
            loop {
                std::thread::sleep(SWEEP);
                sweep();
                round = round.wrapping_add(1);
                if round % MEDIA_EVERY == 0 {
                    sweep_media();
                }
            }
        });
}

/// Rend le cache d'un livre lu une fois (comptage des pages, couverture), sauf
/// si quelqu'un le lit en ce moment.
pub fn release_if_idle(path: &Path) {
    let busy = registry()
        .lock()
        .is_ok_and(|registry| registry.is_active(path, Instant::now()));
    if !busy {
        release(path);
    }
}

/// Une lecture en cours.
#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Active {
    pub rel: String,
    /// Taille du fichier.
    pub size: u64,
    /// Part du fichier présente dans le cache de fichiers ; `None` hors Linux.
    pub cached_bytes: Option<u64>,
    pub requests: u64,
    pub served_bytes: u64,
    /// Secondes depuis la dernière requête.
    pub idle_secs: u64,
    /// Secondes depuis la première requête.
    pub started_secs: u64,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub active: Vec<Active>,
    /// Silence après lequel le cache d'un livre est rendu.
    pub idle_limit_secs: u64,
    /// Cache rendu au système depuis le démarrage, mesuré avant/après.
    pub released_bytes: u64,
    pub released_books: u64,
    /// Pages du cache média de nginx rendues de la même façon.
    pub released_media_bytes: u64,
}

pub fn overview() -> Overview {
    let now = Instant::now();
    let mut active: Vec<(PathBuf, Active)> = registry()
        .lock()
        .map(|registry| {
            registry
                .live
                .iter()
                .filter(|(path, _)| registry.is_active(path, now))
                .map(|(path, reading)| {
                    (
                        path.clone(),
                        Active {
                            rel: reading.rel.clone(),
                            requests: reading.requests,
                            served_bytes: reading.served,
                            idle_secs: now.duration_since(reading.last).as_secs(),
                            started_secs: now.duration_since(reading.started).as_secs(),
                            ..Active::default()
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    // Hors du verrou : `mincore` interroge le noyau fichier par fichier.
    for (path, reading) in &mut active {
        let path = path.as_path();
        reading.size = std::fs::metadata(path).map_or(0, |meta| meta.len());
        reading.cached_bytes = resident_bytes(path);
    }
    let mut active: Vec<Active> = active.into_iter().map(|(_, reading)| reading).collect();
    active.sort_by_key(|reading| reading.idle_secs);

    Overview {
        active,
        idle_limit_secs: IDLE.as_secs(),
        released_bytes: RELEASED_BYTES.load(Ordering::Relaxed),
        released_books: RELEASED_BOOKS.load(Ordering::Relaxed),
        released_media_bytes: RELEASED_MEDIA.load(Ordering::Relaxed),
    }
}

/// Octets du fichier présents dans le cache de fichiers du noyau.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn resident_bytes(path: &Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let len = usize::try_from(file.metadata().ok()?.len()).ok()?;
    if len == 0 {
        return Some(0);
    }
    // SAFETY: le mapping n'est jamais lu ; `mincore` interroge le noyau sans
    // toucher au contenu, un fichier tronqué ne peut donc pas provoquer de SIGBUS.
    let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
    // SAFETY: appel sans pointeur.
    let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
        .ok()?
        .max(1);
    let mut resident = vec![0u8; len.div_ceil(page)];
    // SAFETY: `resident` compte une case par page du mapping de `len` octets.
    let status =
        unsafe { libc::mincore(map.as_ptr().cast_mut().cast(), len, resident.as_mut_ptr()) };
    (status == 0).then(|| {
        let pages = resident.iter().filter(|byte| **byte & 1 == 1).count();
        ((pages * page).min(len)) as u64
    })
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn resident_bytes(_path: &Path) -> Option<u64> {
    None
}

/// Rend le cache d'un livre lu : compte les octets et le livre.
pub fn release(path: &Path) {
    let freed = drop_pages(path);
    if freed > 0 {
        RELEASED_BYTES.fetch_add(freed, Ordering::Relaxed);
        RELEASED_BOOKS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Cache média de nginx : ses fichiers sont des copies de planches et de
/// couvertures, que le noyau garde en mémoire après les avoir écrits et lus.
/// On rend ces pages quand plus personne ne les demande depuis un moment : les
/// fichiers, eux, restent sur disque, nginx les resservira sans repasser par
/// l'API. Le dossier se règle par `EASYBOOK_MEDIA_CACHE` (défaut : celui de
/// `docker/nginx.conf`).
fn sweep_media() {
    let dir = std::env::var_os("EASYBOOK_MEDIA_CACHE")
        .map_or_else(|| PathBuf::from(MEDIA_DIR), PathBuf::from);
    walk_media(&dir, 0);
}

/// Parcourt le cache média (`levels=1:2` : deux étages, puis les fichiers).
fn walk_media(path: &Path, depth: u32) {
    // Deux étages de sous-dossiers, plus la racine : au-delà, ce n'est pas notre
    // affaire.
    if depth > 2 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_media(&path, depth + 1);
        } else {
            release_media(&path);
        }
    }
}

/// Rend les pages d'une entrée du cache média si elle a fini d'être servie.
fn release_media(path: &Path) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if !meta.is_file() || !media_expired(&meta) {
        return;
    }
    let freed = drop_pages(path);
    if freed > 0 {
        RELEASED_MEDIA.fetch_add(freed, Ordering::Relaxed);
    }
}

/// Une entrée du cache média est-elle assez vieille pour que ses pages partent ?
fn media_expired(meta: &std::fs::Metadata) -> bool {
    meta.modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|idle| idle >= MEDIA_IDLE)
}

/// Demande au noyau d'oublier les pages en cache de ce fichier ; rend les octets
/// effectivement libérés.
///
/// Seules les pages propres et non mappées partent : rien n'est écrit, rien n'est
/// perdu, la prochaine lecture repasse par le disque.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn drop_pages(path: &Path) -> u64 {
    use std::os::fd::AsRawFd;

    let Ok(file) = std::fs::File::open(path) else {
        return 0;
    };
    let before = resident_bytes(path).unwrap_or(0);
    // SAFETY: le descripteur est valide pendant tout l'appel.
    unsafe {
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
    before.saturating_sub(resident_bytes(path).unwrap_or(before))
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn drop_pages(_path: &Path) -> u64 {
    0
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn release(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn at(seconds: u64, origin: Instant) -> Instant {
        origin + Duration::from_secs(seconds)
    }

    #[test]
    fn a_reading_accumulates_requests_and_bytes() {
        let origin = Instant::now();
        let mut registry = Registry::default();
        let path = Path::new("/books/a.cbz");
        registry.touch(path, "a.cbz", 100, origin);
        registry.touch(path, "a.cbz", 50, at(5, origin));

        let reading = &registry.live[path];
        assert_eq!((reading.requests, reading.served), (2, 150));
        assert!(registry.is_active(path, at(5 + IDLE.as_secs() - 1, origin)));
    }

    #[test]
    fn a_silent_reading_ends_after_the_idle_delay() {
        let origin = Instant::now();
        let mut registry = Registry::default();
        let quiet = Path::new("/books/quiet.cbz");
        let busy = Path::new("/books/busy.cbz");
        registry.touch(quiet, "quiet.cbz", 1, origin);
        registry.touch(busy, "busy.cbz", 1, at(IDLE.as_secs(), origin));

        let done = registry.expire(at(IDLE.as_secs(), origin));
        assert_eq!(done, vec![quiet.to_path_buf()]);
        assert!(registry.is_active(busy, at(IDLE.as_secs(), origin)));
        assert!(!registry.live.contains_key(quiet));
    }

    #[test]
    fn a_read_book_can_be_released() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("livre.bin");
        std::fs::write(&path, vec![7u8; 4 << 20]).unwrap();
        std::fs::read(&path).unwrap();

        let before = resident_bytes(&path);
        release(&path);
        let after = resident_bytes(&path);
        if cfg!(all(target_os = "linux", target_env = "gnu")) {
            assert!(before.unwrap() >= after.unwrap());
        } else {
            assert_eq!(before, None);
        }
    }

    #[test]
    fn an_empty_or_missing_file_is_not_resident() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("vide");
        std::fs::write(&empty, b"").unwrap();
        if cfg!(all(target_os = "linux", target_env = "gnu")) {
            assert_eq!(resident_bytes(&empty), Some(0));
            assert_eq!(resident_bytes(&dir.path().join("absent")), None);
        }
    }

    #[test]
    fn only_old_media_entries_are_released() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("frais");
        let old = dir.path().join("vieux");
        std::fs::write(&fresh, vec![1u8; 4096]).unwrap();
        std::fs::write(&old, vec![1u8; 4096]).unwrap();
        // Deux heures plus tôt : bien au-delà de `MEDIA_IDLE`.
        let past = SystemTime::now() - Duration::from_secs(2 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(past)
            .unwrap();

        assert!(!media_expired(&std::fs::metadata(&fresh).unwrap()));
        assert!(media_expired(&std::fs::metadata(&old).unwrap()));
    }
}
