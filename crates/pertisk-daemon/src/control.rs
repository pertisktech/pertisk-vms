//! SQLite control plane: users, tokens, tasks, audit.

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use pertisk_api::{
    AuditEvent, CreateUserRequest, Role, TaskRecord, TaskStatus, TokenResponse, UserRecord,
};
use rusqlite::{Connection, params};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("user not found: {0}")]
    UserNotFound(String),
    #[error("username already exists: {0}")]
    UserExists(String),
    #[error("invalid credentials")]
    BadCredentials,
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("password hash: {0}")]
    Password(String),
}

#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub role: Role,
}

#[derive(Debug)]
pub struct ControlStore {
    conn: Mutex<Connection>,
}

impl ControlStore {
    pub fn open(
        path: impl AsRef<Path>,
        admin_password: Option<&str>,
    ) -> Result<Self, ControlError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| ControlError::Message(format!("create control db dir: {err}")))?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL,
                created_unix INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tokens (
                token TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                created_unix INTEGER NOT NULL,
                FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                actor TEXT NOT NULL,
                target TEXT,
                error TEXT,
                created_unix INTEGER NOT NULL,
                finished_unix INTEGER
            );
            CREATE TABLE IF NOT EXISTS audit (
                id TEXT PRIMARY KEY,
                actor TEXT NOT NULL,
                action TEXT NOT NULL,
                target TEXT,
                created_unix INTEGER NOT NULL
            );
            ",
        )?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.ensure_bootstrap_admin(admin_password.unwrap_or("admin"))?;
        Ok(store)
    }

    fn ensure_bootstrap_admin(&self, password: &str) -> Result<(), ControlError> {
        let conn = self.conn.lock().expect("control lock");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        drop(conn);
        if count > 0 {
            return Ok(());
        }
        self.create_user(CreateUserRequest {
            username: "admin".into(),
            password: password.to_string(),
            role: Role::Admin,
        })?;
        tracing::warn!("bootstrap user 'admin' created");
        Ok(())
    }

    pub fn create_user(&self, req: CreateUserRequest) -> Result<UserRecord, ControlError> {
        if req.username.trim().is_empty() || req.password.len() < 4 {
            return Err(ControlError::Message(
                "username required and password must be at least 4 characters".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let hash = hash_password(&req.password)?;
        let now = unix_now();
        let conn = self.conn.lock().expect("control lock");
        match conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_unix) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, req.username, hash, req.role.as_str(), now as i64],
        ) {
            Ok(_) => Ok(UserRecord {
                id,
                username: req.username,
                role: req.role,
            }),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(ControlError::UserExists(req.username))
            }
            Err(err) => Err(err.into()),
        }
    }

    pub fn list_users(&self) -> Result<Vec<UserRecord>, ControlError> {
        let conn = self.conn.lock().expect("control lock");
        let mut stmt = conn.prepare("SELECT id, username, role FROM users ORDER BY username")?;
        let rows = stmt.query_map([], |row| {
            Ok(UserRecord {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get::<_, String>(2)?.parse().unwrap_or(Role::Viewer),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn delete_user(&self, id: &str) -> Result<(), ControlError> {
        let conn = self.conn.lock().expect("control lock");
        let n = conn.execute("DELETE FROM users WHERE id = ?1", params![id])?;
        if n == 0 {
            Err(ControlError::UserNotFound(id.into()))
        } else {
            Ok(())
        }
    }

    pub fn login(&self, username: &str, password: &str) -> Result<TokenResponse, ControlError> {
        let conn = self.conn.lock().expect("control lock");
        let row = conn.query_row(
            "SELECT id, username, password_hash, role FROM users WHERE username = ?1",
            params![username],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        );
        let (id, name, hash, role) = match row {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err(ControlError::BadCredentials);
            }
            Err(err) => return Err(err.into()),
        };
        verify_password(password, &hash)?;
        let role: Role = role.parse().unwrap_or(Role::Viewer);
        let token = format!(
            "{}{}",
            Uuid::new_v4().as_simple(),
            Uuid::new_v4().as_simple()
        );
        let now = unix_now() as i64;
        conn.execute(
            "INSERT INTO tokens (token, user_id, created_unix) VALUES (?1, ?2, ?3)",
            params![token, id, now],
        )?;
        Ok(TokenResponse {
            token,
            username: name,
            role,
        })
    }

    pub fn authenticate(&self, token: &str) -> Result<AuthUser, ControlError> {
        let conn = self.conn.lock().expect("control lock");
        let row = conn.query_row(
            "SELECT users.id, users.username, users.role
             FROM tokens JOIN users ON users.id = tokens.user_id
             WHERE tokens.token = ?1",
            params![token],
            |row| {
                Ok(AuthUser {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    role: row.get::<_, String>(2)?.parse().unwrap_or(Role::Viewer),
                })
            },
        );
        match row {
            Ok(user) => Ok(user),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(ControlError::Unauthorized),
            Err(err) => Err(err.into()),
        }
    }

    pub fn begin_task(
        &self,
        actor: &str,
        kind: &str,
        target: Option<&str>,
    ) -> Result<TaskRecord, ControlError> {
        let record = TaskRecord {
            id: Uuid::new_v4().to_string(),
            kind: kind.into(),
            status: TaskStatus::Running,
            actor: actor.into(),
            target: target.map(str::to_string),
            error: None,
            created_unix: unix_now(),
            finished_unix: None,
        };
        let conn = self.conn.lock().expect("control lock");
        conn.execute(
            "INSERT INTO tasks (id, kind, status, actor, target, error, created_unix, finished_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, NULL)",
            params![
                record.id,
                record.kind,
                record.status.to_string(),
                record.actor,
                record.target,
                record.created_unix as i64
            ],
        )?;
        Ok(record)
    }

    pub fn finish_task(
        &self,
        id: &str,
        result: Result<(), String>,
    ) -> Result<TaskRecord, ControlError> {
        let (status, error) = match result {
            Ok(()) => (TaskStatus::Done, None),
            Err(err) => (TaskStatus::Error, Some(err)),
        };
        let finished = unix_now() as i64;
        let conn = self.conn.lock().expect("control lock");
        conn.execute(
            "UPDATE tasks SET status = ?1, error = ?2, finished_unix = ?3 WHERE id = ?4",
            params![status.to_string(), error, finished, id],
        )?;
        drop(conn);
        self.get_task(id)
    }

    pub fn get_task(&self, id: &str) -> Result<TaskRecord, ControlError> {
        let conn = self.conn.lock().expect("control lock");
        conn.query_row(
            "SELECT id, kind, status, actor, target, error, created_unix, finished_unix FROM tasks WHERE id = ?1",
            params![id],
            row_to_task,
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => ControlError::Message(format!("task not found: {id}")),
            err => err.into(),
        })
    }

    pub fn list_tasks(&self) -> Result<Vec<TaskRecord>, ControlError> {
        let conn = self.conn.lock().expect("control lock");
        let mut stmt = conn.prepare(
            "SELECT id, kind, status, actor, target, error, created_unix, finished_unix
             FROM tasks ORDER BY created_unix DESC, id DESC LIMIT 100",
        )?;
        let rows = stmt.query_map([], row_to_task)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn audit(
        &self,
        actor: &str,
        action: &str,
        target: Option<&str>,
    ) -> Result<(), ControlError> {
        let conn = self.conn.lock().expect("control lock");
        conn.execute(
            "INSERT INTO audit (id, actor, action, target, created_unix) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                Uuid::new_v4().to_string(),
                actor,
                action,
                target,
                unix_now() as i64
            ],
        )?;
        Ok(())
    }

    pub fn list_audit(&self) -> Result<Vec<AuditEvent>, ControlError> {
        let conn = self.conn.lock().expect("control lock");
        let mut stmt = conn.prepare(
            "SELECT id, actor, action, target, created_unix FROM audit ORDER BY created_unix DESC, id DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AuditEvent {
                id: row.get(0)?,
                actor: row.get(1)?,
                action: row.get(2)?,
                target: row.get(3)?,
                created_unix: row.get::<_, i64>(4)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    let status: String = row.get(2)?;
    Ok(TaskRecord {
        id: row.get(0)?,
        kind: row.get(1)?,
        status: match status.as_str() {
            "done" => TaskStatus::Done,
            "error" => TaskStatus::Error,
            _ => TaskStatus::Running,
        },
        actor: row.get(3)?,
        target: row.get(4)?,
        error: row.get(5)?,
        created_unix: row.get::<_, i64>(6)? as u64,
        finished_unix: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
    })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hash_password(password: &str) -> Result<String, ControlError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|err| ControlError::Password(err.to_string()))
}

fn verify_password(password: &str, hash: &str) -> Result<(), ControlError> {
    let parsed = PasswordHash::new(hash).map_err(|err| ControlError::Password(err.to_string()))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| ControlError::BadCredentials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_and_task_flow() {
        let dir = tempfile::tempdir().unwrap();
        let store = ControlStore::open(dir.path().join("c.db"), Some("secret")).unwrap();
        assert!(store.login("admin", "wrong").is_err());
        let token = store.login("admin", "secret").unwrap();
        let user = store.authenticate(&token.token).unwrap();
        assert_eq!(user.username, "admin");
        let task = store.begin_task("admin", "vm.start", Some("vm-1")).unwrap();
        store.finish_task(&task.id, Ok(())).unwrap();
        assert_eq!(store.list_tasks().unwrap()[0].status, TaskStatus::Done);
        store.audit("admin", "POST /v1/vms", Some("vm-1")).unwrap();
        assert_eq!(store.list_audit().unwrap().len(), 1);
    }
}
