//! A terminal pane as the sender of a message, and names across workspaces (ADR 0165, slice 1).
//!
//! The engine puts a binding in the environment of every terminal pane's process. A program the
//! pane runs presents it in `hello` and speaks for the pane, for messaging only, for as long as
//! the pane runs that process. The shell commands are Unix ones, so the module is Unix only.

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
/// pane the way a program in it would. The command is typed once, when the shell has drawn its
/// prompt (what a starting shell is sent before that, it drops), and the file it writes is polled.
fn pane_binding(rig: &Rig, human: &mut Client, pane: &str, file: &str) -> String {
    let path = rig.path.join(file);
    eventually("the shell drew its prompt", || {
        let (_, frames) = human.tail(pane);
        if frames
            .iter()
            .any(|frame| matches!(frame, Frame::Terminal { bytes, .. } if !bytes.is_empty()))
        {
            Ok(())
        } else {
            Err("no output yet".to_owned())
        }
    });
    human.call(
        "pane.input",
        json!({"paneId":pane,
            "bytes":STANDARD.encode(format!(
                "printenv SOYEHT_PANE_BINDING > '{0}.part' && mv '{0}.part' '{0}'\n",
                path.display()
            )),
            "idempotencyKey":format!("binding-{pane}-{file}")}),
    );
    eventually("the pane's shell printed its binding", || {
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
struct Scene {
    human: Client,
    first: String,
    second: String,
    terminal: String,
    other: String,
    binding: String,
}

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
    // And it says why, not that no pane has the name.
    let message = reply["error"]["message"].as_str().unwrap();
    assert!(message.contains("cannot receive messages yet"), "{message}");
}

#[test]
fn a_name_the_own_workspace_carries_once_is_that_pane_and_otherwise_every_workspace_counts() {
    let rig = Rig::new();
    let mut scene = scene(&rig);
    // The terminal is in "Other", beside the agent `second`. Both agents are called Plans.
    rename(&mut scene.human, &scene.second, "PLANS");
    let mut sender = bound_client(&rig, &scene.binding);

    // The own workspace carries the name once: that pane, whatever another workspace holds.
    let own = sender.request(
        "agent.message",
        json!({"targetName":" Plans ","text":"hello","idempotencyKey":"own"}),
    );
    assert_eq!(own["result"]["queued"], true, "{own}");
    assert_eq!(queue(&mut scene.human, &scene.second).len(), 1);
    assert!(queue(&mut scene.human, &scene.first).is_empty());

    // A sender in a third workspace has no such pane: the name is carried twice, which is an
    // error that lists both with their workspace and id, and sends nothing.
    let third = other_workspace(&rig, &mut scene.human, "Third");
    let lone = create_terminal(&mut scene.human, &third, "third-terminal");
    let binding = pane_binding(&rig, &mut scene.human, &lone, "binding-third");
    let mut outsider = bound_client(&rig, &binding);
    let ambiguous = outsider.request(
        "agent.message",
        json!({"targetName":"plans","text":"hello","idempotencyKey":"ambiguous"}),
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
    assert_eq!(queue(&mut scene.human, &scene.second).len(), 1);
    assert!(queue(&mut scene.human, &scene.first).is_empty());

    // The id of either pane settles it.
    let by_id = outsider.request(
        "agent.message",
        json!({"targetPaneId":scene.first,"text":"hello","idempotencyKey":"by-id"}),
    );
    assert_eq!(by_id["result"]["queued"], true, "{by_id}");
    assert_eq!(queue(&mut scene.human, &scene.first).len(), 1);
}

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
    let refused_with = |sender: &mut Client, target: Value, key: &str, reason: &str| {
        let mut params = json!({"text":"hello","idempotencyKey":key});
        params
            .as_object_mut()
            .unwrap()
            .extend(target.as_object().unwrap().clone());
        let refused = sender.request("agent.message", params);
        assert_eq!(kind(&refused), "not_found", "{refused}");
        let message = refused["error"]["message"].as_str().unwrap();
        assert!(message.contains(reason), "{message}");
    };

    // A page, by name or by id, is not an agent: the answer is its reason.
    refused_with(
        &mut sender,
        json!({"targetName":"dashboard"}),
        "page-name",
        "no recorded creator",
    );
    refused_with(
        &mut sender,
        json!({"targetPaneId":page}),
        "page-id",
        "no recorded creator",
    );
    // So is another shell, by name or by id (the sender's own pane is not a peer).
    let shell = create_terminal(&mut scene.human, &scene.other, "scratch-terminal");
    rename(&mut scene.human, &shell, "Scratch");
    refused_with(
        &mut sender,
        json!({"targetName":"scratch"}),
        "shell-name",
        "cannot receive messages yet",
    );
    refused_with(
        &mut sender,
        json!({"targetPaneId":shell}),
        "shell-id",
        "cannot receive messages yet",
    );
    // Neither is listed.
    let listed = sender.call("agent.list", json!({}));
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);

    // A page named like an agent is not a candidate for its name: the agent is the one named.
    rename(&mut scene.human, &scene.first, "Dashboard");
    let sent = sender.request(
        "agent.message",
        json!({"targetName":"DASHBOARD","text":"hello","idempotencyKey":"agent-not-page"}),
    );
    assert_eq!(sent["result"]["queued"], true, "{sent}");
    assert_eq!(queue(&mut scene.human, &scene.first).len(), 1);
}

#[test]
fn a_terminal_binding_speaks_for_messaging_only_and_for_its_own_process() {
    let mut rig = Rig::new();
    let mut scene = scene(&rig);

    // Nothing but messaging, by default: every method of the contract but the two is refused,
    // the person's and the agents' alike, and so is one added later.
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("../../../../contracts/vectors/messages.json")).unwrap();
    for (method, params_type, _, _) in soyeht_protocol::METHODS {
        if matches!(
            *method,
            soyeht_protocol::method::AGENT_LIST | soyeht_protocol::method::AGENT_MESSAGE
        ) {
            continue;
        }
        // The vector of the method's parameters, so the request is valid and only the role is
        // left to refuse it.
        let params = vectors
            .iter()
            .find(|vector| vector["type"] == *params_type)
            .map_or(json!({}), |vector| vector["value"].clone());
        // A fresh connection each: the engine closes one that sends a second `hello`.
        let refused = bound_client(&rig, &scene.binding).request(method, params);
        assert!(refused.get("result").is_none(), "{method}: {refused}");
        // The handshake is done and pairing is for a device, so those are refused before the role
        // is asked; everything else is the role's to refuse.
        if *method != soyeht_protocol::method::HELLO
            && !soyeht_protocol::method_family::PAIR.contains(method)
        {
            assert_eq!(kind(&refused), "forbidden_role", "{method}: {refused}");
        }
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
    // The programs of the pane say so, in words, and the engine that answered is not "unreachable".
    let sent = soyeht(&rig, Some(&scene.binding), &["message", "Plans", "late"]);
    assert!(!sent.status.success());
    let said = stderr(&sent);
    assert!(said.contains("binding is not current"), "{said}");
    assert!(said.contains("another Soyeht profile"), "{said}");
    assert!(!said.contains("cannot connect"), "{said}");
    let (instructions, tools) = mcp_handshake(&rig, Some(&scene.binding));
    assert!(
        instructions.contains("binding is not current"),
        "{instructions}"
    );
    assert_eq!(tools, json!([]));
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

/// `soyeht ARGS` as a program in a terminal pane runs it, with `binding` as the pane's variable.
fn soyeht(rig: &Rig, binding: Option<&str>, args: &[&str]) -> std::process::Output {
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
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// What `soyeht-mcp` answers `initialize` and `tools/list` with, started with `binding`.
fn mcp_handshake(rig: &Rig, binding: Option<&str>) -> (String, Value) {
    let replies = exchange(
        mcp(rig, binding),
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        ],
    );
    let instructions = replies[0]["result"]["instructions"].as_str().unwrap();
    (
        instructions.to_owned(),
        replies[1]["result"]["tools"].clone(),
    )
}

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
    // A terminal pane cannot receive yet, so the tool does not promise an answer.
    let description = replies[1]["result"]["tools"][1]["description"]
        .as_str()
        .unwrap();
    assert!(
        description.contains("no answer will arrive"),
        "{description}"
    );
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

#[test]
fn soyeht_message_in_a_terminal_pane_sends_by_name_and_refuses_outside_one() {
    let rig = Rig::new();
    let mut scene = scene(&rig);
    let run = |binding: Option<&str>, args: &[&str]| soyeht(&rig, binding, args);

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

    // An empty variable is no variable, for this program and for `soyeht-mcp`.
    let outside = run(None, &["message", "Plans", "nobody"]);
    assert!(!outside.status.success());
    let empty = run(Some(""), &["message", "Plans", "nobody"]);
    assert!(!empty.status.success());
    assert!(
        stderr(&empty).contains("not attached to a Soyeht pane"),
        "{}",
        stderr(&empty)
    );
    let (instructions, tools) = mcp_handshake(&rig, Some(""));
    assert!(instructions.contains("not started inside a Soyeht pane"));
    assert_eq!(tools, json!([]));
    assert!(
        String::from_utf8_lossy(&outside.stderr).contains("not attached to a Soyeht pane"),
        "{}",
        String::from_utf8_lossy(&outside.stderr)
    );
}
