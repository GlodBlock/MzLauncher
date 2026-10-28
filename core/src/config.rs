use std::fs;
use std::path::PathBuf;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};
use crate::paths::{atomic_write, AppPaths};
use crate::DATA_VERSION;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    pub download: DownloadConfig,
    pub endpoints: EndpointConfig,
    #[serde(default)]
    pub launch: LaunchConfig,
    #[serde(default)]
    pub runtime_dir: Option<PathBuf>,
    pub microsoft_client_id: Option<String>,
    pub default_account_id: Option<Uuid>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: DATA_VERSION,
            download: DownloadConfig::default(),
            endpoints: EndpointConfig::default(),
            launch: LaunchConfig::default(),
            runtime_dir: None,
            microsoft_client_id: std::env::var("MZLAUNCHER_MICROSOFT_CLIENT_ID").ok(),
            default_account_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    pub java_path: Option<PathBuf>,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            java_path: None,
            min_memory_mb: 512,
            max_memory_mb: 4096,
            jvm_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadConfig {
    pub max_concurrent: usize,
    pub max_retries: u32,
    pub connect_timeout_seconds: u64,
    pub read_timeout_seconds: u64,
    pub proxy_url: Option<String>,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            max_retries: 3,
            connect_timeout_seconds: 20,
            read_timeout_seconds: 60,
            proxy_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointConfig {
    pub version_manifest: String,
    pub java_runtime_manifest: String,
    pub fabric_meta: String,
    pub modrinth_api: String,
    pub curseforge_api: String,
    #[serde(default = "default_forge_maven")]
    pub forge_maven: String,
    #[serde(default = "default_neoforge_maven")]
    pub neoforge_maven: String,
    pub microsoft_authority: String,
    pub xbox_auth: String,
    pub xsts_auth: String,
    pub minecraft_auth: String,
    pub minecraft_profile: String,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            version_manifest: "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".into(),
            java_runtime_manifest: "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json".into(),
            fabric_meta: "https://meta.fabricmc.net".into(),
            modrinth_api: "https://api.modrinth.com/v2".into(),
            curseforge_api: "https://api.curseforge.com/v1".into(),
            forge_maven: default_forge_maven(),
            neoforge_maven: default_neoforge_maven(),
            microsoft_authority: "https://login.microsoftonline.com/consumers/oauth2/v2.0".into(),
            xbox_auth: "https://user.auth.xboxlive.com/user/authenticate".into(),
            xsts_auth: "https://xsts.auth.xboxlive.com/xsts/authorize".into(),
            minecraft_auth: "https://api.minecraftservices.com/authentication/login_with_xbox".into(),
            minecraft_profile: "https://api.minecraftservices.com/minecraft/profile".into(),
        }
    }
}

fn default_forge_maven() -> String {
    "https://maven.minecraftforge.net/net/minecraftforge/forge".into()
}

fn default_neoforge_maven() -> String {
    "https://maven.neoforged.net/releases/net/neoforged/neoforge".into()
}

pub struct ConfigManager {
    paths: AppPaths,
    config: RwLock<AppConfig>,
}

impl ConfigManager {
    pub fn load(paths: AppPaths) -> CoreResult<Self> {
        let file = paths.config_file();
        let config = if file.exists() {
            let bytes = fs::read(&file)?;
            serde_json::from_slice::<AppConfig>(&bytes).map_err(|error| {
                CoreError::Configuration(format!("invalid settings file: {error}"))
            })?
        } else {
            let config = AppConfig::default();
            atomic_write(&file, &serde_json::to_vec_pretty(&config)?)?;
            config
        };
        if config.version > DATA_VERSION {
            return Err(CoreError::Configuration(format!(
                "settings version {} is newer than supported version {DATA_VERSION}",
                config.version
            )));
        }
        config.validate()?;
        Ok(Self {
            paths,
            config: RwLock::new(config),
        })
    }

    pub fn get(&self) -> AppConfig {
        self.config.read().clone()
    }

    pub fn replace(&self, mut config: AppConfig) -> CoreResult<AppConfig> {
        config.version = DATA_VERSION;
        config.validate()?;
        atomic_write(
            &self.paths.config_file(),
            &serde_json::to_vec_pretty(&config)?,
        )?;
        *self.config.write() = config.clone();
        Ok(config)
    }

    pub fn reset(&self) -> CoreResult<AppConfig> {
        self.replace(AppConfig::default())
    }
}

impl AppConfig {
    pub fn runtime_root(&self, paths: &AppPaths) -> PathBuf {
        self.runtime_dir
            .clone()
            .unwrap_or_else(|| paths.runtimes.clone())
    }

    pub fn validate(&self) -> CoreResult<()> {
        if let Some(dir) = &self.runtime_dir {
            if !dir.is_absolute() {
                return Err(CoreError::InvalidInput(
                    "Java runtime directory must be an absolute path".into(),
                ));
            }
        }
        if self
            .launch
            .java_path
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err(CoreError::InvalidInput(
                "Java path must be an absolute path".into(),
            ));
        }
        if !(256..=65_536).contains(&self.launch.min_memory_mb)
            || !(256..=65_536).contains(&self.launch.max_memory_mb)
            || self.launch.min_memory_mb > self.launch.max_memory_mb
        {
            return Err(CoreError::InvalidInput(
                "memory must be between 256 and 65536 MiB, and minimum cannot exceed maximum"
                    .into(),
            ));
        }
        if self
            .launch
            .jvm_args
            .iter()
            .any(|argument| argument.is_empty() || argument.contains('\0'))
        {
            return Err(CoreError::InvalidInput(
                "Java arguments must be non-empty and contain no null characters".into(),
            ));
        }
        if !(1..=32).contains(&self.download.max_concurrent) {
            return Err(CoreError::InvalidInput(
                "download concurrency must be between 1 and 32".into(),
            ));
        }
        if self.download.max_retries > 10
            || !(1..=120).contains(&self.download.connect_timeout_seconds)
            || !(1..=600).contains(&self.download.read_timeout_seconds)
        {
            return Err(CoreError::InvalidInput(
                "download retry or timeout settings are outside supported limits".into(),
            ));
        }
        if let Some(proxy) = &self.download.proxy_url {
            let url = Url::parse(proxy)
                .map_err(|_| CoreError::InvalidInput("invalid proxy URL".into()))?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(CoreError::InvalidInput(
                    "proxy must be HTTP(S) without embedded credentials".into(),
                ));
            }
        }
        for endpoint in [
            &self.endpoints.version_manifest,
            &self.endpoints.java_runtime_manifest,
            &self.endpoints.fabric_meta,
            &self.endpoints.modrinth_api,
            &self.endpoints.curseforge_api,
            &self.endpoints.forge_maven,
            &self.endpoints.neoforge_maven,
            &self.endpoints.microsoft_authority,
            &self.endpoints.xbox_auth,
            &self.endpoints.xsts_auth,
            &self.endpoints.minecraft_auth,
            &self.endpoints.minecraft_profile,
        ] {
            let url = Url::parse(endpoint)
                .map_err(|_| CoreError::InvalidInput(format!("invalid endpoint: {endpoint}")))?;
            let loopback = url
                .host_str()
                .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
            if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
                return Err(CoreError::InvalidInput(format!(
                    "endpoint must use HTTPS (except loopback development endpoints): {endpoint}"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_defaults_are_compatible_and_camel_case() {
        let config = AppConfig::default();
        assert!(config.launch.java_path.is_none());
        assert_eq!(config.launch.min_memory_mb, 512);
        assert_eq!(config.launch.max_memory_mb, 4096);
        assert!(config.launch.jvm_args.is_empty());
        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(value["launch"]["minMemoryMb"], 512);
        assert!(value["launch"]["min_memory_mb"].is_null());
        let mut legacy = value;
        legacy.as_object_mut().unwrap().remove("launch");
        assert!(serde_json::from_value::<AppConfig>(legacy).is_ok());
    }

    #[test]
    fn validates_launch_settings() {
        let mut config = AppConfig::default();
        config.launch.java_path = Some(PathBuf::from("java"));
        assert!(config.validate().is_err());
        config.launch.java_path = None;
        config.launch.max_memory_mb = 65_537;
        assert!(config.validate().is_err());
        config.launch.max_memory_mb = 4096;
        config.launch.jvm_args = vec!["bad\0argument".into()];
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_proxy_credentials() {
        let mut config = AppConfig::default();
        config.download.proxy_url = Some("http://user:password@localhost:8080".into());
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_relative_runtime_directory() {
        let mut config = AppConfig::default();
        config.runtime_dir = Some(PathBuf::from("runtimes"));
        assert!(config.validate().is_err());
        config.runtime_dir = None;
        assert!(config.validate().is_ok());
    }
}
