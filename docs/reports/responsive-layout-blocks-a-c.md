# Responsive layout blocks A-C

Date: 2026-09-20

Final Mac tree: `a2d349c540191496166616e09cae82459d5aebfa`

Product: rewrite, bundle `com.soyeht.next.dev.mac`

This is the short index for the accepted Mac responsive-layout work. The linked
reports remain the measurement and investigation records from the stage when
they were written; this page records the final integrated state.

## Block A - pane layout and manipulation

**Complete.** A versioned canonical pane tree is persisted per workspace. The
Mac renders every visible leaf, keeps terminal identity and scrollback stable,
reserves halo clearance, derives floors from shared metrics, and scrolls a
branch instead of squeezing it below its floor. Header drag supports four-edge
insertion and center swap; dividers resize, clamp at measured floors, and
equalize on double-click. The new-pane chooser is a provisional tree leaf.

Evidence:

- [Pane layout floor measurements](pane-layout-floor-measurements.md)
- [Multi-terminal client cost](multi-terminal-cost.md)
- [New-pane chooser floor](new-pane-chooser-floor.md)

The comparative study of other apps remained outside the repository and
informed the accepted rules.

Delivery: [#338](https://github.com/soyeht/soyeht/pull/338),
[#340](https://github.com/soyeht/soyeht/pull/340),
[#341](https://github.com/soyeht/soyeht/pull/341),
[#344](https://github.com/soyeht/soyeht/pull/344),
[#353](https://github.com/soyeht/soyeht/pull/353),
[#354](https://github.com/soyeht/soyeht/pull/354),
[#355](https://github.com/soyeht/soyeht/pull/355),
[#356](https://github.com/soyeht/soyeht/pull/356), and
[#358](https://github.com/soyeht/soyeht/pull/358).

## Block B - workspace-tab overflow

**Complete.** Workspace order is durable through `created_seq`. The strip keeps
the active workspace visible, reports every hidden tab through exact `+N`
accounting, exposes hidden workspaces in a non-modal list, and uses a fading
sticky copy without a hard backdrop. Closing the active tab selects the next
canonical neighbor, then the previous one.

Block B changes strip policy rather than pane floors, so it has no separate
measurement report. Its shared sizing baseline is the
[pane floor report](pane-layout-floor-measurements.md). Delivery is recorded in
[#369](https://github.com/soyeht/soyeht/pull/369) and
[#370](https://github.com/soyeht/soyeht/pull/370).

## Block C - Mac writing surfaces

**Complete.** Text uses a measured 66-character column with a persisted
per-document full-width option. Screenplays keep a fixed Courier page and scale
as a whole. Presentations preserve a 16:9 stage and compact the slide list to
numbers. Explicit writing-agent companions adapt side-by-side, below, then to a
visible tab strip without overriding user-resized tree weights. Live pane
creation hydrates writing replicas without reconnecting.

Evidence:

- [Writing layout policy measurements](writing-layout-policy.md)
- [Pane layout floor measurements](pane-layout-floor-measurements.md)
- [Native prose rendering spike](prose-rendering-spike.md)

Delivery: [#359](https://github.com/soyeht/soyeht/pull/359),
[#361](https://github.com/soyeht/soyeht/pull/361),
[#362](https://github.com/soyeht/soyeht/pull/362),
[#363](https://github.com/soyeht/soyeht/pull/363),
[#364](https://github.com/soyeht/soyeht/pull/364),
[#366](https://github.com/soyeht/soyeht/pull/366),
[#367](https://github.com/soyeht/soyeht/pull/367),
[#375](https://github.com/soyeht/soyeht/pull/375),
[#377](https://github.com/soyeht/soyeht/pull/377), and
[#378](https://github.com/soyeht/soyeht/pull/378).

## Boundary after A-C

The Mac queue is empty at this tree. The iPhone writing implementation is a
separate Block D and still requires physical-device acceptance. Formatting-bar
behavior and the Present sheet remain design decisions, not unfinished A-C
implementation.
