#!/usr/bin/env python3
"""Exercise editor, language, and usage integrations under disposable launchd jobs."""
import argparse
import json
import os
import plistlib
import shutil
import signal
import subprocess
import time
from pathlib import Path

from e2e import ROOT, require
from e2e_agents import AgentServices, request, until


AUTHOR = {"actor": {"role": "human_local", "client": "editor-e2e"}, "name": "you"}


class EditorServices(AgentServices):
    def install_fakes(self, binaries):
        shutil.copy2(binaries / "soyeht-fake-lsp", self.binaries / "rust-analyzer")
        (self.binaries / "rust-analyzer").chmod(0o700)
        readers = self.data / "usage-readers"
        readers.mkdir(mode=0o700)
        # Keep this fixture beside the other readers in default_commands (sources.rs).
        self.usage_cache = readers / "claude-cache.json"
        self.usage_mode = readers / "mode.json"
        self.set_usage_modes(first="ok", second="ok", third="ok")
        self._write_reader(readers / "second-source", SECOND_SOURCE)
        self._write_reader(readers / "keychain-source", KEYCHAIN_SOURCE)
        self._write_reader(readers / "http-source", HTTP_SOURCE)
        self._set_service_environment(readers)

    def set_usage_modes(self, **values):
        current = {}
        if self.usage_mode.exists():
            current = json.loads(self.usage_mode.read_text())
        current.update(values)
        self.usage_mode.write_text(json.dumps(current))
        if "first" in values:
            if values["first"] == "ok":
                self.usage_cache.write_text(json.dumps({
                    "cachedUsageUtilization": {
                        "fetchedAtMs": int(time.time() * 1000),
                        "utilization": {
                            "five_hour": {"utilization": 25, "resets_at": "later"},
                            "seven_day": {"utilization": 50, "resets_at": "next week"},
                        },
                    },
                }))
            else:
                self.usage_cache.unlink(missing_ok=True)

    def _write_reader(self, path, source):
        path.write_text(source)
        path.chmod(0o700)

    def _set_service_environment(self, readers):
        for component in ("engine", "ptyd"):
            path = self.data / f"{component}.plist"
            values = plistlib.loads(path.read_bytes())
            environment = values["EnvironmentVariables"]
            environment["PATH"] = f"{self.binaries}:/usr/bin:/bin:/usr/sbin:/sbin"
            if component == "engine":
                environment["SOYEHT_TEST_USAGE_READER_DIR"] = str(readers)
                environment["SOYEHT_TEST_USAGE_MODE"] = str(self.usage_mode)
            path.write_bytes(plistlib.dumps(values))


SECOND_SOURCE = """#!/usr/bin/env python3
import json, os, sys
mode=json.load(open(os.environ["SOYEHT_TEST_USAGE_MODE"]))["second"]
if mode != "ok": sys.exit(1)
for line in sys.stdin:
 value=json.loads(line)
 if value.get("id") == 1:
  print(json.dumps({"jsonrpc":"2.0","id":1,"result":{}}),flush=True)
 if value.get("id") == 2:
  result={"rateLimits":{"primary":{"usedPercent":30,"resetsAt":1000},"secondary":{"usedPercent":60,"resetsAt":2000}}}
  print(json.dumps({"jsonrpc":"2.0","id":2,"result":result}),flush=True)
  break
"""

KEYCHAIN_SOURCE = """#!/usr/bin/env python3
import json, os, sys
mode=json.load(open(os.environ["SOYEHT_TEST_USAGE_MODE"]))["third"]
if mode == "login_needed": sys.exit(44)
if "-w" in sys.argv:
 if mode == "no_access": sys.exit(36)
 print("test-token")
"""

HTTP_SOURCE = """#!/usr/bin/env python3
import json, os, sys
sys.stdin.read()
mode=json.load(open(os.environ["SOYEHT_TEST_USAGE_MODE"]))["third"]
if mode != "ok": sys.exit(1)
print(json.dumps({"billingCycleEnd":"later","planUsage":{"totalPercentUsed":40}}))
print("SOYEHT_STATUS:200")
print("SOYEHT_RETRY:")
"""


def create_repository(services):
    repository = services.data / "repository"
    (repository / "src").mkdir(parents=True)
    (repository / "src/main.rs").write_text("// start\nfn main() {\n    ready();\n}\n")
    (repository / "README.md").write_text("needle in project\n")
    subprocess.run(["/usr/bin/git", "init", "-q", repository], check=True)
    subprocess.run(["/usr/bin/git", "-C", repository, "add", "."], check=True)
    subprocess.run([
        "/usr/bin/git", "-C", repository, "-c", "user.name=Test", "-c",
        "user.email=test@example.invalid", "commit", "-qm", "initial",
    ], check=True)
    return repository


def open_editor(client, repository):
    workspace = client.mutate(
        "workspace.create", name="Editor acceptance", path=str(repository),
        idempotencyKey="editor-workspace",
    )["workspace"]["workspaceId"]
    opened = client.mutate(
        "file.open", workspaceId=workspace, path="src/main.rs",
        idempotencyKey="editor-open",
    )
    return workspace, opened["file"]["fileId"]


def wait_file(client, file_id, predicate, message):
    def current():
        files = client.snapshot()[1].get("files", [])
        return next((item for item in files if item["fileId"] == file_id and predicate(item)), None)
    return until(current, message)


def wait_lsp(client, workspace, generation):
    def ready():
        servers = client.call("lsp.status", {"workspaceId": workspace, "language": "rust"})["servers"]
        return next((item for item in servers
                     if item["state"] == "ready" and item["generation"] == generation), None)
    return until(ready, "fake language server did not become ready")


def acceptance_editor_durability(services, client, repository, workspace, file_id, results):
    written = client.mutate(
        "file.write", fileId=file_id, baseRevision="1", author=AUTHOR,
        edits=[{"range": {"start": {"line": 0, "column": 3},
                            "end": {"line": 0, "column": 8}}, "text": "draft"}],
        idempotencyKey="editor-write",
    )
    require(written["file"]["dirty"], "editor write was not acknowledged as a draft")
    restored = services.restart_engine()
    wait_file(restored, file_id, lambda item: item["revision"] == "2", "draft did not survive restart")
    saved = wait_file(restored, file_id, lambda item: not item["dirty"], "autosave did not finish")
    require(saved["diskRevision"] == "2", "autosave did not commit the draft revision")
    require((repository / "src/main.rs").read_text().startswith("// draft"), "autosave did not write disk")
    history = restored.call("file.history", {"fileId": file_id, "limit": 10})
    require(history["items"] and history["items"][0]["author"]["name"] == "you",
            "history did not preserve the writer")
    results.append({"acceptance": 1, "name": "editor_draft_restart_autosave_history", "passed": True})
    return restored


def acceptance_language_restart(services, client, workspace, file_id, results):
    first = wait_lsp(client, workspace, 1)
    symbols = client.call("file.symbols", {"fileId": file_id, "revision": "2"})
    diagnostics = client.call("file.diagnostics", {"fileId": file_id, "revision": "2"})
    position = {"line": 1, "column": 3}
    definition = client.call(
        "file.definition", {"fileId": file_id, "revision": "2", "position": position})
    references = client.call(
        "file.references", {"fileId": file_id, "revision": "2", "position": position})
    require(symbols["items"][0]["name"] == "main", "fake symbols were not routed")
    require(diagnostics["items"][0]["message"] == "deterministic warning", "diagnostic missing")
    require(definition.get("location") and len(references["items"]) == 1,
            "definition or scoped references missing")
    instance = lsp_instance(services)
    require(instance["spawn"]["cwd"] == str(services.data / "repository"), "wrong LSP cwd")
    os.kill(int(instance["pid"]), signal.SIGKILL)
    second = wait_lsp(client, workspace, int(first["generation"]) + 1)
    until(lambda: client.call("file.diagnostics", {"fileId": file_id, "revision": "2"})["items"],
          "diagnostics did not resume after language server restart")
    require(second["state"] == "ready", "restarted language server was not ready")
    results.append({"acceptance": 2, "name": "fake_lsp_queries_restart_and_diagnostics", "passed": True})


def lsp_instance(services):
    registry = json.loads((services.data / "ptyd-registry.json").read_text())
    values = [item for item in registry["instances"].values()
              if item["spawn"]["kind"] == "pipes_lsp" and not item["closed"]]
    require(len(values) == 1, "expected one live language server")
    return values[0]


def acceptance_search_and_diff(client, workspace, file_id, results):
    content = client.mutate(
        "file.search", workspaceId=workspace, query="needle", caseSensitive=True, limit=10,
        idempotencyKey="editor-search",
    )
    paths = client.mutate(
        "file.search", workspaceId=workspace, scope="path", query="main", caseSensitive=True,
        limit=10, idempotencyKey="editor-path-search",
    )
    diff = client.call("file.diff", {"fileId": file_id, "revision": "2"})
    require(content["items"][0]["path"] == "README.md", "content search missed project file")
    require(paths["items"][0]["path"] == "src/main.rs", "path search missed editor file")
    require(diff["hunks"] and diff["blame"], "buffer diff or blame was empty")
    results.append({"acceptance": 3, "name": "project_search_and_buffer_git_diff", "passed": True})


def states(readings):
    return {item["provider"]: item["state"] for item in readings}


def refresh_provider(client, provider, key):
    return client.mutate("usage.refresh", provider=provider, idempotencyKey=key)["providers"][0]


def acceptance_usage(services, client, results):
    refreshed = client.mutate("usage.refresh", idempotencyKey="usage-all")["providers"]
    require(set(states(refreshed).values()) == {"ok"} and len(refreshed) == 3,
            "fake usage readers did not all return ok")
    limited = request(client, "usage.refresh", {"provider": "claude", "idempotencyKey": "limited"})
    require(limited["error"]["data"]["kind"] == "rate_limited", "manual refresh was not limited")
    client = services.restart_engine()
    require(set(states(client.call("usage.list", {})["providers"]).values()) == {"stale"},
            "stored readings did not become stale after restart")
    services.set_usage_modes(first="unavailable")
    require(refresh_provider(client, "claude", "usage-unavailable")["state"] == "unavailable",
            "unavailable state was not normalized")
    client = services.restart_engine()
    services.set_usage_modes(third="login_needed")
    require(refresh_provider(client, "cursor", "usage-login")["state"] == "login_needed",
            "login-needed state was not normalized")
    client = services.restart_engine()
    services.set_usage_modes(third="no_access")
    require(refresh_provider(client, "cursor", "usage-access")["state"] == "no_access",
            "no-access state was not normalized")
    results.append({"acceptance": 4, "name": "fake_usage_states_limit_and_restart", "passed": True})


def run_acceptances(services, results):
    repository = create_repository(services)
    client = services.client("editor-e2e")
    workspace, file_id = open_editor(client, repository)
    wait_lsp(client, workspace, 1)
    client = acceptance_editor_durability(services, client, repository, workspace, file_id, results)
    acceptance_language_restart(services, client, workspace, file_id, results)
    acceptance_search_and_diff(client, workspace, file_id, results)
    acceptance_usage(services, client, results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "engine/target/debug")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    names = ("soyeht", "soyeht-engine", "soyeht-mcp", "soyeht-ptyd", "soyeht-fake-lsp")
    require(all((binaries / name).is_file() for name in names),
            "build workspace binaries before running editor acceptances")
    services = EditorServices(binaries)
    started, results, failure = time.monotonic(), [], None
    try:
        services.install_fakes(binaries)
        services.start("ptyd")
        services.start("engine")
        run_acceptances(services, results)
    except Exception as error:
        failure = f"{type(error).__name__}: {error}"
    finally:
        try:
            services.close()
        except Exception as error:
            failure = f"cleanup failed: {error}"
    report = {"passed": failure is None and len(results) == 4,
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
