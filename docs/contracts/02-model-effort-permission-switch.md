# Contract 02 · Change model, effort or permission mid-conversation (same harness)

Status: SETTLED on 2026-09-10 (Caio's decisions at the end).
Source: (internal analysis, outside the repository), section 4 of each app, ACP v1/v2 protocol and the Claude Code ACP adapter. Date: 2026-09-10.

## What the contract promises

Changing model, effort or permission within the same harness **continues in the
same session**. No continuation package, no new session. The conversation continues.

Applies to ACP panes. Raw terminal mode has no mid-conversation switching:
the value is passed at launch through a flag.

## How it works in ACP (evidence)

1. When the engine opens a session (`session/new`), the agent returns a list
   of “configuration options”, each with an ID, name, current value and
   possible values (`SessionConfigOption`, v1 schema `agent.rs:2328`).
2. To change it, the engine sends `session/set_config_option` with the option
   ID and value (`SetSessionConfigOptionRequest`, `agent.rs:2524`). The agent
   responds with the updated list. That is the confirmation.
3. The Claude Code ACP adapter exposes exactly these options: `mode`
   (permission), `model`, `effort`, `fast` (official Claude Code ACP adapter).
   Applies them live, without restarting.
4. Protocol v1 also has `session/set_mode` for permission; in v2 this became
   an ordinary configuration option (only `session/set_config_option`).
   The engine speaks both: always `set_config_option`; `set_mode` only if
   the v1 agent does not list `mode` among its options.

## Engine rules

- **Model and effort lists: first what the harness advertises.** If the harness
  reports its models and efforts (ACP configuration options or a harness listing
  command), the selector shows them. Only if the harness advertises nothing
  does the selector show our manually maintained per-harness list.
- **If the harness does not support changing mid-conversation**, the engine
  opens a new session automatically with the new value and sends the harness-switch
  continuation package (6 whole messages + file location + notice). The user sees
  a “model changed, conversation restarted” marker in the pane.
- **Show the new value only after agent confirmation** (`set_config_option`
  response). While waiting, the control shows “applying”. On error, it returns
  to the old value and displays the error.
- **The pane stores harness + model + effort + permission together.** Never
  a session ID without its harness (a mistake found in a reference app).
- **Task with harness/model/effort:** when opening the task's pane, the engine
  applies all three immediately after `session/new`, through the same options.
- **If the agent changes the session ID** during application (some adapters
  restart the process internally), the engine treats it as context loss and
  sends the contract 01 package (6 messages + location) in the new session.

## What reference apps do (summary, evidence in internal analysis)

- Six structured apps switch models mid-conversation; none switches harnesses.
- Where the harness supports live changes (Claude SDK, Codex per turn, Pi,
  OpenCode, ACP), they apply without restarting.
- Where it does not (some process-per-turn adapters), they restart and resume
  by cursor; one feeds the transcript back as text when native state is lost.
- One app enables the selector only when support is advertised and marks success
  only on agent confirmation, not on sending.
- Terminal apps type `/model` and `/effort` into the TUI; that does not serve
  Soyeht over ACP.

## Client events

- `pane.config_changing` {pane, option, value}
- `pane.config_changed` {pane, option, value, updated options}
- `pane.config_failed` {pane, option, value, reason}

## Caio's decisions (2026-09-10)

1. Model list: **C**, advertised by the harness; a manual list only for harnesses
   that do not advertise one. “If the harness itself says which models and efforts
   it has, even better.”
2. Harness that cannot switch mid-conversation: **B**, the engine opens a new
   session and sends the 6-message package; “it works more or less the same way”.

## How reference apps build model lists (evidence in internal analysis)

- Claude Code: through the SDK (`supportedModels()`) in one app; by running
  the CLI and reading output in another; a fixed list in two.
- Codex: through app-server (`model/list`, which also returns supported efforts
  and the default effort).
- ACP agents: through session configuration options (`model` category) or
  `availableModels`; one app starts the agent for 2 to 5 seconds just to query
  the catalog, then closes it.
- Grok: `grok models` command. Cline: reading the configuration file.
- Gemini, Amp, Muse: a manually maintained fixed list.
In other words, everyone effectively uses C already.

## Review applied (2026-09-10, findings R01, R15, R16, R25)
Canonical events: `pane.config_changing {configId, value}`, `pane.config_changed {configId, value, options, sessionPreserved}`, `pane.config_failed {configId, value, reason}`. Yolo and “accept edits” follow contract 04 R15. A parameter error or application timeout does NOT restart the session (04 R25); only proven `não_suportado` triggers a new session with a package (Caio's decision B).
