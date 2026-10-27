use super::*;
use crate::engine::{EngineAttachment, EngineResponse};
use soyeht_protocol::AttachmentRef;
use std::collections::VecDeque;

struct FakeEngine {
    role: Role,
    calls: VecDeque<(String, Value)>,
}
impl EnginePort for FakeEngine {
    fn role(&self) -> &Role {
        &self.role
    }
    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.calls.push_back((method.into(), params));
        Ok(json!({"accepted":true}))
    }
}
fn context(task: bool) -> TaskContext {
    TaskContext {
        workspace_id: "workspace".into(),
        pane_id: "pane".into(),
        session_generation: 7,
        task_id: task.then(|| "WS-1".into()),
        attempt: task.then_some(3),
    }
}

#[test]
fn agent_tools_override_spoofed_identity_and_require_a_current_attempt() {
    let engine = FakeEngine {
        role: Role::Agent,
        calls: VecDeque::new(),
    };
    let mut service = Service::new(engine, context(true));
    service
        .call("task_comment", json!({"text":"milestone","taskId":"other"}))
        .unwrap();
    let (method, params) = service.engine.calls.pop_front().unwrap();
    assert_eq!(method, "task.comment");
    assert_eq!(params["workspaceId"], "workspace");
    assert_eq!(params["taskId"], "WS-1");
    assert_eq!(params["paneId"], "pane");
    assert_eq!(params["sessionGeneration"], 7);
    assert_eq!(params["attempt"], 3);
    assert!(params["idempotencyKey"].as_str().is_some());
    assert!(matches!(
        service.call("task_comment", json!({"text":"x","attempt":99})),
        Err(Error::InvalidArguments("unknown tool argument"))
    ));

    let engine = FakeEngine {
        role: Role::Agent,
        calls: VecDeque::new(),
    };
    let mut service = Service::new(engine, context(false));
    assert!(matches!(
        service.call(
            "task_complete",
            json!({"summary":"done","contractChanges":[],"outOfScopeEdits":[]})
        ),
        Err(Error::TaskContextRequired)
    ));
}

#[test]
fn peer_tools_use_binding_identity_and_validate_target() {
    let engine = FakeEngine {
        role: Role::Agent,
        calls: VecDeque::new(),
    };
    let mut service = Service::new(engine, context(false));
    let listing = service.list();
    let names = listing["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"list_agents"));
    assert!(names.contains(&"message_agent"));
    service.call("list_agents", json!({})).unwrap();
    assert_eq!(service.engine.calls.pop_front().unwrap().0, "agent.list");
    service
        .call(
            "message_agent",
            json!({"targetPaneId":"recipient","text":"hello"}),
        )
        .unwrap();
    let (method, params) = service.engine.calls.pop_front().unwrap();
    assert_eq!(method, "agent.message");
    assert_eq!(params["targetPaneId"], "recipient");
    assert!(params["idempotencyKey"].as_str().is_some());
    assert!(params.get("paneId").is_none());
    assert!(
        service
            .call(
                "message_agent",
                json!({"targetPaneId":"recipient","text":"hello","paneId":"spoof"})
            )
            .is_err()
    );
}

#[test]
fn role_lists_are_closed_and_the_lead_cannot_complete() {
    let agent = Service::new(
        FakeEngine {
            role: Role::Agent,
            calls: VecDeque::new(),
        },
        context(false),
    );
    let chief = Service::new(
        FakeEngine {
            role: Role::Chief,
            calls: VecDeque::new(),
        },
        context(false),
    );
    let names = |value: Value| {
        value["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let agent_names = names(agent.list());
    let chief_names = names(chief.list());
    assert!(
        TOOLS
            .iter()
            .all(|tool| tool.method != "pane.experiment.hook")
    );
    assert!(agent_names.contains(&"task_next".into()));
    assert!(agent_names.contains(&"task_complete".into()));
    assert!(!agent_names.contains(&"task_dispatch".into()));
    assert!(chief_names.contains(&"task_dispatch".into()));
    assert!(chief_names.contains(&"task_list".into()));
    assert!(!chief_names.contains(&"task_complete".into()));
    for name in ["writing_structure", "writing_variant_add"] {
        assert!(agent_names.contains(&name.into()));
        assert!(chief_names.contains(&name.into()));
    }
    for name in [
        "web_open",
        "web_navigate",
        "web_snapshot",
        "web_click",
        "web_type",
        "web_press",
        "web_hover",
        "web_select",
        "web_scroll",
        "web_wait",
        "web_evaluate",
        "web_screenshot",
        "web_read",
        "web_console",
        "web_network",
        "web_resize",
        "web_close",
    ] {
        assert!(agent_names.contains(&name.into()));
        assert!(chief_names.contains(&name.into()));
    }
    let agent_tools = agent.list();
    let comment = agent_tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "task_comment")
        .unwrap();
    assert!(comment["inputSchema"]["properties"].get("taskId").is_none());
    let chief_tools = chief.list();
    let update = chief_tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "task_update")
        .unwrap();
    assert!(
        update["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("taskId"))
    );
}

#[test]
fn every_advertised_tool_builds_protocol_valid_parameters() {
    let mut agent = Service::new(
        FakeEngine {
            role: Role::Agent,
            calls: VecDeque::new(),
        },
        context(true),
    );
    for (name, arguments) in [
        ("task_next", json!({})),
        ("task_show", json!({})),
        ("task_update", json!({"version":1,"title":"Revised"})),
        ("task_comment", json!({"text":"Milestone"})),
        ("task_subitem_add", json!({"text":"Plan"})),
        ("task_subitem_done", json!({"subitemId":"WS-1-T01"})),
        (
            "task_block",
            json!({"type":"missing_information","reason":"Input required"}),
        ),
        ("task_release", json!({})),
        ("task_request_review", json!({})),
        (
            "task_complete",
            json!({"summary":"Done","contractChanges":[],"outOfScopeEdits":[]}),
        ),
        (
            "loop_set",
            json!({"prompt":"Continue","interval":5,"maxRounds":10,"stopWhen":"round_limit","enabled":true}),
        ),
        (
            "writing_structure",
            json!({"fileId":"file-1","revision":"1"}),
        ),
        (
            "writing_variant_add",
            json!({"fileId":"file-1","lineId":"element-3","baseRevision":"1","text":"Another take."}),
        ),
    ] {
        assert!(
            service_call_succeeded(&mut agent, name, arguments),
            "{name} did not produce protocol-valid parameters"
        );
    }
    assert_web_tools(&mut agent);
    let web_open = agent
        .engine
        .calls
        .iter()
        .find(|(method, _)| method == "web.open")
        .map(|(_, params)| params)
        .unwrap();
    assert_eq!(web_open["workspaceId"], "workspace");
    assert!(web_open["idempotencyKey"].as_str().is_some());
    let snapshot = agent
        .engine
        .calls
        .iter()
        .find(|(method, _)| method == "web.snapshot")
        .map(|(_, params)| params)
        .unwrap();
    assert!(snapshot.get("idempotencyKey").is_none());

    let mut chief = Service::new(
        FakeEngine {
            role: Role::Chief,
            calls: VecDeque::new(),
        },
        context(false),
    );
    for (name, arguments) in [
        (
            "task_create",
            json!({"title":"Build","mustNotChange":"Existing behavior","executor":{"workspaceDefault":true}}),
        ),
        ("task_show", json!({"taskId":"WS-1"})),
        ("task_list", json!({"limit":20})),
        (
            "task_update",
            json!({"taskId":"WS-1","version":1,"priority":"high"}),
        ),
        (
            "task_comment",
            json!({"taskId":"WS-1","text":"Review this"}),
        ),
        (
            "task_dispatch",
            json!({"taskId":"WS-1","preset":{"workspaceDefault":true}}),
        ),
        ("task_unblock", json!({"taskId":"WS-1"})),
        (
            "loop_set",
            json!({"prompt":"Coordinate","interval":5,"maxRounds":10,"stopWhen":"three_empty","enabled":true}),
        ),
    ] {
        assert!(
            service_call_succeeded(&mut chief, name, arguments),
            "{name} did not produce protocol-valid parameters"
        );
    }
}

fn assert_web_tools(service: &mut Service<FakeEngine>) {
    for (name, arguments) in [
        (
            "web_open",
            json!({"url":"localhost:3000","workspaceId":"spoofed"}),
        ),
        ("web_navigate", json!({"paneId":"web","action":"reload"})),
        ("web_snapshot", json!({"paneId":"web","screenshot":true})),
        ("web_click", json!({"paneId":"web","ref":"e1"})),
        (
            "web_type",
            json!({"paneId":"web","ref":"e1","text":"Hello","submit":true}),
        ),
        ("web_press", json!({"paneId":"web","key":"Enter"})),
        ("web_hover", json!({"paneId":"web","ref":"e2"})),
        (
            "web_select",
            json!({"paneId":"web","ref":"e3","value":"one"}),
        ),
        (
            "web_scroll",
            json!({"paneId":"web","ref":"e4","dx":0,"dy":600}),
        ),
        (
            "web_wait",
            json!({"paneId":"web","text":"Ready","timeoutMs":5000}),
        ),
        (
            "web_evaluate",
            json!({"paneId":"web","js":"document.title"}),
        ),
        ("web_screenshot", json!({"paneId":"web","fullPage":false})),
        ("web_read", json!({"paneId":"web","screens":6})),
        ("web_console", json!({"paneId":"web","since":"0"})),
        ("web_network", json!({"paneId":"web","since":"0"})),
        ("web_resize", json!({"paneId":"web","preset":"responsive"})),
        ("web_close", json!({"paneId":"web"})),
    ] {
        assert!(
            service_call_succeeded(service, name, arguments),
            "{name} did not produce protocol-valid parameters"
        );
    }
}

fn service_call_succeeded(service: &mut Service<FakeEngine>, name: &str, arguments: Value) -> bool {
    service.call(name, arguments).is_ok()
}

/// A host that answers with a picture of this many bytes.
struct ImageEngine(usize);

impl EnginePort for ImageEngine {
    fn role(&self) -> &Role {
        &Role::Agent
    }

    fn request(&mut self, _method: &str, _params: Value) -> Result<Value> {
        Err(Error::UnexpectedFrame)
    }

    fn request_with_attachment(&mut self, method: &str, _params: Value) -> Result<EngineResponse> {
        assert!(matches!(method, "web.screenshot" | "web.read"));
        let reference = AttachmentRef {
            attachment_id: "00112233-4455-4677-8899-aabbccddeeff".into(),
            mime_type: "image/png".into(),
            name: "screenshot.png".into(),
            bytes: self.0.to_string(),
        };
        Ok(EngineResponse {
            value: json!({"paneId":"web","image":reference}),
            attachment: Some(EngineAttachment {
                reference,
                bytes: b"png".repeat(self.0 / 3 + 1)[..self.0].to_vec(),
            }),
        })
    }
}

#[test]
fn a_picture_over_the_limit_is_left_out_and_the_agent_is_told() {
    let mut service = Service::new(
        ImageEngine(soyeht_protocol::limits::WEB_AGENT_IMAGE_BYTES + 1),
        context(false),
    );
    let result = service
        .call("web_read", json!({"paneId":"web","screens":3}))
        .unwrap();
    assert_eq!(result["content"].as_array().unwrap().len(), 2);
    assert_eq!(result["content"][1]["type"], "text");
    assert!(
        result["content"][1]["text"]
            .as_str()
            .unwrap()
            .contains("left out")
    );
}

#[test]
fn screenshot_tool_returns_mcp_image_content_and_structured_reference() {
    let mut service = Service::new(ImageEngine(3), context(false));
    let result = service
        .call("web_screenshot", json!({"paneId":"web"}))
        .unwrap();
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["content"][1]["mimeType"], "image/png");
    assert_eq!(result["content"][1]["data"], "cG5n");
    assert_eq!(result["structuredContent"]["image"]["bytes"], "3");
}

#[test]
fn read_tool_bounds_its_screens_and_returns_the_image_with_the_page_text() {
    let mut service = Service::new(ImageEngine(3), context(false));
    let result = service
        .call("web_read", json!({"paneId":"web","screens":3}))
        .unwrap();
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["structuredContent"]["image"]["bytes"], "3");
    let over = soyeht_protocol::limits::WEB_READ_SCREENS + 1;
    assert!(
        service
            .call("web_read", json!({"paneId":"web","screens":over}))
            .is_err()
    );
    assert!(
        service
            .call("web_read", json!({"paneId":"web","screens":0}))
            .is_err()
    );
}
