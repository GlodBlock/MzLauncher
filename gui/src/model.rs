use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeInfo {
    pub core_version: String,
    pub protocol_version: u32,
    pub data_version: u32,
    pub platform: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub game_version: String,
    pub loader: String,
    pub loader_version: Option<String>,
    pub install_status: String,
    pub java_path: Option<String>,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub account_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_launched_at: Option<String>,
    pub total_play_time_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub verified: bool,
    pub skin_url: Option<String>,
    pub skin_model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub target: Option<String>,
    pub progress: JobProgress,
    pub error: Option<JobError>,
    pub result: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub stage: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionSummary {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginSession {
    pub id: String,
    pub status: String,
    pub authorization_url: String,
    pub account_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshot {
    pub instance_id: String,
    pub status: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInfo {
    pub path: String,
    pub major_version: u32,
    pub architecture: String,
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub version: u32,
    pub download: DownloadSettings,
    pub endpoints: EndpointSettings,
    pub runtime_dir: Option<String>,
    pub microsoft_client_id: Option<String>,
    pub default_account_id: Option<String>,
    pub launch: LaunchSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSettings {
    pub java_path: Option<String>,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSettings {
    pub max_concurrent: usize,
    pub max_retries: u32,
    pub connect_timeout_seconds: u64,
    pub read_timeout_seconds: u64,
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointSettings {
    pub version_manifest: String,
    pub java_runtime_manifest: String,
    pub fabric_meta: String,
    pub modrinth_api: String,
    pub curseforge_api: String,
    pub forge_maven: String,
    pub neoforge_maven: String,
    pub microsoft_authority: String,
    pub xbox_auth: String,
    pub xsts_auth: String,
    pub minecraft_auth: String,
    pub minecraft_profile: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModProject {
    pub provider: String,
    pub project_id: String,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub page_url: Option<String>,
    pub downloads: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub provider: String,
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
    pub loaders: Vec<String>,
    pub channel: String,
    pub dependencies: Vec<ModDependency>,
    pub published_at: Option<String>,
    pub distribution_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDependency {
    pub project_id: String,
    pub version_id: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub id: String,
    pub file: ModFile,
    pub enabled: bool,
    pub managed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackAnalysis {
    pub format: String,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub game_version: String,
    pub loader: String,
    pub loader_version: Option<String>,
    pub mod_count: usize,
    pub override_prefixes: Vec<String>,
    pub warnings: Vec<String>,
}
