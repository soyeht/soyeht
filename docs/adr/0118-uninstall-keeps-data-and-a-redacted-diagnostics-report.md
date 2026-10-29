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
components, followed, when asked, by `soyeht_service::remove_data`. The data stays by default.
Deleting it needs the profile's name typed at a terminal prompt (asked before any service is
touched), or `--yes` for a script; `--yes` alone is refused. `remove_data` carries the rules that
make an irreversible delete safe, in one place: the legacy and "next" guard of every definition,
no relative or dotted path, no link, never a folder that holds the home folder, nothing while the
engine or the supervisor still answers on its endpoint, and the data directory of another profile
that lies inside this one (Dev inside prod) is kept. It is a command of its own rather than a
flag of `service uninstall` because it is a person's decision about their data, with a prompt,
and `service` stays the scriptable seam with no data behaviour.

**`soyeht diagnostics [--lines N]`** prints one Markdown report. It is the command line, not an
engine method, because the case that matters is an engine that does not run, and a wire method
would need the engine. The gathering lives in `soyeht_platform::diagnostics`, which every
client can call without a new layer: the report builder takes the facts the caller has
(version, profile, whether the programs run, the readiness items) and reads the profile's logs,
and `Redactor` takes out the home folder, the user name, every path, link and address, secret
fields and token-shaped runs. A client that wants a "Copy diagnostics" action calls the same
function; the Mac app, which has no Rust, would run the command or wait for a
new engine method (a follow-up: it starts in `contracts/protocol.json` and
`docs/contracts/`).

## Consequences

- A launch installer calls `soyeht uninstall`, and `--delete-data --yes` when the person ticked
  the box.
- `docs/contracts/14-diagnostics.md` lists what the report holds and what is removed. The
  redaction errs toward removing: a long mixed-case word or a two-folder relative path is lost
  even when harmless.
- The report does not say whether the services are registered; that needs the service manager's
  tool, and `soyeht service status` answers it.

## Not verified

The service-manager command sequences of `uninstall` are the ones ADR 0114 tested against a
fake runner; no real `launchctl`, `systemctl` or `schtasks` ran for this change. Data deletion
was tested on the Mac and Linux path shapes with temporary folders; Windows paths were tested as
text for the redaction only.
