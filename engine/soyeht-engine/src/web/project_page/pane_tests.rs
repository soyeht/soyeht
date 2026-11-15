//! The project page as the Core holds it: opened, focused, restored, held to its origin and reloaded.
use super::tests::{ask, ends, get, port_of, write};
use super::*;
use crate::{
    core::Core,
    dispatch::Outcome,
    errors,
    outbox::Receiver,
    web::test_support::{connect_local, create_workspace, next_json, reply, request, temp_root},
};
use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, PaneMode};
use std::{fs, time::Duration};

#[tokio::test]
async fn web_open_of_a_project_file_serves_it_in_a_pane_and_ends_with_the_pane() {
    let root = temp_root();
    write(&root, "pages/cards.html", "cards");
    write(&root, "other.html", "other");
    let state = temp_root();
    let mut core = Core::open(&state).unwrap();
    let (connection, _receiver) = connect_local(&mut core, "project-page-test", json!([])).await;
    let workspace = create_workspace(&mut core, &connection, &root);
    let open = |core: &mut Core, id: u64, mut params: Value| {
        params["workspaceId"] = json!(workspace);
        params["idempotencyKey"] = json!(format!("open-{id}"));
        core.dispatch(&connection, request(id, "web.open", params))
    };
    let Ok(Outcome::Reply(first)) = open(&mut core, 3, json!({"path":"pages/cards.html"})) else {
        panic!("opening a project file should reply")
    };
    let pane = first["paneId"].as_str().unwrap().to_owned();
    let url = core.panes[&pane].stored.pane.url.clone().unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:") && url.ends_with("/cards.html"),
        "{url}"
    );
    assert_eq!(ask(&url, get).await.1, b"cards");

    // The same file focuses its pane; a new pane is asked for; another file is another pane.
    let Ok(Outcome::Reply(again)) = open(&mut core, 4, json!({"path":"./pages/cards.html"})) else {
        panic!("opening the same file should reply")
    };
    assert_eq!(again["paneId"], json!(pane));
    let Ok(Outcome::Reply(second)) = open(
        &mut core,
        5,
        json!({"path":"pages/cards.html","newPane":true}),
    ) else {
        panic!("opening a new pane should reply")
    };
    assert_ne!(second["paneId"], json!(pane));
    let Ok(Outcome::Reply(other)) = open(&mut core, 6, json!({"path":"other.html"})) else {
        panic!("opening another file should reply")
    };
    assert_ne!(other["paneId"], json!(pane));
    assert_eq!(
        core.panes
            .values()
            .filter(|pane| pane.stored.pane.mode == PaneMode::Web)
            .count(),
        3
    );

    // The call names exactly one address, inside the workspace, of a file that is there.
    for (id, params) in [
        (7, json!({})),
        (8, json!({"path":"other.html","url":"example.com"})),
        (9, json!({"path":"../escape.html"})),
        (10, json!({"path":"missing.html"})),
    ] {
        assert!(open(&mut core, id, params).is_err(), "{id}");
    }

    reply(
        &mut core,
        &connection,
        request(
            11,
            "web.close",
            json!({"paneId":pane,"idempotencyKey":"close"}),
        ),
    );
    assert!(!core.web.pages.pages.contains_key(&pane));
    assert!(
        ends(port_of(&url)).await,
        "closing the pane left its page served"
    );
    crate::test_paths::remove_after(core, state);
    fs::remove_dir_all(root).unwrap();
}

/// A core with a web host and a caller, and the page of `cards.html` open in a pane.
struct Hosted {
    core: Core,
    state: std::path::PathBuf,
    host: String,
    host_receiver: Receiver,
    caller: String,
    _caller_receiver: Receiver,
    workspace: String,
    pane: String,
}

async fn hosted_page(root: &Path) -> Hosted {
    write(root, "cards.html", "cards");
    let state = temp_root();
    let mut core = Core::open(&state).unwrap();
    let (host, host_receiver) =
        connect_local(&mut core, "project-page-host", json!(["web_host"])).await;
    let (caller, caller_receiver) =
        connect_local(&mut core, "project-page-caller", json!([])).await;
    let workspace = create_workspace(&mut core, &caller, root);
    let opened = reply(
        &mut core,
        &caller,
        request(
            3,
            "web.open",
            json!({"workspaceId":workspace,"path":"cards.html","idempotencyKey":"open"}),
        ),
    );
    let pane = opened["paneId"].as_str().unwrap().to_owned();
    Hosted {
        core,
        state,
        host,
        host_receiver,
        caller,
        _caller_receiver: caller_receiver,
        workspace,
        pane,
    }
}

async fn nothing_sent(receiver: &mut Receiver) -> bool {
    tokio::time::timeout(Duration::from_millis(150), receiver.next())
        .await
        .is_err()
}

#[tokio::test]
async fn a_changed_project_page_is_reloaded_in_the_pane_that_still_shows_it() {
    let root = temp_root();
    let Hosted {
        mut core,
        state,
        host,
        mut host_receiver,
        pane,
        _caller_receiver,
        ..
    } = hosted_page(&root).await;
    let changes = Arc::clone(&core.web.pages.pages[&pane].changes);

    core.reload_project_pages().unwrap();
    assert!(
        nothing_sent(&mut host_receiver).await,
        "nothing changed yet"
    );

    changes.fetch_add(1, Ordering::Relaxed);
    core.reload_project_pages().unwrap();
    let command = next_json(&mut host_receiver).await;
    assert_eq!(command["method"], "web.command");
    assert_eq!(command["params"]["paneId"], json!(pane));
    assert_eq!(command["params"]["command"], "navigate");
    assert_eq!(command["params"]["args"]["action"], "reload");
    // The engine's own reload is no act of a person: no host may select the pane for it.
    assert_eq!(
        command["params"]["args"]["_soyehtAccess"]["background"],
        true
    );
    assert_eq!(
        command["params"]["args"]["_soyehtAccess"]["session"],
        "human"
    );
    // The host's answer to a command nobody waits for is dropped without harm.
    core.web_host_response(
        &host,
        json!({"jsonrpc":"2.0","id":command["id"],"result":{"paneId":pane}}),
    )
    .unwrap();

    // A change that came while the pane showed another address reloads nothing.
    core.panes.get_mut(&pane).unwrap().stored.pane.url = Some("https://example.com/".into());
    changes.fetch_add(1, Ordering::Relaxed);
    core.reload_project_pages().unwrap();
    assert!(nothing_sent(&mut host_receiver).await);
    crate::test_paths::remove_after(core, state);
    fs::remove_dir_all(root).unwrap();
}

async fn open_path(
    core: &mut Core,
    workspace: &str,
    id: u64,
    path: &str,
    new_pane: bool,
) -> String {
    let connection = core_connection(core);
    let opened = reply(
        core,
        &connection,
        request(
            id,
            "web.open",
            json!({"workspaceId":workspace,"path":path,"newPane":new_pane,
                "idempotencyKey":format!("open-{id}")}),
        ),
    );
    opened["paneId"].as_str().unwrap().to_owned()
}

fn core_connection(core: &Core) -> String {
    core.clients.keys().next().unwrap().clone()
}

#[tokio::test]
async fn the_same_file_is_the_same_pane_only_while_the_pane_still_shows_the_page() {
    let root = temp_root();
    write(&root, "cards.html", "cards");
    let state = temp_root();
    let mut core = Core::open(&state).unwrap();
    let (_connection, _receiver) = connect_local(&mut core, "project-page-same", json!([])).await;
    let connection = core_connection(&core);
    let workspace = create_workspace(&mut core, &connection, &root);
    let first = open_path(&mut core, &workspace, 3, "cards.html", false).await;
    assert_eq!(
        open_path(&mut core, &workspace, 4, "cards.html", false).await,
        first
    );
    // The pane went to another address (a host that does not hold the rule reported it).
    core.panes.get_mut(&first).unwrap().stored.pane.url = Some("http://127.0.0.1:1/other".into());
    let second = open_path(&mut core, &workspace, 5, "cards.html", false).await;
    assert_ne!(second, first, "a pane that left the page is not the page");
    assert_eq!(
        open_path(&mut core, &workspace, 6, "cards.html", false).await,
        second
    );
    crate::test_paths::remove_after(core, state);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_project_page_is_served_again_after_a_restart_at_a_new_address() {
    let root = temp_root();
    write(&root, "site/cards.html", "cards");
    write(&root, "site/data.json", "{\"n\":1}");
    write(&root, "gone.html", "gone");
    let state = temp_root();
    let (pane, gone, old_url) = {
        let mut core = Core::open(&state).unwrap();
        let (_connection, _receiver) =
            connect_local(&mut core, "project-page-restart", json!([])).await;
        let connection = core_connection(&core);
        let workspace = create_workspace(&mut core, &connection, &root);
        let pane = open_path(&mut core, &workspace, 3, "site/cards.html", false).await;
        let gone = open_path(&mut core, &workspace, 4, "gone.html", false).await;
        let stored = &core.panes[&pane].stored.pane;
        assert_eq!(stored.project_path.as_deref(), Some("site/cards.html"));
        (pane, gone, stored.url.clone().unwrap())
    };
    assert!(
        ends(port_of(&old_url)).await,
        "the old run left its page served"
    );
    fs::remove_file(root.join("gone.html")).unwrap();

    let core = Core::open(&state).unwrap();
    let url = core.panes[&pane].stored.pane.url.clone().unwrap();
    assert_ne!(url, old_url, "a new port and a new token");
    assert!(url.ends_with("/cards.html"));
    assert_eq!(ask(&url, get).await.1, b"cards");
    assert_eq!(
        ask(&url.replace("cards.html", "data.json"), get).await.1,
        b"{\"n\":1}"
    );
    // The new address is the pane's record, and a file that is gone leaves its pane as it was.
    assert_eq!(
        core.panes[&pane].stored.pane.project_path.as_deref(),
        Some("site/cards.html")
    );
    assert!(core.web.pages.pages.contains_key(&pane) && !core.web.pages.pages.contains_key(&gone));
    drop(core);
    // And the next restart reads the address it saved: the pane is served once more.
    let core = Core::open(&state).unwrap();
    assert_ne!(core.panes[&pane].stored.pane.url.clone().unwrap(), url);
    crate::test_paths::remove_after(core, state);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_pane_that_holds_a_project_page_does_not_navigate_to_another_origin() {
    let root = temp_root();
    let Hosted {
        mut core,
        state,
        caller,
        workspace,
        pane,
        host_receiver: _host_receiver,
        _caller_receiver,
        ..
    } = hosted_page(&root).await;
    let url = core.panes[&pane].stored.pane.url.clone().unwrap();
    for (id, target) in [
        (4, "http://127.0.0.1:1/".to_owned()),
        (5, "https://example.com/".to_owned()),
        (6, "localhost:1".to_owned()),
    ] {
        let Err(error) = core.dispatch(
            &caller,
            request(
                id,
                "web.navigate",
                json!({"paneId":pane,"url":target,"idempotencyKey":format!("go-{id}")}),
            ),
        ) else {
            panic!("{target} should be refused")
        };
        assert_eq!(
            errors::kind(&error),
            ErrorKind::NavigationRefused,
            "{target}"
        );
    }
    // The page's own address, and a move through the pane's history, are the pane's to make.
    let same = url.replace("cards.html", "other.html");
    for (id, params) in [
        (7, json!({"paneId":pane,"url":same,"idempotencyKey":"same"})),
        (
            8,
            json!({"paneId":pane,"action":"reload","idempotencyKey":"reload"}),
        ),
    ] {
        assert!(matches!(
            core.dispatch(&caller, request(id, "web.navigate", params)),
            Ok(Outcome::Queued)
        ));
    }
    // A pane of an ordinary page goes anywhere.
    let ordinary = reply(
        &mut core,
        &caller,
        request(
            9,
            "web.open",
            json!({"workspaceId":workspace,"url":"example.com","idempotencyKey":"plain"}),
        ),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(matches!(
        core.dispatch(
            &caller,
            request(10, "web.navigate", json!({"paneId":ordinary,"url":"https://other.example/","idempotencyKey":"elsewhere"}))
        ),
        Ok(Outcome::Queued)
    ));
    crate::test_paths::remove_after(core, state);
    fs::remove_dir_all(root).unwrap();
}
