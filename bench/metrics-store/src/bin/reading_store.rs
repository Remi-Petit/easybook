//! Banc d'essai du stockage de l'**historique de lecture** (par utilisateur).
//!
//! Charge visée : celle d'easybook — quelques lecteurs à la fois, une position
//! mise à jour à chaque changement de page/chapitre, et une page
//! d'administration qui liste « qui en est où ». Rien à voir avec les 259 200
//! mesures de l'historique de performances : ici, ce sont des **écritures
//! aléatoires en place** (upsert) sur une table petite (utilisateurs × livres),
//! et des lectures triées par date.
//!
//! Variantes comparées :
//!
//! - `sqlite` : deux tables (une position par livre, une ligne par session),
//!   `INSERT … ON CONFLICT DO UPDATE`, WAL — ce que fait déjà `accounts.db`.
//! - `sqlite-blob` : une ligne par utilisateur contenant un JSON
//!   `{livre: position}` réécrit à chaque écriture. Séduisant (un seul écrivain
//!   par utilisateur), à mesurer.
//! - `redb` : clé-valeur pur Rust (deux tables + deux index écrits à la main).
//! - `duckdb` : les deux tables SQL, moteur colonnaire (feature `duckdb`).
//!
//! Usage :
//!
//!     cargo run --release --bin reading-store-bench -- --engine sqlite
//!     cargo run --release --bin reading-store-bench -- --engine sqlite-blob
//!     cargo run --release --bin reading-store-bench -- --engine redb
//!     cargo run --release --no-default-features --features duckdb \
//!         --bin reading-store-bench -- --engine duckdb
//!
//! Options : `--users 20`, `--books 2000`, `--reads 400` (livres lus par
//! utilisateur), `--writes 3000`, `--dir <dossier>`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Instant;

const SEC: i64 = 1000;
const MIN: i64 = 60 * SEC;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;

/// Une position : l'état d'un livre pour un utilisateur.
#[derive(Clone, Debug)]
struct ProgressRow {
    user: String,
    book: String,
    page: i64,
    pages: i64,
    started_at: i64,
    updated_at: i64,
}

/// Une session de lecture : ouverte au premier contenu demandé, close après
/// inactivité (voir `readings::IDLE` dans le serveur).
#[derive(Clone, Debug)]
struct SessionRow {
    id: i64,
    user: String,
    book: String,
    started_at: i64,
    last_at: i64,
    end_page: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Totals {
    books: i64,
    finished: i64,
    pages: i64,
}

/// Ce qu'une page d'administration demande.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RecentRow {
    book: String,
    page: i64,
    pages: i64,
    at: i64,
}

trait Store {
    /// Crée ou met à jour la position d'un livre.
    fn progress(&mut self, row: &ProgressRow) -> Result<()>;
    /// Ouvre une session.
    fn open_session(&mut self, row: &SessionRow) -> Result<()>;
    /// Met à jour la session en cours (dernière page, dernière activité).
    fn touch_session(&mut self, id: i64, at: i64, page: i64) -> Result<()>;
    /// Les `limit` livres les plus récemment lus par un utilisateur.
    fn recent(&mut self, user: &str, limit: usize) -> Result<Vec<RecentRow>>;
    /// Livres commencés, terminés, pages cumulées.
    fn totals(&mut self, user: &str) -> Result<Totals>;
    /// Les `limit` dernières sessions d'un utilisateur.
    fn sessions(&mut self, user: &str, limit: usize) -> Result<Vec<RecentRow>>;
    fn flush(&mut self) -> Result<()>;
}

fn main() -> Result<()> {
    let args = Args::parse()?;

    let dir = args.dir.join(&args.engine);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("vider {}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir)?;

    let data = Data::generate(&args);
    let writes = data.writes();

    let open_at = Instant::now();
    let mut store = open(&args.engine, &dir)?;
    let open_ms = open_at.elapsed().as_secs_f64() * 1000.0;

    // 1. Remplissage : chaque utilisateur a lu `reads` livres (position + session).
    let fill_at = Instant::now();
    for row in &data.progress {
        store.progress(row)?;
    }
    for row in &data.sessions {
        store.open_session(row)?;
    }
    store.flush()?;
    let fill_ms = fill_at.elapsed().as_secs_f64() * 1000.0;

    // 2. Écritures du lecteur : une position + sa session, durablement. C'est le
    //    chemin chaud : le client envoie sa position au changement de page.
    let mut durations = Vec::with_capacity(writes.len());
    for write in writes {
        let at = Instant::now();
        store.progress(&write.progress)?;
        store.touch_session(write.session_id, write.progress.updated_at, write.progress.page)?;
        durations.push(at.elapsed().as_secs_f64() * 1e6);
    }
    store.flush()?;
    durations.sort_by(f64::total_cmp);
    let write_p50 = percentile(&durations, 0.50);
    let write_p99 = percentile(&durations, 0.99);

    // 3. Les trois lectures de la page d'administration (médiane de 5 passes).
    let recent_us = median(|| {
        let rows = store.recent("user-07", 50).expect("recent");
        rows.len()
    });
    let totals_us = median(|| {
        let totals = store.totals("user-07").expect("totaux");
        (totals.books + totals.finished + totals.pages) as usize
    });
    let sessions_us = median(|| {
        let rows = store.sessions("user-07", 100).expect("sessions");
        rows.len()
    });

    // 4. Contrôle : la même question posée à chaque moteur doit donner la même
    //    réponse (comparaison avec la référence calculée en Rust).
    let mut mismatches = 0;
    let recent = store.recent("user-03", 50)?;
    let expected = data.recent("user-03", 50);
    mismatches += usize::from(recent != expected);
    let totals = store.totals("user-03")?;
    mismatches += usize::from(totals != data.totals("user-03"));
    let sessions = store.sessions("user-03", 100)?;
    mismatches += usize::from(sessions != data.sessions("user-03", 100));

    let disk_mb = disk_bytes(&dir) as f64 / (1024.0 * 1024.0);
    let rss_mb = memory_stats::memory_stats()
        .map(|stats| stats.physical_mem as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0);

    println!(
        "{{\"engine\":\"{}\",\"users\":{},\"books\":{},\"rows\":{},\"open_ms\":{:.1},\
         \"fill_ms\":{:.1},\"write_p50_us\":{:.1},\"write_p99_us\":{:.1},\
         \"recent_us\":{:.1},\"totals_us\":{:.1},\"sessions_us\":{:.1},\
         \"disk_mb\":{:.1},\"rss_mb\":{:.1},\"mismatches\":{}}}",
        args.engine,
        args.users,
        args.books,
        data.progress.len() + data.sessions.len(),
        open_ms,
        fill_ms,
        write_p50,
        write_p99,
        recent_us,
        totals_us,
        sessions_us,
        disk_mb,
        rss_mb,
        mismatches
    );
    Ok(())
}

// ── Charge ──────────────────────────────────────────────────────────────────

/// Une écriture du lecteur : la nouvelle position et la session à toucher.
struct Write {
    progress: ProgressRow,
    session_id: i64,
}

struct Data {
    progress: Vec<ProgressRow>,
    sessions: Vec<SessionRow>,
    writes: Vec<Write>,
}

/// Suite pseudo-aléatoire déterministe (même charge d'un moteur à l'autre).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

impl Data {
    fn generate(args: &Args) -> Self {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let start = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64)
            .max(3 * DAY);
        let mut progress = Vec::with_capacity(args.users * args.reads);
        let mut sessions = Vec::with_capacity(args.users * args.reads);
        let mut next_id = 1_i64;

        for user in 0..args.users {
            for slot in 0..args.reads {
                let book = format!("mangas/serie-{}/tome-{:02}.cbz", rng.below(args.books / 10), slot);
                let pages = 40 + rng.below(360) as i64;
                let started_at = start - (args.reads as i64 - slot as i64) * HOUR;
                let updated_at = started_at + MIN;
                progress.push(ProgressRow {
                    user: format!("user-{user:02}"),
                    book: book.clone(),
                    page: pages / 3,
                    pages,
                    started_at,
                    updated_at,
                });
                sessions.push(SessionRow {
                    id: next_id,
                    user: format!("user-{user:02}"),
                    book,
                    started_at,
                    last_at: updated_at,
                    end_page: pages / 3,
                });
                next_id += 1;
            }
        }

        // Écritures : des positions qui avancent, sur des livres déjà ouverts.
        let mut writes = Vec::with_capacity(args.writes);
        for index in 0..args.writes {
            let pick = rng.below(progress.len());
            let mut row = progress[pick].clone();
            row.page = (row.page + 1).min(row.pages - 1);
            row.updated_at = start - (args.writes as i64 - index as i64) * SEC;
            let session_id = sessions[pick].id;
            progress[pick] = row.clone();
            sessions[pick].last_at = row.updated_at;
            sessions[pick].end_page = row.page;
            writes.push(Write {
                progress: row,
                session_id,
            });
        }

        Self {
            progress,
            sessions,
            writes,
        }
    }

    fn writes(&self) -> &[Write] {
        &self.writes
    }

    fn recent(&self, user: &str, limit: usize) -> Vec<RecentRow> {
        let mut rows: Vec<RecentRow> = self
            .progress
            .iter()
            .filter(|row| row.user == user)
            .map(|row| RecentRow {
                book: row.book.clone(),
                page: row.page,
                pages: row.pages,
                at: row.updated_at,
            })
            .collect();
        rows.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.book.cmp(&b.book)));
        rows.truncate(limit);
        rows
    }

    fn totals(&self, user: &str) -> Totals {
        let rows = self.progress.iter().filter(|row| row.user == user);
        let mut totals = Totals {
            books: 0,
            finished: 0,
            pages: 0,
        };
        for row in rows {
            totals.books += 1;
            totals.pages += row.page + 1;
            if row.page + 1 >= row.pages {
                totals.finished += 1;
            }
        }
        totals
    }

    fn sessions(&self, user: &str, limit: usize) -> Vec<RecentRow> {
        let mut rows: Vec<(i64, RecentRow)> = self
            .sessions
            .iter()
            .filter(|row| row.user == user)
            .map(|row| {
                (
                    row.id,
                    RecentRow {
                        book: row.book.clone(),
                        page: row.end_page,
                        pages: 0,
                        at: row.last_at,
                    },
                )
            })
            .collect();
        rows.sort_by(|a, b| b.1.at.cmp(&a.1.at).then_with(|| b.0.cmp(&a.0)));
        rows.into_iter().take(limit).map(|(_, row)| row).collect()
    }
}

use std::time::{SystemTime, UNIX_EPOCH};

struct Args {
    engine: String,
    users: usize,
    books: usize,
    reads: usize,
    writes: usize,
    dir: PathBuf,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut args = Self {
            engine: String::new(),
            users: 20,
            books: 2000,
            reads: 400,
            writes: 3000,
            dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".data"),
        };
        let mut iter = std::env::args().skip(1);
        while let Some(arg) = iter.next() {
            let mut value = || -> Result<String> {
                iter.next()
                    .with_context(|| format!("valeur manquante après {arg}"))
            };
            match arg.as_str() {
                "--engine" => args.engine = value()?,
                "--users" => args.users = value()?.parse()?,
                "--books" => args.books = value()?.parse()?,
                "--reads" => args.reads = value()?.parse()?,
                "--writes" => args.writes = value()?.parse()?,
                "--dir" => args.dir = PathBuf::from(value()?),
                "--list" => {
                    println!("{}", names().join("\n"));
                    std::process::exit(0);
                }
                other => anyhow::bail!("argument inconnu : {other}"),
            }
        }
        if args.engine.is_empty() {
            anyhow::bail!("--engine requis (disponibles : {})", names().join(", "));
        }
        if args.users == 0 || args.books == 0 || args.reads == 0 {
            anyhow::bail!("--users, --books et --reads doivent être positifs");
        }
        Ok(args)
    }
}

fn names() -> Vec<&'static str> {
    let mut names = vec!["sqlite", "sqlite-blob"];
    #[cfg(feature = "redb")]
    names.push("redb");
    #[cfg(feature = "duckdb")]
    names.push("duckdb");
    names
}

fn open(engine: &str, dir: &Path) -> Result<Box<dyn Store>> {
    match engine {
        "sqlite" => Ok(Box::new(sqlite::Sqlite::open(dir.join("reading.db"), false)?)),
        "sqlite-blob" => Ok(Box::new(sqlite::Sqlite::open(dir.join("reading.db"), true)?)),
        #[cfg(feature = "redb")]
        "redb" => Ok(Box::new(redb_store::Redb::open(dir.join("reading.redb"))?)),
        #[cfg(feature = "duckdb")]
        "duckdb" => Ok(Box::new(duck::Duck::open(dir.join("reading.duckdb"))?)),
        other => anyhow::bail!("moteur inconnu : {other}"),
    }
}

// ── SQLite ──────────────────────────────────────────────────────────────────

mod sqlite {
    use super::*;
    use rusqlite::{Connection, params};

    /// `blob` : une ligne par utilisateur (JSON), au lieu d'une par livre.
    pub struct Sqlite {
        conn: Connection,
        blob: bool,
    }

    impl Sqlite {
        pub fn open(path: PathBuf, blob: bool) -> Result<Self> {
            let conn = Connection::open(&path)?;
            conn.busy_timeout(std::time::Duration::from_secs(5))?;
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 CREATE TABLE IF NOT EXISTS progress(
                     user_id TEXT NOT NULL,
                     book    TEXT NOT NULL,
                     page    INTEGER NOT NULL,
                     pages   INTEGER NOT NULL,
                     started_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL,
                     PRIMARY KEY (user_id, book)
                 ) WITHOUT ROWID;
                 CREATE INDEX IF NOT EXISTS progress_recent
                     ON progress(user_id, updated_at DESC);
                 CREATE TABLE IF NOT EXISTS progress_blob(
                     user_id TEXT PRIMARY KEY,
                     data    TEXT NOT NULL
                 ) WITHOUT ROWID;
                 CREATE TABLE IF NOT EXISTS sessions(
                     id         INTEGER PRIMARY KEY,
                     user_id    TEXT NOT NULL,
                     book       TEXT NOT NULL,
                     started_at INTEGER NOT NULL,
                     last_at    INTEGER NOT NULL,
                     end_page   INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS sessions_user
                     ON sessions(user_id, last_at DESC);",
            )?;
            Ok(Self { conn, blob })
        }
    }

    impl Store for Sqlite {
        fn progress(&mut self, row: &ProgressRow) -> Result<()> {
            if self.blob {
                // Le JSON de l'utilisateur est relu, modifié puis réécrit en
                // entier : une écriture par livre lu, sur un document qui grossit.
                let mut text: String = self
                    .conn
                    .query_row(
                        "SELECT data FROM progress_blob WHERE user_id = ?1",
                        params![row.user],
                        |row| row.get(0),
                    )
                    .unwrap_or_else(|_| "{}".to_string());
                let mut map: std::collections::BTreeMap<String, (i64, i64, i64, i64)> =
                    serde_json::from_str(&text)?;
                map.insert(
                    row.book.clone(),
                    (row.page, row.pages, row.started_at, row.updated_at),
                );
                text = serde_json::to_string(&map)?;
                self.conn.execute(
                    "INSERT INTO progress_blob(user_id, data) VALUES(?1, ?2)
                     ON CONFLICT(user_id) DO UPDATE SET data = excluded.data",
                    params![row.user, text],
                )?;
                return Ok(());
            }
            self.conn.execute(
                "INSERT INTO progress(user_id, book, page, pages, started_at, updated_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(user_id, book) DO UPDATE SET
                     page = excluded.page,
                     pages = excluded.pages,
                     updated_at = excluded.updated_at",
                params![
                    row.user,
                    row.book,
                    row.page,
                    row.pages,
                    row.started_at,
                    row.updated_at
                ],
            )?;
            Ok(())
        }

        fn open_session(&mut self, row: &SessionRow) -> Result<()> {
            self.conn.execute(
                "INSERT INTO sessions(id, user_id, book, started_at, last_at, end_page)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    row.id,
                    row.user,
                    row.book,
                    row.started_at,
                    row.last_at,
                    row.end_page
                ],
            )?;
            Ok(())
        }

        fn touch_session(&mut self, id: i64, at: i64, page: i64) -> Result<()> {
            self.conn.execute(
                "UPDATE sessions SET last_at = ?1, end_page = ?2 WHERE id = ?3",
                params![at, page, id],
            )?;
            Ok(())
        }

        fn recent(&mut self, user: &str, limit: usize) -> Result<Vec<RecentRow>> {
            if self.blob {
                let text: String = self
                    .conn
                    .query_row(
                        "SELECT data FROM progress_blob WHERE user_id = ?1",
                        params![user],
                        |row| row.get(0),
                    )
                    .unwrap_or_else(|_| "{}".to_string());
                let map: std::collections::BTreeMap<String, (i64, i64, i64, i64)> =
                    serde_json::from_str(&text)?;
                let mut rows: Vec<RecentRow> = map
                    .into_iter()
                    .map(|(book, (page, pages, _, at))| RecentRow {
                        book,
                        page,
                        pages,
                        at,
                    })
                    .collect();
                rows.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.book.cmp(&b.book)));
                rows.truncate(limit);
                return Ok(rows);
            }
            let mut stmt = self.conn.prepare(
                "SELECT book, page, pages, updated_at FROM progress
                 WHERE user_id = ?1
                 ORDER BY updated_at DESC, book ASC
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![user, limit as i64], |row| {
                    Ok(RecentRow {
                        book: row.get(0)?,
                        page: row.get(1)?,
                        pages: row.get(2)?,
                        at: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }

        fn totals(&mut self, user: &str) -> Result<Totals> {
            if self.blob {
                let text: String = self
                    .conn
                    .query_row(
                        "SELECT data FROM progress_blob WHERE user_id = ?1",
                        params![user],
                        |row| row.get(0),
                    )
                    .unwrap_or_else(|_| "{}".to_string());
                let map: std::collections::BTreeMap<String, (i64, i64, i64, i64)> =
                    serde_json::from_str(&text)?;
                let mut totals = Totals {
                    books: 0,
                    finished: 0,
                    pages: 0,
                };
                for (_, (page, pages, _, _)) in map {
                    totals.books += 1;
                    totals.pages += page + 1;
                    if page + 1 >= pages {
                        totals.finished += 1;
                    }
                }
                return Ok(totals);
            }
            let totals = self.conn.query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN page + 1 >= pages THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(page + 1), 0)
                 FROM progress WHERE user_id = ?1",
                params![user],
                |row| {
                    Ok(Totals {
                        books: row.get(0)?,
                        finished: row.get(1)?,
                        pages: row.get(2)?,
                    })
                },
            )?;
            Ok(totals)
        }

        fn sessions(&mut self, user: &str, limit: usize) -> Result<Vec<RecentRow>> {
            let mut stmt = self.conn.prepare(
                "SELECT book, end_page, last_at FROM sessions
                 WHERE user_id = ?1
                 ORDER BY last_at DESC, id DESC
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![user, limit as i64], |row| {
                    Ok(RecentRow {
                        book: row.get(0)?,
                        page: row.get(1)?,
                        pages: 0,
                        at: row.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }

        fn flush(&mut self) -> Result<()> {
            self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok(())
        }
    }
}

// ── redb ────────────────────────────────────────────────────────────────────

#[cfg(feature = "redb")]
mod redb_store {
    use super::*;
    use redb::{Database, ReadableTable, TableDefinition};

    const PROGRESS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("progress");
    const RECENT: TableDefinition<&[u8], &[u8]> = TableDefinition::new("recent");
    const SESSIONS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("sessions");
    const USER_SESSIONS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("user_sessions");

    pub struct Redb {
        db: Database,
    }

    fn progress_key(user: &str, book: &str) -> Vec<u8> {
        let mut key = Vec::with_capacity(user.len() + book.len() + 1);
        key.extend_from_slice(user.as_bytes());
        key.push(0);
        key.extend_from_slice(book.as_bytes());
        key
    }

    /// Clé du tri : utilisateur, date **inversée** (les plus récentes d'abord),
    /// puis livre (unicité).
    fn recent_key(user: &str, at: i64, book: &str) -> Vec<u8> {
        let mut key = Vec::with_capacity(user.len() + book.len() + 9);
        key.extend_from_slice(user.as_bytes());
        key.push(0);
        key.extend_from_slice(&(u64::MAX - at as u64).to_be_bytes());
        key.push(0);
        key.extend_from_slice(book.as_bytes());
        key
    }

    fn encode(row: &ProgressRow) -> Vec<u8> {
        let mut value = Vec::with_capacity(32 + row.book.len());
        value.extend_from_slice(&row.page.to_be_bytes());
        value.extend_from_slice(&row.pages.to_be_bytes());
        value.extend_from_slice(&row.started_at.to_be_bytes());
        value.extend_from_slice(&row.updated_at.to_be_bytes());
        value.extend_from_slice(row.book.as_bytes());
        value
    }

    fn decode(book_override: Option<&str>, value: &[u8]) -> RecentRow {
        let page = i64::from_be_bytes(value[0..8].try_into().unwrap());
        let pages = i64::from_be_bytes(value[8..16].try_into().unwrap());
        let at = i64::from_be_bytes(value[24..32].try_into().unwrap());
        let book = match book_override {
            Some(book) => book.to_string(),
            None => String::from_utf8_lossy(&value[32..]).to_string(),
        };
        RecentRow {
            book,
            page,
            pages,
            at,
        }
    }

    impl Redb {
        pub fn open(path: PathBuf) -> Result<Self> {
            let db = Database::create(path)?;
            let write = db.begin_write()?;
            {
                let _ = write.open_table(PROGRESS)?;
                let _ = write.open_table(RECENT)?;
                let _ = write.open_table(SESSIONS)?;
                let _ = write.open_table(USER_SESSIONS)?;
            }
            write.commit()?;
            Ok(Self { db })
        }
    }

    impl Store for Redb {
        fn progress(&mut self, row: &ProgressRow) -> Result<()> {
            let mut write = self.db.begin_write()?;
            // Comme `synchronous = NORMAL` en WAL : durable au niveau du
            // processus (crash applicatif), pas à chaque commit (coupure).
            write.set_durability(redb::Durability::Eventual);
            {
                let mut progress = write.open_table(PROGRESS)?;
                let mut recent = write.open_table(RECENT)?;
                if let Some(old) = progress.get(progress_key(&row.user, &row.book).as_slice())? {
                    let previous = decode(Some(&row.book), old.value());
                    drop(old);
                    recent.remove(recent_key(&row.user, previous.at, &row.book).as_slice())?;
                }
                progress.insert(progress_key(&row.user, &row.book).as_slice(), encode(row).as_slice())?;
                recent.insert(
                    recent_key(&row.user, row.updated_at, &row.book).as_slice(),
                    encode(row).as_slice(),
                )?;
            }
            write.commit()?;
            Ok(())
        }

        fn open_session(&mut self, row: &SessionRow) -> Result<()> {
            let mut write = self.db.begin_write()?;
            write.set_durability(redb::Durability::Eventual);
            {
                let mut sessions = write.open_table(SESSIONS)?;
                let mut index = write.open_table(USER_SESSIONS)?;
                let mut value = Vec::with_capacity(row.user.len() + row.book.len() + 25);
                value.extend_from_slice(row.user.as_bytes());
                value.push(0);
                value.extend_from_slice(row.book.as_bytes());
                value.push(0);
                value.extend_from_slice(&row.started_at.to_be_bytes());
                value.extend_from_slice(&row.last_at.to_be_bytes());
                value.extend_from_slice(&row.end_page.to_be_bytes());
                sessions.insert(row.id.to_be_bytes().as_slice(), value.as_slice())?;
                index.insert(
                    recent_key(&row.user, row.last_at, &row.book).as_slice(),
                    row.id.to_be_bytes().as_slice(),
                )?;
            }
            write.commit()?;
            Ok(())
        }

        fn touch_session(&mut self, id: i64, at: i64, page: i64) -> Result<()> {
            let mut write = self.db.begin_write()?;
            write.set_durability(redb::Durability::Eventual);
            {
                let mut sessions = write.open_table(SESSIONS)?;
                let mut index = write.open_table(USER_SESSIONS)?;
                let key = id.to_be_bytes();
                let Some(value) = sessions.get(key.as_slice())? else {
                    anyhow::bail!("session {id} inconnue");
                };
                let bytes = value.value().to_vec();
                drop(value);
                let Some(split) = bytes.iter().position(|byte| *byte == 0) else {
                    anyhow::bail!("session {id} illisible");
                };
                let user = String::from_utf8_lossy(&bytes[..split]).to_string();
                let rest = &bytes[split + 1..];
                let Some(second) = rest.iter().position(|byte| *byte == 0) else {
                    anyhow::bail!("session {id} illisible");
                };
                let book = String::from_utf8_lossy(&rest[..second]).to_string();
                let started_at = i64::from_be_bytes(rest[second + 1..second + 9].try_into().unwrap());
                let old_last = i64::from_be_bytes(rest[second + 9..second + 17].try_into().unwrap());
                index.remove(recent_key(&user, old_last, &book).as_slice())?;

                let mut updated = Vec::with_capacity(bytes.len());
                updated.extend_from_slice(user.as_bytes());
                updated.push(0);
                updated.extend_from_slice(book.as_bytes());
                updated.push(0);
                updated.extend_from_slice(&started_at.to_be_bytes());
                updated.extend_from_slice(&at.to_be_bytes());
                updated.extend_from_slice(&page.to_be_bytes());
                sessions.insert(key.as_slice(), updated.as_slice())?;
                index.insert(
                    recent_key(&user, at, &book).as_slice(),
                    key.as_slice(),
                )?;
            }
            write.commit()?;
            Ok(())
        }

        fn recent(&mut self, user: &str, limit: usize) -> Result<Vec<RecentRow>> {
            let mut prefix = user.as_bytes().to_vec();
            prefix.push(0);
            let read = self.db.begin_read()?;
            let recent = read.open_table(RECENT)?;
            let mut rows = Vec::with_capacity(limit);
            for entry in recent.range(prefix.as_slice()..)?.take(limit) {
                let (key, value) = entry?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                // Le livre suit la date inversée dans la clé : plus simple que
                // de le relire dans la valeur (qui le contient aussi).
                let book = value.value()[32..].to_vec();
                rows.push(decode(Some(&String::from_utf8_lossy(&book)), value.value()));
            }
            Ok(rows)
        }

        fn totals(&mut self, user: &str) -> Result<Totals> {
            let mut prefix = user.as_bytes().to_vec();
            prefix.push(0);
            let read = self.db.begin_read()?;
            let progress = read.open_table(PROGRESS)?;
            let mut totals = Totals {
                books: 0,
                finished: 0,
                pages: 0,
            };
            for entry in progress.range(prefix.as_slice()..)? {
                let (key, value) = entry?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                let row = decode(None, value.value());
                totals.books += 1;
                totals.pages += row.page + 1;
                if row.page + 1 >= row.pages {
                    totals.finished += 1;
                }
            }
            Ok(totals)
        }

        fn sessions(&mut self, user: &str, limit: usize) -> Result<Vec<RecentRow>> {
            let mut prefix = user.as_bytes().to_vec();
            prefix.push(0);
            let read = self.db.begin_read()?;
            let index = read.open_table(USER_SESSIONS)?;
            let sessions = read.open_table(SESSIONS)?;
            let mut rows = Vec::with_capacity(limit);
            for entry in index.range(prefix.as_slice()..)?.take(limit) {
                let (key, value) = entry?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                let id = value.value();
                let Some(stored) = sessions.get(id)? else {
                    continue;
                };
                let bytes = stored.value();
                let Some(split) = bytes.iter().position(|byte| *byte == 0) else {
                    continue;
                };
                let rest = &bytes[split + 1..];
                let Some(second) = rest.iter().position(|byte| *byte == 0) else {
                    continue;
                };
                let book = String::from_utf8_lossy(&rest[..second]).to_string();
                if rest.len() < second + 25 {
                    eprintln!("session illisible : {:?}", String::from_utf8_lossy(bytes));
                    continue;
                }
                let last_at =
                    i64::from_be_bytes(rest[second + 9..second + 17].try_into().unwrap());
                let end_page = i64::from_be_bytes(rest[second + 17..second + 25].try_into().unwrap());
                rows.push(RecentRow {
                    book,
                    page: end_page,
                    pages: 0,
                    at: last_at,
                });
            }
            Ok(rows)
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }
}

// ── DuckDB ──────────────────────────────────────────────────────────────────

#[cfg(feature = "duckdb")]
mod duck {
    use super::*;
    use duckdb::{Connection, params};

    pub struct Duck {
        conn: Connection,
    }

    impl Duck {
        pub fn open(path: PathBuf) -> Result<Self> {
            let conn = Connection::open(&path)?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS progress(
                     user_id VARCHAR NOT NULL,
                     book    VARCHAR NOT NULL,
                     page    BIGINT  NOT NULL,
                     pages   BIGINT  NOT NULL,
                     started_at BIGINT NOT NULL,
                     updated_at BIGINT NOT NULL,
                     PRIMARY KEY (user_id, book)
                 );
                 CREATE INDEX IF NOT EXISTS progress_recent
                     ON progress(user_id, updated_at);
                 CREATE TABLE IF NOT EXISTS sessions(
                     id         BIGINT PRIMARY KEY,
                     user_id    VARCHAR NOT NULL,
                     book       VARCHAR NOT NULL,
                     started_at BIGINT NOT NULL,
                     last_at    BIGINT NOT NULL,
                     end_page   BIGINT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS sessions_user
                     ON sessions(user_id, last_at);",
            )?;
            Ok(Self { conn })
        }
    }

    impl Store for Duck {
        fn progress(&mut self, row: &ProgressRow) -> Result<()> {
            self.conn.execute(
                "INSERT OR REPLACE INTO progress
                     (user_id, book, page, pages, started_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    row.user,
                    row.book,
                    row.page,
                    row.pages,
                    row.started_at,
                    row.updated_at
                ],
            )?;
            Ok(())
        }

        fn open_session(&mut self, row: &SessionRow) -> Result<()> {
            self.conn.execute(
                "INSERT OR REPLACE INTO sessions
                     (id, user_id, book, started_at, last_at, end_page)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    row.id,
                    row.user,
                    row.book,
                    row.started_at,
                    row.last_at,
                    row.end_page
                ],
            )?;
            Ok(())
        }

        fn touch_session(&mut self, id: i64, at: i64, page: i64) -> Result<()> {
            self.conn.execute(
                "UPDATE sessions SET last_at = ?1, end_page = ?2 WHERE id = ?3",
                params![at, page, id],
            )?;
            Ok(())
        }

        fn recent(&mut self, user: &str, limit: usize) -> Result<Vec<RecentRow>> {
            let mut stmt = self.conn.prepare(
                "SELECT book, page, pages, updated_at FROM progress
                 WHERE user_id = ?1
                 ORDER BY updated_at DESC, book ASC
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![user, limit as i64], |row| {
                    Ok(RecentRow {
                        book: row.get(0)?,
                        page: row.get(1)?,
                        pages: row.get(2)?,
                        at: row.get(3)?,
                    })
                })?
                .collect::<duckdb::Result<Vec<_>>>()?;
            Ok(rows)
        }

        fn totals(&mut self, user: &str) -> Result<Totals> {
            let totals = self.conn.query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN page + 1 >= pages THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(page + 1), 0)
                 FROM progress WHERE user_id = ?1",
                params![user],
                |row| {
                    Ok(Totals {
                        books: row.get(0)?,
                        finished: row.get(1)?,
                        pages: row.get(2)?,
                    })
                },
            )?;
            Ok(totals)
        }

        fn sessions(&mut self, user: &str, limit: usize) -> Result<Vec<RecentRow>> {
            let mut stmt = self.conn.prepare(
                "SELECT book, end_page, last_at FROM sessions
                 WHERE user_id = ?1
                 ORDER BY last_at DESC, id DESC
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![user, limit as i64], |row| {
                    Ok(RecentRow {
                        book: row.get(0)?,
                        page: row.get(1)?,
                        pages: 0,
                        at: row.get(2)?,
                    })
                })?
                .collect::<duckdb::Result<Vec<_>>>()?;
            Ok(rows)
        }

        fn flush(&mut self) -> Result<()> {
            self.conn.execute_batch("CHECKPOINT;")?;
            Ok(())
        }
    }
}

// ── Mesures ─────────────────────────────────────────────────────────────────

/// Médiane, en microsecondes, du temps d'une requête (5 passes).
fn median<F>(mut run: F) -> f64
where
    F: FnMut() -> usize,
{
    let mut times = Vec::with_capacity(5);
    for _ in 0..5 {
        let at = Instant::now();
        let _ = run();
        times.push(at.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index]
}

/// Taille de tous les fichiers d'un dossier, journal compris.
fn disk_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                } else if meta.is_dir() {
                    total += disk_bytes(&entry.path());
                }
            }
        }
    }
    total
}
