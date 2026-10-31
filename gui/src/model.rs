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
    pub checkpoint: Option<Value>,
}

impl Job {
    pub fn instance_id(&self) -> Option<&str> {
        self.target.as_deref().or_else(|| {
            self.checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.get("instanceId"))
                .and_then(Value::as_str)
                .or_else(|| {
                    self.result
                        .as_ref()
                        .and_then(|result| result.get("instanceId"))
                        .and_then(Value::as_str)
                })
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub stage: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
    #[serde(default)]
    pub active_items: Vec<String>,
    #[serde(default)]
    pub pending_items: Vec<String>,
}

impl JobProgress {
    pub fn active_items(&self) -> Vec<&str> {
        if self.active_items.is_empty() {
            self.current_item.as_deref().into_iter().collect()
        } else {
            self.active_items.iter().map(String::as_str).collect()
        }
    }
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
    #[serde(default = "default_ftb_api")]
    pub ftb_api: String,
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
    #[serde(default)]
    pub icon_url: Option<String>,
    pub page_url: Option<String>,
    pub downloads: u64,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
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
pub struct ModpackEntryResult {
    pub name: String,
    pub status: String,
    pub detail: Option<String>,
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

fn default_ftb_api() -> String {
    "https://api.feed-the-beast.com/v1/modpacks".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn job(target: Option<&str>, checkpoint: Value, result: Value) -> Job {
        serde_json::from_value(json!({
            "id": "job-id",
            "kind": "import",
            "status": "running",
            "target": target,
            "progress": {
                "stage": "running",
                "completed": 1,
                "total": 2,
                "currentItem": null,
            },
            "error": null,
            "result": result,
            "checkpoint": checkpoint,
        }))
        .unwrap()
    }

    #[test]
    fn active_items_fall_back_to_legacy_current_item() {
        let legacy = job(None, Value::Null, Value::Null);
        assert!(legacy.progress.active_items().is_empty());

        let mut legacy = legacy.progress;
        legacy.current_item = Some("current.jar".into());
        assert_eq!(legacy.active_items(), vec!["current.jar"]);

        let concurrent = JobProgress {
            active_items: vec!["first.jar".into(), "second.jar".into()],
            ..legacy
        };
        assert_eq!(concurrent.active_items(), vec!["first.jar", "second.jar"]);
    }

    #[test]
    fn job_instance_id_tracks_modpack_creation() {
        let pending = job(None, Value::Null, Value::Null);
        assert_eq!(pending.instance_id(), None);

        let created = job(None, json!({"instanceId": "created-id"}), Value::Null);
        assert_eq!(created.instance_id(), Some("created-id"));

        let targeted = job(
            Some("target-id"),
            json!({"instanceId": "created-id"}),
            Value::Null,
        );
        assert_eq!(targeted.instance_id(), Some("target-id"));
    }
}
