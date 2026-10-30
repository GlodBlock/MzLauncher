use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::{CoreError, CoreResult};
use crate::model::{Instance, LoaderKind, ModDependency, ModFile, ModProvider, ReleaseChannel};

use super::{ModProject, ModProviderApi, ModSearchParams};

pub struct Modrinth {
    client: reqwest::Client,
    base: String,
}

impl Modrinth {
    pub fn new(base: String) -> CoreResult<Self> {
        Ok(Self {
            client: super::client()?,
            base: base.trim_end_matches('/').into(),
        })
    }
}

#[async_trait]
impl ModProviderApi for Modrinth {
    fn provider(&self) -> ModProvider {
        ModProvider::Modrinth
    }

    async fn search(&self, params: &ModSearchParams) -> CoreResult<Vec<ModProject>> {
        let project_type = params.project_type.as_str();
        let mut facets = vec![vec![format!("project_type:{project_type}")]];
        if let Some(version) = &params.game_version {
            facets.push(vec![format!(
                "versions:{}",
                version.replace(['\\', '"'], "")
            )]);
        }
        if let Some(loader) = params.loader {
            facets.push(vec![format!("categories:{}", super::loader_name(loader))]);
        }
        let response: SearchResponse = super::send_json(
            self.client.get(format!("{}/search", self.base)).query(&[
                ("query", params.query.clone()),
                ("limit", "20".to_owned()),
                ("facets", serde_json::to_string(&facets)?),
            ]),
            "modrinth",
        )
        .await?;
        Ok(response
            .hits
            .into_iter()
            .map(|hit| ModProject {
                provider: ModProvider::Modrinth,
                project_id: hit.project_id,
                name: hit.title,
                summary: hit.description,
                author: hit.author,
                icon_url: hit.icon_url,
                page_url: Some(format!("https://modrinth.com/{project_type}/{}", hit.slug)),
                downloads: hit.downloads,
                game_versions: hit.versions,
                loaders: hit
                    .categories
                    .iter()
                    .filter_map(|loader| super::parse_loader(loader))
                    .collect(),
            })
            .collect())
    }

    async fn files(&self, project_id: &str, instance: &Instance) -> CoreResult<Vec<ModFile>> {
        let url = format!("{}/project/{project_id}/version", self.base);
        let versions: Vec<VersionResponse> = super::send_json(
            self.client.get(url).query(&[
                (
                    "game_versions",
                    serde_json::to_string(&vec![&instance.game_version])?,
                ),
                (
                    "loaders",
                    serde_json::to_string(&vec![super::loader_name(instance.loader)])?,
                ),
            ]),
            "modrinth",
        )
        .await?;
        versions
            .into_iter()
            .map(into_mod_file)
            .collect::<CoreResult<Vec<_>>>()
    }

    async fn file(&self, _project_id: &str, file_id: &str) -> CoreResult<ModFile> {
        into_mod_file(
            super::send_json(
                self.client.get(format!("{}/version/{file_id}", self.base)),
                "modrinth",
            )
            .await?,
        )
    }

    async fn modpack_file(
        &self,
        project_id: &str,
        game_version: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> CoreResult<ModFile> {
        let mut request = self
            .client
            .get(format!("{}/project/{project_id}/version", self.base));
        if let Some(version) = game_version {
            request = request.query(&[("game_versions", serde_json::to_string(&vec![version])?)]);
        }
        if let Some(loader) = loader {
            request = request.query(&[(
                "loaders",
                serde_json::to_string(&vec![super::loader_name(loader)])?,
            )]);
        }
        let versions: Vec<VersionResponse> = super::send_json(request, "modrinth").await?;
        versions
            .into_iter()
            .map(into_pack_file)
            .collect::<CoreResult<Vec<_>>>()?
            .into_iter()
            .max_by(|left, right| left.published_at.cmp(&right.published_at))
            .ok_or_else(|| CoreError::NotFound("matching Modrinth modpack version".into()))
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    project_id: String,
    title: String,
    description: String,
    author: String,
    icon_url: Option<String>,
    slug: String,
    downloads: u64,
    #[serde(default)]
    versions: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    id: String,
    project_id: String,
    name: String,
    version_number: String,
    version_type: String,
    date_published: DateTime<Utc>,
    files: Vec<FileResponse>,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    dependencies: Vec<DependencyResponse>,
}

#[derive(Debug, Deserialize)]
struct FileResponse {
    url: String,
    filename: String,
    primary: bool,
    size: u64,
    hashes: HashResponse,
}

#[derive(Debug, Deserialize)]
struct HashResponse {
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DependencyResponse {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: String,
}

fn into_mod_file(version: VersionResponse) -> CoreResult<ModFile> {
    into_mod_file_kind(version, false)
}

fn into_pack_file(version: VersionResponse) -> CoreResult<ModFile> {
    into_mod_file_kind(version, true)
}

fn into_mod_file_kind(version: VersionResponse, modpack: bool) -> CoreResult<ModFile> {
    let file = version
        .files
        .iter()
        .find(|file| modpack && file.filename.ends_with(".mrpack"))
        .or_else(|| version.files.iter().find(|file| file.primary))
        .or(version.files.first())
        .ok_or_else(|| CoreError::Upstream {
            provider: "modrinth".into(),
            status: 200,
            message: "Modrinth version has no files".into(),
        })?;
    Ok(ModFile {
        provider: ModProvider::Modrinth,
        project_id: version.project_id,
        file_id: version.id,
        project_name: version.name,
        version_name: version.version_number,
        file_name: file.filename.clone(),
        url: Some(file.url.clone()),
        size: Some(file.size),
        sha1: file.hashes.sha1.clone(),
        sha256: None,
        game_versions: version.game_versions,
        loaders: version
            .loaders
            .iter()
            .filter_map(|loader| super::parse_loader(loader))
            .collect(),
        channel: match version.version_type.as_str() {
            "release" => ReleaseChannel::Release,
            "beta" => ReleaseChannel::Beta,
            _ => ReleaseChannel::Alpha,
        },
        dependencies: version
            .dependencies
            .into_iter()
            .filter_map(|dependency| {
                Some(ModDependency {
                    project_id: dependency.project_id?,
                    version_id: dependency.version_id,
                    required: dependency.dependency_type == "required",
                })
            })
            .collect(),
        published_at: Some(version.date_published),
        distribution_allowed: true,
    })
}
