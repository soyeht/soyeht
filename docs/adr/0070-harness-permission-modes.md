# ADR 0070: each harness's own permission modes

Status: accepted
Date: 2026-09-30

## Context

The single Yolo toggle mapped a Soyeht policy (`ask`, `accept_edits`,
`allow_all`) onto whatever the harness offered. It recognized Claude's mode
names but not Codex's `read-only`, `agent` and `agent-full-access`, so a Codex
pane with Yolo off still ran in Codex's own auto-approval preset (issue 425).
The UI claimed a policy the harness was not applying. Zed and the Codex app show
the harness's own modes instead.

## Decision

A pane's permission mode is the harness's own mode selector. The engine reads
it from the configuration option with category `mode` and falls back to the
deprecated `modes {availableModes, currentModeId}` announcement. Choices carry
the adapter's `_meta.kind` as `kind` (`standard`, `plan`, `auto_review`,
`full_access`); adapters that predate the kind still get `full_access` on their
well-known bypass identifiers. Clients list the modes with the names the
harness announced and never map them to a Soyeht vocabulary.

Changing the mode is an ordinary `pane.config.set` on that option. The engine
sends `session/set_config_option`, or `session/set_mode` for the legacy
announcement, and shows the new value only after the harness confirms it. A
refusal leaves the confirmed mode in place. A `current_mode_update` or
`config_option_update` the agent sends on its own is shown as the current mode.

The engine keeps one value per pane: the mode requested at creation, then the
last one the harness confirmed, including changes the agent made on its own.
After a restart or session reload it re-applies that mode, before queued
prompts run, when the harness offers the same identifier. A harness switch
does the same; when the new harness does not offer it, the new harness's
preferred mode applies if offered. A mode the harness does not offer is never
guessed: the pane stays in the harness's own mode.

`harness.defaultMode.<harnessId>` is the preferred mode for new panes of that
harness, as the harness names it. Pane creation and task presets can name a
mode explicitly with `permissionMode`. Reviewer panes start in the first mode
the harness announces, which adapters list as their most restrictive.

The engine approves permission requests itself only for agents that announce
no modes at all. It then exposes its own `mode` selector (`ask`,
`accept_edits`, `allow_all`) and applies it at once, without a native request.
For agents with native modes the engine never answers on the person's behalf,
except for a pane-scoped `allow_always` grant. Elicitations are never answered
by any mode.

The `yolo` pane request, the stored policy/previous-policy pair and the Soyeht permission
enumeration are removed. This supersedes the Yolo parts of ADR 0016, ADR 0025
and contract 04 §4/R15.
