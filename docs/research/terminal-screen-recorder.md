# Terminal screen recorder (research tool)

A developer tool that records what terminal panes show, to find out from data how to tell that
the person is in the middle of typing. A message from another agent can then be delivered to a
harness running in a terminal (Claude Code, Codex, ...) without landing in the middle of what the
person types.

The count of keystrokes followed by a wait for "not typing" fails on cases such as `dd` in a
modal editor, where keys change the text without typing it. This tool does not guess a rule. It
keeps the screen as it changes, so that a rule can be tried against recordings of real use.

## What it is, and what it is not

- **Development only.** It is a cargo example of `soyeht-client` (`apps/desktop`,
  `examples/screen_recorder`). `cargo build` does not build an example, none of the packagers
  (`scripts/package_*.py`, `scripts/release_flow.py`) builds one or names it, and
  `scripts/test_package_linux.py` fails if one does. It is not linked into the window, the engine
  or any package, and nothing in the app starts it. The real app records nothing.
- **A client, nothing more.** It connects to a running engine as the window does, through
  `soyeht-client`, and keeps each terminal pane's screen with the client's screen model
  (`TerminalScreen`). Besides the handshake, the state snapshot and the event subscription every client
  makes, the only request it makes is `pane.stream.subscribe` for the panes it records. It never sends input and
  never resizes a pane: it fits its own copy of a screen to the size the engine gives the pane.
  The engine has no code for it.
- **Screens, not keystrokes.** A record holds text the screen shows. The bytes the client sends
  to a pane are not in it, because the recorder sends none and has no such data. A program that
  does not echo (a password prompt) leaves nothing to record, since nothing shows.
- **Local.** Recordings are written under the user's cache folder, in an owner-only directory, with
  owner-only files (`soyeht_platform::fs`). Nothing is sent anywhere.
- **Connecting.** With no option it connects to the engine of this build's profile (the Dev
  engine of the installed app, from a debug build). It prints the socket it uses before it
  connects. To try it without touching a running app, start a disposable engine of your own
  (`soyeht-ptyd --data-dir D`, then `soyeht-engine --data-dir D --listen 127.0.0.1:0`, with `D`
  short enough for a socket path) and pass `--data-dir D`.

## Run it

From `apps/desktop`:

```
cargo run -p soyeht-client --example screen_recorder -- [options]
```

Ctrl-C stops it. Every record is one whole write, so a stop loses nothing. Options (`--help`
lists them):

| Option | Meaning |
|---|---|
| `--socket PATH` / `--data-dir DIR` / `--profile NAME` | which engine; the first given wins |
| `--pane ID_OR_NAME` | record only this pane (repeat); default every terminal pane |
| `--out DIR` | where recordings go |
| `--max-file-mb N` | rotate a pane's file at N MB (default 16) |
| `--keep N` | rotated files kept per pane (default 3) |
| `--max-total-mb N` | stop writing at N MB for the run (default 256) |

A release build (`--release`) keeps up with a flooding pane better. The time of a record is the
time the recorder handled the output, so a recorder that fell behind stamps late.

## Where the files go

```
~/Library/Caches/soyeht-research/terminal-screens/<date>/<pane>.jsonl
```

(`$XDG_CACHE_HOME`, else `~/.cache`, on Linux; `%LOCALAPPDATA%` on Windows.) `<date>` is the UTC
date of the record; a run that crosses midnight continues in the next folder. A file that reaches
its cap is renamed `<pane>.<n>.jsonl` and a new `<pane>.jsonl` begins; only the newest `--keep`
rotated files are kept, and once the files of a run hold `--max-total-mb` the recorder drops
records, and says so, instead of writing. A file never grows past its cap by more than the
header and screen that start it. A file of an earlier run is never appended to; it is put aside as
a rotated file.

## What a record holds

Every line is a JSON object with `v` (format, 1), `t` (milliseconds since the epoch), `m`
(milliseconds since the recorder started, for gaps), `pane` and `kind`:

- `pane`: the facts about the pane the wire gives: `name`, `cwd`, `cols`, `rows`, `pid`, `agent`,
  and `foreground` (always `null`, see below). Written first in every file and again when one of
  them changes.
- `screen`: the whole visible screen: `rows` (text, blank end cut off, empty rows `""`), `cols`,
  `cursor`, `alt`. `why` is `file` (the start of a file) or `interval`. At most one a second per
  pane: after a change it is written at once if the last was a second ago, else when the second
  has passed.
- `change`: written each time what the screen shows changes: `seq`, `alt` (the alternate screen
  is active), `cursor` (`row`, `col`, `hidden`), `line` (the text of the cursor's row), `above` and
  `below` (up to 2 rows each side, nearest the cursor last in `above`, first in `below`) and
  `outBytes` (how much output arrived since the last record). Output that arrives together is one
  record. Output that changes nothing visible (a mode switch) is none.
- `marker`: `reset` (the engine replays the pane from the start), `gap` (output it no longer
  retains was skipped), `connected`, `disconnected`, `closed`.

## The foreground program

It is not available. The engine's own observation record has a `foreground_process` field, and
it is always empty there ("Raw PTY output does not identify the foreground process"). Neither the
engine nor the supervisor puts it on the wire, so the `pane` record has `foreground: null`. What
the wire gives is `pid`, the process the pane started (the shell or the harness's launcher), and
for an agent pane `agent`. A recorder on the same machine could look up the foreground process of
the pty's terminal itself (the process group of the pty), without any change to the engine; this
tool does not.

## Reading a recording

`examples/screen_recorder/analyze.py` prints the moments the input line went from empty to
non-empty and back, with the rows around it and how long the screen then stayed unchanged:

```
uv run python apps/desktop/crates/soyeht-client/examples/screen_recorder/analyze.py \
  ~/Library/Caches/soyeht-research/terminal-screens/<date>/<pane>.jsonl [--screens] [--json]
```

The input line is the cursor's row. It is empty when the text left of the cursor, once blanks and
box-drawing are cut off, ends in a prompt glyph and nothing is right of the cursor. No program is
named, so one rule reads a shell prompt and a harness's input box. The `still` figure of each
moment is the first number a rule like "the input box is empty on screen and has not changed for
N seconds" has to choose. Its tests run with `uv run python -m unittest test_analyze` from the
same folder.
