//! The APNs payload for a question an agent asks the person.
//!
//! The payload carries the same generic words for every question, and the identifiers the phone
//! needs to open the pane. What was asked (a command, a path, a question) is not in it: Apple's
//! servers and the lock screen would see it. The phone fetches the detail over its authenticated
//! connection when the person opens the alert.

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{
    client_policy::{PUSH_NEEDS_YOU_BODY, PUSH_NEEDS_YOU_TITLE},
    event,
};

/// No actions: a tap opens the pane. The alert says nothing of what is asked, so it offers no
/// answer to give from the lock screen.
pub const ATTENTION_CATEGORY: &str = "soyeht.attention";

/// APNs refuses alert payloads larger than 4 KB.
const PAYLOAD_BYTES: usize = 4 * 1024;
/// APNs refuses an `apns-collapse-id` longer than 64 bytes.
const COLLAPSE_ID_BYTES: usize = 64;

/// A durable question event and what the engine knows about its pane.
pub struct Question<'a> {
    pub event: &'a str,
    pub request: &'a Value,
    pub workspace_id: &'a str,
    pub engine_id: &'a str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub request_id: String,
    pub collapse_id: String,
    /// The request's own deadline; APNs stops trying after it.
    pub expires_unix: i64,
    pub payload: Value,
}

fn collapse_id(request_id: &str) -> String {
    if request_id.len() <= COLLAPSE_ID_BYTES {
        request_id.to_owned()
    } else {
        format!("{:x}", Sha256::digest(request_id.as_bytes()))
    }
}

/// Builds the alert, or `None` when the event is not a question or the
/// request lacks the identifiers the phone needs.
pub fn alert(question: &Question<'_>) -> Option<Alert> {
    let request = question.request;
    let request_id = request["requestId"].as_str()?;
    let pane_id = request["paneId"].as_str()?;
    let expires_unix = request["expiresAt"]
        .as_str()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map_or(0, |at| at.timestamp());
    let kind = match question.event {
        event::PANE_PERMISSION_REQUESTED => "permission",
        event::PANE_ELICITATION_REQUESTED => "question",
        _ => return None,
    };
    let mut custom = Map::new();
    custom.insert("engineId".into(), json!(question.engine_id));
    custom.insert("workspaceId".into(), json!(question.workspace_id));
    custom.insert("paneId".into(), json!(pane_id));
    custom.insert("requestId".into(), json!(request_id));
    custom.insert("kind".into(), json!(kind));
    let payload = json!({
        "aps": {
            "alert": {"title": PUSH_NEEDS_YOU_TITLE, "body": PUSH_NEEDS_YOU_BODY},
            "category": ATTENTION_CATEGORY,
            "sound": "default",
            "thread-id": pane_id,
        },
        "soyeht": custom,
    });
    if serde_json::to_vec(&payload).ok()?.len() > PAYLOAD_BYTES {
        return None;
    }
    Some(Alert {
        request_id: request_id.to_owned(),
        collapse_id: collapse_id(request_id),
        expires_unix,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTOR: &str = include_str!("../../../../contracts/vectors/push-permission.json");
    const SECRET: &str = "curl -H 'Authorization: Bearer hunter2' https://example.test";

    fn permission(command: &str) -> Value {
        json!({
            "requestId":"6f1c7a2e-0d5b-4e8f-9a51-2c3d4e5f6a7b",
            "paneId":"0b9f1e4c-5a6d-4c3b-8e2f-1a2b3c4d5e6f",
            "generation":1,"promptId":"prompt","title":"Run command","detail":"",
            "presentation":{"command":command,"fileChanges":[],"previewOmitted":false},
            "options":[
                {"optionId":"allow","kind":"allow_once","name":"Allow"},
                {"optionId":"reject","kind":"reject_once","name":"Reject"}
            ],
            "expiresAt":"2026-10-01T12:30:00.000Z"
        })
    }

    fn question<'a>(event: &'a str, request: &'a Value) -> Question<'a> {
        Question {
            event,
            request,
            workspace_id: "3c2b1a0f-9e8d-4c7b-a6f5-e4d3c2b1a0f9",
            engine_id: "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d",
        }
    }

    #[test]
    fn permission_payload_matches_the_shared_vector() {
        let request = permission("git push origin main");
        let alert = alert(&question("pane.permission_requested", &request)).unwrap();
        let vector: Value = serde_json::from_str(VECTOR).unwrap();
        assert_eq!(alert.payload, vector);
        assert_eq!(alert.collapse_id, "6f1c7a2e-0d5b-4e8f-9a51-2c3d4e5f6a7b");
        assert_eq!(alert.expires_unix, 1_790_857_800);
    }

    #[test]
    fn nothing_the_agent_asked_reaches_the_alert() {
        let mut request = permission(SECRET);
        request["title"] = json!("Edit /home/someone/.ssh/config");
        request["detail"] = json!(SECRET);
        request["domain"] = json!("internal.example.test");
        request["presentation"]["targetPath"] = json!("/home/someone/.ssh/config");
        let asked = alert(&question("pane.permission_requested", &request)).unwrap();
        let form = json!({
            "requestId":"r","paneId":"p","generation":1,"promptId":"x",
            "title":SECRET,"schema":{},"expiresAt":"bad"
        });
        let formed = alert(&question("pane.elicitation_requested", &form)).unwrap();
        for alert in [asked, formed] {
            let text = alert.payload.to_string();
            for leaked in [
                "hunter2",
                "curl",
                ".ssh",
                "internal.example.test",
                "Run command",
            ] {
                assert!(!text.contains(leaked), "{leaked} in {text}");
            }
            assert_eq!(alert.payload["aps"]["alert"]["title"], PUSH_NEEDS_YOU_TITLE);
            assert_eq!(alert.payload["aps"]["alert"]["body"], PUSH_NEEDS_YOU_BODY);
        }
    }

    #[test]
    fn an_alert_offers_no_answer_to_give_from_the_lock_screen() {
        let request = permission("ls");
        let alert = alert(&question("pane.permission_requested", &request)).unwrap();
        assert_eq!(alert.payload["aps"]["category"], ATTENTION_CATEGORY);
        assert!(alert.payload["soyeht"].get("allowOptionId").is_none());
        assert!(alert.payload["soyeht"].get("denyOptionId").is_none());
        assert_eq!(alert.payload["soyeht"]["kind"], "permission");
    }

    #[test]
    fn a_form_question_opens_the_pane() {
        let request = json!({
            "requestId":"r","paneId":"p","generation":1,"promptId":"x",
            "title":"Which branch\nshould I use?","schema":{},"expiresAt":"bad"
        });
        let alert = alert(&question("pane.elicitation_requested", &request)).unwrap();
        assert_eq!(alert.payload["aps"]["category"], ATTENTION_CATEGORY);
        assert_eq!(alert.payload["soyeht"]["kind"], "question");
        assert_eq!(alert.expires_unix, 0);
    }

    #[test]
    fn long_request_ids_collapse_to_a_bounded_digest() {
        let mut request = permission("ls");
        request["requestId"] = json!("r".repeat(128));
        let alert = alert(&question("pane.permission_requested", &request)).unwrap();
        assert_eq!(alert.collapse_id.len(), COLLAPSE_ID_BYTES);
        assert_eq!(alert.payload["soyeht"]["requestId"], "r".repeat(128));
    }

    #[test]
    fn other_events_and_incomplete_requests_produce_nothing() {
        let request = permission("ls");
        assert!(alert(&question("pane.updated", &request)).is_none());
        let missing = json!({"paneId":"p"});
        assert!(alert(&question("pane.permission_requested", &missing)).is_none());
    }
}
