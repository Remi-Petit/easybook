//! Historique de la page Performances : mesures RAM/CPU conservées `N` jours.
//!
//! Une ligne par mesure (SQLite, WAL). Les groupes de programmes tiennent dans
//! un BLOB compact `[id u16, rss u64, cpu f32] × n`, les noms vivant dans le
//! dictionnaire `perf_programs` : une seule table à balayer pour l'agrégat des
//! 30 jours, et un schéma qui accepte un programme inconnu sans migration.
//!
//! Le choix de SQLite et de cette forme est mesuré dans
//! `bench/reports/metrics-store.md` : 24 Mo pour 30 jours à 10 s, 8,6 Mo de RAM,
//! 82 ms pour l'agrégat de 30 jours tous groupes confondus.

use crate::config;
use crate::perf;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Une réponse d'historique est réutilisée pendant ce délai : l'échantillonneur
/// n'ajoute qu'une mesure toutes les 10 s, inutile de rebalayer 30 jours à
/// chaque ouverture de page.
const CACHE_TTL: Duration = Duration::from_secs(15);
/// Fréquence de la purge de rétention (la suppression elle-même est mesurée à
/// 13 ms pour un jour de mesures).
const TRIM_EVERY: Duration = Duration::from_secs(3600);
/// Nombre de points visé : au-delà, la courbe ne gagne rien et le JSON grossit.
const TARGET_POINTS: i64 = 800;
/// Paliers de tranche, en millisecondes. Le premier qui tient sous la cible gagne.
const STEP_LADDER: [i64; 14] = [
    1_000, 2_000, 5_000, 10_000, 15_000, 30_000, 60_000, 120_000, 300_000, 600_000, 900_000,
    1_800_000, 3_600_000, 7_200_000,
];

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// Fenêtres proposées par la page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    ThreeMinutes,
    Hour,
    Day,
    Week,
    Month,
}

impl Range {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "3m" => Some(Self::ThreeMinutes),
            "1h" => Some(Self::Hour),
            "24h" => Some(Self::Day),
            "7d" => Some(Self::Week),
            "30d" => Some(Self::Month),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::ThreeMinutes => "3m",
            Self::Hour => "1h",
            Self::Day => "24h",
            Self::Week => "7d",
            Self::Month => "30d",
        }
    }

    pub fn duration_ms(self) -> i64 {
        match self {
            Self::ThreeMinutes => 3 * 60_000,
            Self::Hour => 3_600_000,
            Self::Day => 86_400_000,
            Self::Week => 7 * 86_400_000,
            Self::Month => 30 * 86_400_000,
        }
    }
}

/// La part d'un programme dans une tranche.
#[derive(Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroupPoint {
    pub rss_avg: f64,
    pub rss_max: i64,
    pub cpu_pct_avg: f64,
    pub cpu_pct_max: f32,
}

/// Une tranche de temps de l'historique.
#[derive(Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub at_ms: i64,
    pub mem_avg: f64,
    pub mem_max: i64,
    pub cpu_pct_avg: f64,
    pub cpu_pct_max: f32,
    pub groups: BTreeMap<String, GroupPoint>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub range: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub step_ms: i64,
    pub points: Vec<Point>,
}

/// Une mesure prête à être écrite.
#[derive(Debug, Clone)]
pub struct Measurement {
    pub at_ms: i64,
    pub mem_total: i64,
    pub mem_anon: i64,
    pub mem_file: i64,
    pub mem_kernel: i64,
    pub cpu_pct: f32,
    pub heap_live: i64,
    pub sessions: i64,
    pub sockets: i64,
    pub groups: Vec<GroupMeasurement>,
}

#[derive(Debug, Clone)]
pub struct GroupMeasurement {
    pub name: String,
    pub rss: i64,
    pub cpu_pct: f32,
}

impl Measurement {
    /// Calcule les taux depuis deux mesures successives (les compteurs CPU sont
    /// cumulés) et regroupe les process par nom, comme la page.
    pub fn between(previous: &perf::Snapshot, current: &perf::Snapshot) -> Option<Self> {
        if !current.supported {
            return None;
        }
        let memory = current.memory.as_ref()?;
        let seconds = (current.at_ms as i64 - previous.at_ms as i64) as f64 / 1000.0;
        if seconds <= 0.0 {
            return None;
        }
        let before: HashMap<u32, u64> = previous
            .processes
            .iter()
            .map(|process| (process.pid, process.cpu_ticks))
            .collect();
        let mut groups: BTreeMap<&str, (i64, f64)> = BTreeMap::new();
        for process in &current.processes {
            let entry = groups.entry(process.name.as_str()).or_default();
            entry.0 += process.rss as i64;
            if let Some(ticks) = before.get(&process.pid) {
                let delta = process.cpu_ticks.saturating_sub(*ticks);
                entry.1 += delta as f64 / current.clock_ticks as f64 / seconds * 100.0;
            }
        }
        let cpu_pct = match (current.cpu_usage_usec, previous.cpu_usage_usec) {
            (Some(now), Some(then)) if now >= then => (now - then) as f64 / 1e6 / seconds * 100.0,
            // Hors cgroup : la somme des process est la seule mesure disponible.
            _ => groups.values().map(|(_, pct)| *pct).sum(),
        };
        Some(Self {
            at_ms: current.at_ms as i64,
            mem_total: memory.current as i64,
            mem_anon: memory.anon as i64,
            mem_file: memory.file as i64,
            mem_kernel: memory.kernel as i64,
            cpu_pct: cpu_pct as f32,
            heap_live: current.app.heap_live as i64,
            sessions: current.app.sessions as i64,
            sockets: current.app.sockets as i64,
            groups: groups
                .into_iter()
                .map(|(name, (rss, cpu_pct))| GroupMeasurement {
                    name: name.to_string(),
                    rss,
                    cpu_pct: cpu_pct as f32,
                })
                .collect(),
        })
    }
}

struct Inner {
    conn: Connection,
    /// Nom de programme → identifiant (u32, pour tenir dans le BLOB).
    programs: HashMap<String, u32>,
    last_trim: Instant,
}

pub struct History {
    inner: Mutex<Inner>,
    retention_ms: i64,
    sample_ms: i64,
    cache: Mutex<HashMap<&'static str, (Instant, String)>>,
}

impl History {
    /// Ouvre (crée au besoin) la base dans `path`.
    pub fn open(path: &Path, settings: &config::Performance) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS perf_samples(
                 at_ms      INTEGER PRIMARY KEY,
                 mem_total  INTEGER NOT NULL,
                 mem_anon   INTEGER NOT NULL,
                 mem_file   INTEGER NOT NULL,
                 mem_kernel INTEGER NOT NULL,
                 cpu_pct    REAL    NOT NULL,
                 heap_live  INTEGER NOT NULL,
                 sessions   INTEGER NOT NULL,
                 sockets    INTEGER NOT NULL,
                 groups     BLOB    NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS perf_programs(
                 id   INTEGER PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE
             );",
        )?;
        let retention_ms = i64::from(settings.retention_days) * 86_400_000;
        // Une base arrêtée longtemps ne doit pas resservir ses vieilles lignes :
        // la première purge a lieu à l'ouverture, pas une heure plus tard.
        conn.execute(
            "DELETE FROM perf_samples WHERE at_ms < ?1",
            [now_ms() - retention_ms],
        )?;
        let _ = conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
        Ok(Self {
            inner: Mutex::new(Inner {
                programs: load_programs(&conn)?,
                conn,
                last_trim: Instant::now(),
            }),
            retention_ms,
            sample_ms: settings.sample_secs as i64 * 1000,
            cache: Mutex::new(HashMap::new()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Un pointeur empoisonné ne rend pas la base inutilisable : on reprend.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Écrit une mesure, puis purge une fois par heure ce qui dépasse la
    /// rétention.
    pub fn record(&self, measurement: &Measurement) -> rusqlite::Result<()> {
        let inner = &mut *self.lock();
        // Champs empruntés séparément : `transaction` tient `conn`, les noms
        // doivent rester accessibles pendant l'écriture.
        let Inner {
            conn,
            programs,
            last_trim,
        } = inner;
        let tx = conn.transaction()?;
        let mut blob = Vec::with_capacity(measurement.groups.len() * 16);
        for group in &measurement.groups {
            let id = program_id(programs, &tx, &group.name)?;
            blob.extend_from_slice(&id.to_le_bytes());
            blob.extend_from_slice(&group.rss.to_le_bytes());
            blob.extend_from_slice(&group.cpu_pct.to_le_bytes());
        }
        tx.execute(
            "INSERT OR REPLACE INTO perf_samples
             (at_ms, mem_total, mem_anon, mem_file, mem_kernel, cpu_pct,
              heap_live, sessions, sockets, groups)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                measurement.at_ms,
                measurement.mem_total,
                measurement.mem_anon,
                measurement.mem_file,
                measurement.mem_kernel,
                measurement.cpu_pct,
                measurement.heap_live,
                measurement.sessions,
                measurement.sockets,
                blob
            ],
        )?;
        tx.commit()?;
        if last_trim.elapsed() >= TRIM_EVERY {
            let before = now_ms() - self.retention_ms;
            conn.execute("DELETE FROM perf_samples WHERE at_ms < ?1", [before])?;
            // Le journal ne doit pas garder les pages supprimées : un point de
            // contrôle par heure suffit et ne bloque pas une écriture de 10 s.
            let _ = conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
            *last_trim = Instant::now();
        }
        Ok(())
    }

    /// Série agrégée prête à être sérialisée, avec un cache court.
    pub fn series_json(&self, range: Range) -> rusqlite::Result<String> {
        if let Ok(cache) = self.cache.lock()
            && let Some((at, text)) = cache.get(range.key())
            && at.elapsed() < CACHE_TTL
        {
            return Ok(text.clone());
        }
        let series = self.series(range)?;
        let text = serde_json::to_string(&series).unwrap_or_else(|_| "{\"points\":[]}".to_string());
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(range.key(), (Instant::now(), text.clone()));
        }
        Ok(text)
    }

    fn series(&self, range: Range) -> rusqlite::Result<Series> {
        let to_ms = now_ms();
        let from_ms = (to_ms - range.duration_ms()).max(to_ms - self.retention_ms);
        let step_ms = step_for(range.duration_ms(), self.sample_ms);
        let points = self.points(from_ms, to_ms, step_ms)?;
        Ok(Series {
            range: range.key().to_string(),
            from_ms,
            to_ms,
            step_ms,
            points,
        })
    }

    /// Un seul balayage de la table : les totaux et les groupes sont agrégés
    /// ensemble (mesuré : 82 ms pour 30 jours).
    fn points(&self, from_ms: i64, to_ms: i64, step_ms: i64) -> rusqlite::Result<Vec<Point>> {
        let inner = self.lock();
        let names: HashMap<u32, String> = inner
            .programs
            .iter()
            .map(|(name, id)| (*id, name.clone()))
            .collect();
        let mut stmt = inner.conn.prepare_cached(
            "SELECT at_ms, mem_total, cpu_pct, groups FROM perf_samples
             WHERE at_ms >= ?1 AND at_ms < ?2 ORDER BY at_ms",
        )?;
        let mut totals: BTreeMap<i64, Acc> = BTreeMap::new();
        let mut group_acc: BTreeMap<(u32, i64), Acc> = BTreeMap::new();
        let mut rows = stmt.query([from_ms, to_ms])?;
        while let Some(row) = rows.next()? {
            let at_ms: i64 = row.get(0)?;
            let bucket = at_ms / step_ms * step_ms;
            totals
                .entry(bucket)
                .or_default()
                .push(row.get::<_, i64>(1)? as f64, row.get::<_, f64>(2)? as f32);
            let blob: Vec<u8> = row.get(3)?;
            for chunk in blob.as_chunks::<16>().0 {
                let id = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
                let rss = i64::from_le_bytes(chunk[4..12].try_into().unwrap());
                let cpu = f32::from_le_bytes(chunk[12..16].try_into().unwrap());
                group_acc
                    .entry((id, bucket))
                    .or_default()
                    .push(rss as f64, cpu);
            }
        }
        let mut points: BTreeMap<i64, Point> = totals
            .into_iter()
            .map(|(at_ms, acc)| {
                let (avg_mem, max_mem, avg_cpu, max_cpu) = acc.values();
                (
                    at_ms,
                    Point {
                        at_ms,
                        mem_avg: avg_mem,
                        mem_max: max_mem as i64,
                        cpu_pct_avg: avg_cpu,
                        cpu_pct_max: max_cpu as f32,
                        groups: BTreeMap::new(),
                    },
                )
            })
            .collect();
        for ((id, at_ms), acc) in group_acc {
            let Some(point) = points.get_mut(&at_ms) else {
                continue;
            };
            let name = names.get(&id).cloned().unwrap_or_else(|| id.to_string());
            let (avg_mem, max_mem, avg_cpu, max_cpu) = acc.values();
            point.groups.insert(
                name,
                GroupPoint {
                    rss_avg: avg_mem,
                    rss_max: max_mem as i64,
                    cpu_pct_avg: avg_cpu,
                    cpu_pct_max: max_cpu as f32,
                },
            );
        }
        Ok(points.into_values().collect())
    }
}

/// Charge le dictionnaire des programmes.
fn load_programs(conn: &Connection) -> rusqlite::Result<HashMap<String, u32>> {
    let mut stmt = conn.prepare("SELECT id, name FROM perf_programs")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)? as u32, row.get::<_, String>(1)?))
    })?;
    let mut programs = HashMap::new();
    for row in rows {
        let (id, name) = row?;
        programs.insert(name, id);
    }
    Ok(programs)
}

/// Identifiant d'un programme, créé à la volée s'il apparaît pour la première fois.
fn program_id(
    programs: &mut HashMap<String, u32>,
    tx: &rusqlite::Transaction,
    name: &str,
) -> rusqlite::Result<u32> {
    if let Some(id) = programs.get(name) {
        return Ok(*id);
    }
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM perf_programs WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .ok();
    let id = match existing {
        Some(id) => id as u32,
        None => {
            tx.execute("INSERT INTO perf_programs(name) VALUES (?1)", [name])?;
            // `perf_programs.id` est la rowid : jamais réutilisée dans une base
            // qui ne fait qu'ajouter des noms.
            tx.last_insert_rowid() as u32
        }
    };
    programs.insert(name.to_string(), id);
    Ok(id)
}

/// Accumulateur d'une tranche : moyenne et maximum, en une passe.
#[derive(Default)]
struct Acc {
    mem_sum: f64,
    mem_max: f64,
    cpu_sum: f64,
    cpu_max: f64,
    count: u64,
}

impl Acc {
    fn push(&mut self, mem: f64, cpu: f32) {
        self.mem_sum += mem;
        self.mem_max = self.mem_max.max(mem);
        self.cpu_sum += cpu as f64;
        self.cpu_max = self.cpu_max.max(cpu as f64);
        self.count += 1;
    }

    fn values(&self) -> (f64, f64, f64, f64) {
        let count = self.count.max(1) as f64;
        (
            self.mem_sum / count,
            self.mem_max,
            self.cpu_sum / count,
            self.cpu_max,
        )
    }
}

/// Tranche à servir pour une fenêtre : la plus fine qui reste sous la cible.
fn step_for(duration_ms: i64, sample_ms: i64) -> i64 {
    let minimum = sample_ms.max(1_000);
    STEP_LADDER
        .iter()
        .copied()
        .filter(|step| *step >= minimum)
        .find(|step| duration_ms / step <= TARGET_POINTS)
        .unwrap_or_else(|| *STEP_LADDER.last().unwrap_or(&3_600_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history() -> History {
        let dir = tempfile::tempdir().unwrap();
        // Le dossier temporaire doit vivre le temps du test : on fuit le chemin.
        let path = dir.keep().join("metrics.db");
        let settings = config::Performance {
            sample_secs: 60,
            ..Default::default()
        };
        History::open(&path, &settings).unwrap()
    }

    fn measurement(at_ms: i64, mem: i64, cpu: f32, rss: i64) -> Measurement {
        Measurement {
            at_ms,
            mem_total: mem,
            mem_anon: mem / 2,
            mem_file: mem / 4,
            mem_kernel: mem / 8,
            cpu_pct: cpu,
            heap_live: 1_000,
            sessions: 1,
            sockets: 2,
            groups: vec![GroupMeasurement {
                name: "easybook".into(),
                rss,
                cpu_pct: cpu,
            }],
        }
    }

    #[test]
    fn writes_and_reads_aggregates() {
        let history = history();
        // Aligné sur la minute **suivante** : les six mesures tombent dans la
        // même tranche et restent dans la fenêtre [maintenant-1h, maintenant).
        let base = (now_ms() - 3_600_000 + 59_999) / 60_000 * 60_000;
        for step in 0..6 {
            let at = base + step * 10_000;
            history
                .record(&measurement(at, 100 + step, 1.0 + step as f32, 40))
                .unwrap();
        }
        let series = history.series(Range::Hour).unwrap();
        assert_eq!(series.range, "1h");
        assert_eq!(series.points.len(), 1, "six mesures dans la même minute");
        let point = &series.points[0];
        assert_eq!(point.mem_max, 105);
        assert!((point.mem_avg - 102.5).abs() < 1e-9);
        let group = point.groups.get("easybook").expect("groupe");
        assert_eq!(group.rss_max, 40);
        assert!((group.cpu_pct_avg - 3.5).abs() < 1e-5);
    }

    #[test]
    fn keeps_distinct_programs_and_reuses_ids() {
        let history = history();
        let at = (now_ms() - 60_000) / 60_000 * 60_000;
        let mut first = measurement(at, 100, 1.0, 40);
        first.groups.push(GroupMeasurement {
            name: "node".into(),
            rss: 60,
            cpu_pct: 0.5,
        });
        history.record(&first).unwrap();
        history
            .record(&measurement(at + 10_000, 110, 2.0, 41))
            .unwrap();
        let series = history.series(Range::Hour).unwrap();
        let point = &series.points[0];
        assert_eq!(point.groups.len(), 2);
        assert_eq!(point.groups["node"].rss_max, 60);
        let inner = history.lock();
        assert_eq!(inner.programs.len(), 2);
    }

    #[test]
    fn drops_measurements_older_than_retention() {
        let settings = config::Performance {
            retention_days: 1,
            ..Default::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let history = History::open(&dir.path().join("m.db"), &settings).unwrap();
        // Une mesure de plus de 30 jours doit être inatteignable.
        let old = now_ms() - 31 * 86_400_000;
        history.record(&measurement(old, 100, 1.0, 40)).unwrap();
        let series = history.series(Range::Month).unwrap();
        assert!(series.points.is_empty());
    }

    #[test]
    fn picks_a_step_that_fits_the_target() {
        assert_eq!(step_for(3 * 60_000, 10_000), 10_000);
        assert_eq!(step_for(3_600_000, 10_000), 10_000);
        assert_eq!(step_for(86_400_000, 10_000), 120_000);
        assert_eq!(step_for(7 * 86_400_000, 10_000), 900_000);
        assert_eq!(step_for(30 * 86_400_000, 10_000), 3_600_000);
        // Un échantillonnage lent ne descend pas sous sa propre période.
        assert_eq!(step_for(3_600_000, 300_000), 300_000);
    }

    #[test]
    fn a_30_second_sampling_still_fills_every_window() {
        // Changer sample_secs ne casse pas les tranches : elles s'adaptent.
        assert_eq!(step_for(3 * 60_000, 30_000), 30_000); // 6 points
        assert_eq!(step_for(3_600_000, 30_000), 30_000); // 120 points
        assert_eq!(step_for(86_400_000, 30_000), 120_000); // 720 points
        assert_eq!(step_for(7 * 86_400_000, 30_000), 900_000); // 672 points
        assert_eq!(step_for(30 * 86_400_000, 30_000), 3_600_000); // 720 points
    }
}
