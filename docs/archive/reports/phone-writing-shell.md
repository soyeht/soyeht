# iPhone writing shell and text surface

Date: 2026-09-20

Base tree: `b3b1b8b` (`main` after block D1)

Scope: block D2. This change renders the D1 policy as the iPhone writing list,
persists only device-local expand/collapse state, and adds the editable text
surface. Screenplay, presentation, and companion Agent content remain D3/D4.

## State and identity

The view has one root `GeometryReader`. It passes that viewport to
`PhoneWritingLayoutPolicy` and draws only the returned rectangles. Every leaf
is keyed by `(paneId, fileId)`, while the persisted state is namespaced by
`engineId + workspaceId`. Unknown or malformed state versions are deleted;
closed documents are pruned by the existing D1 reconciliation.

The header title always comes from the live `Pane.name`. While a document is
still loading, the policy receives the same live pane-name map and never uses a
pane UUID as copy. Creating a text document explicitly replaces any transient
identity for that pane and opens the new leaf without changing sibling state.

Collapsing a leaf first drains the shared editor write queue. It then unmounts
the heavy editor while the engine-backed replica remains in `EngineClient`.
Expansion selects the pane and reloads its bounded viewport.

## Text surface

The phone surface reuses `WritingLayoutPolicy`, the active document font,
`fullWidth`, the shared `EditorEditing` UTF-16 edit planner, and
`EngineClient.enqueueEditorEdits`. It does not create a second writer. The
toolbar exposes the three existing document fonts and the per-document reading
width/full-width preference. Conflict and pending-review facts remain visible.

Newsreader, Nunito, and Courier Prime are copied from the existing Mac font
resource directory and registered in the iOS app. The built app was inspected
to confirm that every declared `UIAppFonts` path exists under `Fonts/`.

## Accessibility and pressure

Headers keep the D1 `44` pt minimum target, expose the complete title and kind
to VoiceOver, name expanded/collapsed state, and identify an explicit companion
link. Visual title truncation does not change the accessibility label. The
outer list scrolls vertically; the policy still creates no horizontal overflow.
The text editor owns its internal document scrolling and uses interactive
keyboard dismissal.

## Automated evidence

The focused `PhoneWriting` run contains 17 passing tests. In addition to D1,
it covers the versioned store namespace, restoration, invalid-state pruning,
workspace-specific removal, live pane names during document loading, and the
explicit open-after-create transition.

Validation commands, run without output pipes:

```sh
swift test --package-path apps/shared --filter PhoneWriting
bash scripts/check_ios.sh
cargo build --locked --workspace  # from engine/, supplies integration fixtures
bash scripts/check_mac.sh
```

The simulator app compiled for arm64 and x86_64 with warnings treated as
errors. The complete Mac gate passed after its Rust integration fixtures were
built. A generic physical-device build also completed with the development
profile for `com.soyeht.next.dev.ios`. Installation and touch/keyboard checks
are recorded in the PR once the dedicated iPhone is connected; no other phone
is used as a substitute.
