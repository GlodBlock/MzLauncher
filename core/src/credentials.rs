use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::error::{CoreError, CoreResult};

const SERVICE_NAME: &str = "com.mzlauncher.core";
const CHUNK_MANIFEST_PREFIX: &str = "mzlauncher-chunks-v1:";
const MAX_CHUNK_UTF16_LEN: usize = 1024;
const MAX_CHUNKS: usize = 64;

#[derive(Clone, Default)]
pub struct CredentialStore;

impl CredentialStore {
    pub fn new() -> Self {
        Self
    }

    pub fn store(&self, key: &str, value: &str) -> CoreResult<()> {
        validate_key(key)?;
        let previous = read_optional(key)?.map(Zeroizing::new);
        let chunks = chunk_secret(value);
        if chunks.len() > MAX_CHUNKS {
            return Err(CoreError::InvalidInput("credential is too large".into()));
        }
        if chunks.len() == 1 {
            write_entry(key, chunks[0].as_str())?;
            if let Some(previous) = previous {
                cleanup_previous_chunks(key, &previous);
            }
            return Ok(());
        }

        let generation = Uuid::new_v4();
        let result = (|| {
            for (index, chunk) in chunks.iter().enumerate() {
                write_entry(&chunk_key(key, generation, index), chunk.as_str())?;
            }
            write_entry(
                key,
                &format!("{CHUNK_MANIFEST_PREFIX}{generation}:{}", chunks.len()),
            )
        })();
        if let Err(error) = result {
            let _ = delete_chunks(key, generation, chunks.len());
            return Err(error);
        }
        if let Some(previous) = previous {
            cleanup_previous_chunks(key, &previous);
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> CoreResult<String> {
        validate_key(key)?;
        let stored = read_entry(key)?;
        let Some((generation, count)) = parse_manifest(&stored)? else {
            return Ok(stored);
        };
        let mut value = String::new();
        for index in 0..count {
            value.push_str(&read_entry(&chunk_key(key, generation, index))?);
        }
        Ok(value)
    }

    pub fn delete(&self, key: &str) -> CoreResult<()> {
        validate_key(key)?;
        let Some(stored) = read_optional(key)? else {
            return Ok(());
        };
        let chunks = parse_manifest(&stored)?;
        let base_result = delete_entry(key);
        let chunks_result = match chunks {
            Some((generation, count)) => delete_chunks(key, generation, count),
            None => Ok(()),
        };
        base_result.and(chunks_result)
    }

    pub fn store_json<T: serde::Serialize>(&self, key: &str, value: &T) -> CoreResult<()> {
        let mut serialized = serde_json::to_string(value)?;
        let result = self.store(key, &serialized);
        serialized.zeroize();
        result
    }

    pub fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> CoreResult<T> {
        let mut serialized = self.get(key)?;
        let result = serde_json::from_str(&serialized).map_err(CoreError::from);
        serialized.zeroize();
        result
    }
}

fn chunk_secret(value: &str) -> Vec<Zeroizing<String>> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut utf16_len = 0;
    for character in value.chars() {
        if !current.is_empty() && utf16_len + character.len_utf16() > MAX_CHUNK_UTF16_LEN {
            chunks.push(Zeroizing::new(std::mem::take(&mut current)));
            utf16_len = 0;
        }
        current.push(character);
        utf16_len += character.len_utf16();
    }
    chunks.push(Zeroizing::new(current));
    chunks
}

fn parse_manifest(value: &str) -> CoreResult<Option<(Uuid, usize)>> {
    let Some(manifest) = value.strip_prefix(CHUNK_MANIFEST_PREFIX) else {
        return Ok(None);
    };
    let invalid = || CoreError::Storage("stored credential metadata is invalid".into());
    let (generation, count) = manifest.split_once(':').ok_or_else(invalid)?;
    let generation = Uuid::parse_str(generation).map_err(|_| invalid())?;
    let count = count.parse::<usize>().map_err(|_| invalid())?;
    if count == 0 || count > MAX_CHUNKS {
        return Err(invalid());
    }
    Ok(Some((generation, count)))
}

fn cleanup_previous_chunks(key: &str, previous: &str) {
    if let Ok(Some((generation, count))) = parse_manifest(previous) {
        let _ = delete_chunks(key, generation, count);
    }
}

fn chunk_key(key: &str, generation: Uuid, index: usize) -> String {
    format!("{key}-chunk-{generation}-{index}")
}

fn read_optional(key: &str) -> CoreResult<Option<String>> {
    let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(credential_error)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(credential_error(error)),
    }
}

fn read_entry(key: &str) -> CoreResult<String> {
    read_optional(key)?.ok_or(CoreError::Unauthorized)
}

fn write_entry(key: &str, value: &str) -> CoreResult<()> {
    let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(credential_error)?;
    entry.set_password(value).map_err(credential_error)
}

fn delete_entry(key: &str) -> CoreResult<()> {
    let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(credential_error)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(credential_error(error)),
    }
}

fn delete_chunks(key: &str, generation: Uuid, count: usize) -> CoreResult<()> {
    let mut first_error = None;
    for index in 0..count {
        if let Err(error) = delete_entry(&chunk_key(key, generation, index)) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn validate_key(key: &str) -> CoreResult<()> {
    if key.is_empty()
        || key.len() > 200
        || !(key.starts_with("account-") || key == "provider-curseforge")
    {
        return Err(CoreError::InvalidInput("invalid credential key".into()));
    }
    Ok(())
}

fn credential_error(error: keyring::Error) -> CoreError {
    CoreError::Authentication(format!(
        "operating system credential storage is unavailable: {error}"
    ))
}
