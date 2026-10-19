# ADR 0097: The editor's autosave and disk watch run off the Core lock

- Status: accepted
- Date: 2026-10-19

## Context

ADR 0095 took the terminal's disk flush out of the engine tick. The editor still did its disk
work inside the tick (`Core::poll_output`, `advance_editor`), under the Core lock:

- An autosave read the file, then `SystemFiles::write_text_if_unchanged` wrote a temporary
  file, ran `sync_all`, renamed it over the target and flushed the directory.
- The disk watch read every watched open file again every 250 ms (`WATCH_INTERVAL_MS`).

A flush, a rename and a directory flush take 10 to 36 ms on an idle disk and seconds under
heavy writes. While one ran, every request and the terminal output of every pane waited, the
failure ADR 0079 forbids and ADR 0095 fixed for terminal output.

## Decision

**The tick starts the work and a later tick applies it.** For each file whose autosave or
watch is due, the tick copies the inputs under the lock (the workspace folder, the relative
path, the text the editor believes the disk holds, and for a save the draft) and starts a
background job with `Core::spawn_background` on a thread of its own (`Background::run_alone`), so a disk that hangs
holds that file's job and no shared worker (the terminal's flushes use those, ADR 0095). The
copy holds the draft once and the disk text once; the checks that decide whether a result still
applies use only the file's path, disk revision and conflict flag. The job resolves the path, reads the file,
compares it, and for a save writes it with `write_text_if_unchanged`. It touches no editor
state. `apply_background` hands its result to `apply_disk_result` under the lock on a later
tick. The code is `editor/disk_effects.rs`.

**One job per file.** The save and the watch of a file share one background key per file, so
at most one write is in flight for a file, and a watch never reads half of a save. A file whose
job still runs stays due and is looked at again on the next tick.

**Moving, deleting or closing a file while its job runs.**

- `project.entry.rename` and `project.entry.delete` that meet a job working on a file they move
  or remove are offloaded (`project_entry_request`, ADR 0079): the request waits off the Core
  lock for the job's signal (`Done`, at most 1.5 s), then runs again under the lock, applying
  the finished jobs first. Without the wait, a write that is renaming its temporary file over
  the old path recreates the file after it was moved, and the saved text lands in the wrong
  place. Only the person who renamed waits; other requests and the terminal keep flowing
  (`tests/slow_disk.rs`). If the disk does not answer in time, the request is refused as
  `stale_version` and nothing moves; the person never sees that for a save that is merely
  slow. An offload is resolved once, so the apply step cannot offload again: if another job
  started meanwhile (the person typed during the write and the next autosave began), the
  second pass refuses as `stale_version` instead, and the handlers refuse the same way for a
  job that started after it. The jobs are found by their keys, so the job that saves a closed
  tab (whose file has no open tab or live pane) is waited for too.
- A tab that closes while its job runs does not wait. The file leaves the open set; when the job
  ends, its result is applied to the file's stored row, and a second job of its own saves what
  is left of the draft, typed after the first job began, as closing a tab promises. That job is
  applied like any other result. No false conflict appears when the file is opened again.
- A result for a file whose path changed since the job began is dropped (`Basis`), so a moved
  file is never marked saved by a write that went to its old place.

**One writer per file between comparison and rename.** The same folder can be open in two
workspaces, which gives two editor files, two keys and two concurrent jobs on one file. The
Core lock used to order their writes; `write_text_if_unchanged` now takes a lock per canonical
path (`editor/write_lock.rs`) around its comparison and rename, so the second writer sees the
first one's text and reports a change, which becomes a conflict instead of an overwrite.

**The result is judged against the file as it is when the job ends**, not as it was when the
job began:

- *Written.* The disk now holds the draft that was copied. The saved version is that text at
  the revision that was copied, with its author. If the draft is still that text, it is cleared
  as before. If the person typed meanwhile, the newer draft keeps its revision and stays dirty,
  and the next autosave writes it. `file.saved` is published with the document as it is now.
- *Changed.* The job found the disk different from what the editor knew, before writing, or
  because a write reported that the file changed in between (`write_text_if_unchanged` returned
  false; nothing was overwritten). The usual rules of contract 09 apply to the file as it is
  now: a draft equal to the disk finishes the save, a different draft is a conflict
  (`file.conflict`, both versions kept), and no draft reloads the disk. If the editor's known
  disk text changed since the job began (a revert, a reopen), the result is dropped and the
  next attempt reads again.
- *A change after the rename.* It is not seen by the job. The next watch or the next save reads
  the disk against the text just written, and finds a conflict or a reload. Nothing is lost:
  the written text is in the file's history.
- *The file closed meanwhile.* See above: the stored row is judged and finished.

A failing job backs off exactly as before (`schedule.rs`, `Streak`); its log lines are
collected by the editor service and returned by the next tick.

**The test seam is the existing one.** `Hooks::editor_disk` lets the test engine give the editor
a disk whose writes take `SOYEHT_TEST_SLOW_DISK_MS`, the pause that already delays a terminal
segment's flush (`soyeht_store::segments::slow_disk`). No production code reads it.

## Consequences

- Requests and terminal output keep flowing while an autosave waits on the disk or a watched
  file is slow to read. `tests/slow_disk.rs` fails on the old code: the longest stretch without
  a terminal frame and the slowest `engine.status` were as long as the save.
- An autosave reaches the disk one tick later than before, and `file.saved` follows the write.
- Still under the lock, because a person waits for the answer: `save_editor_now` (the save of a
  pending draft before a delete, when no job is running) and the project entry requests
  themselves (create, rename, delete: one filesystem change each).
- The jobs run on threads of their own (`Background::run_alone`), one per open file at a time,
  so their number is bounded by the number of open files. A hung mount holds one such thread
  per hung file for as long as it hangs, and nothing else: the shared workers stay free. The
  job is not cancelled (a blocked system call cannot be): that file is not saved or watched
  until the disk answers, a rename or delete of it is refused as `stale_version` after 1.5 s,
  and every other file and request keeps working. A cap on the threads is possible if open
  files on dead mounts ever become common.
