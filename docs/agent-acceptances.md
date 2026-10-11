# Agent session acceptances

Build the Rust workspace, then run `uv run python scripts/e2e_agents.py --report
.cache/e2e-agents.json`. The local merge gate runs this suite after the six
shell acceptances. The retained workflow has the same step; hosted Actions
remains disabled by project decision.

The suite copies the four built executables into one directory per checkout
under `~/Library/Caches/soyeht-e2e/`, replacing a file only when the build
changed it, and loads uniquely named engine and supervisor jobs into launchd (plain
child processes where launchd is missing) with a disposable data directory. It never uses an installed profile or an
authenticated provider. Its only peer is the deterministic native test process.
Cleanup closes the supervised processes and removes the jobs and data
directory, including after a failed assertion.

macOS privacy (TCC) remembers every executable path that asks for a protected
resource, so a fresh copy per run, or a copy inside a checkout under Documents,
would add a record each run. The local gate therefore runs
`scripts/tcc_guard.py` around the acceptances: it fails if the user's TCC
database gains or changes a record for those executables, or if the log shows
a privacy prompt or record for them. It only reads, and skips a source it
cannot read (for example without Full Disk Access) with a message.

The ten results cover streamed updates to two protocol clients and paginated
history; first-response permission handling and expiry; healthy cancellation;
a permission created while the engine is absent; confirmed configuration and
native rejection; a full-access mode versus interactive forms; durable queue and draft
priority; supervisor replacement with and without native load; capability cache
invalidation; and sign-in through a separate terminal.

The streaming clients use separate local connections. Swift connection tests
cover device authentication and WebSocket transport; actual Mac/iPhone rendering
and the real-adapter procedure are separate checks. Passing this suite alone
does not establish device UI or real-provider behavior.

An authenticated, opt-in compatibility check uses the official Claude and Codex
ACP adapters, at the versions the engine's registry snapshot pins, in disposable
profiles:

```sh
SOYEHT_REAL_ACP=1 uv run python scripts/e2e_real_acp.py \
  --report .cache/e2e-real-acp.json
```

This command downloads each pinned adapter into a temporary directory. For each
harness it records the permission modes the adapter announces, switches to the
first (most restrictive) one from a second engine client, requires a real
permission request for a harmless shell command and answers it, runs the same
command in the adapter's full-access mode without a request, changes and
restores the advertised model, and kills only the disposable engine while a
real turn is active. A harness that asks for sign-in is reported as skipped,
not failed; `--harness claude` or `--harness codex` selects one, and
`--model codex=MODEL` starts a harness on another announced model when the
account cannot use the configured one. The shell command writes outside the
workspace and temporary directories, because Codex's "Ask for approval" preset
writes inside them without asking. The adapters start their CLIs without the
person's own extensions: Claude Code with `--strict-mcp-config` and project
settings that disable hooks and user plugins, Codex with every configured MCP
server and the apps, browser, computer-use, hooks and plugins features turned
off. Those extensions often run from other apps' bundles or protected folders,
and under the disposable supervisor they would trigger privacy prompts that
name it. Only the engine's own MCP server remains. It removes its
services, files and adapter installations afterward. The command never runs in the
normal merge gate unless `SOYEHT_REAL_ACP=1` is explicitly present. It does not
replace the physical iPhone procedure.

`--skills` runs only the Soyeht skills check (ADR 0075) with Codex:

```sh
SOYEHT_REAL_ACP=1 uv run python scripts/e2e_real_acp.py --skills \
  --report .cache/e2e-real-skills.json
```

Codex runs with a disposable `CODEX_HOME` that holds only a copy of the
person's `auth.json`, so nothing is written to `~/.codex`.

1. The first pane must list the starter `$create-skill` from the Soyeht skills
   home.
2. The first pane is asked to create a skill with that guide, including its
   zero-context subagent test.
3. The new skill must appear valid in `skills.list`.
4. A second pane must list it as `$name` and follow it when asked for the task
   alone.

The report records the skill's description, how many subagent rows the first
pane showed, and both panes' replies.

A second opt-in check proves agent-to-agent messaging between real panes:

```sh
SOYEHT_REAL_ACP=1 uv run python scripts/e2e_real_agent_messaging.py \
  --report .cache/e2e-real-agent-messaging.json
```

It opens a Claude Code pane and a Codex pane from the pinned registry adapters,
installed and started the same way as above, in the disposable service
environment (not the login shell's `PATH`, which may list protected folders). Claude finds Codex
with `list_agents` and pings it with `message_agent`; Codex replies the same way.
Each side must receive the other's message as a separate turn attributed by name.
It then holds a draft in the Codex pane, has Claude send again, proves the message
waits, sends the draft and requires the typed text first, intact, and the agent
message after it on its own. `--pair SENDER RECIPIENT` chooses other harnesses and
`--model codex=MODEL` selects a model the account accepts through the pinned
adapter. A harness that is not signed in fails the run with its name.

The same opt-in check for every system is `scripts/e2e_real_harnesses.py`: a real reply
from each signed-in harness, one pane switched Claude to Codex and back (it answers on
each side and the transcript shows both handovers), and Claude and Codex messaging each
other. It skips a harness that is not signed in by name instead of failing, and reports
the model each harness used. It runs on macOS, Linux and Windows; see "Agents messaging
each other" in `platforms.md`.

Permission expiry is accelerated by changing only the pending test request's
timestamp while the disposable engine is stopped. No shorter production timeout
or clock override is added. For the engine-outage case, only its uniquely named
job is briefly disabled, killed and removed; the override is re-enabled before
the replacement job is loaded. The supervisor ledger proves that a native
permission exists before the engine returns, and the database proves the engine
had not already received it.

Native load replaces its bounded history after confirmation. The regression
compares the ordered user and assistant messages before and after supervisor
loss, including a cancelled turn with no response text. An empty native replay
chunk must preserve the role boundary without adding an empty visible message.
