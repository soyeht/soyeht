# ADR 0116 · The platform seam, finished: private directories, publishing, the shell

Date: 2026-10-28
Status: accepted. Extends ADR 0086 and ADR 0113: each operating-system rule has one home in
`soyeht-platform`.

## Context

Three rules were still written more than once above the seam:

- A private directory (created owner-only, never a link in its place) had four copies: the
  service's `private_folder`, the engine's experiment export, the engine's pane observations,
  and the pairs of `create_dir_all` and `set_private_dir` that skipped the link check. The copies
  disagreed: one repaired a directory that others could reach, two refused it, and only one
  created the parents.
- Publishing a new file with an exclusive hard link had two copies, `immutable_file::publish`
  and the private `experiment_export::write_atomic` (a name that also belongs to
  `soyeht_platform::fs::write_atomic`, whose contract is the opposite). Only the first fell back
  to a rename on file systems without hard links (FAT, exFAT, some network shares).
- The shell a person starts was chosen twice. The supervisor took `$SHELL` when it named an
  existing file, then the account's shell unless it was `nologin` or `false`, then `bash`, then
  `sh`. The engine's probe of the login shell's `PATH` took `$SHELL` as it was, stale or not,
  and the account's shell as it was, `nologin` included, so one host could start one shell in
  its terminals and ask another for its `PATH`.

## Decision

`soyeht_platform::fs::ensure_private_dir(path)` creates the directory with its parents when it is
missing and makes it owner-only when it exists and others can reach it (`set_private_dir`: on
Windows the protected ACL). On Unix the repair opens the directory with `O_NOFOLLOW |
O_DIRECTORY` and sets the mode on that handle, so a link swapped in after the check is never
followed and its target never changed; on Windows it goes by path, so the parent of the directory
must be one only the owner can change. A link, or an entry that is not a directory, in its
place is refused with `PrivateDirError::Unsafe`, never followed and never replaced. `check_private_dir(path)` is
the same judgment without changing anything, for the readers that must not create. The error is
typed so a caller can tell a link from an I/O failure; the engine maps it to `internal_error` and
the service to `Error::Unsafe`.

`soyeht_platform::fs::publish_new(target, bytes, mode)` is the one way to publish a file that
must not replace anything. A regular file already at the target with the same bytes is accepted
at once, before anything is written, so a retry works in a directory that has become read-only.
Otherwise it writes a temporary file beside the target (`TEMPORARY_PREFIX`, so
`is_temporary_name` hides it everywhere), syncs it, closes it, and links it with an exclusive
hard link. Anything else already at the target, a link included, is `AlreadyExists`. A link that fails for any other reason is logged by kind and the
temporary file is renamed to the target when nothing is there. `WriteMode::OwnerOnly` makes the
file owner-only from its creation and accepts an existing file only if it is owner-only too.

`soyeht_platform::user::preferred_shell()` is the shell choice: a usable `$SHELL`, then the
account's shell unless it refuses logins, then `bash`, then `sh`; "usable" is an existing
absolute file whose path is text (a path that is not UTF-8 is never chosen). The account is read once per process and only when `$SHELL` is not usable, and
`warm_default_login_shell` (called by `Core::open` before the lock) reads it at start, so neither
a terminal started under the Core lock nor a probe waits for the user database. It returns
`Unsupported` on Windows and `NotFound` on a Unix host that has not even a `sh`. The supervisor keeps its own platform policy on top (macOS always starts `zsh`,
Windows the `COMSPEC` interpreter); the engine's probe takes the preferred shell and judges only
whether it can ask it (`bash`, `zsh`, `fish`).

## Consequences

- The engine's experiment directories now repair a directory that others can reach instead of
  refusing it. They are made by the engine inside the owner-only profile, so the only way for
  one to be open is that someone opened it; closing it again is the safe answer.
- The temporary of a publish is `.soyeht-write-*`. The engine no longer knows
  `.soyeht-publish-*`; a leftover of that name from an earlier build is an ordinary file. The
  experiment export cleanup still removes the `.export-<uuid>.tmp` names of earlier builds, and
  the new ones.
- An export on a file system without hard links now succeeds by rename, as an attachment does.
- A stale `$SHELL` or a `nologin` account shell no longer makes the `PATH` probe fail with an
  environment issue; it asks the preferred shell. On Linux that is the shell a terminal starts.
  On macOS a terminal always starts `zsh` (the supervisor's policy, kept), so the probe may ask
  another shell than the terminal runs: the one the person's startup files configure, which is
  where their `PATH` lives.
- `LaunchPathIssue::MissingShell` stays: it is reached when `preferred_shell` finds no shell at
  all (no `sh` on a Unix host), and it maps to the contract's `missing_shell` issue. It is no
  longer reached for an account with no shell or a failed account lookup; those now fall back.
- `lock_singleton`, the log directory and a few other `set_private_dir` callers still create
  with the default mode and tighten afterwards; they do not refuse a link and are left for a
  separate change.
