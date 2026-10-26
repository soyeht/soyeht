#!/usr/bin/env python3
"""Exercise editor, language, and usage integrations under disposable launchd jobs."""
import base64
import json
import os
import shutil
import signal
import subprocess

from e2e import TEST_ENGINE, argument_parser, require, require_binaries, run_suite
from e2e_platform import MACOS, WINDOWS, database, executable
from e2e_agents import AgentServices, request, until


AUTHOR = {"actor": {"role": "human_local", "client": "editor-e2e"}, "name": "you"}


def cursor_state_path(home):
    """Where the engine looks for Cursor's state database under `home` (usage/sources/cursor.rs)."""
    if MACOS:
        root = home / "Library/Application Support"
    elif WINDOWS:
        root = home / "AppData/Roaming"
    else:
        root = home / ".config"
    return root / "Cursor/User/globalStorage/state.vscdb"


class EditorServices(AgentServices):
    def __init__(self, binaries):
        # The usage sources read fixtures only through the test engine's hooks (ADR 0091).
        super().__init__(binaries, engine=TEST_ENGINE)

    def install_fakes(self, binaries):
        analyzer = self.binaries / executable("rust-analyzer")
        shutil.copy2(binaries / executable("soyeht-fake-lsp"), analyzer)
        analyzer.chmod(0o700)
        readers = self.data / "usage-readers"
        readers.mkdir(mode=0o700)
        # The fixture layout the test engine's FixtureHost reads (engine usage/fixture_host.rs): home/
        # stands in for the home directory, the scripts for curl, security and codex.
        home = readers / "home"
        self.claude_login = home / ".claude/.credentials.json"
        self.claude_login.parent.mkdir(parents=True)
        cursor_state = cursor_state_path(home)
        cursor_state.parent.mkdir(parents=True)
        with database(cursor_state) as db:
            db.execute("CREATE TABLE ItemTable (key TEXT UNIQUE, value BLOB)")
            db.execute("INSERT INTO ItemTable VALUES (?, ?)",
                             ("cursorAuth/accessToken", fake_jwt({"sub": "auth0|e2e", "exp": 4102444800})))
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
            # Without a stored login the usage source answers "sign in needed" and starts nothing
            # (ADR 0111): "signed_out". "unavailable" is a signed-in machine whose endpoint cannot
            # read (an expired login), so the session source looks for the program, which the
            # fixture does not have: "tool not found".
            expires = 4102444800000 if values["first"] == "ok" else 1
            self.claude_login.write_text(json.dumps({"claudeAiOauth": {
                "accessToken": "e2e-access-token",
                "expiresAt": expires,
                "scopes": ["user:inference", "user:profile"],
            }}))
            if values["first"] == "signed_out":
                self.claude_login.unlink()

    def _write_reader(self, path, source):
        path.write_text(source)
        path.chmod(0o700)

    def _set_service_environment(self, readers):
        for component in ("engine", "ptyd"):
            self.prepend_path(component, self.binaries)
        self.set_environment("engine", SOYEHT_TEST_USAGE_READER_DIR=str(readers),
                             SOYEHT_TEST_USAGE_MODE=str(self.usage_mode))


SECOND_SOURCE = """#!/usr/bin/env python3
import json, os, sys
mode=json.load(open(os.environ["SOYEHT_TEST_USAGE_MODE"]))["second"]
if mode != "ok": sys.exit(1)
refreshed=False
for line in sys.stdin:
 value=json.loads(line)
 if value.get("id") == 1:
  print(json.dumps({"jsonrpc":"2.0","id":1,"result":{}}),flush=True)
 if value.get("id") == 2 and value.get("params") == {"refreshToken": True}:
  refreshed=True
  print(json.dumps({"jsonrpc":"2.0","id":2,"result":{"account":None,"requiresOpenaiAuth":True}}),flush=True)
 if value.get("id") == 3:
  if not refreshed:
   print(json.dumps({"jsonrpc":"2.0","id":3,"error":{"code":-32603,"message":"401 token_expired"}}),flush=True)
   break
  result={"rateLimits":{"primary":{"usedPercent":30,"resetsAt":1000},"secondary":{"usedPercent":60,"resetsAt":2000}}}
  print(json.dumps({"jsonrpc":"2.0","id":3,"result":result}),flush=True)
  break
"""

KEYCHAIN_SOURCE = """#!/usr/bin/env python3
import sys
sys.exit(44)
"""

HTTP_SOURCE = """#!/usr/bin/env python3
import json, os, sys
sys.stdin.read()
url=sys.argv[-1]
modes=json.load(open(os.environ["SOYEHT_TEST_USAGE_MODE"]))
status, body = 404, {}
if url == "https://api.anthropic.com/api/oauth/usage":
 status = 200 if modes["first"] == "ok" else 500
 body = {"five_hour":{"utilization":25,"resets_at":"2026-10-03T21:00:00Z"},
         "seven_day":{"utilization":50,"resets_at":"2026-10-08T15:00:00Z"}}
if url == "https://cursor.com/api/usage-summary":
 status = {"ok": 200, "login_needed": 401, "no_access": 403}[modes["third"]]
 body = {"billingCycleEnd":"2026-10-20T00:00:00.000Z","individualUsage":{"plan":{"totalPercentUsed":40}}}
print(json.dumps(body))
print("SOYEHT_STATUS:%d" % status)
print("SOYEHT_RETRY:")
"""


def fake_jwt(claims):
    payload = base64.urlsafe_b64encode(json.dumps(claims).encode()).rstrip(b"=").decode()
    return f"e2e.{payload}.signature"


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
    # A failed refresh keeps the last good reading as last known and names why it failed.
    for provider, modes, reason in (
        ("claude", {"first": "unavailable"}, "tool_not_found"),
        ("claude", {"first": "signed_out"}, "login_needed"),
        ("cursor", {"third": "login_needed"}, "login_needed"),
        ("cursor", {"third": "no_access"}, "no_access"),
    ):
        services.set_usage_modes(**modes)
        kept = refresh_provider(client, provider, f"usage-{provider}-{reason}")
        require(kept["state"] == "stale" and kept.get("unavailableReason") == reason
                and kept.get("primary"), f"{provider} did not keep its reading after {reason}")
        client = services.restart_engine()
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
    args = argument_parser(__doc__).parse_args()
    binaries = args.bin_dir.resolve()
    names = ("soyeht", "soyeht-engine", TEST_ENGINE, "soyeht-mcp", "soyeht-ptyd", "soyeht-fake-lsp")
    require_binaries(
        binaries, names,
        "build workspace binaries and the test engine (cargo build -p soyeht-engine --features test-hooks)"
        " before running editor acceptances")
    services = EditorServices(binaries)
    run_suite(services, lambda: services.install_fakes(binaries),
              lambda results: run_acceptances(services, results), 4, args.report)


if __name__ == "__main__":
    main()
