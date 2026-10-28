# ADR 0107: The engine tells the clients why a save failed

- Status: accepted
- Date: 2026-10-24

## Context

The engine saves an edited file on its own, shortly after the last edit (ADR 0097), and
retries a failing save with the backoff of a `Streak` (`editor/schedule.rs`: the wait doubles
from the autosave delay up to 60 s, and the log line is written at attempts 1, 2, 4, 8 and so
on). A client could not tell that a save was failing. The wire had no word for it: a file was
`dirty`, and stayed `dirty`. A person who deleted an open file, typed, and pressed save saw
only "unsaved". The desktop client then waited for the file to stop being dirty, gave up after
three seconds and showed its own timeout as "The engine sent an invalid message", which blames
the protocol for a missing file. The Mac client waited the same way and showed a bare deadline
error. Neither said what had gone wrong.

## Decision

**The engine reports a failed save, with a reason a person can act on.** `EditorSaveFailure` is
`missing` (the file or its folder is gone), `denied` (the system refuses the write, or the file
is read-only), `full` (no room on the disk) or `other`. The engine classifies the I/O error
(`editor/save_failure.rs`); the error's text, which can carry a path, never leaves the engine.

- The non-durable event `file.save_failed {fileId, reason}` is sent when the save's failure
  streak reports (the engine `Streak`): the first failure, each change of kind, and attempts 2, 4,
  8 and so on, not at every retry. A save the person asks for starts the streak over. It carries
  no cursor and is not replayed. A client sets its state from the event and from the page it
  reads, so none depends on getting it at each retry.
- `FileReadResult.saveFailure` carries the same reason while the open file's last attempt has
  failed, so a client that reads the file (or connects after the event) learns it. The page of
  `file.open` for a file that is already open and failing carries it too. It is on the
  read and nowhere else: a `FileDocument` is built in many places (writes, undo, formats, the
  snapshot) that do not know the state, and a field that means "no failure" in one answer and
  "not told" in another would be wrong. It ends when an attempt succeeds (`file.saved`) or the
  person asks to save again.
- A client keeps the reason until the file is saved or the engine's next read says it is not
  failing or has nothing to save; typing does not end it. The rule is one vector,
  `contracts/vectors/editor.json` (`saveStatus`), that both clients pass, and the words the pane
  shows are `x-clientPolicy.texts` (`editorSaveMissing`, `editorSaveDenied`, `editorSaveFull`,
  `editorSaveOther`, and the button, `editorSaveAgain`). A full disk is the engine's own
  `DiskFull` (`errors::kind`), so a full database is told like a full file system.

**The person can save again at once.** `file.save {fileId}` is a command, not an edit: it forgets
the failure shown, starts the failure streak over, saves on the next tick and, if the file is
gone from the disk, creates it again with the draft, in one step: the text is written beside the
target and published with an exclusive link (`creation::create_text_if_absent`), so a failed write
or a crash leaves no file, never an empty one that the next attempt would take for a change made
by someone else. The temporary file is the engine's own (`.soyeht-publish-`): the project listing,
search, tree and file events hide it with the save temporary (`soyeht_platform::fs::TEMPORARY_PREFIX`,
ADR 0113), by one rule
(`immutable_file::is_engine_temporary`). A crash can leave it, so every publish first removes the
engine's temporary files of its own folder that nobody touched for an hour. That is the safest
sweep: it costs one read of a folder that is being written anyway, needs no walk at start-up, and
the age keeps it from taking a file another write is using. When the link fails for any
reason other than an existing target (FAT and exFAT volumes, some network shares), the temporary
file is renamed to the target when nothing is there. The failure is not matched by kind, because
it is not one kind: Linux FAT says not permitted, macOS FAT and exFAT say errno 45 (which Rust does
not categorize), Windows says an uncategorized code. A real cause (a full or failing disk) fails
the rename with the same error. An existing target is still accepted when its bytes are the same
and refused otherwise. That is atomic
as well, so no empty or partial file is ever visible, even after a crash; what it gives up is the
exclusive link, so a program that creates the file in the instant between the check and the rename
is overwritten, as a plain save of the editor would. Creating the final name first and writing it
after was not chosen: a crash in between leaves exactly the empty file this section exists to
prevent. The folders above a deleted file are made again; a workspace folder that is gone
is not (the save fails as `missing`). The new file has the default mode, not the old one. The
answer is the document as it is, "accepted"; the outcome is `file.saved` or the `saveFailure` of
the next `file.read`. The command is not a `mutation` of the contract (no idempotency key): it
does the same thing however often it is sent. The autosave never creates a missing file,
so a file deleted on purpose (a checkout, a clean-up, an agent) is not brought back by the draft
left in its tab. Only the person's save does.

**The editor's save does not replace a file its owner made read-only.** The save writes a new
file and renames it over the old one, which a read-only bit does not stop; the old code replaced
the file and kept it read-only. The autosave and `file.save` (the `DiskJob`) ask the file port
to refuse it (`ReadOnly::Refuse` of `write_text_if_unchanged`; the port decides, so a test port can
say that a file is read-only) and it is told as `denied`. A project replace and an agent's write are
not the person's save: they pass `ReadOnly::Replace`, are unchanged and still replace such a file.
The change is in effect on Unix only: on Windows the rename over a read-only file already
failed. Changing the bit and saving again works. The rule is the read-only bit, not "this user cannot write": a file owned by someone
else in a folder the user can write is still replaced.

**The engine keeps retrying; it does not give up.** The backoff already caps the wait at 60 s,
so a missing file costs 15 attempts in ten minutes (a test pins the number: a 30 s cap would
give more than 20) and one log line at each power of two. The draft is the person's work and the cause is often temporary (a
mount that returns, a checkout that restores the file, a permission fixed in a shell), so
stopping would turn a recoverable condition into silent loss. What was missing was telling the
person, not stopping the engine. A tab that is closed stops being retried (only open files have
autosave), and its draft stays in the store.

## Consequences

- Both clients show "not saved" in the header and a banner under the tabs with the reason and a
  "Save again" button; the tab's close button is the other way out. The desktop's text,
  screenplay and presentation panes show the same strip through the same code. The Mac's writing
  panes do not yet. Closing does not delete the stored draft, and nothing in the app brings it
  back once the file is gone (it returns if the file is opened again).
- A failed save is no longer a client timeout. The desktop client's wait ends at the first
  `saveFailure` or when the file is clean, and a wait that runs out shows nothing of its own.
- `file.save` and `file.save_failed` are new wire names: `x-methods`, `x-events`, the generated
  models of every language and the message vectors carry them.
