use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::error::ProtocolError;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

impl Response {
    pub fn success(id: String, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: String, error: ProtocolError) -> Self {
        Self {
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    Job {
        sequence: u64,
        job: Box<JobSnapshot>,
    },
    Game {
        sequence: u64,
        instance_id: Uuid,
        status: GameStatus,
    },
    Log {
        sequence: u64,
        instance_id: Uuid,
        line: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeParams {
    pub client_version: String,
    pub min_protocol: u32,
    pub max_protocol: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeResult {
    pub core_version: String,
    pub protocol_version: u32,
    pub data_version: u32,
    pub platform: String,
    pub capabilities: Vec<String>,
    pub debug_auth: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Download,
    Install,
    Import,
    ModUpdate,
    CacheClean,
    DataMigration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Failed,
    Succeeded,
    Interrupted,
}

impl JobStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Failed | Self::Succeeded | Self::Interrupted
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub stage: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: Uuid,
    pub kind: JobKind,
    pub status: JobStatus,
    pub target: Option<Uuid>,
    pub idempotency_key: Option<String>,
    pub progress: JobProgress,
    pub result: Option<Value>,
    pub error: Option<ProtocolErrorData>,
    pub checkpoint: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolErrorData {
    pub code: String,
    pub message: String,
    pub message_key: String,
    pub retryable: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoaderKind {
    #[default]
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    NotInstalled,
    Installing,
    Installed,
    Incomplete,
    Broken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub game_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub install_status: InstallStatus,
    pub java_path: Option<String>,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub account_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_launched_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub total_play_time_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Microsoft,
    Debug,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: Uuid,
    pub profile_id: String,
    pub name: String,
    pub kind: AccountKind,
    pub verified: bool,
    pub skin_url: Option<String>,
    pub skin_model: Option<String>,
    pub credential_key: Option<String>,
    pub last_validated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModProvider {
    Modrinth,
    CurseForge,
    Ftb,
    Manual,
    Modpack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    Release,
    Beta,
    Alpha,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDependency {
    pub project_id: String,
    pub version_id: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub provider: ModProvider,
    pub project_id: String,
    pub file_id: String,
    pub project_name: String,
    pub version_name: String,
    pub file_name: String,
    pub url: Option<String>,
    pub size: Option<u64>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<LoaderKind>,
    pub channel: ReleaseChannel,
    pub dependencies: Vec<ModDependency>,
    pub published_at: Option<DateTime<Utc>>,
    pub distribution_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub file: ModFile,
    pub enabled: bool,
    pub managed: bool,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModpackEntryStatus {
    Imported,
    Skipped,
    Failed,
    RequiresManualDownload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackEntryResult {
    pub name: String,
    pub status: ModpackEntryStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackRecord {
    pub instance_id: Uuid,
    pub format: String,
    pub source_name: String,
    pub source_sha256: String,
    pub imported_at: DateTime<Utc>,
    pub entries: Vec<ModpackEntryResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticInfo {
    pub core_version: String,
    pub protocol_version: u32,
    pub data_version: u32,
    pub platform: String,
    pub capabilities: Vec<String>,
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceParams {
    pub name: String,
    pub description: Option<String>,
    pub game_version: String,
    #[serde(default)]
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub java_path: Option<String>,
    pub min_memory_mb: Option<u32>,
    pub max_memory_mb: Option<u32>,
    pub jvm_args: Option<Vec<String>>,
    #[serde(default)]
    pub game_args: Vec<String>,
    pub account_id: Option<Uuid>,
}
