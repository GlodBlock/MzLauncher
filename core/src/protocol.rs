use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::model::{HandshakeParams, HandshakeResult};
use crate::{CORE_VERSION, DATA_VERSION, DEBUG_AUTH, PROTOCOL_VERSION};

pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_JSON_DEPTH: usize = 32;

pub fn handshake(params: HandshakeParams) -> CoreResult<HandshakeResult> {
    if params.client_version.is_empty()
        || params.client_version.len() > 100
        || params.min_protocol > params.max_protocol
        || params.min_protocol > PROTOCOL_VERSION
        || params.max_protocol < PROTOCOL_VERSION
    {
        return Err(CoreError::ProtocolIncompatible {
            client_min: params.min_protocol,
            client_max: params.max_protocol,
            core: PROTOCOL_VERSION,
        });
    }
    Ok(HandshakeResult {
        core_version: CORE_VERSION.into(),
        protocol_version: PROTOCOL_VERSION,
        data_version: DATA_VERSION,
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        capabilities: capabilities(),
        debug_auth: DEBUG_AUTH,
    })
}

pub fn capabilities() -> Vec<String> {
    let mut capabilities = vec![
        "settings".into(),
        "jobs".into(),
        "instances".into(),
        "vanilla".into(),
        "java".into(),
        "launch".into(),
        "microsoft_auth".into(),
        "fabric".into(),
        "forge".into(),
        "neoforge".into(),
        "modrinth".into(),
        "curseforge".into(),
        "modpack_import".into(),
    ];
    if DEBUG_AUTH {
        capabilities.push("debug_auth".into());
    }
    capabilities
}

pub fn validate_message(value: &Value) -> CoreResult<()> {
    validate_depth(value, 0)?;
    let object = value
        .as_object()
        .ok_or_else(|| CoreError::Protocol("request must be a JSON object".into()))?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::Protocol("request id must be a string".into()))?;
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::Protocol("method must be a string".into()))?;
    if id.is_empty() || id.len() > 128 || method.is_empty() || method.len() > 128 {
        return Err(CoreError::Protocol(
            "request id or method has an invalid length".into(),
        ));
    }
    Ok(())
}

fn validate_depth(value: &Value, depth: usize) -> CoreResult<()> {
    if depth > MAX_JSON_DEPTH {
        return Err(CoreError::Protocol("JSON nesting is too deep".into()));
    }
    match value {
        Value::Array(values) => {
            for value in values {
                validate_depth(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                validate_depth(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_incompatible_protocol() {
        let error = handshake(HandshakeParams {
            client_version: "1.0.0".into(),
            min_protocol: 2,
            max_protocol: 3,
        })
        .unwrap_err();
        assert_eq!(error.code(), "protocol_incompatible");
    }

    #[test]
    fn validates_request_shape() {
        let value = serde_json::json!({"id": "1", "method": "system.health"});
        validate_message(&value).unwrap();
    }
}
