# ADR 0113 · One atomic write, in the platform seam

Date: 2026-10-28
Status: accepted. Extends ADR 0086, which put owner-only files in `soyeht-platform`.

## Context

Seven places wrote a temporary file and renamed it over the target, each with its own
choices: the local endpoint file wrote its secret with default permissions and made it
owner-only afterwards, so the secret was briefly readable by others; the launch-path cache
and the desktop preferences used a fixed temporary name that a planted link could redirect;
the skill store did the same with a fixed name and no fsync; the registries and the editor
save were correct but each was its own copy.

## Decision

`soyeht_platform::fs::write_atomic(target, bytes, mode)` is the one way to replace a file by its
path in one step. Three writers are kept apart on purpose (see "Kept apart"); every other
temporary-file rename in the engine and the desktop client was replaced by it. It creates a temporary file in the target's directory under a unique name with
`create_new` and without following a link, writes, syncs, renames over the target, syncs the
directory, and removes the temporary file on any error. `stage_replacement` is the same in two
steps, for the callers that must do something between the write and the rename: check that
nobody else changed the target (the harness registry) or lock the file that is about to be
published (the Windows endpoint file). The staged file removes itself when dropped.

`WriteMode` says who may reach the result:

- `OwnerOnly`: mode 0600 from the creation of the temporary file on Unix; on Windows the
  temporary file gets the owner-only ACL before its first byte. The result is owner-only even
  when it replaces a file that was not. On Windows a file takes its directory's access list
  until `set_private_file` runs, and a handle another user opened in that instant keeps its
  access (the default share mode allows it). The guarantee therefore holds because every
  `OwnerOnly` caller writes only into a private directory (`set_private_dir`): the engine and
  supervisor data directories, and the desktop preferences directory, which is made private
  before the write.
- `KeepPermissions`: the permissions of the file being replaced, for files a person owns (an
  open document, a skill). On Unix the temporary file is owner-only until the permissions are
  copied, so the text of a private file is never more readable than the file was. On Windows
  there is no per-open mode, so the temporary file has its directory's access list until then;
  only the read-only attribute is copied.

The temporary file's name starts with `TEMPORARY_PREFIX`; `is_temporary_name` is the one rule
the editor's listing, search and file events use to hide it (with the publish temporary of
ADR 0107).

## Kept apart

- `immutable_file::publish` and `experiment_export::write_atomic` publish with an exclusive
  hard link and never overwrite; their contract is the opposite of a replacement.
- `agents/filesystem` replaces through a directory handle (`renameat`), so no component of an
  agent-supplied path is followed, and keeps hard links and attributes; it needs the handle,
  which a path-based helper does not have.
- Log rotation renames a file aside; it writes nothing new.

## Consequences

A crash between the creation of the temporary file and the rename leaves one `.soyeht-write-*`
file. The only sweep is `immutable_file::sweep_stale` (ADR 0107), which runs in publish folders
and removes such a file after an hour. The data directory, the skills directory, the supervisor
directory and the desktop preferences directory are never swept, so a leftover stays there. They
are private directories, so it is only an owner-only copy of what the target held (for the
endpoint file, a copy of a token that died with its listener). Earlier fixed names were
overwritten by the next write; a sweep for these directories is left for later. Windows' rename over an existing file is std's
(`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`): it fails on a read-only target or one held
open without delete sharing, which `write_atomic` reports as the error.
