# Platforms

The engine (`engine/`, the Rust workspace) builds and passes its tests on macOS,
Linux and Windows. The native apps are Swift on macOS and iOS; Linux and Windows
have the desktop client (`apps/desktop`, Rust with GPUI, ADR 0084), which the same
check scripts format-check, lint and test.

| Platform | Engine builds | Engine tests | Desktop client | Check script |
| --- | --- | --- | --- | --- |
| macOS (Apple silicon) | yes | yes | builds (development only) | `scripts/check_all.sh` |
| Linux (Ubuntu 26.04, arm64) | yes | yes | yes | `scripts/check_engine_linux.sh` |
| Windows (11 24H2, arm64) | yes | yes, see "Tests" | yes | `scripts/check_engine_windows.ps1` |

Behaviour that exists on only some systems sits behind `cfg` and keeps the same
public shape everywhere. The calls that differ for the local endpoint, owner-only
files and the user's identity live in `engine/soyeht-platform` (ADR 0086). Where a
feature cannot exist on a system, it returns a typed "unsupported" error; it never
does nothing silently.

## What differs

**Linux** (ADR 0085)

- A local client is accepted when it runs as the engine's own user. Only macOS can
  also prove that the peer is the signed Soyeht app, so on Linux and Windows no local
  client counts as a signed UI and the consent-changing methods (`pane.experiment.*`)
  stay refused.
- Moving a project entry to the trash is unsupported (the caller asks before deleting
  permanently). Reading the macOS keychain and browser cookies for usage sources is
  unsupported; those sources report it as such.
- A new terminal pane starts `$SHELL` as a login shell (macOS always starts `zsh`).
- The tailnet address watcher recognises the `tailscale0` interface.

**Windows** (ADR 0086)

- The local endpoint is a loopback port guarded by a secret in an owner-only file
  (contract 08, section 2.5), not a Unix socket. Client and listener prove to each
  other that they know the secret without sending it.
- The skill folders are linked into each harness's folder with a directory symbolic
  link when the account may create one and with a junction otherwise, so skills need
  no special privilege on Windows.
- Terminals run on a pseudo console (ConPTY). Every process the supervisor starts
  lives in a job object that ends with the supervisor, so no guardian process exists
  and closing a terminal also ends processes the person detached from its shell.
- Data folders get a protected ACL for the user and the system. Agent file access
  opens each folder and file as the link it may be and refuses links; it cannot be
  descriptor-relative, so a folder swapped for a link between two steps is the one
  race it cannot close.
- Unsupported, with a typed result: the list of local servers (`lsof`; the poller
  never scans and `servers.local.list` answers `platform_unsupported`), cleaning a
  task worktree (the processes inside it cannot be listed), moving to the trash,
  reading the macOS keychain and browser cookies, the signed-UI check.
  Writing over a read-only file is refused.
- The default shell is `%COMSPEC%`. The engine does not ask a login shell for `PATH`.

## Tests

Tests that need a POSIX shell, `ps`, `perl`, permission modes, or a shell script as
a fake program run on Unix only (they are marked `cfg(unix)`). The fakes the engine
rig needs are Rust programs, so the rig itself runs on Windows. Windows has its own
supervisor tests for terminals, pipe processes and jobs
(`engine/soyeht-ptyd/tests/supervisor_windows.rs`). Tests that create symbolic links
need the right to create them on Windows (Developer Mode, or an elevated shell).

## Agents messaging each other

An agent pane of one harness can message a pane of another on every system, the same
way as on the Mac. Each ACP session gets the engine's own MCP server
(`soyeht-workspace`, the `soyeht-mcp` program installed next to the engine) in
`mcpServers`; the program connects to the engine through `soyeht_platform::local`
(the Unix socket, or on Windows the loopback endpoint file), so nothing about it differs
per system. A pane calls `list_agents` and `message_agent`; the engine queues the text,
and the recipient's harness receives a peer prompt naming the sender and the reply route
once its input is clean. The engine refuses to open an agent whose `soyeht-mcp` is not
installed next to the engine, so build the whole workspace (`cargo build --workspace`),
not only `soyeht-engine`.

`engine/soyeht-engine/tests/agent_messaging.rs` proves the exchange end to end between
two fake harnesses named like Codex and Claude: each calls `list_agents`, a `message_agent`
from either reaches the other as an attributed peer turn, and the answer comes back the
same way. It is part of `cargo test` and so of both check scripts.

The real-harness checks (`scripts/e2e_real_acp.py`, `e2e_real_agent_messaging.py` and
`e2e_real_harnesses.py`) run on every system. Their services start through one seam,
`scripts/e2e_platform.py`: launchd on macOS, plain child processes elsewhere
(`SOYEHT_E2E_LAUNCHER=process|launchd` forces one), and a client reaches the engine
through the Unix socket or, on Windows, the loopback endpoint file of contract 08, section
2.5. They are opt-in (`SOYEHT_REAL_ACP=1`) and need the person to sign Claude Code and
Codex in first; a harness that is not signed in is skipped by name in
`e2e_real_harnesses.py` (Claude's adapter starts `soyeht-mcp` even when signed out,
Codex's creates no session until it is signed in). That script proves, with one-line
prompts, a real reply from each harness, a pane switched Claude to Codex and back, and
Codex and Claude messaging each other through `list_agents` and `message_agent`. On the
Mac run it with `SOYEHT_REAL_ACP=1 uv run python scripts/e2e_real_harnesses.py`; from a
Mac into a VM use `scripts/vmpal_check_real_agents.sh linux|windows "<VM>"` (the VM needs
Node.js 22 or later and Python 3.10 or later besides the engine prerequisites).

## Running the Linux check

On a Linux machine with `build-essential`, `git`, `bash` and `rustup`:

```sh
bash scripts/check_engine_linux.sh
```

It runs `cargo fmt --check`, `cargo clippy -D warnings`, `cargo build` and
`cargo test` for the whole workspace, with `SOYEHT_JOBS` (default 4) build jobs, then
`fmt`, `clippy` and `test` for the desktop client's own workspace (`apps/desktop`).
The pinned toolchain in `rust-toolchain.toml` installs itself on first use.

The desktop client compiles GPUI, which needs `cmake`, `clang` and the development
libraries of Wayland, X11, xkbcommon, fontconfig, freetype, OpenSSL and ALSA (on
Ubuntu: `libwayland-dev libx11-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev
libxcursor-dev libxrandr-dev libxi-dev libfontconfig-dev libfreetype-dev libssl-dev
libasound2-dev`; on Fedora the same names ending in `-devel`). To run it, the system
needs a Vulkan driver (`mesa-vulkan-drivers`); its tests open no window.

### Graphics on Linux

The window is drawn through Vulkan (OpenGL when there is no Vulkan driver). A machine with
a card needs its Mesa or vendor driver. A machine without one (a virtual machine, a remote
session) draws on the processor through Mesa's software renderer, llvmpipe, which is part of
the Vulkan driver package:

| Distribution | Package |
| --- | --- |
| Fedora | `sudo dnf install mesa-vulkan-drivers` |
| Ubuntu | `sudo apt install mesa-vulkan-drivers` |
| Arch | `sudo pacman -S vulkan-swrast` |

Keep Mesa up to date. Fedora 44 on arm64 with Mesa 26.0.3 and LLVM 22.1.8 (what a fresh
install holds after its updates) crashes with a segmentation fault inside LLVM, in
`llvmpipe_update_fs`, while compiling the first shader the window draws, so the window never
appears. `sudo dnf upgrade mesa-vulkan-drivers` (Mesa 26.2.4) fixes it. `vulkaninfo --summary`
shows the driver in use. The client prints one line to stderr when it draws on a software
renderer, naming it and its Mesa version, and when no driver can open the window it prints the
packages above.

## Running the Windows check

On Windows with rustup, the Visual Studio C++ build tools (MSVC and a Windows SDK),
`git`, and, on ARM64, LLVM (`winget install LLVM.LLVM`, for the C code of `ring`),
in an elevated PowerShell (symbolic links):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check_engine_windows.ps1
```

It runs the same four steps as the Linux script, then the desktop client's three, and
first checks that the account can create symbolic links.

## Running them through VMPal

[VMPal](https://vmpal.com/docs/command-line) runs the check inside a VM from the
Mac, so the same commit is tested on every system:

```sh
scripts/vmpal_check_engine.sh linux   "Ubuntu VM 1"
scripts/vmpal_check_engine.sh windows "Windows VM 1"
```

The script sends the committed engine sources (`git archive HEAD`, so local changes
and `target/` stay out) and unpacks them into a fresh source folder named after the
commit, `soyeht-check/src-<short sha>` in the VM account's home, so a file that an
earlier commit had and this one deleted can never leak into a run. All runs share one
build folder, `soyeht-check/target` (`CARGO_TARGET_DIR`), so dependencies are built
once and a second run is incremental. Old `src-*` folders stay until someone deletes
them in the VM. Set `VMPAL_CLI` to use a VMPal command line other than the default
install path.

One-time VM setup. Take a snapshot first so the VM can return to a clean state:
`vmpal snapshot "<VM>" <name>`.

- Ubuntu: `apt-get install build-essential git curl`, then rustup with the profile
  `minimal`.
- Windows: `winget install Microsoft.VisualStudio.2022.BuildTools` with the
  `VCTools` workload and the ARM64 tools, `winget install Git.Git`, `LLVM.LLVM` on
  ARM64, and rustup (`aarch64-pc-windows-msvc` host). The check needs symbolic links,
  so the VMPal exec must be elevated: the script runs it with `--admin`.

The scripts are deliberately not steps of `scripts/check_all.sh`, which stays the
single merge-gate list and runs on macOS. To make Linux and Windows gate steps later,
add two lines that call `scripts/vmpal_check_engine.sh linux "<VM>"` and
`... windows "<VM>"` (or run the two check scripts on Linux and Windows runners) to
`check_all.sh`, and update `test_gate_list.py` expectations together with them.
