# ADR 0118: Uninstall keeps the data unless asked, and one redacted report goes into an issue

- Status: accepted
- Date: 2026-10-29

## Context

Two gaps stood between a working install and a launch. Nothing removed the login services, the
data or the supervisor in one step: `soyeht service uninstall` (ADR 0114) removed the
registration and kept the data, with deleting the data left to "the uninstaller to ask", which
did not exist. And a person whose Soyeht broke had nothing to send us: the logs are bounded,
rotating and free of paths (ADRs 0082, 0088, 0101), but nobody gathers them, and a log line can
still carry more than it should.

## Decision

**`soyeht uninstall [--delete-data [--yes]]`** is the command an uninstaller or a person runs,
beside the low-level `soyeht service uninstall`. It is the same `Manager::uninstall` of both
components, followed, when asked, by the deletion of the data. The data stays by default.
Deleting it needs the profile's name typed at a terminal prompt, or `--yes` for a script; `--yes`
alone is refused. It is a command of its own rather than a flag of `service uninstall` because it
is a person's decision about their data, with a prompt, and `service` stays the scriptable seam
with no data behaviour.

The deletion is irreversible, so it is built to be refused rather than wrong
(`soyeht_service::plan_removal`, then `Removal::execute`):

- **Only the profile's own folder.** The first version accepted any path with "next" in it, which
  would have deleted `~/nextcloud`. The directory must now equal, after canonicalizing both
  (`soyeht_platform::fs::canonicalize`), the folder `contracts/profiles.json` names below the
  system's folder for application data. A custom `--data-dir` is refused, whatever its name.
- **Canonical paths for every check.** The legacy and "next" guard, the home-folder check and
  the keep list read canonical paths, so a link in the path, or the home spelled another way,
  cannot defeat them; the canonical path is what is deleted. A data directory that is itself a
  link is refused. A link inside is removed, never followed (a link to a folder on Windows is
  removed as a folder).
- **Check, confirm, unregister, wait, delete.** The plan changes nothing and runs before the
  prompt and before any service is touched, so a refusal leaves the services registered. After
  unregistering, the command polls the endpoints for up to 15 seconds for the programs to stop;
  one that still answers (a copy the app started) keeps the data, and the message says the
  services are already unregistered and to run the command again.
- The data directory of another profile that lies inside this one (Dev inside prod) is kept.

**`soyeht diagnostics [--lines N]`** prints one Markdown report. It is the command line, not an
engine method, because the case that matters is an engine that does not run, and a wire method
would need the engine. The gathering lives in `soyeht_platform::diagnostics`, which every
client can call without a new layer: the report builder takes the facts the caller has
(version, profile, whether the programs run, the readiness items) and reads the profile's logs,
and `Redactor` takes out the home folder, the user name, the machine's name and addresses, every
path (an unquoted one through the blanks of its folder names), link and e-mail address, secret
fields (tokens, pairing codes, PINs, keys, sessions) and token-shaped runs, and keeps the ids of
panes and workspaces. The readiness family and item ids pass through it too. A client that wants a "Copy diagnostics" action calls the same
function; the Mac app, which has no Rust, would run the command or wait for a
new engine method (a follow-up: it starts in `contracts/protocol.json` and
`docs/contracts/`).

## Consequences

- A launch installer calls `soyeht uninstall`, and `--delete-data --yes` when the person ticked
  the box.
- `docs/contracts/14-diagnostics.md` lists what the report holds and what is removed. The
  redaction errs toward removing: a long mixed-case word, a two-folder relative path, or the words
  that follow an unquoted absolute path on its line are lost even when harmless. One token shape
  is kept on purpose: a ULID standing alone, like a UUID, because a pane id is more useful in an
  issue than the rare random key that looks like one.
- A custom `--data-dir` cannot be deleted by `soyeht uninstall`. A test or a script that wants it
  gone deletes it itself.
- The report does not say whether the services are registered; that needs the service manager's
  tool, and `soyeht service status` answers it.

## Not verified

The service-manager command sequences of `uninstall` are the ones ADR 0114 tested against a
fake runner; no real `launchctl`, `systemctl` or `schtasks` ran for this change. Data deletion
was tested on the Mac with temporary folders (links, a listening socket, the nested Dev profile);
the Windows link removal compiles and has a test that runs only on Windows, which has not been
run, and Windows paths were tested as text for the redaction only.
