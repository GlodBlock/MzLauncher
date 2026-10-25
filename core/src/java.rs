use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::EndpointConfig;
use crate::download::{DownloadSpec, Downloader};
use crate::error::{CoreError, CoreResult};
use crate::job::JobContext;
use crate::model::JobProgress;
use crate::paths::{safe_join, safe_relative_path, AppPaths};
use crate::version::{JavaRequirement, RemoteFile};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInfo {
    pub path: PathBuf,
    pub major_version: u32,
    pub architecture: String,
    pub managed: bool,
    pub component: Option<String>,
}

#[derive(Clone)]
pub struct JavaManager {
    paths: AppPaths,
    downloader: Downloader,
    endpoints: EndpointConfig,
}

#[derive(Debug, Deserialize)]
struct RuntimeChannel {
    #[serde(flatten)]
    components: HashMap<String, RuntimeProduct>,
}

#[derive(Debug, Deserialize)]
struct RuntimeProduct {
    manifest: RemoteFile,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeEntry>,
}

#[derive(Debug, Deserialize)]
struct RuntimeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    downloads: Option<RuntimeDownloads>,
    #[serde(default)]
    executable: bool,
}

#[derive(Debug, Deserialize)]
struct RuntimeDownloads {
    raw: RemoteFile,
}

impl JavaManager {
    pub fn new(paths: AppPaths, downloader: Downloader, endpoints: EndpointConfig) -> Self {
        Self {
            paths,
            downloader,
            endpoints,
        }
    }

    pub async fn discover(&self) -> Vec<JavaInfo> {
        let mut candidates = Vec::new();
        if let Some(java_home) = std::env::var_os("JAVA_HOME") {
            candidates.push(PathBuf::from(java_home).join("bin").join(java_executable()));
        }
        if let Some(path) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&path) {
                candidates.push(directory.join(java_executable()));
            }
        }
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for candidate in candidates {
            if seen.insert(candidate.clone()) && candidate.is_file() {
                if let Ok(info) = self.validate(&candidate).await {
                    result.push(info);
                }
            }
        }
        result
    }

    pub async fn validate(&self, path: &Path) -> CoreResult<JavaInfo> {
        if !path.is_file() {
            return Err(CoreError::NotFound(format!(
                "Java executable {}",
                path.display()
            )));
        }
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new(path)
                .arg("-XshowSettings:properties")
                .arg("-version")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| CoreError::InvalidInput("Java version check timed out".into()))?
        .map_err(CoreError::Io)?;
        if !output.status.success() {
            return Err(CoreError::InvalidInput(format!(
                "Java executable failed its version check: {}",
                path.display()
            )));
        }
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let specification = property(&text, "java.specification.version");
        let version = property(&text, "java.version");
        let version = specification.or(version).ok_or_else(|| {
            CoreError::InvalidInput("could not determine Java major version".into())
        })?;
        let major_version = parse_major_version(&version)?;
        let architecture = property(&text, "os.arch").unwrap_or_else(|| "unknown".into());
        Ok(JavaInfo {
            path: path.to_path_buf(),
            major_version,
            architecture,
            managed: false,
            component: None,
        })
    }

    pub async fn select(
        &self,
        requirement: &JavaRequirement,
        preferred: Option<&str>,
    ) -> CoreResult<JavaInfo> {
        if let Some(preferred) = preferred {
            let info = self.validate(Path::new(preferred)).await?;
            return ensure_compatible(info, requirement);
        }
        if let Some(component) = &requirement.component {
            let managed = self
                .paths
                .runtimes
                .join(component)
                .join("bin")
                .join(java_executable());
            if managed.is_file() {
                let mut info = ensure_compatible(self.validate(&managed).await?, requirement)?;
                info.managed = true;
                info.component = Some(component.clone());
                return Ok(info);
            }
        }
        for info in self.discover().await {
            if let Ok(info) = ensure_compatible(info, requirement) {
                return Ok(info);
            }
        }
        Err(CoreError::NotFound(format!(
            "a compatible Java {} runtime",
            requirement.major_version
        )))
    }

    pub async fn ensure(
        &self,
        requirement: &JavaRequirement,
        preferred: Option<&str>,
        context: &JobContext,
    ) -> CoreResult<JavaInfo> {
        if let Ok(info) = self.select(requirement, preferred).await {
            return Ok(info);
        }
        if preferred.is_some() {
            return self.select(requirement, preferred).await;
        }
        let component = requirement.component.clone().ok_or_else(|| {
            CoreError::Unsupported(format!(
                "no Mojang runtime component is known for Java {}",
                requirement.major_version
            ))
        })?;
        self.install_runtime(&component, requirement.major_version, context)
            .await
    }

    async fn install_runtime(
        &self,
        component: &str,
        required_major: u32,
        context: &JobContext,
    ) -> CoreResult<JavaInfo> {
        safe_relative_path(component)?;
        let products: HashMap<String, RuntimeChannel> = self
            .downloader
            .fetch_json(&self.endpoints.java_runtime_manifest, "mojang")
            .await?;
        let product = products
            .get(runtime_platform())
            .and_then(|channel| channel.components.get(component))
            .ok_or_else(|| {
                CoreError::NotFound(format!(
                    "Mojang runtime {component} for {}",
                    runtime_platform()
                ))
            })?;
        let root = safe_join(&self.paths.runtimes, component)?;
        let manifest_path = root.join("runtime.json");
        self.downloader
            .download(
                DownloadSpec {
                    url: product.manifest.url.clone(),
                    destination: manifest_path.clone(),
                    size: product.manifest.size,
                    sha1: product.manifest.sha1.clone(),
                    sha256: None,
                    provider: "mojang".into(),
                },
                Some(context),
            )
            .await?;
        let manifest: RuntimeManifest =
            serde_json::from_slice(&tokio::fs::read(manifest_path).await?)?;
        let mut specs = Vec::new();
        let mut executables = Vec::new();
        for (name, entry) in &manifest.files {
            let relative = safe_relative_path(name)?;
            if entry.entry_type == "link" {
                return Err(CoreError::Unsupported(
                    "linked Java runtime entries are not supported on this platform".into(),
                ));
            }
            if entry.entry_type != "file" {
                continue;
            }
            let raw = &entry
                .downloads
                .as_ref()
                .ok_or_else(|| CoreError::Upstream {
                    provider: "mojang".into(),
                    status: 200,
                    message: format!("runtime file {name} has no downloads"),
                })?
                .raw;
            let destination = root.join(relative);
            if entry.executable {
                executables.push(destination.clone());
            }
            specs.push(DownloadSpec {
                url: raw.url.clone(),
                destination,
                size: raw.size,
                sha1: raw.sha1.clone(),
                sha256: None,
                provider: "mojang".into(),
            });
        }
        let total = specs.len() as u64;
        let completed = Arc::new(AtomicU64::new(0));
        let downloader = self.downloader.clone();
        stream::iter(specs.into_iter().map(Ok::<_, CoreError>))
            .try_for_each_concurrent(4, |spec| {
                let downloader = downloader.clone();
                let completed = completed.clone();
                async move {
                    context.check_cancelled()?;
                    downloader.download(spec, None).await?;
                    let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    if done % 8 == 0 || done == total {
                        context.progress(JobProgress {
                            stage: "installing_java".into(),
                            completed: done,
                            total: Some(total),
                            current_item: None,
                        })?;
                    }
                    Ok(())
                }
            })
            .await?;
        set_executable_permissions(&executables)?;
        let java = root.join("bin").join(java_executable());
        let mut info = self.validate(&java).await?;
        if info.major_version < required_major {
            return Err(CoreError::Upstream {
                provider: "mojang".into(),
                status: 200,
                message: format!(
                    "runtime {component} provides Java {}, expected at least {required_major}",
                    info.major_version
                ),
            });
        }
        info.managed = true;
        info.component = Some(component.into());
        Ok(info)
    }
}

fn ensure_compatible(info: JavaInfo, requirement: &JavaRequirement) -> CoreResult<JavaInfo> {
    if info.major_version < requirement.major_version {
        return Err(CoreError::InvalidInput(format!(
            "Java {} is too old; Java {} or newer is required",
            info.major_version, requirement.major_version
        )));
    }
    if !architecture_matches(&info.architecture) {
        return Err(CoreError::InvalidInput(format!(
            "Java architecture {} does not match {}",
            info.architecture,
            std::env::consts::ARCH
        )));
    }
    Ok(info)
}

fn property(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.trim().split_once('=')?;
        if key.trim() == name {
            Some(value.trim().trim_matches('"').to_owned())
        } else {
            None
        }
    })
}

fn parse_major_version(version: &str) -> CoreResult<u32> {
    let mut parts = version.split(['.', '-', '_']);
    let first = parts.next().unwrap_or_default();
    let major = if first == "1" {
        parts.next().unwrap_or_default()
    } else {
        first
    };
    major
        .parse()
        .map_err(|_| CoreError::InvalidInput(format!("unsupported Java version string: {version}")))
}

fn architecture_matches(architecture: &str) -> bool {
    match std::env::consts::ARCH {
        "x86_64" => matches!(architecture, "amd64" | "x86_64"),
        "x86" => matches!(architecture, "x86" | "i386" | "i686"),
        "aarch64" => matches!(architecture, "aarch64" | "arm64"),
        _ => true,
    }
}

fn runtime_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "x86") => "windows-x86",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        ("linux", "x86") => "linux-i386",
        _ => "linux",
    }
}

fn java_executable() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

#[cfg(unix)]
fn set_executable_permissions(paths: &[PathBuf]) -> CoreResult<()> {
    use std::os::unix::fs::PermissionsExt;
    for path in paths {
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_executable_permissions(_paths: &[PathBuf]) -> CoreResult<()> {
    Ok(())
}

pub fn java_from_value(value: &Value) -> CoreResult<JavaRequirement> {
    serde_json::from_value(value.clone()).map_err(CoreError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_and_modern_versions() {
        assert_eq!(parse_major_version("1.8.0_402").unwrap(), 8);
        assert_eq!(parse_major_version("17.0.10").unwrap(), 17);
        assert_eq!(parse_major_version("21").unwrap(), 21);
    }

    #[test]
    fn parses_java_settings() {
        let text = "Property settings:\n    java.specification.version = 21\n    os.arch = amd64\n";
        assert_eq!(
            property(text, "java.specification.version").as_deref(),
            Some("21")
        );
        assert_eq!(property(text, "os.arch").as_deref(), Some("amd64"));
    }
}
