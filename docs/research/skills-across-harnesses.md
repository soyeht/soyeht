# Skills across harnesses

Research for ADR 0075, as of 2026-10-03. Every claim cites an official page,
a changelog or the source of the version Soyeht launches. Claims that could
not be checked are listed at the end.

Versions checked:

- Soyeht launches `@agentclientprotocol/claude-agent-acp@0.77.0` and
  `@agentclientprotocol/codex-acp@1.11.0`
  (`engine/soyeht-engine/src/agents/acp_registry_snapshot.json`).
- claude-agent-acp 0.77.0 bundles `@anthropic-ai/claude-agent-sdk` 0.3.270
  (its `package.json`).
- codex-acp 1.11.0 runs `codex app-server` from `@openai/codex` `^0.153.4`.
- The latest releases at the time of writing were claude-agent-acp 0.85.1 and
  codex-acp 2.1.1. The behaviour below is the same in both lines.

## 1. The open Agent Skills format

Spec: <https://agentskills.io/specification>.

**Folder.** A skill is a folder holding `SKILL.md`. It may also hold
`scripts/`, `references/` and `assets/`.

**Frontmatter.** `SKILL.md` starts with YAML frontmatter, then Markdown
instructions.

- `name` is required: 1 to 64 characters of `a-z`, `0-9` and `-`, no hyphen
  at either end, no `--`, and it must equal the folder name.
- `description` is required: 1 to 1024 characters.
- Optional fields: `license`, `compatibility` (at most 500 characters),
  `metadata` (string to string) and the experimental `allowed-tools`.

**Progressive disclosure.** About 100 tokens of metadata per skill sit in
context. The body loads when the skill is used, and should stay under 5000
tokens. Resources load only as needed.

**Locations.** The spec itself does not mandate any. The implementer guide
recommends scanning `<project>/.agents/skills/` and `~/.agents/skills/`, which
"have emerged as a widely-adopted convention for cross-client skill sharing"
(<https://agentskills.io/client-implementation/adding-skills-support>).

**Adopters.** The client list (<https://agentskills.io/clients>) names 45
clients. Among them: Claude Code, ChatGPT and Codex, Cursor, Gemini CLI,
OpenCode, GitHub Copilot and VS Code.

| Tool | Project folders | User folders | Source |
|---|---|---|---|
| Codex | `.agents/skills` (and `.codex/skills` in source) | `~/.agents/skills`; `$CODEX_HOME/skills` deprecated | §4 |
| Cursor | `.agents/skills`, `.cursor/skills`, plus `.claude/skills`, `.codex/skills` | same under `~` | <https://cursor.com/docs/context/skills> |
| Gemini CLI | `.gemini/skills` or `.agents/skills` | same under `~` | <https://geminicli.com/docs/cli/skills/> |
| OpenCode | `.opencode/skills`, `.claude/skills`, `.agents/skills` | `~/.config/opencode/skills`, `~/.claude/skills`, `~/.agents/skills` | <https://opencode.ai/docs/skills/> |
| VS Code Copilot | `.github/skills`, `.claude/skills`, `.agents/skills` | `~/.copilot/skills`, `~/.claude/skills`, `~/.agents/skills` | <https://code.visualstudio.com/docs/copilot/customization/agent-skills> |
| GitHub Copilot | `.github/skills`, `.claude/skills`, `.agents/skills` | `~/.copilot/skills`, `~/.agents/skills` | <https://docs.github.com/en/copilot/concepts/agents/about-agent-skills> |
| Claude Code | `.claude/skills` only | `~/.claude/skills` | §2 |

`.agents/skills` is the one folder name most harnesses share. Claude Code is
the exception.

## 2. Claude Code skills

Docs: <https://code.claude.com/docs/en/skills>.

### Format

Every frontmatter field is optional, and `description` is recommended. The
fields are:

- `name`, which defaults to the folder name;
- `description` and `when_to_use`;
- `argument-hint` and `arguments`;
- `disable-model-invocation` and `user-invocable`;
- `allowed-tools` and `disallowed-tools`;
- `model`, `effort`, `context` (`fork`), `agent`, `background`;
- `hooks`, `paths`, `shell`;
- `metadata`, `license`, `compatibility`.

Other format rules:

- Supporting files are linked from `SKILL.md`. Scripts are "executed, not
  loaded".
- `${CLAUDE_SKILL_DIR}` names the skill's folder.
- `$ARGUMENTS`, `$N` and named arguments are substituted. Unused arguments
  are appended as `ARGUMENTS: <value>`.
- Outside Claude Code (claude.ai, Skills API), only `name`, `description`,
  `license`, `compatibility`, `metadata` and `allowed-tools` are accepted.

### Locations and precedence

The locations are:

- **Enterprise:** `.claude/skills` in the managed settings directory.
- **Personal:** `~/.claude/skills/<name>/SKILL.md`.
- **Project:** `.claude/skills`, from the start directory and every parent up
  to the repository root.
- **Nested:** `<subdir>/.claude/skills`, loaded the first time Claude touches
  a file there.
- **Additional directory:** `--add-dir`, or the SDK's `additionalDirectories`.
- **Plugin:** `<plugin>/skills`, invoked as `/plugin:skill`.

Symlinked skill folders are followed. Precedence is "Enterprise over personal,
and personal over project". A skill beats a `.claude/commands` file of the
same name.

The docs and the changelog never mention `.agents/skills` (see Unverified).

### Live change detection

From the docs: "When you add, edit, or remove a skill under
`~/.claude/skills/`, the project `.claude/skills/`, or a `.claude/skills/`
inside an `--add-dir` directory, Claude Code picks up the change within the
current session, without a restart."

A top-level skills folder created after the session started needs
`/reload-skills`.

### Progressive disclosure

The skill listing in context holds names and descriptions. Its budget is "1%
of the model's context window", with an 8,000-character fallback
(<https://code.claude.com/docs/en/env-vars>). A description plus
`when_to_use` is truncated at 1,536 characters. With
`disable-model-invocation: true` the description is left out.

An invoked skill's body "enters the conversation as a single message and
stays there across later turns". Supporting files are read on demand.

### Invocation

- People type `/skill-name arguments`.
- The model uses the `Skill` tool.
- Permission rules can name skills, as in `Skill(commit)`.
- Changelog 2.1.3 "Merged slash commands and skills"
  (<https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md>).
- `.claude/commands/*.md` is the older format and still works.

### allowed-tools

`allowed-tools` lists the tools Claude may use without asking while the
skill is active. A skill can therefore widen what an agent does unprompted.

## 3. Claude Agent SDK and claude-agent-acp

SDK docs: <https://code.claude.com/docs/en/agent-sdk/skills>.

**Discovery.**

- The SDK loads skills from the locations that `settingSources` governs.
- When `skills` is omitted, "discovered skills are enabled and the Skill tool
  is available".
- There is no API to register a skill programmatically.

**Commands.**

- A prompt beginning `/<name>` runs a user-invocable skill.
- `supportedCommands()` returns `SlashCommand { name, description,
  argumentHint }`.
- `commands_changed` pushes the full list when it changes mid-session
  (`sdk.d.ts` 0.3.270).

**Options that matter for delivery** (`sdk.d.ts` 0.3.270):

- `additionalDirectories?: string[]`: "Additional directories Claude can
  access beyond the current working directory."
- `plugins?: SdkPluginConfig[]`: local plugins that provide "custom commands,
  agents, skills, and hooks".

claude-agent-acp 0.77.0, `dist/acp-agent.js`:

**Setting sources.**

- Line 6066: `settingSources: ["user", "project", "local"]`, so user and
  project skills load.
- It is followed by `...userProvidedOptions`, taken from the client's
  `session/new` `_meta.claudeCode.options` (line 5956). Soyeht already passes
  hook settings this way (`agents/hooks.rs`).

**Additional directories.**

- The agent advertises `sessionCapabilities.additionalDirectories` (around
  line 904).
- It merges the official ACP `additionalDirectories` of `session/new` and
  `session/load` with `_meta.claudeCode.options.additionalDirectories`, then
  passes them to the SDK (lines 5995 to 5998 and 6206 to 6210).

**Commands.**

- `available_commands_update` is sent after new, load and resume (lines 978,
  998 and 1014).
- It is built from `query.supportedCommands()`, so skills are included
  (line 5507).
- It is re-sent on the SDK's `commands_changed` (lines 2963 to 2976): "The
  client should REPLACE its cached command list".
- Each entry is `{name, description, input: argumentHint ? {hint} : null}`.
  A few terminal-only commands are filtered out (lines 6926 to 6962).

**Prompts starting with "/".** They reach the SDK unchanged and Claude Code
dispatches them. The only rewrite is `/mcp:server:command` (around line 6981).

## 4. Codex skills

Docs: <https://learn.chatgpt.com/docs/build-skills>.

**Format.** `SKILL.md` with `name` and `description`. Optional `scripts/`,
`references/`, `assets/` and `agents/openai.yaml`.

**Locations (docs).**

- REPO: `.agents/skills` in the cwd, its parents, and the repository root.
- USER: `~/.agents/skills`.
- ADMIN: `/etc/codex/skills`.
- SYSTEM: bundled.

**Source** (`codex-rs/ext/skills/src/host_roots.rs`, identical at
`rust-v0.153.4`):

- `.agents/skills` is the folder name (lines 24 to 25).
- The user root is `dirs::home_dir().join(".agents/skills")` (line 105). It
  uses the real home, not `CODEX_HOME`.
- `$CODEX_HOME/skills` is "Deprecated … kept for backward compatibility"
  (line 96).
- The project config layer adds `<project>/.codex/skills`.
- Repository scanning walks from the project root (default marker `.git`)
  down to the cwd (lines 136 to 179).

**Loader limits** (`loader/mod.rs` lines 22 to 32): name 64 characters,
description 1024 characters, scan depth 6. Symlinked folders are followed
for user, repository and admin scopes.

**Invocation.**

- `$skill-name` anywhere in the text, or implicitly through the description.
- The sigil is `$` (`utils/plugins/src/mention_syntax.rs`).
- Mention names match `[a-zA-Z0-9_\-:]` (`skills/src/mentions.rs`).
- Skills are listed within "2% of the context window (or 8,000 characters
  when unknown)", and the body loads when used.

**Change notification.** The app-server protocol has `skills/changed`,
"emitted when watched local skill files change"
(`app-server-protocol/schema/typescript/v2/SkillsChangedNotification.ts`).

**Custom prompts.** `~/.codex/prompts/*.md`, invoked as `/prompts:name`, are
"Deprecated. Use skills for reusable prompts"
(<https://learn.chatgpt.com/docs/custom-prompts>).

**AGENTS.md.** One global file from `$CODEX_HOME`, then one file per folder
from the Git root to the cwd, concatenated, at most 32 KiB by default
(<https://learn.chatgpt.com/docs/agent-configuration/agents-md>).

## 5. codex-acp

Checked at 1.11.0, cross-checked against 2.1.1.

**Additional directories.**

- The agent advertises `sessionCapabilities.additionalDirectories`
  (`CodexAcpServer.ts` line 346).
- For `session/new`, `session/load` and resume, it reads the ACP
  `additionalDirectories`, which must be absolute paths
  (`CodexAcpClient.ts` lines 1423 to 1448).
- It registers `<directory>/.agents/skills` as extra skill roots with
  app-server `skills/extraRoots/set`, then calls
  `skills/list { forceReload: true }` (`CodexAcpClient.ts` lines 806 to 823).

**Commands** (`CodexCommands.ts`, `publish()`).

- The built-in commands are `plan`, `mcp`, `skills`, `status`, `review`,
  `review-branch`, `review-commit`, `compact`, `goal`, `rename` and `logout`.
- Each skill is listed as `"$" + skill.name`, with no input hint.
- Custom prompts are not listed.
- The list is published when a session opens. `skills/changed` is not acted
  on (`CodexEventHandler.ts` line 599), so a skill added mid-session does not
  re-send the list.

**Prompts starting with "/".**

- Built-in commands run locally.
- A command starting with `$`, or one the adapter does not know, goes to
  Codex unchanged ("Let Codex resolve unrecognized commands as raw prompts").
- Codex's `$` mention scanner then selects the skill. That last step is read
  from the source, not run.

**Environment.** The process environment passes through to codex, so
`CODEX_HOME` applies.

## 6. ACP slash commands

Page: <https://agentclientprotocol.com/protocol/slash-commands>.

- An agent may send `session/update` with `available_commands_update`
  carrying `availableCommands: [{ name, description, input?: { hint } }]`.
- It can re-send the list at any time; the new list replaces the old.
- "Commands are included as regular user messages in prompt requests", for
  example the text `/web agent client protocol`.
- Schema (`@agentclientprotocol/sdk` 1.7.0): `AvailableCommand` has `name`,
  `description`, an optional `input` (an `UnstructuredCommandInput` with
  `hint`) and `_meta`.

## 7. What this means for Soyeht

No single folder reaches both harnesses natively.

- **Claude Code** reads only `.claude/skills`. It watches that folder inside
  any `--add-dir` / `additionalDirectories` entry.
- **Codex** reads `.agents/skills`, and codex-acp adds
  `<additional directory>/.agents/skills` as an extra root.
- **Both adapters** accept the standard ACP `additionalDirectories`. A Soyeht
  folder shaped like a project root, with `.agents/skills` plus a
  `.claude/skills` link to it, therefore reaches both natively, without
  writing to `~/.claude`, `~/.codex` or `~/.agents`.
- **Invocation.** Claude Code invokes a skill as `/name arguments`, Codex as
  `$name arguments`. Both arrive through the ACP prompt text.

ADR 0075 records the design built on these facts.

## Unverified

- **Claude Code and `.agents/skills`.** That Claude Code does not read
  `.agents/skills` is inferred from the docs and changelog never mentioning it.
- **`CLAUDE_CONFIG_DIR`.** Whether it moves `~/.claude/skills` is not stated.
- **Live reload through a symlinked folder.** It is not documented whether
  Claude Code's live change detection follows a `.claude/skills` that is
  itself a symlink, rather than a symlinked skill folder inside it. The
  engine tests use a fake agent.
- **Codex picking up new skills mid-session.** It is not proven that Codex
  sees a skill added to an extra root during a session without a new session.
  The app-server watches skill files and emits `skills/changed`, but
  codex-acp does not refresh its command list.
- **When Codex introduced skills.** Skills were announced as experimental on
  2025-12-15
  (<https://community.openai.com/t/skills-for-codex-experimental-support-starting-today/1369367>).
  The CLI version is not confirmed.
- **Codex's full built-in command list.** Not checked beyond the commands
  codex-acp advertises.
