#!/usr/bin/env python3
"""Exercise slice-two agent sessions with disposable services (launchd on macOS)."""
import argparse
import json
import shutil
import time
from pathlib import Path

from e2e import ROOT, Services, receive, require, send
from e2e_platform import connect_endpoint, database
from generated_profile import DEV_PTYD_SOCKET


def until(action, message, seconds=20):
    deadline = time.monotonic() + seconds
    while True:
        value = action()
        if value:
            return value
        require(time.monotonic() < deadline, message)
        time.sleep(0.02)


AgentServices = Services


def query(services, sql, args=()):
    with database(services.data / "soyeht.db") as db:
        return db.execute(sql, args).fetchall()


def events(services, method, pane=None):
    values = [json.loads(row[0]) for row in query(
        services, "SELECT params FROM events WHERE method=? ORDER BY id", (method,))]
    return [value for value in values if pane is None or value.get("paneId") == pane]


def ended(services, prompt):
    return any(event.get("promptId") == prompt for event in events(services, "pane.turn_ended"))


def turn(services, client, pane, text, **params):
    prompt = client.mutate("pane.prompt", paneId=pane, text=text, **params)["promptId"]
    until(lambda: ended(services, prompt), "agent did not finish its prompt")
    return prompt


def request(client, method, params):
    client.next += 1
    send(client.stream, {"jsonrpc": "2.0", "id": client.next, "method": method, "params": params})
    while True:
        message = receive(client.stream)
        if message.get("id") == client.next:
            return message
        client.pending.append(message)


def question(client, pane, kind="permission"):
    return next((entry["request"] for entry in client.snapshot()[1].get("requests", [])
                 if entry["kind"] == kind and entry["request"]["paneId"] == pane), None)


def turn_items(client, pane, prompt):
    """The transcript rows that belong to one prompt."""
    items = client.call("pane.transcript", {"paneId": pane, "limit": 100})["items"]
    return [item for item in items if item.get("promptId") == prompt]


def assistant_text(items):
    return " ".join(item["text"] for item in items
                    if item["role"] == "assistant" and item["kind"] == "message").strip()


def transcript(client, pane):
    values, cursor = [], None
    while True:
        params = {"paneId": pane, "limit": 2}
        if cursor:
            params["before"] = cursor
        page = client.call("pane.transcript", params)
        values.extend(page["items"])
        cursor = page.get("nextCursor")
        if cursor is None:
            break
    identities = [(item["generation"], item["seq"], item["localId"]) for item in values]
    require(len(identities) == len(set(identities)), "transcript pagination repeated a message")
    return sorted(values, key=lambda item: int(item["seq"]))


def open_agent(client, workspace, harness):
    pane = client.mutate("pane.create", workspaceId=workspace, mode="agent", harness=harness)["paneId"]
    until(lambda: client.pane(pane)["agent"].get("sessionId"), "agent session did not open")
    return pane


def supervisor_pending(services, generation, instance):
    with connect_endpoint(services.data / DEV_PTYD_SOCKET, timeout=5) as stream:
        send(stream, {"method": "hello", "params": {"version": services.ptyd_version,
                                                     "engine_generation": generation}})
        require(receive(stream)["status"] == "welcome", "supervisor handshake failed")
        send(stream, {"method": "pending_requests", "params": {"instance_id": instance}})
        result = receive(stream)
        require(result["status"] == "pending_requests", "pending ledger unavailable")
        require(not result["value"]["recovery_gap"], "pending ledger lost a question")
        return result["value"]["items"]


def harness_registry(services):
    fake = services.binaries / "soyeht-fake-acp"
    entries = []
    for harness, policy in [("test", "supported"), ("no-load", "unsupported"),
                            ("advertised", "advertised-only"), ("login", "supported")]:
        args = ["--state-dir", str(services.data / harness), "--load", policy]
        if harness == "login":
            args.append("--require-login")
        entries.append({"id": harness, "name": harness, "command": str(fake),
                        "args": args, "version": "1"})
    registry = services.data / "harnesses.json"
    registry.write_text(json.dumps(entries))
    return entries, registry


def acceptance_streaming(services, client, pane, results):
    observers = [services.client(name) for name in ("first-observer", "second-observer")]
    for observer in observers:
        cursor = observer.snapshot()[0]["cursor"]
        observer.pending.clear()
        observer.call("events.subscribe", {"since": cursor})
    prompt = client.mutate("pane.prompt", paneId=pane, text="hello world")["promptId"]
    copies = []
    for observer in observers:
        updates = []
        while True:
            message = receive(observer.stream)
            params = message.get("params", {})
            if params.get("paneId") != pane:
                continue
            if message.get("method") == "pane.update":
                updates.append(params)
            if message.get("method") == "pane.turn_ended" and params["promptId"] == prompt:
                break
        copies.append(updates)
        observer.close()
    require(copies[0] == copies[1], "two clients received different conversation updates")
    require(sum(item["kind"] == "agent_message_chunk" for item in copies[0]) >= 2,
            "response was not streamed in chunks")
    before = transcript(client, pane)
    require(any(item["role"] == "user" and item["text"] == "hello world" for item in before),
            "accepted user prompt was not persisted")
    client = services.restart_engine()
    require(transcript(client, pane) == before, "engine restart changed the transcript")
    results.append({"acceptance": 1, "name": "two_clients_stream_and_page_history", "passed": True})
    return client


def acceptance_permission(services, client, pane, results):
    turn_id = client.mutate("pane.prompt", paneId=pane, text="permission")["promptId"]
    first = until(lambda: question(client, pane), "permission was not presented")
    other = services.client("second-answer")
    require(question(other, pane) == first, "permission differs between clients")
    accepted = client.mutate("pane.permission.answer", paneId=pane, requestId=first["requestId"], optionId="once")
    refused = other.mutate("pane.permission.answer", paneId=pane, requestId=first["requestId"], optionId="deny")
    require(accepted == refused and accepted["by"] == "mac", "first permission answer did not win")
    until(lambda: ended(services, turn_id), "permission turn did not finish")
    client.mutate("pane.prompt", paneId=pane, text="permission")
    expiry = until(lambda: question(client, pane), "expiring permission missing")
    services.stop_engine()
    with database(services.data / "soyeht.db") as db:
        db.execute("UPDATE agent_requests SET expires_ms=0 WHERE request_id=?", (expiry["requestId"],))
    services.start("engine")
    client = services.client()
    until(lambda: any(item.get("attentionId") == expiry["requestId"] for item in client.snapshot()[1]["attention"]),
          "expired permission was not placed in attention")
    require(all(not item.get("requestId") for item in client.snapshot()[1]["attention"]),
            "expired question remained actionable")
    receipt = client.mutate("pane.permission.answer", paneId=pane, requestId=expiry["requestId"], optionId="once")
    require(receipt["result"] == "expired", "expired permission was accepted")
    until(lambda: client.pane(pane)["state"] == "idle", "expiry did not finish the turn")
    results.append({"acceptance": 2, "name": "first_answer_and_durable_expiry", "passed": True})
    return client


def acceptance_cancel(services, client, pane, results):
    original = client.pane(pane)
    client.mutate("pane.prompt", paneId=pane, text="silent")
    until(lambda: client.pane(pane)["state"] == "working", "silent turn did not start")
    started = time.monotonic()
    client.mutate("pane.cancel", paneId=pane)
    until(lambda: client.pane(pane)["state"] == "idle", "cancel was not confirmed", seconds=10)
    require(time.monotonic() - started < 10 and client.pane(pane)["pid"] == original["pid"],
            "healthy cancellation killed or exceeded its deadline")
    require(client.pane(pane)["agent"]["queuePaused"], "interruption did not pause the queue")
    client.mutate("pane.queue.resume", paneId=pane)
    results.append({"acceptance": 3, "name": "cancel_preserves_process_and_pauses_queue", "passed": True})


def acceptance_engine_outage(services, client, pane, results):
    original = client.pane(pane)
    generation = client.call("engine.status", {})["generation"]
    # The harness holds the turn until the release file exists, so the permission request can
    # only be made once the engine is down, however long the service manager takes to stop it.
    harness_directory = services.data / "test"
    turn_id = client.mutate("pane.prompt", paneId=pane, text="permission-gated")["promptId"]
    until(lambda: (harness_directory / "permission.waiting").exists(), "gated turn did not start")
    services.launcher.take_down("engine")
    (harness_directory / "permission.release").write_bytes(b"continue")
    supervisor = json.loads((services.data / "ptyd-registry.json").read_text())
    instance = next(value["instance_id"] for value in supervisor["instances"].values()
                    if value.get("pid") == original["pid"])
    until(lambda: any(item["method"] == "session/request_permission"
                      for item in supervisor_pending(services, generation, instance)),
          "harness did not ask during the engine outage")
    require(not query(services, "SELECT request_id FROM agent_requests WHERE pane_id=? AND result IS NULL", (pane,)),
            "permission had already reached the engine before the outage")
    services.start("engine")
    client = services.client()
    recovered = until(lambda: question(client, pane), "outage permission was not recovered")
    require(client.pane(pane)["pid"] == original["pid"], "engine outage replaced the harness")
    client.mutate("pane.permission.answer", paneId=pane, requestId=recovered["requestId"], optionId="once")
    until(lambda: ended(services, turn_id), "recovered permission did not finish")
    require(len([event for event in events(services, "pane.permission_answered", pane)
                 if event["requestId"] == recovered["requestId"]]) == 1, "recovered permission was answered twice")
    results.append({"acceptance": 4, "name": "permission_created_during_engine_outage", "passed": True})
    return client


def acceptance_configuration(services, client, pane, results):
    session = client.pane(pane)["agent"]["sessionId"]
    for key, value in [("model", "model-b"), ("effort", "high")]:
        confirmed = client.mutate("pane.config.set", paneId=pane, configId=key, value=value)
        require(confirmed["sessionPreserved"], "configuration restarted the session")
    error = request(client, "pane.config.set", {"paneId": pane, "configId": "model",
                    "value": "rejected-model", "idempotencyKey": "refused-model"})
    require(error["error"]["data"]["kind"] == "invalid_params", "native parameter error was not preserved")
    require(client.pane(pane)["agent"]["sessionId"] == session, "parameter error reset the session")
    require(events(services, "pane.config_changing", pane) and events(services, "pane.config_changed", pane),
            "configuration lifecycle events missing")
    results.append({"acceptance": 5, "name": "native_configuration_confirmation", "passed": True})


def set_mode(client, pane, mode):
    applied = client.mutate("pane.config.set", paneId=pane, configId="mode", value=mode)
    require(applied["effective"] == mode, f"the harness did not confirm mode {mode}")


def acceptance_full_access_forms(services, client, pane, results):
    set_mode(client, pane, "allow_all")
    count = len(events(services, "pane.permission_requested", pane))
    turn(services, client, pane, "permission")
    require(len(events(services, "pane.permission_requested", pane)) == count,
            "the harness's full-access mode still asked")
    form_prompt = client.mutate("pane.prompt", paneId=pane, text="elicitation")["promptId"]
    form = until(lambda: question(client, pane, "elicitation"), "full access hid an elicitation")
    require(not ended(services, form_prompt), "full access answered the form")
    client.mutate("pane.elicitation.answer", paneId=pane, requestId=form["requestId"], fields={"answer": "yes"})
    until(lambda: ended(services, form_prompt), "form response did not finish")
    set_mode(client, pane, "ask")
    results.append({"acceptance": 6, "name": "permission_modes_never_answer_forms", "passed": True})


def acceptance_queue(services, client, pane, results):
    client.mutate("pane.prompt", paneId=pane, text="permission")
    pending = until(lambda: question(client, pane), "queue setup permission missing")
    waiting = client.mutate("pane.prompt", paneId=pane, text="queued human")["promptId"]
    require(any(item["promptId"] == waiting for item in client.call("pane.queue.list", {"paneId": pane})["items"]),
            "busy prompt was not queued")
    client.mutate("pane.permission.answer", paneId=pane, requestId=pending["requestId"], optionId="once")
    until(lambda: ended(services, waiting), "human queue did not dispatch after idle")
    client.mutate("pane.draft", paneId=pane, active=True)
    automatic = client.mutate("pane.prompt", paneId=pane, text="automatic prompt", origin="automatic")["promptId"]
    client = services.restart_engine()
    require(any(item["promptId"] == automatic for item in client.call("pane.queue.list", {"paneId": pane})["items"]),
            "automatic prompt ignored the durable draft signal")
    require(not ended(services, automatic), "automatic prompt ran over a human draft")
    client.mutate("pane.draft", paneId=pane, active=False)
    until(lambda: ended(services, automatic), "automatic queue did not resume after draft ended")
    results.append({"acceptance": 7, "name": "durable_queue_respects_human_draft", "passed": True})
    return client


def acceptance_supervisor_loss(services, client, workspace, pane, results):
    without = open_agent(client, workspace, "no-load")
    turn(services, client, without, "no resume history")
    old = {key: client.pane(key) for key in (pane, without)}
    preserved = [(item["role"], item["text"]) for item in transcript(client, pane)
                 if item["kind"] == "message"]
    services.launcher.crash("ptyd")
    for key in (pane, without):
        until(lambda key=key: client.pane(key)["generation"] > old[key]["generation"]
              and client.pane(key)["state"] in ("idle", "starting")
              and client.pane(key)["phrase"] == "Agent session ready", "supervisor replacement was not reconciled")
    require(client.pane(pane)["agent"]["sessionId"] == old[pane]["agent"]["sessionId"], "supported load lost native session")
    require(client.pane(without)["agent"]["sessionId"] != old[without]["agent"]["sessionId"], "unsupported load reused a lost session")
    require(events(services, "pane.session_lost", without), "unsupported load was silent")
    restored = [(item["role"], item["text"]) for item in transcript(client, pane)
                if item["kind"] == "message"]
    require(restored == preserved, f"native replay changed conversation messages: expected {preserved!r}; received {restored!r}")
    results.append({"acceptance": 8, "name": "supervisor_loss_load_or_visible_session_loss", "passed": True})


def acceptance_probe_cache(services, client, workspace, entries, registry, results):
    advertised = open_agent(client, workspace, "advertised")
    fingerprint = client.pane(advertised)["agent"]["fingerprint"]
    cached = query(services, "SELECT payload FROM agent_probes WHERE fingerprint=? AND method='session/load'", (fingerprint,))
    require(len(cached) == 1 and json.loads(cached[0][0])["result"] == "unsupported", "advertised-only load was trusted")
    client.mutate("pane.close", paneId=advertised)
    again = open_agent(client, workspace, "advertised")
    require(client.pane(again)["agent"]["fingerprint"] == fingerprint, "same harness changed fingerprint")
    require(query(services, "SELECT payload FROM agent_probes WHERE fingerprint=? AND method='session/load'", (fingerprint,)) == cached,
            "unsupported capability was probed again")
    client.mutate("pane.close", paneId=again)
    entry = next(entry for entry in entries if entry["id"] == "advertised")
    entry["args"] += ["--version-label", "2"]
    entry["version"] = "2"
    registry.write_text(json.dumps(entries))
    changed = open_agent(client, workspace, "advertised")
    new_fingerprint = client.pane(changed)["agent"]["fingerprint"]
    require(new_fingerprint != fingerprint and query(services,
            "SELECT payload FROM agent_probes WHERE fingerprint=? AND method='session/load'", (new_fingerprint,)),
            "changed harness version did not get a new probe")
    results.append({"acceptance": 9, "name": "probe_cache_requires_version_change", "passed": True})


def acceptance_login(services, client, workspace, results):
    login = client.mutate("pane.create", workspaceId=workspace, mode="agent", harness="login")["paneId"]
    until(lambda: events(services, "pane.needs_login", login), "native login requirement missing")
    started = client.mutate("pane.auth.start", paneId=login, authMethodId="login")
    terminal = started["terminalPaneId"]
    created = next((event["pane"] for event in events(services, "pane.created")
                    if event["pane"]["paneId"] == terminal), None)
    require(terminal != login and created is not None and created["mode"] == "terminal",
            "login did not use a separate raw terminal")
    until(lambda: events(services, "pane.auth_finished", login)
          and client.pane(login)["agent"].get("sessionId"), "login did not unlock a session")
    require(client.pane(terminal) is None, "completed sign-in terminal remained open")
    results.append({"acceptance": 10, "name": "native_login_terminal", "passed": True})


def acceptances(services, results):
    entries, registry = harness_registry(services)
    client = services.client("first")
    workspace = client.mutate("workspace.create", name="Agent acceptances", path=str(services.data))["workspace"]["workspaceId"]
    pane = open_agent(client, workspace, "test")
    client = acceptance_streaming(services, client, pane, results)
    client = acceptance_permission(services, client, pane, results)
    acceptance_cancel(services, client, pane, results)
    client = acceptance_engine_outage(services, client, pane, results)
    acceptance_configuration(services, client, pane, results)
    acceptance_full_access_forms(services, client, pane, results)
    client = acceptance_queue(services, client, pane, results)
    acceptance_supervisor_loss(services, client, workspace, pane, results)
    acceptance_probe_cache(services, client, workspace, entries, registry, results)
    acceptance_login(services, client, workspace, results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "engine/target/debug")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    require(all((binaries / name).is_file() for name in
                ("soyeht", "soyeht-engine", "soyeht-mcp", "soyeht-ptyd", "soyeht-fake-acp")),
            "build workspace binaries before running agent acceptances")
    services = AgentServices(binaries)
    started, results, failure = time.monotonic(), [], None
    try:
        shutil.copy2(binaries / "soyeht-fake-acp", services.binaries / "soyeht-fake-acp")
        services.start("ptyd")
        services.start("engine")
        acceptances(services, results)
    except Exception as error:
        failure = f"{type(error).__name__}: {error}"
    finally:
        try:
            services.close()
        except Exception as error:
            failure = f"cleanup failed: {error}"
    report = {"passed": failure is None and len(results) == 10,
              "seconds": round(time.monotonic() - started, 2), "results": results}
    if failure:
        report["error"] = failure
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if not report["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
