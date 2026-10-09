# Platforms

The engine (`engine/`, the Rust workspace) is built to run on macOS, Linux and,
in a later change, Windows. The apps are macOS and iOS only; the engine is what
other platforms need first.

| Platform | Engine builds | Engine tests | Check script |
| --- | --- | --- | --- |
| macOS (Apple silicon) | yes | yes | `scripts/check_all.sh` |
| Linux (Ubuntu 26.04, arm64) | yes | yes | `scripts/check_engine_linux.sh` |

Behaviour that exists on only some systems sits behind `cfg` and keeps the same
public shape everywhere. Where a feature cannot exist on a system, it returns a
typed "unsupported" error; it never does nothing silently. What differs on
Linux today:

- A local client is accepted when it runs as the engine's own user. Only macOS
  can also prove that the peer is the signed Soyeht app, so on Linux no local
  client counts as a signed UI and the consent-changing methods
  (`pane.experiment.*`) stay refused (ADR 0085).
- Moving a project entry to the trash is unsupported (the caller asks before
  deleting permanently). Reading the macOS keychain and browser cookies for
  usage sources is unsupported; those sources report it as such.
- A new terminal pane starts `$SHELL` as a login shell (macOS always starts
  `zsh`).
- The tailnet address watcher recognises the `tailscale0` interface.

## Running the Linux check

On a Linux machine with `build-essential`, `git`, `bash` and `rustup`:

```sh
bash scripts/check_engine_linux.sh
```

It runs `cargo fmt --check`, `cargo clippy -D warnings`, `cargo build` and
`cargo test` for the whole workspace, with `SOYEHT_JOBS` (default 4) build jobs.
The pinned toolchain in `rust-toolchain.toml` installs itself on first use.

## Running it through VMPal

[VMPal](https://vmpal.com/docs/command-line) runs the check inside a VM from the
Mac, so the same commit is tested on both systems:

```sh
scripts/vmpal_check_engine.sh linux "Ubuntu VM 1"
```

The script sends the committed engine sources (`git archive HEAD`, so local
changes and `target/` stay out), unpacks them over `soyeht-check/` in the VM
account's home, and runs the check there. The build products stay in the VM, so
a second run is incremental. Set `VMPAL_CLI` to use a VMPal command line other
than the default install path.

One-time VM setup (Ubuntu): `apt-get install build-essential git curl`, then
rustup with the profile `minimal`. Take a snapshot first so the VM can be
returned to a clean state: `vmpal snapshot "Ubuntu VM 1" <name>`.

The scripts are deliberately not steps of `scripts/check_all.sh`, which stays
the single merge-gate list and runs on macOS. To make Linux a gate step later,
add one line that calls `scripts/vmpal_check_engine.sh linux "<VM name>"` (or
run `scripts/check_engine_linux.sh` on a Linux runner) to `check_all.sh`, and
update `test_gate_list.py` expectations together with it.
