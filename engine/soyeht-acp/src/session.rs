use crate::{Error, RpcError, config::Options};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{AuthMethod, ProbeResult};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuthKind {
    Agent,
    Terminal,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeAuth {
    pub public: AuthMethod,
    pub kind: AuthKind,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Initialization {
    pub version: String,
    pub advertised_load: bool,
    pub auth_methods: Vec<NativeAuth>,
}

impl Initialization {
    pub fn parse(value: &Value) -> Result<Self, Error> {
        if value["protocolVersion"] != 1 {
            return Err(Error("unsupported ACP version; expected v1".into()));
        }
        let mut auth_methods = Vec::new();
        if let Some(methods) = value.get("authMethods").filter(|v| !v.is_null()) {
            for method in methods
                .as_array()
                .ok_or_else(|| Error("authMethods must be an array".into()))?
            {
                let id = method["id"]
                    .as_str()
                    .ok_or_else(|| Error("authentication method has no ID".into()))?;
                let name = method["name"]
                    .as_str()
                    .ok_or_else(|| Error("authentication method has no name".into()))?;
                let kind = match method["type"].as_str() {
                    None | Some("agent") => AuthKind::Agent,
                    Some("terminal") => AuthKind::Terminal,
                    _ => AuthKind::Unsupported,
                };
                auth_methods.push(NativeAuth {
                    public: AuthMethod {
                        id: id.into(),
                        name: name.into(),
                        description: method["description"].as_str().map(str::to_owned),
                        terminal: kind == AuthKind::Terminal,
                    },
                    kind,
                    args: method
                        .get("args")
                        .filter(|v| !v.is_null())
                        .map(|v| serde_json::from_value(v.clone()))
                        .transpose()?
                        .unwrap_or_default(),
                    env: method
                        .get("env")
                        .filter(|v| !v.is_null())
                        .map(|v| serde_json::from_value(v.clone()))
                        .transpose()?
                        .unwrap_or_default(),
                });
            }
        }
        if auth_methods.len() > 32
            || serde_json::to_vec(&auth_methods)?.len() > 256 * 1024
            || auth_methods.iter().enumerate().any(|(index, method)| {
                let id = &method.public.id;
                id.is_empty()
                    || id.chars().count() > 128
                    || id.contains('\0')
                    || method.public.name.is_empty()
                    || method.public.name.chars().count() > 256
                    || method
                        .public
                        .description
                        .as_ref()
                        .is_some_and(|v| v.chars().count() > 4096)
                    || auth_methods[..index]
                        .iter()
                        .any(|prior| prior.public.id == *id)
                    || method.args.len() > 64
                    || method.env.len() > 64
                    || method
                        .args
                        .iter()
                        .any(|v| v.contains('\0') || v.len() > 16 * 1024)
                    || method.env.iter().any(|(k, v)| {
                        k.is_empty()
                            || k.contains(['=', '\0'])
                            || v.contains('\0')
                            || k.len() + v.len() > 16 * 1024
                    })
                    || method.args.iter().map(String::len).sum::<usize>()
                        + method
                            .env
                            .iter()
                            .map(|(k, v)| k.len() + v.len())
                            .sum::<usize>()
                        > 64 * 1024
            })
        {
            return Err(Error(
                "authentication metadata exceeds identity or launch bounds".into(),
            ));
        }
        let version = value["agentInfo"]["version"]
            .as_str()
            .unwrap_or("unreported");
        if version.chars().count() > 256 {
            return Err(Error("agent version exceeds metadata bounds".into()));
        }
        Ok(Self {
            version: version.into(),
            advertised_load: value["agentCapabilities"]["loadSession"]
                .as_bool()
                .unwrap_or(false),
            auth_methods,
        })
    }
}

/// The binary digest is supplied by the launcher after hashing the executable.
/// Canonical JSON makes configuration key order irrelevant; no input is logged.
pub fn fingerprint(
    binary_digest: &str,
    reported_version: &str,
    configuration: &Value,
) -> Result<String, Error> {
    let encoded = serde_json::to_vec(&json!([binary_digest, reported_version, configuration]))
        .map_err(Error::from)?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

pub fn probe_result(response: Result<&Value, &RpcError>) -> ProbeResult {
    match response {
        Ok(_) => ProbeResult::Worked,
        Err(error) if error.code == -32601 => ProbeResult::Unsupported,
        Err(error) if error.code == -32000 => ProbeResult::Unauthenticated,
        Err(_) => ProbeResult::Inconclusive,
    }
}

pub fn session_id(response: &Value) -> Result<String, Error> {
    response["sessionId"]
        .as_str()
        .filter(|id| !id.is_empty() && id.chars().count() <= 128 && !id.contains('\0'))
        .map(str::to_owned)
        .ok_or_else(|| Error("session/new response has no session ID".into()))
}

pub fn stop_reason(response: &Value) -> Result<&str, Error> {
    match response["stopReason"].as_str() {
        Some(
            reason @ ("end_turn" | "max_tokens" | "max_turn_requests" | "refusal" | "cancelled"),
        ) => Ok(reason),
        _ => Err(Error(
            "v1 prompt response has no recognized stop reason".into(),
        )),
    }
}

#[derive(Debug, Clone)]
pub enum Update {
    Message { thought: bool, content: Value },
    UserMessage(Value),
    Tool { update: bool, fields: Value },
    Plan(Value),
    Usage(Value),
    Configuration(Options),
    Mode(String),
}

/// Session identity is checked before any update can change the owning pane.
pub fn update(expected_session: &str, params: &Value) -> Result<Option<Update>, Error> {
    if params["sessionId"].as_str() != Some(expected_session) {
        return Err(Error("update belongs to another ACP session".into()));
    }
    let value = &params["update"];
    let kind = value["sessionUpdate"]
        .as_str()
        .ok_or_else(|| Error("session update has no discriminator".into()))?;
    Ok(Some(match kind {
        "user_message_chunk" => Update::UserMessage(
            value
                .get("content")
                .filter(|v| v.is_object())
                .ok_or_else(|| Error("user update has no content".into()))?
                .clone(),
        ),
        "agent_message_chunk" | "agent_thought_chunk" => {
            let content = value
                .get("content")
                .filter(|v| v.is_object())
                .ok_or_else(|| Error("message update has no content".into()))?;
            Update::Message {
                thought: kind == "agent_thought_chunk",
                content: content.clone(),
            }
        }
        "tool_call" | "tool_call_update" => {
            if !value["toolCallId"].is_string() {
                return Err(Error("tool update has no call ID".into()));
            }
            Update::Tool {
                update: kind == "tool_call_update",
                fields: value.clone(),
            }
        }
        "plan" => Update::Plan(
            value["entries"]
                .as_array()
                .map(|v| json!(v))
                .ok_or_else(|| Error("plan has no entries".into()))?,
        ),
        "usage_update" => Update::Usage(value.clone()),
        "config_option_update" => Update::Configuration(Options::from_response(value)?),
        "current_mode_update" => Update::Mode(
            value["currentModeId"]
                .as_str()
                .ok_or_else(|| Error("mode update has no mode ID".into()))?
                .into(),
        ),
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_claims_do_not_turn_parameter_errors_into_proof() {
        let initialization = Initialization::parse(
            &json!({"protocolVersion":1,"agentCapabilities":{"loadSession":true}}),
        )
        .unwrap();
        assert!(initialization.advertised_load);
        for (code, expected) in [
            (-32601, ProbeResult::Unsupported),
            (-32000, ProbeResult::Unauthenticated),
            (-32602, ProbeResult::Inconclusive),
            (-32002, ProbeResult::Inconclusive),
        ] {
            assert_eq!(
                probe_result(Err(&RpcError {
                    code,
                    message: "test".into(),
                    data: None
                })),
                expected
            );
        }
        assert!(Initialization::parse(&json!({"protocolVersion":2})).is_err());
        assert!(stop_reason(&json!({"_meta":{}})).is_err());
        assert_eq!(
            stop_reason(&json!({"stopReason":"cancelled"})).unwrap(),
            "cancelled"
        );
    }

    #[test]
    fn terminal_auth_is_a_separate_invocation_and_fingerprint_tracks_inputs() {
        let value = json!({"protocolVersion":1,"agentInfo":{"version":"7"},"authMethods":[{"id":"login","name":"Sign in","type":"terminal","args":["login"],"env":{"MODE":"login"}},{"id":"browser","name":"Browser"}]});
        let info = Initialization::parse(&value).unwrap();
        assert_eq!(info.auth_methods[0].kind, AuthKind::Terminal);
        assert_eq!(info.auth_methods[0].args, ["login"]);
        assert_eq!(info.auth_methods[1].kind, AuthKind::Agent);
        let a = fingerprint("digest", "7", &json!({"b":2,"a":1})).unwrap();
        assert_eq!(
            a,
            fingerprint("digest", "7", &json!({"a":1,"b":2})).unwrap()
        );
        assert_ne!(
            a,
            fingerprint("different", "7", &json!({"a":1,"b":2})).unwrap()
        );
        assert_ne!(
            a,
            fingerprint("digest", "8", &json!({"a":1,"b":2})).unwrap()
        );
        assert_ne!(
            a,
            fingerprint("digest", "7", &json!({"a":2,"b":2})).unwrap()
        );
    }

    #[test]
    fn native_metadata_cannot_break_public_ids_or_launch_limits() {
        let valid = json!({"protocolVersion":1,"agentInfo":{"version":"1"},
            "authMethods":[{"id":"login","name":"Sign in","type":"terminal","args":["login"],"env":{"MODE":"login"}}]});
        assert!(Initialization::parse(&valid).is_ok());
        for (field, value) in [
            ("id", json!("")),
            ("id", json!("x".repeat(129))),
            ("name", json!("x".repeat(257))),
            ("description", json!("x".repeat(4097))),
            ("args", json!(["x".repeat(16 * 1024 + 1)])),
            ("env", json!({"BAD=KEY":"x"})),
        ] {
            let mut rejected = valid.clone();
            rejected["authMethods"][0][field] = value;
            assert!(
                Initialization::parse(&rejected).is_err(),
                "accepted invalid {field}"
            );
        }
        let mut duplicate = valid.clone();
        duplicate["authMethods"]
            .as_array_mut()
            .unwrap()
            .push(valid["authMethods"][0].clone());
        assert!(Initialization::parse(&duplicate).is_err());
        let mut version = valid;
        version["agentInfo"]["version"] = json!("x".repeat(257));
        assert!(Initialization::parse(&version).is_err());
        assert!(session_id(&json!({"sessionId":"x".repeat(129)})).is_err());
        assert!(session_id(&json!({"sessionId":"x".repeat(128)})).is_ok());
    }

    #[test]
    fn updates_cannot_cross_session_boundaries() {
        let params = json!({"sessionId":"one","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}}});
        assert!(update("two", &params).is_err());
        assert!(matches!(
            update("one", &params).unwrap(),
            Some(Update::Message { thought: false, .. })
        ));
        assert!(
            update(
                "one",
                &json!({"sessionId":"one","update":{"sessionUpdate":"_future"}})
            )
            .unwrap()
            .is_none()
        );
    }
}
