use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::config::EndpointConfig;
use crate::download::Downloader;
use crate::error::{CoreError, CoreResult};
use crate::java::JavaManager;
use crate::job::JobContext;
use crate::loader::LoaderService;
use crate::model::{CreateInstanceParams, InstallStatus, Instance, LoaderKind};
use crate::paths::AppPaths;
use crate::store::Store;
use crate::version::VersionService;

#[derive(Clone)]
pub struct InstanceService {
    paths: AppPaths,
    store: Arc<Store>,
    versions: VersionService,
    java: JavaManager,
    loaders: LoaderService,
}

impl InstanceService {
    pub fn new(
        paths: AppPaths,
        store: Arc<Store>,
        downloader: Downloader,
        endpoints: EndpointConfig,
    ) -> Self {
        let versions = VersionService::new(paths.clone(), downloader.clone(), endpoints.clone());
        let java = JavaManager::new(paths.clone(), downloader.clone(), endpoints.clone());
        let loaders = LoaderService::new(paths.clone(), downloader, endpoints, java.clone());
        Self {
            paths,
            store,
            versions,
            java,
            loaders,
        }
    }

    pub fn versions(&self) -> &VersionService {
        &self.versions
    }

    pub fn java(&self) -> &JavaManager {
        &self.java
    }

    pub fn loaders(&self) -> &LoaderService {
        &self.loaders
    }

    pub fn create(&self, params: CreateInstanceParams) -> CoreResult<Instance> {
        validate_instance_params(&params)?;
        if params.loader != LoaderKind::Vanilla && params.loader_version.is_none() {
            return Err(CoreError::InvalidInput(
                "a loader version is required for modded instances".into(),
            ));
        }
        let now = Utc::now();
        let instance = Instance {
            id: Uuid::new_v4(),
            name: params.name,
            description: params.description,
            game_version: params.game_version,
            loader: params.loader,
            loader_version: params.loader_version,
            install_status: InstallStatus::NotInstalled,
            java_path: params.java_path,
            min_memory_mb: params.min_memory_mb,
            max_memory_mb: params.max_memory_mb,
            jvm_args: params.jvm_args,
            game_args: params.game_args,
            account_id: params.account_id,
            created_at: now,
            updated_at: now,
            last_launched_at: None,
        };
        let root = self.paths.instance(instance.id);
        for directory in [
            "",
            "mods",
            "config",
            "saves",
            "screenshots",
            "resourcepacks",
            "shaderpacks",
            "logs",
            "crash-reports",
            "natives",
        ] {
            std::fs::create_dir_all(root.join(directory))?;
        }
        self.store.save_instance(&instance)?;
        Ok(instance)
    }

    pub fn get(&self, id: Uuid) -> CoreResult<Instance> {
        self.store.instance(id)
    }

    pub fn list(&self) -> CoreResult<Vec<Instance>> {
        self.store.instances()
    }

    pub fn rename(&self, id: Uuid, name: String) -> CoreResult<Instance> {
        validate_name(&name)?;
        let mut instance = self.store.instance(id)?;
        instance.name = name;
        instance.updated_at = Utc::now();
        self.store.save_instance(&instance)?;
        Ok(instance)
    }

    pub fn update_settings(&self, mut instance: Instance) -> CoreResult<Instance> {
        let current = self.store.instance(instance.id)?;
        validate_name(&instance.name)?;
        validate_memory(instance.min_memory_mb, instance.max_memory_mb)?;
        validate_arguments(&instance.jvm_args)?;
        validate_arguments(&instance.game_args)?;
        if instance.game_version.is_empty()
            || instance.game_version.contains(['/', '\\', '\0'])
            || (instance.loader != LoaderKind::Vanilla && instance.loader_version.is_none())
        {
            return Err(CoreError::InvalidInput(
                "invalid game version or loader settings".into(),
            ));
        }
        let version_changed = current.game_version != instance.game_version
            || current.loader != instance.loader
            || current.loader_version != instance.loader_version;
        instance.created_at = current.created_at;
        instance.install_status = if version_changed {
            InstallStatus::NotInstalled
        } else {
            current.install_status
        };
        instance.last_launched_at = current.last_launched_at;
        instance.updated_at = Utc::now();
        self.store.save_instance(&instance)?;
        Ok(instance)
    }

    pub async fn copy(&self, id: Uuid, name: String) -> CoreResult<Instance> {
        validate_name(&name)?;
        let source = self.store.instance(id)?;
        let mut copied = source;
        copied.id = Uuid::new_v4();
        copied.name = name;
        copied.created_at = Utc::now();
        copied.updated_at = copied.created_at;
        copied.last_launched_at = None;
        let source_path = self.paths.instance(id);
        let temporary = self.paths.instances.join(format!(".copy-{}", copied.id));
        let destination = self.paths.instance(copied.id);
        let source_for_task = source_path.clone();
        let temporary_for_task = temporary.clone();
        tokio::task::spawn_blocking(move || copy_tree(&source_for_task, &temporary_for_task))
            .await
            .map_err(|error| CoreError::Internal(error.to_string()))??;
        if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
            let _ = tokio::fs::remove_dir_all(&temporary).await;
            return Err(error.into());
        }
        self.store.save_instance(&copied)?;
        Ok(copied)
    }

    pub async fn delete(&self, id: Uuid) -> CoreResult<()> {
        self.store.instance(id)?;
        self.store.delete_instance(id)?;
        let path = self.paths.instance(id);
        if path.exists() {
            tokio::fs::remove_dir_all(path).await?;
        }
        Ok(())
    }

    pub fn validate(&self, id: Uuid) -> CoreResult<Value> {
        let instance = self.store.instance(id)?;
        let root = self.paths.instance(id);
        let client = self
            .paths
            .content
            .join("versions")
            .join(&instance.game_version)
            .join(format!("{}.jar", instance.game_version));
        let mut issues = Vec::new();
        if !root.is_dir() {
            issues.push(json!({"code": "instance_directory_missing", "message": "instance directory is missing"}));
        }
        if instance.install_status == InstallStatus::Installed && !client.is_file() {
            issues.push(json!({"code": "client_missing", "message": "game client is missing"}));
        }
        if instance.install_status != InstallStatus::Installed {
            issues.push(json!({"code": "not_installed", "message": "game files are not completely installed"}));
        }
        Ok(json!({
            "instanceId": id,
            "valid": issues.is_empty(),
            "issues": issues,
        }))
    }

    pub fn set_install_status(&self, id: Uuid, status: InstallStatus) -> CoreResult<()> {
        let mut instance = self.store.instance(id)?;
        instance.install_status = status;
        instance.updated_at = Utc::now();
        self.store.save_instance(&instance)
    }

    pub async fn install(&self, id: Uuid, context: &JobContext) -> CoreResult<Value> {
        let mut instance = self.store.instance(id)?;
        instance.install_status = InstallStatus::Installing;
        instance.updated_at = Utc::now();
        self.store.save_instance(&instance)?;
        context.checkpoint(json!({"stage": "installing", "instanceId": id}))?;
        let result = async {
            let base = self.versions.merged(&instance.game_version).await?;
            let java_requirement = base
                .java_version
                .unwrap_or(crate::version::JavaRequirement {
                    component: Some("jre-legacy".into()),
                    major_version: 8,
                });
            self.java
                .ensure(&java_requirement, instance.java_path.as_deref(), context)
                .await?;
            let profile = self.loaders.prepare(&instance, context).await?;
            let mut result = self.versions.install(&instance, context).await?;
            result["loaderProfile"] = serde_json::to_value(profile)?;
            let requirement = crate::java::java_from_value(&result["javaVersion"])?;
            let java = self
                .java
                .ensure(&requirement, instance.java_path.as_deref(), context)
                .await?;
            result["java"] = serde_json::to_value(java)?;
            Ok(result)
        }
        .await;
        let mut instance = self.store.instance(id)?;
        instance.install_status = if result.is_ok() {
            InstallStatus::Installed
        } else {
            InstallStatus::Incomplete
        };
        instance.updated_at = Utc::now();
        self.store.save_instance(&instance)?;
        result
    }
}

fn validate_instance_params(params: &CreateInstanceParams) -> CoreResult<()> {
    validate_name(&params.name)?;
    if params.game_version.is_empty()
        || params.game_version.len() > 100
        || params.game_version.contains(['/', '\\', '\0'])
    {
        return Err(CoreError::InvalidInput("invalid game version".into()));
    }
    validate_memory(params.min_memory_mb, params.max_memory_mb)?;
    validate_arguments(&params.jvm_args)?;
    validate_arguments(&params.game_args)
}

fn validate_name(name: &str) -> CoreResult<()> {
    if name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(CoreError::InvalidInput(
            "instance name must contain 1 to 80 printable characters".into(),
        ));
    }
    Ok(())
}

fn validate_memory(minimum: u32, maximum: u32) -> CoreResult<()> {
    if !(256..=131_072).contains(&minimum)
        || !(256..=131_072).contains(&maximum)
        || minimum > maximum
    {
        return Err(CoreError::InvalidInput(
            "memory must be between 256 and 131072 MiB, and minimum cannot exceed maximum".into(),
        ));
    }
    Ok(())
}

fn validate_arguments(arguments: &[String]) -> CoreResult<()> {
    if arguments.len() > 128
        || arguments
            .iter()
            .any(|argument| argument.is_empty() || argument.len() > 4096 || argument.contains('\0'))
    {
        return Err(CoreError::InvalidInput(
            "invalid Java or game argument list".into(),
        ));
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> CoreResult<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(CoreError::Storage(format!(
            "cannot copy a symlink from an instance: {}",
            source.display()
        )));
    }
    if metadata.is_dir() {
        std::fs::create_dir_all(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

pub fn instance_root(paths: &AppPaths, id: Uuid) -> PathBuf {
    paths.instance(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DownloadConfig;

    fn service() -> (tempfile::TempDir, InstanceService) {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(temp.path().join("data"));
        paths.initialize().unwrap();
        let store = Arc::new(Store::open(&paths).unwrap());
        let downloader = Downloader::new(DownloadConfig::default()).unwrap();
        let service = InstanceService::new(paths, store, downloader, EndpointConfig::default());
        (temp, service)
    }

    #[test]
    fn creates_isolated_instance_directories() {
        let (_temp, service) = service();
        let first = service.create(params("first")).unwrap();
        let second = service.create(params("second")).unwrap();
        assert_ne!(first.id, second.id);
        assert!(service.paths.instance(first.id).join("saves").is_dir());
        assert!(service.paths.instance(second.id).join("mods").is_dir());
    }

    #[test]
    fn rejects_invalid_memory_and_arguments() {
        let (_temp, service) = service();
        let mut params = params("invalid");
        params.min_memory_mb = 8192;
        params.max_memory_mb = 1024;
        assert!(service.create(params).is_err());
    }

    fn params(name: &str) -> CreateInstanceParams {
        CreateInstanceParams {
            name: name.into(),
            description: None,
            game_version: "1.21.1".into(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
            java_path: None,
            min_memory_mb: 512,
            max_memory_mb: 4096,
            jvm_args: vec![],
            game_args: vec![],
            account_id: None,
        }
    }
}
