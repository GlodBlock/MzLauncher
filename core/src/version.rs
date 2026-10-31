use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::EndpointConfig;
use crate::download::{DownloadSpec, Downloader};
use crate::error::{CoreError, CoreResult};
use crate::job::JobContext;
use crate::model::{Instance, JobProgress};
use crate::paths::{safe_join, safe_relative_path, AppPaths};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: String,
    pub url: String,
    pub sha1: Option<String>,
    pub release_time: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct VersionManifest {
    versions: Vec<VersionSummary>,
}

type ManifestCache =
    Arc<tokio::sync::RwLock<Option<(chrono::DateTime<chrono::Utc>, VersionManifest)>>>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    pub url: String,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexReference {
    pub id: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub total_size: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRequirement {
    pub component: Option<String>,
    pub major_version: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValues,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValues {
    One(String),
    Many(Vec<String>),
}

impl ArgumentValues {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionArguments {
    #[serde(default)]
    pub game: Vec<ArgumentValue>,
    #[serde(default)]
    pub jvm: Vec<ArgumentValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRules {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDownloads {
    pub artifact: Option<RemoteFile>,
    #[serde(default)]
    pub classifiers: HashMap<String, RemoteFile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: LibraryDownloads,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub natives: HashMap<String, String>,
    pub extract: Option<ExtractRules>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDetails {
    pub id: String,
    pub inherits_from: Option<String>,
    pub main_class: Option<String>,
    pub assets: Option<String>,
    pub asset_index: Option<AssetIndexReference>,
    #[serde(default)]
    pub downloads: HashMap<String, RemoteFile>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    pub arguments: Option<VersionArguments>,
    pub minecraft_arguments: Option<String>,
    pub java_version: Option<JavaRequirement>,
    #[serde(rename = "type")]
    pub release_type: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MergedVersion {
    pub id: String,
    pub main_class: String,
    pub assets: String,
    pub asset_index: Option<AssetIndexReference>,
    pub downloads: HashMap<String, RemoteFile>,
    pub libraries: Vec<Library>,
    pub arguments: Option<VersionArguments>,
    pub minecraft_arguments: Option<String>,
    pub java_version: Option<JavaRequirement>,
    pub release_type: String,
}

#[derive(Clone)]
pub struct VersionService {
    paths: AppPaths,
    downloader: Downloader,
    endpoints: EndpointConfig,
    manifest_cache: ManifestCache,
}

#[derive(Debug, Deserialize)]
struct AssetIndex {
    objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

impl VersionService {
    pub fn new(paths: AppPaths, downloader: Downloader, endpoints: EndpointConfig) -> Self {
        Self {
            paths,
            downloader,
            endpoints,
            manifest_cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    async fn manifest(&self) -> CoreResult<VersionManifest> {
        if let Some((fetched_at, manifest)) = &*self.manifest_cache.read().await {
            if chrono::Utc::now() - *fetched_at < chrono::Duration::minutes(5) {
                return Ok(manifest.clone());
            }
        }
        match self
            .downloader
            .fetch_json::<VersionManifest>(&self.endpoints.version_manifest, "mojang")
            .await
        {
            Ok(manifest) => {
                let wrapper = serde_json::json!({"versions": &manifest.versions});
                if let Ok(bytes) = serde_json::to_vec_pretty(&wrapper) {
                    let _ = crate::paths::atomic_write(
                        &self.paths.cache.join("version_manifest_v2.json"),
                        &bytes,
                    );
                }
                *self.manifest_cache.write().await = Some((chrono::Utc::now(), manifest.clone()));
                Ok(manifest)
            }
            Err(error) => {
                if let Some((_, manifest)) = &*self.manifest_cache.read().await {
                    return Ok(manifest.clone());
                }
                let cached = tokio::fs::read(self.paths.cache.join("version_manifest_v2.json"))
                    .await
                    .map_err(|_| error)?;
                serde_json::from_slice(&cached).map_err(CoreError::from)
            }
        }
    }

    pub async fn list(&self, include_snapshots: bool) -> CoreResult<Vec<VersionSummary>> {
        let manifest = self.manifest().await?;
        Ok(manifest
            .versions
            .into_iter()
            .filter(|version| include_snapshots || version.release_type == "release")
            .collect())
    }

    pub async fn merged_for_instance(&self, instance: &Instance) -> CoreResult<MergedVersion> {
        if instance.loader == crate::model::LoaderKind::Vanilla {
            return self.merged(&instance.game_version).await;
        }
        let profile_id = loader_profile_id(instance);
        let path = self
            .paths
            .content
            .join("versions")
            .join(&profile_id)
            .join(format!("{profile_id}.json"));
        let details: VersionDetails = serde_json::from_slice(&tokio::fs::read(&path).await?)?;
        let parent = details.inherits_from.clone().ok_or_else(|| {
            CoreError::InvalidInput(format!("loader profile {profile_id} has no parent version"))
        })?;
        let mut merged = self.merged(&parent).await?;
        apply_details(&mut merged, details);
        if merged.main_class.is_empty() {
            return Err(CoreError::InvalidInput(
                "loader profile has no main class".into(),
            ));
        }
        Ok(merged)
    }

    pub async fn merged(&self, version: &str) -> CoreResult<MergedVersion> {
        safe_relative_path(version)?;
        let manifest = self.manifest().await?;
        let mut chain = Vec::new();
        let mut current = version.to_owned();
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                return Err(CoreError::InvalidInput(
                    "version inheritance contains a cycle".into(),
                ));
            }
            if chain.len() >= 16 {
                return Err(CoreError::InvalidInput(
                    "version inheritance chain is too deep".into(),
                ));
            }
            let summary = manifest
                .versions
                .iter()
                .find(|candidate| candidate.id == current)
                .cloned()
                .ok_or_else(|| CoreError::NotFound(format!("game version {current}")))?;
            let details = self.load_details(&summary).await?;
            let parent = details.inherits_from.clone();
            chain.push(details);
            match parent {
                Some(parent) => current = parent,
                None => break,
            }
        }
        merge_chain(chain.into_iter().rev().collect())
    }

    async fn load_details(&self, summary: &VersionSummary) -> CoreResult<VersionDetails> {
        let directory = safe_join(&self.paths.content.join("versions"), &summary.id)?;
        let path = directory.join(format!("{}.json", summary.id));
        self.downloader
            .download(
                DownloadSpec {
                    url: summary.url.clone(),
                    destination: path.clone(),
                    size: None,
                    sha1: summary.sha1.clone(),
                    sha256: None,
                    provider: "mojang".into(),
                },
                None,
            )
            .await?;
        let bytes = tokio::fs::read(&path).await?;
        serde_json::from_slice(&bytes).map_err(|error| CoreError::Upstream {
            provider: "mojang".into(),
            status: 200,
            message: format!("invalid version metadata for {}: {error}", summary.id),
        })
    }

    pub async fn install(&self, instance: &Instance, context: &JobContext) -> CoreResult<Value> {
        let merged = self.merged_for_instance(instance).await?;
        let instance_root = self.paths.instance(instance.id);
        let natives = instance_root.join("natives");
        tokio::fs::create_dir_all(&natives).await?;
        let mut specs = Vec::new();
        let mut native_archives = Vec::new();
        let client = merged
            .downloads
            .get("client")
            .ok_or_else(|| CoreError::Upstream {
                provider: "mojang".into(),
                status: 200,
                message: "version has no client download".into(),
            })?;
        let version_directory =
            safe_join(&self.paths.content.join("versions"), &instance.game_version)?;
        specs.push(remote_spec(
            client,
            version_directory.join(format!("{}.jar", instance.game_version)),
            "mojang",
        ));
        for library in merged
            .libraries
            .iter()
            .filter(|library| rules_allow(&library.rules, &HashMap::new()))
        {
            if library.downloads.artifact.is_some() || library.url.is_some() {
                let artifact = library_artifact(library)?;
                let path = artifact.path.clone().ok_or_else(|| {
                    CoreError::InvalidInput(format!(
                        "library {} has no artifact path",
                        library.name
                    ))
                })?;
                specs.push(remote_spec(
                    &artifact,
                    safe_join(&self.paths.content.join("libraries"), &path)?,
                    "mojang",
                ));
            }
            if let Some(classifier) = library.natives.get(os_name()) {
                let classifier = classifier.replace("${arch}", java_arch());
                if let Some(native) = library.downloads.classifiers.get(&classifier) {
                    let path = native.path.clone().ok_or_else(|| {
                        CoreError::InvalidInput(format!(
                            "native library {} has no path",
                            library.name
                        ))
                    })?;
                    let destination = safe_join(&self.paths.content.join("libraries"), &path)?;
                    specs.push(remote_spec(native, destination.clone(), "mojang"));
                    native_archives
                        .push((destination, library.extract.clone().unwrap_or_default()));
                }
            }
        }

        let asset_index = merged
            .asset_index
            .as_ref()
            .ok_or_else(|| CoreError::Upstream {
                provider: "mojang".into(),
                status: 200,
                message: "version has no asset index".into(),
            })?;
        let asset_index_path = self
            .paths
            .content
            .join("assets")
            .join("indexes")
            .join(format!("{}.json", asset_index.id));
        self.downloader
            .download(
                DownloadSpec {
                    url: asset_index.url.clone(),
                    destination: asset_index_path.clone(),
                    size: asset_index.size,
                    sha1: asset_index.sha1.clone(),
                    sha256: None,
                    provider: "mojang".into(),
                },
                Some(context),
            )
            .await?;
        let index: AssetIndex = serde_json::from_slice(&tokio::fs::read(asset_index_path).await?)?;
        let asset_count = index.objects.len();
        for object in index.objects.values() {
            let relative = format!("{}/{}", &object.hash[..2], object.hash);
            specs.push(DownloadSpec {
                url: format!("https://resources.download.minecraft.net/{relative}"),
                destination: self
                    .paths
                    .content
                    .join("assets")
                    .join("objects")
                    .join(&relative),
                size: Some(object.size),
                sha1: Some(object.hash.clone()),
                sha256: None,
                provider: "mojang".into(),
            });
        }

        let total = specs.len() as u64;
        let completed = Arc::new(AtomicU64::new(0));
        let pending = Arc::new(std::sync::Mutex::new(
            specs
                .iter()
                .map(|spec| spec.destination.display().to_string())
                .collect::<Vec<_>>(),
        ));
        context.progress(JobProgress::download_queue(
            "installing_game",
            0,
            Some(total),
            pending.lock().unwrap().clone(),
        ))?;
        let max_concurrent = self.downloader.max_concurrent();
        let downloader = self.downloader.clone();
        stream::iter(specs.into_iter().map(Ok::<_, CoreError>))
            .try_for_each_concurrent(max_concurrent, |spec| {
                let downloader = downloader.clone();
                let completed = completed.clone();
                let pending = pending.clone();
                async move {
                    context.check_cancelled()?;
                    let item = spec.destination.display().to_string();
                    downloader.download(spec, None).await?;
                    let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    let remaining = {
                        let mut pending = pending.lock().unwrap();
                        pending.retain(|candidate| candidate != &item);
                        pending.clone()
                    };
                    if done % 8 == 0 || done == total {
                        context.progress(JobProgress::download_queue(
                            "installing_game",
                            done,
                            Some(total),
                            remaining,
                        ))?;
                    }
                    Ok(())
                }
            })
            .await?;
        for (archive, rules) in native_archives {
            let natives = natives.clone();
            tokio::task::spawn_blocking(move || extract_natives(&archive, &natives, &rules))
                .await
                .map_err(|error| CoreError::Internal(error.to_string()))??;
        }
        let java_version = merged.java_version.clone().unwrap_or(JavaRequirement {
            component: Some("jre-legacy".into()),
            major_version: 8,
        });
        Ok(serde_json::json!({
            "version": merged.id,
            "assets": asset_count,
            "libraries": merged.libraries.len(),
            "javaVersion": java_version,
        }))
    }
}

pub fn loader_profile_id(instance: &Instance) -> String {
    let loader = match instance.loader {
        crate::model::LoaderKind::Fabric => "fabric",
        crate::model::LoaderKind::Forge => "forge",
        crate::model::LoaderKind::NeoForge => "neoforge",
        crate::model::LoaderKind::Quilt => "quilt",
        crate::model::LoaderKind::Vanilla => "vanilla",
    };
    format!(
        "{loader}-{}-{}",
        instance.game_version,
        instance.loader_version.as_deref().unwrap_or("unknown")
    )
}

fn merge_chain(chain: Vec<VersionDetails>) -> CoreResult<MergedVersion> {
    let mut merged = MergedVersion::default();
    for details in chain {
        apply_details(&mut merged, details);
    }
    if merged.id.is_empty() || merged.main_class.is_empty() {
        return Err(CoreError::Upstream {
            provider: "mojang".into(),
            status: 200,
            message: "merged version metadata is incomplete".into(),
        });
    }
    Ok(merged)
}

fn apply_details(merged: &mut MergedVersion, details: VersionDetails) {
    merged.id = details.id;
    if let Some(value) = details.main_class {
        merged.main_class = value;
    }
    if let Some(value) = details.assets {
        merged.assets = value;
    }
    if details.asset_index.is_some() {
        merged.asset_index = details.asset_index;
    }
    merged.downloads.extend(details.downloads);
    merged.libraries.extend(details.libraries);
    if let Some(arguments) = details.arguments {
        let merged_arguments = merged
            .arguments
            .get_or_insert_with(VersionArguments::default);
        merged_arguments.jvm.extend(arguments.jvm);
        merged_arguments.game.extend(arguments.game);
    }
    if details.minecraft_arguments.is_some() {
        merged.minecraft_arguments = details.minecraft_arguments;
    }
    if details.java_version.is_some() {
        merged.java_version = details.java_version;
    }
    if let Some(value) = details.release_type {
        merged.release_type = value;
    }
}

pub fn rules_allow(rules: &[Rule], features: &HashMap<String, bool>) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        let os_matches = rule.os.as_ref().is_none_or(|os| {
            os.name.as_deref().is_none_or(|name| name == os_name())
                && os.arch.as_deref().is_none_or(|arch| {
                    (arch == "x86" && std::env::consts::ARCH == "x86")
                        || (arch == "amd64" && std::env::consts::ARCH == "x86_64")
                        || (arch == "arm64" && std::env::consts::ARCH == "aarch64")
                })
        });
        let features_match = rule.features.as_ref().is_none_or(|required| {
            required
                .iter()
                .all(|(key, value)| features.get(key) == Some(value))
        });
        if os_matches && features_match {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "osx",
        os => os,
    }
}

pub fn java_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "32",
        _ => "64",
    }
}

pub fn library_artifact(library: &Library) -> CoreResult<RemoteFile> {
    if let Some(artifact) = &library.downloads.artifact {
        return Ok(artifact.clone());
    }
    let base = library.url.as_ref().ok_or_else(|| {
        CoreError::InvalidInput(format!("library {} has no download metadata", library.name))
    })?;
    let path = maven_artifact_path(&library.name)?;
    Ok(RemoteFile {
        url: format!("{}/{path}", base.trim_end_matches('/')),
        sha1: library.sha1.clone(),
        sha256: library.sha256.clone(),
        size: library.size,
        path: Some(path),
    })
}

fn maven_artifact_path(name: &str) -> CoreResult<String> {
    let parts = name.split(':').collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len()) || parts.iter().any(|part| part.is_empty()) {
        return Err(CoreError::InvalidInput(format!(
            "invalid Maven library name: {name}"
        )));
    }
    let classifier = parts
        .get(3)
        .map(|classifier| format!("-{classifier}"))
        .unwrap_or_default();
    Ok(format!(
        "{}/{}/{}/{}-{}{}.jar",
        parts[0].replace('.', "/"),
        parts[1],
        parts[2],
        parts[1],
        parts[2],
        classifier
    ))
}

fn remote_spec(file: &RemoteFile, destination: PathBuf, provider: &str) -> DownloadSpec {
    DownloadSpec {
        url: file.url.clone(),
        destination,
        size: file.size,
        sha1: file.sha1.clone(),
        sha256: file.sha256.clone(),
        provider: provider.into(),
    }
}

fn extract_natives(archive: &Path, destination: &Path, rules: &ExtractRules) -> CoreResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        if entry.is_dir()
            || rules
                .exclude
                .iter()
                .any(|prefix| entry.name().starts_with(prefix))
        {
            continue;
        }
        let relative = safe_relative_path(entry.name())?;
        let output = destination.join(relative);
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(output)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_ordered_platform_rules() {
        let rules = vec![
            Rule {
                action: "allow".into(),
                ..Rule::default()
            },
            Rule {
                action: "disallow".into(),
                os: Some(OsRule {
                    name: Some("unmatched".into()),
                    arch: None,
                }),
                features: None,
            },
        ];
        assert!(rules_allow(&rules, &HashMap::new()));
    }

    #[test]
    fn rejects_unknown_feature_rules() {
        let rules = vec![Rule {
            action: "allow".into(),
            features: Some(HashMap::from([("is_demo_user".into(), true)])),
            ..Rule::default()
        }];
        assert!(!rules_allow(&rules, &HashMap::new()));
    }

    #[test]
    fn inheritance_appends_loader_arguments() {
        let parent = VersionDetails {
            id: "base".into(),
            main_class: Some("base.Main".into()),
            arguments: Some(VersionArguments {
                game: vec![ArgumentValue::Plain("--username".into())],
                jvm: vec![ArgumentValue::Plain("-cp".into())],
            }),
            ..VersionDetails::default()
        };
        let child = VersionDetails {
            id: "loader-profile".into(),
            main_class: Some("loader.Main".into()),
            arguments: Some(VersionArguments {
                game: vec![ArgumentValue::Plain("--accessToken".into())],
                jvm: vec![ArgumentValue::Plain("-Dloader=true".into())],
            }),
            ..VersionDetails::default()
        };
        let merged = merge_chain(vec![parent, child]).unwrap();
        let arguments = merged.arguments.unwrap();
        assert_eq!(arguments.game.len(), 2);
        assert_eq!(arguments.jvm.len(), 2);
        assert_eq!(merged.id, "loader-profile");
        assert_eq!(merged.main_class, "loader.Main");
    }
}
