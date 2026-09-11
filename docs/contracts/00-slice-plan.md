# Slice plan · from zero to the product in your hands

Each slice is vertical (crossing engine, contract and at least one client),
has automated acceptances and closes only when they are green. The order is
intended to prove the foundation first and leave the state experiment until last.
Date: 2026-09-10.

| # | Slice | Included scope | Contracts | What it proves |
|---|---|---|---|---|
| 1 | **Foundation** | engine + ptyd under launchd (Dev profile), protocol schema with generated types, database, screen segments, one shell pane, CLI, Mac app displaying it, iPhone app pairing and reading | 06, 07, 08 | separate process, single protocol, reconnection, persistence, updates without killing processes |
| 2 | **Agent over ACP** | agent pane with our chat: open ACP v1 harness, prompt, queue, permission (Mac and iPhone respond), cancel, login; harness under ptyd survives engine restart; harness · model · effort selector; change model/effort/permission mid-conversation; yolo button; probing by version | 04, 02, 05 (ACP part), 07 | the product's core works and survives updates |
| 3 | **Tasks** | workspace list, board and list views, create (dialog and MCP), dispatch, `task_next` with reservation and return, subitems, comments, state phrase, “your turn/system's turn” badges, typed blocking, per-pane loop, lead agent, CI feedback to the owning pane, attention inbox, worktree per task (worktrunk) | 03, 08 | workflow with multiple agents |
| 4 | **Editor and app bar** | native editor pane (buffer, highlighting, save and history, palette and search, LSP, diff, agent marks and review, Markdown, Vim); left rail with view switching; subscription usage rings with the three readers | 09, 10 | the code pane is fast and the rail reaches every view |
| 5 | **Browser** | human-mode spike first (trusted system events into the web view, Safari identity, user activation); web pane with native web view, local servers in the new pane, link routing, pick element to the agent composer (source file:line for React, Vue and Svelte), MCP `web_*` tools with snapshot and actions, console and network, session permission and incognito; human mode with pane-owned cadence and hand-back on CAPTCHA or login | 11 | the agent and the user share one real browser, and third-party sites see a person |
| 6 | **Harness switching** | continuation prompt from the single file, 6 messages, transcript location, switch and return, frozen queue, failure with restoration | 01 | real multi-harness support |
| 7 | **Raw terminal and the experiment** | frame recording + hook labels + export; detector interface; current app's regex as baseline; “no signal” and “unknown” states | 05 | the detector can be measured and chosen using data |
| 8 | **Complete iPhone** | task board, dispatch, yolo, change model, push (APNs through the engine), Live Activity, reconnection on every screen | 06, 03 | supervision away from home |
| 9 | **Linux and Windows** | TypeScript app with the same protocol client, terminal and agent pane; named pipe on Windows | 08 | the product beyond Mac |
| 10 | **Production** | signing and notarization, TestFlight, engine updater that does not kill ptyd, migration from the current app to the new one (workspaces, paired devices), production profile, changelog | 07 | replacing today's app with the new one |

After slice 10 come the open proposals you wanted to discuss later:
shared workspace context, an event-driven lead agent, agent review,
PR contract, WikiSkill.

Size rule: each slice is a self-contained objective such as
`00-slice-1-objective.md`, executed to completion by Codex in goal mode,
reviewed by me against the contracts, and approved by you by examining the
product in the Dev profile.
