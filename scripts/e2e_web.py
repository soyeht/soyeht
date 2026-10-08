#!/usr/bin/env python3
"""Exercise web-pane routing and discovery under disposable launchd jobs."""
import argparse
import json
import os
import socket
import time
from pathlib import Path

from e2e import ROOT, Services, receive, require, send
from e2e_agents import request

LOCAL_SERVER_WINDOW_SECONDS = 15


def web_panes(client):
    return [pane for pane in client.snapshot()[1]["panes"] if pane["mode"] == "web"]


def open_web(client, workspace, url, key, **params):
    return client.mutate(
        "web.open", workspaceId=workspace, url=url, idempotencyKey=key, **params
    )["paneId"]


def acceptance_open_restart_and_limits(services, client, results):
    workspace = client.mutate(
        "workspace.create", name="Web acceptance", path=str(services.data),
        idempotencyKey="web-workspace",
    )["workspace"]["workspaceId"]
    first = open_web(client, workspace, "localhost:4173", "web-open")
    focused = open_web(client, workspace, "http://localhost:4173/", "web-focus")
    private = open_web(
        client, workspace, "example.com", "web-private", incognito=True, newPane=True
    )
    require(first == focused and first != private, "web pane reuse or explicit new pane failed")
    panes = {pane["paneId"]: pane for pane in web_panes(client)}
    require(panes[first]["url"] == "http://localhost:4173/", "local URL was not normalized")
    require(panes[private]["url"] == "https://example.com/", "host URL was not normalized")
    require(panes[private]["incognito"], "incognito state was not persisted")
    refused = request(
        client, "web.open",
        {"workspaceId": workspace, "url": "file:///tmp/no", "idempotencyKey": "bad-scheme"},
    )
    require(refused["error"]["data"]["kind"] == "navigation_refused",
            "unsupported scheme did not fail closed")
    human = request(
        client, "web.open",
        {"workspaceId": workspace, "url": "example.com", "mode": "human",
         "idempotencyKey": "host-human"},
    )
    require(human["error"]["data"]["kind"] == "web_host_unavailable",
            "host human mode was not disabled")
    client = services.restart_engine()
    restored = {pane["paneId"]: pane for pane in web_panes(client)}
    require(restored[first]["url"] == panes[first]["url"], "web URL did not survive restart")
    require(restored[private]["incognito"], "incognito state did not survive restart")
    results.append({"acceptance": 1, "name": "web_open_restart_and_limits", "passed": True})
    return client, workspace, first


def acceptance_reverse_host_routing(services, caller, pane, results):
    host = services.client("web-host", ["web_host"])
    caller.next += 1
    operation = caller.next
    parameters = {
        "paneId": pane, "url": "localhost:5173", "idempotencyKey": "navigate-once"
    }
    send(caller.stream, {
        "jsonrpc": "2.0", "id": operation, "method": "web.navigate", "params": parameters,
    })
    command = receive(host.stream)
    require(command["method"] == "web.command", "engine did not route a reverse host command")
    require(command["params"]["command"] == "navigate", "engine routed the wrong command")
    require(command["params"]["args"]["url"] == "http://localhost:5173/",
            "reverse command did not carry the normalized URL")
    require(command["params"]["args"]["_soyehtAccess"]["session"] == "human",
            "reverse command omitted authenticated human access")
    send(host.stream, {
        "jsonrpc": "2.0", "id": command["id"], "result": {"paneId": pane},
    })
    reply = receive_for(caller, operation)
    require(reply["result"] == {"paneId": pane, "accepted": True},
            "host result was not translated to the public result")
    repeated = request(caller, "web.navigate", parameters)
    require(repeated["result"] == reply["result"], "routed mutation was not idempotent")
    results.append({"acceptance": 2, "name": "reverse_web_host_routing", "passed": True})
    return host


def receive_for(client, request_id):
    while True:
        message = receive(client.stream)
        if message.get("id") == request_id:
            return message
        client.pending.append(message)


def acceptance_missing_host_fails_closed(caller, host, pane, results):
    host.close()
    failed = request(
        caller, "web.navigate",
        {"paneId": pane, "action": "reload", "idempotencyKey": "missing-host"},
    )
    require(failed["error"]["data"]["kind"] == "web_host_unavailable",
            "missing web host did not fail closed")
    results.append({"acceptance": 3, "name": "missing_web_host_fails_closed", "passed": True})


def open_low_listener():
    """Listen on the lowest free unprivileged loopback port.

    The engine lists local servers sorted by port and capped at 50, so an
    ephemeral port can fall past the cap on a host with many listeners. The
    lowest free port above the privileged range sorts ahead of those listeners.
    SO_REUSEADDR is left off so BSD never lets the bind coexist with another
    process's wildcard listener on the same port.
    """
    for port in range(1024, 65536):
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            listener.bind(("127.0.0.1", port))
        except OSError:
            listener.close()
            continue
        listener.listen()
        return listener
    raise OSError("no free unprivileged port for the local server acceptance")


def wait_for_local_server(client, port, listed, message):
    """Poll servers.local.list until this process's listener is (not) listed.

    The engine rescans every 5s, and a scan under host load can come back
    empty, so allow three rescan periods. Matching on pid as well as port keeps
    another process that reuses the port from changing the verdict.
    """
    deadline = time.monotonic() + LOCAL_SERVER_WINDOW_SECONDS
    while True:
        servers = client.call("servers.local.list", {})["servers"]
        require(len(servers) <= 50, "local server list exceeded its bound")
        if any(server["port"] == port and server["pid"] == os.getpid()
               for server in servers) == listed:
            return
        require(time.monotonic() < deadline, message)
        time.sleep(1)


def acceptance_local_server_changes(client, listener, results):
    port = listener.getsockname()[1]
    cursor = client.snapshot()[0]["cursor"]
    client.call("events.subscribe", {"since": cursor})
    wait_for_local_server(client, port, True, "live local server was not discovered")
    listener.close()
    client.pending.clear()
    wait_for_local_server(client, port, False, "closed local server remained listed")
    changes = [message for message in client.pending
               if message.get("method") == "servers.local.changed"]
    require(changes and len(changes[-1]["params"]["servers"]) <= 50,
            "local server change was not published as a bounded event")
    results.append({"acceptance": 4, "name": "bounded_local_server_changes", "passed": True})


def run_acceptances(services, listener, results):
    caller = services.client("web-e2e")
    caller, workspace, pane = acceptance_open_restart_and_limits(
        services, caller, results
    )
    host = acceptance_reverse_host_routing(services, caller, pane, results)
    acceptance_missing_host_fails_closed(caller, host, pane, results)
    acceptance_local_server_changes(caller, listener, results)
    require(len(web_panes(caller)) == 2, "web acceptance left an unexpected pane count")
    for item in web_panes(caller):
        caller.mutate("pane.close", paneId=item["paneId"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "engine/target/debug")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    require(all((binaries / name).is_file() for name in
                ("soyeht", "soyeht-engine", "soyeht-mcp", "soyeht-ptyd")),
            "build workspace binaries before running web acceptances")
    listener = open_low_listener()
    services = Services(binaries)
    started, results, failure = time.monotonic(), [], None
    try:
        services.start("ptyd")
        services.start("engine")
        run_acceptances(services, listener, results)
    except Exception as error:
        failure = str(error)
    finally:
        listener.close()
        try:
            services.close()
        except Exception as error:
            failure = f"cleanup failed: {error}"
    report = {
        "passed": failure is None and len(results) == 4,
        "seconds": round(time.monotonic() - started, 2), "results": results,
    }
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
