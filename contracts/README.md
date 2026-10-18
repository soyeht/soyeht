# Executable protocol

`protocol.json` is the single source of formats. `x-methods` binds each method
to its parameters and result. `x-events` binds each notification to its parameters
and declares whether it has a durable cursor. `x-limits` defines shared limits;
`x-framing` describes binary frame integers.

Methods whose parameters declare `paneId` use it as their operation-log route.
`x-paneRoute` names a differently spelled singular pane identifier.
`x-paneNonRouteIds` lists every other parameter name whose type is `Id` or an
array of `Id`. Generation fails when an identifier name is neither the route
nor explicitly classified, and also fails when a listed name becomes unused.
This exhaustiveness covers only `Id`-typed parameters: a future pane route
represented as a plain string under another name must be declared separately.

`x-settings` defines each setting the engine stores and the clients read and change through
`settings.get` and `settings.set`: its key, type, default and bounds. A key ending in `.<name>`
is a family whose parameter (a harness ID) is bounded by `parameter`. The generator emits
`engine/soyeht-protocol/src/setting.rs` (`setting::*` key constants, `default_value`, `validate`
and the family key builders), `contracts/swift/Sources/SoyehtProtocol/Setting.swift` (`Setting`)
and `contracts/typescript/src/settings.ts`. The engine validates with the generated code, and
`check_wire_names.py` fails on a setting key typed by hand in production code. The link
preferences of the Mac (open external links in Soyeht) are not engine settings: they are kept per
device, because the system browser they choose between is the device's own.

`x-clientPolicy` holds what the clients agree on without sending it over the wire: the request,
greeting, reconnect (first wait and ceiling) and draft-beat timings, the page size of each list
request (`maximum` takes the request's own limit), the terminal input chunk, the usage and context
band thresholds, the ring opacities, the weekly-window keyword, the provider names, the texts two
clients both store (the harness switch wording and the reason a card moved on the task board), the
image types an agent takes inline, what each pane state means (whether a turn runs in it and the
palette role of its dot) and the category rule of the model, effort and permission selectors. The
generator emits `engine/soyeht-protocol/src/client_policy.rs` (with `PaneState::as_str`,
`is_turn_running` and `dot`), `contracts/swift/Sources/SoyehtProtocol/ClientPolicy.swift` and
`contracts/typescript/src/clientPolicy.ts`. `vectors/usage.json` holds the cases of the usage
presentation and the selector roles, `vectors/pane-state-presentation.json` the dot, running flag, terminal exit notice
and status word of every pane state, `vectors/reconnect.json` the waits between reconnects and
`vectors/permission-preview.json` the lines a file change shows before it is approved and how many
lines a text takes, and `vectors/editor.json` when the visible lines need another page of a file,
which editor tab shows and which takes over, what a keystroke changes and sends, and how the
project tree answers the keys; the Mac and the desktop client both pass them. The editor's
constants (`editor`: the read-ahead, the tab width, the bounded tab history, the save wait, the
indent), the pairs it closes (`editorPairs`) and the palette role of each syntax highlight
(`highlightRoles`) are generated with the rest, and the editor's metrics (`editor` in
`design-tokens.json`) are tokens like any other.

`design-tokens.json` is the single source for the design tokens: the nine
palettes (colors per role, ANSI slots, agent plates), radii, spacing, type scale,
elevations, layout metrics, composer metrics and font families. The generator emits
`apps/shared/Sources/SoyehtClient/Generated.swift` (`TerminalColorTheme`, the palettes and `DesignTokens`)
and `apps/desktop/crates/soyeht-desktop/src/theme/generated.rs` (`ThemeName`, the palettes and
`f32` constants per section). Names are the JSON keys: `radius.compact` is
`DesignTokens.Radius.compact` in Swift and `theme::radius::COMPACT` in Rust. The
Mac values are pinned to the pre-generation ones by `test_design_tokens.py`.

`profiles.json` is the single source for installed profile values. It defines
the Dev data directory, socket names, TCP port and launchd labels. The same
generator emits constants for Rust, Swift and Python, plus the launchd service
templates. Runtime code and installation scripts consume those generated values.

The format is JSON Schema 2020-12. The generator accepts local references in
`$defs`, objects with required and optional fields, arrays, strings, numbers,
integers, booleans, null, enum and oneOf. It accepts numeric and length limits,
pattern and contentEncoding declared in the schema.
Any unknown keyword in a type stops generation.

Readers ignore unknown fields, as required by contract 08. Enums remain closed.
Generated models represent types; state rules, identity, authorization and transport
limits belong to the engine. The TypeScript reader validates parameters against
the schema before returning types. Serde and Codable check types during deserialization.
Frame size limits are checked by all three binary codecs.

Every method and event name has a generated form so that no client types one by hand:
`RequestMethod` and `EventName` enums in Swift, `method::*` and `event::*` string constants
(which also work in `match` patterns) and the `EventName` enum in Rust. The frame kinds
(`x-framing.types`) and the frame bounds come from the same file: `FrameKind` and
`ProtocolLimits` in Swift, `limits::FRAME_KIND_*` and `limits::*` in Rust.
`scripts/check_wire_names.py` fails on a string literal equal to a name in production code.

Requests and notifications use the JSON-RPC 2.0 envelope. `welcome` is the result
of `hello`. Challenges, snapshot parts, stream events, local-server changes,
web console counts and web driving state are notifications without a state
cursor; durable events carry `params.eventId`. 64-bit integers travel as decimal
strings; in binary frames they are big-endian u64.

To change a format, edit the schema and corresponding vectors:

```sh
uv run python contracts/generate.py
```

The generator does not use the network. It produces Rust, Swift and TypeScript
models, profile constants, limits, service templates and test dispatchers.
Generated files are included in the same commit as their source. `--check`
writes nothing and fails if they differ.

`vectors/messages.json` contains type values and full envelopes for every method
and event. The same values pass through Rust and Swift types and the TypeScript
reader. `vectors/frames.json` fixes reference bytes in both directions. Negative
controls check truncation, excessive size, invalid frame kinds, incorrect JSON
types and unknown enums.

`vectors/markdown.json` is the specification of how agent replies are drawn (ADR 0087): each
vector is a Markdown input and the blocks it must become, optionally as a reply that is still
arriving (`"streaming": true`). The Swift client (`MarkdownTests`) and the desktop client
(`tests/markdown_vectors.rs`) both pass the whole file. `vectors/web-links.json` fixes which
bare web addresses in a reply become links (ADR 0087): a text and the addresses found in it,
passed by `WebLinkRoutingTests` and `tests/web_link_vectors.rs`.

`vectors/agent-cards.json`, `attachments.json`, `loop-panel.json` and `tasks.json` fix what
the clients compute for the person rather than what travels: the wording of the transcript's
cards, how an attachment is prepared and sized, how the loop panel reads a pane's loop, and how
a task is grouped, labelled and dated. The Swift tests in `apps/shared` and the Rust tests in
`apps/desktop` run the same file, so the Mac and the desktop cannot drift apart.

Rules that the harness switch, Preferences and the rail compute for the person are vectors too:
`vectors/harness-picker.json` (how the harness pickers group, order, name and preselect
harnesses, and what the chip and the switch button say), `vectors/usage-presentation.json` (the
window a usage ring follows, its band and percentage, the sentences for a missing reading, the
spans of time) and `vectors/agent-account.json` (the account section of a signed-in agent pane:
switch account and sign out). The Mac passes them in `SharedVectorsTests.swift`, the desktop client
in the tests of `harness_picker`, `usage` and `account` (`apps/desktop/crates/soyeht-client`).

`vectors/pane-layout.json` fixes the rules the clients apply to a workspace's pane tree: how
a stored layout is read and which panes it keeps (`PaneLayoutState`, `layout::stored_tree`),
merging and collapsing splits, splitting a pane, the width a tree needs, where each pane
sits at that width, and where a pane goes that the tree does not list (`PaneLayoutArrangement`,
`layout::arrange`): primary panes (editor, web, writing) left to right, auxiliary panes
(terminal, agent) stacked on the right, and in the automatic mode every branch weighed by the
smallest size its panes need. Each case gives the floors to weigh with; the Mac measures one
per kind, the desktop passes one for all.
