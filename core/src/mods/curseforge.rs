use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::credentials::CredentialStore;
use crate::error::{CoreError, CoreResult};
use crate::model::{Instance, LoaderKind, ModDependency, ModFile, ModProvider, ReleaseChannel};

use super::{CurseForgeProjectInfo, ModProject, ModProviderApi, ModSearchParams};

const PROJECT_NAME_BATCH_SIZE: usize = 50;

pub struct CurseForge {
    client: reqwest::Client,
    base: String,
    credentials: CredentialStore,
    downloader: crate::download::Downloader,
}

impl CurseForge {
    pub fn new(base: String, downloader: crate::download::Downloader) -> CoreResult<Self> {
        Ok(Self {
            client: super::client()?,
            base: base.trim_end_matches('/').into(),
            credentials: CredentialStore::new(),
            downloader,
        })
    }

    pub async fn changelog(&self, project_id: &str, file_id: &str) -> CoreResult<String> {
        let response: DataResponse<String> = super::send_json(
            self.request(
                reqwest::Method::GET,
                &format!("/mods/{project_id}/files/{file_id}/changelog"),
            )?,
            "curseforge",
            self.downloader.max_retries(),
        )
        .await?;
        Ok(plain_text_changelog(&response.data))
    }

    pub async fn projects(
        &self,
        project_ids: &[String],
    ) -> CoreResult<HashMap<String, CurseForgeProjectInfo>> {
        let ids = project_ids
            .iter()
            .filter_map(|id| id.parse::<u64>().ok())
            .collect::<Vec<_>>();
        let mut projects = HashMap::new();
        for ids in ids.chunks(PROJECT_NAME_BATCH_SIZE) {
            let response: DataResponse<Vec<ProjectResponse>> = super::send_json(
                self.request(reqwest::Method::POST, "/mods")?
                    .json(&serde_json::json!({"modIds": ids})),
                "curseforge",
                self.downloader.max_retries(),
            )
            .await?;
            projects.extend(response.data.into_iter().map(|project| {
                (
                    project.id.to_string(),
                    CurseForgeProjectInfo {
                        name: project.name,
                        class_id: project.class_id,
                    },
                )
            }));
        }
        Ok(projects)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> CoreResult<reqwest::RequestBuilder> {
        let key = std::env::var("MZLAUNCHER_CURSEFORGE_API_KEY")
            .ok()
            .filter(|key| !key.is_empty())
            .or_else(|| self.credentials.get("provider-curseforge").ok())
            .or_else(crate::secret::builtin_curseforge_api_key)
            .ok_or_else(|| {
                CoreError::Configuration(
                    "CurseForge requires MZLAUNCHER_CURSEFORGE_API_KEY or a key stored in the system keyring"
                        .into(),
                )
            })?;
        Ok(self
            .client
            .request(method, format!("{}{path}", self.base))
            .header("x-api-key", key)
            .header("Accept", "application/json"))
    }
}

#[async_trait]
impl ModProviderApi for CurseForge {
    fn provider(&self) -> ModProvider {
        ModProvider::CurseForge
    }

    async fn search(&self, params: &ModSearchParams) -> CoreResult<Vec<ModProject>> {
        let class_id = match params.project_type {
            super::ModProjectType::Mod => "6",
            super::ModProjectType::Modpack => "4471",
        };
        let page_size = if params.project_type == super::ModProjectType::Modpack {
            super::MODPACK_SEARCH_BATCH_SIZE
        } else {
            20
        };
        let mut request = self.request(reqwest::Method::GET, "/mods/search")?.query(&[
            ("gameId", "432".to_owned()),
            ("classId", class_id.to_owned()),
            ("searchFilter", params.query.clone()),
            ("index", params.offset.to_string()),
            ("pageSize", page_size.to_string()),
        ]);
        if params.project_type == super::ModProjectType::Modpack {
            request = request.query(&[("sortField", "6"), ("sortOrder", "desc")]);
        }
        if let Some(version) = &params.game_version {
            request = request.query(&[("gameVersion", version)]);
        }
        if let Some(loader) = params.loader {
            request = request.query(&[("modLoaderType", loader_type(loader).to_string())]);
        }
        let response: DataResponse<Vec<ProjectResponse>> =
            super::send_json(request, "curseforge", self.downloader.max_retries()).await?;
        Ok(response
            .data
            .into_iter()
            .map(|project| {
                let mut game_versions = project
                    .latest_files
                    .iter()
                    .flat_map(|file| file.game_versions.iter())
                    .filter(|version| {
                        version
                            .chars()
                            .next()
                            .is_some_and(|character| character.is_ascii_digit())
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                game_versions.sort();
                game_versions.dedup();
                let mut loaders = project
                    .latest_files
                    .iter()
                    .flat_map(|file| file.game_versions.iter())
                    .filter_map(|value| super::parse_loader(&value.to_ascii_lowercase()))
                    .collect::<Vec<_>>();
                loaders.sort_by_key(|loader| super::loader_name(*loader));
                loaders.dedup();
                ModProject {
                    provider: ModProvider::CurseForge,
                    project_id: project.id.to_string(),
                    name: project.name,
                    summary: project.summary,
                    author: project
                        .authors
                        .first()
                        .map(|author| author.name.clone())
                        .unwrap_or_default(),
                    icon_url: project.logo.and_then(|logo| logo.thumbnail_url),
                    page_url: project.links.and_then(|links| links.website_url),
                    downloads: project.download_count,
                    game_versions,
                    loaders,
                }
            })
            .collect())
    }

    async fn files(&self, project_id: &str, instance: &Instance) -> CoreResult<Vec<ModFile>> {
        let response: DataResponse<Vec<FileResponse>> = super::send_json(
            self.request(reqwest::Method::GET, &format!("/mods/{project_id}/files"))?
                .query(&[("gameVersion", instance.game_version.as_str())]),
            "curseforge",
            self.downloader.max_retries(),
        )
        .await?;
        response.data.into_iter().map(into_mod_file).collect()
    }

    async fn file(&self, project_id: &str, file_id: &str) -> CoreResult<ModFile> {
        let response: DataResponse<FileResponse> = super::send_json(
            self.request(
                reqwest::Method::GET,
                &format!("/mods/{project_id}/files/{file_id}"),
            )?,
            "curseforge",
            self.downloader.max_retries(),
        )
        .await?;
        into_mod_file(response.data)
    }

    async fn modpack_files(
        &self,
        project_id: &str,
        game_version: Option<&str>,
        loader: Option<LoaderKind>,
    ) -> CoreResult<Vec<ModFile>> {
        let mut request =
            self.request(reqwest::Method::GET, &format!("/mods/{project_id}/files"))?;
        if let Some(version) = game_version {
            request = request.query(&[("gameVersion", version)]);
            if let Some(loader) = loader {
                request = request.query(&[("modLoaderType", loader_type(loader))]);
            }
        }
        let response: DataResponse<Vec<FileResponse>> =
            super::send_json(request, "curseforge", self.downloader.max_retries()).await?;
        response.data.into_iter().map(into_mod_file).collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectResponse {
    id: u64,
    name: String,
    #[serde(default)]
    class_id: u64,
    summary: String,
    authors: Vec<AuthorResponse>,
    logo: Option<LogoResponse>,
    links: Option<LinksResponse>,
    download_count: u64,
    #[serde(default)]
    latest_files: Vec<FileResponse>,
}

#[derive(Debug, Deserialize)]
struct AuthorResponse {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogoResponse {
    thumbnail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinksResponse {
    website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileResponse {
    id: u64,
    mod_id: u64,
    display_name: String,
    file_name: String,
    download_url: Option<String>,
    release_type: u8,
    file_date: DateTime<Utc>,
    file_length: u64,
    hashes: Vec<HashResponse>,
    game_versions: Vec<String>,
    dependencies: Vec<DependencyResponse>,
}

#[derive(Debug, Deserialize)]
struct HashResponse {
    value: String,
    algo: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DependencyResponse {
    mod_id: u64,
    relation_type: u8,
}

fn into_mod_file(file: FileResponse) -> CoreResult<ModFile> {
    let sha1 = file
        .hashes
        .iter()
        .find(|hash| hash.algo == 1)
        .map(|hash| hash.value.clone());
    Ok(ModFile {
        provider: ModProvider::CurseForge,
        project_id: file.mod_id.to_string(),
        file_id: file.id.to_string(),
        project_name: file.display_name.clone(),
        version_name: file.display_name,
        file_name: file.file_name,
        url: file.download_url.clone(),
        size: Some(file.file_length),
        sha1,
        sha256: None,
        loaders: file
            .game_versions
            .iter()
            .filter_map(|value| super::parse_loader(&value.to_ascii_lowercase()))
            .collect(),
        game_versions: file.game_versions,
        channel: match file.release_type {
            1 => ReleaseChannel::Release,
            2 => ReleaseChannel::Beta,
            _ => ReleaseChannel::Alpha,
        },
        dependencies: file
            .dependencies
            .into_iter()
            .map(|dependency| ModDependency {
                project_id: dependency.mod_id.to_string(),
                version_id: None,
                required: dependency.relation_type == 3,
            })
            .collect(),
        published_at: Some(file.file_date),
        distribution_allowed: file.download_url.is_some(),
    })
}

fn plain_text_changelog(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('<') {
        output.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            output.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let raw_tag = rest[start + 1..start + end].trim();
        let closing = raw_tag.starts_with('/');
        let tag = raw_tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(
            tag.as_str(),
            "br" | "p" | "div" | "ul" | "ol" | "h1" | "h2" | "h3"
        ) || (tag == "li" && !closing)
        {
            output.push('\n');
        }
        rest = &rest[start + end + 1..];
    }
    output.push_str(rest);
    let text = output
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let mut normalized = String::with_capacity(text.len());
    let mut previous_blank = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            if previous_blank {
                continue;
            }
            previous_blank = true;
        } else {
            previous_blank = false;
        }
        normalized.push_str(line);
        normalized.push('\n');
    }
    normalized.trim().to_owned()
}

fn loader_type(loader: LoaderKind) -> &'static str {
    match loader {
        LoaderKind::Forge => "1",
        LoaderKind::Fabric => "4",
        LoaderKind::Quilt => "5",
        LoaderKind::NeoForge => "6",
        LoaderKind::Vanilla => "0",
    }
}
