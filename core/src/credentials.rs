use zeroize::Zeroize;

use crate::error::{CoreError, CoreResult};

const SERVICE_NAME: &str = "com.mzlauncher.core";

#[derive(Clone, Default)]
pub struct CredentialStore;

impl CredentialStore {
    pub fn new() -> Self {
        Self
    }

    pub fn store(&self, key: &str, value: &str) -> CoreResult<()> {
        validate_key(key)?;
        let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(credential_error)?;
        entry.set_password(value).map_err(credential_error)
    }

    pub fn get(&self, key: &str) -> CoreResult<String> {
        validate_key(key)?;
        let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(credential_error)?;
        entry.get_password().map_err(|error| match error {
            keyring::Error::NoEntry => CoreError::Unauthorized,
            other => credential_error(other),
        })
    }

    pub fn delete(&self, key: &str) -> CoreResult<()> {
        validate_key(key)?;
        let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(credential_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(credential_error(error)),
        }
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
