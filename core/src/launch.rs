use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::auth::AuthService;
use crate::config::ConfigManager;
use crate::error::{CoreError, CoreResult};
use crate::java::{JavaInfo, JavaManager};
use crate::job::JobManager;
use crate::model::{Account, AccountKind, GameStatus, Instance};
use crate::paths::AppPaths;
use crate::store::Store;
use crate::version::{rules_allow, ArgumentValue, JavaRequirement, MergedVersion, VersionService};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshot {
    pub instance_id: Uuid,
    pub status: GameStatus,
    pub process_id: Option<u32>,
    pub started_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
}

struct GameEntry {
    snapshot: GameSnapshot,
    child: Arc<Mutex<Child>>,
    started_at: Instant,
}

struct LaunchPlan {
    java: JavaInfo,
    account: Account,
    working_directory: PathBuf,
    arguments: Vec<String>,
    sensitive: Vec<String>,
    warnings: Vec<String>,
}

fn can_launch(status: crate::model::InstallStatus, has_modpack_record: bool) -> bool {
    status == crate::model::InstallStatus::Installed
        || (status == crate::model::InstallStatus::Incomplete && has_modpack_record)
}

#[derive(Clone)]
pub struct LaunchService {
    paths: AppPaths,
    store: Arc<Store>,
    config: Arc<ConfigManager>,
    auth: AuthService,
    versions: VersionService,
    java: JavaManager,
    jobs: JobManager,
    processes: Arc<Mutex<HashMap<Uuid, GameEntry>>>,
}

impl LaunchService {
    pub fn new(
        paths: AppPaths,
        store: Arc<Store>,
        config: Arc<ConfigManager>,
        auth: AuthService,
        versions: VersionService,
        java: JavaManager,
        jobs: JobManager,
    ) -> Self {
        Self {
            paths,
            store,
            config,
            auth,
            versions,
            java,
            jobs,
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn preflight(&self, id: Uuid) -> CoreResult<Value> {
        match self.prepare(id).await {
            Ok(plan) => {
                let warnings = plan
                    .warnings
                    .iter()
                    .map(|message| json!({"code": "modpack_incomplete", "message": message}))
                    .collect::<Vec<_>>();
                Ok(json!({
                    "instanceId": id,
                    "valid": true,
                    "issues": [],
                    "warnings": warnings,
                    "java": plan.java,
                    "account": {
                        "id": plan.account.id,
                        "name": plan.account.name,
                        "kind": plan.account.kind,
                        "verified": plan.account.verified,
                    },
                }))
            }
            Err(error) => Ok(json!({
                "instanceId": id,
                "valid": false,
                "issues": [{
                    "code": error.code(),
                    "message": error.to_string(),
                    "retryable": error.retryable(),
                }],
            })),
        }
    }

    async fn prepare(&self, id: Uuid) -> CoreResult<LaunchPlan> {
        let instance = self.store.instance(id)?;
        let has_modpack_record = self.store.modpack(id)?.is_some();
        if !can_launch(instance.install_status, has_modpack_record) {
            return Err(CoreError::Conflict(
                "instance is not completely installed".into(),
            ));
        }
        let warnings = (instance.install_status == crate::model::InstallStatus::Incomplete)
            .then(|| "some modpack files are incomplete".to_owned())
            .into_iter()
            .collect();
        let working_directory = self.paths.instance(id);
        if !working_directory.is_dir() {
            return Err(CoreError::NotFound("instance working directory".into()));
        }
        let account_id = instance
            .account_id
            .or(self.config.get().default_account_id)
            .ok_or(CoreError::Unauthorized)?;
        let account = self.store.account(account_id)?;
        let access_token = match account.kind {
            AccountKind::Microsoft => {
                if !account.verified {
                    return Err(CoreError::Unauthorized);
                }
                self.auth.access_token(account.id).await?
            }
            AccountKind::Debug => {
                if !cfg!(feature = "debug") {
                    return Err(CoreError::Forbidden(
                        "this binary does not allow debug accounts".into(),
                    ));
                }
                "0".into()
            }
        };
        let merged = self.versions.merged_for_instance(&instance).await?;
        let requirement = merged.java_version.clone().unwrap_or(JavaRequirement {
            component: Some("jre-legacy".into()),
            major_version: 8,
        });
        let java = self
            .java
            .select(&requirement, instance.java_path.as_deref())
            .await?;
        let arguments = self.arguments(&instance, &account, &access_token, &merged, &java)?;
        Ok(LaunchPlan {
            java,
            account,
            working_directory,
            arguments,
            sensitive: vec![access_token],
            warnings,
        })
    }

    fn arguments(
        &self,
        instance: &Instance,
        account: &Account,
        access_token: &str,
        version: &MergedVersion,
        java: &JavaInfo,
    ) -> CoreResult<Vec<String>> {
        let instance_root = self.paths.instance(instance.id);
        let natives = instance_root.join("natives");
        let client = self
            .paths
            .content
            .join("versions")
            .join(&instance.game_version)
            .join(format!("{}.jar", instance.game_version));
        if !client.is_file() {
            return Err(CoreError::NotFound("Minecraft client jar".into()));
        }
        let mut libraries = Vec::new();
        for library in version
            .libraries
            .iter()
            .filter(|library| rules_allow(&library.rules, &HashMap::new()))
        {
            if library.downloads.artifact.is_some() || library.url.is_some() {
                let artifact = crate::version::library_artifact(library)?;
                let path = artifact.path.as_ref().ok_or_else(|| {
                    CoreError::InvalidInput(format!("library {} has no path", library.name))
                })?;
                let path = crate::paths::safe_join(&self.paths.content.join("libraries"), path)?;
                if !path.is_file() {
                    return Err(CoreError::NotFound(format!("library {}", path.display())));
                }
                libraries.push(path);
            }
        }
        libraries.push(client);
        let classpath = libraries
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(if cfg!(windows) { ";" } else { ":" });
        let assets_root = self.paths.content.join("assets");
        let asset_index = version
            .asset_index
            .as_ref()
            .map(|index| index.id.clone())
            .unwrap_or_else(|| version.assets.clone());
        let placeholders = BTreeMap::from([
            ("auth_player_name".into(), account.name.clone()),
            ("auth_uuid".into(), account.profile_id.clone()),
            ("auth_access_token".into(), access_token.into()),
            ("auth_session".into(), access_token.into()),
            (
                "user_type".into(),
                match account.kind {
                    AccountKind::Microsoft => "msa".into(),
                    AccountKind::Debug => "offline".into(),
                },
            ),
            ("user_properties".into(), "{}".into()),
            ("version_name".into(), instance.game_version.clone()),
            (
                "game_directory".into(),
                instance_root.to_string_lossy().into_owned(),
            ),
            (
                "assets_root".into(),
                assets_root.to_string_lossy().into_owned(),
            ),
            ("assets_index_name".into(), asset_index),
            ("version_type".into(), version.release_type.clone()),
            (
                "natives_directory".into(),
                natives.to_string_lossy().into_owned(),
            ),
            (
                "library_directory".into(),
                self.paths
                    .content
                    .join("libraries")
                    .to_string_lossy()
                    .into_owned(),
            ),
            (
                "classpath_separator".into(),
                if cfg!(windows) {
                    ";".into()
                } else {
                    ":".into()
                },
            ),
            ("classpath".into(), classpath),
            ("launcher_name".into(), "MzLauncher".into()),
            ("launcher_version".into(), crate::CORE_VERSION.into()),
        ]);
        let mut arguments = vec![
            format!("-Xms{}M", instance.min_memory_mb),
            format!("-Xmx{}M", instance.max_memory_mb),
        ];
        arguments.extend(instance.jvm_args.iter().cloned());
        if let Some(version_arguments) = &version.arguments {
            arguments.extend(resolve_arguments(
                &version_arguments.jvm,
                &placeholders,
                &HashMap::new(),
            ));
            arguments.push(version.main_class.clone());
            arguments.extend(resolve_arguments(
                &version_arguments.game,
                &placeholders,
                &HashMap::new(),
            ));
        } else {
            arguments.push(format!("-Djava.library.path={}", natives.to_string_lossy()));
            arguments.push("-cp".into());
            arguments.push(placeholders["classpath"].clone());
            arguments.push(version.main_class.clone());
            let legacy = version
                .minecraft_arguments
                .as_ref()
                .ok_or_else(|| CoreError::InvalidInput("version has no game arguments".into()))?;
            arguments.extend(
                legacy
                    .split_whitespace()
                    .map(|argument| replace_placeholders(argument, &placeholders)),
            );
        }
        arguments.extend(instance.game_args.iter().cloned());
        if arguments.iter().any(|argument| argument.contains('\0')) {
            return Err(CoreError::InvalidInput(
                "launch arguments contain a null character".into(),
            ));
        }
        let _ = java;
        Ok(arguments)
    }

    pub async fn launch(&self, id: Uuid) -> CoreResult<GameSnapshot> {
        if let Some(existing) = self.status(id).await? {
            if matches!(
                existing.status,
                GameStatus::Starting | GameStatus::Running | GameStatus::Stopping
            ) {
                return Err(CoreError::Conflict(
                    "this instance already has a running game process".into(),
                ));
            }
        }
        let plan = self.prepare(id).await?;
        let log_directory = plan.working_directory.join("logs");
        tokio::fs::create_dir_all(&log_directory).await?;
        let latest = log_directory.join("latest.log");
        let previous = log_directory.join("previous.log");
        if latest.exists() {
            let _ = tokio::fs::remove_file(&previous).await;
            tokio::fs::rename(&latest, &previous).await?;
        }
        let log_file = Arc::new(Mutex::new(tokio::fs::File::create(&latest).await?));
        let mut command = tokio::process::Command::new(&plan.java.path);
        command
            .args(&plan.arguments)
            .current_dir(&plan.working_directory)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(false);
        let mut child = command
            .spawn()
            .map_err(|error| CoreError::InvalidInput(format!("could not start Java: {error}")))?;
        let process_id = child.id();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CoreError::Internal("game stdout was not captured".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CoreError::Internal("game stderr was not captured".into()))?;
        let snapshot = GameSnapshot {
            instance_id: id,
            status: GameStatus::Running,
            process_id,
            started_at: Some(Utc::now()),
            exit_code: None,
        };
        let child = Arc::new(Mutex::new(child));
        self.processes.lock().await.insert(
            id,
            GameEntry {
                snapshot: snapshot.clone(),
                child: child.clone(),
                started_at: Instant::now(),
            },
        );
        self.jobs.emit_game(id, GameStatus::Running);
        self.spawn_log_pump(
            id,
            BufReader::new(stdout),
            log_file.clone(),
            plan.sensitive.clone(),
        );
        self.spawn_log_pump(id, BufReader::new(stderr), log_file, plan.sensitive);
        let service = self.clone();
        tokio::spawn(async move {
            service.monitor(id, child).await;
        });
        let mut instance = self.store.instance(id)?;
        instance.last_launched_at = Some(Utc::now());
        instance.updated_at = Utc::now();
        self.store.save_instance(&instance)?;
        Ok(snapshot)
    }

    fn spawn_log_pump(
        &self,
        id: Uuid,
        reader: impl AsyncRead + Unpin + Send + 'static,
        file: Arc<Mutex<tokio::fs::File>>,
        sensitive: Vec<String>,
    ) {
        let jobs = self.jobs.clone();
        tokio::spawn(async move {
            if let Err(error) = pump_logs(reader, file, id, jobs, sensitive).await {
                tracing::warn!(%id, %error, "game log capture stopped");
            }
        });
    }

    async fn monitor(&self, id: Uuid, child: Arc<Mutex<Child>>) {
        loop {
            let result = child.lock().await.try_wait();
            match result {
                Ok(Some(status)) => {
                    let game_status = if status.success() {
                        GameStatus::Stopped
                    } else {
                        GameStatus::Failed
                    };
                    let started_at = {
                        let mut processes = self.processes.lock().await;
                        processes.get_mut(&id).map(|entry| {
                            entry.snapshot.status = game_status;
                            entry.snapshot.exit_code = status.code();
                            entry.started_at
                        })
                    };
                    if let Some(started_at) = started_at {
                        if let Err(error) = self.record_play_time(id, started_at.elapsed()) {
                            tracing::warn!(%id, %error, "could not save instance play time");
                        }
                    }
                    self.jobs.emit_game(id, game_status);
                    break;
                }
                Ok(None) => tokio::time::sleep(Duration::from_millis(250)).await,
                Err(error) => {
                    tracing::warn!(%id, %error, "could not monitor game process");
                    self.jobs.emit_game(id, GameStatus::Failed);
                    break;
                }
            }
        }
    }

    fn record_play_time(&self, id: Uuid, elapsed: Duration) -> CoreResult<()> {
        let mut instance = self.store.instance(id)?;
        add_play_time(&mut instance, elapsed);
        instance.updated_at = Utc::now();
        self.store.save_instance(&instance)
    }

    pub async fn status(&self, id: Uuid) -> CoreResult<Option<GameSnapshot>> {
        self.store.instance(id)?;
        Ok(self
            .processes
            .lock()
            .await
            .get(&id)
            .map(|entry| entry.snapshot.clone()))
    }

    pub async fn stop(&self, id: Uuid, force: bool) -> CoreResult<GameSnapshot> {
        let child = {
            let mut processes = self.processes.lock().await;
            let entry = processes
                .get_mut(&id)
                .ok_or_else(|| CoreError::NotFound("running game process".into()))?;
            if matches!(
                entry.snapshot.status,
                GameStatus::Stopped | GameStatus::Failed
            ) {
                return Ok(entry.snapshot.clone());
            }
            entry.snapshot.status = GameStatus::Stopping;
            entry.child.clone()
        };
        let mut child = child.lock().await;
        if force {
            child.kill().await?;
        } else if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(b"stop\n").await?;
            stdin.flush().await?;
        } else {
            return Err(CoreError::Unsupported(
                "game stdin is unavailable; use explicit force stop".into(),
            ));
        }
        self.jobs.emit_game(id, GameStatus::Stopping);
        self.status(id)
            .await?
            .ok_or_else(|| CoreError::NotFound("game process state".into()))
    }

    pub async fn logs(&self, id: Uuid, tail: usize) -> CoreResult<Vec<String>> {
        self.store.instance(id)?;
        let path = self.paths.instance(id).join("logs").join("latest.log");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = tokio::fs::File::open(path).await?;
        let length = file.metadata().await?.len();
        let start = length.saturating_sub(2 * 1024 * 1024);
        file.seek(std::io::SeekFrom::Start(start)).await?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).await?;
        if start > 0 {
            if let Some(newline) = bytes.iter().position(|byte| *byte == b'\n') {
                bytes.drain(..=newline);
            }
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines = text.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        Ok(lines
            .into_iter()
            .rev()
            .take(tail.clamp(1, 2000))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }
}

fn resolve_arguments(
    arguments: &[ArgumentValue],
    placeholders: &BTreeMap<String, String>,
    features: &HashMap<String, bool>,
) -> Vec<String> {
    let mut result = Vec::new();
    for argument in arguments {
        match argument {
            ArgumentValue::Plain(value) => result.push(replace_placeholders(value, placeholders)),
            ArgumentValue::Conditional { rules, value } if rules_allow(rules, features) => {
                result.extend(
                    value
                        .clone()
                        .into_vec()
                        .iter()
                        .map(|argument| replace_placeholders(argument, placeholders)),
                );
            }
            ArgumentValue::Conditional { .. } => {}
        }
    }
    result
}

fn replace_placeholders(argument: &str, placeholders: &BTreeMap<String, String>) -> String {
    let mut result = argument.to_owned();
    for (key, value) in placeholders {
        result = result.replace(&format!("${{{key}}}"), value);
    }
    result
}

async fn pump_logs(
    mut reader: impl AsyncRead + Unpin,
    file: Arc<Mutex<tokio::fs::File>>,
    id: Uuid,
    jobs: JobManager,
    sensitive: Vec<String>,
) -> CoreResult<()> {
    let mut pending = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            if !pending.is_empty() {
                write_log_line(&file, id, &jobs, &pending, &sensitive).await?;
            }
            break;
        }
        pending.extend_from_slice(&buffer[..read]);
        while let Some(position) = pending.iter().position(|byte| *byte == b'\n') {
            let line = pending.drain(..=position).collect::<Vec<_>>();
            write_log_line(&file, id, &jobs, &line, &sensitive).await?;
        }
        if pending.len() > 64 * 1024 {
            pending.drain(..pending.len() - 64 * 1024);
        }
    }
    file.lock().await.flush().await?;
    Ok(())
}

async fn write_log_line(
    file: &Arc<Mutex<tokio::fs::File>>,
    id: Uuid,
    jobs: &JobManager,
    bytes: &[u8],
    sensitive: &[String],
) -> CoreResult<()> {
    let mut line = String::from_utf8_lossy(bytes)
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    for secret in sensitive.iter().filter(|secret| secret.len() >= 4) {
        line = line.replace(secret, "<redacted>");
    }
    let mut file = file.lock().await;
    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    drop(file);
    jobs.emit_log(id, line);
    Ok(())
}

fn add_play_time(instance: &mut Instance, elapsed: Duration) {
    let milliseconds = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    instance.total_play_time_ms = instance.total_play_time_ms.saturating_add(milliseconds);
}

pub fn is_game_directory(path: &Path) -> bool {
    path.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_play_time_without_overflow() {
        let now = Utc::now();
        let mut instance: Instance = serde_json::from_value(json!({
            "id": Uuid::new_v4(),
            "name": "test",
            "description": null,
            "gameVersion": "1.21.1",
            "loader": "vanilla",
            "loaderVersion": null,
            "installStatus": "installed",
            "javaPath": null,
            "minMemoryMb": 512,
            "maxMemoryMb": 4096,
            "jvmArgs": [],
            "gameArgs": [],
            "accountId": null,
            "createdAt": now,
            "updatedAt": now,
            "lastLaunchedAt": null
        }))
        .unwrap();
        assert_eq!(instance.total_play_time_ms, 0);
        add_play_time(&mut instance, Duration::from_millis(1500));
        add_play_time(&mut instance, Duration::from_millis(250));
        assert_eq!(instance.total_play_time_ms, 1750);
        instance.total_play_time_ms = u64::MAX;
        add_play_time(&mut instance, Duration::from_millis(1));
        assert_eq!(instance.total_play_time_ms, u64::MAX);
    }

    #[test]
    fn launches_incomplete_modpacks_only_with_a_saved_record() {
        assert!(can_launch(crate::model::InstallStatus::Installed, false));
        assert!(!can_launch(crate::model::InstallStatus::Incomplete, false));
        assert!(can_launch(crate::model::InstallStatus::Incomplete, true));
        assert!(!can_launch(crate::model::InstallStatus::NotInstalled, true));
    }

    #[test]
    fn replaces_only_known_placeholders() {
        let placeholders = BTreeMap::from([
            ("auth_player_name".into(), "Player".into()),
            ("auth_access_token".into(), "secret".into()),
        ]);
        assert_eq!(
            replace_placeholders("--username ${auth_player_name} ${unknown}", &placeholders),
            "--username Player ${unknown}"
        );
    }
}
