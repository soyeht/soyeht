# Terminal messaging spike (slice 0 of ADR 0165)

What a Claude Code, Codex, OpenCode or Cursor Agent that the person starts in a terminal pane hands
to the processes it starts, how a pane shows each of them to `ps`, whether each takes a framed
(bracketed) paste whole, and which first character of a pasted message changes what Enter does.
Run on 2026-11-14, on macOS (arm64) with the four real binaries, and on a Windows 11 24H2 VM through
a pseudoconsole. ADR 0165 waits on these answers before it promises that a harness can send; the
per-harness table of that ADR is corrected from the one below.

Nothing here is a product change. No engine, supervisor or app was started: each case runs a
real binary inside a plain pty that a small script owns (the shell is the session leader, as in
a terminal pane), with a probe stdio MCP server, a probe shell command and an `$EDITOR` stand-in
that copies the harness's input buffer to a file. The scripts are quoted below, not committed.

## Versions

| | Version | Notes |
|---|---|---|
| Claude Code | 2.1.325 (macOS), 2.1.296 (Windows) | the owner's login was in use |
| Codex | 0.164.0, then 0.169.0 | **upgraded during the spike by a mistake of the rig, see "Side effects"**; cases 1 and 3 ran on both |
| OpenCode | 2.2.5 | isolated config, data and state directories and a private service port |
| Cursor Agent | 2026.09.02 | its terminal UI refuses to start without a login, which the spike does not perform |

## Method, and the rules it kept

- Each harness ran with a clean pane environment (`HOME`, `PATH`, `TERM`, `LANG`, `SHELL`, `PS1`) plus
  `SOYEHT_PANE_BINDING=probe-123`. No variable of the process that drove the spike was passed on.
- MCP servers came from per-launch configuration only: `claude --strict-mcp-config --mcp-config FILE
  --setting-sources local`; `codex -c mcp_servers.probe.command=...` with every server of the user's
  configuration switched off through `-c mcp_servers.NAME.enabled=false`, and `--disable hooks`,
  `plugins`, `apps`, `computer_use`, `browser_use`; a project `opencode.json` and isolated `XDG_*`
  directories for OpenCode; a project `.cursor/mcp.json` for Cursor. The user-level files were
  read to know what to switch off and never written by the spike (the harnesses themselves write
  their own session and state files when they run).
- No harness was logged in by the spike. A harness that needs a login before it starts its MCP
  servers is recorded as such. Codex and Claude ran on the owner's existing login for the cases
  that needed a model turn (one tiny prompt each: "run this command, answer OK"); OpenCode used its
  free model.
- The pasted payload is `w0000 w0001 ...` of exact length, so a clipped tail or a changed byte shows;
  "multi" is `[livia → novo · m:7f3a]` followed by lines of 12 words. A harness's input buffer was
  read back through its "edit in `$EDITOR`" key (Ctrl+G for Claude and Codex, Ctrl+X E for OpenCode) and compared
  byte for byte with what was pasted.
- The probe MCP server logs, at `initialize` and `tools/list`: its pid, parent, process group, controlling
  terminal (`open("/dev/tty")`), the environment variable, the names of the environment it received,
  and for each ancestor whether that process's own environment holds the variable (`ps -E`).

## Answers

### 1. Does the MCP server the harness starts inherit the pane's environment?

| | Server's parent | Controlling terminal | Pane variable | Fallback that works |
|---|---|---|---|---|
| Claude Code | `claude` itself, in the pane's foreground process group | the pane's | **arrives** (the whole environment) | none needed |
| Codex 0.164.0 and 0.169.0 | `codex` itself, in its own process group (no app-server in the chain) | the pane's | **does not arrive**: an allowlist only (`HOME LANG LOGNAME PATH SHELL TERM USER`) | `env_vars = ["SOYEHT_PANE_BINDING"]` in the MCP entry (measured on both versions); the parent `codex` holds the variable; the terminal is the pane's |
| OpenCode, default | `opencode serve --service`, the background service; its parent is the pane's `opencode` when that pane started the service, else 1 | **none** | **does not arrive**; with the service started by pane A, **pane B receives A's value** (the `environment` entry with `{env:VAR}` is resolved by the service too) | `opencode --standalone` (a private server, child of the pane's `opencode`: the whole environment arrives, still no controlling terminal); otherwise none for the MCP server (see question 2) |
| Cursor Agent | not measured in the interface (login); `mcp list-tools` path: the `cursor-agent` node process | the pane's | not measured in the interface; `mcp list-tools` path: **does not arrive** (allowlist: `HOME LC_CTYPE LOGNAME PATH SHELL TERM USER`) | `"env": {"SOYEHT_PANE_BINDING": "${env:SOYEHT_PANE_BINDING}"}` in the entry (measured on the `mcp list-tools` path) |

Evidence, trimmed (`<home>` and `<scratch>` stand for paths of the machine). Claude, the server's own
record and the chain it printed:

```
start pid 71026 ppid 70940 binding probe-123 ctty /dev/tty
    71026 70940 ttys002 binding=probe-123  python probe_mcp.py claude <scratch>/log-claude.jsonl
    70940 70893 ttys002 binding=probe-123  claude --strict-mcp-config --mcp-config <scratch>/claude-mcp.json --setting-sources local
    70893 70888 ttys002 binding=None       bash --norc --noprofile -i          <- the pane's shell
```

Codex 0.169.0, the same record. The server is a child of `codex` and the pane's terminal is its
own, yet its environment holds only the allowlist, and `codex` (its parent) holds the variable:

```
start pid 95495 ppid 95185 binding None ctty /dev/tty
   env names: CPATH HOME LANG LIBRARY_PATH LOGNAME MANPATH PATH SDKROOT SHELL TERM USER __CF_USER_TEXT_ENCODING
    95495 95185 ttys002 binding=None        python probe_mcp.py codex ...
    95185 95081 ttys002 binding=probe-123   codex --disable hooks ... -c 'mcp_servers.probe.args=[...]'
```

(`CPATH`, `LIBRARY_PATH`, `MANPATH`, `SDKROOT` and `__CF_USER_TEXT_ENCODING` come from the system's
`python3` shim, not from the harness.) With `-c 'mcp_servers.probe.env_vars=["SOYEHT_PANE_BINDING"]'`
the same record shows `binding probe-123` and the name in `env names`.

OpenCode, default mode, two panes with different values, the second in another project:

```
pane A (started the service) -> MCP start pid 36593 ppid 36087 binding probe-AAA ctty none
   chain: probe(36593) <- opencode serve --service (36087) <- opencode (36061, pane A)
pane B (binding probe-BBB, other project, environment={"SOYEHT_PANE_BINDING": "{env:SOYEHT_PANE_BINDING}"})
   MCP start pid 36997 ppid 36087 binding probe-AAA ctty none
```

When the service is already running (started from a shell with no binding) the first pane gets
`binding None`, parent `opencode serve --service`, whose own parent is 1. `--standalone`:

```
start pid 33586 ppid 33272 binding probe-123 ctty none (Device not configured)
    33586 33272 ??       binding=probe-123  python probe_mcp.py opencode ...
    33272 33247 ??       binding=probe-123  opencode serve --stdio --port 0
    33247 33068 ttys002  binding=probe-123  opencode --standalone
```

Cursor, `cursor-agent mcp list-tools probe` typed in the pane, with `HOME` pointing at an empty directory (an
approval is needed first, `cursor-agent mcp enable probe`):

```
MCP start pid 50266 ppid 49986 binding None ctty /dev/tty
env names: CPATH HOME LC_CTYPE LIBRARY_PATH LOGNAME MANPATH PATH SDKROOT SHELL TERM USER __CF_USER_TEXT_ENCODING
   49986 49353 ttys002 binding=probe-123  cursor-agent --use-system-ca <version dir>/index.js
# with "env": {"SOYEHT_PANE_BINDING": "${env:SOYEHT_PANE_BINDING}"}:  binding probe-123
```

Cursor's terminal interface with the real home shows "Press any key to log in" and never starts a
server; with a made-up API key it answers "the provided API key is invalid" and exits. Its `persist`
mode was not run for the same reason.

### 2. Does a shell command the harness runs see the variable?

Each case is one tiny model turn: "run this exact command with your shell tool, then answer only OK".

| | Command's parent | Controlling terminal | Pane variable |
|---|---|---|---|
| Claude Code | a `/bin/bash -c` that `claude` started | none | **arrives** (the whole environment, plus its own `CLAUDE*` variables) |
| Codex 0.169.0 (default sandbox, workspace write) | a process of `codex` (the sandbox denied `ps`, so only `ppid` was logged) | none (the sandbox answered "operation not permitted") | **arrives** |
| OpenCode `--standalone` | `opencode serve --stdio`, child of the pane's `opencode` | none | **arrives** |
| OpenCode, default | `opencode serve --service`, started with no variable | none | **arrives**: the interface sends its own environment with the command, unlike the MCP server |
| Cursor Agent | not measured (login) | | |

So for OpenCode in its default mode the MCP server has no way to learn the pane, but `soyeht message`
run through the shell tool does. Records, OpenCode default mode (the service was started with no
variable and has none, the command has it):

```
SHELL pid 71266 ppid 71043 binding probe-123 ctty none
    71266 71043 ??  binding=probe-123  python probe_mcp.py shell-tool ...
    71043     1 ??  binding=None       opencode serve --service
```

Codex also runs `zsh` shell-snapshot processes in its own process group as soon as it starts.

### 3. What `ps` shows for the foreground program

The foreground group is `tcgetpgrp` of the pane's pty; the executable path is `proc_pidpath`. Lines are
`pid ppid pgid tty exe | argv`:

```
claude    ...  /<home>/.local/share/claude/versions/2.1.325   | claude --strict-mcp-config ...
codex     ...  /opt/homebrew/Caskroom/codex/0.169.0/bin/codex | codex --disable hooks ...
opencode  ...  /opt/homebrew/Cellar/opencode-v2/2.2.5/bin/opencode | opencode --standalone
cursor    ...  /<home>/.local/share/cursor-agent/versions/2026.09.02-c22c1a3/node | /<home>/.local/bin/cursor-agent --use-system-ca /<home>/.local/share/cursor-agent/versions/2026.09.02-c22c1a3/index.js --approve-mcps --trust
```

- **Claude Code**: the executable is the file named after the version, `argv[0]` is `claude`; the
  foreground group is `claude` and the MCP servers it starts (same process group). The `claude`
  symlink never shows.
- **Codex**: a native binary under the package manager's directory, `argv[0]` is `codex`. MCP servers and its
  shell snapshots are in their own groups, so the foreground group is `codex` alone.
- **OpenCode**: a native binary, `argv[0]` is `opencode`. In the default mode the pane shows no child
  at all (the service is elsewhere); with `--standalone`, `opencode serve --stdio --port 0` is a child
  in its own session with no terminal.
- **Cursor Agent**: the `cursor-agent` script ends in an `exec` of `node`, so no shell script stays in
  the tree: the executable is the bundled `node`, `argv[0]` is the path **as invoked** (the `agent`
  symlink would give `agent`), and the real program is the script argument (`index.js`) after
  `--use-system-ca`. The foreground group is that one process.
- The probe servers and shell commands show up under the same names in a process table, so a rule
  that matches a harness by the executable, `argv[0]` or script argument should look at the
  foreground group's leader.

### 4. Bracketed paste

All three runnable harnesses switch DECSET 2004 on when they start (read from the output stream;
Claude also `?2031` and `?1004`, Codex `?1004`, OpenCode the alternate screen `?1049` and mouse
modes). Cursor's login screen does not, and its input line was not reached.

A framed paste (`ESC[200~ ... ESC[201~`, one write, no Enter) of each size, then the buffer read back
through `$EDITOR`:

| Paste | Claude 2.1.325 | Codex 0.169.0 | OpenCode 2.2.5 |
|---|---|---|---|
| 300 B, one line | verbatim, identical | verbatim, identical | `[Pasted ~1 lines]`, identical +1 trailing space |
| 800 B | verbatim, identical | verbatim, identical | collapsed, +1 space |
| 1022 B | `[Pasted text #N]`, identical | `[Pasted Content 1022 chars]`, identical | collapsed, +1 space |
| 4096 B | `[Pasted text #N]`, identical | `[Pasted Content 4096 chars]`, identical | collapsed, +1 space |
| 12288 B | collapsed, identical | `[Pasted Content 12288 chars]`, identical | collapsed, +1 space |
| 60000 B | collapsed, identical | `[Pasted Content 60000 chars]`, identical | collapsed, +1 space |
| multi-line 4145 / 12441 / 60749 B | `[Pasted text #N +51 lines]` (and `+154`, `+750`), identical | `[Pasted Content 4142 chars]` (characters, not bytes), identical | `[Pasted ~52 lines]` (`~155`, `~751`), +1 space |

- **No clipping** on any harness with framing, at 1022 and 1024 bytes included (the sizes at which the
  old app's unframed paste lost its head).
- **What the line shows**: Claude and Codex show the text up to a size between 800 and 1022 bytes, then a
  collapsed form (Claude `[Pasted text #N]`, with `+K lines` for K newlines; Codex `[Pasted Content N
  chars]`, counting characters). OpenCode collapses even 300 bytes to `[Pasted ~N lines]`; a two-line
  message of about 50 bytes was shown verbatim. A profile that expects the first line of the message on
  the screen after a paste is right only for short messages: the collapsed forms say nothing of the text.
- **OpenCode appends one space** to the buffer after a pasted block (read back +1 byte, always a trailing
  space). A check that compares the exact bytes must ignore that one trailing space, as it did for Claude's
  padding in the old app.
- **Unframed**, for comparison: Claude 2.1.325 kept all of 4096 and 12288 bytes but split them into 4
  and 12 `[Pasted text #N]` pieces, byte-identical when joined (the clipping of 2.1.268 and 2.1.273 did
  not reproduce on this version); Codex kept 4096 whole and split 12288 into three placeholders, identical;
  **OpenCode lost 24 of 12288 bytes** (12264 read back). Unframed typing stays refused.
- **Cursor Agent**: not measured (see question 1).
- Typing while busy: a line submitted to Codex a few seconds after start-up showed `Queued follow-up inputs`;
  the harness keeps it and runs it later.

### 5. A first character that changes what Enter does

Pasted as `<c>livia → novo · m:7f3a] check this` plus a second line, then (for the effects that need
it) Enter. `[` is the first character of the planned envelope.

| First character | Claude Code | Codex | OpenCode |
|---|---|---|---|
| `!` | **shell mode** at once (the prompt turns into `!`, the footer says "! for shell mode"); Enter ran `!echo ...` and the harness then took the output into a model turn | **shell mode** shown in the footer; Enter a few seconds after start-up queued it (`Queued follow-up inputs`) instead of running it, so its effect was not seen | **shell mode on submit**: Enter ran the line in bash, `!` included (`!echo: command not found`, exit 127) |
| `/` | text stays; Enter: "Unknown command: /nosuchcmd" and the rest handled as arguments of an unknown skill | text stays; Enter: "Unrecognized command '/nosuchcmd'", **not sent to the model** | text stays; Enter sent it to the model as plain text (no local command handling) |
| `@` | text stays, no picker after a paste | the same | the same |
| `#` | text stays, no mode | the same | the same |
| `[` | text stays | text stays | text stays |

The pasted `/` and `!` are the ones that act; `/` acts only when the first word is a command the harness
has (a real name such as a built-in would run it). `[` triggered nothing on any of the three. Cursor: not
measured.

### 6. Windows: ConPTY

A small program creates a pseudoconsole (`CreatePseudoConsole`, `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`),
starts a console program in it, waits for the program to say READY, and writes one framed paste to the
pseudoconsole's input pipe. Windows 11 24H2, build 26100.9711. The first host-side result is a fact about
the engine's future code, not the harness: **a child created with the pseudoconsole attribute inherits
the host's own standard handles when those are redirected** (the host's stdio was a service session's
pipes), and the child then wrote to the host's output and read the host's empty input. The
fix is `STARTF_USESTDHANDLES` with `INVALID_HANDLE_VALUE` for the three handles, which every case below uses.

| Case | Result |
|---|---|
| Child with `ENABLE_VIRTUAL_TERMINAL_INPUT` reads raw bytes; framed paste of 1022, 4096, 12288 bytes (one line) | **intact**: both markers present, the payload between them identical, byte for byte |
| The same with 4096 and 12288 bytes in lines (`\n`) and `→`, `·` (the child set the console input code page to UTF-8) | **intact**, `\n` kept as `\n` |
| The same child that wrote `ESC[?2004h` | the host's output stream carries `ESC[?2004h` after `ESC[?9001h ESC[?1004h`: **ConPTY passes the program's mode through** |
| The same child that does not write it | no `ESC[?2004h` in the output, and the markers still reach a VT-input child: ConPTY does not invent the mode and does not strip the markers |
| A child with ordinary console input (no VT input) | the markers are consumed and the text arrives as typed characters; a one-line 4096 paste is intact, but **the `\n` of a multi-line paste are lost** (4039 of 4096 characters in the multi-line case) |
| Custom variable: set in the host's environment, `lpEnvironment = NULL` | the child **and a grandchild** (`cmd /c echo %VAR%`) see it |
| Custom variable: an explicit environment block with `CREATE_UNICODE_ENVIRONMENT` | the child and the grandchild see it |
| Claude Code 2.1.296 (installed on the VM) started in the pseudoconsole with the probe MCP server | the server is a **child of `claude.exe`**, has a console, and **holds the variable** |
| A framed paste of 4096 and 12288 bytes (one line) into Claude, buffer read back through `$EDITOR` | **identical** |
| A framed paste of 12288 bytes in lines with `→` and `·` into Claude | **not identical**: the two non-ASCII characters arrived as the UTF-8 bytes read as Windows-1252 (`â†’`, `Â·`), with or without `chcp 65001` before the launch; ASCII and `\n` were intact |

Windows records (the chain is read with a process table; `console=True` is `GetConsoleProcessList` > 0):

```
start pid=9088 ppid=9336 binding=probe-123 console=True chain=python.exe <- claude.exe <- conpty_probe.exe <- powershell.exe <- ... 
```

The Claude test also shows that on this build Claude starts its MCP servers when the interface says "Not logged in".
The mojibake is the open item of this section: the test program with the input code page set to UTF-8
read `→` and `·` correctly through the same pseudoconsole, so ConPTY's decoding of the input pipe is
not the cause, but the cause was not isolated further (Claude's console input path, or the way the pseudoconsole
was created). The ADR's header uses `→` and `·`, and the messages of a Portuguese speaker carry accents.

## Conclusions, per harness

- **Claude Code**: pane binding via environment **works** (macOS, MCP server and shell tool; Windows,
  MCP server), and the pane's terminal is the server's own. No fallback. A framed paste is taken whole
  up to 60000 bytes; `!` is the first-character hazard (and `/` with a real command name).
- **Codex**: pane binding via environment **does not work** for the MCP server: its environment is
  an allowlist. **Fallback needed: `env_vars = ["SOYEHT_PANE_BINDING"]` in the MCP entry** (measured on 0.164.0 and
  0.169.0; the connect step writes the entry, so it can carry it). The two weaker fallbacks both hold: the
  parent `codex` has the variable in its environment, and the server's controlling terminal is the pane's. The shell tool
  has the variable. The shared `app-server` of the ADR did not appear in this tree on either version: a plain
  `codex` runs its own MCP servers as children (`codex --remote` and the `codex agents` browser were not run).
- **OpenCode**: pane binding via environment **does not work** in its default mode, and is **wrong**,
  not just missing, when the pane that started the service is another pane's. **Fallback needed**: the
  MCP route cannot identify the pane (the server has no terminal, its ancestors are the service); a harness
  launched with `--standalone` works; otherwise sending goes through the CLI the shell tool runs
  (`soyeht message`), which holds the variable in both modes. Receiving is unaffected. The MCP entry
  must not be the way OpenCode proves its pane unless started standalone.
- **Cursor Agent**: **cannot be concluded on this machine**: its interface needs a login, which the
  spike does not perform. On the one path measured (`mcp list-tools`) the environment is an allowlist and
  `${env:SOYEHT_PANE_BINDING}` in the entry's `env` carries the variable. Run the same probe from a logged-in
  session before the slice that promises Cursor can send; paste, hazards and the shell tool are still open.

For the ADR: receiving works for all four through the typed channel (framed paste is intact to
60000 bytes on the three that were measured; unframed is clipped on OpenCode); sending through MCP works
for Claude as is, for Codex with `env_vars`, for OpenCode only standalone, for Cursor unproven; sending
through `soyeht message` from the shell tool works wherever the shell tool has the variable (Claude, Codex,
OpenCode in both modes). The expectations of the "Per harness" table that this corrects: the shared
app-server of Codex is not in the way; OpenCode's service is, and in the worst way (another pane's value).

## Side effects of the spike, and what was not measured

- **Codex was upgraded from 0.164.0 to 0.169.0 by the rig**, on the machine it ran on. Its start-up
  offered "Update now" and a rig bug typed unframed text into that dialog, which accepted it as its
  first entry. The package manager ran the cask upgrade, after refreshing its own metadata. The rig now refuses to send
  anything until a clean prompt shows. Cases 1 and 3 had run on 0.164.0 before and were repeated on
  0.169.0 with the same result; the paste and hazard tables are 0.169.0.
- Claude Code on the Windows VM updated itself while it ran. The VM was reverted to the snapshot taken
  before the spike, and that snapshot was then deleted.
- Each run of a harness wrote its own session and state files in its own home directory (Claude,
  Codex, Cursor's login-free attempts); no configuration file of the user was edited.
- Not measured: Cursor's interface (login), `codex --remote` and the Codex app-server path, `cursor-agent
  persist`, Codex and OpenCode on Windows (the VM has Codex as a script and OpenCode not at all), Linux (the
  checks that need a pty and `/proc` are the same shapes, not run), the foreground shapes on Windows.

## The probe, quoted

The stdio server every macOS case used (a Python 3 script, started with the harness's own launch syntax;
it takes a tag and a log path; as a shell command, `probe_mcp.py shell-tool LOG` logs once and exits):

```python
#!/usr/bin/python3
import json, os, subprocess, sys, time

LOG = sys.argv[2] if len(sys.argv) > 2 else os.environ.get("PROBE_LOG", "probe-mcp.log")
VAR = "SOYEHT_PANE_BINDING"
TAG = sys.argv[1] if len(sys.argv) > 1 else "probe"

def ps(*args):
    return subprocess.run(["ps", *args], capture_output=True, text=True).stdout

def chain():
    """From this process up to launchd: each one's tty, and whether its own environment holds the binding."""
    rows, pid = [], os.getpid()
    while pid > 1 and len(rows) < 25:
        out = ps("-ww", "-o", "pid=,ppid=,pgid=,sess=,tty=,command=", "-p", str(pid)).strip()
        if not out:
            break
        cols = out.split(None, 5)
        env = ps("-ww", "-E", "-o", "command=", "-p", str(pid))
        rows.append({"pid": int(cols[0]), "ppid": int(cols[1]), "pgid": cols[2], "tty": cols[4],
                     "cmd": cols[5][:160], "has_binding": (VAR + "=") in env,
                     "binding": (env.split(VAR + "=", 1)[1].split()[0] if (VAR + "=") in env else None)})
        pid = int(cols[1])
    return rows

def controlling_tty():
    try:
        fd = os.open("/dev/tty", os.O_RDWR); name = os.ttyname(fd); os.close(fd); return name
    except OSError as e:
        return "none (%s)" % e.strerror

def safe(f, *a):
    try:
        return f(*a)
    except Exception as e:  # a sandboxed probe may not be allowed to run ps
        return "error: %s" % e

def log(event, **extra):
    rec = {"t": round(time.time(), 3), "tag": TAG, "event": event, "pid": os.getpid(), "ppid": os.getppid(),
           "binding": os.environ.get(VAR), "controlling_tty": safe(controlling_tty), "argv": sys.argv,
           "env_names": sorted(os.environ), "term": os.environ.get("TERM"), "chain": safe(chain)}
    rec.update(extra)
    with open(LOG, "a") as f:
        f.write(json.dumps(rec) + "\n")

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n"); sys.stdout.flush()

if TAG.startswith("shell"):          # run as a shell command: log once and leave
    log("shell-command"); sys.exit(0)
log("start")
for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    method, mid = m.get("method"), m.get("id")
    if method == "initialize":
        log("initialize", client=m.get("params", {}).get("clientInfo"))
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": m.get("params", {}).get("protocolVersion", "2024-11-05"),
            "capabilities": {"tools": {}}, "serverInfo": {"name": "probe", "version": "0.0.1"}}})
    elif method == "tools/list":
        log("tools/list")
        send({"jsonrpc": "2.0", "id": mid, "result": {"tools": [{"name": "probe_echo",
              "description": "Returns the pane binding this server sees.",
              "inputSchema": {"type": "object", "properties": {}}}]}})
    elif method == "tools/call":
        log("tools/call", tool=m.get("params", {}).get("name"))
        send({"jsonrpc": "2.0", "id": mid, "result": {"content": [
              {"type": "text", "text": "binding=%s" % os.environ.get(VAR)}]}})
    elif mid is not None:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "no such method"}})
log("stdin-closed")
```

The other pieces, in words and the lines that matter:

- **The pane**: `pty.fork()` runs `bash --norc --noprofile -i` with the clean environment above in a 120 by 40 pty;
  a reader thread feeds a terminal-emulator library, records every `ESC[?Nh` and `ESC[?Nl` (that is how DECSET 2004 was read) and
  answers the queries a TUI waits on (`ESC[?u`, `ESC[>q`, OSC 10 and 11). The harness is typed into the shell as a command line.
  Input is `ESC[200~` + text + `ESC[201~` in one `write`; the foreground group is `os.tcgetpgrp(master_fd)` and the
  executable is `proc_pidpath` through `libproc`.
- **The editor stand-in** (`EDITOR` and `VISUAL`), which makes a harness hand over its input buffer:

  ```sh
  #!/bin/bash
  mkdir -p "$DUMP_DIR"; cp "$1" "$DUMP_DIR/editor-$(date +%s).txt"; exit 0
  ```

- **OpenCode isolation**: `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME` and `XDG_CACHE_HOME` in a scratch
  directory, a project `opencode.json` with `{"mcp": {"probe": {"type": "local", "command": [PROBE, "opencode", LOG], "enabled": true}}}`,
  `opencode service set port NNNNN` to a free port, and `opencode service start` from a shell with no variable
  to have a service that is not any pane's. The user's own service was never touched.
- **Windows**: one C# program, compiled on the VM by `Add-Type`, in two roles. As host it calls
  `CreatePipe` twice and `CreatePseudoConsole`, builds a `STARTUPINFOEX` with
  `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`, and sets `dwFlags = STARTF_USESTDHANDLES` with the three handles
  `INVALID_HANDLE_VALUE`; the creation flags are `EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT`.
  It writes the paste with one `WriteFile`. As child it switches stdin with `SetConsoleMode` to
  `ENABLE_VIRTUAL_TERMINAL_INPUT` (or leaves it plain), writes `ESC[?2004h` and `READY` to the console, reads
  `ReadFile` until a second of silence, and writes what it got, its own `SOYEHT_PANE_BINDING` and a grandchild's
  (`cmd /c echo %SOYEHT_PANE_BINDING%`) to a file. A second role, `run`, starts any command line in a
  pseudoconsole and plays a script of `waitfor`, `send`, `sendhex` and `paste` steps, which is how Claude
  Code was driven there. The Windows probe MCP server is the same idea in Python: it logs `os.environ`, the
  process table from `Get-CimInstance Win32_Process` and `GetConsoleProcessList`.
