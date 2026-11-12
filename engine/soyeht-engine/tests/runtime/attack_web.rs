//! Attacks through the address of a web pane: the schemes that run code or read files, an
//! address with credentials in it, and the number of panes a workspace may hold. The policy
//! refuses with `navigation_refused`, and no pane is made.
use super::*;
use soyeht_protocol::limits;

fn kind(answer: &Value) -> &str {
    answer["error"]["data"]["kind"]
        .as_str()
        .unwrap_or("<no error>")
}

const FORBIDDEN: &[&str] = &[
    "file:///etc/passwd",
    "FILE:///etc/passwd",
    "file://localhost/etc/passwd",
    "javascript:alert(document.cookie)",
    "JaVaScRiPt:alert(1)",
    " javascript:alert(1)",
    "java\nscript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "vbscript:msgbox(1)",
    "blob:https://example.com/00000000-0000-0000-0000-000000000000",
    "about:blank",
    "chrome://settings",
    "view-source:https://example.com",
    "ftp://example.com/file",
    "ws://example.com/socket",
    "http://user:password@example.com/",
    "https://token@example.com/",
    "http://exa mple.com/",
    "https://",
    "   ",
];

fn web_panes(client: &mut Client) -> usize {
    client
        .section("panes")
        .iter()
        .filter(|pane| pane["mode"] == "web")
        .count()
}

#[test]
fn an_address_the_policy_forbids_makes_no_pane_by_any_method() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = client.add_workspace(&rig.path, "web-attack");
    let host_pane = client.call(
        "web.open",
        json!({"workspaceId":workspace,"url":"https://example.com/","idempotencyKey":"good"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = web_panes(&mut client);
    for (index, url) in FORBIDDEN.iter().enumerate() {
        let opened = client.request(
            "web.open",
            json!({"workspaceId":workspace,"url":url,"newPane":true,
                "idempotencyKey":format!("open-{index}")}),
        );
        assert_eq!(
            kind(&opened),
            "navigation_refused",
            "web.open {url:?}: {opened}"
        );
        let created = client.request(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"web","url":url,
                "idempotencyKey":format!("create-{index}")}),
        );
        assert_eq!(
            kind(&created),
            "navigation_refused",
            "pane.create {url:?}: {created}"
        );
        let navigated = client.request(
            "web.navigate",
            json!({"paneId":host_pane,"url":url,"idempotencyKey":format!("go-{index}")}),
        );
        assert_eq!(
            kind(&navigated),
            "navigation_refused",
            "web.navigate {url:?}: {navigated}"
        );
    }
    assert_eq!(web_panes(&mut client), before);
    // The pane still shows the address it had.
    assert_eq!(client.pane(&host_pane)["url"], "https://example.com/");
}

#[test]
fn a_workspace_holds_no_more_web_panes_than_the_contract_allows() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = client.add_workspace(&rig.path, "web-flood");
    for index in 0..limits::WEB_PANES_PER_WORKSPACE {
        client.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"web","url":format!("https://example.com/{index}"),
                "idempotencyKey":format!("pane-{index}")}),
        );
    }
    let over = client.request(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"web","url":"https://example.com/over",
            "idempotencyKey":"over"}),
    );
    assert_eq!(kind(&over), "invalid_params", "{over}");
    assert_eq!(over["error"]["message"], "Workspace web pane limit reached");
    assert_eq!(web_panes(&mut client), limits::WEB_PANES_PER_WORKSPACE);
}
