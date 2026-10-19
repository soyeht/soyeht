//! Terminal sign-in end to end. The fake login behaves like a browser login: it
//! owns a localhost listener until the redirect arrives, so these tests prove
//! that the login process outlives everything that happens while the person is
//! still in the browser, and that every ending is visible and recoverable.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    thread,
    time::Duration,
};

mod support;
use support::*;

/// An agent pane whose login waits for a browser redirect.
struct Agent {
    rig: Rig,
    client: Client,
    pane: String,
}

/// The agent either needs a sign-in or is already signed in as `account-a`;
/// the next browser login chooses `account-b`.
fn agent(signed_in: bool, can_sign_out: bool) -> Agent {
    let rig = Rig::new();
    let binary = sibling_binary("soyeht-fake-acp");
    let mut arguments = vec![
        "--state-dir".to_owned(),
        rig.path.join("native").to_string_lossy().into_owned(),
        "--require-login".to_owned(),
    ];
    if can_sign_out {
        arguments.push("--logout".into());
    }
    fs::write(
        rig.path.join("harnesses.json"),
        serde_json::to_vec(&json!([{
            "id":"auth-test","name":"Auth test","command":binary,
            "args":arguments,"version":"test"
        }]))
        .unwrap(),
    )
    .unwrap();
    let native = rig.path.join("native");
    fs::create_dir_all(&native).unwrap();
    fs::write(native.join("login.listen"), b"fixture").unwrap();
    fs::write(native.join("login.account"), "account-b").unwrap();
    if signed_in {
        fs::write(native.join("login.ok"), "account-a").unwrap();
    }
    let mut client = rig.client();
    let (workspace, _) = client.create(&rig);
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"auth-test",
            "idempotencyKey":"login-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    if signed_in {
        wait_until("agent session", || {
            session_id(&mut client, &pane).is_string()
        });
    } else {
        wait_until("agent requests sign-in", || {
            client.pane(&pane)["phrase"] == "Sign-in required"
        });
    }
    Agent { rig, client, pane }
}

/// A signed-out agent whose sign-in terminal runs and listens on the returned port.
fn login_in_progress() -> (Rig, Client, String, String, u16) {
    let Agent {
        rig,
        mut client,
        pane,
    } = agent(false, false);
    let terminal = start_login(&mut client, &pane, "login-start");
    let port = callback_port(&rig);
    (rig, client, pane, terminal, port)
}

fn start_login(client: &mut Client, pane: &str, key: &str) -> String {
    client.call(
        "pane.auth.start",
        json!({"paneId":pane,"authMethodId":"login","idempotencyKey":key}),
    )["terminalPaneId"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn call_pane(client: &mut Client, method: &str, pane: &str, key: &str) -> Value {
    client.call(method, json!({"paneId":pane,"idempotencyKey":key}))
}

fn refusal(client: &mut Client, method: &str, pane: &str, key: &str) -> Value {
    let reply = client.request(method, json!({"paneId":pane,"idempotencyKey":key}));
    reply["error"]["data"]["kind"].clone()
}

fn callback_port(rig: &Rig) -> u16 {
    let path = rig.path.join("native/login.port");
    wait_until("login listener", || path.is_file());
    fs::read_to_string(path).unwrap().trim().parse().unwrap()
}

fn forget_listener(rig: &Rig) {
    fs::remove_file(rig.path.join("native/login.port")).unwrap();
}

/// What the browser does after sign-in: a request to the localhost listener.
fn redirect_browser(port: u16) {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .expect("the login listener must still be open when the browser redirects");
    stream
        .write_all(b"GET /callback?code=x&state=y HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
}

fn listener_is_open(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// Stops a pane's process the way a crash would. The supervisor allows one
/// engine connection, so the process is addressed by its pid instead.
fn kill_pane_process(client: &mut Client, pane: &str) {
    let pid = client.pane(pane)["pid"].as_i64().unwrap();
    end_process(i32::try_from(pid).unwrap());
}

#[cfg(unix)]
fn end_process(pid: i32) {
    // SAFETY: the pid is the live process the supervisor reported for this pane.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
}

#[cfg(windows)]
fn end_process(pid: i32) {
    let ended = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output()
        .unwrap();
    assert!(ended.status.success(), "{ended:?}");
}

fn session_id(client: &mut Client, pane: &str) -> Value {
    client.pane(pane)["agent"]["sessionId"].clone()
}

fn login_phase(client: &mut Client, pane: &str) -> Value {
    client.pane(pane)["agent"]["login"]["phase"].clone()
}

fn account(rig: &Rig) -> String {
    fs::read_to_string(rig.path.join("native/login.ok")).unwrap()
}

fn terminal_exists(client: &mut Client, terminal: &str) -> bool {
    client
        .section("panes")
        .iter()
        .any(|pane| pane["paneId"] == terminal)
}

/// The next `pane.auth_finished` for `pane`; earlier ones are forgotten.
fn auth_finished(observer: &mut Client, pane: &str) -> Value {
    let finished =
        observer.wait_for_notification("pane.auth_finished", |params| params["paneId"] == pane);
    observer.messages.clear();
    finished
}

/// A sign-in that ended without signing in, with `reason` starting or containing `fragment`.
fn assert_unfinished(finished: &Value, fragment: &str) {
    assert_eq!(finished["ok"], false, "{finished}");
    assert!(
        finished["reason"].as_str().unwrap().contains(fragment),
        "{finished}"
    );
}

fn subscribe(rig: &Rig) -> Client {
    let mut observer = rig.client();
    let (snapshot, _) = observer.snapshot();
    observer.call("events.subscribe", json!({"since":snapshot["cursor"]}));
    observer.messages.clear();
    observer
}

#[test]
fn login_listener_survives_a_repeated_start_and_an_engine_restart() {
    let (mut rig, mut client, pane, terminal, port) = login_in_progress();
    // A second click, with a new idempotency key, must reuse the first login.
    assert_eq!(
        start_login(&mut client, &pane, "login-start-again"),
        terminal
    );
    thread::sleep(Duration::from_millis(500));
    assert_ne!(client.pane(&terminal)["state"], "dead");
    assert_eq!(login_phase(&mut client, &pane), "waiting");
    assert_eq!(
        client.pane(&pane)["agent"]["login"]["terminalPaneId"],
        terminal
    );

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_ne!(client.pane(&terminal)["state"], "dead");
    assert_eq!(login_phase(&mut client, &pane), "waiting");
    assert_eq!(
        start_login(&mut client, &pane, "login-start-after-restart"),
        terminal
    );

    redirect_browser(port);
    wait_until("agent session after sign-in", || {
        session_id(&mut client, &pane).is_string()
    });
    assert!(client.pane(&pane)["agent"]["login"].is_null());
}

#[test]
fn login_completes_after_the_adapter_process_died_meanwhile() {
    let (_rig, mut client, pane, terminal, port) = login_in_progress();
    kill_pane_process(&mut client, &pane);
    client.wait_state(&pane, "dead");
    assert_ne!(client.pane(&terminal)["state"], "dead");
    redirect_browser(port);
    wait_until("agent session after sign-in", || {
        session_id(&mut client, &pane).is_string()
    });
}

#[test]
fn a_sign_in_that_crashed_keeps_its_failed_exit_status_for_clients_to_show() {
    let (_rig, mut client, _pane, terminal, _port) = login_in_progress();
    assert!(client.pane(&terminal)["exitCode"].is_null());
    kill_pane_process(&mut client, &terminal);
    let dead = client.wait_state(&terminal, "dead");
    assert!(
        dead["exitCode"].as_i64().is_some_and(|code| code != 0),
        "{dead}"
    );
}

#[test]
fn a_sign_in_that_exits_cleanly_keeps_a_zero_exit_status_for_clients_to_show() {
    let Agent {
        rig,
        mut client,
        pane,
    } = agent(false, false);
    let native = rig.path.join("native");
    fs::remove_file(native.join("login.listen")).unwrap();
    fs::write(native.join("login.cancel"), b"fixture").unwrap();
    let terminal = start_login(&mut client, &pane, "login-start");
    wait_until("the sign-in started", || {
        native.join("login.started").exists()
    });
    fs::write(native.join("login.release"), b"release").unwrap();
    let dead = client.wait_state(&terminal, "dead");
    assert_eq!(dead["exitCode"].as_i64(), Some(0), "{dead}");
}

#[test]
fn a_running_sign_in_can_be_cancelled_and_started_again() {
    let Agent {
        rig,
        mut client,
        pane,
    } = agent(false, false);
    let mut observer = subscribe(&rig);
    let terminal = start_login(&mut client, &pane, "login-start");
    let port = callback_port(&rig);
    assert!(listener_is_open(port));

    call_pane(&mut client, "pane.auth.cancel", &pane, "login-cancel");
    let finished = auth_finished(&mut observer, &pane);
    assert_unfinished(&finished, "Sign-in was cancelled.");
    assert!(!terminal_exists(&mut client, &terminal));
    wait_until("login process stopped", || !listener_is_open(port));
    assert!(client.pane(&pane)["agent"]["login"].is_null());
    assert_eq!(client.pane(&pane)["agent"]["authMethods"][0]["id"], "login");
    // Nothing is left to cancel.
    assert_eq!(
        refusal(&mut client, "pane.auth.cancel", &pane, "login-cancel-again"),
        "invalid_params"
    );

    forget_listener(&rig);
    let second = start_login(&mut client, &pane, "login-start-again");
    assert_ne!(second, terminal);
    redirect_browser(callback_port(&rig));
    wait_until("agent session after sign-in", || {
        session_id(&mut client, &pane).is_string()
    });
}

#[test]
fn switching_account_restarts_the_session_and_keeps_the_history() {
    let Agent {
        rig,
        mut client,
        pane,
    } = agent(true, false);
    let mut observer = subscribe(&rig);
    wait_until("a signed-in pane offers the account switch", || {
        client.pane(&pane)["agent"]["reauthMethods"][0]["id"] == "login"
    });
    assert!(
        client.pane(&pane)["agent"]["authMethods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"before the switch","idempotencyKey":"before-switch"}),
    );
    wait_for_user_message(&mut client, &pane, "before the switch");
    client.wait_state(&pane, "idle");
    let session = session_id(&mut client, &pane);
    let generation = client.pane(&pane)["generation"].as_u64().unwrap();

    let terminal = start_login(&mut client, &pane, "switch-start");
    assert_eq!(login_phase(&mut client, &pane), "waiting");
    assert!(client.pane(&pane)["agent"]["reauthMethods"].is_null());
    // The working session is untouched while the person is in the browser.
    assert_eq!(session_id(&mut client, &pane), session);
    assert_eq!(account(&rig), "account-a");

    redirect_browser(callback_port(&rig));
    let finished = auth_finished(&mut observer, &pane);
    assert_eq!(finished["ok"], true, "{finished}");
    assert_eq!(account(&rig), "account-b");
    assert!(!terminal_exists(&mut client, &terminal));
    wait_until("the new account is picked up", || {
        client.pane(&pane)["generation"].as_u64().unwrap() > generation
    });
    wait_until("the restarted session", || {
        client.pane(&pane)["agent"]["login"].is_null()
            && client.pane(&pane)["agent"]["reauthMethods"][0]["id"] == "login"
    });
    assert_eq!(session_id(&mut client, &pane), session);
    let items = transcript_items(&mut client, &pane);
    assert!(
        items
            .iter()
            .any(|item| item["text"].as_str() == Some("before the switch")),
        "{items:?}"
    );
}

#[test]
fn a_cancelled_or_interrupted_account_switch_keeps_the_session_usable() {
    let Agent {
        rig,
        mut client,
        pane,
    } = agent(true, false);
    let mut observer = subscribe(&rig);
    wait_until("account switch offered", || {
        client.pane(&pane)["agent"]["reauthMethods"][0]["id"] == "login"
    });
    let session = session_id(&mut client, &pane);
    let generation = client.pane(&pane)["generation"].as_u64().unwrap();

    start_login(&mut client, &pane, "switch-start");
    call_pane(&mut client, "pane.auth.cancel", &pane, "switch-cancel");
    assert_eq!(auth_finished(&mut observer, &pane)["ok"], false);
    wait_until("switch offered again", || {
        client.pane(&pane)["agent"]["reauthMethods"][0]["id"] == "login"
    });

    forget_listener(&rig);
    let terminal = start_login(&mut client, &pane, "switch-start-again");
    callback_port(&rig);
    kill_pane_process(&mut client, &terminal);
    let finished = auth_finished(&mut observer, &pane);
    assert_unfinished(&finished, "Sign-in failed or was interrupted");

    // The pane never lost its account or its session.
    assert_eq!(account(&rig), "account-a");
    assert_eq!(session_id(&mut client, &pane), session);
    assert!(
        client.pane(&pane)["agent"]["authMethods"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        client.pane(&pane)["generation"].as_u64().unwrap(),
        generation
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"still works","idempotencyKey":"after-failed-switch"}),
    );
    wait_for_user_message(&mut client, &pane, "still works");
}

#[test]
fn the_account_cannot_change_during_a_turn() {
    let Agent {
        rig: _rig,
        mut client,
        pane,
    } = agent(true, true);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"silent-no-output","idempotencyKey":"long-turn"}),
    );
    wait_until("the turn is active", || {
        client.pane(&pane)["agent"]["activePromptId"].is_string()
    });
    assert_eq!(
        refusal(&mut client, "pane.auth.start", &pane, "switch-during-turn"),
        "pane_busy"
    );
    assert_eq!(
        refusal(&mut client, "pane.auth.logout", &pane, "logout-during-turn"),
        "pane_busy"
    );
    call_pane(&mut client, "pane.cancel", &pane, "end-turn");
    client.wait_state(&pane, "idle");
}

#[test]
fn signing_out_asks_for_a_sign_in_and_signing_in_resumes_the_session() {
    let Agent {
        rig,
        mut client,
        pane,
    } = agent(true, true);
    wait_until("sign out offered", || {
        client.pane(&pane)["agent"]["canSignOut"] == true
    });
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"before signing out","idempotencyKey":"before-logout"}),
    );
    wait_for_user_message(&mut client, &pane, "before signing out");
    client.wait_state(&pane, "idle");
    let session = session_id(&mut client, &pane);

    call_pane(&mut client, "pane.auth.logout", &pane, "logout");
    wait_until("the agent asks for a sign-in", || {
        client.pane(&pane)["phrase"] == "Sign-in required"
    });
    assert!(!rig.path.join("native/login.ok").exists());
    let signed_out = client.pane(&pane);
    assert_eq!(signed_out["agent"]["authMethods"][0]["id"], "login");
    assert!(signed_out["agent"]["canSignOut"].is_null());
    assert!(signed_out["agent"]["reauthMethods"].is_null());
    // The conversation stays: the session id is what the next sign-in resumes.
    assert_eq!(signed_out["agent"]["sessionId"], session);

    start_login(&mut client, &pane, "login-after-logout");
    redirect_browser(callback_port(&rig));
    wait_until("the session resumes", || {
        let view = client.pane(&pane);
        view["agent"]["login"].is_null()
            && view["agent"]["authMethods"].as_array().unwrap().is_empty()
            && view["phrase"] != "Sign-in required"
    });
    assert_eq!(session_id(&mut client, &pane), session);
    assert_eq!(account(&rig), "account-b");
    let items = transcript_items(&mut client, &pane);
    assert!(
        items
            .iter()
            .any(|item| item["text"].as_str() == Some("before signing out")),
        "{items:?}"
    );
}

#[test]
fn signing_out_is_refused_when_the_agent_cannot_do_it() {
    let Agent {
        rig: _rig,
        mut client,
        pane,
    } = agent(true, false);
    assert!(client.pane(&pane)["agent"]["canSignOut"].is_null());
    assert_eq!(
        refusal(&mut client, "pane.auth.logout", &pane, "logout"),
        "invalid_params"
    );
}

#[test]
fn a_sign_in_lost_to_a_supervisor_restart_is_reported_and_can_be_started_again() {
    let Agent {
        mut rig,
        mut client,
        pane,
    } = agent(false, false);
    let (snapshot, _) = client.snapshot();
    let cursor = snapshot["cursor"].clone();
    let terminal = start_login(&mut client, &pane, "login-start");
    let port = callback_port(&rig);

    drop(client);
    rig.kill_engine();
    rig.restart_ptyd();
    wait_until("the login process stopped with its supervisor", || {
        !listener_is_open(port)
    });
    rig.start();

    let mut observer = rig.client();
    observer.call("events.subscribe", json!({"since":cursor}));
    let finished = auth_finished(&mut observer, &pane);
    assert_unfinished(&finished, "background services restarted");
    let mut client = rig.client();
    assert_eq!(client.pane(&terminal)["state"], "dead");
    wait_until("the agent asks for a sign-in again", || {
        client.pane(&pane)["phrase"] == "Sign-in required"
    });
    assert!(client.pane(&pane)["agent"]["login"].is_null());

    forget_listener(&rig);
    start_login(&mut client, &pane, "login-start-again");
    redirect_browser(callback_port(&rig));
    wait_until("agent session after sign-in", || {
        session_id(&mut client, &pane).is_string()
    });
}
