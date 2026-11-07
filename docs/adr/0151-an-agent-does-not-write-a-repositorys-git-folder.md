# ADR 0151: An agent does not write a repository's .git folder

- Status: accepted. Extends ADR 0016 (the agent pane contract) and ADR 0143 (adapters start in the
  engine's folder).
- Date: 2026-11-07

## Context

An agent reads and writes text files through the engine (`fs/read_text_file`, `fs/write_text_file`),
confined to the pane's folder with no symlink followed. Inside that folder a repository's own control
folder was an ordinary target. What it holds is run by git later (hooks), names programs git starts
(configuration), or points a linked worktree at another repository (the `.git` file). The engine's
`accept_edits` policy approves an edit inside the folder without asking, so an agent allowed to edit
project files could reach code that runs outside every permission it was given.

## Decision

- **The engine refuses a write whose path has a `.git` component**, at any depth, in any case, as
  Windows reads the name (`.git.`, `GIT~1`, and an NTFS stream, `.git::$INDEX_ALLOCATION`,
  `.git:x`; on Windows any name with a `:` is refused). The check is one function,
  `filesystem::is_repository_control`; the effect that carries out `fs/write_text_file` is where
  it applies, before the editor sees the path, and `permits_edit` (the automatic approval of
  `accept_edits`) says no for the same paths.
- **A refusal has its own code** (`-32002` in the answer to the adapter, not the generic failure)
  and the log gets one line, `engine.agent_write_refused` with `reason=git_folder`, so an agent
  that probes for the folder is visible. Reads are unchanged.

## What this does not cover

- Files the agent's own tools write (a shell, an adapter's editor): the engine does not see them.
  Their limits are the adapter's permissions.
- A control folder with another name: `GIT_DIR`, a separate git directory inside the root that a
  `.git` file points to.
- A hook folder that is tracked in the repository (`.githooks`, `.husky`) which a setting points
  git to: it is an ordinary project file, and what makes git run it is a setting in `.git`, which
  is refused.
