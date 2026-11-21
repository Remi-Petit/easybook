//! Les moteurs comparés, derrière une interface commune.
//!
//! Chaque moteur reçoit exactement la même charge et doit rendre les mêmes
//! agrégats. Les moteurs clé-valeur agrègent en Rust (c'est ce que ferait le
//! serveur), les moteurs SQL agrègent en SQL.

use crate::model::{Bucket, ProcSample, RawPoint, Sample, SampleSet, GROUPS};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

#[cfg(feature = "redb")]
use redb::ReadableTable;

pub trait Store {
    /// Écrit un lot de mesures, durablement, en une transaction.
    fn append(&mut self, batch: &[SampleSet]) -> Result<()>;
    fn raw(&mut self, from: i64, to: i64) -> Result<Vec<RawPoint>>;
    fn buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<Bucket>>;
    fn proc_buckets(&mut self, from: i64, to: i64, step: i64, idx: u16) -> Result<Vec<Bucket>>;
    /// Tous les groupes en une seule passe : `(nom, agrégat)`, triés par tranche.
    fn group_buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<(String, Bucket)>>;
    /// Supprime les mesures antérieures à `before` ; rend le nombre de mesures.
    fn trim(&mut self, before: i64) -> Result<u64>;
    /// Force le vidage sur disque et, si le moteur en a un, tronque le journal.
    fn flush(&mut self) -> Result<()>;
}

#[derive(Default)]
pub(crate) struct Acc {
    mem_sum: f64,
    mem_max: i64,
    cpu_sum: f64,
    cpu_max: f32,
    count: u64,
}

impl Acc {
    pub(crate) fn push(&mut self, mem: i64, cpu: f32) {
        self.mem_sum += mem as f64;
        self.mem_max = self.mem_max.max(mem);
        self.cpu_sum += cpu as f64;
        self.cpu_max = self.cpu_max.max(cpu);
        self.count += 1;
    }

    pub(crate) fn finish(self, at_ms: i64) -> Bucket {
        let count = self.count.max(1) as f64;
        Bucket {
            at_ms,
            avg_mem: self.mem_sum / count,
            max_mem: self.mem_max,
            avg_cpu: self.cpu_sum / count,
            max_cpu: self.cpu_max,
        }
    }
}

pub(crate) fn bucketize<I>(points: I, step: i64) -> Vec<Bucket>
where
    I: IntoIterator<Item = (i64, i64, f32)>,
{
    let mut buckets: BTreeMap<i64, Acc> = BTreeMap::new();
    for (at, mem, cpu) in points {
        buckets.entry(at / step * step).or_default().push(mem, cpu);
    }
    buckets
        .into_iter()
        .map(|(at_ms, acc)| acc.finish(at_ms))
        .collect()
}

/// Valeur d'une mesure agrégée : `mem_total, mem_anon, mem_file, cpu_pct`.
fn encode_sample(sample: &Sample) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(28);
    bytes.extend_from_slice(&sample.mem_total.to_le_bytes());
    bytes.extend_from_slice(&sample.mem_anon.to_le_bytes());
    bytes.extend_from_slice(&sample.mem_file.to_le_bytes());
    bytes.extend_from_slice(&sample.cpu_pct.to_le_bytes());
    bytes
}

fn decode_sample(bytes: &[u8]) -> (i64, f32) {
    let total = i64::from_le_bytes(bytes[0..8].try_into().unwrap());
    let cpu = f32::from_le_bytes(bytes[24..28].try_into().unwrap());
    (total, cpu)
}

fn encode_proc(proc: &ProcSample) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&proc.rss.to_le_bytes());
    bytes.extend_from_slice(&proc.cpu_pct.to_le_bytes());
    bytes
}

fn decode_proc(bytes: &[u8]) -> (i64, f32) {
    let rss = i64::from_le_bytes(bytes[0..8].try_into().unwrap());
    let cpu = f32::from_le_bytes(bytes[8..12].try_into().unwrap());
    (rss, cpu)
}

/// Clé composite d'une part de groupe : idx (16 bits) puis date (48 bits).
/// Reste dans un `u64` : la date tient jusqu'en l'an 10889.
fn proc_key(idx: u16, at_ms: i64) -> u64 {
    ((idx as u64) << 48) | (at_ms as u64 & 0x0000_FFFF_FFFF_FFFF)
}

/// Plage de clés pour LMDB : `heed` attend `RangeBounds<[u8]>`, alors que les
/// plages standard ne s'implémentent que pour des types dimensionnés.
#[cfg(feature = "heed")]
struct KeyRange {
    start: Option<Vec<u8>>,
    end: Option<Vec<u8>>,
}

#[cfg(feature = "heed")]
impl std::ops::RangeBounds<[u8]> for KeyRange {
    fn start_bound(&self) -> std::ops::Bound<&[u8]> {
        match &self.start {
            Some(start) => std::ops::Bound::Included(start.as_slice()),
            None => std::ops::Bound::Unbounded,
        }
    }

    fn end_bound(&self) -> std::ops::Bound<&[u8]> {
        match &self.end {
            Some(end) => std::ops::Bound::Excluded(end.as_slice()),
            None => std::ops::Bound::Unbounded,
        }
    }
}

// ---------------------------------------------------------------------------
// SQLite
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite")]
pub struct Sqlite {
    conn: rusqlite::Connection,
}

#[cfg(feature = "sqlite")]
impl Sqlite {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path).context("ouvrir SQLite")?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS samples(
                 at_ms INTEGER PRIMARY KEY,
                 mem_total INTEGER NOT NULL,
                 mem_anon INTEGER NOT NULL,
                 mem_file INTEGER NOT NULL,
                 cpu_pct REAL NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS procs(
                 at_ms INTEGER NOT NULL,
                 idx INTEGER NOT NULL,
                 rss INTEGER NOT NULL,
                 cpu_pct REAL NOT NULL,
                 PRIMARY KEY(at_ms, idx)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS procs_group ON procs(idx, at_ms, rss, cpu_pct);",
        )?;
        Ok(Self { conn })
    }
}

#[cfg(feature = "sqlite")]
impl Store for Sqlite {
    fn append(&mut self, batch: &[SampleSet]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut sample = tx.prepare_cached(
                "INSERT OR REPLACE INTO samples(at_ms, mem_total, mem_anon, mem_file, cpu_pct)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut proc = tx.prepare_cached(
                "INSERT OR REPLACE INTO procs(at_ms, idx, rss, cpu_pct) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for set in batch {
                let s = &set.sample;
                sample.execute(rusqlite::params![
                    s.at_ms,
                    s.mem_total,
                    s.mem_anon,
                    s.mem_file,
                    s.cpu_pct
                ])?;
                for p in &set.procs {
                    proc.execute(rusqlite::params![p.at_ms, p.idx, p.rss, p.cpu_pct])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn raw(&mut self, from: i64, to: i64) -> Result<Vec<RawPoint>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT at_ms, mem_total, cpu_pct FROM samples
             WHERE at_ms >= ?1 AND at_ms < ?2 ORDER BY at_ms",
        )?;
        let rows = stmt.query_map([from, to], |row| {
            Ok(RawPoint {
                at_ms: row.get(0)?,
                mem_total: row.get(1)?,
                cpu_pct: row.get::<_, f64>(2)? as f32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<Bucket>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT (at_ms / ?1) * ?1 AS bucket,
                    avg(mem_total), max(mem_total), avg(cpu_pct), max(cpu_pct)
             FROM samples WHERE at_ms >= ?2 AND at_ms < ?3
             GROUP BY bucket ORDER BY bucket",
        )?;
        let rows = stmt.query_map([step, from, to], |row| {
            Ok(Bucket {
                at_ms: row.get(0)?,
                avg_mem: row.get(1)?,
                max_mem: row.get(2)?,
                avg_cpu: row.get(3)?,
                max_cpu: row.get::<_, f64>(4)? as f32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn proc_buckets(&mut self, from: i64, to: i64, step: i64, idx: u16) -> Result<Vec<Bucket>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT (at_ms / ?1) * ?1 AS bucket,
                    avg(rss), max(rss), avg(cpu_pct), max(cpu_pct)
             FROM procs WHERE idx = ?4 AND at_ms >= ?2 AND at_ms < ?3
             GROUP BY bucket ORDER BY bucket",
        )?;
        let rows = stmt.query_map([step, from, to, idx as i64], |row| {
            Ok(Bucket {
                at_ms: row.get(0)?,
                avg_mem: row.get(1)?,
                max_mem: row.get(2)?,
                avg_cpu: row.get(3)?,
                max_cpu: row.get::<_, f64>(4)? as f32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn group_buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<(String, Bucket)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT idx, (at_ms / ?1) * ?1 AS bucket,
                    avg(rss), max(rss), avg(cpu_pct), max(cpu_pct)
             FROM procs INDEXED BY procs_group WHERE at_ms >= ?2 AND at_ms < ?3
             GROUP BY idx, bucket ORDER BY bucket, idx",
        )?;
        let rows = stmt.query_map([step, from, to], |row| {
            let idx: i64 = row.get(0)?;
            Ok((
                GROUPS[idx as usize].to_string(),
                Bucket {
                    at_ms: row.get(1)?,
                    avg_mem: row.get(2)?,
                    max_mem: row.get(3)?,
                    avg_cpu: row.get(4)?,
                    max_cpu: row.get::<_, f64>(5)? as f32,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn trim(&mut self, before: i64) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute("DELETE FROM samples WHERE at_ms < ?1", [before])? as u64;
        tx.execute("DELETE FROM procs WHERE at_ms < ?1", [before])?;
        tx.commit()?;
        Ok(removed)
    }

    fn flush(&mut self) -> Result<()> {
        // Comme le fera le serveur : un point de contrôle tronque le journal.
        self.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SQLite, une ligne par mesure (les groupes en BLOB compact)
// ---------------------------------------------------------------------------

/// Variante retenue pour l'application : une seule table, les quatre groupes
/// tenant dans 48 octets par mesure. L'agrégat de tous les groupes se fait en
/// Rust sur un seul balayage, au lieu d'un `GROUP BY` sur 1,3 million de lignes.
#[cfg(feature = "sqlite")]
pub struct SqliteBlob {
    conn: rusqlite::Connection,
}

#[cfg(feature = "sqlite")]
fn encode_groups(procs: &[ProcSample]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(procs.len() * 12);
    for proc in procs {
        bytes.extend_from_slice(&proc.rss.to_le_bytes());
        bytes.extend_from_slice(&proc.cpu_pct.to_le_bytes());
    }
    bytes
}

#[cfg(feature = "sqlite")]
fn decode_groups(bytes: &[u8]) -> Vec<(i64, f32)> {
    bytes
        .chunks_exact(12)
        .map(|chunk| {
            (
                i64::from_le_bytes(chunk[0..8].try_into().unwrap()),
                f32::from_le_bytes(chunk[8..12].try_into().unwrap()),
            )
        })
        .collect()
}

#[cfg(feature = "sqlite")]
impl SqliteBlob {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path).context("ouvrir SQLite")?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS samples(
                 at_ms INTEGER PRIMARY KEY,
                 mem_total INTEGER NOT NULL,
                 mem_anon INTEGER NOT NULL,
                 mem_file INTEGER NOT NULL,
                 cpu_pct REAL NOT NULL,
                 groups BLOB NOT NULL
             ) WITHOUT ROWID;",
        )?;
        Ok(Self { conn })
    }
}

#[cfg(feature = "sqlite")]
impl Store for SqliteBlob {
    fn append(&mut self, batch: &[SampleSet]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO samples
                 (at_ms, mem_total, mem_anon, mem_file, cpu_pct, groups)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for set in batch {
                let s = &set.sample;
                stmt.execute(rusqlite::params![
                    s.at_ms,
                    s.mem_total,
                    s.mem_anon,
                    s.mem_file,
                    s.cpu_pct,
                    encode_groups(&set.procs)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn raw(&mut self, from: i64, to: i64) -> Result<Vec<RawPoint>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT at_ms, mem_total, cpu_pct FROM samples
             WHERE at_ms >= ?1 AND at_ms < ?2 ORDER BY at_ms",
        )?;
        let rows = stmt.query_map([from, to], |row| {
            Ok(RawPoint {
                at_ms: row.get(0)?,
                mem_total: row.get(1)?,
                cpu_pct: row.get::<_, f64>(2)? as f32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<Bucket>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT (at_ms / ?1) * ?1 AS bucket,
                    avg(mem_total), max(mem_total), avg(cpu_pct), max(cpu_pct)
             FROM samples WHERE at_ms >= ?2 AND at_ms < ?3
             GROUP BY bucket ORDER BY bucket",
        )?;
        let rows = stmt.query_map([step, from, to], |row| {
            Ok(Bucket {
                at_ms: row.get(0)?,
                avg_mem: row.get(1)?,
                max_mem: row.get(2)?,
                avg_cpu: row.get(3)?,
                max_cpu: row.get::<_, f64>(4)? as f32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn proc_buckets(&mut self, from: i64, to: i64, step: i64, idx: u16) -> Result<Vec<Bucket>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT at_ms, groups FROM samples WHERE at_ms >= ?1 AND at_ms < ?2 ORDER BY at_ms",
        )?;
        let rows = stmt.query_map([from, to], |row| {
            let at_ms: i64 = row.get(0)?;
            let groups: Vec<u8> = row.get(1)?;
            let (rss, cpu_pct) = decode_groups(&groups)[idx as usize];
            Ok((at_ms, rss, cpu_pct))
        })?;
        let points = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(bucketize(points, step))
    }

    fn group_buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<(String, Bucket)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT at_ms, groups FROM samples WHERE at_ms >= ?1 AND at_ms < ?2 ORDER BY at_ms",
        )?;
        let rows = stmt.query_map([from, to], |row| {
            let at_ms: i64 = row.get(0)?;
            let groups: Vec<u8> = row.get(1)?;
            Ok((at_ms, groups))
        })?;
        let mut buckets: BTreeMap<(usize, i64), Acc> = BTreeMap::new();
        for row in rows {
            let (at_ms, groups) = row?;
            let bucket = at_ms / step * step;
            for (idx, (rss, cpu_pct)) in decode_groups(&groups).into_iter().enumerate() {
                buckets
                    .entry((idx, bucket))
                    .or_default()
                    .push(rss, cpu_pct);
            }
        }
        Ok(buckets
            .into_iter()
            .map(|((idx, at_ms), acc)| (GROUPS[idx].to_string(), acc.finish(at_ms)))
            .collect())
    }

    fn trim(&mut self, before: i64) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute("DELETE FROM samples WHERE at_ms < ?1", [before])? as u64;
        tx.commit()?;
        Ok(removed)
    }

    fn flush(&mut self) -> Result<()> {
        self.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DuckDB
// ---------------------------------------------------------------------------
#[cfg(feature = "duckdb")]
pub struct Duck {
    conn: duckdb::Connection,
}

#[cfg(feature = "duckdb")]
impl Duck {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = duckdb::Connection::open(path).context("ouvrir DuckDB")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS samples(
                 at_ms BIGINT PRIMARY KEY,
                 mem_total BIGINT NOT NULL,
                 mem_anon BIGINT NOT NULL,
                 mem_file BIGINT NOT NULL,
                 cpu_pct REAL NOT NULL
             );
             CREATE TABLE IF NOT EXISTS procs(
                 at_ms BIGINT NOT NULL,
                 idx INTEGER NOT NULL,
                 rss BIGINT NOT NULL,
                 cpu_pct REAL NOT NULL,
                 PRIMARY KEY(at_ms, idx)
             );
             CREATE INDEX IF NOT EXISTS procs_group ON procs(idx, at_ms);",
        )?;
        Ok(Self { conn })
    }
}

#[cfg(feature = "duckdb")]
impl Store for Duck {
    fn append(&mut self, batch: &[SampleSet]) -> Result<()> {
        let mut samples = self.conn.appender("samples")?;
        let mut procs = self.conn.appender("procs")?;
        for set in batch {
            let s = &set.sample;
            samples.append_row(duckdb::params![
                s.at_ms,
                s.mem_total,
                s.mem_anon,
                s.mem_file,
                s.cpu_pct
            ])?;
            for p in &set.procs {
                procs.append_row(duckdb::params![p.at_ms, p.idx as i32, p.rss, p.cpu_pct])?;
            }
        }
        samples.flush()?;
        procs.flush()?;
        Ok(())
    }

    fn raw(&mut self, from: i64, to: i64) -> Result<Vec<RawPoint>> {
        let sql = format!(
            "SELECT at_ms, mem_total, cpu_pct FROM samples\n             WHERE at_ms >= {from} AND at_ms < {to} ORDER BY at_ms"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(duckdb::params![], |row| {
            Ok(RawPoint {
                at_ms: row.get(0)?,
                mem_total: row.get(1)?,
                cpu_pct: row.get::<_, f32>(2)?,
            })
        })?;
        Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
    }

    fn buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<Bucket>> {
        let sql = format!(
            "SELECT (at_ms // {step}) * {step} AS bucket,\n                    avg(mem_total), max(mem_total), avg(cpu_pct), max(cpu_pct)\n             FROM samples WHERE at_ms >= {from} AND at_ms < {to}\n             GROUP BY bucket ORDER BY bucket"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(duckdb::params![], |row| {
            Ok(Bucket {
                at_ms: row.get(0)?,
                avg_mem: row.get(1)?,
                max_mem: row.get(2)?,
                avg_cpu: row.get(3)?,
                max_cpu: row.get::<_, f32>(4)?,
            })
        })?;
        Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
    }

    fn proc_buckets(&mut self, from: i64, to: i64, step: i64, idx: u16) -> Result<Vec<Bucket>> {
        let sql = format!(
            "SELECT (at_ms // {step}) * {step} AS bucket,\n                    avg(rss), max(rss), avg(cpu_pct), max(cpu_pct)\n             FROM procs WHERE idx = {idx} AND at_ms >= {from} AND at_ms < {to}\n             GROUP BY bucket ORDER BY bucket"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(duckdb::params![], |row| {
            Ok(Bucket {
                at_ms: row.get(0)?,
                avg_mem: row.get(1)?,
                max_mem: row.get(2)?,
                avg_cpu: row.get(3)?,
                max_cpu: row.get::<_, f32>(4)?,
            })
        })?;
        Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
    }

    fn group_buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<(String, Bucket)>> {
        let sql = format!(
            "SELECT idx, (at_ms // {step}) * {step} AS bucket,\n                    avg(rss), max(rss), avg(cpu_pct), max(cpu_pct)\n             FROM procs WHERE at_ms >= {from} AND at_ms < {to}\n             GROUP BY idx, bucket ORDER BY bucket, idx"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(duckdb::params![], |row| {
            let idx: i64 = row.get(0)?;
            Ok((
                GROUPS[idx as usize].to_string(),
                Bucket {
                    at_ms: row.get(1)?,
                    avg_mem: row.get(2)?,
                    max_mem: row.get(3)?,
                    avg_cpu: row.get(4)?,
                    max_cpu: row.get::<_, f32>(5)?,
                },
            ))
        })?;
        Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
    }

    fn trim(&mut self, before: i64) -> Result<u64> {
        let removed = self
            .conn
            .execute("DELETE FROM samples WHERE at_ms < ?", duckdb::params![before])?;
        self.conn
            .execute("DELETE FROM procs WHERE at_ms < ?", duckdb::params![before])?;
        Ok(removed as u64)
    }

    fn flush(&mut self) -> Result<()> {
        self.conn.execute_batch("CHECKPOINT;")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// redb
// ---------------------------------------------------------------------------

#[cfg(feature = "redb")]
pub struct Redb {
    db: redb::Database,
}

#[cfg(feature = "redb")]
const REDB_SAMPLES: redb::TableDefinition<u64, &[u8]> =
    redb::TableDefinition::new("samples");
#[cfg(feature = "redb")]
const REDB_PROCS: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("procs");

#[cfg(feature = "redb")]
impl Redb {
    pub fn open(path: &Path) -> Result<Self> {
        let db = redb::Database::create(path).context("ouvrir redb")?;
        let txn = db.begin_write()?;
        {
            txn.open_table(REDB_SAMPLES)?;
            txn.open_table(REDB_PROCS)?;
        }
        txn.commit()?;
        Ok(Self { db })
    }
}

#[cfg(feature = "redb")]
impl Store for Redb {
    fn append(&mut self, batch: &[SampleSet]) -> Result<()> {
        let txn = self.db.begin_write()?;
        {
            let mut samples = txn.open_table(REDB_SAMPLES)?;
            let mut procs = txn.open_table(REDB_PROCS)?;
            for set in batch {
                samples.insert(set.sample.at_ms as u64, encode_sample(&set.sample).as_slice())?;
                for p in &set.procs {
                    procs.insert(proc_key(p.idx, p.at_ms), encode_proc(p).as_slice())?;
                }
            }
        }
        txn.commit()?;
        Ok(())
    }

    fn raw(&mut self, from: i64, to: i64) -> Result<Vec<RawPoint>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(REDB_SAMPLES)?;
        let mut out = Vec::new();
        for entry in table.range(from as u64..to as u64)? {
            let (key, value) = entry?;
            let at_ms = key.value() as i64;
            let (mem_total, cpu_pct) = decode_sample(value.value());
            out.push(RawPoint {
                at_ms,
                mem_total,
                cpu_pct,
            });
        }
        Ok(out)
    }

    fn buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<Bucket>> {
        let points = self.raw(from, to)?;
        Ok(bucketize(
            points.into_iter().map(|p| (p.at_ms, p.mem_total, p.cpu_pct)),
            step,
        ))
    }

    fn proc_buckets(&mut self, from: i64, to: i64, step: i64, idx: u16) -> Result<Vec<Bucket>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(REDB_PROCS)?;
        let mut points = Vec::new();
        for entry in table.range(proc_key(idx, from)..proc_key(idx, to))? {
            let (key, value) = entry?;
            let at_ms = (key.value() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let (rss, cpu_pct) = decode_proc(value.value());
            points.push((at_ms, rss, cpu_pct));
        }
        Ok(bucketize(points, step))
    }

    fn group_buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<(String, Bucket)>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(REDB_PROCS)?;
        let mut buckets: BTreeMap<(usize, i64), Acc> = BTreeMap::new();
        for idx in 0..GROUPS.len() {
            for entry in table.range(proc_key(idx as u16, from)..proc_key(idx as u16, to))? {
                let (key, value) = entry?;
                let at_ms = (key.value() & 0x0000_FFFF_FFFF_FFFF) as i64;
                let (rss, cpu_pct) = decode_proc(value.value());
                buckets
                    .entry((idx, at_ms / step * step))
                    .or_default()
                    .push(rss, cpu_pct);
            }
        }
        Ok(buckets
            .into_iter()
            .map(|((idx, at_ms), acc)| (GROUPS[idx].to_string(), acc.finish(at_ms)))
            .collect())
    }

    fn trim(&mut self, before: i64) -> Result<u64> {
        let txn = self.db.begin_write()?;
        let removed;
        {
            let mut samples = txn.open_table(REDB_SAMPLES)?;
            let keys: Vec<u64> = samples
                .range(0..before as u64)?
                .map(|entry| Ok(entry?.0.value()))
                .collect::<Result<Vec<_>>>()?;
            removed = keys.len() as u64;
            for key in keys {
                samples.remove(key)?;
            }
            let mut procs = txn.open_table(REDB_PROCS)?;
            let keys: Vec<u64> = procs
                .iter()?
                .map(|entry| Ok(entry?.0.value()))
                .collect::<Result<Vec<_>>>()?;
            for key in keys {
                if (key & 0x0000_FFFF_FFFF_FFFF) < before as u64 {
                    procs.remove(key)?;
                }
            }
        }
        txn.commit()?;
        Ok(removed)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LMDB (heed)
// ---------------------------------------------------------------------------

#[cfg(feature = "heed")]
pub struct Lmdb {
    env: heed::Env,
    samples: heed::Database<heed::types::Bytes, heed::types::Bytes>,
    procs: heed::Database<heed::types::Bytes, heed::types::Bytes>,
}

#[cfg(feature = "heed")]
impl Lmdb {
    pub fn open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)?;
        let mut options = heed::EnvOpenOptions::new();
        options.map_size(4 * 1024 * 1024 * 1024).max_dbs(2);
        // `open` est `unsafe` : la taille de mapping doit rester sous celle du
        // disque, ce qui est le cas ici (30 jours ≈ quelques dizaines de Mo).
        let env = unsafe { options.open(path) }.context("ouvrir LMDB")?;
        let mut txn = env.write_txn().context("transaction LMDB")?;
        let samples = env
            .create_database(&mut txn, Some("samples"))
            .context("créer la base samples")?;
        let procs = env
            .create_database(&mut txn, Some("procs"))
            .context("créer la base procs")?;
        txn.commit().context("valider la création")?;
        Ok(Self {
            env,
            samples,
            procs,
        })
    }
}

#[cfg(feature = "heed")]
impl Store for Lmdb {
    fn append(&mut self, batch: &[SampleSet]) -> Result<()> {
        let mut txn = self.env.write_txn()?;
        for set in batch {
            let key = (set.sample.at_ms as u64).to_be_bytes();
            self.samples
                .put(&mut txn, &key[..], &encode_sample(&set.sample))
                .with_context(|| format!("put sample at {}", set.sample.at_ms))?;
            for p in &set.procs {
                let key = proc_key(p.idx, p.at_ms).to_be_bytes();
                self.procs
                    .put(&mut txn, &key[..], &encode_proc(p))
                    .with_context(|| format!("put proc {} at {}", p.idx, p.at_ms))?;
            }
        }
        txn.commit()?;
        Ok(())
    }

    fn raw(&mut self, from: i64, to: i64) -> Result<Vec<RawPoint>> {
        let txn = self.env.read_txn()?;
        let start = (from as u64).to_be_bytes();
        let end = (to as u64).to_be_bytes();
        let mut out = Vec::new();
        for entry in self.samples.range(
            &txn,
            &KeyRange {
                start: Some(start.to_vec()),
                end: Some(end.to_vec()),
            },
        )? {
            let (key, value) = entry?;
            let at_ms = u64::from_be_bytes(key.try_into().unwrap()) as i64;
            let (mem_total, cpu_pct) = decode_sample(value);
            out.push(RawPoint {
                at_ms,
                mem_total,
                cpu_pct,
            });
        }
        Ok(out)
    }

    fn buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<Bucket>> {
        let points = self.raw(from, to)?;
        Ok(bucketize(
            points.into_iter().map(|p| (p.at_ms, p.mem_total, p.cpu_pct)),
            step,
        ))
    }

    fn proc_buckets(&mut self, from: i64, to: i64, step: i64, idx: u16) -> Result<Vec<Bucket>> {
        let txn = self.env.read_txn()?;
        let start = proc_key(idx, from).to_be_bytes();
        let end = proc_key(idx, to).to_be_bytes();
        let mut points = Vec::new();
        for entry in self.procs.range(
            &txn,
            &KeyRange {
                start: Some(start.to_vec()),
                end: Some(end.to_vec()),
            },
        )? {
            let (key, value) = entry?;
            let key = u64::from_be_bytes(key.try_into().unwrap());
            let at_ms = (key & 0x0000_FFFF_FFFF_FFFF) as i64;
            let (rss, cpu_pct) = decode_proc(value);
            points.push((at_ms, rss, cpu_pct));
        }
        Ok(bucketize(points, step))
    }

    fn group_buckets(&mut self, from: i64, to: i64, step: i64) -> Result<Vec<(String, Bucket)>> {
        let txn = self.env.read_txn()?;
        let mut buckets: BTreeMap<(usize, i64), Acc> = BTreeMap::new();
        for idx in 0..GROUPS.len() {
            let start = proc_key(idx as u16, from).to_be_bytes();
            let end = proc_key(idx as u16, to).to_be_bytes();
            for entry in self.procs.range(
                &txn,
                &KeyRange {
                    start: Some(start.to_vec()),
                    end: Some(end.to_vec()),
                },
            )? {
                let (key, value) = entry?;
                let key = u64::from_be_bytes(key.try_into().unwrap());
                let at_ms = (key & 0x0000_FFFF_FFFF_FFFF) as i64;
                let (rss, cpu_pct) = decode_proc(value);
                buckets
                    .entry((idx, at_ms / step * step))
                    .or_default()
                    .push(rss, cpu_pct);
            }
        }
        Ok(buckets
            .into_iter()
            .map(|((idx, at_ms), acc)| (GROUPS[idx].to_string(), acc.finish(at_ms)))
            .collect())
    }

    fn trim(&mut self, before: i64) -> Result<u64> {
        let txn = self.env.read_txn()?;
        let end = (before as u64).to_be_bytes();
        let keys: Vec<Vec<u8>> = self
            .samples
            .range(
                &txn,
                &KeyRange {
                    start: None,
                    end: Some(end.to_vec()),
                },
            )?
            .map(|entry| Ok(entry?.0.to_vec()))
            .collect::<Result<Vec<_>>>()?;
        drop(txn);
        let mut txn = self.env.write_txn()?;
        for key in &keys {
            self.samples.delete(&mut txn, key.as_slice())?;
        }
        let mut proc_keys: Vec<Vec<u8>> = Vec::new();
        {
            let rtxn = self.env.read_txn()?;
            for entry in self.procs.iter(&rtxn)? {
                let (key, _) = entry?;
                if u64::from_be_bytes(key.try_into().unwrap()) & 0x0000_FFFF_FFFF_FFFF
                    < before as u64
                {
                    proc_keys.push(key.to_vec());
                }
            }
        }
        for key in &proc_keys {
            self.procs.delete(&mut txn, key.as_slice())?;
        }
        txn.commit()?;
        Ok(keys.len() as u64)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

pub fn names() -> Vec<&'static str> {
    vec![
        #[cfg(feature = "sqlite")]
        "sqlite-wal",
        #[cfg(feature = "sqlite")]
        "sqlite-blob",
        #[cfg(feature = "duckdb")]
        "duckdb",
        #[cfg(feature = "redb")]
        "redb",
        #[cfg(feature = "heed")]
        "lmdb",
    ]
}

pub fn open(name: &str, dir: &Path) -> Result<Box<dyn Store>> {
    match name {
        #[cfg(feature = "sqlite")]
        "sqlite-wal" => Ok(Box::new(Sqlite::open(&dir.join("sqlite.db"))?)),
        #[cfg(feature = "sqlite")]
        "sqlite-blob" => Ok(Box::new(SqliteBlob::open(&dir.join("sqlite.db"))?)),
        #[cfg(feature = "duckdb")]
        "duckdb" => Ok(Box::new(Duck::open(&dir.join("duck.duckdb"))?)),
        #[cfg(feature = "redb")]
        "redb" => Ok(Box::new(Redb::open(&dir.join("redb.db"))?)),
        #[cfg(feature = "heed")]
        "lmdb" => Ok(Box::new(Lmdb::open(&dir.join("lmdb"))?)),
        other => anyhow::bail!(
            "moteur inconnu : {other} (disponibles : {})",
            names().join(", ")
        ),
    }
}

/// Taille apparente de la base : tous les fichiers du dossier (journal compris).
pub fn disk_bytes(dir: &Path) -> u64 {
    fn walk(dir: &Path, total: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, total);
            } else if let Ok(meta) = path.metadata() {
                *total += meta.len();
            }
        }
    }
    let mut total = 0;
    walk(dir, &mut total);
    total
}

/// Nombre de groupes (pour les rapports).
pub fn group_count() -> usize {
    GROUPS.len()
}
