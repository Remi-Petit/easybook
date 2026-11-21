//! Performances : mémoire et CPU du conteneur, process par process.
//!
//! Tout vient de `/proc` et du cgroup v2 : aucune dépendance, aucun process
//! lancé. Hors Linux (ou sans cgroup v2), les champs concernés sont absents et
//! l'interface s'en accommode.
//!
//! Les process listés sont ceux de l'**arbre du parent de l'API** : dans l'image
//! Docker, c'est le script d'entrée, donc nginx, l'interface Node et l'API
//! elle-même — sans les shells ouverts par `docker exec`.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const CGROUP: &str = "/sys/fs/cgroup";
/// Fréquence de `utime`/`stime` dans `/proc/<pid>/stat` : 100 Hz sur Linux.
const CLOCK_TICKS: u64 = 100;

/// Mémoire du cgroup, en octets.
#[derive(Serialize, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    /// Mémoire comptée au cgroup (ce que `docker stats` affiche, cache compris).
    pub current: u64,
    /// Plafond, s'il y en a un.
    pub limit: Option<u64>,
    /// Mémoire privée des programmes (tas, piles).
    pub anon: u64,
    /// Cache de fichiers (binaires, bibliothèques, livres lus) : récupérable.
    pub file: u64,
    pub kernel: u64,
    pub shmem: u64,
}

/// Un process du conteneur.
#[derive(Serialize, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    pub pid: u32,
    pub name: String,
    /// Mémoire résidente = `anon` + `file` + `shmem`.
    pub rss: u64,
    pub anon: u64,
    pub file: u64,
    pub threads: u32,
    /// Temps CPU cumulé (utilisateur + système), en ticks de [`CLOCK_TICKS`] Hz.
    pub cpu_ticks: u64,
}

/// Ce que l'application garde elle-même en mémoire.
#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub cover_cache_bytes: u64,
    /// Budget du cache de couvertures (voir `thumbnail::CACHE_BYTES`).
    pub cover_cache_budget: u64,
    pub cover_cache_entries: u64,
    pub page_cache_entries: u64,
    /// Tas vivant de l'API, compté par l'allocateur (hors piles et `mmap`).
    pub heap_live: u64,
    /// Plus haut niveau du tas depuis le démarrage.
    pub heap_peak: u64,
    /// Catalogue sérialisé gardé en mémoire.
    pub catalog_bytes: u64,
    /// Archives ZIP gardées ouvertes entre deux lectures.
    pub open_archives: u64,
    /// Sessions ouvertes en base (toutes personnes confondues).
    pub sessions: u64,
    /// Onglets WebSocket connectés.
    pub sockets: u64,
    /// Mémoire réclamée par un hachage Argon2 — transitoire, pas résidente.
    pub argon2_bytes: u64,
    /// Vérifications de session et microsecondes passées à les vérifier.
    pub guard_calls: u64,
    pub guard_usec: u64,
    /// Hachages de mot de passe et microsecondes passées à hacher.
    pub hash_calls: u64,
    pub hash_usec: u64,
}

/// Ce que l'API seule sait de plus que `/proc` : rempli par `api.rs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Detail {
    pub catalog_bytes: u64,
    pub sessions: u64,
    pub sockets: u64,
    pub guard_calls: u64,
    pub guard_usec: u64,
}

/// Réglages de l'historique, lus par la page (`config.yml`, section
/// `performance`).
#[derive(Serialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// `false` : aucune mesure enregistrée, la page reste en direct.
    pub history: bool,
    /// Fréquence d'enregistrement côté serveur, en secondes.
    pub sample_secs: u64,
    /// Fréquence de rafraîchissement de la page, en secondes.
    pub live_secs: u64,
    /// Durée de conservation, en jours.
    pub retention_days: u32,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// `false` quand `/proc` n'est pas lisible (hors Linux).
    pub supported: bool,
    /// Instant de la mesure, en millisecondes depuis l'epoch : base des taux CPU.
    pub at_ms: u64,
    pub cpus: u64,
    /// Plafond CPU du cgroup, en nombre de cœurs.
    pub cpu_limit: Option<f64>,
    pub clock_ticks: u64,
    pub memory: Option<Memory>,
    /// Temps CPU cumulé du cgroup, en microsecondes.
    pub cpu_usage_usec: Option<u64>,
    pub processes: Vec<Process>,
    pub app: App,
    pub readings: crate::readings::Overview,
    /// Fréquences et rétention de l'historique.
    pub settings: Settings,
}

pub fn snapshot(detail: Detail) -> Snapshot {
    let supported = fs::metadata("/proc/self/status").is_ok();
    let (cover_entries, cover_bytes) = crate::thumbnail::cache_stats();
    let (hash_calls, hash_usec) = crate::accounts::hash_stats();
    // `memory.current` n'existe pas à la racine des cgroups : sans lui, les
    // compteurs du cgroup décrivent toute la machine, pas l'application.
    let memory = memory();
    let scoped = memory.is_some();
    let performance = crate::config::performance();
    Snapshot {
        supported,
        at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64),
        cpus: std::thread::available_parallelism().map_or(1, |n| n.get() as u64),
        cpu_limit: read("cpu.max").and_then(|text| parse_cpu_max(&text)),
        clock_ticks: CLOCK_TICKS,
        memory,
        cpu_usage_usec: read("cpu.stat").filter(|_| scoped).and_then(|text| {
            parse_pairs(&text)
                .find(|(key, _)| *key == "usage_usec")
                .map(|(_, value)| value)
        }),
        processes: if supported { processes() } else { Vec::new() },
        app: App {
            cover_cache_entries: cover_entries as u64,
            cover_cache_bytes: cover_bytes as u64,
            cover_cache_budget: crate::thumbnail::CACHE_BYTES as u64,
            page_cache_entries: crate::pages::cache_entries() as u64,
            heap_live: crate::alloc::live(),
            heap_peak: crate::alloc::peak(),
            catalog_bytes: detail.catalog_bytes,
            open_archives: crate::archive::pooled() as u64,
            sessions: detail.sessions,
            sockets: detail.sockets,
            // Coût mémoire d'un hachage Argon2id, lu dans ses paramètres
            // (`m_cost` en Kio) : c'est lui qui explique le pic de RSS au login.
            argon2_bytes: u64::from(argon2::Params::default().m_cost()) * 1024,
            guard_calls: detail.guard_calls,
            guard_usec: detail.guard_usec,
            hash_calls,
            hash_usec,
        },
        readings: crate::readings::overview(),
        settings: Settings {
            history: performance.enabled,
            sample_secs: performance.sample_secs,
            live_secs: performance.live_secs,
            retention_days: performance.retention_days,
        },
    }
}

fn read(file: &str) -> Option<String> {
    fs::read_to_string(format!("{CGROUP}/{file}")).ok()
}

fn memory() -> Option<Memory> {
    let current = read("memory.current")?.trim().parse().ok()?;
    let stat = read("memory.stat")?;
    let field = |name: &str| {
        parse_pairs(&stat)
            .find(|(key, _)| *key == name)
            .map_or(0, |(_, value)| value)
    };
    Some(Memory {
        current,
        limit: read("memory.max").and_then(|text| text.trim().parse().ok()),
        anon: field("anon"),
        file: field("file"),
        kernel: field("kernel"),
        shmem: field("shmem"),
    })
}

/// Lignes `clé valeur` de `memory.stat` et `cpu.stat`.
fn parse_pairs(text: &str) -> impl Iterator<Item = (&str, u64)> {
    text.lines().filter_map(|line| {
        let (key, value) = line.split_once(' ')?;
        Some((key, value.trim().parse().ok()?))
    })
}

/// `cpu.max` : `max 100000` (aucun plafond) ou `200000 100000` (2 cœurs).
fn parse_cpu_max(text: &str) -> Option<f64> {
    let (quota, period) = text.trim().split_once(' ')?;
    let (quota, period): (f64, f64) = (quota.parse().ok()?, period.parse().ok()?);
    (period > 0.0).then_some(quota / period)
}

fn processes() -> Vec<Process> {
    let Ok(dir) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    // pid -> (parent, temps CPU)
    let mut stats: HashMap<u32, (u32, u64)> = HashMap::new();
    for entry in dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(stat) = fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|text| parse_stat(&text))
        {
            stats.insert(pid, stat);
        }
    }

    let me = std::process::id();
    let root = match stats.get(&me) {
        Some(&(parent, _)) if parent != 0 => parent,
        _ => me,
    };
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&pid, &(parent, _)) in &stats {
        children.entry(parent).or_default().push(pid);
    }
    let mut tree = vec![root];
    let mut next = 0;
    while let Some(&pid) = tree.get(next) {
        next += 1;
        tree.extend(children.get(&pid).into_iter().flatten());
    }

    let mut found: Vec<Process> = tree
        .into_iter()
        .filter_map(|pid| {
            let mut process =
                parse_status(&fs::read_to_string(format!("/proc/{pid}/status")).ok()?)?;
            process.pid = pid;
            process.cpu_ticks = stats.get(&pid)?.1;
            Some(process)
        })
        .collect();
    found.sort_by_key(|process| std::cmp::Reverse(process.rss));
    found
}

/// `/proc/<pid>/status` : nom, mémoire et threads. `None` pour un thread du
/// noyau (pas de mémoire résidente).
fn parse_status(text: &str) -> Option<Process> {
    let mut process = Process::default();
    let mut has_rss = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let kilobytes = || {
            value
                .trim_end_matches(" kB")
                .trim()
                .parse::<u64>()
                .ok()
                .map(|kb| kb * 1024)
        };
        match key {
            "Name" => process.name = value.to_string(),
            "VmRSS" => {
                process.rss = kilobytes()?;
                has_rss = true;
            }
            "RssAnon" => process.anon = kilobytes()?,
            "RssFile" => process.file = kilobytes()?,
            "Threads" => process.threads = value.parse().ok()?,
            _ => {}
        }
    }
    has_rss.then_some(process)
}

/// `/proc/<pid>/stat` : (parent, `utime + stime`). Le nom (champ 2) est entre
/// parenthèses et peut contenir des espaces : on repart de la dernière `)`.
fn parse_stat(text: &str) -> Option<(u32, u64)> {
    let after = &text[text.rfind(')')? + 1..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    // `fields[0]` est le champ 3 (état) : ppid = champ 4, utime = 14, stime = 15.
    let ticks = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    Some((fields.get(1)?.parse().ok()?, ticks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_key_value_pairs() {
        let pairs: Vec<_> = parse_pairs("anon 100\nfile 200\nusage_usec 7\n").collect();
        assert_eq!(pairs, vec![("anon", 100), ("file", 200), ("usage_usec", 7)]);
    }

    #[test]
    fn reads_the_cpu_limit() {
        assert_eq!(parse_cpu_max("max 100000\n"), None);
        assert_eq!(parse_cpu_max("200000 100000\n"), Some(2.0));
        assert_eq!(parse_cpu_max("50000 100000"), Some(0.5));
    }

    #[test]
    fn reads_a_process_status() {
        let text = "Name:\tnode\nThreads:\t11\nVmRSS:\t   47996 kB\nRssAnon:\t   24316 kB\nRssFile:\t   23680 kB\n";
        let process = parse_status(text).unwrap();
        assert_eq!(process.name, "node");
        assert_eq!(process.rss, 47996 * 1024);
        assert_eq!(process.anon, 24316 * 1024);
        assert_eq!(process.file, 23680 * 1024);
        assert_eq!(process.threads, 11);
    }

    #[test]
    fn a_kernel_thread_has_no_resident_memory() {
        assert!(parse_status("Name:\tkworker/0:1\nThreads:\t1\n").is_none());
    }

    #[test]
    fn cpu_ticks_and_parent_survive_a_name_with_spaces_and_parentheses() {
        // Champs 3 à 15 : état, ppid (1), …, utime (14) = 40, stime (15) = 2.
        let text = "42 (we ird) name) S 7 42 42 0 -1 4194560 100 0 0 0 40 2 0 0 20 0 1 0 5";
        assert_eq!(parse_stat(text), Some((7, 42)));
    }

    #[test]
    fn lists_this_process() {
        let snapshot = snapshot(Detail::default());
        if !snapshot.supported {
            return;
        }
        let me = std::process::id();
        let mine = snapshot
            .processes
            .iter()
            .find(|p| p.pid == me)
            .expect("process courant");
        assert!(mine.rss > 0 && mine.threads >= 1);
    }
}
