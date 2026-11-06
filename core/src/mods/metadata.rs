use std::io::Read;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::Value;

const MAX_METADATA_SIZE: u64 = 1024 * 1024;
const MAX_ICON_SIZE: u64 = 512 * 1024;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct JarMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub icon_base64: Option<String>,
    icon_path: Option<String>,
}

pub fn read_jar_metadata(path: &Path) -> Option<JarMetadata> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut metadata = JarMetadata::default();

    if let Some(value) = read_zip_text(&mut archive, "fabric.mod.json")
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    {
        read_json_metadata(&value, &mut metadata, "icon");
    }
    if metadata.name.is_none() || metadata.version.is_none() {
        if let Some(value) = read_zip_text(&mut archive, "quilt.mod.json")
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value.get("quilt_loader")?.get("metadata").cloned())
        {
            read_json_metadata(&value, &mut metadata, "icon");
        }
    }
    if metadata.name.is_none() || metadata.version.is_none() {
        if let Some(value) = read_zip_text(&mut archive, "mcmod.info")
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value.as_array().and_then(|items| items.first()).cloned())
        {
            read_json_metadata(&value, &mut metadata, "logoFile");
        }
    }
    if metadata.name.is_none() || metadata.version.is_none() || metadata.icon_path.is_none() {
        for path in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
            let Some(text) = read_zip_text(&mut archive, path) else {
                continue;
            };
            metadata.name = metadata.name.or_else(|| toml_field(&text, "displayName"));
            metadata.version = metadata.version.or_else(|| toml_field(&text, "version"));
            metadata.icon_path = metadata.icon_path.or_else(|| toml_field(&text, "logoFile"));
        }
    }

    if let Some(icon_path) = metadata.icon_path.as_deref() {
        let safe_path = crate::paths::safe_relative_path(icon_path).ok();
        if let Some(safe_path) = safe_path {
            let entry = safe_path.to_string_lossy().replace('\\', "/");
            metadata.icon_base64 = read_zip_bytes(&mut archive, &entry, MAX_ICON_SIZE)
                .map(|bytes| STANDARD.encode(bytes));
        }
    }
    (metadata.name.is_some() || metadata.version.is_some() || metadata.icon_base64.is_some())
        .then_some(metadata)
}

fn read_json_metadata(value: &Value, metadata: &mut JarMetadata, icon_key: &str) {
    if metadata.name.is_none() {
        metadata.name = value
            .get("name")
            .and_then(Value::as_str)
            .and_then(clean_value);
    }
    if metadata.version.is_none() {
        metadata.version = value
            .get("version")
            .and_then(Value::as_str)
            .and_then(clean_value);
    }
    if metadata.icon_path.is_none() {
        metadata.icon_path = value
            .get(icon_key)
            .and_then(Value::as_str)
            .and_then(clean_value);
    }
}

fn clean_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !(value.starts_with("${") && value.ends_with('}')))
        .then(|| value.to_owned())
}

fn toml_field(text: &str, field: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != field {
            return None;
        }
        let value = value.trim();
        if value.starts_with('"') {
            serde_json::from_str::<String>(value)
                .ok()
                .and_then(|value| clean_value(&value))
        } else {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
                .and_then(clean_value)
        }
    })
}

fn read_zip_text(archive: &mut zip::ZipArchive<std::fs::File>, path: &str) -> Option<String> {
    read_zip_bytes(archive, path, MAX_METADATA_SIZE).and_then(|bytes| String::from_utf8(bytes).ok())
}

fn read_zip_bytes(
    archive: &mut zip::ZipArchive<std::fs::File>,
    path: &str,
    limit: u64,
) -> Option<Vec<u8>> {
    let entry = archive.by_name(path).ok()?;
    if entry.size() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mod.jar");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
        (temp, path)
    }

    #[test]
    fn reads_fabric_metadata_and_icon() {
        let (_temp, path) = jar(&[
            (
                "fabric.mod.json",
                br#"{"name":"Fabric Mod","version":"1.2.3","icon":"assets/mod/icon.png"}"#,
            ),
            ("assets/mod/icon.png", b"png"),
        ]);
        let metadata = read_jar_metadata(&path).unwrap();
        assert_eq!(metadata.name.as_deref(), Some("Fabric Mod"));
        assert_eq!(metadata.version.as_deref(), Some("1.2.3"));
        assert_eq!(metadata.icon_base64.as_deref(), Some("cG5n"));
    }

    #[test]
    fn reads_neoforge_toml_metadata() {
        let (_temp, path) = jar(&[(
            "META-INF/neoforge.mods.toml",
            br#"[[mods]]
displayName = "Neo Mod"
version = "2.0"
logoFile = "logo.png"
"#,
        )]);
        let metadata = read_jar_metadata(&path).unwrap();
        assert_eq!(metadata.name.as_deref(), Some("Neo Mod"));
        assert_eq!(metadata.version.as_deref(), Some("2.0"));
    }

    #[test]
    fn invalid_jar_has_no_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("broken.jar");
        std::fs::write(&path, b"not a jar").unwrap();
        assert_eq!(read_jar_metadata(&path), None);
    }
}
