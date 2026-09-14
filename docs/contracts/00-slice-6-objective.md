# Objective · New Soyeht slice 6: the writing editor (text, presentation, screenplay)

Self-contained, for execution to completion without questions to Caio. Same
absolute safeguards as slices 1–5 (`00-slice-1-objective.md` §0/§5 and the
slice-5 header): no production, Dev profile `Soyeht.next/dev`, no secrets in
the repo, no tags/releases, no competitor brand names in product docs
(open formats — Markdown, Fountain — and integration tools the agent drives
via MCP are fine to name; apps we take inspiration from are not), English-only
repo, Apache-2.0, no AI attribution in commits/PRs. Autonomy rule: ambiguity
becomes an ADR and work continues; self-merge with the green local gate (four
ratchets + font/color gates + restricted-vocabulary scan); victoria reviews
each PR after merge for correctness, contract preservation, design fidelity and
safety. Do ALL git in your own slice-6 clone/worktree, NEVER in the main clone
`/Users/macstudio/Documents/soyeht` (it is design-only and holds the encrypted
`.pen`; a hard reset there destroys uncommitted design work). Date: 2026-09-14.

## 0. Governing design

There is no pre-closed prose contract; this objective is the scope. The design
is the source of truth, in `design/soyeht-desktop.pen` (read-only), exported as
PNGs at `/Users/macstudio/Documents/soyeht-design-exports/slice6/`:

- **Tela 14 `aiVQb` — Texto**: hybrid live-preview Markdown editor, focus mode,
  a click-to-format toolbar (font selector, heading level, bold/italic/list/
  quote/link/image) that writes valid Markdown underneath, and the agent
  rewrite/comment suggestion (accept/reject) + composer.
- **Tela 15 `VQ8hk` — Apresentação**: the same `.md` in present mode — slide
  thumbnails on the left, Markdown editor on the right; `---` (or three Returns)
  is a slide break; a heading is what shows on the slide; ordinary text is
  speaker notes; images drag/drop/paste; the app auto-lays-out the slide.
- **Tela 17 `e0Zlg` — left rail · Escrita**: three rail items (Textos /
  Apresentações / Roteiros), each opening its own section list.
- **Tela 18 `DFqGr` — Roteiro**: a screenplay editor — a scene navigator on the
  left (collapsible), a screenplay sheet in Courier with standard auto-formatted
  elements, and an element toolbar (element-type selector, Tab/Enter advance
  hint, focus). A line may carry agent-generated **variants** (alternate takes):
  a variant-bearing line shows a dotted underline, and on hover an inline control
  cycles them (‹ n/N › · + · −).
- **Tela 19 `Y7sdbI` — Quadro de ideias (Beat Board)**: a board of idea cards
  (categorised) each linkable to a scene, with a "+ nova ideia" affordance; the
  agent pane opens beside it like any other pane.

Consume the `.pen`; never change it. Texto, Apresentação and the rail are
approved; the Roteiro and Beat Board screens are recently reworked and may get
minor visual tweaks — build in the order in §1 so the later screens settle, and
re-check the exported PNG before starting each screen (ask victoria for a fresh
export if in doubt). The built UI ships in **English** and must be
translation-ready (i18n is a later slice); the `.pen` copy is Portuguese for
review only.

## Architecture and maintainability (decided first, before any feature)

The **first non-spike PR is a boundary ADR** (like ADR 0042 for the code editor
and ADR 0055 for the browser). It must state, and every later PR must hold:

1. **The editor surface is the client's; the engine never renders prose.** The
   engine owns the document/pane record and file I/O, and routes agent
   suggestions — it does not lay out Markdown, slides or screenplay. **Reuse the
   existing editor infrastructure** rather than reinvent it: file ownership
   (ADR 0046), save-conflict and history (ADR 0047), agent change-request
   delivery (ADR 0052) and the durable language/format model (ADR 0050). A prose
   document is another editor surface over the same owned local file.
2. **Bounded pure cores behind ports.** Anything that can be pure Rust is a
   crate depending only on `soyeht-protocol` + small libs, never on
   engine/store/ptyd:
   - a **document-structure core** that segments a Markdown document into slides
     (`---` / blank-run rule; heading = slide, body = notes) as bounded types;
   - a **screenplay core** that parses Fountain plain text into ordered elements
     (scene heading, action, character, parenthetical, dialogue, transition,
     shot) and the scene list for the navigator, including a bounded per-line
     **variant model** (ordered alternates + a selected index; the canonical
     Fountain keeps only the selected line, alternates in a bounded sidecar the
     agent also reads);
   - a **beat-board core**: idea cards and their scene links as bounded types;
   - a **section model**: three document sections over local files, bounded
     lists.
   One error enum per crate, mapped once to the wire.
3. **The Mac editor-surface layer is a bounded module**, colours and fonts only
   from `MacTheme`/`SoyehtThemeColors`. This slice introduces new **prose fonts**
   (a serif reading face for Texto, a Courier face for screenplay) — add them to
   the font-gate allowlist in the same PR that first uses them; introduce no
   unapproved colours. If the rendering path is WebView-based (see §1.1), its
   WebKit use joins the single Web boundary from slice 5 or a sibling bounded
   prose-web module — the ADR decides; no WebKit leaks elsewhere.
4. **Additive wire only.** `PaneMode` gains the writing surface(s) — the ADR
   decides one `document` mode with a kind (text / presentation / screenplay)
   versus separate modes; default to the **smallest additive surface**. New
   methods/events/errors are added; nothing in slices 1–5 changes.
   `generate.py --check` stays green; protocol version stays 1.
5. **Everything bounded** — document size, slide count, scene count, beat-card
   count and links, variants per line, suggestion size, and pasted-image bytes (reuse the slice-5
   2 MiB image bound and attachment assembler). Enforced in the pure cores and
   re-checked before any wire response.

## 1. Order of work

1.1 **Prose-rendering spike + boundary ADR (FIRST, gates everything).** Decide
the prose rendering path — the native TextKit surface used by the code editor
versus an embedded WebView editor reusing the slice-5 web base — measured
against: hybrid live preview (Markdown renders inline; raw syntax shows only on
the active line), the accept/reject agent-suggestion overlay, image paste, and
performance on a large document. Record the decision, the boundaries above, and
the reuse of ADRs 0046/0047/0050/0052 in a new ADR. No feature PR precedes it.

1.2 **Texto** (Tela 14): open/edit/save a local `.md` as a writing pane; hybrid
live-preview; focus mode (active line/sentence emphasised, the rest dimmed); the
click-to-format toolbar writing valid Markdown (heading levels follow Markdown
rules, not free font sizes); a font control.

1.3 **Sections + rail** (Tela 17): three rail items open three bounded section
lists over three separate local folders (one per type; subfolders allowed);
open a file as a pane; create a new document.

1.4 **Apresentação** (Tela 15): the same `.md` in present mode; `---`/blank-run
segments slides; heading = slide content, body = speaker notes; thumbnails
reflect segmentation; image paste; an auto-layout that the writer does not
hand-place; a present action.

1.5 **Roteiro** (Tela 18): a Fountain-backed screenplay editor; elements
auto-format; Tab/Enter auto-advances between elements; SmartType autocompletes
transitions, character extensions and already-used character names and
locations; a collapsible scene navigator lists scenes. **Line variants (agent-generated alternate takes):** any
line can hold ordered variants; a variant-bearing line is marked with a dotted
underline; the inline control appears on hover only — ‹ n/N › cycles, `+` requests
a new variant, `−` removes the current one (only when N > 1). `+` (or selecting
text and adding a variant) sends the line plus surrounding scene context to the
chosen agent through the agent-assist / change-request path (ADR 0052); the agent
generates the replacement, which is appended as a new variant and shown in place.
The writer cycles and keeps the preferred one — the variants are the model's, the
choice is the writer's. Roteiro only in v1 (the mechanic can extend to Texto later).

1.6 **Quadro de ideias / Beat Board** (Tela 19): create idea cards; link a card
to a scene (the link is visible from both sides) and reorganise freely; the link
persists with the document; the agent pane opens beside it.

## 2. Acceptance (work ends only when all pass)

1. **[local/report] Spike + boundary ADR.** The prose-rendering decision and the
   boundaries are recorded in an ADR; `docs/reports/slice-6.md` links it. No
   feature landed before it.
2. **[CI] Texto.** A writing pane opens, edits and saves a local `.md` through
   the owned-file path (ADR 0046/0047); Markdown renders inline with raw syntax
   only on the active line; the toolbar produces valid Markdown for heading
   levels, bold, italic, list, quote, link and image; focus mode and the font
   control work.
3. **[CI] Sections and rail.** Each of the three rail items opens a bounded list
   over its own local folder; a file opens as a pane; a new document is created
   in the right folder; the three types never mix.
4. **[CI] Apresentação.** `---` or a blank run segments slides; a heading is the
   slide's shown content and ordinary text is notes; the thumbnail list matches
   the segmentation; an image pastes into a slide; present mode renders the
   slides with the auto-layout.
5. **[CI] Roteiro.** Fountain text parses into the ordered elements; typing
   auto-formats each element and Tab/Enter advances to the next; SmartType
   offers transitions, character extensions and known names/locations; the scene
   navigator lists every scene and navigates to it. A line holds ordered agent-generated variants: requesting one
   adds a variant through the agent path; the dotted-underline mark plus the
   hover-only control (‹ n/N › · + · −) cycle, add and remove them; only the
   selected variant is the canonical line in the plain Fountain (alternates in a
   bounded sidecar the agent also reads); variants survive reload.
6. **[CI] Beat Board.** An idea card is created, linked to a scene and shown as
   linked from both the card and the scene; cards reorganise; the link survives
   reload (persisted in the document or a sidecar).
7. **[CI] Agent assist.** Selecting text and sending it to an agent returns the
   rewrite/comment as accept/reject suggestions on the same local file through
   the change-request path (ADR 0052); the same agent edits the same file the
   writer sees.
8. **[reviewer] Design fidelity.** victoria compares the running Texto,
   Apresentação, rail, Roteiro and Beat Board panes against `.pen` Tela 14, 15,
   17, 18 and 19 on palette, spacing, type, radii, shadow and each control's
   shape/placement, and pastes the image pairs in `docs/reports/slice-6.md`.
9. **[CI] Fonts and colours.** The new prose fonts are in the font-gate
   allowlist; no unapproved Mac/iPhone colours; the font and colour ratchets are
   green.
10. **[CI] No regression.** Every slice 1–5 acceptance and test still passes; the
    four ratchets, the font and colour gates and the restricted-vocabulary scan
    are green; `generate.py --check` clean; protocol version still 1.

## 3. Outside this slice

- **Media generation from a roteiro** (an agent driving an image generator such
  as ComfyUI or a video tool such as DaVinci Resolve over MCP to produce each
  scene). The roteiro file structure must *support* it — a scene can carry a
  reference image, a caption and a prompt idea — but driving the generators is a
  later slice.
- **Knowledge organiser / PKM** (backlinks, wiki-links, a graph). Deferred to a
  later discussion; local `.md` storage keeps it addable on top. The Beat
  Board's scene links are the only linking in v1.
- Real-time multi-human collaboration, plugins, cloud sync.
- Export to `.pdf` / slide-deck / screenplay-app formats.
- Language switching / i18n (its own future slice); v1 UI ships in English and
  is translation-ready.

## 4. Workflow and delivery

Small PRs (one core, one screen, or one section each), green local gate,
self-merge, an ADR per ambiguity. Design-fidelity acceptance on every screen.
On completion, `docs/reports/slice-6.md` with the per-screen image pairs and the
spike/ADR links, and a message to victoria with the PR links. victoria reviews
each PR after merge for correctness, contract preservation, design fidelity, and
— for the agent assist — safety (the change-request bounds, no engine-side prose
rendering, path/permission scoping, the additive wire, and the file-ownership
reuse).
