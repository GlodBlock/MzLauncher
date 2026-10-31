use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use reqwest::header::{
    CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE, RETRY_AFTER,
};
use reqwest::{StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Semaphore};

use crate::config::DownloadConfig;
use crate::error::{CoreError, CoreResult};
use crate::job::JobContext;
use crate::model::JobProgress;
use crate::paths::atomic_write;
use crate::CORE_VERSION;

const MAX_JSON_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct DownloadSpec {
    pub url: String,
    pub destination: PathBuf,
    pub size: Option<u64>,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    pub path: PathBuf,
    pub bytes: u64,
    pub reused: bool,
}

#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
    config: DownloadConfig,
    semaphore: Arc<Semaphore>,
    target_locks: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialMetadata {
    url: String,
    size: Option<u64>,
    sha1: Option<String>,
    sha256: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl Downloader {
    pub fn new(config: DownloadConfig) -> CoreResult<Self> {
        config_validation(&config)?;
        let mut builder = reqwest::Client::builder()
            .user_agent(format!("MzLauncher-Core/{CORE_VERSION}"))
            .connect_timeout(Duration::from_secs(config.connect_timeout_seconds))
            .redirect(reqwest::redirect::Policy::limited(10))
            .https_only(false);
        if let Some(proxy) = &config.proxy_url {
            builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|error| {
                CoreError::Configuration(format!("invalid proxy configuration: {error}"))
            })?);
        }
        Ok(Self {
            client: builder
                .build()
                .map_err(|error| CoreError::Configuration(error.to_string()))?,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent)),
            target_locks: Arc::new(Mutex::new(HashMap::new())),
            config,
        })
    }

    pub async fn fetch_json<T: DeserializeOwned>(
        &self,
        url: &str,
        provider: &str,
    ) -> CoreResult<T> {
        validate_url(url)?;
        let response = self.client.get(url).send().await.map_err(network_error)?;
        let response = check_response(response, provider).await?;
        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|size| size > MAX_JSON_BYTES)
        {
            return Err(CoreError::Upstream {
                provider: provider.into(),
                status: StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
                message: "metadata response is too large".into(),
            });
        }
        let bytes = response.bytes().await.map_err(network_error)?;
        if bytes.len() > MAX_JSON_BYTES {
            return Err(CoreError::Upstream {
                provider: provider.into(),
                status: StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
                message: "metadata response is too large".into(),
            });
        }
        serde_json::from_slice(&bytes).map_err(|error| CoreError::Upstream {
            provider: provider.into(),
            status: StatusCode::OK.as_u16(),
            message: format!("invalid metadata: {error}"),
        })
    }

    pub async fn fetch_bytes(
        &self,
        url: &str,
        provider: &str,
        max_bytes: usize,
    ) -> CoreResult<Vec<u8>> {
        validate_url(url)?;
        let response = self.client.get(url).send().await.map_err(network_error)?;
        let response = check_response(response, provider).await?;
        if response
            .content_length()
            .is_some_and(|size| size > max_bytes as u64)
        {
            return Err(CoreError::Upstream {
                provider: provider.into(),
                status: StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
                message: "response is too large".into(),
            });
        }
        let bytes = response.bytes().await.map_err(network_error)?;
        if bytes.len() > max_bytes {
            return Err(CoreError::Upstream {
                provider: provider.into(),
                status: StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
                message: "response is too large".into(),
            });
        }
        Ok(bytes.to_vec())
    }

    pub async fn fetch_text(&self, url: &str, provider: &str) -> CoreResult<String> {
        let bytes = self.fetch_bytes(url, provider, MAX_JSON_BYTES).await?;
        String::from_utf8(bytes).map_err(|_| CoreError::Upstream {
            provider: provider.into(),
            status: StatusCode::OK.as_u16(),
            message: "metadata is not valid UTF-8".into(),
        })
    }

    pub async fn download(
        &self,
        spec: DownloadSpec,
        context: Option<&JobContext>,
    ) -> CoreResult<DownloadOutcome> {
        validate_url(&spec.url)?;
        validate_hashes(&spec)?;
        if let Some(parent) = spec.destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if file_matches(&spec.destination, &spec).await? {
            return Ok(DownloadOutcome {
                bytes: tokio::fs::metadata(&spec.destination).await?.len(),
                path: spec.destination,
                reused: true,
            });
        }

        let target_lock = {
            let mut locks = self.target_locks.lock().await;
            locks
                .entry(spec.destination.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _target_guard = target_lock.lock().await;
        if file_matches(&spec.destination, &spec).await? {
            return Ok(DownloadOutcome {
                bytes: tokio::fs::metadata(&spec.destination).await?.len(),
                path: spec.destination,
                reused: true,
            });
        }
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| CoreError::Internal("download scheduler is unavailable".into()))?;
        let partial = partial_path(&spec.destination);
        let metadata_path = partial.with_extension("json");
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            check_cancelled(context)?;
            if attempt > 0 {
                let delay = retry_delay(attempt, last_error.as_ref());
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = cancel_wait(context) => return Err(CoreError::Cancelled),
                }
            }
            match self
                .download_attempt(&spec, &partial, &metadata_path, context)
                .await
            {
                Ok(()) => {
                    if !file_matches(&partial, &spec).await? {
                        let _ = tokio::fs::remove_file(&partial).await;
                        return Err(CoreError::Checksum {
                            path: spec.destination.display().to_string(),
                        });
                    }
                    let bytes = tokio::fs::metadata(&partial).await?.len();
                    replace_file(partial, spec.destination.clone()).await?;
                    let _ = tokio::fs::remove_file(&metadata_path).await;
                    return Ok(DownloadOutcome {
                        path: spec.destination,
                        bytes,
                        reused: false,
                    });
                }
                Err(error) if error.retryable() && attempt < self.config.max_retries => {
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| CoreError::Network("download failed".into())))
    }

    async fn download_attempt(
        &self,
        spec: &DownloadSpec,
        partial: &Path,
        metadata_path: &Path,
        context: Option<&JobContext>,
    ) -> CoreResult<()> {
        let previous = read_partial_metadata(metadata_path).await;
        let mut offset = if previous.as_ref().is_some_and(|meta| {
            meta.url == spec.url
                && meta.size == spec.size
                && meta.sha1 == spec.sha1
                && meta.sha256 == spec.sha256
        }) {
            tokio::fs::metadata(partial)
                .await
                .map(|meta| meta.len())
                .unwrap_or(0)
        } else {
            0
        };
        if spec.size.is_some_and(|size| offset > size) {
            offset = 0;
        }
        let mut request = self.client.get(&spec.url);
        if offset > 0 {
            request = request.header(RANGE, format!("bytes={offset}-"));
            if let Some(previous) = &previous {
                if let Some(validator) = previous.etag.as_ref().or(previous.last_modified.as_ref())
                {
                    request = request.header(IF_RANGE, validator);
                }
            }
        }
        let response = request.send().await.map_err(network_error)?;
        if matches!(
            response.status(),
            StatusCode::RANGE_NOT_SATISFIABLE | StatusCode::NOT_FOUND
        ) && offset > 0
        {
            if response.status() == StatusCode::RANGE_NOT_SATISFIABLE
                && spec.size == Some(offset)
                && file_matches(partial, spec).await?
            {
                return Ok(());
            }
            reset_partial(partial).await?;
            return Err(CoreError::Network(
                "server rejected the resume range".into(),
            ));
        }
        let mut response = check_response(response, &spec.provider).await?;
        if response.status() == StatusCode::PARTIAL_CONTENT
            && !valid_content_range(
                header(&response, CONTENT_RANGE).as_deref(),
                offset,
                spec.size,
                response.content_length(),
            )
        {
            reset_partial(partial).await?;
            return Err(CoreError::Network(
                "server returned an inconsistent content range".into(),
            ));
        }
        if offset > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
            offset = 0;
            reset_partial(partial).await?;
        }
        if offset == 0 && response.status() == StatusCode::PARTIAL_CONTENT {
            reset_partial(partial).await?;
        }
        if offset == 0 {
            let metadata = PartialMetadata {
                url: spec.url.clone(),
                size: spec.size,
                sha1: spec.sha1.clone(),
                sha256: spec.sha256.clone(),
                etag: header(&response, ETAG),
                last_modified: header(&response, LAST_MODIFIED),
            };
            atomic_write(metadata_path, &serde_json::to_vec(&metadata)?)?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .open(partial)
            .await?;
        let total = spec.size.or_else(|| {
            response
                .content_length()
                .and_then(|length| length.checked_add(offset))
        });
        loop {
            let next = tokio::select! {
                result = tokio::time::timeout(
                    Duration::from_secs(self.config.read_timeout_seconds),
                    response.chunk(),
                ) => result,
                _ = cancel_wait(context) => return Err(CoreError::Cancelled),
            };
            let Some(chunk) = next
                .map_err(|_| CoreError::Network("download read timed out".into()))?
                .map_err(network_error)?
            else {
                break;
            };
            file.write_all(&chunk).await?;
            offset = offset
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| CoreError::InvalidInput("download size overflow".into()))?;
            if spec.size.is_some_and(|expected| offset > expected) {
                return Err(CoreError::Checksum {
                    path: spec.destination.display().to_string(),
                });
            }
            if let Some(context) = context {
                context.download_progress(JobProgress {
                    stage: "downloading".into(),
                    completed: offset,
                    total,
                    current_item: Some(spec.destination.display().to_string()),
                    pending_items: Vec::new(),
                })?;
            }
        }
        file.flush().await?;
        file.sync_all().await?;
        Ok(())
    }
}

async fn check_response(
    response: reqwest::Response,
    provider: &str,
) -> CoreResult<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let retry_after = header(&response, RETRY_AFTER);
    let message = if status == StatusCode::TOO_MANY_REQUESTS {
        format!(
            "rate limited; retry after {}",
            retry_after.unwrap_or_else(|| "unspecified".into())
        )
    } else {
        format!("HTTP {}", status.as_u16())
    };
    Err(CoreError::Upstream {
        provider: provider.into(),
        status: status.as_u16(),
        message,
    })
}

fn header(response: &reqwest::Response, name: reqwest::header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn valid_content_range(
    value: Option<&str>,
    offset: u64,
    expected_size: Option<u64>,
    response_length: Option<u64>,
) -> bool {
    let Some(value) = value else {
        return false;
    };
    let Some(range) = value.strip_prefix("bytes ") else {
        return false;
    };
    let Some((bounds, total)) = range.split_once('/') else {
        return false;
    };
    let Some((start, end)) = bounds.split_once('-') else {
        return false;
    };
    let (Ok(start), Ok(end)) = (start.parse::<u64>(), end.parse::<u64>()) else {
        return false;
    };
    if start != offset || end < start {
        return false;
    }
    if response_length.is_some_and(|length| length != end - start + 1) {
        return false;
    }
    if total != "*" {
        let Ok(total) = total.parse::<u64>() else {
            return false;
        };
        if expected_size.is_some_and(|expected| expected != total) || end >= total {
            return false;
        }
    }
    true
}

fn network_error(error: reqwest::Error) -> CoreError {
    CoreError::Network(error.without_url().to_string())
}

fn validate_url(value: &str) -> CoreResult<Url> {
    let url =
        Url::parse(value).map_err(|_| CoreError::InvalidInput("invalid download URL".into()))?;
    let loopback = url
        .host_str()
        .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(CoreError::InvalidInput(
            "downloads must use HTTPS (except loopback development endpoints)".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CoreError::InvalidInput(
            "download URLs cannot contain embedded credentials".into(),
        ));
    }
    Ok(url)
}

fn validate_hashes(spec: &DownloadSpec) -> CoreResult<()> {
    for (hash, length) in [(&spec.sha1, 40), (&spec.sha256, 64)] {
        if let Some(hash) = hash {
            if hash.len() != length || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(CoreError::InvalidInput(
                    "download contains an invalid expected hash".into(),
                ));
            }
        }
    }
    Ok(())
}

async fn file_matches(path: &Path, spec: &DownloadSpec) -> CoreResult<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let path = path.to_path_buf();
    let spec = spec.clone();
    tokio::task::spawn_blocking(move || file_matches_sync(&path, &spec))
        .await
        .map_err(|error| CoreError::Internal(error.to_string()))?
}

fn file_matches_sync(path: &Path, spec: &DownloadSpec) -> CoreResult<bool> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() || spec.size.is_some_and(|size| metadata.len() != size) {
        return Ok(false);
    }
    if spec.sha1.is_none() && spec.sha256.is_none() {
        return Ok(true);
    }
    let mut file = std::fs::File::open(path)?;
    let mut sha1 = spec.sha1.as_ref().map(|_| Sha1::new());
    let mut sha256 = spec.sha256.as_ref().map(|_| Sha256::new());
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        use std::io::Read;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if let Some(hash) = &mut sha1 {
            hash.update(&buffer[..read]);
        }
        if let Some(hash) = &mut sha256 {
            hash.update(&buffer[..read]);
        }
    }
    if let (Some(actual), Some(expected)) = (sha1, &spec.sha1) {
        if !hex::encode(actual.finalize()).eq_ignore_ascii_case(expected) {
            return Ok(false);
        }
    }
    if let (Some(actual), Some(expected)) = (sha256, &spec.sha256) {
        if !hex::encode(actual.finalize()).eq_ignore_ascii_case(expected) {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn read_partial_metadata(path: &Path) -> Option<PartialMetadata> {
    serde_json::from_slice(&tokio::fs::read(path).await.ok()?).ok()
}

async fn reset_partial(path: &Path) -> CoreResult<()> {
    if path.exists() {
        tokio::fs::remove_file(path).await?;
    }
    Ok(())
}

fn partial_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    destination.with_file_name(format!(".{name}.part"))
}

pub async fn replace_file(source: PathBuf, destination: PathBuf) -> CoreResult<()> {
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            let source = source
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let destination = destination
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let result = unsafe {
                MoveFileExW(
                    source.as_ptr(),
                    destination.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if result == 0 {
                return Err(CoreError::Io(std::io::Error::last_os_error()));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            std::fs::rename(source, destination)?;
            Ok(())
        }
    })
    .await
    .map_err(|error| CoreError::Internal(error.to_string()))?
}

async fn cancel_wait(context: Option<&JobContext>) {
    match context {
        Some(context) => context.cancelled().await,
        None => std::future::pending().await,
    }
}

fn check_cancelled(context: Option<&JobContext>) -> CoreResult<()> {
    context.map_or(Ok(()), JobContext::check_cancelled)
}

fn retry_delay(attempt: u32, error: Option<&CoreError>) -> Duration {
    if let Some(CoreError::Upstream { message, .. }) = error {
        if let Some(seconds) = message
            .split("retry after ")
            .nth(1)
            .and_then(|value| value.parse::<u64>().ok())
        {
            return Duration::from_secs(seconds.min(120));
        }
    }
    let base = 500_u64.saturating_mul(1_u64 << attempt.min(5));
    Duration::from_millis(base + rand::thread_rng().gen_range(0..250))
}

fn config_validation(config: &DownloadConfig) -> CoreResult<()> {
    if !(1..=32).contains(&config.max_concurrent) || config.read_timeout_seconds == 0 {
        return Err(CoreError::Configuration("invalid download settings".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verifies_existing_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("content.bin");
        tokio::fs::write(&path, b"hello").await.unwrap();
        let spec = DownloadSpec {
            url: "https://example.com/content".into(),
            destination: path,
            size: Some(5),
            sha1: Some("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".into()),
            sha256: None,
            provider: "test".into(),
        };
        assert!(file_matches(&spec.destination, &spec).await.unwrap());
    }

    #[test]
    fn rejects_non_https_remote_content() {
        assert!(validate_url("http://example.com/file").is_err());
        assert!(validate_url("http://127.0.0.1:8080/file").is_ok());
    }

    #[test]
    fn validates_content_range_against_local_offset() {
        assert!(valid_content_range(
            Some("bytes 6-10/11"),
            6,
            Some(11),
            Some(5),
        ));
        assert!(!valid_content_range(
            Some("bytes 5-10/11"),
            6,
            Some(11),
            Some(5),
        ));
        assert!(!valid_content_range(
            Some("bytes 6-10/12"),
            6,
            Some(11),
            Some(5),
        ));
        assert!(!valid_content_range(None, 6, Some(11), Some(5)));
    }
}
