use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::download::{DownloadSpec, Downloader};
use crate::error::{CoreError, CoreResult};
use crate::instance::InstanceService;
use crate::job::JobContext;
use crate::model::{
    CreateInstanceParams, InstallStatus, LoaderKind, ModFile, ModProvider, ModpackEntryResult,
    ModpackEntryStatus, ModpackRecord, ReleaseChannel,
};
use crate::mods::ModService;
use crate::paths::{safe_relative_path, AppPaths};
use crate::store::Store;

pub(crate) const MAX_ENTRIES: usize = 50_000;
pub(crate) const MAX_ENTRY_SIZE: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_TOTAL_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 200;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModpackFormat {
    CurseForge,
    Modrinth,
    Ftb,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackAnalysis {
    pub format: ModpackFormat,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub game_version: String,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub mod_count: usize,
    pub override_prefixes: Vec<String>,
    pub warnings: Vec<String>,
    pub source_sha256: String,
    #[serde(skip_serializing)]
    pub files: Vec<PackFile>,
}

#[derive(Debug, Clone)]
pub struct PackFile {
    pub path: String,
    pub provider: ModProvider,
    pub project_id: Option<String>,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub size: Option<u64>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub required: bool,
}

struct ModpackDownloadItem {
    path: String,
    name: String,
}

struct ModpackDownloadQueue {
    active: Vec<ModpackDownloadItem>,
    pending: Vec<ModpackDownloadItem>,
}

impl ModpackDownloadQueue {
    fn new(files: &[PackFile], project_names: &HashMap<String, String>) -> Self {
        Self {
            active: Vec::new(),
            pending: files
                .iter()
                .map(|file| ModpackDownloadItem {
                    path: file.path.clone(),
                    name: download_item_name(file, project_names),
                })
                .collect(),
        }
    }

    fn start(&mut self, path: &str) {
        if let Some(index) = self.pending.iter().position(|item| item.path == path) {
            let item = self.pending.remove(index);
            self.active.push(item);
        }
    }

    fn finish(&mut self, path: &str) {
        self.active.retain(|item| item.path != path);
    }

    fn progress(&self, completed: u64, total: u64) -> crate::model::JobProgress {
        crate::model::JobProgress::concurrent_download_queue(
            "importing_modpack",
            completed,
            Some(total),
            self.active.iter().map(|item| item.name.clone()).collect(),
            self.pending.iter().map(|item| item.name.clone()).collect(),
        )
    }
}

#[derive(Clone)]
pub struct ModpackService {
    paths: AppPaths,
    store: Arc<Store>,
    downloader: Downloader,
    instances: InstanceService,
    mods: ModService,
    browser: crate::modpack_browser::ModpackBrowserService,
}

use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseManifest {
    name: String,
    version: Option<String>,
    author: Option<String>,
    minecraft: CurseMinecraft,
    files: Vec<CurseFile>,
    overrides: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseMinecraft {
    version: String,
    mod_loaders: Vec<CurseLoader>,
}

#[derive(Debug, Deserialize)]
struct CurseLoader {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModrinthIndex {
    version_id: Option<String>,
    name: String,
    dependencies: HashMap<String, String>,
    files: Vec<ModrinthFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModrinthFile {
    path: String,
    hashes: ModrinthHashes,
    downloads: Vec<String>,
    file_size: u64,
    env: Option<ModrinthEnvironment>,
}

#[derive(Debug, Deserialize)]
struct ModrinthHashes {
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthEnvironment {
    client: Option<String>,
}

impl ModpackService {
    pub fn new(
        paths: AppPaths,
        store: Arc<Store>,
        downloader: Downloader,
        instances: InstanceService,
        mods: ModService,
        ftb_base: String,
    ) -> CoreResult<Self> {
        let browser = crate::modpack_browser::ModpackBrowserService::new(
            paths.clone(),
            downloader.clone(),
            mods.clone(),
            ftb_base,
        )?;
        Ok(Self {
            paths,
            store,
            downloader,
            instances,
            mods,
            browser,
        })
    }

    pub async fn analyze(&self, path: &Path) -> CoreResult<ModpackAnalysis> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || analyze_archive(&path))
            .await
            .map_err(|error| CoreError::Internal(error.to_string()))?
    }

    pub async fn search(
        &self,
        params: crate::mods::ModSearchParams,
    ) -> CoreResult<Vec<crate::mods::ModProject>> {
        self.browser.search(params).await
    }

    pub async fn icon(&self, url: &str) -> CoreResult<String> {
        self.browser.icon(url).await
    }

    pub async fn import(
        &self,
        path: &Path,
        name: Option<String>,
        context: &JobContext,
    ) -> CoreResult<Value> {
        let analysis = self.analyze(path).await?;
        self.import_analysis(
            analysis,
            Some(path.to_path_buf()),
            name,
            None,
            None,
            context,
        )
        .await
    }

    pub async fn import_remote(
        &self,
        params: crate::modpack_browser::ModpackDownloadParams,
        context: &JobContext,
    ) -> CoreResult<Value> {
        let name = params.name.clone();
        let instance_id = params.instance_id;
        let resume = match instance_id {
            Some(id) => self.resume_checkpoint(id, context.id())?,
            None => None,
        };
        match self.browser.fetch(&params, context).await? {
            crate::modpack_browser::RemoteModpack::Archive { path, name: source } => {
                let analysis = self.analyze(&path).await?;
                self.import_analysis(
                    analysis,
                    Some(path),
                    name.or(Some(source)),
                    instance_id,
                    resume,
                    context,
                )
                .await
            }
            crate::modpack_browser::RemoteModpack::Prepared(analysis) => {
                self.import_analysis(analysis, None, name, instance_id, resume, context)
                    .await
            }
        }
    }

    fn resume_checkpoint(
        &self,
        instance_id: uuid::Uuid,
        current_job_id: uuid::Uuid,
    ) -> CoreResult<Option<Value>> {
        let instance_id = instance_id.to_string();
        Ok(self
            .store
            .jobs()?
            .into_iter()
            .filter(|job| job.kind == crate::model::JobKind::Import && job.id != current_job_id)
            .filter_map(|job| job.checkpoint)
            .find(|checkpoint| {
                checkpoint.get("instanceId").and_then(Value::as_str) == Some(instance_id.as_str())
            }))
    }

    async fn import_analysis(
        &self,
        analysis: ModpackAnalysis,
        archive: Option<std::path::PathBuf>,
        name: Option<String>,
        existing_instance_id: Option<uuid::Uuid>,
        resume: Option<Value>,
        context: &JobContext,
    ) -> CoreResult<Value> {
        let existing = match existing_instance_id {
            Some(id) => match self.instances.get(id) {
                Ok(instance) => Some(instance),
                Err(CoreError::NotFound(_)) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        if let Some(instance) = &existing {
            if instance.game_version != analysis.game_version
                || instance.loader != analysis.loader
                || instance.loader_version != analysis.loader_version
            {
                return Err(CoreError::InvalidInput(
                    "modpack does not match the instance being resumed".into(),
                ));
            }
        }
        let resuming = existing.is_some();
        let instance = match existing {
            Some(instance) => instance,
            None => self.instances.create(CreateInstanceParams {
                name: name.unwrap_or_else(|| analysis.name.clone()),
                description: Some(format!("Imported {:?} modpack", analysis.format)),
                game_version: analysis.game_version.clone(),
                loader: analysis.loader,
                loader_version: analysis.loader_version.clone(),
                java_path: None,
                min_memory_mb: Some(512),
                max_memory_mb: Some(4096),
                jvm_args: Some(Vec::new()),
                game_args: Vec::new(),
                account_id: None,
            })?,
        };
        context.update_checkpoint(|checkpoint| {
            checkpoint.insert("stage".into(), serde_json::json!("instance_created"));
            checkpoint.insert("instanceId".into(), serde_json::json!(instance.id));
            checkpoint.insert(
                "sourceSha256".into(),
                serde_json::json!(analysis.source_sha256),
            );
        })?;
        let game_ready = resume
            .as_ref()
            .and_then(|checkpoint| checkpoint.get("gameReady"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !game_ready {
            if let Err(error) = self.instances.install(instance.id, context).await {
                if !resuming {
                    let _ = self.instances.delete(instance.id).await;
                }
                return Err(error);
            }
        } else if instance.install_status != InstallStatus::Installed {
            self.instances
                .set_install_status(instance.id, InstallStatus::Installed)?;
        }
        let previous_files = resume
            .as_ref()
            .and_then(|checkpoint| checkpoint.get("modpackFiles"))
            .filter(|files| files.is_object())
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        context.update_checkpoint(|checkpoint| {
            checkpoint.insert("gameReady".into(), Value::Bool(true));
            checkpoint.insert("modpackFiles".into(), previous_files);
        })?;

        let mut entries = Vec::new();
        let mut pending = Vec::new();
        for file in &analysis.files {
            match resumed_file_status(resume.as_ref(), &file.path) {
                Some(status @ (ModpackEntryStatus::Imported | ModpackEntryStatus::Skipped)) => {
                    entries.push(entry(file, status, None))
                }
                _ => pending.push(file.clone()),
            }
        }
        let project_ids = pending
            .iter()
            .filter(|file| file.provider == ModProvider::CurseForge)
            .filter_map(|file| file.project_id.clone())
            .collect::<Vec<_>>();
        let project_names = self
            .mods
            .curseforge_project_names(&project_ids)
            .await
            .unwrap_or_default();
        let total = analysis.files.len() as u64;
        let completed = Arc::new(AtomicU64::new(entries.len() as u64));
        let download_queue = ModpackDownloadQueue::new(&pending, &project_names);
        context.progress(download_queue.progress(completed.load(Ordering::SeqCst), total))?;
        let download_queue = Arc::new(std::sync::Mutex::new(download_queue));
        let mut downloads = stream::iter(pending.into_iter().map(|file| {
            let instance = instance.clone();
            let completed = completed.clone();
            let download_queue = download_queue.clone();
            async move {
                context.check_cancelled()?;
                {
                    let mut queue = download_queue.lock().unwrap();
                    queue.start(&file.path);
                    context.progress(queue.progress(completed.load(Ordering::SeqCst), total))?;
                }
                let result = if !file.required {
                    Ok(entry(&file, ModpackEntryStatus::Skipped, None))
                } else {
                    match self.import_file(&instance, &file, context).await {
                        Ok(()) => Ok(entry(&file, ModpackEntryStatus::Imported, None)),
                        Err(error) if error.is_cancelled() => Err(error),
                        Err(error) => {
                            let status = if matches!(error, CoreError::Forbidden(_)) {
                                ModpackEntryStatus::RequiresManualDownload
                            } else {
                                ModpackEntryStatus::Failed
                            };
                            Ok(entry(&file, status, Some(error.to_string())))
                        }
                    }
                };
                Ok::<_, CoreError>((file, result))
            }
        }))
        .buffer_unordered(self.downloader.max_concurrent());
        while let Some(result) = downloads.next().await {
            let (file, result) = result?;
            let result = result?;
            let status = result.status;
            entries.push(result);
            context.update_checkpoint(|checkpoint| {
                let files = checkpoint
                    .entry("modpackFiles")
                    .or_insert_with(|| serde_json::json!({}));
                if !files.is_object() {
                    *files = serde_json::json!({});
                }
                files
                    .as_object_mut()
                    .expect("modpack file state must be an object")
                    .insert(file.path.clone(), serde_json::to_value(status).unwrap());
            })?;
            let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
            let mut queue = download_queue.lock().unwrap();
            queue.finish(&file.path);
            context.progress(queue.progress(done, total))?;
        }
        if let Some(archive) = archive {
            let destination = self.paths.instance(instance.id);
            let prefixes = analysis.override_prefixes.clone();
            tokio::task::spawn_blocking(move || apply_overrides(&archive, &destination, &prefixes))
                .await
                .map_err(|error| CoreError::Internal(error.to_string()))??;
        }
        let complete = entries.iter().all(|entry| {
            matches!(
                entry.status,
                ModpackEntryStatus::Imported | ModpackEntryStatus::Skipped
            )
        });
        if !complete {
            self.instances
                .set_install_status(instance.id, InstallStatus::Incomplete)?;
        }
        let record = ModpackRecord {
            instance_id: instance.id,
            format: format!("{:?}", analysis.format).to_ascii_lowercase(),
            source_name: analysis.name.clone(),
            source_sha256: analysis.source_sha256.clone(),
            imported_at: chrono::Utc::now(),
            entries: entries.clone(),
        };
        self.store.save_modpack(&record)?;
        Ok(serde_json::json!({
            "instanceId": instance.id,
            "complete": complete,
            "entries": entries,
            "analysis": analysis,
        }))
    }

    async fn import_file(
        &self,
        instance: &crate::model::Instance,
        file: &PackFile,
        context: &JobContext,
    ) -> CoreResult<()> {
        let mod_file = match file.provider {
            ModProvider::CurseForge => {
                let mut mod_file = self
                    .mods
                    .provider_file(
                        ModProvider::CurseForge,
                        file.project_id.as_deref().unwrap_or_default(),
                        file.file_id.as_deref().unwrap_or_default(),
                    )
                    .await?;
                if mod_file.url.is_none() {
                    mod_file.url = Some(curseforge_download_url(file));
                    mod_file.distribution_allowed = true;
                }
                mod_file
            }
            ModProvider::Modpack => ModFile {
                provider: ModProvider::Modpack,
                project_id: file.path.clone(),
                file_id: file.sha1.clone().unwrap_or_else(|| file.path.clone()),
                project_name: file.path.clone(),
                version_name: "modpack".into(),
                file_name: Path::new(&file.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                url: file.url.clone(),
                size: file.size,
                sha1: file.sha1.clone(),
                sha256: file.sha256.clone(),
                game_versions: vec![instance.game_version.clone()],
                loaders: vec![instance.loader],
                channel: ReleaseChannel::Release,
                dependencies: Vec::new(),
                published_at: None,
                distribution_allowed: file.url.is_some(),
            },
            _ => {
                return Err(CoreError::InvalidInput(
                    "unsupported modpack file provider".into(),
                ))
            }
        };
        if file.path.starts_with("mods/") && file.path.ends_with(".jar") {
            self.mods
                .install_without_progress(instance.id, mod_file, context)
                .await?;
        } else {
            let destination = self
                .paths
                .instance(instance.id)
                .join(safe_relative_path(&file.path)?);
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            self.downloader
                .download_without_progress(
                    DownloadSpec {
                        url: file.url.clone().ok_or_else(|| {
                            CoreError::Forbidden(format!("{} requires manual download", file.path))
                        })?,
                        destination,
                        size: file.size,
                        sha1: file.sha1.clone(),
                        sha256: file.sha256.clone(),
                        provider: "modpack".into(),
                    },
                    Some(context),
                )
                .await?;
        }
        Ok(())
    }
}

fn analyze_archive(path: &Path) -> CoreResult<ModpackAnalysis> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    if archive.len() > MAX_ENTRIES {
        return Err(CoreError::InvalidInput(
            "modpack has too many entries".into(),
        ));
    }
    let mut total_size = 0_u64;
    let mut paths = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        inspect_entry(&entry)?;
        total_size = total_size.saturating_add(entry.size());
        if total_size > MAX_TOTAL_SIZE {
            return Err(CoreError::InvalidInput(
                "modpack uncompressed size exceeds the limit".into(),
            ));
        }
        let normalized = safe_relative_path(entry.name())?
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        if !paths.insert(normalized) {
            return Err(CoreError::InvalidInput(format!(
                "modpack contains a duplicate target path: {}",
                entry.name()
            )));
        }
    }
    let source_sha256 = file_sha256(path)?;
    if let Some(text) = read_manifest(&mut archive, "modrinth.index.json")? {
        let index: ModrinthIndex = serde_json::from_str(&text)?;
        return analyze_modrinth(index, source_sha256, &archive);
    }
    if let Some(text) = read_manifest(&mut archive, "manifest.json")? {
        let manifest: CurseManifest = serde_json::from_str(&text)?;
        return analyze_curseforge(manifest, source_sha256, &archive);
    }
    Err(CoreError::InvalidInput(
        "archive is neither a Modrinth nor CurseForge modpack".into(),
    ))
}

fn read_manifest(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> CoreResult<Option<String>> {
    let mut file = match archive.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    read_limited(&mut file).map(Some)
}

fn analyze_modrinth(
    index: ModrinthIndex,
    source_sha256: String,
    archive: &zip::ZipArchive<std::fs::File>,
) -> CoreResult<ModpackAnalysis> {
    let game_version = index
        .dependencies
        .get("minecraft")
        .cloned()
        .ok_or_else(|| CoreError::InvalidInput("Modrinth pack has no Minecraft version".into()))?;
    let (loader, loader_version) = modrinth_loader(&index.dependencies)?;
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    for file in index.files {
        if file.env.and_then(|env| env.client).as_deref() == Some("unsupported") {
            warnings.push(format!("{} is not required on the client", file.path));
            continue;
        }
        safe_relative_path(&file.path)?;
        files.push(PackFile {
            path: file.path,
            provider: ModProvider::Modpack,
            project_id: None,
            file_id: None,
            url: file.downloads.first().cloned(),
            size: Some(file.file_size),
            sha1: file.hashes.sha1,
            sha256: None,
            required: true,
        });
    }
    let override_prefixes = existing_prefixes(archive, &["overrides/", "client-overrides/"]);
    Ok(ModpackAnalysis {
        format: ModpackFormat::Modrinth,
        name: index.name,
        version: index.version_id,
        author: None,
        game_version,
        loader,
        loader_version,
        mod_count: files
            .iter()
            .filter(|file| file.path.starts_with("mods/"))
            .count(),
        override_prefixes,
        warnings,
        source_sha256,
        files,
    })
}

fn analyze_curseforge(
    manifest: CurseManifest,
    source_sha256: String,
    archive: &zip::ZipArchive<std::fs::File>,
) -> CoreResult<ModpackAnalysis> {
    let loader = manifest
        .minecraft
        .mod_loaders
        .first()
        .ok_or_else(|| CoreError::InvalidInput("CurseForge pack has no mod loader".into()))?;
    let (kind, version) = parse_loader_id(&loader.id)?;
    let files = manifest
        .files
        .into_iter()
        .map(|file| PackFile {
            path: format!("mods/{}-{}.jar", file.project_id, file.file_id),
            provider: ModProvider::CurseForge,
            project_id: Some(file.project_id.to_string()),
            file_id: Some(file.file_id.to_string()),
            url: None,
            size: None,
            sha1: None,
            sha256: None,
            required: file.required,
        })
        .collect::<Vec<_>>();
    let mut prefixes = Vec::new();
    if let Some(overrides) = manifest.overrides {
        let prefix = format!("{}/", overrides.trim_matches('/'));
        safe_relative_path(&prefix)?;
        prefixes = existing_prefixes(archive, &[prefix.as_str()]);
    }
    let client_overrides = existing_prefixes(archive, &["client-overrides/"]);
    prefixes.extend(client_overrides);
    Ok(ModpackAnalysis {
        format: ModpackFormat::CurseForge,
        name: manifest.name,
        version: manifest.version,
        author: manifest.author,
        game_version: manifest.minecraft.version,
        loader: kind,
        loader_version: Some(version),
        mod_count: files.len(),
        override_prefixes: prefixes,
        warnings: Vec::new(),
        source_sha256,
        files,
    })
}

fn inspect_entry<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> CoreResult<()> {
    safe_relative_path(entry.name())?;
    if entry.size() > MAX_ENTRY_SIZE {
        return Err(CoreError::InvalidInput(format!(
            "modpack entry is too large: {}",
            entry.name()
        )));
    }
    if entry.compressed_size() > 0 && entry.size() / entry.compressed_size() > MAX_COMPRESSION_RATIO
    {
        return Err(CoreError::InvalidInput(
            "modpack compression ratio exceeds the limit".into(),
        ));
    }
    if let Some(mode) = entry.unix_mode() {
        let file_type = mode & 0o170000;
        if file_type == 0o120000 {
            return Err(CoreError::InvalidInput(
                "modpack symbolic links are not allowed".into(),
            ));
        }
        if !matches!(file_type, 0 | 0o040000 | 0o100000) {
            return Err(CoreError::InvalidInput(
                "modpack contains a special file".into(),
            ));
        }
    }
    Ok(())
}

fn read_limited(reader: &mut impl Read) -> CoreResult<String> {
    let mut bytes = Vec::new();
    reader.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(CoreError::InvalidInput(
            "modpack manifest is too large".into(),
        ));
    }
    String::from_utf8(bytes).map_err(|_| CoreError::InvalidInput("manifest is not UTF-8".into()))
}

fn apply_overrides(archive_path: &Path, destination: &Path, prefixes: &[String]) -> CoreResult<()> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        inspect_entry(&entry)?;
        let Some(prefix) = prefixes
            .iter()
            .find(|prefix| entry.name().starts_with(prefix.as_str()))
        else {
            continue;
        };
        let relative = entry.name()[prefix.len()..].trim_matches('/');
        if relative.is_empty() || entry.is_dir() {
            continue;
        }
        let relative = safe_relative_path(relative)?;
        let output = destination.join(relative);
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(output)?;
        let copied = std::io::copy(&mut entry.take(MAX_ENTRY_SIZE + 1), &mut file)?;
        total = total.saturating_add(copied);
        if copied > MAX_ENTRY_SIZE || total > MAX_TOTAL_SIZE {
            return Err(CoreError::InvalidInput(
                "override extraction exceeds resource limits".into(),
            ));
        }
    }
    Ok(())
}

fn existing_prefixes(archive: &zip::ZipArchive<std::fs::File>, candidates: &[&str]) -> Vec<String> {
    candidates
        .iter()
        .filter(|candidate| {
            archive
                .file_names()
                .any(|name| name.starts_with(*candidate))
        })
        .map(|candidate| (*candidate).to_owned())
        .collect()
}

fn modrinth_loader(
    dependencies: &HashMap<String, String>,
) -> CoreResult<(LoaderKind, Option<String>)> {
    for (key, loader) in [
        ("fabric-loader", LoaderKind::Fabric),
        ("forge", LoaderKind::Forge),
        ("neoforge", LoaderKind::NeoForge),
        ("quilt-loader", LoaderKind::Quilt),
    ] {
        if let Some(version) = dependencies.get(key) {
            return Ok((loader, Some(version.clone())));
        }
    }
    Ok((LoaderKind::Vanilla, None))
}

fn parse_loader_id(id: &str) -> CoreResult<(LoaderKind, String)> {
    let (name, version) = id
        .split_once('-')
        .ok_or_else(|| CoreError::InvalidInput(format!("invalid loader id: {id}")))?;
    let loader = match name {
        "fabric" => LoaderKind::Fabric,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        "quilt" => LoaderKind::Quilt,
        _ => return Err(CoreError::Unsupported(format!("loader {name}"))),
    };
    Ok((loader, version.to_owned()))
}

fn file_sha256(path: &Path) -> CoreResult<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(hex::encode(hash.finalize()))
}

fn curseforge_download_url(file: &PackFile) -> String {
    format!(
        "https://www.curseforge.com/api/v1/mods/{}/files/{}/download",
        file.project_id.as_deref().unwrap_or_default(),
        file.file_id.as_deref().unwrap_or_default()
    )
}

fn download_item_name(file: &PackFile, project_names: &HashMap<String, String>) -> String {
    file.project_id
        .as_ref()
        .and_then(|id| project_names.get(id))
        .cloned()
        .unwrap_or_else(|| file.path.clone())
}

fn resumed_file_status(resume: Option<&Value>, path: &str) -> Option<ModpackEntryStatus> {
    let status = resume?.get("modpackFiles")?.get(path)?;
    serde_json::from_value(status.clone()).ok()
}

fn entry(
    file: &PackFile,
    status: ModpackEntryStatus,
    detail: Option<String>,
) -> ModpackEntryResult {
    ModpackEntryResult {
        name: file.path.clone(),
        status,
        detail,
    }
}

pub fn pack_file_name(file: &Path) -> Option<&str> {
    file.file_name().and_then(|name| name.to_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_items_prefer_project_names_with_path_fallback() {
        let file = PackFile {
            path: "mods/238222-123456.jar".into(),
            provider: ModProvider::CurseForge,
            project_id: Some("238222".into()),
            file_id: Some("123456".into()),
            url: None,
            size: None,
            sha1: None,
            sha256: None,
            required: true,
        };
        let names = HashMap::from([("238222".into(), "Just Enough Items".into())]);
        assert_eq!(download_item_name(&file, &names), "Just Enough Items");
        assert_eq!(
            download_item_name(&file, &HashMap::new()),
            "mods/238222-123456.jar"
        );
        assert_eq!(
            curseforge_download_url(&file),
            "https://www.curseforge.com/api/v1/mods/238222/files/123456/download"
        );
    }

    #[test]
    fn download_queue_tracks_out_of_order_concurrent_files() {
        let first = PackFile {
            path: "mods/first.jar".into(),
            provider: ModProvider::Modrinth,
            project_id: None,
            file_id: None,
            url: None,
            size: None,
            sha1: None,
            sha256: None,
            required: true,
        };
        let second = PackFile {
            path: "mods/second.jar".into(),
            ..first.clone()
        };
        let mut queue = ModpackDownloadQueue::new(&[first, second], &HashMap::new());
        queue.start("mods/second.jar");
        queue.start("mods/first.jar");
        let progress = queue.progress(0, 2);
        assert_eq!(
            progress.active_items,
            vec!["mods/second.jar", "mods/first.jar"]
        );
        assert!(progress.pending_items.is_empty());

        queue.finish("mods/second.jar");
        let progress = queue.progress(1, 2);
        assert_eq!(progress.current_item.as_deref(), Some("mods/first.jar"));
        assert_eq!(progress.active_items, vec!["mods/first.jar"]);
    }

    #[test]
    fn restores_completed_modpack_file_states() {
        let checkpoint = serde_json::json!({
            "modpackFiles": {
                "mods/done.jar": "imported",
                "mods/retry.jar": "failed"
            }
        });
        assert_eq!(
            resumed_file_status(Some(&checkpoint), "mods/done.jar"),
            Some(ModpackEntryStatus::Imported)
        );
        assert_eq!(
            resumed_file_status(Some(&checkpoint), "mods/retry.jar"),
            Some(ModpackEntryStatus::Failed)
        );
        assert_eq!(resumed_file_status(Some(&checkpoint), "mods/new.jar"), None);
    }
}
