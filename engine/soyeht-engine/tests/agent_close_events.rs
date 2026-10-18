//! What a client is told when an agent pane closes: a replica that applies the events in order
//! must end without the pane, so no state or pane update may come with `pane.closed`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;

mod support;
use support::*;

#[test]
fn closing_an_agent_pane_publishes_no_update_of_it_beside_pane_closed() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let before = client.snapshot().0["cursor"].clone();
    client.call(
        "pane.close",
        json!({"paneId":pane,"idempotencyKey":"close-agent-pane"}),
    );
    client.messages.clear();
    client.call("events.subscribe", json!({"since":before}));

    let closed = client.notifications("pane.closed");
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!(closed[0]["paneId"], pane);
    for update in ["pane.updated", "pane.state_changed"] {
        let about_pane: Vec<_> = client
            .notifications(update)
            .into_iter()
            .filter(|params| params["paneId"] == pane || params["pane"]["paneId"] == pane)
            .collect();
        assert!(
            about_pane.is_empty(),
            "{update} after the close: {about_pane:?}"
        );
    }
}
