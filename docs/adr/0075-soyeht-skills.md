# ADR 0075: Soyeht skills

- Status: accepted
- Date: 2026-10-03

## Context

The owner wants one place where a person keeps skills that work in every
harness Soyeht runs.

- Typing "/" in the composer lists the skills and the harness's own commands.
- A skill also loads by itself when its description matches, as in Claude
  Code.
- People create skills in the app, and often by asking an agent: "create a
  skill that teaches you to do X".

The facts behind this design, with sources, are in
`docs/research/skills-across-harnesses.md`. In short:

- Claude Code reads only `.claude/skills`.
- Codex and most other harnesses read `.agents/skills`, the cross-tool
  convention of the open Agent Skills format.
- Both pinned ACP adapters accept the standard `additionalDirectories` on
  `session/new` and `session/load`:
  - Claude Code watches `<directory>/.claude/skills` live.
  - codex-acp registers `<directory>/.agents/skills` as an extra skill root.
- Both adapters advertise their skills in ACP `available_commands_update`:
  Claude Code as `name`, Codex as `$name`.

## Decision

### Format

A skill is a folder holding a `SKILL.md` in the open Agent Skills format
(<https://agentskills.io/specification>):

- `name` matches the folder name and is 1 to 64 lowercase letters, digits and
  single hyphens;
- `description` is 1 to 1024 characters.

This is the strictest form every harness accepts. Soyeht reads `name`,
`description` and, to show it, `allowed-tools`. Other fields stay in the file
for the harness that reads them. The `soyeht-skills` crate owns the format,
one folder's create/read/update/delete, and the on-disk layout.

### Stores

- **User skills.** They live in `<data>/skills/.agents/skills/<name>/`. The
  skills home `<data>/skills` is laid out like a project root, and its
  `.claude/skills` is a link to `.agents/skills`. It is a plain folder that
  people, the Preferences form and agents all write with ordinary file tools.
- **Workspace skills.** They live in `<workspace>/.agents/skills`, the
  cross-tool folder that travels with the repository.

A workspace skill hides a user skill of the same name, the open format's
convention that a project overrides the user.

### Delivery

When an agent advertises `sessionCapabilities.additionalDirectories`, every
production `session/new` and `session/load` lists:

- **the skills home.** Claude Code loads its `.claude/skills` and watches it.
  codex-acp loads its `.agents/skills`.
- **a project bridge,** when the session's project keeps skills: a folder
  `<data>/skill-bridges/<hash of cwd>` whose `.claude/skills` links to
  `<cwd>/.agents/skills`. Codex reads the project's folder by itself; the
  bridge gives Claude Code the same skills.

A new or edited skill reaches running Claude Code sessions without a restart.

Probe sessions receive no folders. `agents/opening.rs` composes these
folders with the existing Claude Code hooks for every opening path, including
harness switches.

**Fallback.** For an agent without the capability, the soyeht MCP server's
instructions name the store. They tell the model to read a matching
`SKILL.md` and how to create a skill. Every Soyeht pane receives that server.

**Rejected options:**

- **Syncing or linking into `~/.claude/skills`, `~/.codex` or
  `~/.agents/skills`.** It changes the person's own harness configuration,
  collides with their skills, and leaves files behind when Soyeht is removed.
- **Exposing skills only as MCP tools or prompts.** There is no native
  auto-loading, and prompts appear under MCP names.
- **Injecting skills at session start.** It costs tokens in every session and
  loses progressive disclosure.

### Harness commands

`soyeht-acp` parses `available_commands_update`. The update is a replacement
list, bounded to `agentCommands` entries; entries a person cannot type are
dropped. The engine keeps the list with the agent checkpoint, so an adopted
process keeps it across an engine restart.

Clients read it with `pane.commands.list` and refresh on the durable
`pane.commands_changed { paneId }`. The list stays out of `pane.updated`,
because Claude Code can advertise hundreds of skills.

### The "/" list

The shared `SlashCatalog` (`apps/shared`) builds the list for Mac, iPhone and
iPad.

**What it lists.** Usable Soyeht skills of the pane's workspace (the user's
plus the workspace's) come first. The pane's harness commands follow. A
harness copy of a Soyeht skill, `name` or `$name`, appears only as the skill.

**Filtering.** Typing filters by name prefix, then by name substring, then by
a description word. Arrow keys move the selection, Return or Tab chooses,
Escape closes, and a tap chooses. Choosing writes `/name ` into the composer.

**Sending.** Text is sent as typed, except `/skill arguments` for a Soyeht
skill:

- `/name arguments` when the harness advertises `name` (Claude Code);
- `$name arguments` when it advertises `$name` (Codex);
- otherwise `Use the Soyeht skill "name": read <path>/SKILL.md and follow it.`
  followed by the arguments.

The UI is a thin list above the field; the composer redesign will restyle it.

### Protocol

- `skills.list { workspaceId? }` returns the user's skills, plus the
  workspace's when one is named, and the store folders. Listing a workspace
  makes the engine follow it for that connection.
- `skills.read`, `skills.create`, `skills.update` and `skills.delete` act on
  one scope. `skills.update` replaces `SKILL.md` only when the base revision
  (the file's SHA-256) still matches.
- `skills.changed { scope, workspaceId? }` is a transient event.
- New limits: `skillBytes` 256 KiB, `skillEntries` 256, `agentCommands` 256.

### Watching

The engine follows the user store and every workspace store a client listed,
with `notify` file events as the project panel does.

- A burst is read once it settles.
- A folder is announced only when its listing changed.
- A store that does not exist yet is looked for every two seconds.
- The engine's own mutations mark their scope at once.

A skill an agent writes therefore appears in every client's list, and in
running Claude Code sessions, without a restart.

### Creating skills

The simplest path works in every harness: an agent writes
`<store>/<name>/SKILL.md` with its normal file tools.

**The guide.** When the user store is first created it gets one starter skill,
`create-skill`. It holds the store's absolute path and a short procedure:

1. Write the folder.
2. Test it with a zero-context agent: a fresh subagent with no history (the
   harness's own subagent feature, or a new Soyeht pane messaged through the
   soyeht MCP tools). The tester gets only a realistic task that should
   trigger the skill, never its purpose.
3. Fix the skill and repeat until the tester succeeds.
4. Report what was tested.

The writer of a skill knows its purpose, and only a reader without that
context shows whether the text is enough. A person who deletes the guide
keeps it deleted. The soyeht MCP instructions point to the same guide.

**Other paths.** Preferences > Skills has a form and opens the folder or a
`SKILL.md` in the person's editor.

**Validation.** Listing keeps a broken folder with a `problem` the
Preferences list shows in red, such as "The frontmatter has no description."
`skills.update` refuses text that would break the skill. Nothing is hidden
silently.

### Conflicts and duplicates

Soyeht never writes into the harnesses' own folders, so no file of the
person's is ever replaced.

A Soyeht skill named like one in the person's `~/.claude/skills` or
`~/.agents/skills` is resolved by the harness's own precedence:

- Claude Code keeps one command per name;
- Codex shows both in its selector.

The "/" list shows one entry per name.

### Trust

Skills are instructions and may carry scripts.

- Soyeht never runs a skill's scripts. Agents run them with their own tools
  under the pane's permission mode.
- The additional directories give agents file access to the skills home and
  the project bridge only.
- The store belongs to the person, and agents may write it, which is the
  point of agent-made skills. An agent misled by untrusted content could
  therefore plant a skill that later loads in every harness. Mitigations:
  - every change is announced and listed;
  - Preferences shows each skill's `allowed-tools`, with which Claude Code
    pre-approves tools while the skill is active;
  - a skill is plain text the person can read and delete.
- Workspace skills arrive with the repository and carry the same trust as its
  `AGENTS.md` or `.claude/skills`.
- Deleting a skill removes its folder after the Preferences confirmation.

## Consequences

- Claude Code and Codex load Soyeht skills natively, with progressive
  disclosure and auto-loading by description, and run them from "/".
- codex-acp publishes its commands only when a session opens. A skill added
  later reaches Codex's own list in the next session, but the "/" list shows
  it at once from the store.
- A project bridge exists only when the project kept skills when the session
  opened. A project's first skill reaches a running Claude Code session after
  `/reload-skills` or in the next session.
- Harnesses without `additionalDirectories` rely on the MCP instructions and
  on the "/" fallback that points at `SKILL.md`.
- The research lists what is unverified: live reload through a linked
  `.claude/skills` folder in real Claude Code, and Codex picking up skills
  mid-session.
