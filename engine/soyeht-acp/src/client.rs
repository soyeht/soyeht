use crate::{Error, Message, RpcError};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvVariable {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StdioMcpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<EnvVariable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Call {
    pub method: String,
    pub params: Value,
    pub context: String,
    pub sent_at_ms: u64,
}

/// Serialize together with the engine's outbox BEFORE sending a native request.
/// On adoption restore this value; do not initialize or create another session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Client {
    next_id: u64,
    pub pending: BTreeMap<String, Call>,
}

impl Client {
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        context: &str,
        now_ms: u64,
    ) -> Result<Message, Error> {
        if self.pending.len() >= 128 {
            return Err(Error::TooManyPendingCalls);
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(Error::RequestIdExhausted)?;
        let id = format!("s-{}", self.next_id);
        let message = Message::Request {
            id: json!(id),
            method: method.into(),
            params: params.clone(),
        };
        message.encode()?;
        self.pending.insert(
            id,
            Call {
                method: method.into(),
                params,
                context: context.into(),
                sent_at_ms: now_ms,
            },
        );
        Ok(message)
    }

    pub fn response(&mut self, message: Message) -> Result<(Call, Result<Value, RpcError>), Error> {
        let Message::Response { id, result } = message else {
            return Err(Error::ExpectedResponse);
        };
        let call = id
            .as_str()
            .and_then(|id| self.pending.remove(id))
            .ok_or(Error::UnmatchedResponse)?;
        Ok((call, result))
    }

    pub fn initialize(&mut self, now_ms: u64) -> Result<Message, Error> {
        self.request("initialize", initialize_params(), "initialize", now_ms)
    }
}

/// The one `initialize` request every harness process receives. Subagents are
/// advertised twice; see `subagents` for why.
pub fn initialize_params() -> Value {
    json!({"protocolVersion":1,
        "clientInfo":{"name":"Soyeht","version":env!("CARGO_PKG_VERSION")},
        "clientCapabilities":{"fs":{"readTextFile":true,"writeTextFile":true},"terminal":true,
            "auth":{"terminal":true},"elicitation":{"form":{}},
            "session":{"configOptions":{"boolean":{}}},"subagents":{},
            "_meta":{"jetbrains":{"air":{"version":1,"capabilities":["nativeSubagentSessions"]}}}}})
}

pub struct PromptImage<'a> {
    pub mime_type: &'a str,
    pub bytes: &'a [u8],
}

pub fn prompt_with_images(session_id: &str, text: &str, images: &[PromptImage<'_>]) -> Value {
    let mut prompt = vec![json!({"type":"text","text":text})];
    prompt.extend(images.iter().map(|image| {
        json!({
            "type":"image",
            "data":STANDARD.encode(image.bytes),
            "mimeType":image.mime_type
        })
    }));
    json!({"sessionId":session_id,"prompt":prompt})
}

pub fn prompt(session_id: &str, text: &str) -> Value {
    prompt_with_images(session_id, text, &[])
}

pub fn cancel(session_id: &str) -> Message {
    Message::Notification {
        method: "session/cancel".into(),
        params: json!({"sessionId":session_id}),
    }
}

pub fn new_session(cwd: &str) -> Value {
    new_session_with_mcp(cwd, &[])
}
pub fn load_session(cwd: &str, session_id: &str) -> Value {
    load_session_with_mcp(cwd, session_id, &[])
}
pub fn new_session_with_mcp(cwd: &str, servers: &[StdioMcpServer]) -> Value {
    json!({"cwd":cwd,"mcpServers":servers})
}
pub fn load_session_with_mcp(cwd: &str, session_id: &str, servers: &[StdioMcpServer]) -> Value {
    json!({"cwd":cwd,"sessionId":session_id,"mcpServers":servers})
}

pub fn permission_answer(id: Value, selected: Option<&str>) -> Message {
    let outcome = match selected {
        Some(id) => json!({"outcome":"selected","optionId":id}),
        None => json!({"outcome":"cancelled"}),
    };
    Message::Response {
        id,
        result: Ok(json!({"outcome":outcome})),
    }
}

pub fn elicitation_answer(id: Value, content: Option<Value>) -> Message {
    Message::Response {
        id,
        result: Ok(match content {
            Some(content) => json!({"action":"accept","content":content}),
            None => json!({"action":"cancel"}),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restoration_correlates_a_late_prompt_response_without_opening_another_session() {
        let mut client = Client::default();
        let message = client
            .request(
                "session/prompt",
                prompt("session-1", "hello"),
                "prompt-1",
                5,
            )
            .unwrap();
        let Message::Request { id, .. } = message else {
            unreachable!()
        };
        let mut restored: Client =
            serde_json::from_slice(&serde_json::to_vec(&client).unwrap()).unwrap();
        let (call, result) = restored
            .response(Message::Response {
                id,
                result: Ok(json!({"stopReason":"cancelled"})),
            })
            .unwrap();
        assert_eq!(call.context, "prompt-1");
        assert_eq!(result.unwrap()["stopReason"], "cancelled");
        assert!(restored.pending.is_empty());
        let next = restored
            .request(
                "session/prompt",
                prompt("session-1", "again"),
                "prompt-2",
                6,
            )
            .unwrap();
        assert_eq!(next.value()["id"], "s-2");
    }

    #[test]
    fn stdio_mcp_declaration_matches_the_acp_v1_shape() {
        let server = StdioMcpServer {
            name: "Soyeht".into(),
            command: "/opt/soyeht/bin/soyeht-mcp".into(),
            args: vec!["--binding".into(), "opaque".into()],
            env: vec![EnvVariable {
                name: "LANG".into(),
                value: "en_US.UTF-8".into(),
            }],
        };
        let value = new_session_with_mcp("/tmp/worktree", std::slice::from_ref(&server));
        assert_eq!(value["mcpServers"][0]["name"], "Soyeht");
        assert_eq!(
            value["mcpServers"][0]["command"],
            "/opt/soyeht/bin/soyeht-mcp"
        );
        assert_eq!(value["mcpServers"][0]["args"][1], "opaque");
        assert_eq!(value["mcpServers"][0]["env"][0]["name"], "LANG");
        assert!(value["mcpServers"][0].get("type").is_none());
        let loaded = load_session_with_mcp("/tmp/worktree", "session-1", &[server]);
        assert_eq!(loaded["sessionId"], "session-1");
        assert_eq!(loaded["mcpServers"][0], value["mcpServers"][0]);
    }

    #[test]
    fn prompt_images_follow_the_text_as_exact_acp_content_blocks() {
        let value = prompt_with_images(
            "session-1",
            "Inspect the selected element.",
            &[
                PromptImage {
                    mime_type: "image/png",
                    bytes: b"first",
                },
                PromptImage {
                    mime_type: "image/png",
                    bytes: b"second",
                },
            ],
        );
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["prompt"][0]["type"], "text");
        assert_eq!(value["prompt"][0]["text"], "Inspect the selected element.");
        assert_eq!(value["prompt"][1]["type"], "image");
        assert_eq!(value["prompt"][1]["mimeType"], "image/png");
        assert_eq!(value["prompt"][1]["data"], STANDARD.encode(b"first"));
        assert_eq!(value["prompt"][2]["data"], STANDARD.encode(b"second"));
        assert!(value["prompt"][1].get("uri").is_none());
    }
}
