use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::{limits, ndjson};

/// ACP v1 `auth_required` JSON-RPC error code.
pub const AUTH_REQUIRED_CODE: i32 = -32000;

pub fn is_auth_required(error: &RpcError) -> bool {
    error.code == AUTH_REQUIRED_CODE
        // The claude-acp adapter reports authentication failures as an internal error with
        // a typed errorKind in data, rather than the ACP auth_required code.
        || error
            .data
            .as_ref()
            .and_then(|data| data.get("errorKind"))
            .and_then(Value::as_str)
            == Some("authentication_failed")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug)]
pub enum Error {
    Json(serde_json::Error),
    Protocol(soyeht_protocol::Error),
    MessageTooLarge,
    MissingJsonRpcVersion,
    InvalidMethod,
    RequestContainsResponse,
    ParametersNotObject,
    InvalidRequestId,
    InvalidResponseId,
    AmbiguousResponse,
    TooManyPendingCalls,
    RequestIdExhausted,
    ExpectedResponse,
    UnmatchedResponse,
    MissingConfigField(String),
    ConfigurationIdentifierTooLong,
    ConfigurationChoicesTooLarge,
    MissingSelectChoices,
    MissingGroupedChoices,
    DuplicateConfigurationChoices,
    UnsupportedConfigurationType,
    MissingCurrentConfigValue,
    ConfigurationValueType,
    ConfigurationOptionsNotArray,
    DuplicateConfigurationOptions,
    MissingSessionModes,
    ConfigurationMetadataTooLarge,
    UnknownConfigurationOption,
    UnavailableConfigurationChoice,
    MissingModeOption,
    MissingConfirmedOptions,
    UnconfirmedConfigurationValue,
    UnsupportedVersion,
    AuthenticationMethodsNotArray,
    AuthenticationMethodMissingId,
    AuthenticationMethodMissingName,
    AuthenticationMetadataTooLarge,
    AgentVersionTooLong,
    MissingSessionId,
    MissingStopReason,
    WrongSession,
    MissingUpdateKind,
    MissingUserContent,
    MissingMessageContent,
    MissingToolCallId,
    MissingPlanEntries,
    MissingModeId,
    ContinuationTemplate,
    ContinuationMetadata,
    ContinuationMessages,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol: {error}"),
            Self::MessageTooLarge => formatter.write_str("ACP line exceeds 1 MiB"),
            Self::MissingJsonRpcVersion => formatter.write_str("JSON-RPC 2.0 envelope required"),
            Self::InvalidMethod => formatter.write_str("invalid ACP method"),
            Self::RequestContainsResponse => formatter.write_str("request contains a response"),
            Self::ParametersNotObject => formatter.write_str("ACP parameters must be an object"),
            Self::InvalidRequestId => formatter.write_str("invalid ACP request ID"),
            Self::InvalidResponseId => formatter.write_str("invalid ACP response ID"),
            Self::AmbiguousResponse => {
                formatter.write_str("response needs exactly one result or error")
            }
            Self::TooManyPendingCalls => formatter.write_str("too many outstanding ACP calls"),
            Self::RequestIdExhausted => formatter.write_str("ACP request ID exhausted"),
            Self::ExpectedResponse => formatter.write_str("expected native response"),
            Self::UnmatchedResponse => formatter.write_str("unmatched ACP response"),
            Self::MissingConfigField(field) => write!(formatter, "missing config field: {field}"),
            Self::ConfigurationIdentifierTooLong => formatter
                .write_str("native configuration identifier exceeds the public protocol limit"),
            Self::ConfigurationChoicesTooLarge => {
                formatter.write_str("configuration choices exceed the metadata limit")
            }
            Self::MissingSelectChoices => formatter.write_str("missing select choices"),
            Self::MissingGroupedChoices => formatter.write_str("missing grouped choices"),
            Self::DuplicateConfigurationChoices => {
                formatter.write_str("too many or duplicate configuration choices")
            }
            Self::UnsupportedConfigurationType => {
                formatter.write_str("unsupported configuration option type")
            }
            Self::MissingCurrentConfigValue => formatter.write_str("missing current config value"),
            Self::ConfigurationValueType => {
                formatter.write_str("configuration current value has the wrong type")
            }
            Self::ConfigurationOptionsNotArray => {
                formatter.write_str("configOptions must be an array")
            }
            Self::DuplicateConfigurationOptions => {
                formatter.write_str("too many or duplicate configuration options")
            }
            Self::MissingSessionModes => formatter.write_str("missing session modes"),
            Self::ConfigurationMetadataTooLarge => {
                formatter.write_str("configuration metadata exceeds its limit")
            }
            Self::UnknownConfigurationOption => formatter.write_str("unknown configuration option"),
            Self::UnavailableConfigurationChoice => {
                formatter.write_str("configuration value is not an available choice")
            }
            Self::MissingModeOption => formatter.write_str("missing mode option"),
            Self::MissingConfirmedOptions => {
                formatter.write_str("configuration response omitted confirmed options")
            }
            Self::UnconfirmedConfigurationValue => {
                formatter.write_str("native response did not confirm the requested value")
            }
            Self::UnsupportedVersion => formatter.write_str("unsupported ACP version; expected v1"),
            Self::AuthenticationMethodsNotArray => {
                formatter.write_str("authMethods must be an array")
            }
            Self::AuthenticationMethodMissingId => {
                formatter.write_str("authentication method has no ID")
            }
            Self::AuthenticationMethodMissingName => {
                formatter.write_str("authentication method has no name")
            }
            Self::AuthenticationMetadataTooLarge => {
                formatter.write_str("authentication metadata exceeds identity or launch bounds")
            }
            Self::AgentVersionTooLong => {
                formatter.write_str("agent version exceeds metadata bounds")
            }
            Self::MissingSessionId => formatter.write_str("session/new response has no session ID"),
            Self::MissingStopReason => {
                formatter.write_str("v1 prompt response has no recognized stop reason")
            }
            Self::WrongSession => formatter.write_str("update belongs to another ACP session"),
            Self::MissingUpdateKind => formatter.write_str("session update has no discriminator"),
            Self::MissingUserContent => formatter.write_str("user update has no content"),
            Self::MissingMessageContent => formatter.write_str("message update has no content"),
            Self::MissingToolCallId => formatter.write_str("tool update has no call ID"),
            Self::MissingPlanEntries => formatter.write_str("plan has no entries"),
            Self::MissingModeId => formatter.write_str("mode update has no mode ID"),
            Self::ContinuationTemplate => formatter.write_str("continuation template is invalid"),
            Self::ContinuationMetadata => {
                formatter.write_str("continuation metadata exceeds its bound")
            }
            Self::ContinuationMessages => {
                formatter.write_str("continuation needs one whole message within its bound")
            }
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Protocol(error) => Some(error),
            _ => None,
        }
    }
}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<soyeht_protocol::Error> for Error {
    fn from(error: soyeht_protocol::Error) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Result<Value, RpcError>,
    },
}

fn valid_id(id: &Value) -> bool {
    id.is_null() || id.is_string() || id.is_i64()
}

impl Message {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > limits::ACP_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        let value: Value = serde_json::from_slice(bytes)?;
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(Error::MissingJsonRpcVersion);
        }
        if let Some(method) = value.get("method") {
            let method = method
                .as_str()
                .filter(|method| !method.is_empty() && method.len() <= 256)
                .ok_or(Error::InvalidMethod)?
                .to_owned();
            if value.get("result").is_some() || value.get("error").is_some() {
                return Err(Error::RequestContainsResponse);
            }
            let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
            if !params.is_object() {
                return Err(Error::ParametersNotObject);
            }
            return match value.get("id") {
                Some(id) if valid_id(id) => Ok(Self::Request {
                    id: id.clone(),
                    method,
                    params,
                }),
                Some(_) => Err(Error::InvalidRequestId),
                None => Ok(Self::Notification { method, params }),
            };
        }
        let id = value
            .get("id")
            .filter(|id| id.is_null() || valid_id(id))
            .ok_or(Error::InvalidResponseId)?
            .clone();
        let result = match (value.get("result"), value.get("error")) {
            (Some(result), None) => Ok(result.clone()),
            (None, Some(error)) => Err(serde_json::from_value(error.clone())?),
            _ => return Err(Error::AmbiguousResponse),
        };
        Ok(Self::Response { id, result })
    }

    pub fn value(&self) -> Value {
        match self {
            Self::Request { id, method, params } => {
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
            }
            Self::Notification { method, params } => {
                json!({"jsonrpc":"2.0","method":method,"params":params})
            }
            Self::Response {
                id,
                result: Ok(result),
            } => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Self::Response {
                id,
                result: Err(error),
            } => json!({"jsonrpc":"2.0","id":id,"error":error}),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut bytes = serde_json::to_vec(&self.value())?;
        if bytes.len() > limits::ACP_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[derive(Debug)]
pub struct Record {
    pub from_seq: u64,
    pub to_seq: u64,
    pub message: Message,
}

/// Parse complete records separated by the protocol crate's sole NDJSON
/// decoder. Partial bytes remain there until a complete record is available.
pub fn decode(
    decoder: &mut ndjson::Decoder,
    offset: u64,
    bytes: &[u8],
) -> Result<Vec<Record>, Error> {
    let batch = decoder.push(offset, bytes)?;
    if batch.oversized {
        return Err(Error::MessageTooLarge);
    }
    batch
        .lines
        .into_iter()
        .map(|line| {
            Ok(Record {
                from_seq: line.from_seq,
                to_seq: line.to_seq,
                message: Message::parse(&line.bytes)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_utf8_and_multiple_lines_keep_exact_record_offsets() {
        let message = Message::Notification {
            method: "session/update".into(),
            params: json!({"text":"Hello 🌍"}),
        };
        let bytes = message.encode().unwrap();
        let mut decoder = ndjson::Decoder::new(31);
        let mut records = Vec::new();
        for byte in &bytes {
            let offset = decoder.next_offset();
            records.extend(decode(&mut decoder, offset, &[*byte]).unwrap());
        }
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].from_seq, 31);
        assert_eq!(records[0].to_seq, 31 + bytes.len() as u64);
        assert_eq!(records[0].message, message);
        assert!(decode(&mut decoder, 31, &bytes).is_err());
    }

    #[test]
    fn rejects_ambiguous_envelopes_invalid_ids_and_unbounded_lines() {
        for value in [
            json!({"id":1,"result":{}}),
            json!({"jsonrpc":"2.0","id":1.5,"result":{}}),
            json!({"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"x"}}),
            json!({"jsonrpc":"2.0","id":1,"method":"x","result":{}}),
            json!({"jsonrpc":"2.0","method":"x","params":[]}),
        ] {
            assert!(Message::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let mut decoder = ndjson::Decoder::new(0);
        assert!(decode(&mut decoder, 0, &vec![b'x'; limits::ACP_MESSAGE_BYTES + 1]).is_err());
    }
}
