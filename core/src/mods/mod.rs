mod curseforge;
mod modrinth;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::config::EndpointConfig;
use crate::credentials::CredentialStore;
use crate::download::{DownloadSpec, Downloader};
use crate::error::{CoreError, CoreResult};
use crate::job::JobContext;
use crate::model::{
    InstallStatus, InstalledMod, Instance, LoaderKind, ModFile, ModProvider, ReleaseChannel,
};
use crate::paths::{safe_relative_path, AppPaths};
use crate::store::Store;

pub use curseforge::CurseForge;
pub use modrinth::Modrinth;

pub const MODPACK_SEARCH_BATCH_SIZE: usize = 21;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModProjectType {
    #[default]
    Mod,
    Modpack,
}

impl ModProjectType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Modpack => "modpack",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModProject {
    pub provider: ModProvider,
    pub project_id: String,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub icon_url: Option<String>,
    pub page_url: Option<String>,
    pub downloads: u64,
    pub game_versions: Vec<String>,
    pub loaders: Vec<LoaderKind>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSearchParams {
    pub provider: ModProvider,
    pub query: String,
    pub game_version: Option<String>,
    pub loader: Option<LoaderKind>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub project_type: ModProjectType,
}

#[async_trait]
pub trait ModProviderApi: Send + Sync {
    fn provider(&self) -> ModProvider;
    async fn search(&self, params: &ModSearchParams) -> CoreResult<Vec<ModProject>>;
    async fn files(&self, project_id: &str, instance: &Instance) -> CoreResult<Vec<ModFile>>;
    async fn file(&self, project_id: &str, file_id: &str) -> CoreResult<ModFile>;
    async fn modpack_file(
        &self,
        project_id: &str,
        game_version: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> CoreResult<ModFile>;
}

#[derive(Clone)]
pub struct ModService {
    paths: AppPaths,
    store: Arc<Store>,
    downloader: Downloader,
    modrinth: Arc<Modrinth>,
    curseforge: Arc<CurseForge>,
}

impl ModService {
    pub fn new(
        paths: AppPaths,
        store: Arc<Store>,
        downloader: Downloader,
        endpoints: EndpointConfig,
    ) -> CoreResult<Self> {
        Ok(Self {
            paths,
            store,
            downloader,
            modrinth: Arc::new(Modrinth::new(endpoints.modrinth_api)?),
            curseforge: Arc::new(CurseForge::new(endpoints.curseforge_api)?),
        })
    }

    pub async fn search(&self, params: ModSearchParams) -> CoreResult<Vec<ModProject>> {
        if params.query.chars().count() > 200 {
            return Err(CoreError::InvalidInput(
                "Mod search query is too long".into(),
            ));
        }
        self.provider(params.provider)?.search(&params).await
    }

    pub async fn search_modpacks(
        &self,
        mut params: ModSearchParams,
    ) -> CoreResult<Vec<ModProject>> {
        params.project_type = ModProjectType::Modpack;
        if params.query.chars().count() > 200 {
            return Err(CoreError::InvalidInput(
                "Modpack search query is too long".into(),
            ));
        }
        self.provider(params.provider)?.search(&params).await
    }

    pub async fn modpack_file(
        &self,
        provider: ModProvider,
        project_id: &str,
        game_version: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> CoreResult<ModFile> {
        self.provider(provider)?
            .modpack_file(project_id, game_version, loader)
            .await
    }

    pub async fn files(
        &self,
        provider: ModProvider,
        instance_id: Uuid,
        project_id: &str,
    ) -> CoreResult<Vec<ModFile>> {
        let instance = self.store.instance(instance_id)?;
        let mut files = self
            .provider(provider)?
            .files(project_id, &instance)
            .await?;
        files.retain(|file| compatible(file, &instance));
        files.sort_by(|left, right| {
            channel_rank(left.channel)
                .cmp(&channel_rank(right.channel))
                .then_with(|| right.published_at.cmp(&left.published_at))
        });
        Ok(files)
    }

    pub async fn provider_file(
        &self,
        provider: ModProvider,
        project_id: &str,
        file_id: &str,
    ) -> CoreResult<ModFile> {
        self.provider(provider)?.file(project_id, file_id).await
    }

    pub fn resolve<'a>(
        &'a self,
        instance_id: Uuid,
        file: ModFile,
    ) -> BoxFuture<'a, CoreResult<Vec<ModFile>>> {
        async move {
            let instance = self.store.instance(instance_id)?;
            let mut plan = Vec::new();
            let mut pending = vec![file];
            let mut visited = HashSet::new();
            while let Some(file) = pending.pop() {
                if plan.len() >= 64 {
                    return Err(CoreError::InvalidInput(
                        "Mod dependency plan exceeds 64 files".into(),
                    ));
                }
                if !visited.insert((file.provider, file.project_id.clone(), file.file_id.clone())) {
                    continue;
                }
                if !compatible(&file, &instance) {
                    return Err(CoreError::Conflict(format!(
                        "{} does not support {} / {:?}",
                        file.file_name, instance.game_version, instance.loader
                    )));
                }
                if !file.distribution_allowed || file.url.is_none() {
                    return Err(CoreError::Forbidden(format!(
                        "{} does not allow automatic third-party download",
                        file.file_name
                    )));
                }
                for dependency in &file.dependencies {
                    if !dependency.required
                        || visited
                            .iter()
                            .any(|(_, project, _)| project == &dependency.project_id)
                    {
                        continue;
                    }
                    let provider = self.provider(file.provider)?;
                    let dependency_file = if let Some(version_id) = &dependency.version_id {
                        provider.file(&dependency.project_id, version_id).await?
                    } else {
                        let mut files = provider.files(&dependency.project_id, &instance).await?;
                        files.retain(|candidate| compatible(candidate, &instance));
                        files.sort_by_key(|file| std::cmp::Reverse(file.published_at));
                        files.into_iter().next().ok_or_else(|| {
                            CoreError::NotFound(format!(
                                "compatible dependency {}",
                                dependency.project_id
                            ))
                        })?
                    };
                    pending.push(dependency_file);
                }
                plan.push(file);
            }
            Ok(plan)
        }
        .boxed()
    }

    pub async fn install(
        &self,
        instance_id: Uuid,
        file: ModFile,
        context: &JobContext,
    ) -> CoreResult<Value> {
        let plan = self.resolve(instance_id, file).await?;
        let mut installed = Vec::new();
        for file in plan {
            context.check_cancelled()?;
            installed.push(
                self.install_one(instance_id, file, false, Some(context))
                    .await?,
            );
        }
        serde_json::to_value(installed).map_err(CoreError::from)
    }

    async fn install_one(
        &self,
        instance_id: Uuid,
        file: ModFile,
        replace: bool,
        context: Option<&JobContext>,
    ) -> CoreResult<InstalledMod> {
        let instance = self.store.instance(instance_id)?;
        if instance.install_status != InstallStatus::Installed {
            return Err(CoreError::Conflict(
                "install the instance before installing Mods".into(),
            ));
        }
        validate_mod_filename(&file.file_name)?;
        let existing = self.store.mods(instance_id)?;
        if let Some(existing) = existing.iter().find(|installed| {
            installed.file.provider == file.provider && installed.file.project_id == file.project_id
        }) {
            if existing.file.file_id == file.file_id {
                return Ok(existing.clone());
            }
            if !replace {
                return Err(CoreError::Conflict(format!(
                    "{} is already installed with another version",
                    existing.file.project_name
                )));
            }
        }
        if let Some(conflict) = existing.iter().find(|installed| {
            installed.file.file_name == file.file_name
                && installed.file.file_id != file.file_id
                && !(replace
                    && installed.file.provider == file.provider
                    && installed.file.project_id == file.project_id)
        }) {
            return Err(CoreError::Conflict(format!(
                "Mod filename is already used by {}",
                conflict.file.project_name
            )));
        }
        let cache = self
            .paths
            .content
            .join("mod-files")
            .join(provider_name(file.provider))
            .join(&file.file_id)
            .join(&file.file_name);
        self.downloader
            .download(
                DownloadSpec {
                    url: file.url.clone().ok_or_else(|| {
                        CoreError::Forbidden("Mod has no automatic download URL".into())
                    })?,
                    destination: cache.clone(),
                    size: file.size,
                    sha1: file.sha1.clone(),
                    sha256: file.sha256.clone(),
                    provider: provider_name(file.provider).into(),
                },
                context,
            )
            .await?;
        let destination = self
            .paths
            .instance(instance_id)
            .join("mods")
            .join(&file.file_name);
        let temporary = destination.with_file_name(format!(
            ".{}.{}.installing",
            file.file_name,
            Uuid::new_v4()
        ));
        tokio::fs::copy(&cache, &temporary).await?;
        if destination.exists() && !replace {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(CoreError::AlreadyExists(format!(
                "Mod file {}",
                destination.display()
            )));
        }
        let committed = if replace {
            crate::download::replace_file(temporary.clone(), destination.clone()).await
        } else {
            tokio::fs::rename(&temporary, &destination)
                .await
                .map_err(CoreError::from)
        };
        if let Err(error) = committed {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }
        let installed = InstalledMod {
            id: Uuid::new_v4(),
            instance_id,
            file,
            enabled: true,
            managed: true,
            installed_at: Utc::now(),
        };
        self.store.save_mod(&installed)?;
        if replace {
            for old in existing.iter().filter(|old| {
                old.file.provider == installed.file.provider
                    && old.file.project_id == installed.file.project_id
            }) {
                let old_path = mod_path(&self.paths, old);
                self.store.delete_mod(old.id)?;
                if old_path != destination && old_path.exists() {
                    tokio::fs::remove_file(old_path).await?;
                }
            }
        }
        Ok(installed)
    }

    pub fn list(&self, instance_id: Uuid) -> CoreResult<Vec<InstalledMod>> {
        self.store.instance(instance_id)?;
        self.scan_manual(instance_id)?;
        self.store.mods(instance_id)
    }

    fn scan_manual(&self, instance_id: Uuid) -> CoreResult<()> {
        let instance = self.store.instance(instance_id)?;
        let tracked = self.store.mods(instance_id)?;
        let directory = self.paths.instance(instance_id).join("mods");
        if !directory.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let enabled = !name.ends_with(".disabled");
            let base_name = name.strip_suffix(".disabled").unwrap_or(&name).to_owned();
            if !base_name.ends_with(".jar")
                || tracked
                    .iter()
                    .any(|installed| installed.file.file_name == base_name)
            {
                continue;
            }
            let metadata = entry.metadata()?;
            self.store.save_mod(&InstalledMod {
                id: Uuid::new_v4(),
                instance_id,
                file: ModFile {
                    provider: ModProvider::Manual,
                    project_id: base_name.clone(),
                    file_id: base_name.clone(),
                    project_name: base_name.clone(),
                    version_name: "untracked".into(),
                    file_name: base_name,
                    url: None,
                    size: Some(metadata.len()),
                    sha1: None,
                    sha256: None,
                    game_versions: vec![instance.game_version.clone()],
                    loaders: vec![instance.loader],
                    channel: ReleaseChannel::Release,
                    dependencies: Vec::new(),
                    published_at: None,
                    distribution_allowed: false,
                },
                enabled,
                managed: false,
                installed_at: Utc::now(),
            })?;
        }
        Ok(())
    }

    pub fn set_enabled(&self, id: Uuid, enabled: bool) -> CoreResult<InstalledMod> {
        let mut installed = self.find_mod(id)?;
        if installed.enabled == enabled {
            return Ok(installed);
        }
        let old_path = mod_path(&self.paths, &installed);
        installed.enabled = enabled;
        let new_path = mod_path(&self.paths, &installed);
        if old_path.exists() {
            std::fs::rename(old_path, new_path)?;
        }
        self.store.save_mod(&installed)?;
        Ok(installed)
    }

    pub fn remove(&self, id: Uuid) -> CoreResult<()> {
        let installed = self.find_mod(id)?;
        if !installed.managed {
            return Err(CoreError::Forbidden(
                "manually installed Mods must be removed by the user".into(),
            ));
        }
        let path = mod_path(&self.paths, &installed);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        self.store.delete_mod(id)
    }

    pub async fn update(&self, instance_id: Uuid, context: &JobContext) -> CoreResult<Value> {
        let installed = self.store.mods(instance_id)?;
        let mut results = Vec::new();
        for current in installed {
            context.check_cancelled()?;
            if !current.managed
                || matches!(
                    current.file.provider,
                    ModProvider::Manual | ModProvider::Modpack
                )
            {
                continue;
            }
            let result = async {
                let instance = self.store.instance(instance_id)?;
                let mut files = self
                    .provider(current.file.provider)?
                    .files(&current.file.project_id, &instance)
                    .await?;
                files.retain(|file| compatible(file, &instance));
                files.sort_by_key(|file| std::cmp::Reverse(file.published_at));
                let candidate = files.into_iter().next().ok_or_else(|| {
                    CoreError::NotFound(format!("update for {}", current.file.project_name))
                })?;
                if candidate.file_id == current.file.file_id {
                    return serde_json::to_value(&current).map_err(CoreError::from);
                }
                let updated = self
                    .install_one(instance_id, candidate, true, Some(context))
                    .await?;
                serde_json::to_value(updated).map_err(CoreError::from)
            }
            .await;
            results.push(match result {
                Ok(value) => value,
                Err(error) => serde_json::json!({
                    "id": current.id,
                    "error": error.protocol_error(),
                }),
            });
        }
        Ok(Value::Array(results))
    }

    pub fn set_curseforge_key(&self, key: &str) -> CoreResult<()> {
        if key.trim().is_empty() || key.len() > 500 {
            return Err(CoreError::InvalidInput("invalid CurseForge API key".into()));
        }
        CredentialStore::new().store("provider-curseforge", key)
    }

    pub fn delete_curseforge_key(&self) -> CoreResult<()> {
        CredentialStore::new().delete("provider-curseforge")
    }

    fn find_mod(&self, id: Uuid) -> CoreResult<InstalledMod> {
        for instance in self.store.instances()? {
            if let Some(installed) = self
                .store
                .mods(instance.id)?
                .into_iter()
                .find(|installed| installed.id == id)
            {
                return Ok(installed);
            }
        }
        Err(CoreError::NotFound(format!("Mod {id}")))
    }

    fn provider(&self, provider: ModProvider) -> CoreResult<&dyn ModProviderApi> {
        match provider {
            ModProvider::Modrinth => Ok(self.modrinth.as_ref()),
            ModProvider::CurseForge => Ok(self.curseforge.as_ref()),
            _ => Err(CoreError::InvalidInput("invalid Mod provider".into())),
        }
    }
}

pub fn compatible(file: &ModFile, instance: &Instance) -> bool {
    file.game_versions
        .iter()
        .any(|version| version == &instance.game_version)
        && file.loaders.contains(&instance.loader)
}

pub fn loader_name(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Vanilla => "vanilla",
        LoaderKind::Fabric => "fabric",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
        LoaderKind::Quilt => "quilt",
    }
}

pub fn parse_loader(loader: &str) -> Option<LoaderKind> {
    match loader {
        "fabric" => Some(LoaderKind::Fabric),
        "forge" => Some(LoaderKind::Forge),
        "neoforge" => Some(LoaderKind::NeoForge),
        "quilt" => Some(LoaderKind::Quilt),
        _ => None,
    }
}

fn provider_name(provider: ModProvider) -> &'static str {
    match provider {
        ModProvider::Modrinth => "modrinth",
        ModProvider::CurseForge => "curseforge",
        ModProvider::Ftb => "ftb",
        ModProvider::Manual => "manual",
        ModProvider::Modpack => "modpack",
    }
}

fn channel_rank(channel: ReleaseChannel) -> u8 {
    match channel {
        ReleaseChannel::Release => 0,
        ReleaseChannel::Beta => 1,
        ReleaseChannel::Alpha => 2,
    }
}

fn validate_mod_filename(name: &str) -> CoreResult<()> {
    let path = safe_relative_path(name)?;
    if path.components().count() != 1 || !name.ends_with(".jar") {
        return Err(CoreError::InvalidInput(
            "Mod filename must be a single .jar filename".into(),
        ));
    }
    Ok(())
}

fn mod_path(paths: &AppPaths, installed: &InstalledMod) -> PathBuf {
    let mut path = paths
        .instance(installed.instance_id)
        .join("mods")
        .join(&installed.file.file_name);
    if !installed.enabled {
        path = path.with_extension("jar.disabled");
    }
    path
}

pub(crate) fn client() -> CoreResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("MzLauncher-Core/{}", crate::CORE_VERSION))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| CoreError::Configuration(error.to_string()))
}

pub(crate) async fn send_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
    provider: &str,
) -> CoreResult<T> {
    let response = request
        .send()
        .await
        .map_err(|error| CoreError::Network(error.without_url().to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(CoreError::Upstream {
            provider: provider.into(),
            status: status.as_u16(),
            message: format!("provider returned HTTP {}", status.as_u16()),
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length > 32 * 1024 * 1024)
    {
        return Err(CoreError::Upstream {
            provider: provider.into(),
            status: 413,
            message: "provider response is too large".into(),
        });
    }
    response.json().await.map_err(|error| CoreError::Upstream {
        provider: provider.into(),
        status: 200,
        message: format!("invalid provider response: {error}"),
    })
}

pub fn mod_path_for_test(paths: &AppPaths, installed: &InstalledMod) -> PathBuf {
    mod_path(paths, installed)
}

pub fn path_is_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn replaces_same_filename_mod_after_staging_new_content() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 1024];
            let _ = socket.read(&mut buffer).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nnew")
                .await
                .unwrap();
        });
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(temp.path().join("data"));
        paths.initialize().unwrap();
        let store = Arc::new(Store::open(&paths).unwrap());
        let instance = test_instance();
        store.save_instance(&instance).unwrap();
        let mods = paths.instance(instance.id).join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("example.jar"), b"old").unwrap();
        let old = InstalledMod {
            id: Uuid::new_v4(),
            instance_id: instance.id,
            file: mod_file("old", None),
            enabled: true,
            managed: true,
            installed_at: Utc::now(),
        };
        store.save_mod(&old).unwrap();
        let service = ModService::new(
            paths,
            store.clone(),
            Downloader::new(crate::config::DownloadConfig::default()).unwrap(),
            EndpointConfig::default(),
        )
        .unwrap();
        let updated = service
            .install_one(
                instance.id,
                mod_file("new", Some(format!("http://{address}/example.jar"))),
                true,
                None,
            )
            .await
            .unwrap();
        assert_eq!(updated.file.file_id, "new");
        assert_eq!(
            std::fs::read(service.paths.instance(instance.id).join("mods/example.jar")).unwrap(),
            b"new"
        );
        let records = store.mods(instance.id).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].file.file_id, "new");
        server.await.unwrap();
    }

    fn test_instance() -> Instance {
        Instance {
            id: Uuid::new_v4(),
            name: "test".into(),
            description: None,
            game_version: "1.21.1".into(),
            loader: LoaderKind::Fabric,
            loader_version: Some("0.16.10".into()),
            install_status: InstallStatus::Installed,
            java_path: None,
            min_memory_mb: 512,
            max_memory_mb: 4096,
            jvm_args: vec![],
            game_args: vec![],
            account_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_launched_at: None,
            total_play_time_ms: 0,
        }
    }

    fn mod_file(id: &str, url: Option<String>) -> ModFile {
        ModFile {
            provider: ModProvider::Modrinth,
            project_id: "project".into(),
            file_id: id.into(),
            project_name: "Example".into(),
            version_name: id.into(),
            file_name: "example.jar".into(),
            url,
            size: Some(3),
            sha1: None,
            sha256: None,
            game_versions: vec!["1.21.1".into()],
            loaders: vec![LoaderKind::Fabric],
            channel: ReleaseChannel::Release,
            dependencies: vec![],
            published_at: None,
            distribution_allowed: true,
        }
    }
}
