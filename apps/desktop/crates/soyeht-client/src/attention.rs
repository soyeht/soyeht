//! What needs the person, and when the window tells them while it is not in front.
//!
//! This is the Mac's `AgentAttentionPolicy` and `AgentAttentionNotificationCursor`: a request is a
//! pending permission or question of an agent pane, the first connected view is existing state and
//! not news, and a request that was seen once is never announced again. The cases both clients
//! pass are `contracts/vectors/agent-attention.json`.

use crate::agent::AgentView;
use soyeht_protocol::{Pane, PaneMode};
use std::collections::HashSet;

/// A permission or a question an agent waits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub request_id: String,
    pub pane_id: String,
    pub workspace_id: String,
    pub pane_name: String,
    pub generation: u32,
}

impl Request {
    /// Names the request across reconnects: the pane, the session generation and the request.
    pub fn identity(&self) -> String {
        format!("{}:{}:{}", self.pane_id, self.generation, self.request_id)
    }
}

/// The agent panes of `panes` with the state the window holds for them. `view` gives that state.
fn agent_views<'a>(
    panes: &'a [Pane],
    view: impl Fn(&str) -> Option<&'a AgentView> + 'a,
) -> impl Iterator<Item = (&'a Pane, &'a AgentView)> {
    panes
        .iter()
        .filter(|pane| pane.mode == PaneMode::Agent)
        .filter_map(move |pane| view(&pane.pane_id).map(|view| (pane, view)))
}

/// What an agent pane waits on: the id and generation of its permission and of its question.
fn asked(view: &AgentView) -> impl Iterator<Item = (&String, u32)> {
    [
        view.permission
            .as_ref()
            .map(|request| (&request.request_id, request.generation)),
        view.cards
            .elicitation
            .as_ref()
            .map(|request| (&request.request_id, request.generation)),
    ]
    .into_iter()
    .flatten()
}

/// The requests the agent panes of `panes` wait on, ordered by identity. `view` gives the state
/// the window holds for an agent pane.
pub fn requests<'a>(
    panes: &'a [Pane],
    view: impl Fn(&str) -> Option<&'a AgentView> + 'a,
) -> Vec<Request> {
    let mut found: Vec<Request> = agent_views(panes, view)
        .flat_map(|(pane, view)| {
            asked(view).map(|(request_id, generation)| Request {
                request_id: request_id.clone(),
                pane_id: pane.pane_id.clone(),
                workspace_id: pane.workspace_id.clone(),
                pane_name: pane.name.clone(),
                generation,
            })
        })
        .collect();
    found.sort_by_cached_key(Request::identity);
    found
}

/// How many requests the panes of `workspace` (or of all of them) wait on: [`requests`] without
/// building them, for badges that count on every frame.
pub fn count<'a>(
    panes: &'a [Pane],
    view: impl Fn(&str) -> Option<&'a AgentView> + 'a,
    workspace: Option<&str>,
) -> usize {
    agent_views(panes, view)
        .filter(|(pane, _)| workspace.is_none_or(|id| pane.workspace_id == id))
        .map(|(_, view)| asked(view).count())
        .sum()
}

/// Whether the request a notification named is still pending in that pane and workspace, so that
/// a click can open it.
pub fn is_pending(requests: &[Request], workspace_id: &str, pane_id: &str, identity: &str) -> bool {
    requests.iter().any(|request| {
        request.workspace_id == workspace_id
            && request.pane_id == pane_id
            && request.identity() == identity
    })
}

/// Tells which requests are new. Retaining what was seen keeps a reconnect from announcing it
/// again.
#[derive(Debug, Default)]
pub struct Cursor {
    seen: HashSet<String>,
    seeded: bool,
}

impl Cursor {
    /// The requests of `current` the person has not been told about. While the engine is out of
    /// reach nothing is new, and the first connected view only seeds the cursor.
    pub fn new_requests(&mut self, current: &[Request], connected: bool) -> Vec<Request> {
        if !connected {
            return Vec::new();
        }
        let first = !self.seeded;
        self.seeded = true;
        let new: Vec<Request> = current
            .iter()
            .filter(|request| !self.seen.contains(&request.identity()))
            .cloned()
            .collect();
        self.seen.extend(current.iter().map(Request::identity));
        if first { Vec::new() } else { new }
    }
}

/// Whether notifications are on until the person turns them off: on both clients.
pub const ENABLED_BY_DEFAULT: bool = true;

/// The first line of the notification.
pub const TITLE: &str = "Agent needs your input";
const UNKNOWN_WORKSPACE: &str = "Workspace";

/// The second line: the pane that asks and the workspace it is in.
pub fn body(pane_name: &str, workspace_name: Option<&str>) -> String {
    format!(
        "{pane_name} \u{b7} {}",
        workspace_name.unwrap_or(UNKNOWN_WORKSPACE)
    )
}

/// Whether a notification is raised: the person allowed them and the window is not in front.
pub fn should_post(enabled: bool, window_active: bool) -> bool {
    enabled && !window_active
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cases, pane, strings, vector};
    use serde_json::{Value, json};
    use soyeht_protocol::{ElicitationRequest, PermissionRequest};
    use std::collections::HashMap;

    fn text(value: &Value) -> String {
        value.as_str().unwrap().to_owned()
    }

    fn request(identity: &str, workspace_id: &str) -> Request {
        let mut parts = identity.split(':');
        let pane_id = parts.next().unwrap().to_owned();
        Request {
            request_id: parts.next_back().unwrap().to_owned(),
            generation: parts.next().unwrap().parse().unwrap(),
            pane_name: pane_id.clone(),
            pane_id,
            workspace_id: workspace_id.to_owned(),
        }
    }

    #[test]
    fn what_the_panes_ask_is_the_shared_vector() {
        for case in cases("agent-attention", "requests") {
            let mut panes = Vec::new();
            let mut views: HashMap<String, AgentView> = HashMap::new();
            for entry in case["panes"].as_array().unwrap() {
                let id = text(&entry["paneId"]);
                let mode = if entry["mode"] == "agent" {
                    "agent"
                } else {
                    "terminal"
                };
                let mut model = pane(&id, &text(&entry["workspaceId"]), mode);
                model.name = text(&entry["name"]);
                panes.push(model);
                let mut view = AgentView::default();
                if let Some(ask) = entry.get("permission") {
                    view.permission = Some(
                        serde_json::from_value::<PermissionRequest>(json!({
                            "requestId": ask["requestId"], "paneId": id,
                            "generation": ask["generation"], "promptId": "prompt", "title": "",
                            "detail": "", "options": [], "expiresAt": "later"
                        }))
                        .unwrap(),
                    );
                }
                if let Some(ask) = entry.get("elicitation") {
                    view.cards.elicitation = Some(
                        serde_json::from_value::<ElicitationRequest>(json!({
                            "requestId": ask["requestId"], "paneId": id,
                            "generation": ask["generation"], "promptId": "prompt", "title": "",
                            "schema": {}, "expiresAt": "later"
                        }))
                        .unwrap(),
                    );
                }
                views.insert(id, view);
            }
            let found = requests(&panes, |id| views.get(id));
            assert_eq!(
                count(&panes, |id| views.get(id), None),
                found.len(),
                "{}",
                text(&case["name"])
            );
            let name = text(&case["name"]);
            let identities: Vec<String> = found.iter().map(Request::identity).collect();
            assert_eq!(identities, strings(&case["identities"]), "{name}");
            assert_eq!(
                found.len() as u64,
                case["total"].as_u64().unwrap(),
                "{name}"
            );
            for (workspace, count) in case["byWorkspace"].as_object().unwrap() {
                let here = found
                    .iter()
                    .filter(|r| &r.workspace_id == workspace)
                    .count();
                assert_eq!(here as u64, count.as_u64().unwrap(), "{name} {workspace}");
            }
        }
    }

    #[test]
    fn the_cursor_announces_what_the_shared_vector_says() {
        for case in cases("agent-attention", "cursor") {
            let mut cursor = Cursor::default();
            for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
                let current: Vec<Request> = strings(&step["identities"])
                    .iter()
                    .map(|identity| request(identity, "w"))
                    .collect();
                let new = cursor.new_requests(&current, step["connected"].as_bool().unwrap());
                let identities: Vec<String> = new.iter().map(Request::identity).collect();
                assert_eq!(
                    identities,
                    strings(&step["new"]),
                    "{} step {index}",
                    text(&case["name"])
                );
            }
        }
    }

    #[test]
    fn the_notification_text_is_the_shared_vector() {
        let vector = vector("agent-attention");
        assert_eq!(TITLE, vector["notification"]["title"].as_str().unwrap());
        for item in vector["notification"]["bodies"].as_array().unwrap() {
            assert_eq!(
                body(&text(&item["paneName"]), item["workspaceName"].as_str()),
                text(&item["body"])
            );
        }
    }

    #[test]
    fn a_notification_is_raised_as_the_shared_vector_says() {
        for item in cases("agent-attention", "delivery") {
            assert_eq!(
                should_post(
                    item["enabled"].as_bool().unwrap(),
                    item["appActive"].as_bool().unwrap()
                ),
                item["post"].as_bool().unwrap()
            );
        }
    }

    #[test]
    fn notifications_are_on_by_default_as_the_shared_vector_says() {
        assert_eq!(
            ENABLED_BY_DEFAULT,
            vector("agent-attention")["enabledByDefault"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn a_click_opens_only_a_request_that_is_still_pending() {
        for item in cases("agent-attention", "route") {
            let pending = &item["pending"];
            let held: Vec<Request> = item["requests"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    request(
                        r["identity"].as_str().unwrap(),
                        r["workspaceId"].as_str().unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                is_pending(
                    &held,
                    pending["workspaceId"].as_str().unwrap(),
                    pending["paneId"].as_str().unwrap(),
                    pending["identity"].as_str().unwrap()
                ),
                item["openable"].as_bool().unwrap(),
                "{}",
                text(&item["name"])
            );
        }
    }
}
