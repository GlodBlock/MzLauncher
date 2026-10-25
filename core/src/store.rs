use std::fs::{File, OpenOptions};

use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};
use crate::model::{Account, InstalledMod, JobSnapshot, JobStatus, ProtocolErrorData};
use crate::model::{Instance, ModpackRecord};
use crate::paths::AppPaths;
use crate::DATA_VERSION;

pub struct Store {
    connection: Mutex<Connection>,
    _lock: File,
}

impl Store {
    pub fn open(paths: &AppPaths) -> CoreResult<Self> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(paths.lock_file())?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| {
            CoreError::Conflict("another core process is using this data directory".into())
        })?;
        let connection = Connection::open(paths.database_file())?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;",
        )?;
        let store = Self {
            connection: Mutex::new(connection),
            _lock: lock,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> CoreResult<()> {
        let connection = self.connection.lock();
        let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > DATA_VERSION {
            return Err(CoreError::Storage(format!(
                "database version {version} is newer than supported version {DATA_VERSION}"
            )));
        }
        if version == 0 {
            connection.execute_batch(
                "BEGIN IMMEDIATE;
                CREATE TABLE instances (
                    id TEXT PRIMARY KEY,
                    payload TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE accounts (
                    id TEXT PRIMARY KEY,
                    payload TEXT NOT NULL
                );
                CREATE TABLE mods (
                    id TEXT PRIMARY KEY,
                    instance_id TEXT NOT NULL,
                    payload TEXT NOT NULL,
                    FOREIGN KEY(instance_id) REFERENCES instances(id) ON DELETE CASCADE
                );
                CREATE INDEX mods_instance ON mods(instance_id);
                CREATE TABLE jobs (
                    id TEXT PRIMARY KEY,
                    idempotency_key TEXT UNIQUE,
                    payload TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE modpacks (
                    instance_id TEXT PRIMARY KEY,
                    payload TEXT NOT NULL,
                    FOREIGN KEY(instance_id) REFERENCES instances(id) ON DELETE CASCADE
                );
                PRAGMA user_version = 1;
                COMMIT;",
            )?;
        }
        Ok(())
    }

    pub fn save_instance(&self, instance: &Instance) -> CoreResult<()> {
        self.connection.lock().execute(
            "INSERT INTO instances(id, payload, updated_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload, updated_at = excluded.updated_at",
            params![instance.id.to_string(), serde_json::to_string(instance)?, instance.updated_at],
        )?;
        Ok(())
    }

    pub fn instance(&self, id: Uuid) -> CoreResult<Instance> {
        self.connection
            .lock()
            .query_row(
                "SELECT payload FROM instances WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str(&payload).map_err(CoreError::from))
            .transpose()?
            .ok_or_else(|| CoreError::NotFound(format!("instance {id}")))
    }

    pub fn instances(&self) -> CoreResult<Vec<Instance>> {
        let connection = self.connection.lock();
        let mut statement =
            connection.prepare("SELECT payload FROM instances ORDER BY updated_at DESC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let payload = row?;
            serde_json::from_str(&payload).map_err(CoreError::from)
        })
        .collect()
    }

    pub fn delete_instance(&self, id: Uuid) -> CoreResult<()> {
        let changed = self.connection.lock().execute(
            "DELETE FROM instances WHERE id = ?1",
            params![id.to_string()],
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound(format!("instance {id}")));
        }
        Ok(())
    }

    pub fn save_account(&self, account: &Account) -> CoreResult<()> {
        self.connection.lock().execute(
            "INSERT INTO accounts(id, payload) VALUES(?1, ?2)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            params![account.id.to_string(), serde_json::to_string(account)?],
        )?;
        Ok(())
    }

    pub fn account(&self, id: Uuid) -> CoreResult<Account> {
        self.connection
            .lock()
            .query_row(
                "SELECT payload FROM accounts WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str(&payload).map_err(CoreError::from))
            .transpose()?
            .ok_or_else(|| CoreError::NotFound(format!("account {id}")))
    }

    pub fn accounts(&self) -> CoreResult<Vec<Account>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare("SELECT payload FROM accounts ORDER BY id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let payload = row?;
            serde_json::from_str(&payload).map_err(CoreError::from)
        })
        .collect()
    }

    pub fn delete_account(&self, id: Uuid) -> CoreResult<()> {
        self.connection.lock().execute(
            "DELETE FROM accounts WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn save_mod(&self, installed: &InstalledMod) -> CoreResult<()> {
        self.connection.lock().execute(
            "INSERT INTO mods(id, instance_id, payload) VALUES(?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            params![
                installed.id.to_string(),
                installed.instance_id.to_string(),
                serde_json::to_string(installed)?
            ],
        )?;
        Ok(())
    }

    pub fn mods(&self, instance_id: Uuid) -> CoreResult<Vec<InstalledMod>> {
        let connection = self.connection.lock();
        let mut statement =
            connection.prepare("SELECT payload FROM mods WHERE instance_id = ?1 ORDER BY id")?;
        let rows = statement.query_map(params![instance_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| {
            let payload = row?;
            serde_json::from_str(&payload).map_err(CoreError::from)
        })
        .collect()
    }

    pub fn delete_mod(&self, id: Uuid) -> CoreResult<()> {
        self.connection
            .lock()
            .execute("DELETE FROM mods WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

    pub fn save_job(&self, job: &JobSnapshot) -> CoreResult<()> {
        self.connection.lock().execute(
            "INSERT INTO jobs(id, idempotency_key, payload, created_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            params![
                job.id.to_string(),
                job.idempotency_key,
                serde_json::to_string(job)?,
                job.created_at
            ],
        )?;
        Ok(())
    }

    pub fn job(&self, id: Uuid) -> CoreResult<JobSnapshot> {
        self.connection
            .lock()
            .query_row(
                "SELECT payload FROM jobs WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str(&payload).map_err(CoreError::from))
            .transpose()?
            .ok_or_else(|| CoreError::NotFound(format!("job {id}")))
    }

    pub fn job_by_idempotency_key(&self, key: &str) -> CoreResult<Option<JobSnapshot>> {
        self.connection
            .lock()
            .query_row(
                "SELECT payload FROM jobs WHERE idempotency_key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str(&payload).map_err(CoreError::from))
            .transpose()
    }

    pub fn jobs(&self) -> CoreResult<Vec<JobSnapshot>> {
        let connection = self.connection.lock();
        let mut statement =
            connection.prepare("SELECT payload FROM jobs ORDER BY created_at DESC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let payload = row?;
            serde_json::from_str(&payload).map_err(CoreError::from)
        })
        .collect()
    }

    pub fn recover_jobs(&self) -> CoreResult<usize> {
        let active = self
            .jobs()?
            .into_iter()
            .filter(|job| !job.status.is_terminal())
            .collect::<Vec<_>>();
        let count = active.len();
        for mut job in active {
            job.status = JobStatus::Interrupted;
            job.finished_at = Some(Utc::now());
            job.error = Some(ProtocolErrorData {
                code: "interrupted".into(),
                message: "operation interrupted by core restart".into(),
                message_key: "error.interrupted".into(),
                retryable: true,
                cancelled: false,
            });
            self.save_job(&job)?;
        }
        Ok(count)
    }

    pub fn save_modpack(&self, record: &ModpackRecord) -> CoreResult<()> {
        self.connection.lock().execute(
            "INSERT INTO modpacks(instance_id, payload) VALUES(?1, ?2)
             ON CONFLICT(instance_id) DO UPDATE SET payload = excluded.payload",
            params![
                record.instance_id.to_string(),
                serde_json::to_string(record)?
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InstallStatus, LoaderKind};

    #[test]
    fn persists_isolated_instances_and_recovers_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(temp.path().join("data"));
        paths.initialize().unwrap();
        let store = Store::open(&paths).unwrap();
        for name in ["one", "two"] {
            let now = Utc::now();
            store
                .save_instance(&Instance {
                    id: Uuid::new_v4(),
                    name: name.into(),
                    description: None,
                    game_version: "1.21.1".into(),
                    loader: LoaderKind::Vanilla,
                    loader_version: None,
                    install_status: InstallStatus::NotInstalled,
                    java_path: None,
                    min_memory_mb: 512,
                    max_memory_mb: 4096,
                    jvm_args: vec![],
                    game_args: vec![],
                    account_id: None,
                    created_at: now,
                    updated_at: now,
                    last_launched_at: None,
                })
                .unwrap();
        }
        assert_eq!(store.instances().unwrap().len(), 2);
    }
}
