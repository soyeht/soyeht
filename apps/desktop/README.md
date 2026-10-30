# Soyeht desktop client

The desktop client for Linux and Windows, written in Rust with GPUI (the UI
framework of the Zed editor). It is another client of the engine's protocol: it
depends on `engine/soyeht-protocol` and shares no message type by copy. The
decisions are in `docs/adr/0084-desktop-client-gpui.md`.

It also builds and runs on macOS, which is how it is developed.

## Layout

| Crate | Role |
| --- | --- |
| `crates/soyeht-client` | Engine connection, handshake, terminal streams, client state (workspaces, panes, split layout, agent transcripts, usage), terminal screen, key encoding. No window system. |
| `crates/soyeht-desktop` | The GPUI window: design tokens (`theme`), neumorphic surfaces (`neo`), the top bar, rail, pane cards, agent panes and composer (`views`), terminal painter. |

This is its own cargo workspace with its own `Cargo.lock`; `engine/` does not
build it and it does not build the engine.

## What works

The window is laid out as the Mac app is: a top bar of workspace tabs (rounded pills with a
close mark, in a strip that scrolls sideways with a fade and a chevron over each end that hides
tabs, a "+N" pill listing the tabs that are not wholly in view, a "+" that opens a terminal or an
agent), the rail down the left (the places, the usage rings with their percentages, settings),
and the panes of the selected workspace as neumorphic cards with a flat interior. The agents'
place and the workspace tabs carry a badge with the requests agents wait on, or a dot while an
agent works.

- Connects to a running engine on its local endpoint, as the Mac app does (a Unix socket on
  Linux and macOS, the engine's loopback endpoint file on Windows, through `soyeht-platform`),
  and reconnects when the engine restarts.
- Panes are tiled as the workspace's split tree says (`workspace.layout`), and the split buttons
  of a pane header split it to the right or below and store the new arrangement.
- Terminal panes show live output (the engine streams raw terminal bytes, parsed
  with `vt100`), accept typing, paste, resize with the window, and scroll back. A resize
  reflows the text as the Mac's terminal does: the lines above the cursor's wrap again at the
  new width and join again when it grows, the history comes back with more rows, and the
  cursor's own line is cut and left to the shell (`soyeht_client::screen`, passing
  `contracts/vectors/terminal-resize.json`). The normal screen is reflowed under a program on the
  alternate one. While a divider or the window is dragged the screen keeps its size and the view
  shows what fits; the screen is fitted, and the shell told, once the size has held for a moment
  (`screen::settle`). What a narrower screen cut from the cursor's line is put back if the
  screen is made wide again before anything is written to it. Known differences from SwiftTerm:
  rows that ended early for a wide character are not joined again (the parser does not mark them
  as wrapped), a screen of one row or one column is only cut, and the autowrap, insert, charset
  and saved-cursor modes of the normal screen are reset by a resize.
- What a key types in a terminal (a letter, a dead key and its letter, AltGr, text an input
  method commits) reaches the program as text, through GPUI's input handler
  (`workbench::text_input`, the way the composer takes it), so `~` then `a` is `ã` on a
  Portuguese, French, Spanish or German layout. Keys that act (Ctrl+C, Enter, the arrows, function
  keys, Alt+letter) go as key events (`soyeht_client::keys`); the rule that tells them apart is
  `soyeht_client::editor::keys::types_text`. The text an input method is composing is drawn at
  the cursor until it commits.
- Text in a terminal is selected with the mouse: a drag selects cells (a drag held past the top
  or bottom keeps scrolling, faster the further the pointer is), a double-click a word (a path or
  an address is one word), a triple-click a line, and Shift+click grows the selection. A selection
  stays on its text while the view scrolls, and goes, as on the Mac, when the person types, when
  the output has a line feed, when the screen is resized, and also when output rewrites that text
  or switches to the alternate screen. Copy is Ctrl+Shift+C on Linux and Windows
  (Command+C on the Mac) and the context menu's Copy; Ctrl+C stays the interrupt. The context menu
  (secondary button) also has Paste and, over an address, Open Link. Ctrl+click (Command+click on
  the Mac) opens an `http` or `https` address, even one that wraps over rows, in the system browser;
  no other scheme opens. A relative `path:line` or `path:line:column` (`src/main.rs:12`) is a link
  too when the file is in the pane's workspace: the same key opens it in the editor with the caret
  at that line, and the same holds for such text in an agent's reply (a code span included), where
  a plain click opens it. On Windows the text is read in the windows dialect: `\` separates like `/`
  (`src\main.rs:2`, `.\src\main.rs:3:2`), a drive (`C:\x\y.rs:3`, `C:/x/y.rs:3:1`) or a UNC
  share (`\\server\share\x.rs:1`) starts an absolute path, a path in quotes may hold whitespace
  (`"C:\my dir\a.rs:4"`), and the MSVC `file(line,column)` counts as a location; cargo's and
  rustc's `--> src\main.rs:3:5` is found like any other path. On Linux a backslash is a legal
  character of a file name that no tool prints in a path, so it is not part of one there. Names
  are kept as written and the file system decides their case, so `SRC\Main.rs` opens `src\main.rs`
  on Windows, and the editor opens the file under its real name. A path that is outside the
  workspace (an absolute one of another folder, or a symbolic link out of it too) or not there is
  not a link. What a reference is: `soyeht_client::file_links`, the Mac's `FilePathLinks`, both
  passing `contracts/vectors/file-paths.json` for the dialects they read. The selection and the address
  rules are `soyeht_client::screen`'s, with unit tests; the painter only draws them.
- Agent panes show the transcript (the person's messages, replies drawn as Markdown, thoughts, tool rows, plans,
  command output in a mono well, hand-overs), take a
  message from the composer (text comes through GPUI's input handler, so dead keys, input
  methods and emoji work), and offer the harness, model, effort and permission pickers, the
  context ring and send or stop. Permission requests are answered in a card that shows the exact
  command, tool arguments and file changes. The header names the agent, the current plan step and
  the latest tool calls (the whole line is in its tooltip), and counts the running turn.
- The composer, and every text field of the window, edits like a text field of the system
  (`soyeht_client::text_input`, with unit tests; the field only draws it): Ctrl+A, C, X and V
  (Command on the Mac; Ctrl+Shift+V pastes too), selection with Shift and the arrows, Home and
  End, Ctrl+Left and Right by words, Ctrl+Backspace and Delete by words, Up and Down through
  lines as they are drawn (wrapped ones too), a click for the caret, a drag, a double click (a
  word) and a triple click (a line) for a selection, and Ctrl+Z with Ctrl+Shift+Z or Ctrl+Y for
  undo and redo (a run of typing, of Backspace or of Delete is one step). Return sends and
  Shift+Return adds a line. On the Mac Control+A and Control+E go to the ends of the line.
- Typing `/` at the start of the composer opens the Mac's list of the Soyeht skills (the pane's
  workspace's and the user's) and the commands the pane's harness advertised (`pane.commands.list`,
  refreshed on `pane.commands_changed`; skills on `skills.changed`), filtered as you type. Up and
  Down move the choice, Return or Tab writes `/name ` in the line, Escape closes the list, and a
  click chooses. Sending `/skill arguments` for a skill the harness does not know points the agent
  at the skill's `SKILL.md`. The rules are `soyeht_client::slash` with
  `contracts/vectors/slash-completion.json`, which the Mac passes too.
- The transcript's text is selected with the pointer, one block at a time (a paragraph, a code
  block, a command's output, a message): a drag, a double click or a triple click, and Ctrl+C
  (Ctrl+Shift+C, Command+C on the Mac) copies the selection when the composer has none. A message,
  a code block and a command's output also show a Copy button while the pointer is over them: it
  copies the message's Markdown source, the code or the output whole. A selection across blocks
  is not made.
- A pane opened from "+" or a split takes the keyboard.
- Preferences, from the rail's gear, laid out as the Mac's sheet: the panels down the left, the open
  one beside them. Appearance (a card with a swatch for each of the nine palettes); Harnesses (a
  raised row for each harness the engine lists: version, the model, effort and permission mode a new
  pane starts in, how ready it is, "default", the reason behind its status, "Make default", the
  permission mode chosen before any pane runs it, Rescan); Usage (the providers with their
  percentage, and for the one chosen its windows, time to reset, the state when there is no reading
  and the source; a click on a ring of the rail opens it); Skills (the user's skills: list, create,
  delete). The footer says the build and whether the engine is connected.
- The harness chip of an agent opens the Mac's switch popover: what switching does, a search over the
  harnesses (installed ones by readiness, the rest under "More harnesses"), a rescan, the model and
  effort of the harness picked (loaded from the engine's `model.list`; a model the agent refused
  before says so), the button "Restart conversation with ..." that sends the engine's
  `pane.harness.switch` with the choice, and under it the account of the agent that runs now
  (switch account, sign out). The chip names the harness a switch is moving to and waits for it.
- A terminal pane says when its process has exited or has gone quiet, and when output was not
  recorded, in the lines the Mac shows above and below its screen.
- On Linux and Windows the top bar carries the window controls (minimize, maximize, close),
  drags the window, and on Linux the window edges resize it. The window opens no larger than
  its display (a Wayland session names its displays only after the application has started, so
  the window waits for them: opened earlier, it would be larger than a small screen holds, and
  restoring it after a maximize would push its buttons off the screen). A tiling desktop
  (Hyprland, sway and the like, told by `XDG_CURRENT_DESKTOP`) ignores minimize and maximize, so a window there gets only the close button and no resize
  edges; a window snapped to half of the screen on GNOME or KDE keeps the buttons and loses the
  resize edges. The window announces itself as `soyeht` (Wayland `app_id`, X11 class) for window
  rules.
- A pane never gets narrower than its composer's controls need. When the panes need more room
  than the window has, the stage scrolls sideways (the wheel moves it only sideways, so
  transcripts and terminals keep their own scrolling) and fades at each end that hides more;
  the pane that was focused or just opened is scrolled into view. The composer keeps its chips at
  full size and scrolls them sideways in the same way.
- The "+" menu lists the installed agents by readiness, each with how ready it is, and the others
  under a collapsed "More harnesses" entry; an agent that cannot start is shown dimmed. The grouping
  and order are the rules of `contracts/vectors/harness-picker.json`, which the Mac passes too. Popup menus are as tall as the Mac's popover at most, scroll inside, and close on
  Escape.
- The transcript carries the Mac's cards: messages queued behind a running turn (with remove and
  resume), the notice when a turn goes quiet, the sign-in card, the answer form for an agent's
  question, subagent cards, and "Load earlier messages" for older history.
- The composer takes attachments: the paperclip opens a picker, files and images can be dropped
  or pasted, and chips above the message line show what will go (with a thumbnail for images).
  Images are prepared and sent with the prompt by the same rules as the Mac.
- The pane header's loop button opens the loop panel (the Mac's "Loop and lead controls"): the
  loop prompt, interval and on/off, the pane's role, and the draft heartbeat that keeps a loop
  from sending over a half-written message.
- The rail's Tasks surface shows the workspace's board and list, a task's detail with its
  subitems, attempts and timeline, the "new task" sheet with create and dispatch, and the
  attention popover for what needs the person.
- The Editor place shows the Mac's editor pane: a tab strip (the engine's order, a preview tab
  in italics until its file is edited, a dot for unsaved edits, a close button on hover), the
  project panel (a lazy tree that respects `.gitignore`, opens a file on a click and keeps its tab
  on a double click, answers the arrow keys, Return and Space), the lines with a gutter and the
  engine's syntax colors, the caret and selection (mouse, double and triple click, shift and word
  motions, Home, End, Page Up and Down), typing with auto-closing brackets and auto-indent,
  copy, cut and paste, the engine's undo and redo (Ctrl+Z, Ctrl+Shift+Z or Ctrl+Y; Command on the
  Mac), save (Ctrl+S: waits for the engine to save the draft), in-file find (Ctrl+F, Ctrl+G,
  Escape) with case, whole word and regex toggles, and a status bar that names the file's line
  ending. A character is a grapheme cluster (a flag, a family, a letter with its accents, a Hangul
  syllable) and takes the cells a terminal gives it (two for a wide character); a `\r\n` line break
  is not text, and new lines of a CRLF file break the same way. Only the lines in view are
  drawn and the replica holds one page of at most `editorViewportLines` lines, read ahead by the
  rules of `contracts/vectors/editor.json`, which the Mac passes too. Left out: replace, multi
  cursor, soft wrap, Vim mode, the command palette and file name search, go to definition and
  references, diagnostics, the git gutter and diff, blame, the agent's review and cursor, file
  history and the conflict comparison (a conflict is shown on a banner), Markdown preview, tab
  drag and drop, and creating, renaming and deleting entries of the project panel. What a key
  types reaches the editor, as in a terminal, through GPUI's input handler (`workbench::text_input`),
  so dead keys, AltGr and input methods work; the text being composed is drawn under the caret and
  the input method's candidate window sits at the caret.
- A window with no workspace asks "What do you want to make?" and offers "Start with an agent"
  (also the first entry of the "+" menu): one card with the first message, the folder (the
  selected workspace's, or one chosen from the picker or the recent folders) and the agent, chosen
  from the installed ones (the default is the one chosen in Preferences, else the one that was
  ready most recently, as on the Mac). Enter makes the workspace if the folder has none, opens the
  agent in it and sends the message. What stops it and which agent stays chosen are
  `soyeht_client::new_agent`, which passes `contracts/vectors/new-pane-form.json` with the Mac's
  `NewPaneForm`. It also offers the platform's folder picker
  (the same entry is in the "+" menu and on Ctrl+Shift+O, Command+O on the Mac). A folder that
  already has a workspace opens that one; otherwise the engine makes a workspace named for the
  folder and a terminal opens in it. Panes start in the folder of their workspace. A double click
  on a workspace tab or on a pane's title renames it in place (Return keeps the name, Escape drops
  it), and the tab's close mark closes the workspace and selects the next tab, or the previous one
  when it was last. The "+" menu also lists the recent folders (the roots of the workspaces, then
  the folders panes run in). The naming, path, reuse, recent-folder, rename and close rules are
  `contracts/vectors/workspaces.json`, which the Mac and iOS clients pass too.
- The Texts place lists the documents of the selected workspace, a page at a time, as the Mac's
  section view does: the section's name and what it holds, "New document" (also "Text" in the "+"
  menu) with a form for a title and an optional subfolder, a line that narrows the list by title
  or path, and a row for each document, which opens in a text pane. A text pane has the file's
  name, its words (the engine's count), whether it is saved, the text in a column of reading
  width (headings and bold, italic, quote, code and link runs drawn in their look, the Markdown
  marks left in the text), a caret and selection, typing, copy, cut, paste and undo through the
  same editing model as the agent composer (`soyeht_client::text_input`), and over a selected
  part the toolbar of formats (bold, italic, headings, lists, quote; Ctrl+B and Ctrl+I, Command on
  the Mac) which the engine applies. What is typed is sent to the engine as the one edit that
  turns the page into the field's text, to the revision the edit before it made; the engine
  saves the draft shortly after the last edit (Ctrl+S waits for it), and a change of the file
  on disk, by an agent or by another client, reaches the open text. Closing a text with edits not
  saved asks first. The button beside the close mark opens an agent beside the text, and the two
  are a writing companion pair as on the Mac. The names, counts, search, new-document form,
  close question and toolbar are `contracts/vectors/writing.json`, and the pairing is the
  `companionSplit` cases of `contracts/vectors/pane-layout.json`; the Mac passes both. Left out:
  the serif reading face, the font, width and focus options, a toolbar that follows the selection
  and the link and image formats, the hybrid view that hides the Markdown marks of the lines the
  caret is not on, the list's sizes and dates, and a text longer than one page of
  `editorViewportLines` lines or with a lone carriage return (only a notice is shown, not the
  text).
- The Screenplays place is the same list, form and search for the screenplays of the workspace
  ("Screenplay" in the "+" menu makes one), and a screenplay opens in a pane that is the text
  pane with a page: Courier Prime at the Mac's size and margins, each line set by its element
  (scene headings and shots bold, characters centered and bold, parentheticals centered, dialogue
  set in from both sides, action plain, transitions on the right, the opening one on the left),
  the marks that force an element (`.`, `@`, `>`, a closing `:`) kept in the text and not
  shown. The engine says which element a line is (`writing.structure`, which also gives the
  scenes the header counts); the lines keep their element as the person types and take the
  engine's answer when it is for the text on screen. Return ends the line and starts the next
  element below it, Tab does the same with the other element (action to character, dialogue to
  parenthetical), and Tab on an empty line only seeds the element there; Shift+Return adds a
  line. Editing, autosave, closing and the companion agent are the text pane's. Which element
  follows, the edit Return and Tab make and how a line sits on the page are
  `contracts/vectors/screenplay.json`, which the Mac passes too (ADR 0105). Left out: the scene
  rail, the idea board and the variants of a line, SmartType and the element menu, the focus
  mode, scaling the page to the pane, centered lines that wrap drawn centered, and a screenplay
  longer than one page of `editorViewportLines` lines.
- The Presentations place is the same list, form and search for the presentations of the workspace
  ("Presentation" in the "+" menu makes one), and a presentation opens in a pane that is the text
  pane with the Mac's body: a rail of the slides the engine split the Markdown into (a numbered
  thumbnail with the slide's title and notes, or a strip of numbers while the pane is narrow), the
  source in a 16:9 stage in JetBrains Mono (the heading lines in the accent and bold, the `---` that
  starts a slide muted, the image lines on a well), and under it what the parts of the source
  mean. The header counts the slides, the slide the caret is on is marked in the rail, and a click
  on a slide puts the caret on its first line. The Present button of the header puts a slide show
  over the pane: the slide's title large and centered on a 16:9 stage over the palette's letterbox;
  the right arrow, Page Down, Space and a click go to the next slide, the left arrow and Page Up to
  the one before, Escape closes it. Editing, autosave, closing and the companion agent are the text
  pane's. The slide words, which slide a line belongs to, the layout and the show are
  `contracts/vectors/presentation.json`, which the Mac passes too (ADR 0106). Left out: the images
  (the rail and the show do not draw them), the speaker notes in the show, a show in a window of its
  own and full screen, the options menu (font, width), and a presentation longer than one page of
  `editorViewportLines` lines.
- Web panes (Linux, ADR 0110): the page is drawn off screen by the system's WebKitGTK (loaded at
  run time; a machine without it says so in the pane) and painted by the window, so menus and
  popovers stay above it on Wayland and X11. The person types, clicks and scrolls as in a
  browser, with an address line and back, forward and reload; the profile (cookies, local storage)
  is kept under the engine's data directory, so a sign-in lasts. The window is the engine's web
  host: an agent's `web_snapshot`, `web_screenshot`, `web_read`, `web_navigate`, `web_scroll` and
  `web_wait` reach the page (`soyeht_client::web_host`); a person's input in the pane stops an
  agent until "Hand back to the agent". Left out: click, type, press, hover, select, evaluate,
  console and network for agents, a `<select>` list, a context menu, pop-up windows and file
  choosers, text composition shown inside the page, and Windows (WebView2, phase 2). Dictation is
  listed with a notice.
- Left out of Preferences, because the Mac's panel is not a feature of this client or of the engine:
  General (system alerts and Dock badges: an operating-system notification setting), Engine, Phone
  and Tasks and loops (placeholders on the Mac too), Shortcuts (Vim mode: the editor has none), Links (opening a link in a web pane) and Dictation. Of the Harnesses panel, adding, editing and
  removing a custom harness (the engine has `harness.custom.*`; the form is its own piece of work)
  and the raw terminal row (a static display on the Mac) are not done.

## Run

```sh
cd apps/desktop
cargo run --release -p soyeht-desktop
```

When nothing answers at the engine socket, the window starts `soyeht-ptyd` and `soyeht-engine`
from the folder of its own executable (ADR 0102): a release puts the four programs
(`soyeht-desktop`, `soyeht-engine`, `soyeht-ptyd`, `soyeht-mcp`) side by side. They keep running
when the window closes. A `cargo run` build has them elsewhere, so start the engine first as
`docs/dev-local.md` says, or copy `engine/target/release/soyeht-{engine,ptyd,mcp}` next to
`target/release/soyeht-desktop`. The window shows "Starting the engine..." meanwhile, and a
problem line naming the missing program when one is not there.

On Linux the window needs a Vulkan driver; a machine without a graphics card uses Mesa's
software renderer, which needs an up-to-date Mesa (see "Graphics on Linux" in
`docs/platforms.md`: `mesa-vulkan-drivers`, and `sudo dnf upgrade mesa-vulkan-drivers` on
Fedora).

The socket is found in the data directory of the client's profile, in the system's native data
location (`~/Library/Application Support/Soyeht.next/dev` on macOS, `%LOCALAPPDATA%\Soyeht.next\dev`
on Windows, `${XDG_DATA_HOME:-~/.local/share}/soyeht-next/dev` on Linux, ADR 0112), which is where
the engine keeps it. A build is the Dev profile unless `SOYEHT_PROFILE=prod` is set (or the build
was compiled with `SOYEHT_BUILD_PROFILE=prod`). To reach another engine, set the socket path; the
window then starts no engine of its own:

```sh
SOYEHT_ENGINE_SOCKET=/path/to/engine.sock cargo run -p soyeht-desktop
```

Every log line is written to standard error and also to `logs/desktop.log` in that profile
directory, rotated at 16 MiB (see "The desktop client's log" in `docs/platforms.md`): a client
started from a menu, or a Windows release build, has no terminal to show them.

Shortcuts use Command on macOS and Control+Shift on Linux and Windows, so that
they never take keys a terminal program needs: New Terminal (`t`), Paste (`v`),
Copy (`c`), Quit (`q`).

GPUI needs the platform SDKs: Xcode with the Metal toolchain on macOS; Vulkan
drivers and the Wayland or X11 development libraries on Linux; the Windows SDK
on Windows.

## Check

```sh
cd apps/desktop
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

`scripts/check_all.sh` runs the same three steps. The first run on a machine
fetches the Zed git dependencies and compiles GPUI, which takes several minutes.
The rules both clients share (card wording, attachment preparation, loop and task
presentation) live in `contracts/vectors/` and are run by the Swift and Rust tests alike. The client crate tests
include an end-to-end run against a scripted engine on a real local socket; the
palettes and tokens in `theme` are generated from `contracts/design-tokens.json`.

## Neumorphic surfaces

`neo.rs` draws the raised, inset and pane-shell surfaces from the same tokens as the Mac. GPUI's
`blur_radius` is a Gaussian's standard deviation, so it is half the token's blur (the design
tool's blur). The Mac stacks its two shadows, so its dark shadow is cast by the light halo too; the
desktop emulates that with three shadows whose scales are in the `desktop` section of the tokens
(`shadow*Scale`, `shadow*Opacity`), fitted against renders of the Mac's own `neoRaised`. The wells
(`neoWell`) use the plain inset pair. GPUI has no squircle corners, no blend modes and no rounded
clipping of children, so corners are circular arcs and content that touches a rounded corner is
rounded by its own radius.

`cargo run --release --features dev-tools --example neo_gallery -- milk` opens every surface on one canvas at fixed
positions (no text), for comparing with the Mac's render of the same surfaces pixel by pixel.

## Rules

- Colors and sizes come only from `theme`, generated from `contracts/design-tokens.json`
  (also the source of the Swift palette). No literals.
- Only the pane shell is raised; everything inside a pane is flat.
- Wire types, framing and method names come from `soyeht-protocol`. A missing
  engine capability is a contract change, not a client-side guess.
- GPUI is pinned to one commit; an upgrade is its own pull request.

## Comparing with the design and the Mac

`examples/design_audit.rs` opens the window on a recorded engine state
(`examples/design_audit.json`: workspaces, panes, a transcript for each agent, permission requests,
terminal output, usage and harnesses, all in the wire's own shapes), with no engine:

```sh
cargo run --release --features dev-tools --example design_audit -- crates/soyeht-desktop/examples/design_audit.json milk \
  --workspace ws-ci --size 1200x700 --open config:claude:model
```

The replay and the gallery are behind the `dev-tools` feature, so the shipping binary does not
carry them.

`--open` takes `new-pane`, `overflow`, `settings` or `settings:<panel>` (`appearance`, `harnesses`,
`usage`, `skills`), `harness:<pane>`, `config:<pane>:<option>`, `loop:<pane>`, `tasks`, `tasks:list`,
`task:<id>` or `new-task`.
`examples/design_audit_features.json` is the second recorded state, with the workspaces that show
the cards, the attachments, the loop panel and the tasks.
Besides the state of the engine the fixtures carry `catalogues` (what `model.list` answers for each
harness), `settings`, `skills`, `commands` (what `pane.commands.list` answers for each pane), `drafts` (text already in a composer) and `notifications` (events replayed after the state).
The same file feeds the Mac's own views, rendered offscreen, so that both clients draw the same
conversation. The `.pen` frames "Interior da pane" (approved) and `DCzvR`, `OtYMZ` are the
references for the interior, the composer and the header.

## Assets

Fonts and the provider logos are the Mac app's own resources, embedded from `apps/mac` and
`apps/shared`, so no second copy exists to drift.

- The fonts (Inter, Nunito, JetBrains Mono) are under the SIL Open Font License 1.1. GPUI on Linux
  draws a variable font at its default weight, so the medium, semibold, bold and extra bold cuts of
  Inter, and its italic and bold italic (the slant axis), are instanced from the Mac's variable `Inter.ttf` by `scripts/instance_inter_fonts.py` and
  kept in `crates/soyeht-desktop/assets/fonts`. The license
  texts that must travel with a binary are in `crates/soyeht-desktop/assets/licenses/`.
- The line icons in `crates/soyeht-desktop/assets/icons` are Lucide: ISC license, and the MIT
  license for the icons derived from Feather. Both are in
  `crates/soyeht-desktop/assets/LICENSE-lucide.txt`. They are the closest open equivalent of the
  Mac client's system symbols, which cannot be redistributed.
- The provider logos are the marks of their owners. They are drawn only to identify a provider's
  agent and usage, as in the Mac app, and are not covered by the licenses above.
- The application icon is the Mac app's, repackaged without resizing into `packaging/` (the Windows
  `.ico` and resource script, the Linux icon theme and `.desktop` entry). `docs/platforms.md`, "Release
  builds", describes them, the release profile and the build id.
- Desktop-only measurements (window frame, popups, glyph sizes, the line height of each face: GPUI's
  default line is 1.618 times the size, SwiftUI's is the font's own) are in the `desktop` section of
  `contracts/design-tokens.json`. They are generated for Rust only: the Mac output does not
  mention them and does not change when they do.
