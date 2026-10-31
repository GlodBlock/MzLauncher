use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::download::{DownloadSpec, Downloader};
use crate::error::{CoreError, CoreResult};
use crate::job::JobContext;
use crate::model::{LoaderKind, ModProvider};
use crate::modpack::{ModpackAnalysis, ModpackFormat, PackFile};
use crate::mods::{
    loader_name, ModProject, ModSearchParams, ModService, MODPACK_SEARCH_BATCH_SIZE,
};
use crate::paths::{safe_relative_path, AppPaths};

const FTB_CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_SEARCH_RESULTS: usize = 20;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackDownloadParams {
    pub provider: ModProvider,
    pub project_id: String,
    pub project_name: Option<String>,
    pub game_version: Option<String>,
    pub loader: Option<LoaderKind>,
    pub name: Option<String>,
    pub idempotency_key: Option<String>,
}

pub enum RemoteModpack {
    Archive { path: PathBuf, name: String },
    Prepared(ModpackAnalysis),
}

#[derive(Clone)]
pub struct ModpackBrowserService {
    paths: AppPaths,
    downloader: Downloader,
    mods: ModService,
    ftb: reqwest::Client,
    ftb_base: String,
    ftb_catalog: Arc<tokio::sync::Mutex<Option<(Instant, Vec<ModProject>)>>>,
}

impl ModpackBrowserService {
    pub fn new(
        paths: AppPaths,
        downloader: Downloader,
        mods: ModService,
        ftb_base: String,
    ) -> CoreResult<Self> {
        Ok(Self {
            paths,
            downloader,
            mods,
            ftb: crate::mods::client()?,
            ftb_base: ftb_base.trim_end_matches('/').into(),
            ftb_catalog: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    pub async fn search(&self, params: ModSearchParams) -> CoreResult<Vec<ModProject>> {
        if params.query.chars().count() > 200 {
            return Err(CoreError::InvalidInput(
                "Modpack search query is too long".into(),
            ));
        }
        match params.provider {
            ModProvider::Ftb => self.search_ftb(&params).await,
            ModProvider::Modrinth | ModProvider::CurseForge => {
                self.search_api_modpacks(params).await
            }
            _ => Err(CoreError::InvalidInput("invalid modpack provider".into())),
        }
    }

    async fn search_api_modpacks(
        &self,
        mut params: ModSearchParams,
    ) -> CoreResult<Vec<ModProject>> {
        let requested_offset = params.offset;
        let required = requested_offset.saturating_add(MAX_SEARCH_RESULTS + 1);
        let query = params.query.clone();
        let mut projects = Vec::new();
        let mut provider_offset = 0;
        loop {
            params.offset = provider_offset;
            let batch = self.mods.search_modpacks(params.clone()).await?;
            let received = batch.len();
            projects.extend(
                batch
                    .into_iter()
                    .filter(|project| title_matches(&project.name, &query)),
            );
            if projects.len() >= required || received < MODPACK_SEARCH_BATCH_SIZE {
                break;
            }
            provider_offset = provider_offset.saturating_add(received);
        }
        Ok(paginate_by_downloads(projects, requested_offset))
    }

    pub async fn fetch(
        &self,
        params: &ModpackDownloadParams,
        context: &JobContext,
    ) -> CoreResult<RemoteModpack> {
        validate_remote_id(&params.project_id)?;
        match params.provider {
            ModProvider::Ftb => self.fetch_ftb(params).await.map(RemoteModpack::Prepared),
            ModProvider::Modrinth | ModProvider::CurseForge => {
                let file = self
                    .mods
                    .modpack_file(
                        params.provider,
                        &params.project_id,
                        params.game_version.as_deref(),
                        params.loader,
                    )
                    .await?;
                if !file.distribution_allowed {
                    return Err(CoreError::Forbidden(
                        "this modpack does not allow automatic download".into(),
                    ));
                }
                let url = file.url.clone().ok_or_else(|| {
                    CoreError::Forbidden("modpack has no automatic download URL".into())
                })?;
                let extension = match params.provider {
                    ModProvider::Modrinth => "mrpack",
                    _ => "zip",
                };
                let provider = match params.provider {
                    ModProvider::Modrinth => "modrinth",
                    _ => "curseforge",
                };
                let path = self
                    .paths
                    .cache
                    .join("modpack-archives")
                    .join(provider)
                    .join(format!("{}.{}", file.file_id, extension));
                self.downloader
                    .download(
                        DownloadSpec {
                            url,
                            destination: path.clone(),
                            size: file.size,
                            sha1: file.sha1,
                            sha256: file.sha256,
                            provider: provider.into(),
                        },
                        Some(context),
                    )
                    .await?;
                let name = params
                    .project_name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(&file.project_name)
                    .to_owned();
                Ok(RemoteModpack::Archive { path, name })
            }
            _ => Err(CoreError::InvalidInput("invalid modpack provider".into())),
        }
    }

    async fn search_ftb(&self, params: &ModSearchParams) -> CoreResult<Vec<ModProject>> {
        let unfiltered = params.query.trim().is_empty()
            && params.game_version.is_none()
            && params.loader.is_none();
        if unfiltered {
            let limit = params.offset.saturating_add(MAX_SEARCH_RESULTS + 1);
            let response: FtbPackList = crate::mods::send_json(
                self.ftb.get(format!(
                    "{}/public/modpack/popular/plays/{limit}",
                    self.ftb_base
                )),
                "ftb",
            )
            .await?;
            let projects = self.ftb_projects(response.packs).await?;
            return Ok(projects
                .into_iter()
                .skip(params.offset)
                .take(MAX_SEARCH_RESULTS + 1)
                .collect());
        }

        let cached = self
            .ftb_catalog
            .lock()
            .await
            .as_ref()
            .and_then(|(loaded, packs)| (loaded.elapsed() < FTB_CACHE_TTL).then(|| packs.clone()));
        let projects = match cached {
            Some(projects) => projects,
            None => {
                let response: FtbPackList = crate::mods::send_json(
                    self.ftb
                        .get(format!("{}/public/modpack/all", self.ftb_base)),
                    "ftb",
                )
                .await?;
                let projects = self.ftb_projects(response.packs).await?;
                *self.ftb_catalog.lock().await = Some((Instant::now(), projects.clone()));
                projects
            }
        };
        let projects = projects
            .into_iter()
            .filter(|project| {
                title_matches(&project.name, &params.query)
                    && params
                        .game_version
                        .as_ref()
                        .is_none_or(|version| project.game_versions.contains(version))
                    && params
                        .loader
                        .is_none_or(|loader| project.loaders.contains(&loader))
            })
            .collect();
        Ok(paginate_by_downloads(projects, params.offset))
    }

    async fn ftb_projects(&self, ids: Vec<u64>) -> CoreResult<Vec<ModProject>> {
        let mut projects = Vec::new();
        let mut requests =
            futures_util::stream::iter(ids.into_iter().enumerate().map(|(index, id)| {
                let client = self.ftb.clone();
                let url = format!("{}/public/modpack/{id}", self.ftb_base);
                async move {
                    let project: FtbProject =
                        crate::mods::send_json(client.get(url), "ftb").await?;
                    Ok::<(usize, FtbProject), CoreError>((index, project))
                }
            }))
            .buffer_unordered(6);
        while let Some(result) = requests.next().await {
            let (index, project) = result?;
            projects.push((index, ftb_project(project)));
        }
        projects.sort_by_key(|(index, _)| *index);
        Ok(projects.into_iter().map(|(_, project)| project).collect())
    }

    async fn fetch_ftb(&self, params: &ModpackDownloadParams) -> CoreResult<ModpackAnalysis> {
        let project_id = params
            .project_id
            .parse::<u64>()
            .map_err(|_| CoreError::InvalidInput("invalid FTB project id".into()))?;
        let project: FtbProject = crate::mods::send_json(
            self.ftb
                .get(format!("{}/public/modpack/{project_id}", self.ftb_base)),
            "ftb",
        )
        .await?;
        let version = project
            .versions
            .iter()
            .filter(|version| {
                !version.private
                    && ftb_version_matches(version, params.game_version.as_deref(), params.loader)
            })
            .max_by_key(|version| version.updated)
            .ok_or_else(|| CoreError::NotFound("matching FTB modpack version".into()))?;
        let url = format!(
            "{}/public/modpack/{project_id}/{}",
            self.ftb_base, version.id
        );
        let text = self.downloader.fetch_text(&url, "ftb").await?;
        let source_sha256 = hex::encode(Sha256::digest(text.as_bytes()));
        let manifest: FtbVersionManifest = decode_ftb(&text)?;
        let game_version = manifest
            .targets
            .iter()
            .find(|target| target.kind == "game")
            .map(|target| target.version.clone())
            .ok_or_else(|| CoreError::InvalidInput("FTB pack has no Minecraft version".into()))?;
        let loader_target = manifest
            .targets
            .iter()
            .find(|target| target.kind == "modloader");
        let loader = loader_target
            .and_then(|target| crate::mods::parse_loader(&target.name))
            .unwrap_or(LoaderKind::Vanilla);
        let loader_version = loader_target.map(|target| target.version.clone());
        if loader != LoaderKind::Vanilla && loader_version.is_none() {
            return Err(CoreError::InvalidInput(
                "FTB pack has no loader version".into(),
            ));
        }
        if manifest.files.len() > crate::modpack::MAX_ENTRIES {
            return Err(CoreError::InvalidInput(
                "FTB pack has too many files".into(),
            ));
        }
        let mut total_size = 0_u64;
        let mut files = Vec::new();
        let mut warnings = Vec::new();
        for file in manifest.files {
            let path = std::path::Path::new(&file.path)
                .join(&file.name)
                .to_string_lossy()
                .replace('\\', "/");
            safe_relative_path(&path)?;
            if file.server_only {
                continue;
            }
            if file.optional {
                warnings.push(format!("optional file not installed: {path}"));
                continue;
            }
            if file.size > crate::modpack::MAX_ENTRY_SIZE {
                return Err(CoreError::InvalidInput(format!(
                    "FTB pack entry is too large: {path}"
                )));
            }
            total_size = total_size.saturating_add(file.size);
            if total_size > crate::modpack::MAX_TOTAL_SIZE {
                return Err(CoreError::InvalidInput(
                    "FTB pack uncompressed size exceeds the limit".into(),
                ));
            }
            let sha1 = file.sha1.or(file.hashes.sha1);
            let sha256 = file.hashes.sha256;
            files.push(PackFile {
                file_id: sha1.clone().or_else(|| sha256.clone()),
                path,
                provider: ModProvider::Modpack,
                project_id: None,
                url: Some(file.url),
                size: Some(file.size),
                sha1,
                sha256,
                required: true,
            });
        }
        let mod_count = files
            .iter()
            .filter(|file| file.path.starts_with("mods/") && file.path.ends_with(".jar"))
            .count();
        Ok(ModpackAnalysis {
            format: ModpackFormat::Ftb,
            name: project.name,
            version: Some(manifest.name),
            author: project.authors.first().map(|author| author.name.clone()),
            game_version,
            loader,
            loader_version,
            mod_count,
            override_prefixes: Vec::new(),
            warnings,
            source_sha256,
            files,
        })
    }
}

fn title_matches(title: &str, query: &str) -> bool {
    let title = title.to_lowercase();
    query
        .split_whitespace()
        .all(|term| title.contains(&term.to_lowercase()))
}

fn paginate_by_downloads(mut projects: Vec<ModProject>, offset: usize) -> Vec<ModProject> {
    projects.sort_by(|left, right| {
        right
            .downloads
            .cmp(&left.downloads)
            .then_with(|| left.project_id.cmp(&right.project_id))
    });
    projects
        .into_iter()
        .skip(offset)
        .take(MAX_SEARCH_RESULTS + 1)
        .collect()
}

fn ftb_project(project: FtbProject) -> ModProject {
    let mut game_versions = project
        .versions
        .iter()
        .flat_map(|version| version.targets.iter())
        .filter(|target| target.kind == "game")
        .map(|target| target.version.clone())
        .collect::<Vec<_>>();
    game_versions.sort();
    game_versions.dedup();
    let mut loaders = project
        .versions
        .iter()
        .flat_map(|version| version.targets.iter())
        .filter(|target| target.kind == "modloader")
        .filter_map(|target| crate::mods::parse_loader(&target.name))
        .collect::<Vec<_>>();
    loaders.sort_by_key(|loader| loader_name(*loader));
    loaders.dedup();
    ModProject {
        provider: ModProvider::Ftb,
        project_id: project.id.to_string(),
        name: project.name,
        summary: project.synopsis,
        author: project
            .authors
            .first()
            .map(|author| author.name.clone())
            .unwrap_or_default(),
        icon_url: project
            .art
            .iter()
            .find(|art| art.kind == "square")
            .map(|art| art.url.clone()),
        page_url: Some(format!(
            "https://www.feed-the-beast.com/modpack/{}",
            project.id
        )),
        downloads: project.installs,
        game_versions,
        loaders,
    }
}

fn ftb_version_matches(
    version: &FtbVersion,
    game_version: Option<&str>,
    loader: Option<LoaderKind>,
) -> bool {
    version.targets.iter().any(|target| {
        target.kind == "game" && game_version.is_none_or(|version| target.version == version)
    }) && loader.is_none_or(|loader| {
        version.targets.iter().any(|target| {
            target.kind == "modloader" && crate::mods::parse_loader(&target.name) == Some(loader)
        })
    })
}

pub(crate) fn validate_remote_id(value: &str) -> CoreResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(CoreError::InvalidInput("invalid modpack project id".into()));
    }
    Ok(())
}

fn decode_ftb<T: serde::de::DeserializeOwned>(text: &str) -> CoreResult<T> {
    serde_json::from_str(text).map_err(|error| CoreError::Upstream {
        provider: "ftb".into(),
        status: 200,
        message: format!("invalid provider response: {error}"),
    })
}

#[derive(Debug, Deserialize)]
struct FtbPackList {
    packs: Vec<u64>,
}

#[derive(Debug, Deserialize)]
struct FtbProject {
    id: u64,
    name: String,
    #[serde(default)]
    synopsis: String,
    #[serde(default)]
    authors: Vec<FtbAuthor>,
    #[serde(default)]
    art: Vec<FtbArt>,
    #[serde(default)]
    installs: u64,
    #[serde(default)]
    versions: Vec<FtbVersion>,
}

#[derive(Debug, Deserialize)]
struct FtbAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
struct FtbArt {
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct FtbVersion {
    id: u64,
    #[serde(default)]
    updated: u64,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    targets: Vec<FtbTarget>,
}

#[derive(Debug, Deserialize)]
struct FtbTarget {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct FtbVersionManifest {
    name: String,
    targets: Vec<FtbTarget>,
    files: Vec<FtbFile>,
}

#[derive(Debug, Deserialize)]
struct FtbFile {
    path: String,
    name: String,
    url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    hashes: FtbHashes,
    #[serde(default, rename = "serveronly")]
    server_only: bool,
    #[serde(default)]
    optional: bool,
}

#[derive(Debug, Default, Deserialize)]
struct FtbHashes {
    sha1: Option<String>,
    sha256: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: usize, downloads: u64) -> ModProject {
        ModProject {
            provider: ModProvider::Ftb,
            project_id: id.to_string(),
            name: format!("Pack {id}"),
            summary: String::new(),
            author: String::new(),
            icon_url: None,
            page_url: None,
            downloads,
            game_versions: Vec::new(),
            loaders: Vec::new(),
        }
    }

    #[test]
    fn title_search_is_case_insensitive_and_requires_every_term() {
        assert!(title_matches("All the Mods 10", "all mods"));
        assert!(title_matches("Create Above and Beyond", "CREATE beyond"));
        assert!(title_matches("Any Pack", "   "));
        assert!(!title_matches("All the Mods 10", "all missing"));
        assert!(!title_matches("Another Pack", "all"));
    }

    #[test]
    fn pagination_sorts_by_downloads_and_keeps_one_probe_item() {
        let projects = (0..45).map(|index| project(index, index as u64)).collect();
        let first = paginate_by_downloads(projects, 0);
        assert_eq!(first.len(), 21);
        assert_eq!(first[0].downloads, 44);
        assert_eq!(first[20].downloads, 24);

        let projects = (0..45).map(|index| project(index, index as u64)).collect();
        let second = paginate_by_downloads(projects, 20);
        assert_eq!(second.len(), 21);
        assert_eq!(second[0].downloads, 24);

        let projects = (0..45).map(|index| project(index, index as u64)).collect();
        let last = paginate_by_downloads(projects, 40);
        assert_eq!(last.len(), 5);
        assert_eq!(last[0].downloads, 4);
    }
}
