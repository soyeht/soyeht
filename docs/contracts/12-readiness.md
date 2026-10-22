# Contract 12 · Readiness checklist

Status: v1. Clients: Mac (Swift), Linux and Windows (Rust desktop). The iPhone and iPad can
show the same list from the same method.

A person who installs Soyeht must be told what is missing on the machine that runs the engine
and how to fix it, in the words of that machine. The engine answers; the clients only draw.

## 1. The method

`engine.readiness {}` → `{system, items}`, no mutation.

- `system` is `macos | debian | fedora | windows | other`. `debian` and `fedora` are the Linux
  families, told apart by `ID` and `ID_LIKE` of `/etc/os-release`; any other Linux is `other`
  and gets no install command.
- `items` is the list, in display order. Each item is `{id, name, kind, required, found,
  hint?}`.
  - `kind` is `tool` (a program the engine runs) or `harness` (the CLI of a harness of the
    registry).
  - `required` is true for a tool without which part of the engine does not work. A harness is
    never required alone: the checklist is complete when any one of them is found.
  - `hint` is the command that installs the item on `system`. It is present only while
    `found` is false and the table has a command for that system.

The list and the commands are `x-readiness` in `contracts/protocol.json`
(`uv run python contracts/generate.py` writes the engine's table). A client keeps no command.

## 2. What the engine checks

| id | Looked up in | Why |
|---|---|---|
| `git` | the engine's `PATH`; on macOS the Command Line Tools behind the launcher | source control panes and task worktrees |
| `node` | `npx` on the login shell's `PATH` | the harness adapters start through `npx` |
| `wt` | the engine's `PATH` | a worktree for each task (ADR 0029) |
| `lsof` | the engine's `PATH` and the standard places; not listed on Windows | the list of local servers |
| harnesses of the table | the same lookup `harness.list` uses for `installed` | an agent to talk to |

Nothing is started to find out: a program is looked up, not run. The lookups run off the Core
lock (ADR 0079).

## 3. What a client does with it

The client adds the one row the engine cannot answer, "the engine is reachable", from its own
connection, and shows the rest as the engine sent it. The rules, passed by both clients from
`contracts/vectors/readiness.json`:

- Without a connection the list is that one missing row and nothing else.
- A row is `ready` when found and `missing` otherwise. A missing required tool is a blocker; a
  missing optional tool is a suggestion.
- The checklist is `complete` when the engine is reachable, no required tool is missing and at
  least one harness is found. Without a found harness it says to install one.
- `Check again` asks the engine again; nothing is cached.
