use std::fs;
use std::path::{Component, Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
#[cfg(not(windows))]
use directories::ProjectDirs;
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub instances: PathBuf,
    pub content: PathBuf,
    pub runtimes: PathBuf,
    pub temp: PathBuf,
    pub logs: PathBuf,
    pub cache: PathBuf,
}

impl AppPaths {
    pub fn discover(override_root: Option<PathBuf>) -> CoreResult<Self> {
        let root = match override_root {
            Some(path) => path,
            None => default_root()?,
        };
        Ok(Self::new(root))
    }

    pub fn new(root: PathBuf) -> Self {
        Self {
            config: root.join("config"),
            state: root.join("state"),
            instances: root.join("instances"),
            content: root.join("content"),
            runtimes: root.join("runtimes"),
            temp: root.join("temp"),
            logs: root.join("logs"),
            cache: root.join("cache"),
            root,
        }
    }

    pub fn initialize(&self) -> CoreResult<()> {
        for path in [
            &self.root,
            &self.config,
            &self.state,
            &self.instances,
            &self.content,
            &self.runtimes,
            &self.temp,
            &self.logs,
            &self.cache,
        ] {
            fs::create_dir_all(path)?;
            if fs::symlink_metadata(path)?.file_type().is_symlink() {
                return Err(CoreError::Storage(format!(
                    "managed directory cannot be a symlink: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    pub fn instance(&self, id: Uuid) -> PathBuf {
        self.instances.join(id.to_string())
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn database_file(&self) -> PathBuf {
        self.state.join("core.sqlite3")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.state.join("core.lock")
    }
}

#[cfg(windows)]
fn default_root() -> CoreResult<PathBuf> {
    let executable = std::env::current_exe().map_err(CoreError::Io)?;
    let directory = executable
        .parent()
        .ok_or_else(|| CoreError::Configuration("cannot determine the install directory".into()))?;
    Ok(directory.to_path_buf())
}

#[cfg(not(windows))]
fn default_root() -> CoreResult<PathBuf> {
    Ok(ProjectDirs::from("com", "MzLauncher", "MzLauncher")
        .ok_or_else(|| {
            CoreError::Configuration("cannot determine the platform data directory".into())
        })?
        .data_local_dir()
        .join("core"))
}

pub fn safe_relative_path(value: &str) -> CoreResult<PathBuf> {
    if value.is_empty() || value.len() > 1024 || value.contains('\0') {
        return Err(CoreError::InvalidInput("invalid relative path".into()));
    }
    let normalized = value.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.starts_with("//")
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|character| *character == b':')
    {
        return Err(CoreError::InvalidInput(
            "absolute paths are not allowed".into(),
        ));
    }

    let path = Path::new(&normalized);
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    CoreError::InvalidInput("path must contain valid UTF-8".into())
                })?;
                validate_path_part(part)?;
                output.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CoreError::InvalidInput(
                    "path escapes the managed directory".into(),
                ));
            }
        }
    }
    if output.as_os_str().is_empty() {
        return Err(CoreError::InvalidInput("empty file path".into()));
    }
    Ok(output)
}

pub fn safe_join(root: &Path, relative: &str) -> CoreResult<PathBuf> {
    Ok(root.join(safe_relative_path(relative)?))
}

pub fn atomic_write(path: &Path, contents: &[u8]) -> CoreResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| CoreError::Storage(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent)?;
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| {
            use std::io::Write;
            file.write_all(contents)?;
            file.sync_all()
        })
        .map_err(|error| CoreError::Storage(error.to_string()))
}

fn validate_path_part(part: &str) -> CoreResult<()> {
    if part.len() > 255
        || part.ends_with([' ', '.'])
        || part.chars().any(|character| {
            character.is_control() || matches!(character, ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return Err(CoreError::InvalidInput(format!(
            "invalid path component: {part}"
        )));
    }
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if reserved {
        return Err(CoreError::InvalidInput(format!(
            "reserved path component: {part}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_archive_path_escape() {
        for path in [
            "../escape",
            "/absolute",
            "C:/escape",
            "folder/../../escape",
            "folder\\..\\escape",
            "NUL",
            "con.txt",
            "normal.",
        ] {
            assert!(safe_relative_path(path).is_err(), "accepted {path}");
        }
    }

    #[test]
    fn accepts_normal_relative_path() {
        assert_eq!(
            safe_relative_path("overrides/config/client.toml").unwrap(),
            PathBuf::from("overrides/config/client.toml")
        );
    }
}
