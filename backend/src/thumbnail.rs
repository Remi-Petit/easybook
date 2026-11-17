//! Couvertures : extraction depuis le livre, vignette, cache mémoire et disque.
//!
//! On ne sert jamais l'image d'origine au catalogue : une couverture d'EPUB pèse
//! souvent plusieurs mégaoctets, une planche de bande dessinée davantage encore,
//! et une page en affiche des dizaines. Chaque couverture est donc réduite une
//! fois, puis gardée en mémoire **et sur disque** : le calcul (rendu d'un PDF,
//! lecture d'une archive) n'est pas refait après un redémarrage.
//!
//! Le cache disque vit dans un dossier **caché** de la bibliothèque
//! ([`DEFAULT_CACHE_DIR`]) : le scan et le watcher ignorent les éléments cachés,
//! il n'apparaît donc pas dans le catalogue.
//!
//! D'où vient la couverture, selon le format :
//! - **EPUB** : celle que le livre déclare (voir [`crate::formats::epub`]) ;
//! - **CBZ** et **CBR** : la première page, par convention du format ;
//! - **PDF** : la première page, rendue en image (voir [`crate::formats::pdf`]).

use crate::formats::{self, Format};
use image::{DynamicImage, ImageFormat};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

/// Dossier du cache disque, relatif à la racine de la bibliothèque. Le point
/// initial le cache : le scan l'ignore comme tout élément caché.
pub const DEFAULT_CACHE_DIR: &str = ".easybook/covers";

/// Largeur maximale d'une vignette (la hauteur suit le ratio du livre).
const WIDTH: u32 = 400;
/// Budget du cache mémoire, en octets de vignettes. Au-delà, les moins récemment
/// utilisées sont évincées une à une : c'est un accélérateur, le disque reste la
/// référence.
const CACHE_BYTES: usize = 32 * 1024 * 1024;
/// Nombre maximal de couvertures calculées en même temps (requêtes et
/// préchauffage confondus) : chaque calcul décode une image entière en pixels.
const MAX_BUILDS: usize = 2;
/// Révision de l'extraction, inscrite dans le nom des fichiers du cache disque.
///
/// À incrémenter quand la façon de produire une vignette change : les anciennes
/// sont alors régénérées (le préchauffage les remplace, voir [`warm`]). Elle est
/// distincte de `COVER_REVISION` de l'interface, qui ne vaut que pour le cache du
/// navigateur.
const DISK_REVISION: u32 = 1;

struct Cached {
    bytes: Vec<u8>,
    /// Rang du dernier accès (voir `Cache::tick`).
    used: u64,
}

#[derive(Default)]
struct Cache {
    map: HashMap<String, Cached>,
    tick: u64,
    bytes: usize,
}

/// Portail comptant les calculs en cours : au-delà de [`MAX_BUILDS`], on attend.
struct Gate {
    running: Mutex<usize>,
    freed: Condvar,
}

struct Permit;

fn gate() -> &'static Gate {
    static GATE: OnceLock<Gate> = OnceLock::new();
    GATE.get_or_init(|| Gate {
        running: Mutex::new(0),
        freed: Condvar::new(),
    })
}

fn acquire() -> Permit {
    let gate = gate();
    let mut running = gate.running.lock().unwrap_or_else(|e| e.into_inner());
    while *running >= MAX_BUILDS {
        running = gate.freed.wait(running).unwrap_or_else(|e| e.into_inner());
    }
    *running += 1;
    Permit
}

impl Drop for Permit {
    fn drop(&mut self) {
        let gate = gate();
        *gate.running.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        gate.freed.notify_one();
    }
}

/// Couverture d'un livre, en JPEG, à la taille d'une vignette.
///
/// `version` fait partie de la clé du cache mémoire : la date de modification
/// du fichier (un livre remplacé change d'image) et, le cas échéant, la révision
/// demandée par l'interface. `disk` est le dossier du cache persistant (`None` :
/// mémoire seule).
pub fn cover(path: &Path, rel: &str, version: &str, disk: Option<&Path>) -> Option<Vec<u8>> {
    let key = format!("{rel}|{version}");
    if let Some(bytes) = lookup(&key) {
        return Some(bytes);
    }

    let entry = disk.and_then(|dir| DiskEntry::new(dir, path, rel));
    if let Some(bytes) = entry.as_ref().and_then(DiskEntry::read) {
        store(key, bytes.clone());
        return Some(bytes);
    }

    let made = build(path, rel, entry.as_ref())?;
    release_memory();
    store(key, made.clone());
    Some(made)
}

/// Extrait la couverture du livre, la réduit et, si possible, l'écrit sur disque.
fn build(path: &Path, rel: &str, entry: Option<&DiskEntry>) -> Option<Vec<u8>> {
    let _permit = acquire();
    let original = match Format::from_path(rel) {
        Some(format) => formats::module(format).cover(path),
        None => None,
    }?;

    // Une image illisible (format exotique) reste mieux qu'une absence : on la
    // sert telle quelle, le navigateur saura peut-être l'afficher. Elle n'est pas
    // gardée sur disque : ce n'est pas une vignette.
    match shrink(&original) {
        Some(thumb) => {
            if let Some(entry) = entry {
                entry.write(&thumb);
            }
            Some(thumb)
        }
        None => Some(original),
    }
}

/// Bilan d'un préchauffage.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Warmed {
    /// Vignettes produites pendant ce passage.
    pub made: usize,
    /// Livres dont la vignette était déjà sur disque.
    pub kept: usize,
    /// Livres sans couverture (ou illisible).
    pub missing: usize,
    /// Fichiers du cache supprimés (livre disparu, ancienne version).
    pub removed: usize,
}

/// Produit sur disque la vignette de **tous** les livres qui n'en ont pas encore,
/// et supprime celles des livres disparus.
///
/// Pensé pour tourner en tâche de fond, au démarrage puis à chaque changement de
/// la bibliothèque : l'interface trouve les couvertures déjà prêtes. Les livres
/// sont traités par [`MAX_BUILDS`] threads au plus (le portail de `build` borne
/// aussi la mémoire en cas de requêtes simultanées). Le cache mémoire n'est pas
/// rempli : seul le disque l'est.
pub fn warm(root: &Path, dir: &Path) -> Warmed {
    let mut live: Vec<String> = Vec::new();
    let mut todo: Vec<(PathBuf, String, DiskEntry)> = Vec::new();
    let mut kept = 0;

    for book in crate::library::scan(root).books {
        let path = root.join(&book.rel);
        let Some(entry) = DiskEntry::new(dir, &path, &book.rel) else {
            continue;
        };
        live.push(entry.name.clone());

        if entry.read().is_some() {
            kept += 1;
        } else {
            todo.push((path, book.rel, entry));
        }
    }

    let next = AtomicUsize::new(0);
    let made = AtomicUsize::new(0);
    let missing = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..MAX_BUILDS.min(todo.len()) {
            scope.spawn(|| {
                while let Some((path, rel, entry)) = todo.get(next.fetch_add(1, Ordering::Relaxed))
                {
                    let counter = if build(path, rel, Some(entry)).is_some() {
                        &made
                    } else {
                        &missing
                    };
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });

    let mut report = Warmed {
        made: made.into_inner(),
        kept,
        missing: missing.into_inner(),
        removed: 0,
    };

    // Une bibliothèque vide (dossier démonté, scan en échec) ne doit pas vider le
    // cache : il resservira au retour des livres.
    if !live.is_empty() {
        report.removed = prune(dir, &live);
    }
    release_memory();
    report
}

/// Supprime les vignettes (`<16 hex>-….jpg`) qui ne correspondent à aucun livre.
/// Les autres fichiers du dossier ne sont jamais touchés.
fn prune(dir: &Path, live: &[String]) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_cover_file(&name)
            && !live.contains(&name)
            && std::fs::remove_file(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

fn is_cover_file(name: &str) -> bool {
    name.ends_with(".jpg")
        && name.len() > 17
        && name.as_bytes()[16] == b'-'
        && name[..16].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Une vignette sur disque : `<hash du chemin>-<taille>-<date>-<révision>.jpg`.
///
/// Le hash du chemin sert de préfixe : écrire une nouvelle version d'un livre
/// supprime les anciennes (livre remplacé, révision d'extraction changée), le
/// dossier ne grossit donc pas.
struct DiskEntry {
    dir: PathBuf,
    prefix: String,
    name: String,
}

impl DiskEntry {
    fn new(dir: &Path, path: &Path, rel: &str) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_secs());
        let prefix = format!("{:016x}-", fnv1a(rel.as_bytes()));
        let name = format!("{prefix}{}-{modified}-r{DISK_REVISION}.jpg", meta.len());
        Some(Self {
            dir: dir.to_path_buf(),
            prefix,
            name,
        })
    }

    fn read(&self) -> Option<Vec<u8>> {
        let bytes = std::fs::read(self.dir.join(&self.name)).ok()?;
        // Un fichier tronqué (arrêt en pleine écriture) ne doit pas devenir une
        // couverture : un JPEG commence par `FF D8`.
        bytes.starts_with(&[0xFF, 0xD8]).then_some(bytes)
    }

    /// Écrit la vignette ; un dossier en lecture seule n'est pas une erreur : la
    /// couverture reste servie depuis la mémoire.
    fn write(&self, bytes: &[u8]) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        // Écriture dans un fichier temporaire puis renommage : un lecteur ne voit
        // jamais un fichier à moitié écrit.
        let tmp = self.dir.join(format!(
            ".{}.{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if std::fs::write(&tmp, bytes).is_err()
            || std::fs::rename(&tmp, self.dir.join(&self.name)).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
            return;
        }

        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&self.prefix) && name != self.name {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// FNV-1a 64 bits : stable d'une version de Rust à l'autre, contrairement à
/// `DefaultHasher`, dont l'algorithme n'est pas garanti.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Réduit une image à [`WIDTH`] de large, en JPEG.
fn shrink(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let thumb: DynamicImage = image.thumbnail(WIDTH, WIDTH * 3 / 2);
    let mut out = Vec::new();
    thumb
        .to_rgb8()
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
        .ok()?;
    Some(out)
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

fn lookup(key: &str) -> Option<Vec<u8>> {
    let mut cache = cache().lock().ok()?;
    cache.tick += 1;
    let tick = cache.tick;
    let entry = cache.map.get_mut(key)?;
    entry.used = tick;
    Some(entry.bytes.clone())
}

fn store(key: String, bytes: Vec<u8>) {
    let Ok(mut cache) = cache().lock() else {
        return;
    };
    cache.tick += 1;
    let used = cache.tick;
    cache.bytes += bytes.len();
    if let Some(old) = cache.map.insert(key, Cached { bytes, used }) {
        cache.bytes -= old.bytes.len();
    }
    // Éviction des moins récemment utilisées jusqu'à rentrer dans le budget.
    while cache.bytes > CACHE_BYTES && cache.map.len() > 1 {
        let Some(oldest) = cache
            .map
            .iter()
            .min_by_key(|(_, entry)| entry.used)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        if let Some(old) = cache.map.remove(&oldest) {
            cache.bytes -= old.bytes.len();
        }
    }
}

/// Rend à l'OS la mémoire libérée par un décodage d'image : sans cela, glibc la
/// garde dans ses arènes et le RSS reste au niveau du pic.
fn release_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` n'a aucune précondition ; il ne fait que rendre des pages libres.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// À appeler au démarrage : une seule arène glibc. Avec une arène par thread, la
/// mémoire décodée par chaque thread de calcul reste répartie et épinglée.
pub fn tune_allocator() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `mallopt` règle un paramètre global de l'allocateur, sans précondition.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn thumbnails_are_smaller_and_keep_the_ratio() {
        // Une image « bruitée » : elle ne se comprime pas à presque rien, comme
        // une vraie planche.
        let mut source = RgbaImage::new(900, 1350);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 233) as u8, 255]);
        }
        let mut png = Vec::new();
        source
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();

        let thumb = shrink(&png).expect("vignette produite");
        assert_eq!(&thumb[..2], &[0xFF, 0xD8], "encodée en JPEG");
        assert!(thumb.len() < png.len(), "la vignette doit être plus légère");

        let decoded = image::load_from_memory(&thumb).unwrap();
        assert_eq!(decoded.width(), WIDTH);
        assert_eq!(decoded.height(), WIDTH * 3 / 2);
    }

    #[test]
    fn unknown_bytes_are_not_a_cover() {
        assert!(shrink(b"pas une image").is_none());
    }

    /// CBZ d'une page (PNG de `width` x 120) dans `dir`.
    fn cbz_sized(dir: &Path, name: &str, width: u32) -> PathBuf {
        let mut png = Vec::new();
        RgbaImage::from_pixel(width, 120, Rgba([200, 30, 30, 255]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("p1.png", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, &png).unwrap();
        zip.finish().unwrap();
        path
    }

    fn cbz_in(dir: &Path, name: &str) -> PathBuf {
        cbz_sized(dir, name, 80)
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn a_cover_is_persisted_and_reused_from_disk() {
        let books = tempfile::tempdir().unwrap();
        let cache = books.path().join(".easybook/covers");
        let path = cbz_in(books.path(), "disque-a.cbz");

        let first = cover(&path, "disque-a.cbz", "1|3", Some(&cache)).expect("couverture");
        assert_eq!(files_in(&cache).len(), 1, "une vignette écrite sur disque");

        // Lecture directe du fichier : la vignette est bien sur disque, intacte.
        let entry = DiskEntry::new(&cache, &path, "disque-a.cbz").unwrap();
        assert_eq!(entry.read().as_deref(), Some(first.as_slice()));
    }

    #[test]
    fn a_replaced_book_replaces_the_old_file() {
        let books = tempfile::tempdir().unwrap();
        let cache = books.path().join(".easybook/covers");
        let path = cbz_in(books.path(), "disque-b.cbz");

        cover(&path, "disque-b.cbz", "1|3", Some(&cache)).unwrap();
        // Même chemin, autre contenu (donc autre taille) : un autre livre.
        cbz_sized(books.path(), "disque-b.cbz", 200);
        cover(&path, "disque-b.cbz", "2|3", Some(&cache)).unwrap();
        assert_eq!(
            files_in(&cache).len(),
            1,
            "l'ancienne version est supprimée"
        );

        // Un autre livre n'est pas touché par le nettoyage.
        let other = cbz_in(books.path(), "disque-c.cbz");
        cover(&other, "disque-c.cbz", "1|3", Some(&cache)).unwrap();
        assert_eq!(files_in(&cache).len(), 2);
    }

    #[test]
    fn the_client_revision_does_not_change_the_disk_file() {
        let books = tempfile::tempdir().unwrap();
        let cache = books.path().join(".easybook/covers");
        let path = cbz_in(books.path(), "disque-f.cbz");

        cover(&path, "disque-f.cbz", "1|2", Some(&cache)).unwrap();
        cover(&path, "disque-f.cbz", "1|3", Some(&cache)).unwrap();
        assert_eq!(files_in(&cache).len(), 1);
    }

    #[test]
    fn warming_creates_every_cover_and_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Serie")).unwrap();
        cbz_in(root.path(), "un.cbz");
        cbz_in(&root.path().join("Serie"), "deux.cbz");
        // Un fichier qui n'est pas une image : livre sans couverture.
        std::fs::write(root.path().join("vide.epub"), b"pas un epub").unwrap();
        let cache = root.path().join(DEFAULT_CACHE_DIR);

        let first = warm(root.path(), &cache);
        assert_eq!(
            first,
            Warmed {
                made: 2,
                kept: 0,
                missing: 1,
                removed: 0
            }
        );
        assert_eq!(files_in(&cache).len(), 2);

        let second = warm(root.path(), &cache);
        assert_eq!((second.made, second.kept), (0, 2), "rien à refaire");
    }

    #[test]
    fn warming_removes_covers_of_deleted_books_only() {
        let root = tempfile::tempdir().unwrap();
        cbz_in(root.path(), "garde.cbz");
        let gone = cbz_in(root.path(), "parti.cbz");
        let cache = root.path().join(DEFAULT_CACHE_DIR);
        warm(root.path(), &cache);
        assert_eq!(files_in(&cache).len(), 2);

        // Un fichier qui n'est pas une vignette n'est jamais supprimé.
        std::fs::write(cache.join("notes.txt"), b"x").unwrap();
        std::fs::remove_file(gone).unwrap();

        let report = warm(root.path(), &cache);
        assert_eq!(report.removed, 1);
        assert_eq!(files_in(&cache).len(), 2, "1 vignette + notes.txt");
        assert!(cache.join("notes.txt").exists());
    }

    #[test]
    fn an_empty_library_does_not_wipe_the_cache() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join(DEFAULT_CACHE_DIR);
        let book = cbz_in(root.path(), "un.cbz");
        warm(root.path(), &cache);
        std::fs::remove_file(book).unwrap();

        assert_eq!(warm(root.path(), &cache).removed, 0);
        assert_eq!(files_in(&cache).len(), 1);
    }

    #[test]
    fn a_truncated_file_is_not_served() {
        let books = tempfile::tempdir().unwrap();
        let cache = books.path().join("c");
        let path = cbz_in(books.path(), "disque-d.cbz");
        let entry = DiskEntry::new(&cache, &path, "disque-d.cbz").unwrap();

        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join(&entry.name), b"pas un jpeg").unwrap();
        assert!(entry.read().is_none());
    }

    #[test]
    fn an_unwritable_cache_does_not_break_the_cover() {
        let books = tempfile::tempdir().unwrap();
        let path = cbz_in(books.path(), "disque-e.cbz");
        // Le "dossier" du cache est un fichier : impossible d'y écrire.
        let blocker = books.path().join("bloque");
        std::fs::write(&blocker, b"x").unwrap();

        assert!(cover(&path, "disque-e.cbz", "1|3", Some(&blocker.join("sous"))).is_some());
    }
}
