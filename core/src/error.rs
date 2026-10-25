use serde::Serialize;
use serde_json::{Map, Value};

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("incompatible protocol: client {client_min}..={client_max}, core {core}")]
    ProtocolIncompatible {
        client_min: u32,
        client_max: u32,
        core: u32,
    },
    #[error("authentication required")]
    Unauthorized,
    #[error("operation is forbidden: {0}")]
    Forbidden(String),
    #[error("authentication failed: {0}")]
    Authentication(String),
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("upstream {provider} returned {status}: {message}")]
    Upstream {
        provider: String,
        status: u16,
        message: String,
    },
    #[error("checksum mismatch for {path}")]
    Checksum { path: String },
    #[error("operation cancelled")]
    Cancelled,
    #[error("operation interrupted by core restart")]
    Interrupted,
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("archive error: {0}")]
    Archive(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl CoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::NotFound(_) => "not_found",
            Self::AlreadyExists(_) => "already_exists",
            Self::Conflict(_) => "conflict",
            Self::Protocol(_) => "protocol_error",
            Self::ProtocolIncompatible { .. } => "protocol_incompatible",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Authentication(_) => "authentication_failed",
            Self::Configuration(_) => "configuration_error",
            Self::Storage(_) => "storage_error",
            Self::Network(_) => "network_error",
            Self::Upstream { .. } => "upstream_error",
            Self::Checksum { .. } => "checksum_mismatch",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Unsupported(_) => "unsupported",
            Self::Io(_) => "io_error",
            Self::Json(_) => "invalid_json",
            Self::Database(_) => "database_error",
            Self::Archive(_) => "archive_error",
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn message_key(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "error.invalid_input",
            Self::NotFound(_) => "error.not_found",
            Self::AlreadyExists(_) => "error.already_exists",
            Self::Conflict(_) => "error.conflict",
            Self::Protocol(_) => "error.protocol",
            Self::ProtocolIncompatible { .. } => "error.protocol_incompatible",
            Self::Unauthorized => "error.unauthorized",
            Self::Forbidden(_) => "error.forbidden",
            Self::Authentication(_) => "error.authentication",
            Self::Configuration(_) => "error.configuration",
            Self::Storage(_) => "error.storage",
            Self::Network(_) => "error.network",
            Self::Upstream { .. } => "error.upstream",
            Self::Checksum { .. } => "error.checksum",
            Self::Cancelled => "error.cancelled",
            Self::Interrupted => "error.interrupted",
            Self::Unsupported(_) => "error.unsupported",
            Self::Io(_) => "error.io",
            Self::Json(_) => "error.invalid_json",
            Self::Database(_) => "error.database",
            Self::Archive(_) => "error.archive",
            Self::Internal(_) => "error.internal",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Network(_)
                | Self::Upstream {
                    status: 408 | 429 | 500..=599,
                    ..
                }
                | Self::Interrupted
        )
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    pub fn protocol_error(&self) -> ProtocolError {
        let mut params = Map::new();
        match self {
            Self::ProtocolIncompatible {
                client_min,
                client_max,
                core,
            } => {
                params.insert("client_min".into(), (*client_min).into());
                params.insert("client_max".into(), (*client_max).into());
                params.insert("core".into(), (*core).into());
            }
            Self::Upstream {
                provider, status, ..
            } => {
                params.insert("provider".into(), provider.clone().into());
                params.insert("status".into(), (*status).into());
            }
            _ => {}
        }
        ProtocolError {
            code: self.code(),
            message: self.to_string(),
            message_key: self.message_key(),
            params: Value::Object(params),
            retryable: self.retryable(),
            cancelled: self.is_cancelled(),
            job_id: None,
        }
    }
}

impl From<zip::result::ZipError> for CoreError {
    fn from(value: zip::result::ZipError) -> Self {
        Self::Archive(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProtocolError {
    pub code: &'static str,
    pub message: String,
    pub message_key: &'static str,
    pub params: Value,
    pub retryable: bool,
    pub cancelled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}
