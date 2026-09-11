use crate::{Error, Message, RpcError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
        self.request("initialize", json!({"protocolVersion":1,
            "clientInfo":{"name":"Soyeht","version":env!("CARGO_PKG_VERSION")},
            "clientCapabilities":{"fs":{"readTextFile":true,"writeTextFile":true},"terminal":true,
                "auth":{"terminal":true},"elicitation":{"form":{}},"session":{"configOptions":{"boolean":{}}}}}), "initialize", now_ms)
    }
}

pub fn prompt(session_id: &str, text: &str) -> Value {
    json!({"sessionId":session_id,"prompt":[{"type":"text","text":text}]})
}

pub fn cancel(session_id: &str) -> Message {
    Message::Notification {
        method: "session/cancel".into(),
        params: json!({"sessionId":session_id}),
    }
}

pub fn new_session(cwd: &str) -> Value {
    json!({"cwd":cwd,"mcpServers":[]})
}
pub fn load_session(cwd: &str, session_id: &str) -> Value {
    json!({"cwd":cwd,"sessionId":session_id,"mcpServers":[]})
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
}
