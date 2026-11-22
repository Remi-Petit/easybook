//! Comptes : utilisateurs, identités OIDC liées, sessions (SQLite).
//!
//! Aucun identifiant séquentiel : tout est identifié par un UUID v4. Les mots de
//! passe sont hachés en Argon2id ; les jetons de session et d'invitation ne sont
//! jamais stockés en clair (seule leur empreinte SHA-256 l'est), si bien qu'une
//! copie de la base ne permet de rejouer aucune session.
//!
//! Le module est synchrone : l'API l'appelle depuis le pool bloquant (le hachage
//! Argon2 coûte plusieurs dizaines de millisecondes).

use argon2::Argon2;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MIN_PASSWORD: usize = 8;
/// Au-delà, Argon2 ne protège plus de rien et devient un moyen de saturer le CPU.
pub const MAX_PASSWORD: usize = 256;

/// Version du schéma (`PRAGMA user_version`).
const SCHEMA_VERSION: i64 = 6;

const SCHEMA: &str = "
CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name  TEXT,
    email         TEXT COLLATE NOCASE,
    password_hash TEXT,
    role          TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    last_login_at INTEGER,
    must_change_password INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX users_email ON users (email);

CREATE TABLE identities (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    issuer     TEXT NOT NULL,
    subject    TEXT NOT NULL,
    email      TEXT,
    created_at INTEGER NOT NULL,
    UNIQUE (issuer, subject)
);
CREATE INDEX identities_user ON identities (user_id);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    ip         TEXT,
    user_agent TEXT,
    method     TEXT,
    last_seen_at INTEGER
);
CREATE INDEX sessions_user ON sessions (user_id);

CREATE TABLE login_events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    at         INTEGER NOT NULL,
    success    INTEGER NOT NULL,
    method     TEXT NOT NULL,
    ip         TEXT,
    user_agent TEXT
);
CREATE INDEX login_events_user ON login_events (user_id, at);

CREATE TABLE link_tokens (
    token_hash TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);

-- Passkeys (WebAuthn) : la clé publique et son état, tels que la bibliothèque
-- WebAuthn les sérialise. `credential_id` n'est qu'un index de recherche ;
-- `passkey` fait foi (il porte aussi le compteur anti-rejeu).
CREATE TABLE passkeys (
    id            TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    credential_id BLOB NOT NULL UNIQUE,
    passkey       TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    last_used_at  INTEGER
);
CREATE INDEX passkeys_user ON passkeys (user_id);
";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }

    fn parse(text: &str) -> Self {
        if text == "admin" {
            Role::Admin
        } else {
            Role::User
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub role: Role,
    pub disabled: bool,
    /// `false` pour un compte qui ne se connecte que par OIDC.
    pub has_password: bool,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
    /// Mot de passe temporaire : l'interface propose de le changer à la connexion.
    pub must_change_password: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Identity {
    pub id: String,
    pub user_id: String,
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub created_at: i64,
}

/// Passkey d'un compte, telle que l'interface l'affiche (sans la clé publique).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PasskeyInfo {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// Passkey telle que la base la garde : l'état WebAuthn sérialisé reste opaque
/// pour ce module (`passkeys.rs` le relit).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredPasskey {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub credential_id: Vec<u8>,
    pub passkey: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// D'où vient une connexion : adresse, navigateur, et par quel moyen.
#[derive(Debug, Clone, Default)]
pub struct SessionMeta {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    /// `password` ou `sso`.
    pub method: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionInfo {
    /// Début du hash du jeton : identifie la session sans permettre de l'utiliser.
    pub id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub method: Option<String>,
    /// Dernière requête vue (relevée au plus une fois par minute).
    pub last_seen_at: i64,
    /// Un onglet de cette session est ouvert (renseigné par l'API, pas par la base).
    #[serde(default)]
    pub online: bool,
    /// C'est la session de celui qui regarde (renseigné par l'API).
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoginEvent {
    pub at: i64,
    pub success: bool,
    pub method: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Connexions gardées par compte : au-delà, les plus anciennes partent.
const LOGIN_HISTORY_KEPT: i64 = 200;

fn insert_login_event(
    conn: &Connection,
    user_id: &str,
    success: bool,
    meta: &SessionMeta,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO login_events (user_id, at, success, method, ip, user_agent)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            user_id,
            now(),
            success,
            meta.method,
            meta.ip,
            meta.user_agent
        ],
    )?;
    conn.execute(
        "DELETE FROM login_events WHERE user_id = ?1 AND id NOT IN
         (SELECT id FROM login_events WHERE user_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2)",
        params![user_id, LOGIN_HISTORY_KEPT],
    )?;
    Ok(())
}

#[derive(Debug, Default, Clone)]
pub struct NewUser {
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
    pub role: Option<Role>,
    /// Le mot de passe donné est temporaire.
    pub must_change_password: bool,
}

/// Modification d'un compte : `None` = inchangé, chaîne vide = effacer.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct UserPatch {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub role: Option<Role>,
    pub disabled: Option<bool>,
}

/// Erreur métier ; [`AccountError::code`] est ce que l'interface traduit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountError {
    InvalidUsername,
    InvalidEmail,
    WeakPassword,
    UsernameTaken,
    EmailTaken,
    /// L'adresse est portée par un compte qui n'est pas administrateur.
    EmailOwnedByUser,
    IdentityTaken,
    NotFound,
    /// Le dernier administrateur actif ne peut être ni retiré, ni rétrogradé.
    LastAdmin,
    /// Retirer la dernière façon de se connecter verrouillerait le compte.
    LastCredential,
    /// Cette passkey est déjà enregistrée (sur ce compte ou un autre).
    PasskeyTaken,
    Internal(String),
}

impl AccountError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUsername => "invalid_username",
            Self::InvalidEmail => "invalid_email",
            Self::WeakPassword => "weak_password",
            Self::UsernameTaken => "username_taken",
            Self::EmailTaken => "email_taken",
            Self::EmailOwnedByUser => "email_owned_by_user",
            Self::IdentityTaken => "identity_taken",
            Self::NotFound => "not_found",
            Self::LastAdmin => "last_admin",
            Self::LastCredential => "last_credential",
            Self::PasskeyTaken => "passkey_taken",
            Self::Internal(_) => "internal",
        }
    }
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(detail) => write!(f, "erreur interne : {detail}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for AccountError {}

impl From<rusqlite::Error> for AccountError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

type Result<T> = std::result::Result<T, AccountError>;

/// Secondes Unix.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// `n` octets aléatoires du système, en base64 URL.
pub fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

fn digest(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Coût cumulé du hachage Argon2 : la seule partie vraiment chère d'une
/// connexion (mémoire-hard par construction).
static HASH_CALLS: AtomicU64 = AtomicU64::new(0);
static HASH_USEC: AtomicU64 = AtomicU64::new(0);

fn note_hash(started: std::time::Instant) {
    HASH_CALLS.fetch_add(1, AtomicOrdering::Relaxed);
    HASH_USEC.fetch_add(
        started.elapsed().as_micros() as u64,
        AtomicOrdering::Relaxed,
    );
}

/// (hachages effectués, microsecondes passées à hacher) depuis le démarrage.
pub fn hash_stats() -> (u64, u64) {
    (
        HASH_CALLS.load(AtomicOrdering::Relaxed),
        HASH_USEC.load(AtomicOrdering::Relaxed),
    )
}

fn hash_password(password: &str) -> Result<String> {
    let started = std::time::Instant::now();
    let salt = SaltString::generate(&mut OsRng);
    let hashed = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AccountError::Internal(e.to_string()));
    note_hash(started);
    hashed
}

fn verify_password(hash: &str, password: &str) -> bool {
    let started = std::time::Instant::now();
    let verified = PasswordHash::new(hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    });
    note_hash(started);
    verified
}

/// Hache d'un mot de passe inconnu : un identifiant absent coûte le même temps
/// qu'un mauvais mot de passe, ce qui ne révèle pas quels comptes existent.
fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password(&random_token(16)).unwrap_or_default())
}

fn check_password(password: &str) -> Result<()> {
    let length = password.chars().count();
    // Huit espaces ne sont pas un mot de passe.
    if (MIN_PASSWORD..=MAX_PASSWORD).contains(&length) && !password.trim().is_empty() {
        Ok(())
    } else {
        Err(AccountError::WeakPassword)
    }
}

pub fn clean_username(raw: &str) -> Result<String> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(AccountError::InvalidUsername);
    }
    Ok(name.to_string())
}

/// Adresse nettoyée ; chaîne vide : aucune.
fn clean_email(raw: &str) -> Result<Option<String>> {
    let email = raw.trim();
    if email.is_empty() {
        return Ok(None);
    }
    if email.len() > 254 || !email.contains('@') || email.chars().any(char::is_whitespace) {
        return Err(AccountError::InvalidEmail);
    }
    Ok(Some(email.to_string()))
}

/// Adresse obligatoire (la connexion se fait par e-mail).
fn require_email(raw: &str) -> Result<String> {
    clean_email(raw)?.ok_or(AccountError::InvalidEmail)
}

fn constraint_error(e: rusqlite::Error) -> AccountError {
    if !is_unique_violation(&e) {
        return e.into();
    }
    if e.to_string().contains("users.email") {
        AccountError::EmailTaken
    } else {
        AccountError::UsernameTaken
    }
}

fn clean_text(raw: &str) -> Option<String> {
    let text = raw.trim();
    (!text.is_empty()).then(|| text.chars().take(128).collect())
}

const USER_COLUMNS: &str = "id, username, display_name, email, password_hash IS NOT NULL, role, \
     disabled, created_at, last_login_at, must_change_password";

fn user_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        email: row.get(3)?,
        has_password: row.get(4)?,
        role: Role::parse(&row.get::<_, String>(5)?),
        disabled: row.get(6)?,
        created_at: row.get(7)?,
        last_login_at: row.get(8)?,
        must_change_password: row.get(9)?,
    })
}

fn identity_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<Identity> {
    Ok(Identity {
        id: row.get(0)?,
        user_id: row.get(1)?,
        issuer: row.get(2)?,
        subject: row.get(3)?,
        email: row.get(4)?,
        created_at: row.get(5)?,
    })
}

const PASSKEY_COLUMNS: &str = "id, user_id, name, credential_id, passkey, created_at, last_used_at";

fn passkey_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredPasskey> {
    Ok(StoredPasskey {
        id: row.get(0)?,
        user_id: row.get(1)?,
        name: row.get(2)?,
        credential_id: row.get(3)?,
        passkey: row.get(4)?,
        created_at: row.get(5)?,
        last_used_at: row.get(6)?,
    })
}

fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

fn fetch_user(conn: &Connection, id: &str) -> rusqlite::Result<Option<User>> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"),
        [id],
        user_from,
    )
    .optional()
}

/// Administrateurs actifs, `except` mis à part.
fn other_active_admins(conn: &Connection, except: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM users WHERE role = 'admin' AND disabled = 0 AND id != ?1",
        [except],
        |row| row.get(0),
    )
}

/// Le compte créé au premier démarrage.
#[derive(Debug, Clone)]
pub struct Bootstrap {
    pub user: User,
    /// Le mot de passe, **seulement** s'il a été tiré au hasard : il n'existe
    /// alors nulle part ailleurs qu'à l'écran de démarrage.
    pub generated_password: Option<String>,
}

/// Ce qu'a fait [`Accounts::reset_admin`].
#[derive(Debug, Clone, PartialEq)]
pub enum ResetOutcome {
    Created(User),
    Updated(User),
}

pub struct Accounts {
    conn: Mutex<Connection>,
}

impl Accounts {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| AccountError::Internal(format!("{} : {e}", dir.display())))?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        // WAL : les lectures (une par requête authentifiée) n'attendent pas une
        // écriture. Sans effet, et sans erreur, sur une base en mémoire.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        let mut version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 0 {
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            version = SCHEMA_VERSION;
        }
        if version == 1 {
            // v2 : la connexion se fait par e-mail, unique et obligatoire.
            conn.execute_batch(
                "UPDATE users SET email = lower(username) || '@easybook.local' WHERE email IS NULL;
                 DROP INDEX IF EXISTS users_email;
                 CREATE UNIQUE INDEX users_email ON users (email);",
            )?;
            conn.pragma_update(None, "user_version", 2)?;
            version = 2;
        }
        if version == 2 {
            // v3 : mot de passe temporaire, à changer à la première connexion.
            conn.execute_batch(
                "ALTER TABLE users ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0;",
            )?;
            conn.pragma_update(None, "user_version", 3)?;
            version = 3;
        }
        if version == 3 {
            // v4 : d'où vient chaque session, et l'historique des connexions.
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN ip TEXT;
                 ALTER TABLE sessions ADD COLUMN user_agent TEXT;
                 ALTER TABLE sessions ADD COLUMN method TEXT;
                 CREATE TABLE login_events (
                     id         INTEGER PRIMARY KEY AUTOINCREMENT,
                     user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
                     at         INTEGER NOT NULL,
                     success    INTEGER NOT NULL,
                     method     TEXT NOT NULL,
                     ip         TEXT,
                     user_agent TEXT
                 );
                 CREATE INDEX login_events_user ON login_events (user_id, at);",
            )?;
            conn.pragma_update(None, "user_version", 4)?;
            version = 4;
        }
        if version == 4 {
            // v5 : dernier signe de vie de chaque session.
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN last_seen_at INTEGER;
                 UPDATE sessions SET last_seen_at = created_at;",
            )?;
            conn.pragma_update(None, "user_version", 5)?;
            version = 5;
        }
        if version == 5 {
            // v6 : passkeys (WebAuthn).
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS passkeys (
                     id            TEXT PRIMARY KEY,
                     user_id       TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
                     name          TEXT NOT NULL,
                     credential_id BLOB NOT NULL UNIQUE,
                     passkey       TEXT NOT NULL,
                     created_at    INTEGER NOT NULL,
                     last_used_at  INTEGER
                 );
                 CREATE INDEX IF NOT EXISTS passkeys_user ON passkeys (user_id);",
            )?;
            conn.pragma_update(None, "user_version", 6)?;
            version = 6;
        }
        if version != SCHEMA_VERSION {
            return Err(AccountError::Internal(format!(
                "schéma des comptes en version {version}, {SCHEMA_VERSION} attendue"
            )));
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ── Utilisateurs ─────────────────────────────────────────────────────

    pub fn create_user(&self, new: NewUser) -> Result<User> {
        let email = require_email(new.email.as_deref().unwrap_or_default())?;
        // Sans identifiant, il est tiré de l'adresse : il ne sert qu'à l'affichage.
        let username = if new.username.trim().is_empty() {
            self.available_username(email.split('@').next().unwrap_or_default())?
        } else {
            clean_username(&new.username)?
        };
        let hash = match &new.password {
            Some(password) => {
                check_password(password)?;
                Some(hash_password(password)?)
            }
            None => None,
        };
        let id = uuid::Uuid::new_v4().to_string();
        let display_name = new.display_name.as_deref().and_then(clean_text);
        let role = new.role.unwrap_or(Role::User);

        let conn = self.conn();
        conn.execute(
            "INSERT INTO users (id, username, display_name, email, password_hash, role, created_at, must_change_password)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                username,
                display_name,
                email,
                hash,
                role.as_str(),
                now(),
                new.must_change_password && hash.is_some()
            ],
        )
        .map_err(constraint_error)?;
        fetch_user(&conn, &id)?.ok_or(AccountError::NotFound)
    }

    pub fn user(&self, id: &str) -> Result<Option<User>> {
        Ok(fetch_user(&self.conn(), id)?)
    }

    pub fn user_by_username(&self, username: &str) -> Result<Option<User>> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE username = ?1"),
                [username.trim()],
                user_from,
            )
            .optional()?)
    }

    /// Le compte de cette adresse (unique, sans tenir compte de la casse).
    pub fn user_by_email(&self, email: &str) -> Result<Option<User>> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE email = ?1"),
                [email.trim()],
                user_from,
            )
            .optional()?)
    }

    pub fn list_users(&self) -> Result<Vec<User>> {
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            "SELECT {USER_COLUMNS} FROM users ORDER BY username COLLATE NOCASE"
        ))?;
        Ok(statement
            .query_map([], user_from)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn count_users(&self) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?)
    }

    pub fn update_user(&self, id: &str, patch: &UserPatch) -> Result<User> {
        let email = match &patch.email {
            Some(email) => Some(require_email(email)?),
            None => None,
        };
        let conn = self.conn();
        let current = fetch_user(&conn, id)?.ok_or(AccountError::NotFound)?;

        let role = patch.role.unwrap_or(current.role);
        let disabled = patch.disabled.unwrap_or(current.disabled);
        let was_active_admin = current.role == Role::Admin && !current.disabled;
        let is_active_admin = role == Role::Admin && !disabled;
        if was_active_admin && !is_active_admin && other_active_admins(&conn, id)? == 0 {
            return Err(AccountError::LastAdmin);
        }

        let display_name = match &patch.display_name {
            Some(name) => clean_text(name),
            None => current.display_name.clone(),
        };
        let email = email.or(current.email.clone());
        conn.execute(
            "UPDATE users SET display_name = ?2, email = ?3, role = ?4, disabled = ?5 WHERE id = ?1",
            params![id, display_name, email, role.as_str(), disabled],
        )
        .map_err(constraint_error)?;
        if disabled {
            conn.execute("DELETE FROM sessions WHERE user_id = ?1", [id])?;
        }
        fetch_user(&conn, id)?.ok_or(AccountError::NotFound)
    }

    /// Change le mot de passe et **révoque toutes les sessions** du compte : un
    /// mot de passe changé parce qu'il a fuité ne doit laisser aucune session
    /// ouverte derrière lui.
    pub fn set_password(&self, id: &str, password: &str) -> Result<()> {
        self.store_password(id, password, false)
    }

    /// Comme [`Self::set_password`], mais le compte devra (ou pourra) le changer à sa connexion.
    pub fn set_temporary_password(&self, id: &str, password: &str) -> Result<()> {
        self.store_password(id, password, true)
    }

    /// Renonce à l'invite de changement : le mot de passe actuel est gardé.
    pub fn clear_must_change_password(&self, id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE users SET must_change_password = 0 WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    fn store_password(&self, id: &str, password: &str, temporary: bool) -> Result<()> {
        check_password(password)?;
        let hash = hash_password(password)?;
        let conn = self.conn();
        let changed = conn.execute(
            "UPDATE users SET password_hash = ?2, must_change_password = ?3 WHERE id = ?1",
            params![id, hash, temporary],
        )?;
        if changed == 0 {
            return Err(AccountError::NotFound);
        }
        conn.execute("DELETE FROM sessions WHERE user_id = ?1", [id])?;
        Ok(())
    }

    pub fn delete_user(&self, id: &str) -> Result<()> {
        let conn = self.conn();
        let user = fetch_user(&conn, id)?.ok_or(AccountError::NotFound)?;
        if user.role == Role::Admin && !user.disabled && other_active_admins(&conn, id)? == 0 {
            return Err(AccountError::LastAdmin);
        }
        conn.execute("DELETE FROM users WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Vérifie une adresse e-mail et un mot de passe. Un compte inactif, sans mot
    /// de passe ou inconnu rend `None`, après le même travail de hachage.
    pub fn authenticate(&self, email: &str, password: &str) -> Result<Option<User>> {
        let row: Option<(User, Option<String>)> = {
            let conn = self.conn();
            conn.query_row(
                &format!("SELECT {USER_COLUMNS}, password_hash FROM users WHERE email = ?1"),
                [email.trim()],
                |row| Ok((user_from(row)?, row.get(10)?)),
            )
            .optional()?
        };
        let Some((user, hash)) = row else {
            verify_password(dummy_hash(), password);
            return Ok(None);
        };
        let ok = verify_password(hash.as_deref().unwrap_or_else(|| dummy_hash()), password);
        if !ok || user.disabled || hash.is_none() {
            return Ok(None);
        }
        Ok(Some(user))
    }

    /// Vérifie le mot de passe d'un compte connu (changement de mot de passe).
    pub fn check_user_password(&self, id: &str, password: &str) -> Result<bool> {
        let hash: Option<String> = self
            .conn()
            .query_row(
                "SELECT password_hash FROM users WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(hash.is_some_and(|hash| verify_password(&hash, password)))
    }

    pub fn touch_login(&self, id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE users SET last_login_at = ?2 WHERE id = ?1",
            params![id, now()],
        )?;
        Ok(())
    }

    /// Un identifiant libre, dérivé de `base` (`alice`, `alice-2`, `alice-3`…).
    pub fn available_username(&self, base: &str) -> Result<String> {
        let cleaned: String = base
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(48)
            .collect();
        let base = if cleaned.is_empty() {
            "user".to_string()
        } else {
            cleaned
        };
        let conn = self.conn();
        let taken = |name: &str| -> rusqlite::Result<bool> {
            conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM users WHERE username = ?1)",
                [name],
                |row| row.get(0),
            )
        };
        if !taken(&base)? {
            return Ok(base);
        }
        for suffix in 2..10_000 {
            let candidate = format!("{base}-{suffix}");
            if !taken(&candidate)? {
                return Ok(candidate);
            }
        }
        Ok(format!("{base}-{}", random_token(4)))
    }

    /// Premier démarrage : sans aucun compte, en crée un administrateur.
    ///
    /// Sans mot de passe imposé (variable d'environnement), il est tiré au hasard
    /// et rendu à l'appelant, qui l'affiche une seule fois.
    pub fn bootstrap_admin(
        &self,
        email: &str,
        password: Option<String>,
    ) -> Result<Option<Bootstrap>> {
        if self.count_users()? > 0 {
            return Ok(None);
        }
        let generated = password.is_none().then(|| random_token(15));
        let user = self.create_user(NewUser {
            email: Some(email.to_string()),
            password: password.or_else(|| generated.clone()),
            role: Some(Role::Admin),
            ..NewUser::default()
        })?;
        Ok(Some(Bootstrap {
            user,
            generated_password: generated,
        }))
    }

    /// Force l'administrateur désigné par `email`.
    ///
    /// Le compte est créé s'il n'existe pas ; sinon il est réactivé, ramené au
    /// rôle d'administrateur et son mot de passe est remplacé (ce qui révoque
    /// ses sessions). `EASYBOOK_ADMIN_*` n'étant lu qu'au premier démarrage,
    /// c'est la façon de le rejouer à la demande.
    ///
    /// Une adresse déjà portée par un compte **non administrateur** est refusée
    /// plutôt que promue : la promotion reste une décision explicite.
    pub fn reset_admin(&self, email: &str, password: &str) -> Result<ResetOutcome> {
        let email = require_email(email)?;
        check_password(password)?;
        match self.user_by_email(&email)? {
            Some(user) if user.role == Role::Admin => {
                self.update_user(
                    &user.id,
                    &UserPatch {
                        role: Some(Role::Admin),
                        disabled: Some(false),
                        ..UserPatch::default()
                    },
                )?;
                self.set_password(&user.id, password)?;
                let user = self.user(&user.id)?.ok_or(AccountError::NotFound)?;
                Ok(ResetOutcome::Updated(user))
            }
            Some(_) => Err(AccountError::EmailOwnedByUser),
            None => {
                let user = self.create_user(NewUser {
                    email: Some(email),
                    password: Some(password.to_string()),
                    role: Some(Role::Admin),
                    ..NewUser::default()
                })?;
                Ok(ResetOutcome::Created(user))
            }
        }
    }

    // ── Sessions ─────────────────────────────────────────────────────────

    /// Nombre de sessions ouvertes, toutes personnes confondues.
    pub fn session_count(&self) -> Result<i64> {
        self.conn()
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .map_err(AccountError::from)
    }

    /// Ouvre une session et rend son jeton (la seule fois où il existe en clair).
    pub fn create_session(&self, user_id: &str, ttl_secs: i64) -> Result<String> {
        let token = random_token(32);
        let created = now();
        let conn = self.conn();
        // Ménage opportuniste : pas de tâche de fond pour des sessions échues.
        conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", [created])?;
        conn.execute(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?3)",
            params![digest(&token), user_id, created, created + ttl_secs],
        )?;
        Ok(token)
    }

    /// L'utilisateur d'une session valide (non échue, compte actif).
    pub fn session_user(&self, token: &str) -> Result<Option<User>> {
        let (hash, at) = (digest(token), now());
        let conn = self.conn();
        let user = conn
            .query_row(
                &format!(
                    "SELECT {USER_COLUMNS} FROM users WHERE disabled = 0 AND id =
                     (SELECT user_id FROM sessions WHERE token_hash = ?1 AND expires_at > ?2)"
                ),
                params![hash, at],
                user_from,
            )
            .optional()?;
        if user.is_some() {
            // Au plus une écriture par minute et par session : la garde passe ici à chaque requête.
            conn.execute(
                "UPDATE sessions SET last_seen_at = ?2
                 WHERE token_hash = ?1 AND COALESCE(last_seen_at, 0) < ?2 - 60",
                params![hash, at],
            )?;
        }
        Ok(user)
    }

    /// Identifiant public d'une session (voir [`SessionInfo::id`]).
    pub fn session_id(token: &str) -> String {
        digest(token).chars().take(12).collect()
    }

    /// Ferme une session et rend son compte, si elle existait.
    pub fn end_session(&self, token: &str) -> Result<Option<String>> {
        let conn = self.conn();
        let hash = digest(token);
        let user_id: Option<String> = conn
            .query_row(
                "SELECT user_id FROM sessions WHERE token_hash = ?1",
                [&hash],
                |row| row.get(0),
            )
            .optional()?;
        conn.execute("DELETE FROM sessions WHERE token_hash = ?1", [&hash])?;
        Ok(user_id)
    }

    /// Ferme une session d'un compte, par son identifiant public.
    pub fn delete_session_by_id(&self, user_id: &str, session_id: &str) -> Result<bool> {
        let removed = self.conn().execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND substr(token_hash, 1, 12) = ?2",
            params![user_id, session_id],
        )?;
        Ok(removed > 0)
    }

    /// Ferme les sessions d'un compte, sauf `keep` (identifiant public). Rend leur nombre.
    pub fn delete_sessions_except(&self, user_id: &str, keep: Option<&str>) -> Result<usize> {
        Ok(self.conn().execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND substr(token_hash, 1, 12) != ?2",
            params![user_id, keep.unwrap_or("")],
        )?)
    }

    pub fn delete_session(&self, token: &str) -> Result<()> {
        self.conn().execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            [digest(token)],
        )?;
        Ok(())
    }

    pub fn delete_sessions_of(&self, user_id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM sessions WHERE user_id = ?1", [user_id])?;
        Ok(())
    }

    /// Note d'où vient une session (adresse, navigateur, mode de connexion).
    pub fn tag_session(&self, token: &str, meta: &SessionMeta) -> Result<()> {
        self.conn().execute(
            "UPDATE sessions SET ip = ?2, user_agent = ?3, method = ?4 WHERE token_hash = ?1",
            params![digest(token), meta.ip, meta.user_agent, meta.method],
        )?;
        Ok(())
    }

    /// Ouverture réussie d'une session : la décrit, l'inscrit à l'historique, et rend le compte.
    pub fn log_session_start(&self, token: &str, meta: &SessionMeta) -> Result<Option<String>> {
        self.tag_session(token, meta)?;
        let conn = self.conn();
        let user_id: Option<String> = conn
            .query_row(
                "SELECT user_id FROM sessions WHERE token_hash = ?1",
                [digest(token)],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(user_id) = &user_id {
            insert_login_event(&conn, user_id, true, meta)?;
        }
        Ok(user_id)
    }

    /// Tentative ratée sur un compte qui existe (un identifiant inconnu n'est pas noté).
    pub fn log_failed_login(&self, email: &str, meta: &SessionMeta) -> Result<Option<String>> {
        let conn = self.conn();
        let user_id: Option<String> = conn
            .query_row(
                "SELECT id FROM users WHERE email = ?1",
                [email.trim()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(user_id) = &user_id {
            insert_login_event(&conn, user_id, false, meta)?;
        }
        Ok(user_id)
    }

    /// Sessions encore valides d'un compte, la plus récente d'abord.
    pub fn active_sessions(&self, user_id: &str) -> Result<Vec<SessionInfo>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT substr(token_hash, 1, 12), created_at, expires_at, ip, user_agent, method,
                    COALESCE(last_seen_at, created_at)
             FROM sessions WHERE user_id = ?1 AND expires_at > ?2
             ORDER BY created_at DESC",
        )?;
        Ok(statement
            .query_map(params![user_id, now()], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    expires_at: row.get(2)?,
                    ip: row.get(3)?,
                    user_agent: row.get(4)?,
                    method: row.get(5)?,
                    last_seen_at: row.get(6)?,
                    online: false,
                    current: false,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Dernières connexions (réussies ou non) d'un compte, la plus récente d'abord.
    pub fn login_history(&self, user_id: &str, limit: i64) -> Result<Vec<LoginEvent>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT at, success, method, ip, user_agent FROM login_events
             WHERE user_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2",
        )?;
        Ok(statement
            .query_map(params![user_id, limit], |row| {
                Ok(LoginEvent {
                    at: row.get(0)?,
                    success: row.get(1)?,
                    method: row.get(2)?,
                    ip: row.get(3)?,
                    user_agent: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    // ── Identités OIDC ───────────────────────────────────────────────────

    pub fn identity(&self, issuer: &str, subject: &str) -> Result<Option<Identity>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, user_id, issuer, subject, email, created_at FROM identities
                 WHERE issuer = ?1 AND subject = ?2",
                [issuer, subject],
                identity_from,
            )
            .optional()?)
    }

    pub fn identities_of(&self, user_id: &str) -> Result<Vec<Identity>> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT id, user_id, issuer, subject, email, created_at FROM identities
             WHERE user_id = ?1 ORDER BY created_at, id",
        )?;
        Ok(statement
            .query_map([user_id], identity_from)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Lie une identité OIDC à un compte. Refait sur le même compte : sans effet ;
    /// déjà liée à un autre compte : refus.
    pub fn link_identity(
        &self,
        user_id: &str,
        issuer: &str,
        subject: &str,
        email: Option<&str>,
    ) -> Result<Identity> {
        let conn = self.conn();
        if fetch_user(&conn, user_id)?.is_none() {
            return Err(AccountError::NotFound);
        }
        let existing = conn
            .query_row(
                "SELECT id, user_id, issuer, subject, email, created_at FROM identities
                 WHERE issuer = ?1 AND subject = ?2",
                [issuer, subject],
                identity_from,
            )
            .optional()?;
        if let Some(existing) = existing {
            return if existing.user_id == user_id {
                Ok(existing)
            } else {
                Err(AccountError::IdentityTaken)
            };
        }
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = now();
        conn.execute(
            "INSERT INTO identities (id, user_id, issuer, subject, email, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, user_id, issuer, subject, email, created_at],
        )?;
        Ok(Identity {
            id,
            user_id: user_id.to_string(),
            issuer: issuer.to_string(),
            subject: subject.to_string(),
            email: email.map(str::to_string),
            created_at,
        })
    }

    /// Délie une identité. Refusé si c'était la seule façon de se connecter.
    pub fn unlink_identity(&self, user_id: &str, identity_id: &str) -> Result<()> {
        let conn = self.conn();
        let user = fetch_user(&conn, user_id)?.ok_or(AccountError::NotFound)?;
        let owned: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM identities WHERE id = ?1 AND user_id = ?2)",
            [identity_id, user_id],
            |row| row.get(0),
        )?;
        if !owned {
            return Err(AccountError::NotFound);
        }
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM identities WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?;
        let passkeys: i64 = conn.query_row(
            "SELECT COUNT(*) FROM passkeys WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?;
        if !user.has_password && count + passkeys <= 1 {
            return Err(AccountError::LastCredential);
        }
        conn.execute("DELETE FROM identities WHERE id = ?1", [identity_id])?;
        Ok(())
    }

    // ── Passkeys (WebAuthn) ──────────────────────────────────────────────

    /// Enregistre une passkey vérifiée. Refusé si son identifiant de
    /// credential existe déjà (il ne peut appartenir qu'à un seul compte).
    pub fn add_passkey(
        &self,
        user_id: &str,
        name: &str,
        credential_id: &[u8],
        passkey: &str,
    ) -> Result<PasskeyInfo> {
        let cleaned: String = name.chars().filter(|c| !c.is_control()).collect();
        let cleaned = cleaned.trim();
        let name: String = if cleaned.is_empty() {
            "Passkey".to_string()
        } else {
            cleaned.chars().take(64).collect()
        };
        let conn = self.conn();
        if fetch_user(&conn, user_id)?.is_none() {
            return Err(AccountError::NotFound);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = now();
        let inserted = conn.execute(
            "INSERT INTO passkeys (id, user_id, name, credential_id, passkey, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, user_id, name, credential_id, passkey, created_at],
        );
        match inserted {
            Ok(_) => Ok(PasskeyInfo {
                id,
                name,
                created_at,
                last_used_at: None,
            }),
            Err(e) if is_unique_violation(&e) => Err(AccountError::PasskeyTaken),
            Err(e) => Err(e.into()),
        }
    }

    /// Les passkeys d'un compte, la plus récente d'abord.
    pub fn passkeys_of(&self, user_id: &str) -> Result<Vec<StoredPasskey>> {
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            "SELECT {PASSKEY_COLUMNS} FROM passkeys WHERE user_id = ?1
             ORDER BY created_at DESC, id DESC"
        ))?;
        Ok(statement
            .query_map([user_id], passkey_from)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// La passkey de ce compte portant cet identifiant de credential.
    pub fn passkey_of(&self, user_id: &str, credential_id: &[u8]) -> Result<Option<StoredPasskey>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {PASSKEY_COLUMNS} FROM passkeys
                     WHERE user_id = ?1 AND credential_id = ?2"
                ),
                params![user_id, credential_id],
                passkey_from,
            )
            .optional()?)
    }

    /// Enregistre l'état mis à jour d'une passkey (compteur anti-rejeu, état de
    /// sauvegarde) après une connexion réussie.
    pub fn touch_passkey(&self, user_id: &str, id: &str, passkey: &str) -> Result<()> {
        let changed = self.conn().execute(
            "UPDATE passkeys SET passkey = ?3, last_used_at = ?4 WHERE id = ?1 AND user_id = ?2",
            params![id, user_id, passkey, now()],
        )?;
        if changed == 0 {
            return Err(AccountError::NotFound);
        }
        Ok(())
    }

    /// Retire une passkey. Refusé si c'était la dernière façon de se connecter.
    pub fn delete_passkey(&self, user_id: &str, id: &str) -> Result<()> {
        let conn = self.conn();
        let user = fetch_user(&conn, user_id)?.ok_or(AccountError::NotFound)?;
        let owned: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE id = ?1 AND user_id = ?2)",
            [id, user_id],
            |row| row.get(0),
        )?;
        if !owned {
            return Err(AccountError::NotFound);
        }
        let identities: i64 = conn.query_row(
            "SELECT COUNT(*) FROM identities WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?;
        let passkeys: i64 = conn.query_row(
            "SELECT COUNT(*) FROM passkeys WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?;
        if !user.has_password && identities == 0 && passkeys <= 1 {
            return Err(AccountError::LastCredential);
        }
        conn.execute("DELETE FROM passkeys WHERE id = ?1", [id])?;
        Ok(())
    }

    // ── Invitations de liaison ───────────────────────────────────────────

    /// Jeton à usage unique qui autorise **une** connexion OIDC à se lier à ce
    /// compte. Rendu en clair une seule fois, à l'administrateur.
    pub fn create_link_token(&self, user_id: &str, ttl_secs: i64) -> Result<String> {
        let token = random_token(24);
        let conn = self.conn();
        if fetch_user(&conn, user_id)?.is_none() {
            return Err(AccountError::NotFound);
        }
        conn.execute("DELETE FROM link_tokens WHERE expires_at <= ?1", [now()])?;
        conn.execute(
            "INSERT INTO link_tokens (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)",
            params![digest(&token), user_id, now() + ttl_secs],
        )?;
        Ok(token)
    }

    /// Le compte visé par une invitation encore valide (sans la consommer).
    pub fn link_token_user(&self, token: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT user_id FROM link_tokens WHERE token_hash = ?1 AND expires_at > ?2",
                params![digest(token), now()],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Consomme l'invitation : `true` si elle était valide, une seule fois.
    pub fn consume_link_token(&self, token: &str) -> Result<bool> {
        let removed = self.conn().execute(
            "DELETE FROM link_tokens WHERE token_hash = ?1 AND expires_at > ?2",
            params![digest(token), now()],
        )?;
        Ok(removed == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Accounts {
        Accounts::open_in_memory().unwrap()
    }

    fn user(store: &Accounts, name: &str, role: Role) -> User {
        store
            .create_user(NewUser {
                username: name.into(),
                email: Some(format!("{name}@local.test")),
                password: Some("motdepasse1".into()),
                role: Some(role),
                ..NewUser::default()
            })
            .unwrap()
    }

    #[test]
    fn ids_are_uuids_and_passwords_are_hashed() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        assert!(uuid::Uuid::parse_str(&alice.id).is_ok());
        assert!(alice.has_password);

        let stored: String = store
            .conn()
            .query_row("SELECT password_hash FROM users", [], |row| row.get(0))
            .unwrap();
        assert!(stored.starts_with("$argon2id$"));
        assert!(!stored.contains("motdepasse1"));
    }

    fn with_email(username: &str, email: &str) -> NewUser {
        NewUser {
            username: username.into(),
            email: Some(email.into()),
            ..NewUser::default()
        }
    }

    #[test]
    fn usernames_are_unique_regardless_of_case() {
        let store = store();
        user(&store, "Alice", Role::User);
        let again = store.create_user(with_email("alice", "autre@local.test"));
        assert_eq!(again.unwrap_err(), AccountError::UsernameTaken);
        assert_eq!(
            store.user_by_username("ALICE").unwrap().unwrap().username,
            "Alice"
        );
    }

    #[test]
    fn emails_are_unique_regardless_of_case() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        let again = store.create_user(with_email("autre", "ALICE@local.TEST"));
        assert_eq!(again.unwrap_err(), AccountError::EmailTaken);
        assert_eq!(
            store
                .user_by_email(" Alice@LOCAL.test ")
                .unwrap()
                .unwrap()
                .id,
            alice.id
        );
        assert!(store.user_by_email("zed@local.test").unwrap().is_none());

        // Changer l'adresse d'un compte vers celle d'un autre : refusé.
        let bob = user(&store, "bob", Role::User);
        let taken = UserPatch {
            email: Some("alice@local.test".into()),
            ..UserPatch::default()
        };
        assert_eq!(
            store.update_user(&bob.id, &taken).unwrap_err(),
            AccountError::EmailTaken
        );
    }

    #[test]
    fn the_email_is_required_and_the_username_derived_from_it() {
        let store = store();
        let missing = store.create_user(NewUser {
            username: "bob".into(),
            ..NewUser::default()
        });
        assert_eq!(missing.unwrap_err(), AccountError::InvalidEmail);

        let derived = store
            .create_user(NewUser {
                email: Some("Marie.Curie@example.org".into()),
                ..NewUser::default()
            })
            .unwrap();
        assert_eq!(derived.username, "Marie.Curie");
        let twin = store
            .create_user(NewUser {
                email: Some("marie.curie@example.net".into()),
                ..NewUser::default()
            })
            .unwrap();
        assert_eq!(twin.username, "marie.curie-2");

        // L'adresse d'un compte ne peut pas être effacée.
        let clear = UserPatch {
            email: Some(String::new()),
            ..UserPatch::default()
        };
        assert_eq!(
            store.update_user(&derived.id, &clear).unwrap_err(),
            AccountError::InvalidEmail
        );
    }

    #[test]
    fn only_the_email_logs_in_never_the_username() {
        let store = store();
        user(&store, "alice", Role::User);
        assert!(
            store
                .authenticate("alice@local.test", "motdepasse1")
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .authenticate("alice", "motdepasse1")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn input_is_validated() {
        let store = store();
        let bad = |new: NewUser| store.create_user(new).unwrap_err();
        let named = |username: &str| NewUser {
            username: username.into(),
            email: Some("bob@local.test".into()),
            ..NewUser::default()
        };
        assert_eq!(bad(named("a\nb")), AccountError::InvalidUsername);
        assert_eq!(
            bad(NewUser {
                password: Some("court".into()),
                ..named("bob")
            }),
            AccountError::WeakPassword
        );
        // Trop court, ou blanc : refusé.
        for password in ["court", "        ", "\t\t\t\t\t\t\t\t"] {
            assert_eq!(
                bad(NewUser {
                    password: Some(password.into()),
                    ..named("bob")
                }),
                AccountError::WeakPassword,
                "{password:?}"
            );
        }
        for email in ["pas-une-adresse", "a b@c.d", ""] {
            assert_eq!(
                bad(with_email("bob", email)),
                AccountError::InvalidEmail,
                "{email:?}"
            );
        }
    }

    #[test]
    fn authentication_checks_password_and_state() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        assert!(
            store
                .authenticate("alice@local.test", "motdepasse1")
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .authenticate(" ALICE@LOCAL.TEST ", "motdepasse1")
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .authenticate("alice@local.test", "mauvais")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .authenticate("inconnu@local.test", "motdepasse1")
                .unwrap()
                .is_none()
        );

        store
            .update_user(
                &alice.id,
                &UserPatch {
                    disabled: Some(true),
                    ..UserPatch::default()
                },
            )
            .unwrap();
        assert!(
            store
                .authenticate("alice@local.test", "motdepasse1")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_oidc_only_account_cannot_use_a_password() {
        let store = store();
        let carol = store
            .create_user(with_email("carol", "carol@local.test"))
            .unwrap();
        assert!(!carol.has_password);
        assert!(
            store
                .authenticate("carol@local.test", "")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .authenticate("carol@local.test", "motdepasse1")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn sessions_expire_and_are_stored_hashed() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        let token = store.create_session(&alice.id, 3600).unwrap();
        assert_eq!(store.session_user(&token).unwrap().unwrap().id, alice.id);
        assert!(store.session_user("autre").unwrap().is_none());

        let stored: String = store
            .conn()
            .query_row("SELECT token_hash FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_ne!(stored, token);

        let expired = store.create_session(&alice.id, -1).unwrap();
        assert!(store.session_user(&expired).unwrap().is_none());

        store.delete_session(&token).unwrap();
        assert!(store.session_user(&token).unwrap().is_none());
    }

    #[test]
    fn disabling_or_changing_the_password_revokes_sessions() {
        let store = store();
        user(&store, "root", Role::Admin);
        let alice = user(&store, "alice", Role::User);

        let first = store.create_session(&alice.id, 3600).unwrap();
        store.set_password(&alice.id, "nouveaumdp1").unwrap();
        assert!(store.session_user(&first).unwrap().is_none());
        assert!(
            store
                .authenticate("alice@local.test", "nouveaumdp1")
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .authenticate("alice@local.test", "motdepasse1")
                .unwrap()
                .is_none()
        );

        let second = store.create_session(&alice.id, 3600).unwrap();
        store
            .update_user(
                &alice.id,
                &UserPatch {
                    disabled: Some(true),
                    ..UserPatch::default()
                },
            )
            .unwrap();
        assert!(store.session_user(&second).unwrap().is_none());
    }

    #[test]
    fn the_last_active_admin_is_protected() {
        let store = store();
        let root = user(&store, "root", Role::Admin);
        let demote = UserPatch {
            role: Some(Role::User),
            ..UserPatch::default()
        };
        let disable = UserPatch {
            disabled: Some(true),
            ..UserPatch::default()
        };

        assert_eq!(
            store.update_user(&root.id, &demote).unwrap_err(),
            AccountError::LastAdmin
        );
        assert_eq!(
            store.update_user(&root.id, &disable).unwrap_err(),
            AccountError::LastAdmin
        );
        assert_eq!(
            store.delete_user(&root.id).unwrap_err(),
            AccountError::LastAdmin
        );

        // Un second administrateur libère le premier.
        let other = user(&store, "other", Role::Admin);
        store.update_user(&root.id, &demote).unwrap();
        assert_eq!(
            store.delete_user(&other.id).unwrap_err(),
            AccountError::LastAdmin
        );
    }

    #[test]
    fn update_changes_only_what_is_given() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        let updated = store
            .update_user(
                &alice.id,
                &UserPatch {
                    display_name: Some("  Alice L.  ".into()),
                    email: Some("alice@example.org".into()),
                    ..UserPatch::default()
                },
            )
            .unwrap();
        assert_eq!(updated.display_name.as_deref(), Some("Alice L."));
        assert_eq!(updated.email.as_deref(), Some("alice@example.org"));
        assert_eq!(updated.role, Role::User);

        // Seul ce qui est donné change.
        let renamed = store
            .update_user(
                &alice.id,
                &UserPatch {
                    display_name: Some(String::new()),
                    ..UserPatch::default()
                },
            )
            .unwrap();
        assert_eq!(renamed.display_name, None);
        assert_eq!(renamed.email.as_deref(), Some("alice@example.org"));
        assert_eq!(
            store
                .update_user("inconnu", &UserPatch::default())
                .unwrap_err(),
            AccountError::NotFound
        );
    }

    #[test]
    fn identities_link_once_and_unlink_safely() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        let bob = user(&store, "bob", Role::User);
        let carol = store
            .create_user(with_email("carol", "carol@local.test"))
            .unwrap();

        let identity = store
            .link_identity(&alice.id, "https://idp", "sub-1", Some("a@x.org"))
            .unwrap();
        // Relier deux fois le même compte ne change rien.
        let again = store
            .link_identity(&alice.id, "https://idp", "sub-1", None)
            .unwrap();
        assert_eq!(again.id, identity.id);
        // Une identité appartient à un seul compte.
        assert_eq!(
            store
                .link_identity(&bob.id, "https://idp", "sub-1", None)
                .unwrap_err(),
            AccountError::IdentityTaken
        );
        // Même sujet, autre émetteur : une autre identité.
        assert!(
            store
                .link_identity(&bob.id, "https://autre", "sub-1", None)
                .is_ok()
        );

        assert_eq!(
            store
                .identity("https://idp", "sub-1")
                .unwrap()
                .unwrap()
                .user_id,
            alice.id
        );
        assert_eq!(store.identities_of(&alice.id).unwrap().len(), 1);

        // Alice a un mot de passe : elle peut délier sa seule identité.
        store.unlink_identity(&alice.id, &identity.id).unwrap();
        assert!(store.identity("https://idp", "sub-1").unwrap().is_none());

        // Carol n'a que son identité : la délier la verrouillerait.
        let only = store
            .link_identity(&carol.id, "https://idp", "sub-3", None)
            .unwrap();
        assert_eq!(
            store.unlink_identity(&carol.id, &only.id).unwrap_err(),
            AccountError::LastCredential
        );
        // Une identité qui n'est pas la sienne est introuvable.
        assert_eq!(
            store.unlink_identity(&alice.id, &only.id).unwrap_err(),
            AccountError::NotFound
        );
    }

    #[test]
    fn passkeys_are_stored_listed_and_removed() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        let first = store
            .add_passkey(&alice.id, "Portable", b"cred-1", "{}")
            .unwrap();
        assert_eq!(first.name, "Portable");
        assert_eq!(first.last_used_at, None);
        // Le nom vide prend un libellé par défaut, et les caractères de contrôle partent.
        let second = store
            .add_passkey(&alice.id, "  \u{7}", b"cred-2", "{}")
            .unwrap();
        assert_eq!(second.name, "Passkey");

        let list = store.passkeys_of(&alice.id).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|row| row.credential_id == b"cred-1"));

        store
            .touch_passkey(&alice.id, &first.id, "{\"état\":1}")
            .unwrap();
        let touched = store.passkey_of(&alice.id, b"cred-1").unwrap().unwrap();
        assert_eq!(touched.passkey, "{\"état\":1}");
        assert!(touched.last_used_at.is_some());

        store.delete_passkey(&alice.id, &first.id).unwrap();
        assert_eq!(store.passkeys_of(&alice.id).unwrap().len(), 1);
        // Une passkey d'un autre compte est introuvable.
        assert_eq!(
            store.delete_passkey(&alice.id, &first.id).unwrap_err(),
            AccountError::NotFound
        );
    }

    #[test]
    fn a_credential_cannot_belong_to_two_accounts() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        let bob = user(&store, "bob", Role::User);
        store.add_passkey(&alice.id, "A", b"same", "{}").unwrap();
        assert_eq!(
            store.add_passkey(&bob.id, "B", b"same", "{}").unwrap_err(),
            AccountError::PasskeyTaken
        );
    }

    #[test]
    fn removing_every_way_to_sign_in_is_refused() {
        let store = store();
        // Un compte sans mot de passe, avec une seule passkey : impossible de la retirer.
        let alice = store
            .create_user(with_email("alice", "alice@local.test"))
            .unwrap();
        let only = store
            .add_passkey(&alice.id, "Portable", b"cred", "{}")
            .unwrap();
        assert_eq!(
            store.delete_passkey(&alice.id, &only.id).unwrap_err(),
            AccountError::LastCredential
        );
        // Avec un mot de passe, la retirer ne verrouille rien.
        store.set_password(&alice.id, "motdepasse2").unwrap();
        store.delete_passkey(&alice.id, &only.id).unwrap();
    }

    #[test]
    fn deleting_a_user_removes_its_identities_and_sessions() {
        let store = store();
        user(&store, "root", Role::Admin);
        let alice = user(&store, "alice", Role::User);
        store
            .link_identity(&alice.id, "https://idp", "sub-1", None)
            .unwrap();
        let token = store.create_session(&alice.id, 3600).unwrap();

        store.delete_user(&alice.id).unwrap();
        assert!(store.identity("https://idp", "sub-1").unwrap().is_none());
        assert!(store.session_user(&token).unwrap().is_none());
    }

    #[test]
    fn link_tokens_are_single_use_and_expire() {
        let store = store();
        let alice = user(&store, "alice", Role::User);
        let token = store.create_link_token(&alice.id, 600).unwrap();

        assert_eq!(
            store.link_token_user(&token).unwrap().as_deref(),
            Some(alice.id.as_str())
        );
        assert!(store.link_token_user("faux").unwrap().is_none());
        assert!(store.consume_link_token(&token).unwrap());
        assert!(!store.consume_link_token(&token).unwrap());
        assert!(store.link_token_user(&token).unwrap().is_none());

        let stale = store.create_link_token(&alice.id, -1).unwrap();
        assert!(store.link_token_user(&stale).unwrap().is_none());
        assert!(!store.consume_link_token(&stale).unwrap());
        assert_eq!(
            store.create_link_token("inconnu", 600).unwrap_err(),
            AccountError::NotFound
        );
    }

    #[test]
    fn available_username_avoids_collisions() {
        let store = store();
        assert_eq!(store.available_username("alice").unwrap(), "alice");
        user(&store, "alice", Role::User);
        assert_eq!(store.available_username("alice").unwrap(), "alice-2");
        user(&store, "alice-2", Role::User);
        assert_eq!(store.available_username("Alice").unwrap(), "Alice-3");
        assert_eq!(store.available_username("  ").unwrap(), "user");
    }

    #[test]
    fn bootstrap_creates_an_admin_only_on_an_empty_base() {
        let store = store();
        let first = store
            .bootstrap_admin("admin@x.test", None)
            .unwrap()
            .unwrap();
        assert_eq!(first.user.role, Role::Admin);
        let password = first.generated_password.unwrap();
        assert!(
            store
                .authenticate("admin@x.test", &password)
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .bootstrap_admin("autre@x.test", None)
                .unwrap()
                .is_none()
        );

        let imposed = Accounts::open_in_memory().unwrap();
        let boot = imposed
            .bootstrap_admin("root@x.test", Some("imposé-123".into()))
            .unwrap()
            .unwrap();
        assert!(boot.generated_password.is_none());
        assert!(
            imposed
                .authenticate("root@x.test", "imposé-123")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn reset_admin_creates_then_updates_the_same_account() {
        let store = store();
        let created = store.reset_admin("admin@x.test", "premier-mdp-1").unwrap();
        let ResetOutcome::Created(first) = created else {
            panic!("attendu Created");
        };
        assert_eq!(first.role, Role::Admin);
        assert!(
            store
                .authenticate("admin@x.test", "premier-mdp-1")
                .unwrap()
                .is_some()
        );

        // Rejouer avec un autre mot de passe ne crée pas de doublon.
        let updated = store.reset_admin("ADMIN@x.test", "second-mdp-2").unwrap();
        let ResetOutcome::Updated(second) = updated else {
            panic!("attendu Updated");
        };
        assert_eq!(second.id, first.id);
        assert_eq!(store.count_users().unwrap(), 1);
        assert!(
            store
                .authenticate("admin@x.test", "premier-mdp-1")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .authenticate("admin@x.test", "second-mdp-2")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn reset_admin_reactivates_a_disabled_admin() {
        let store = store();
        let _other = user(&store, "other", Role::Admin);
        let root = user(&store, "root", Role::Admin);
        store
            .update_user(
                &root.id,
                &UserPatch {
                    disabled: Some(true),
                    ..UserPatch::default()
                },
            )
            .unwrap();
        assert!(
            store
                .authenticate("root@local.test", "motdepasse1")
                .unwrap()
                .is_none()
        );

        store
            .reset_admin("root@local.test", "nouveau-mdp-3")
            .unwrap();
        assert!(
            store
                .authenticate("root@local.test", "nouveau-mdp-3")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn reset_admin_refuses_an_address_owned_by_a_plain_user() {
        let store = store();
        user(&store, "alice", Role::User);
        assert_eq!(
            store
                .reset_admin("alice@local.test", "nouveau-mdp-4")
                .unwrap_err(),
            AccountError::EmailOwnedByUser
        );
        // Rien n'a bougé : ni le rôle, ni le mot de passe.
        assert!(
            store
                .authenticate("alice@local.test", "motdepasse1")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn the_base_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sous/accounts.db");
        let id = {
            let store = Accounts::open(&path).unwrap();
            user(&store, "alice", Role::User).id
        };
        let store = Accounts::open(&path).unwrap();
        assert_eq!(store.user(&id).unwrap().unwrap().username, "alice");
        assert_eq!(store.count_users().unwrap(), 1);
    }

    #[test]
    fn a_v1_base_is_migrated_to_email_login() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.execute_batch(
                "ALTER TABLE users DROP COLUMN must_change_password;
                 ALTER TABLE sessions DROP COLUMN ip;
                 ALTER TABLE sessions DROP COLUMN user_agent;
                 ALTER TABLE sessions DROP COLUMN method;
                 ALTER TABLE sessions DROP COLUMN last_seen_at;
                 DROP TABLE login_events;
                 DROP INDEX users_email;
                 CREATE INDEX users_email ON users (email);
                 INSERT INTO users (id, username, role, created_at) VALUES ('u1', 'Admin', 'admin', 0);
                 INSERT INTO users (id, username, email, role, created_at)
                     VALUES ('u2', 'bob', 'bob@x.test', 'user', 0);",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let store = Accounts::open(&path).unwrap();
        // Un compte sans adresse en reçoit une, déduite de son nom.
        assert_eq!(
            store.user("u1").unwrap().unwrap().email.as_deref(),
            Some("admin@easybook.local")
        );
        assert_eq!(
            store.user("u2").unwrap().unwrap().email.as_deref(),
            Some("bob@x.test")
        );
        // La colonne de la v3 existe, désactivée.
        assert!(!store.user("u1").unwrap().unwrap().must_change_password);
        // L'unicité est désormais imposée.
        assert_eq!(
            store
                .create_user(with_email("x", "BOB@x.test"))
                .unwrap_err(),
            AccountError::EmailTaken
        );
    }

    #[test]
    fn a_temporary_password_is_flagged_until_changed_or_skipped() {
        let store = Accounts::open_in_memory().unwrap();
        let alice = store
            .create_user(NewUser {
                email: Some("alice@x.test".into()),
                password: Some("temporaire-01".into()),
                must_change_password: true,
                ..NewUser::default()
            })
            .unwrap();
        assert!(alice.must_change_password);

        store.clear_must_change_password(&alice.id).unwrap();
        assert!(!store.user(&alice.id).unwrap().unwrap().must_change_password);

        store
            .set_temporary_password(&alice.id, "temporaire-02")
            .unwrap();
        assert!(store.user(&alice.id).unwrap().unwrap().must_change_password);
        // Un vrai changement lève le drapeau.
        store.set_password(&alice.id, "definitif-01").unwrap();
        assert!(!store.user(&alice.id).unwrap().unwrap().must_change_password);

        // Sans mot de passe, rien à changer.
        let carol = store
            .create_user(NewUser {
                email: Some("carol@x.test".into()),
                must_change_password: true,
                ..NewUser::default()
            })
            .unwrap();
        assert!(!carol.must_change_password);
    }

    #[test]
    fn sessions_and_login_history_are_described() {
        let store = Accounts::open_in_memory().unwrap();
        let alice = user(&store, "alice", Role::User);
        let meta = |method: &'static str, ip: &str| SessionMeta {
            ip: Some(ip.into()),
            user_agent: Some("Mozilla/5.0 test".into()),
            method,
        };

        let first = store.create_session(&alice.id, 3600).unwrap();
        store
            .log_session_start(&first, &meta("password", "10.0.0.1"))
            .unwrap();
        let second = store.create_session(&alice.id, 3600).unwrap();
        store
            .log_session_start(&second, &meta("sso", "10.0.0.2"))
            .unwrap();
        // Une session échue ne compte pas.
        let expired = store.create_session(&alice.id, 3600).unwrap();
        store
            .tag_session(&expired, &meta("password", "10.0.0.3"))
            .unwrap();
        store
            .conn()
            .execute(
                "UPDATE sessions SET expires_at = 1 WHERE token_hash = ?1",
                [digest(&expired)],
            )
            .unwrap();

        let sessions = store.active_sessions(&alice.id).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|s| s.method.as_deref() == Some("sso")));
        assert!(sessions.iter().all(|s| s.id.len() == 12));
        // L'identifiant ne permet pas de se faire passer pour la session.
        assert!(sessions.iter().all(|s| s.id != first && s.id != second));

        store
            .log_failed_login("ALICE@local.test", &meta("password", "10.0.0.9"))
            .unwrap();
        // Un identifiant inconnu ne laisse aucune trace.
        store
            .log_failed_login("nobody@x.test", &meta("password", "10.0.0.9"))
            .unwrap();
        let history = store.login_history(&alice.id, 50).unwrap();
        assert_eq!(history.len(), 3);
        assert!(!history[0].success);
        assert_eq!(history[0].ip.as_deref(), Some("10.0.0.9"));
        assert_eq!(history.iter().filter(|e| e.success).count(), 2);

        // L'historique est borné.
        for _ in 0..(LOGIN_HISTORY_KEPT + 20) {
            store
                .log_failed_login("alice@local.test", &meta("password", "1.1.1.1"))
                .unwrap();
        }
        assert_eq!(
            store.login_history(&alice.id, 1000).unwrap().len() as i64,
            LOGIN_HISTORY_KEPT
        );
    }

    #[test]
    fn sessions_are_closed_one_by_one_or_all_but_one() {
        let store = Accounts::open_in_memory().unwrap();
        let alice = user(&store, "alice", Role::User);
        let bob = user(&store, "bob", Role::User);
        let tokens: Vec<String> = (0..3)
            .map(|_| store.create_session(&alice.id, 3600).unwrap())
            .collect();
        let bobs = store.create_session(&bob.id, 3600).unwrap();

        // Une session se ferme par son identifiant public, et seulement dans son compte.
        let target = Accounts::session_id(&tokens[0]);
        assert!(!store.delete_session_by_id(&bob.id, &target).unwrap());
        assert!(store.delete_session_by_id(&alice.id, &target).unwrap());
        assert!(!store.delete_session_by_id(&alice.id, &target).unwrap());
        assert!(store.session_user(&tokens[0]).unwrap().is_none());
        assert!(store.session_user(&tokens[1]).unwrap().is_some());

        // Toutes sauf celle de l'appelant.
        let keep = Accounts::session_id(&tokens[1]);
        assert_eq!(
            store
                .delete_sessions_except(&alice.id, Some(&keep))
                .unwrap(),
            1
        );
        assert!(store.session_user(&tokens[1]).unwrap().is_some());
        assert!(store.session_user(&tokens[2]).unwrap().is_none());
        assert!(store.session_user(&bobs).unwrap().is_some());
        assert_eq!(store.delete_sessions_except(&alice.id, None).unwrap(), 1);

        // `end_session` rend le compte, et ne répond rien pour un jeton inconnu.
        assert_eq!(
            store.end_session(&bobs).unwrap().as_deref(),
            Some(bob.id.as_str())
        );
        assert_eq!(store.end_session(&bobs).unwrap(), None);
    }

    #[test]
    fn the_last_sign_of_life_of_a_session_is_kept_up_to_date() {
        let store = Accounts::open_in_memory().unwrap();
        let alice = user(&store, "alice", Role::User);
        let token = store.create_session(&alice.id, 3600).unwrap();
        let seen = |store: &Accounts| store.active_sessions(&alice.id).unwrap()[0].last_seen_at;

        let opened = seen(&store);
        // Moins d'une minute après : pas de nouvelle écriture.
        store.session_user(&token).unwrap();
        assert_eq!(seen(&store), opened);

        // Une session inactive depuis un moment est vue à nouveau.
        store
            .conn()
            .execute("UPDATE sessions SET last_seen_at = last_seen_at - 600", [])
            .unwrap();
        let stale = seen(&store);
        store.session_user(&token).unwrap();
        assert!(seen(&store) > stale);
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.db");
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        assert!(matches!(
            Accounts::open(&path),
            Err(AccountError::Internal(_))
        ));
    }
}
