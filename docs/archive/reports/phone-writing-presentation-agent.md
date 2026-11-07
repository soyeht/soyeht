# iPhone presentation, explicit companion, and accessibility

Date: 2026-09-20

Base tree: `df9f7f9` (block D3, PR #373)

Scope: block D4. This completes the approved phone writing surface with a 16:9
presentation, the Agent tab for an explicit writing companion, accessibility
facts, and device-local navigation that does not move shared engine focus.

## Presentation

The phone consumes `WritingLayoutPolicy` with the same measured floors and
metrics as the Mac surface. The policy selects the numeric strip at compact
widths and thumbnails only after their measured threshold fits. The stage
rectangle is always 16:9 and aspect-fits without deformation. Selecting a slide
loads its existing editor viewport; it does not create another writer or change
pane focus. Images use `writing.asset.read` for the active slide.

## Explicit companion

Only a valid `PaneLayoutCompanion` produces the two 44 pt Document and Agent
tabs. Both remain visible while the leaf is collapsed. The Agent tab loads the
existing transcript without selecting the pane and mounts
`AgentConversationView` only while that local tab is open. Sending a message is
the explicit remote action: immediately before the existing prompt path, it may
select that agent in the engine.

## Local selection and accessibility

Entering a phone writing workspace, toggling a leaf, choosing either local tab,
restoring state, rotating, or relaunching never calls `EngineClient.select`.
`PhoneWritingFocusPolicy` routes only `sendToAgent` to that callback; its test
counts zero selections for all local actions and exactly one for the remote
action.

Headers expose the full document name, kind, expanded state, link state, and
stable pane identifier. Companion tabs expose selected state. Slide numbers,
thumbnails, stage, text editor, and screenplay editor have stable identifiers
and descriptive labels; visual truncation never changes the accessible title.

## Automated evidence

- Focused phone-writing policy and state tests include focus routing and the
  presentation matrix at 375, 390, 430, and landscape 812 pt.
- `scripts/check_ios.sh` builds both simulator architectures.
- `scripts/check_mac.sh` protects the shared-client callers.
- A signed physical build is produced for the dedicated iPhone Devs.

Physical VoiceOver, keyboard, rotation, background, relaunch, presentation,
companion, and local-focus evidence is added when that dedicated phone is
available. The iPhone Caio is not a substitute.
