use super::{Changes, filesystem};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::{Message, RpcError};
use soyeht_protocol::ErrorKind;
use soyeht_store::agents::{self, NativeRequest};
use std::path::Path;
use uuid::Uuid;

pub fn finish(
    request: &NativeRequest,
    result: std::result::Result<Value, RpcError>,
    changes: &mut Changes,
) {
    let native = Message::Response {
        id: request.native_id.clone(),
        result,
    };
    changes
        .answers
        .push((request.request_id.clone(), json!({"native":native.value()})));
    changes.outgoing.push(native);
}

pub fn failure(message: impl Into<String>) -> RpcError {
    RpcError {
        code: -32603,
        message: message.into(),
        data: None,
    }
}

impl Core {
    pub fn run_native_files(&mut self, pane: &str) -> Result<()> {
        let requests = self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?;
        for request in requests
            .into_iter()
            .filter(|r| r.method.starts_with("fs/"))
            .take(4)
        {
            let state = self.agent_state(pane)?;
            let saved = self.panes[pane].stored.clone();
            let root = Path::new(&saved.pane.cwd);
            let result = if request.generation != saved.pane.generation
                || request.params["sessionId"].as_str() != state.session_id.as_deref()
            {
                Err(failure("Native effect belongs to another session"))
            } else if let Some(path) = request.params["path"].as_str() {
                let path = Path::new(path);
                match request.method.as_str() {
                    "fs/read_text_file" => filesystem::read(root, path)
                        .map_err(|error| failure(error.to_string()))
                        .and_then(|text| {
                            let start = request
                                .params
                                .get("line")
                                .map(|value| {
                                    value
                                        .as_u64()
                                        .filter(|n| *n > 0)
                                        .ok_or_else(|| failure("Line must be a positive integer"))
                                })
                                .transpose()?
                                .unwrap_or(1)
                                - 1;
                            let limit = request
                                .params
                                .get("limit")
                                .map(|value| {
                                    value.as_u64().filter(|n| *n > 0).ok_or_else(|| {
                                        failure("Line limit must be a positive integer")
                                    })
                                })
                                .transpose()?
                                .unwrap_or(u64::MAX);
                            let content: String = text
                                .split_inclusive('\n')
                                .skip(usize::try_from(start).unwrap_or(usize::MAX))
                                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                                .collect();
                            Ok(json!({"content":content}))
                        }),
                    "fs/write_text_file" => {
                        if let Some(content) = request.params["content"].as_str() {
                            if request.public.get("started").is_some() {
                                // An interrupted write is never blindly repeated. A
                                // matching target proves its intended final value;
                                // another value needs a new explicit agent request.
                                if filesystem::read(root, path)
                                    .is_ok_and(|actual| actual == content)
                                {
                                    Ok(json!({}))
                                } else {
                                    Err(failure(
                                        "Interrupted file write could not be reconciled; inspect the file before retrying",
                                    ))
                                }
                            } else {
                                let now_ms = self.now_ms();
                                StorePort::mutate(
                                    &mut self.store,
                                    "native-effect",
                                    &Uuid::new_v4().to_string(),
                                    "start",
                                    |tx| {
                                        agents::start_effect(
                                            tx,
                                            &request.request_id,
                                            &json!({"at":now_ms}),
                                        )?;
                                        Ok((json!({}), vec![]))
                                    },
                                )
                                .map_err(errors::storage)?;
                                filesystem::write_with_key(root, path, content, &request.request_id)
                                    .map(|()| json!({}))
                                    .map_err(|error| failure(error.to_string()))
                            }
                        } else {
                            Err(failure("Text content is required"))
                        }
                    }
                    _ => Err(RpcError {
                        code: -32601,
                        message: "Unknown filesystem method".into(),
                        data: None,
                    }),
                }
            } else {
                Err(failure("Absolute file path is required"))
            };
            let result = if (Message::Response {
                id: request.native_id.clone(),
                result: result.clone(),
            })
            .encode()
            .is_err()
            {
                Err(failure("File result exceeds the native message limit"))
            } else {
                result
            };
            let mut changes = Changes::default();
            finish(&request, result, &mut changes);
            self.commit_agent(saved, state, changes, None, json!({}))?;
        }
        Ok(())
    }
}

pub fn receive(
    pane: &str,
    generation: u32,
    from: u64,
    id: Value,
    method: String,
    params: Value,
    now_ms: i64,
) -> NativeRequest {
    let expires_ms = if method == "terminal/wait_for_exit" {
        i64::MAX
    } else {
        now_ms + 30_000
    };
    NativeRequest {
        request_id: Uuid::new_v4().to_string(),
        pane_id: pane.into(),
        generation,
        source_seq: from,
        native_id: id,
        method,
        params,
        public: json!({}),
        expires_ms,
        result: None,
    }
}

pub fn require_session(state: &super::state::State, params: &Value) -> Result<()> {
    if state.session_id.is_none() || params["sessionId"].as_str() != state.session_id.as_deref() {
        return Err(errors::error(
            ErrorKind::InvalidParams,
            "Native effect belongs to another session",
        ));
    }
    Ok(())
}
