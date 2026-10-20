# How to add a pane type

A pane type is a value of `PaneMode` (`terminal`, `agent`, `editor`, `web`, `writing`).
The engine owns what a pane holds and every rule about it; each client only draws it. The
steps below follow how `web` (a browser pane) and `writing` (text, presentation and
screenplay documents) were added, in the order the layers of `AGENTS.md` require: contract,
protocol, domain crate, engine, shared client, views. Read `docs/contracts/11-browser.md`
and ADR 0055 and ADR 0061 first: they are the worked examples of the decisions you will
make. Paths are relative to the repository root.

Decide before you write code:

- Does the pane run a process (a terminal or an agent), or is it a view over state the
  engine owns (a document, a page)? Only the second kind is covered here. A process pane
  also needs the supervisor and `soyeht-pane-state`; start from the terminal and agent
  paths in `engine/soyeht-engine/src/panes.rs` instead.
- Is it a new `PaneMode` at all? A writing kind (`text`, `presentation`, `screenplay`) is a
  closed field of the `writing` mode, and ADR 0061 explains why a beat board is a view of a
  screenplay and not a mode. Add a mode only when the pane has its own state and methods.
- Which rules do the Mac, iPhone and desktop clients compute for the person without a
  round trip (wording, grouping, sizes)? Those go in `contracts/vectors/`, not in a view.

## 1. Write the contract

1. Add `docs/contracts/NN-<name>.md` (next free number): what the pane promises, the methods
   and events, who may call them (`human_local`, `human_device`, `agent`, `chief`), what is
   persisted, and what is outside the first version. Add an ADR in `docs/adr/` when you
   change a boundary (a new crate, a new owner of a file, a new permission kind).
2. Name wire methods and events in backticks in the document. `scripts/check_doc_names.py`
   fails when a document names one that does not exist in `contracts/protocol.json`, so write
   the schema (next step) and the document together.

## 2. Change the protocol and regenerate

In `contracts/protocol.json`:

1. Add the value to the `PaneMode` enum, and a matching kind to `WorkspacePaneLayoutKind`
   (the pane tree stores a kind per leaf).
2. Add the parameters and results of your methods to `$defs`, and the methods to `x-methods`
   (`"mutation": true` for anything that changes state, with an `idempotencyKey` parameter;
   a parameter named `paneId` is the operation-log route, `contracts/README.md`). Add notifications to
   `x-events`; an event that a client must not miss while disconnected is durable and
   carries `eventId`.
3. Add a limit to `x-limits` for every size or count you accept. Do not type a number in
   Rust or Swift.
4. Rules that all clients must share but that do not travel on the wire go in
   `x-clientPolicy`, or as a vector file in `contracts/vectors/`.
5. Add vectors to `contracts/vectors/messages.json`: a request, a response and an event for
   each new method and event. The Rust, Swift and TypeScript tests all read it.
6. Run `uv run python contracts/generate.py`, and commit the generated files with the source
   (`engine/soyeht-protocol/src/generated.rs`, `contracts/swift/...`,
   `contracts/typescript/src/...`, `apps/shared/Sources/SoyehtClient/Generated.swift`).
   Use `method::*`, `event::*` and `limits::*` in Rust, and `RequestMethod`, `EventName` and
   `ProtocolLimits` in Swift. `scripts/check_wire_names.py` fails on a hand-typed name.

Adding a variant to a generated enum makes every exhaustive `match` on `PaneMode` fail to
compile. That list is your to-do list for the next steps.

## 3. Put the rules in a pure crate

If the pane has rules that can be tested without the engine (parsing, normalization,
bounded projections), put them in a domain crate beside `engine/soyeht-web` and
`engine/soyeht-writing`:

1. Create `engine/soyeht-<name>/`, add it to the `members` of `engine/Cargo.toml`.
2. It may depend only on `soyeht-protocol` and `soyeht-platform` (and `soyeht-process` when
   it runs a command), has no IO and no async runtime, and uses no `unwrap`, `expect` or
   `panic!` in production code.
3. Place it in `BLOCKS` of `scripts/check_layers.py` and in the layer list of `AGENTS.md`;
   `uv run python scripts/check_layers.py` fails otherwise.

## 4. Persist it

Only `engine/soyeht-store` has SQL (`scripts/hardening_gate.py check-no-sql-outside-store`).

1. Add a migration `engine/soyeht-store/migrations/NNN-<name>.sql` (the next number) and
   apply it in `engine/soyeht-store/src/database.rs`, as the recent ones are.
2. Add a module (`web.rs` and `writing.rs` are the models), its methods on the store, and
   the matching methods on `StorePort` in `engine/soyeht-engine/src/ports.rs`.
3. Add tests in `engine/soyeht-store/tests/<name>.rs`, and one beside the migration tests of
   `engine/soyeht-store/tests/storage.rs` showing that an existing database upgrades.
4. A document-backed pane reuses the owned-file path (opening, saving, conflicts, history)
   and does not introduce a second file writer (ADR 0046, ADR 0047, ADR 0061).

## 5. Add it to the engine

Everything under `engine/soyeht-engine/src/`; the Core lock rules of `AGENTS.md` apply
(copy the inputs under the lock, run blocking work through `Outcome::Offload` or
`Core::spawn_background`, apply the result under the lock).

1. Add a module `<name>/` with a `handles(method)` function, as `web/` and `writing/` have,
   and declare it in `lib.rs`.
2. Route the methods in `dispatch.rs`, in the `match` of the request dispatcher.
3. Create the pane: `pane_create` in `panes.rs` branches on `params["mode"]` before the
   terminal and agent path, and calls your creator. Persist the pane and emit
   `model::pane_created` through the same transaction (`persist_new_pane`,
   `pane_created`), as `web/panes.rs` does.
4. Say that the pane has no process: add the mode to the lists that skip the supervisor in
   `finish_close` (`panes.rs`) and `reconcile_pane` (`core.rs`), and map it in
   `workspace_layout.rs` to its layout kind. Experiment capture and recording stay refused
   for any mode that is not a terminal or an agent.
5. Check the caller's role and workspace in every method (`authorize_web_workspace` in
   `web/permissions.rs` is the model). Anything that grants a capability a person must
   consent to is a permission kind that no agent permission mode answers, and a
   consent-changing method stays refused to a client that is not the signed local UI
   (`docs/platforms.md`, "What differs").
6. If clients need the pane in their first state, add a snapshot section (see
   `writing_snapshot` and the sections pushed in `dispatch.rs`).
7. Tests: unit tests next to the code; an integration test in
   `engine/soyeht-engine/tests/` (`writing_new_document.rs` is the model); and, when the pane
   has a user-visible flow, an end-to-end script like `scripts/e2e_web.py` or
   `scripts/e2e_writing.py` and a line for it in `scripts/check_all.sh` (a gate step, which
   makes the change a gate change: it needs the full tier and a review). The engine's
   contract checks (ADR 0090) also validate your events and results against the schema when
   the tests run with `--features soyeht-engine/contract-checks`.
8. If the CLI should drive it, add the command to `engine/soyeht-cli/src/commands.rs`
   (`USAGE` and the dispatch).

## 6. Make the shared client understand it

Logic goes in `apps/shared` so `swift test --package-path apps/shared` covers it; the Mac
and iPhone views only render and forward input (`AGENTS.md`, layers 5 and 6).

1. Add the replica that applies your events and snapshot section (`WebReplica.swift`,
   `EditorReplica.swift`), and route the events in `EngineClient.swift` and
   `PaneCreatedReplica.swift`.
2. Handle the new case in the exhaustive switches the compiler now reports:
   `PaneLayoutTree.swift` (kind and role, primary or auxiliary), `PaneLayoutPersistence.swift`,
   `PaneLayoutFloors.swift` (the smallest size the pane needs), `PaneClosePolicy.swift`
   (what closing asks), `PaneDeparture.swift` (what a client forgets and where the selection
   goes when a pane leaves), `WorkspaceNavigation.swift`.
3. Presentation rules that the desktop client must reproduce go in a vector under
   `contracts/vectors/` first, then in both implementations. `pane-layout.json` already holds
   the layout cases; add a case that uses your kind with its floors.
4. Test with the vectors and with fakes. Connection tests start the real engine from
   `engine/target`, so build the engine first.

## 7. Draw it

Colors, radii, spacing, type sizes and shadows come from `contracts/design-tokens.json`
(`DesignTokens` in Swift, `theme::*` in Rust). A pane card is the only raised surface; the
interior is flat. `scripts/check_mac_view_colors.py`, `check_pane_interior.py` and
`check_mac_text_fonts.py` enforce this.

- **Mac.** `apps/mac/Sources/PaneCard.swift` chooses the body of a pane by mode; add your
  view there. Add it to the rail and the new-pane entries (`AppRail.swift`,
  `WorkspaceWindow.swift`) if people open it from there.
- **iPhone.** `PhonePaneDetail.swift`, `PhoneNewPaneSheet.swift`, `PhoneWorkspaceSplit.swift`.
- **Desktop (Linux and Windows).** `apps/desktop/crates/soyeht-client` holds state and
  layout (`state/`, `layout.rs`, `commands/`); `apps/desktop/crates/soyeht-desktop` holds the
  views (`views/pane.rs` picks the body by mode, `destination.rs` lists the rail's places).
  A mode the desktop does not draw yet shows the notice from `mode_notice` in `views/pane.rs`,
  which is the honest first step: ship the engine and the Mac, then the desktop view, and
  say so in the contract. Logic that both clients share is a vector both pass.

## 8. Check it

```sh
uv run python contracts/generate.py --check
uv run python scripts/check_doc_names.py
uv run python scripts/check_layers.py
uv run python scripts/check_quality.py
(cd engine && cargo test --locked --workspace --features soyeht-engine/contract-checks)
(cd apps/desktop && cargo test --locked --workspace)
swift test --package-path apps/shared        # macOS
SOYEHT_SKIP_FORBIDDEN_TERMS=1 bash scripts/check_all.sh --tier fast
```

A change to the engine core, the protocol or `soyeht-store` needs a second reviewer
(`CONTRIBUTING.md`, "Review and merge"). The pull request template lists the rest.
