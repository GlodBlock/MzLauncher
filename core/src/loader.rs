use std::path::{Path, PathBuf};
use std::time::Duration;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::config::EndpointConfig;
use crate::download::{DownloadSpec, Downloader};
use crate::error::{CoreError, CoreResult};
use crate::java::JavaManager;
use crate::job::JobContext;
use crate::model::{Instance, LoaderKind};
use crate::paths::{atomic_write, safe_relative_path, AppPaths};
use crate::version::{loader_profile_id, JavaRequirement, VersionDetails};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Clone)]
pub struct LoaderService {
    paths: AppPaths,
    downloader: Downloader,
    endpoints: EndpointConfig,
    java: JavaManager,
}

impl LoaderService {
    pub fn new(
        paths: AppPaths,
        downloader: Downloader,
        endpoints: EndpointConfig,
        java: JavaManager,
    ) -> Self {
        Self {
            paths,
            downloader,
            endpoints,
            java,
        }
    }

    pub async fn list(
        &self,
        game_version: &str,
        loader: LoaderKind,
    ) -> CoreResult<Vec<LoaderVersion>> {
        match loader {
            LoaderKind::Vanilla => Ok(Vec::new()),
            LoaderKind::Fabric => {
                let url = format!(
                    "{}/v2/versions/loader/{game_version}",
                    self.endpoints.fabric_meta.trim_end_matches('/')
                );
                let entries: Vec<Value> = self.downloader.fetch_json(&url, "fabric").await?;
                Ok(entries
                    .into_iter()
                    .filter_map(|entry| {
                        Some(LoaderVersion {
                            version: entry.get("loader")?.get("version")?.as_str()?.to_owned(),
                            stable: entry
                                .get("loader")?
                                .get("stable")?
                                .as_bool()
                                .unwrap_or(false),
                        })
                    })
                    .collect())
            }
            LoaderKind::Forge | LoaderKind::NeoForge => {
                let base = self.maven_base(loader);
                let metadata = self
                    .downloader
                    .fetch_text(&format!("{base}/maven-metadata.xml"), loader_name(loader))
                    .await?;
                let versions = maven_versions(&metadata)?;
                Ok(versions
                    .into_iter()
                    .filter(|version| {
                        loader == LoaderKind::NeoForge
                            || version.starts_with(&format!("{game_version}-"))
                    })
                    .map(|version| LoaderVersion {
                        stable: !version.contains('-') || loader == LoaderKind::Forge,
                        version,
                    })
                    .collect())
            }
            LoaderKind::Quilt => Err(CoreError::Unsupported(
                "Quilt is not enabled in this core version".into(),
            )),
        }
    }

    pub async fn prepare(
        &self,
        instance: &Instance,
        java_requirement: &JavaRequirement,
        context: &JobContext,
    ) -> CoreResult<String> {
        match instance.loader {
            LoaderKind::Vanilla => Ok(instance.game_version.clone()),
            LoaderKind::Fabric => self.prepare_fabric(instance).await,
            LoaderKind::Forge | LoaderKind::NeoForge => {
                self.prepare_installer(instance, java_requirement, context)
                    .await
            }
            LoaderKind::Quilt => Err(CoreError::Unsupported(
                "Quilt is not enabled in this core version".into(),
            )),
        }
    }

    async fn prepare_fabric(&self, instance: &Instance) -> CoreResult<String> {
        let loader_version = required_loader_version(instance)?;
        let url = format!(
            "{}/v2/versions/loader/{}/{}/profile/json",
            self.endpoints.fabric_meta.trim_end_matches('/'),
            instance.game_version,
            loader_version
        );
        let temporary = self.paths.cache.join(format!(
            "fabric-{}-{loader_version}.json",
            instance.game_version
        ));
        self.downloader
            .download(
                DownloadSpec {
                    url,
                    destination: temporary.clone(),
                    size: None,
                    sha1: None,
                    sha256: None,
                    provider: "fabric".into(),
                },
                None,
            )
            .await?;
        let details: VersionDetails = serde_json::from_slice(&tokio::fs::read(&temporary).await?)?;
        if details.id.is_empty() || details.main_class.is_none() {
            return Err(CoreError::Upstream {
                provider: "fabric".into(),
                status: 200,
                message: "Fabric returned an incomplete launch profile".into(),
            });
        }
        self.save_profile(&details, instance).await?;
        Ok(details.id)
    }

    async fn prepare_installer(
        &self,
        instance: &Instance,
        java_requirement: &JavaRequirement,
        context: &JobContext,
    ) -> CoreResult<String> {
        let loader_version = required_loader_version(instance)?;
        let base = self.maven_base(instance.loader);
        let (artifact, installer_arguments) = match instance.loader {
            LoaderKind::Forge => (
                format!("forge-{loader_version}-installer.jar"),
                vec!["--installClient".to_owned()],
            ),
            LoaderKind::NeoForge => (
                format!("neoforge-{loader_version}-installer.jar"),
                vec!["--install-client".to_owned()],
            ),
            _ => return Err(CoreError::Unsupported("unsupported installer".into())),
        };
        let url = format!("{base}/{loader_version}/{artifact}");
        let work = self
            .paths
            .temp
            .join(format!("loader-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&work).await?;
        atomic_write(&work.join("launcher_profiles.json"), br#"{"profiles":{}}"#)?;
        let installer = work.join("installer.jar");
        self.downloader
            .download(
                DownloadSpec {
                    url,
                    destination: installer.clone(),
                    size: None,
                    sha1: None,
                    sha256: None,
                    provider: loader_name(instance.loader).into(),
                },
                Some(context),
            )
            .await?;
        let java = self
            .java
            .select(java_requirement, instance.java_path.as_deref())
            .await?;
        let mut command = tokio::process::Command::new(java.path);
        command
            .arg("-jar")
            .arg(&installer)
            .args(installer_arguments)
            .arg(&work)
            .current_dir(&work)
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(600), async {
            tokio::select! {
                output = command.output() => output.map_err(CoreError::from),
                _ = context.cancelled() => Err(CoreError::Cancelled),
            }
        })
        .await
        .map_err(|_| CoreError::Network("mod loader installer timed out".into()))??;
        if !output.status.success() {
            tracing::warn!(
                exit_code = output.status.code(),
                stdout = %String::from_utf8_lossy(&output.stdout),
                stderr = %String::from_utf8_lossy(&output.stderr),
                "mod loader installer failed"
            );
            let _ = tokio::fs::remove_dir_all(&work).await;
            return Err(CoreError::Upstream {
                provider: loader_name(instance.loader).into(),
                status: output.status.code().unwrap_or(1) as u16,
                message: "mod loader installer failed; see core logs for the exit status".into(),
            });
        }
        let work_for_scan = work.clone();
        let game_version = instance.game_version.clone();
        let details = tokio::task::spawn_blocking(move || {
            load_installer_profile(&work_for_scan, &game_version)
        })
        .await
        .map_err(|error| CoreError::Internal(error.to_string()))??;
        self.save_profile(&details, instance).await?;
        let generated_libraries = work.join("libraries");
        if generated_libraries.is_dir() {
            let destination = self.paths.content.join("libraries");
            tokio::task::spawn_blocking(move || {
                copy_generated_libraries(&generated_libraries, &destination)
            })
            .await
            .map_err(|error| CoreError::Internal(error.to_string()))??;
        }
        let _ = tokio::fs::remove_dir_all(&work).await;
        Ok(details.id)
    }

    async fn save_profile(&self, details: &VersionDetails, instance: &Instance) -> CoreResult<()> {
        safe_relative_path(&details.id)?;
        let bytes = serde_json::to_vec_pretty(details)?;
        let directory = self.paths.content.join("versions").join(&details.id);
        tokio::fs::create_dir_all(&directory).await?;
        atomic_write(&directory.join(format!("{}.json", details.id)), &bytes)?;
        let alias = loader_profile_id(instance);
        if alias != details.id {
            safe_relative_path(&alias)?;
            let alias_directory = self.paths.content.join("versions").join(&alias);
            tokio::fs::create_dir_all(&alias_directory).await?;
            atomic_write(&alias_directory.join(format!("{alias}.json")), &bytes)?;
        }
        Ok(())
    }

    fn maven_base(&self, loader: LoaderKind) -> &str {
        match loader {
            LoaderKind::Forge => self.endpoints.forge_maven.trim_end_matches('/'),
            LoaderKind::NeoForge => self.endpoints.neoforge_maven.trim_end_matches('/'),
            _ => "",
        }
    }
}

fn required_loader_version(instance: &Instance) -> CoreResult<&str> {
    instance
        .loader_version
        .as_deref()
        .filter(|version| !version.is_empty() && safe_relative_path(version).is_ok())
        .ok_or_else(|| CoreError::InvalidInput("loader version is required".into()))
}

fn loader_name(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Fabric => "fabric",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Vanilla => "vanilla",
    }
}

fn maven_versions(metadata: &str) -> CoreResult<Vec<String>> {
    let expression = Regex::new(r"(?s)<version>\s*([^<\s]+)\s*</version>")
        .map_err(|error| CoreError::Internal(error.to_string()))?;
    let versions = expression
        .captures_iter(metadata)
        .filter_map(|capture| capture.get(1).map(|value| value.as_str().to_owned()))
        .collect::<Vec<_>>();
    if versions.is_empty() {
        return Err(CoreError::InvalidInput(
            "Maven metadata contains no versions".into(),
        ));
    }
    Ok(versions)
}

fn find_profiles(root: &Path) -> CoreResult<Vec<PathBuf>> {
    let mut profiles = Vec::new();
    let versions = root.join("versions");
    if !versions.is_dir() {
        return Ok(profiles);
    }
    for profile in std::fs::read_dir(versions)? {
        let profile = profile?;
        if !profile.file_type()?.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(profile.path())? {
            let file = file?;
            if file
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                profiles.push(file.path());
            }
        }
    }
    profiles.sort();
    Ok(profiles)
}

fn load_installer_profile(root: &Path, game_version: &str) -> CoreResult<VersionDetails> {
    for profile in find_profiles(root)? {
        let details: VersionDetails = serde_json::from_slice(&std::fs::read(profile)?)?;
        if details.id != game_version && details.inherits_from.as_deref() == Some(game_version) {
            return Ok(details);
        }
    }
    Err(CoreError::InvalidInput(
        "installer did not generate a client launch profile for the game version".into(),
    ))
}

fn copy_generated_libraries(source: &Path, destination: &Path) -> CoreResult<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(CoreError::InvalidInput(
                "installer generated a symbolic link in libraries".into(),
            ));
        }
        let output = destination.join(entry.file_name());
        if file_type.is_dir() {
            std::fs::create_dir_all(&output)?;
            copy_generated_libraries(&entry.path(), &output)?;
        } else if file_type.is_file() && !output.exists() {
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), output)?;
        }
    }
    Ok(())
}

pub fn expected_profile_id(instance: &Instance) -> String {
    loader_profile_id(instance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_maven_version_list() {
        let xml = "<metadata><versioning><versions><version>1.21.1-52.0.0</version><version>1.21.1-52.0.1</version></versions></versioning></metadata>";
        assert_eq!(maven_versions(xml).unwrap().len(), 2);
    }

    #[test]
    fn finds_standard_installer_profile_layout() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("versions").join("example-profile");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("example-profile.json"), b"{}").unwrap();
        let profiles = find_profiles(temp.path()).unwrap();
        assert_eq!(profiles, vec![directory.join("example-profile.json")]);
    }

    #[test]
    fn selects_loader_profile_instead_of_vanilla_profile() {
        let temp = tempfile::tempdir().unwrap();
        for (id, contents) in [
            ("1.21.1", r#"{"id":"1.21.1"}"#),
            (
                "neoforge-21.1.265",
                r#"{"id":"neoforge-21.1.265","inheritsFrom":"1.21.1"}"#,
            ),
        ] {
            let directory = temp.path().join("versions").join(id);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("{id}.json")), contents).unwrap();
        }
        let details = load_installer_profile(temp.path(), "1.21.1").unwrap();
        assert_eq!(details.id, "neoforge-21.1.265");
        assert_eq!(details.inherits_from.as_deref(), Some("1.21.1"));
    }
}
