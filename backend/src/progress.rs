//! Historique de lecture : où en est chaque utilisateur dans chaque livre.
//!
//! Deux tables dans `reading.db` (SQLite, WAL, à côté de `accounts.db`) :
//!
//! - `reading_progress` : une ligne par (utilisateur, livre) — la position
//!   courante, écrasée à chaque changement de page ou de chapitre ;
//! - `reading_sessions` : une ligne par **session** de lecture, poursuivie tant
//!   que la même paire écrit à moins de [`IDLE`] d'intervalle (le même seuil que
//!   [`crate::readings`], qui décide côté serveur quand une lecture s'arrête).
//!
//! Le choix de SQLite et de ce schéma est mesuré dans
//! `bench/reports/reading-store.md` : 37 µs par écriture, 60 µs pour la liste
//! « derniers lus », 1,7 Mo pour 8 000 livres lus.
//!
//! Aucune donnée n'est écrite sans identité : sans compte (authentification
//! désactivée), rien n'est enregistré — il n'y aurait personne à qui rattacher
//! une position.

use rusqlite::{Connection, params};
use serde::Serialize;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// Silence après lequel une nouvelle position ouvre une nouvelle session.
///
/// Le même seuil que `readings::IDLE` : une lecture « en cours » pour le
/// serveur (cache de fichiers, page Performances) et une session d'historique
/// désignent la même chose.
pub const IDLE_SECS: i64 = crate::readings::IDLE.as_secs() as i64;

/// Longueur maximale d'un chemin ou d'un titre gardé en base.
const MAX_TEXT: usize = 512;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// Ce qu'un lecteur envoie : sa position dans un livre.
#[derive(Debug, Clone)]
pub struct Entry {
    pub book: String,
    pub title: String,
    pub format: String,
    /// Index 0-based de la page (BD, PDF) ou du chapitre (EPUB).
    pub page: i64,
    /// Nombre total de pages/chapitres connu du lecteur (`0` = inconnu).
    pub pages: i64,
}

/// La position d'un utilisateur dans un livre.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BookProgress {
    pub book: String,
    pub title: String,
    pub format: String,
    pub page: i64,
    pub pages: i64,
    pub started_at: i64,
    pub updated_at: i64,
}

/// Une session de lecture.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: i64,
    pub book: String,
    pub title: String,
    pub started_at: i64,
    pub last_at: i64,
    pub end_page: i64,
    pub pages: i64,
}

/// Les chiffres d'un utilisateur, pour les cartes de la page d'administration.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// Livres commencés.
    pub books: i64,
    /// Livres dont la dernière page connue est la dernière.
    pub finished: i64,
    /// Pages (ou chapitres) cumulés : une approximation de l'effort de lecture.
    pub pages: i64,
    /// Dernière activité, `0` si l'utilisateur n'a jamais lu.
    pub last_at: i64,
}

/// Ce que la page d'administration affiche.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub summary: Summary,
    pub recent: Vec<BookProgress>,
    pub sessions: Vec<Session>,
}

pub struct Progress {
    conn: Mutex<Connection>,
}

impl Progress {
    /// Ouvre (crée au besoin) la base dans `path`.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS reading_progress(
                 user_id    TEXT NOT NULL,
                 book       TEXT NOT NULL,
                 title      TEXT NOT NULL,
                 format     TEXT NOT NULL,
                 page       INTEGER NOT NULL,
                 pages      INTEGER NOT NULL,
                 started_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 PRIMARY KEY (user_id, book)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS reading_recent
                 ON reading_progress(user_id, updated_at DESC);
             CREATE TABLE IF NOT EXISTS reading_sessions(
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 user_id    TEXT NOT NULL,
                 book       TEXT NOT NULL,
                 title      TEXT NOT NULL,
                 started_at INTEGER NOT NULL,
                 last_at    INTEGER NOT NULL,
                 end_page   INTEGER NOT NULL,
                 pages      INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS reading_sessions_user
                 ON reading_sessions(user_id, last_at DESC);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Enregistre une position : met à jour la ligne du livre et poursuit — ou
    /// ouvre — la session de lecture.
    ///
    /// Tout tient dans une transaction : la position et la session ne peuvent
    /// pas se désaccorder.
    pub fn save(&self, user: &str, entry: &Entry) -> rusqlite::Result<()> {
        let now = now();
        let page = entry.page.max(0);
        let pages = entry.pages.max(0);
        // Une position ne dépasse jamais la dernière page connue.
        let page = if pages > 0 { page.min(pages - 1) } else { page };
        let book = clip(&entry.book);
        let title = clip(&entry.title);
        let format = clip(&entry.format);

        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO reading_progress
                 (user_id, book, title, format, page, pages, started_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(user_id, book) DO UPDATE SET
                 title = excluded.title,
                 format = excluded.format,
                 page = excluded.page,
                 pages = MAX(excluded.pages, reading_progress.pages),
                 updated_at = excluded.updated_at",
            params![user, book, title, format, page, pages, now],
        )?;

        // La session en cours est celle qui a écrit il y a moins de `IDLE_SECS`.
        let resumed = tx.execute(
            "UPDATE reading_sessions
                SET last_at = ?1, end_page = ?2, pages = MAX(pages, ?3)
              WHERE id = (
                    SELECT id FROM reading_sessions
                     WHERE user_id = ?4 AND book = ?5 AND last_at >= ?6
                     ORDER BY last_at DESC LIMIT 1
              )",
            params![now, page, pages, user, book, now - IDLE_SECS],
        )?;
        if resumed == 0 {
            tx.execute(
                "INSERT INTO reading_sessions
                     (user_id, book, title, started_at, last_at, end_page, pages)
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6)",
                params![user, book, title, now, page, pages],
            )?;
        }
        tx.commit()
    }

    /// La position d'un utilisateur dans un livre, si elle existe.
    pub fn position(&self, user: &str, book: &str) -> rusqlite::Result<Option<BookProgress>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT book, title, format, page, pages, started_at, updated_at
               FROM reading_progress
              WHERE user_id = ?1 AND book = ?2",
        )?;
        let mut rows = stmt.query(params![user, book])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(BookProgress {
            book: row.get(0)?,
            title: row.get(1)?,
            format: row.get(2)?,
            page: row.get(3)?,
            pages: row.get(4)?,
            started_at: row.get(5)?,
            updated_at: row.get(6)?,
        }))
    }

    /// Livres les plus récemment lus par `user`, du plus récent au plus ancien.
    pub fn recent(&self, user: &str, limit: usize) -> rusqlite::Result<Vec<BookProgress>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT book, title, format, page, pages, started_at, updated_at
               FROM reading_progress
              WHERE user_id = ?1
              ORDER BY updated_at DESC, book ASC
              LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![user, limit as i64], |row| {
                Ok(BookProgress {
                    book: row.get(0)?,
                    title: row.get(1)?,
                    format: row.get(2)?,
                    page: row.get(3)?,
                    pages: row.get(4)?,
                    started_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Sessions récentes de `user`, de la plus récente à la plus ancienne.
    pub fn sessions(&self, user: &str, limit: usize) -> rusqlite::Result<Vec<Session>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, book, title, started_at, last_at, end_page, pages
               FROM reading_sessions
              WHERE user_id = ?1
              ORDER BY last_at DESC, id DESC
              LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![user, limit as i64], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    book: row.get(1)?,
                    title: row.get(2)?,
                    started_at: row.get(3)?,
                    last_at: row.get(4)?,
                    end_page: row.get(5)?,
                    pages: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Les chiffres d'un utilisateur (une seule passe sur la table).
    pub fn summary(&self, user: &str) -> rusqlite::Result<Summary> {
        let conn = self.lock();
        conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN pages > 0 AND page + 1 >= pages THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(page + 1), 0),
                    COALESCE(MAX(updated_at), 0)
               FROM reading_progress
              WHERE user_id = ?1",
            params![user],
            |row| {
                Ok(Summary {
                    books: row.get(0)?,
                    finished: row.get(1)?,
                    pages: row.get(2)?,
                    last_at: row.get(3)?,
                })
            },
        )
    }

    /// Tout ce qu'affiche la page d'administration, en une fois.
    pub fn history(&self, user: &str, limit: usize) -> rusqlite::Result<History> {
        Ok(History {
            summary: self.summary(user)?,
            recent: self.recent(user, limit)?,
            sessions: self.sessions(user, limit)?,
        })
    }
}

/// Tronque un texte envoyé par le client (nom de fichier, titre) et remplace
/// les caractères de contrôle : la base ne doit pas dépendre de sa bonne foi.
fn clip(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}

impl Progress {
    /// Base en mémoire, pour les tests et les essais.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE reading_progress(
                 user_id TEXT NOT NULL, book TEXT NOT NULL, title TEXT NOT NULL,
                 format TEXT NOT NULL, page INTEGER NOT NULL, pages INTEGER NOT NULL,
                 started_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                 PRIMARY KEY (user_id, book)) WITHOUT ROWID;
             CREATE INDEX reading_recent ON reading_progress(user_id, updated_at DESC);
             CREATE TABLE reading_sessions(
                 id INTEGER PRIMARY KEY AUTOINCREMENT, user_id TEXT NOT NULL,
                 book TEXT NOT NULL, title TEXT NOT NULL, started_at INTEGER NOT NULL,
                 last_at INTEGER NOT NULL, end_page INTEGER NOT NULL,
                 pages INTEGER NOT NULL);
             CREATE INDEX reading_sessions_user ON reading_sessions(user_id, last_at DESC);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(book: &str, page: i64, pages: i64) -> Entry {
        Entry {
            book: book.to_string(),
            title: book.trim_end_matches(".cbz").to_string(),
            format: "cbz".to_string(),
            page,
            pages,
        }
    }

    #[test]
    fn positions_are_upserted_and_sessions_are_pursued() {
        let progress = Progress::open_in_memory().unwrap();
        progress
            .save("u1", &entry("mangas/T1.cbz", 3, 100))
            .unwrap();
        progress
            .save("u1", &entry("mangas/T1.cbz", 5, 100))
            .unwrap();

        let recent = progress.recent("u1", 10).unwrap();
        assert_eq!(recent.len(), 1, "une seule ligne par livre");
        assert_eq!(recent[0].page, 5);
        assert_eq!(recent[0].pages, 100);

        // Deux écritures rapprochées : une seule session.
        let sessions = progress.sessions("u1", 10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].end_page, 5);
        assert_eq!(sessions[0].started_at, sessions[0].last_at);
    }

    #[test]
    fn a_silence_opens_a_new_session() {
        let progress = Progress::open_in_memory().unwrap();
        progress.save("u1", &entry("mangas/T1.cbz", 1, 10)).unwrap();
        // On vieillit la session au-delà du silence admis.
        progress
            .lock()
            .execute(
                "UPDATE reading_sessions SET last_at = last_at - ?1",
                params![IDLE_SECS + 1],
            )
            .unwrap();
        progress.save("u1", &entry("mangas/T1.cbz", 2, 10)).unwrap();

        let sessions = progress.sessions("u1", 10).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions[0].last_at > sessions[1].last_at);
    }

    #[test]
    fn summary_counts_started_finished_and_pages() {
        let progress = Progress::open_in_memory().unwrap();
        progress.save("u1", &entry("a.cbz", 9, 10)).unwrap();
        progress.save("u1", &entry("b.cbz", 0, 20)).unwrap();
        progress.save("u2", &entry("c.cbz", 4, 10)).unwrap();

        let summary = progress.summary("u1").unwrap();
        assert_eq!(summary.books, 2);
        assert_eq!(summary.finished, 1);
        assert_eq!(summary.pages, 11);
        assert!(summary.last_at > 0);

        // Chaque utilisateur ne voit que ses lignes.
        assert_eq!(progress.recent("u2", 10).unwrap().len(), 1);
    }

    #[test]
    fn a_position_beyond_the_last_page_is_clamped() {
        let progress = Progress::open_in_memory().unwrap();
        progress.save("u1", &entry("a.cbz", 5_000, 10)).unwrap();
        assert_eq!(progress.recent("u1", 1).unwrap()[0].page, 9);

        // Un total inconnu (0) laisse la position telle quelle.
        progress.save("u1", &entry("b.epub", 12, 0)).unwrap();
        let rows = progress.recent("u1", 10).unwrap();
        let epub = rows.iter().find(|row| row.book == "b.epub").unwrap();
        assert_eq!(epub.page, 12);
        assert_eq!(epub.pages, 0);
    }

    #[test]
    fn control_characters_are_dropped() {
        let progress = Progress::open_in_memory().unwrap();
        let mut row = entry("a.cbz", 0, 10);
        row.title = "Titre\u{0} sale".to_string();
        progress.save("u1", &row).unwrap();
        assert_eq!(progress.recent("u1", 1).unwrap()[0].title, "Titre sale");
    }
}
