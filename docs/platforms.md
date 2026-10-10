# Platforms

The engine (`engine/`, the Rust workspace) builds and passes its tests on macOS,
Linux and Windows. The apps are macOS and iOS only; the engine is what other
platforms need first.

| Platform | Engine builds | Engine tests | Check script |
| --- | --- | --- | --- |
| macOS (Apple silicon) | yes | yes | `scripts/check_all.sh` |
| Linux (Ubuntu 26.04, arm64) | yes | yes | `scripts/check_engine_linux.sh` |
| Windows (11 24H2, arm64) | yes | yes, see "Tests" | `scripts/check_engine_windows.ps1` |

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

## Running the Linux check

On a Linux machine with `build-essential`, `git`, `bash` and `rustup`:

```sh
bash scripts/check_engine_linux.sh
```

It runs `cargo fmt --check`, `cargo clippy -D warnings`, `cargo build` and
`cargo test` for the whole workspace, with `SOYEHT_JOBS` (default 4) build jobs.
The pinned toolchain in `rust-toolchain.toml` installs itself on first use.

## Running the Windows check

On Windows with rustup, the Visual Studio C++ build tools (MSVC and a Windows SDK),
`git`, and, on ARM64, LLVM (`winget install LLVM.LLVM`, for the C code of `ring`),
in an elevated PowerShell (symbolic links):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check_engine_windows.ps1
```

It runs the same four steps as the Linux script and first checks that the account can
create symbolic links.

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
