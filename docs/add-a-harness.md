# How to add a harness

A harness is an agent program that Soyeht runs in an agent pane over ACP v1 (the Agent
Client Protocol): the engine starts it as a child process and speaks line-delimited JSON
over its standard input and output (`docs/contracts/04-acp-session.md`). The clients never
speak ACP; they receive the engine's events. So adding a harness is almost always an engine
change or no code change at all, and no client change.

There are three levels. Start at the first and go further only if you need to.

| Level | You change | Result |
| --- | --- | --- |
| 1. A local harness | One file in your data directory, or Preferences on the Mac | Your machine lists it, opens panes with it, switches to and from it. Nothing to merge. |
| 2. A built-in harness | The pinned registry snapshot in the engine | Every install lists it. |
| 3. Special behaviour | Engine code, per harness | Usage rings, skills folders, terminal hooks, a display name. Only when the harness needs it. |

Background to read: ADR 0066 (catalogue and authentication boundaries), ADR 0067 (switching),
ADR 0070 (permission modes), contracts 01, 02 and 04.

## What a harness must be

An executable that speaks ACP v1 on stdio. The engine refuses version 2. For the pane to be
useful it should answer `session/new`, advertise its `configOptions` (model, effort and
permission mode are found by their ACP category, `x-clientPolicy.configRoles` in
`contracts/protocol.json`), and report `session/update` notifications. The engine probes
`session/list`, `session/resume` and `session/set_config_option` once per harness version
and falls back with a visible notice when one fails, so a harness that lacks them still
works. Sign-in is the ACP `-32000` response with its `authMethods`.

## Level 1: a local harness

Local harnesses are entries of `harnesses.json` in the profile's data directory (in the Dev
profile, `~/Library/Application Support/Soyeht.next/dev/harnesses.json` on every system; a
`--data-dir` engine reads its own). The file is a JSON array of launch entries:

```json
[
  {
    "id": "my-agent",
    "name": "My agent",
    "command": "my-agent-acp",
    "args": ["--stdio"],
    "version": "1",
    "environment": [{"key": "MY_AGENT_LOG", "value": "warn"}]
  }
]
```

`environment` is optional; every other field is required (`HarnessLaunch` in
`contracts/protocol.json`). The engine validates the file and the same rules apply to the
wire methods `harness.custom.create`, `harness.custom.update`, `harness.custom.remove` and
`harness.custom.list`, which the Mac's Preferences (Harnesses, "Add custom") uses:

- `id` is 1 to 128 characters and must not start with `registry.`; the built-in and custom
  names do not mix, and a custom entry cannot replace a built-in one.
- `command` is a bare program name (looked up on the interactive `PATH`) or an absolute
  path. Shell wrappers (`env`, `sh`, `bash`, `dash`, `fish`, `ksh`, `zsh`) are refused,
  because a launch is never a shell string.
- At most 64 arguments of 4096 bytes, at most 32 environment variables, and no `PATH`, `HOME`,
  `USER`, `LOGNAME` or `SOYEHT_*` key (the harness keeps the person's real environment so its
  own credentials stay visible). At most 64 entries, and the file is at most 1 MiB.
- An entry used by a live or restorable pane cannot be edited or removed; close the panes
  first.
- A custom entry carries no install command and no secret. It counts as installed when its
  command resolves to a regular executable file.

Check it with the CLI against a running engine (`CONTRIBUTING.md`, "Run the engine and a
client in a dev profile"):

```sh
soyeht --profile dev --data-dir "$DATA" harness list
soyeht --profile dev --data-dir "$DATA" model list my-agent
soyeht --profile dev --data-dir "$DATA" pane create --agent my-agent --cwd "$PWD"
```

The engine reads `harnesses.json` again when the catalogue is asked for, so you do not need to
restart it. The desktop client lists any harness the engine lists; it has no form for adding
one yet.

## Level 2: a built-in harness

Built-in harnesses are the pinned snapshot `engine/soyeht-engine/src/agents/acp_registry_snapshot.json`,
derived from a reviewed version of the public ACP registry, and loaded by
`engine/soyeht-engine/src/agents/registry.rs`. There is no network read at startup, launch or
restore. The snapshot is deliberately hard to edit by hand: its bytes, its upstream source and
its entry count are checked.

1. Add the entry to `agents` in the snapshot: `id` (must start with `registry.`), `name`,
   `version`, `command`, `args`, and optionally `environment`, `userDirectories` (folders
   under the home directory that show a local install) and `localCommands` (program names
   that show a local install).
2. A package-runner entry must pin its package: `npx` with `--yes` and `name@version`, or
   `uvx` with `name==version`, never `latest`. Any other command is a single program name.
3. Update the constants at the top of `registry.rs`: `BUILTIN_AGENT_COUNT`, `SNAPSHOT_DIGEST`
   (the SHA-256 of the snapshot file) and, when you move to a newer upstream registry, the
   snapshot's `sourceVersion` and `sourceSha256` together with `SOURCE_DIGEST` and the version
   literal that `builtin_entries` compares. A hand-added entry that does not come from a reviewed
   upstream version must say so in the pull request.
4. Extend `embedded_catalogue_is_pinned_complete_and_contains_required_agents` in `registry.rs`
   with your id, and any test that names the harness.
5. A catalogue entry is launchable, not necessarily installed: a runner-backed entry counts as
   installed only after a successful native session or when a `localCommands` program
   resolves, so it never becomes the default merely because `npx` exists
   (ADR 0066).

## Level 3: behaviour that is specific to one harness

Most harnesses need none of this. Do it only for a measured difference, and keep the rule in
one place.

- **Display name.** Panes are named after the harness, not its adapter (the adapter named
  "Claude Agent" gives panes named "Claude"). The rule is `harness_label` in
  `engine/soyeht-engine/src/agents/naming.rs`.
- **Skills.** Soyeht keeps one skill store for every harness. A harness that reads
  `.agents/skills` needs nothing; one that reads another folder is bridged in
  `engine/soyeht-skills/src/home.rs` (ADR 0075, `docs/research/skills-across-harnesses.md`).
- **Usage rings.** A subscription provider's reader is a source in
  `engine/soyeht-usage/src/sources/`, with a fallback chain in the planner (ADR 0077). The
  provider's display name is `usageProviderNames` in `x-clientPolicy`
  (`contracts/protocol.json`); add it there and regenerate
  (`uv run python contracts/generate.py`), and both clients pick it up.
- **Terminal hook labels.** Session-scoped hooks for experiment ground truth exist for one
  harness (`engine/soyeht-engine/src/agents/hooks.rs`, `terminal_harness.rs`). A second one
  adds its own settings builder there and its id to the check in `TerminalHarnessSession::prepare`.
- **The switch prompt** (`docs/contracts/prompts/harness-switch.en.md`) is shared by every
  harness. Change it only for a reason that holds for all of them.

The clients show whatever the engine lists, group it by readiness, and preselect by the
rules of `contracts/vectors/harness-picker.json` (the Mac and the desktop client both pass
it). If your harness needs a client to behave differently, change that vector first, then
both implementations.

## Tests

- Unit tests live in `registry.rs` (the catalogue and the custom file), `catalog.rs` (setup
  state) and `naming.rs` (labels). Follow the existing cases.
- The deterministic peer is `soyeht-fake-acp` (`engine/soyeht-acp/tests/fake-agent/`), a
  scripted ACP agent built with the workspace. The engine tests write a `harnesses.json`
  pointing at it (`engine/soyeht-engine/tests/agent_login.rs` is the model), and
  `scripts/e2e_agents.py` runs the engine end to end against it. Add a behaviour to the fake
  and a test, not a dependency on a real provider.
- A real harness is an opt-in check that needs a signed-in account:
  `SOYEHT_REAL_ACP=1 uv run python scripts/e2e_real_acp.py` (`docs/agent-acceptances.md`,
  `docs/platforms.md`). It is not part of the fast tier.
- Before the pull request: `cd engine && cargo test --locked -p soyeht-engine`, then the
  checks listed in `CONTRIBUTING.md`. A change under `engine/soyeht-engine/src/agents/` is in
  the engine core, so the pull request needs a second reviewer.
