//! A terminal pane as the sender of a message, and names across workspaces (ADR 0165, slice 1).
//!
//! The engine puts a binding in the environment of every terminal pane's process. A program the
//! pane runs presents it in `hello` and speaks for the pane, for messaging only, for as long as
//! the pane runs that process.

use super::{agent_message_names::rename, *};

fn kind(response: &Value) -> &str {
    response["error"]["data"]["kind"].as_str().unwrap()
}

fn hold(human: &mut Client, pane: &str) {
    human.call(
        "pane.draft",
        json!({"paneId":pane,"active":true,"idempotencyKey":format!("hold-{pane}")}),
    );
}

fn queue(human: &mut Client, pane: &str) -> Vec<Value> {
    human.call("pane.queue.list", json!({"paneId":pane,"limit":64}))["items"]
        .as_array()
        .unwrap()
        .clone()
}

/// A second workspace on its own folder.
fn other_workspace(rig: &Rig, human: &mut Client, name: &str) -> String {
    let folder = rig.path.join(name);
    fs::create_dir_all(&folder).unwrap();
    create_workspace(human, name, &folder, &format!("workspace-{name}"))
}

/// What the shell of the terminal pane `pane` has in `SOYEHT_PANE_BINDING`, read through the
/// pane the way a program in it would. A shell that is still starting drops what is typed before
/// it reads the terminal, so the command is typed again until it has run.
#[cfg(unix)]
fn pane_binding(rig: &Rig, human: &mut Client, pane: &str, file: &str) -> String {
    let path = rig.path.join(file);
    let mut attempt = 0;
    eventually("the pane's shell printed its binding", || {
        attempt += 1;
        human.call(
            "pane.input",
            json!({"paneId":pane,
                "bytes":STANDARD.encode(format!(
                    "printenv SOYEHT_PANE_BINDING > '{0}.part' && mv '{0}.part' '{0}'\n",
                    path.display()
                )),
                "idempotencyKey":format!("binding-{pane}-{file}-{attempt}")}),
        );
        thread::sleep(Duration::from_millis(300));
        fs::read_to_string(&path)
            .ok()
            .filter(|text| text.ends_with('\n'))
            .map(|text| text.trim().to_owned())
            .ok_or_else(|| "no binding yet".to_owned())
    })
}

fn bound_client(rig: &Rig, binding: &str) -> Client {
    let (socket, hello) = binding_hello(&rig.path.join("engine.sock"), binding);
    assert_eq!(hello["result"]["role"], "agent", "{hello}");
    Client {
        socket,
        next: 1,
        messages: vec![],
    }
}

fn create_terminal(human: &mut Client, workspace: &str, key: &str) -> String {
    let pane = human.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":key}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    eventually("the terminal pane has a process", || {
        if human.pane(&pane)["pid"].is_null() {
            Err("no process yet".into())
        } else {
            Ok(())
        }
    });
    pane
}

/// Agent `first` in the workspace "Test", agent `second` in the workspace "Other", and a terminal
/// pane in "Other", with the agents held so a message stays in the queue where a test reads it.
#[cfg(unix)]
struct Scene {
    human: Client,
    first: String,
    second: String,
    terminal: String,
    other: String,
    binding: String,
}

#[cfg(unix)]
fn scene(rig: &Rig) -> Scene {
    let (mut human, first) = rig.agent();
    let other = other_workspace(rig, &mut human, "Other");
    let (_, second) = rig.agent_in_workspace(&other);
    hold(&mut human, &first);
    hold(&mut human, &second);
    rename(&mut human, &first, "Plans");
    rename(&mut human, &second, "Reviews");
    let terminal = create_terminal(&mut human, &other, "terminal-pane");
    let binding = pane_binding(rig, &mut human, &terminal, "binding");
    Scene {
        human,
        first,
        second,
        terminal,
        other,
        binding,
    }
}

#[cfg(unix)]
#[test]
fn a_terminal_pane_lists_and_messages_agents_of_every_workspace_by_name() {
    let rig = Rig::new();
    let mut scene = scene(&rig);
    let mut sender = bound_client(&rig, &scene.binding);

    // The agents of every workspace, the sender's own workspace first, each saying where it is.
    let listed = sender.call("agent.list", json!({}));
    let items = listed["items"].as_array().unwrap();
    let names: Vec<_> = items
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Reviews", "Plans"], "{listed}");
    assert_eq!(items[0]["workspaceName"], "Other");
    assert_eq!(items[1]["workspaceName"], "Test");
    assert!(items.iter().all(|item| item["kind"] == "agent"));

    // A message names an agent in another workspace; the recipient's queue says who sent it.
    let sent = sender.request(
        "agent.message",
        json!({"targetName":"plans","text":"hello from a shell","idempotencyKey":"to-plans"}),
    );
    assert_eq!(sent["result"]["queued"], true, "{sent}");
    let id = sent["result"]["messageId"].as_str().unwrap();
    assert!(id.starts_with("m:"), "{id}");
    let items = queue(&mut scene.human, &scene.first);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["text"], "hello from a shell");
    let source = &items[0]["agentMessage"];
    assert_eq!(source["paneId"], scene.terminal);
    assert_eq!(source["kind"], "terminal");
    assert_eq!(source["workspaceName"], "Other");
    // Repeating the request with its key gives the stored answer and queues nothing more.
    let again = sender.request(
        "agent.message",
        json!({"targetName":"plans","text":"hello from a shell","idempotencyKey":"to-plans"}),
    );
    assert_eq!(again["result"], sent["result"]);
    assert_eq!(queue(&mut scene.human, &scene.first).len(), 1);

    // The agent answers the shell's name... which a terminal pane cannot receive yet.
    let shell_name = scene.human.pane(&scene.terminal)["name"]
        .as_str()
        .unwrap()
        .to_owned();
    let reply = agent_client(&rig, &scene.first).request(
        "agent.message",
        json!({"targetName":shell_name,"text":"reply","idempotencyKey":"reply"}),
    );
    assert_eq!(kind(&reply), "not_found", "{reply}");
}

#[cfg(unix)]
#[test]
fn a_name_two_panes_of_different_workspaces_carry_is_ambiguous() {
    let rig = Rig::new();
    let mut scene = scene(&rig);
    rename(&mut scene.human, &scene.second, "PLANS");
    let mut sender = bound_client(&rig, &scene.binding);

    let ambiguous = sender.request(
        "agent.message",
        json!({"targetName":" Plans ","text":"hello","idempotencyKey":"ambiguous"}),
    );
    assert_eq!(kind(&ambiguous), "invalid_params", "{ambiguous}");
    let message = ambiguous["error"]["message"].as_str().unwrap();
    for expected in [
        &scene.first[..],
        &scene.second[..],
        "workspace Test",
        "workspace Other",
    ] {
        assert!(message.contains(expected), "{message}");
    }
    assert!(queue(&mut scene.human, &scene.first).is_empty());
    assert!(queue(&mut scene.human, &scene.second).is_empty());

    // The id of either pane settles it.
    let by_id = sender.request(
        "agent.message",
        json!({"targetPaneId":scene.second,"text":"hello","idempotencyKey":"by-id"}),
    );
    assert_eq!(by_id["result"]["queued"], true, "{by_id}");
    assert_eq!(queue(&mut scene.human, &scene.second).len(), 1);
}

#[cfg(unix)]
#[test]
fn a_web_page_and_a_terminal_pane_say_why_they_take_no_message() {
    let rig = Rig::new();
    let mut scene = scene(&rig);
    let page = scene.human.call(
        "pane.create",
        json!({"workspaceId":scene.other,"mode":"web","url":"https://example.com/",
            "name":"Dashboard","idempotencyKey":"page"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut sender = bound_client(&rig, &scene.binding);

    for target in [
        json!({"targetName":"dashboard"}),
        json!({"targetPaneId":page}),
    ] {
        let mut params = json!({"text":"hello","idempotencyKey":format!("page-{target}")});
        params
            .as_object_mut()
            .unwrap()
            .extend(target.as_object().unwrap().clone());
        let refused = sender.request("agent.message", params);
        assert_eq!(kind(&refused), "not_found", "{refused}");
        assert!(
            refused["error"]["message"]
                .as_str()
                .unwrap()
                .contains("no recorded creator"),
            "{refused}"
        );
    }
    // The page is not a pane an agent can be listed as, and a shell is not an agent either.
    let listed = sender.call("agent.list", json!({}));
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);
    let shell = sender.request(
        "agent.message",
        json!({"targetPaneId":scene.terminal,"text":"hello","idempotencyKey":"shell"}),
    );
    assert_eq!(kind(&shell), "not_found", "{shell}");
}

#[cfg(unix)]
#[test]
fn a_terminal_binding_speaks_for_messaging_only_and_for_its_own_process() {
    let mut rig = Rig::new();
    let mut scene = scene(&rig);
    let mut sender = bound_client(&rig, &scene.binding);

    // Nothing but messaging: the engine refuses the rest, the person's powers and the agents'.
    for (method, params) in [
        ("workspace.list", json!({})),
        ("task.list", json!({"workspaceId":scene.other,"limit":10})),
        (
            "web.open",
            json!({"workspaceId":scene.other,"url":"https://example.com/","idempotencyKey":"no-web"}),
        ),
        (
            "pane.input",
            json!({"paneId":scene.first,"bytes":STANDARD.encode("x"),"idempotencyKey":"no-input"}),
        ),
        (
            "pane.prompt",
            json!({"paneId":scene.first,"text":"x","idempotencyKey":"no-prompt"}),
        ),
    ] {
        let refused = sender.request(method, params);
        assert_eq!(kind(&refused), "forbidden_role", "{method}: {refused}");
    }
    assert!(queue(&mut scene.human, &scene.first).is_empty());
    // A made-up binding, another pane's, and one with the right pane and a wrong tag are refused.
    let (pane, tag) = scene.binding.split_once('.').unwrap();
    for candidate in [
        "unknown".to_owned(),
        format!("{pane}.{}", tag.chars().rev().collect::<String>()),
        format!("{}.{tag}", scene.first),
    ] {
        let (_, hello) = binding_hello(&rig.path.join("engine.sock"), &candidate);
        assert_eq!(kind(&hello), "unauthorized", "{candidate}: {hello}");
    }

    // The engine restarts: the pane's process lives on in the supervisor and its binding, which
    // the engine derives and does not store, still opens a connection.
    drop(scene.human);
    rig.kill_engine();
    rig.start();
    let mut after_restart = bound_client(&rig, &scene.binding);
    assert_eq!(
        after_restart.call("agent.list", json!({}))["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    drop(after_restart);

    // The supervisor is replaced, so the pane gets a new process, and the binding of the old one
    // ends: the connection that holds it is refused at its next request, and a new `hello` with
    // it is too.
    let mut human = rig.client();
    let before = human.pane(&scene.terminal)["pid"].clone();
    let mut old = bound_client(&rig, &scene.binding);
    rig.ptyd.kill().unwrap();
    rig.ptyd.wait().unwrap();
    rig.restart_ptyd();
    eventually("the pane runs a new process", || {
        let pid = human.pane(&scene.terminal)["pid"].clone();
        if pid.is_null() || pid == before {
            Err(pid.to_string())
        } else {
            Ok(())
        }
    });
    let stale = old.request("agent.list", json!({}));
    assert_eq!(kind(&stale), "unauthorized", "{stale}");
    let (_, hello) = binding_hello(&rig.path.join("engine.sock"), &scene.binding);
    assert_eq!(kind(&hello), "unauthorized", "{hello}");
    // The new process has a binding of its own.
    let renewed = pane_binding(&rig, &mut human, &scene.terminal, "binding-after");
    assert_ne!(renewed, scene.binding);
    assert_eq!(
        bound_client(&rig, &renewed).call("agent.list", json!({}))["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // A closed pane has none.
    human.call(
        "pane.close",
        json!({"paneId":scene.terminal,"idempotencyKey":"close-terminal"}),
    );
    let (_, hello) = binding_hello(&rig.path.join("engine.sock"), &renewed);
    assert_eq!(kind(&hello), "unauthorized", "{hello}");
}

/// The lines a program writes to its standard output, one JSON value each.
#[cfg(unix)]
fn exchange(mut child: std::process::Child, requests: &[Value]) -> Vec<Value> {
    use std::io::{BufRead, BufReader};
    let mut input = child.stdin.take().unwrap();
    for request in requests {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let replies = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    assert!(child.wait().unwrap().success());
    replies
}

#[cfg(unix)]
fn mcp(rig: &Rig, binding: Option<&str>) -> std::process::Child {
    let mut command = Command::new(sibling_binary("soyeht-mcp"));
    command
        .arg("--data-dir")
        .arg(&rig.path)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(binding) = binding {
        command.env("SOYEHT_PANE_BINDING", binding);
    }
    command.spawn().unwrap()
}

#[cfg(unix)]
#[test]
fn soyeht_mcp_in_a_terminal_pane_offers_two_tools_and_outside_one_it_offers_none() {
    let rig = Rig::new();
    let mut scene = scene(&rig);
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
    let list = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});

    // Started where the pane's environment is: the tools of a terminal pane, one connection.
    let call = |name: &str, arguments: Value| json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":name,"arguments":arguments}});
    let replies = exchange(
        mcp(&rig, Some(&scene.binding)),
        &[
            initialize.clone(),
            list.clone(),
            call("list_agents", json!({})),
            call(
                "message_agent",
                json!({"targetName":"Plans","text":"from the harness"}),
            ),
        ],
    );
    let names: Vec<_> = replies[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["list_agents", "message_agent"]);
    assert_eq!(
        replies[2]["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(replies[3]["result"]["isError"], false, "{}", replies[3]);
    let items = queue(&mut scene.human, &scene.first);
    assert_eq!(items[0]["text"], "from the harness");
    assert_eq!(items[0]["agentMessage"]["kind"], "terminal");

    // Started outside a Soyeht pane: it answers, offers nothing and does not fail.
    let replies = exchange(mcp(&rig, None), &[initialize, list]);
    assert_eq!(replies[1]["result"]["tools"], json!([]));
}

#[cfg(unix)]
#[test]
fn soyeht_message_in_a_terminal_pane_sends_by_name_and_refuses_outside_one() {
    let rig = Rig::new();
    let mut scene = scene(&rig);
    let run = |binding: Option<&str>, args: &[&str]| {
        let mut command = Command::new(sibling_binary("soyeht"));
        command
            .args(["--profile", "dev", "--data-dir"])
            .arg(&rig.path)
            .args(args)
            .env_clear();
        if let Some(binding) = binding {
            command.env("SOYEHT_PANE_BINDING", binding);
        }
        command.output().unwrap()
    };

    let sent = run(
        Some(&scene.binding),
        &["message", "Plans", "from the command line"],
    );
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    let answer: Value = serde_json::from_slice(&sent.stdout).unwrap();
    assert_eq!(answer["queued"], true, "{answer}");
    assert!(answer["messageId"].as_str().unwrap().starts_with("m:"));
    let by_id = run(
        Some(&scene.binding),
        &["message", &scene.second, "by pane id"],
    );
    assert!(
        by_id.status.success(),
        "{}",
        String::from_utf8_lossy(&by_id.stderr)
    );
    assert_eq!(
        queue(&mut scene.human, &scene.first)[0]["text"],
        "from the command line"
    );
    assert_eq!(
        queue(&mut scene.human, &scene.second)[0]["text"],
        "by pane id"
    );

    let outside = run(None, &["message", "Plans", "nobody"]);
    assert!(!outside.status.success());
    assert!(
        String::from_utf8_lossy(&outside.stderr).contains("not attached to a Soyeht pane"),
        "{}",
        String::from_utf8_lossy(&outside.stderr)
    );
}
