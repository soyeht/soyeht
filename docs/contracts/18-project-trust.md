# Contract 18 · Project trust

Status: v1. Decision: ADR 0150. Code: `soyeht_engine::project_trust`, `soyeht_store::project_trust`,
`soyeht_lsp::{refusal, initialization_options}`. Texts: `projectTrust*` of `x-clientPolicy.texts` in
`contracts/protocol.json`.

A project folder holds files that other programs run: a language server reads its build description
(a build script, a package manifest, a compilation database) and runs what they name. Opening a
repository someone else wrote must not run that repository's code until the person says it may. This
contract gives each folder one answer, says who may give it, what the engine does while the folder has
none, and how the next feature that runs a folder's files (the repository-defined helpers) reads the
same answer.

## 1. The state

`ProjectTrust` has three values:

| Value | Meaning |
| --- | --- |
| `undecided` | Nobody has answered for this folder. It is treated as `restricted`. |
| `restricted` | The person chose to keep the folder restricted. The clients do not ask again. |
| `trusted` | The person trusts the folder. Its files may run code. |

The key is the folder, not the workspace: the canonical path of the workspace's `path`
(`soyeht_platform::fs::canonicalize`). Two workspaces over one folder, and one folder opened again after
its workspace was closed, share the answer; a folder replaced by a link to another is a different folder.
The store keeps one row per folder (`project_trust`, schema 33) with the answer and when it was given.
There is no default and no inheritance: a subfolder is not trusted because its parent is, and nothing
trusts a folder except a decision of the person. An engine that is upgraded from a version without the
state starts every folder `undecided`.

## 2. Who decides

`project.trust.set {workspaceId, trust: "trusted" | "restricted", idempotencyKey}` →
`{workspaceIds, trust}` (`workspaceIds`: every open workspace over that folder).

| Role | May decide |
| --- | --- |
| `human_local` | on macOS only the signed Soyeht app; a shell, a script, or a terminal's program on the local socket gets `forbidden_role`. Where the system cannot tell the app from another program (Windows, Linux) any program of the same user counts as the person, as for pairing (ADR 0128). |
| `human_device` (iPhone, iPad) | yes |
| `agent`, `chief` | never (`forbidden_role`): the agent allow-list of the dispatcher does not name the method, and the handler refuses every other role again |

`project.trust.set` is a mutation with an idempotency key. It writes the row and the event in one
transaction, then every language server of that folder follows the new answer (section 4).

## 3. What the clients receive and show

- The snapshot has a `trust` section: one `ProjectTrustEntry {workspaceId, trust}` per open workspace.
- `project.trust_changed {workspaceIds, trust}` is durable. It follows `project.trust.set`, and it follows
  every `workspace.created` with the answer for the new workspace's folder, `undecided` included, so a
  client learns a workspace is undecided from the event and asks, with no reconnect.
- The first time a workspace whose trust is `undecided` is active, each client asks "Trust this folder?"
  (`projectTrustTitle`, `projectTrustSummary`) with two buttons, `projectTrustTrust` and
  `projectTrustRestrict`, and a dismissal that defers the question until the app starts again (it is not
  an answer). Restrict is the default button. The Mac, the iPhone and iPad, and the Windows and Linux
  app all do this from the same replica and policy; a restricted workspace keeps a visible way to open the
  question again (`projectTrustRestrictedNote`).
- `lsp.status` says what a server is doing: `state: "restricted"` with a `detail` naming the reason when it
  did not start, and `safeMode: true` when it runs with the code-running features off. The editor shows both.

## 4. Language servers without trust

`soyeht_lsp::refusal(language, trusted)` is the only list of which servers start in a folder that is not
trusted, and `soyeht_lsp::initialization_options` is the only list of what they are told. A server that
cannot be proved to run none of the folder's files does not start. The evidence, from each server's own
source or documentation and, for rust-analyzer, from running version 1.98.1 against a fixture whose every
hostile element only writes a marker file:

| Server | Not trusted | Why |
| --- | --- | --- |
| `rust-analyzer` | does not start | Switching build scripts, proc macros and check-on-save off (`cargo.buildScripts.enable`, `procMacro.enable`, `checkOnSave` of `initializationOptions`; [configuration](https://rust-analyzer.github.io/book/configuration.html)) stops `build.rs` and macro expansion, but `cargo metadata` still ran the `rustc-wrapper`, `rustc` and `rustc-workspace-wrapper` that the folder's `.cargo/config.toml` names, and `rustup` ran the program a `rust-toolchain.toml` `path` names, and a `rust-analyzer.toml` in the folder sets `rustfmt.overrideCommand` above the client's options (`crates/rust-analyzer/src/config.rs`: workspace-level keys are read from the folder first). A safe start needs the real toolchain binary by absolute path, a scrubbed environment (`RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`, `RUSTC`, `RUSTUP_TOOLCHAIN` and their `CARGO_BUILD_*` forms), no formatting, and offline cargo, with the credential-provider and other cargo configuration keys not audited; the supervisor's environment allowlist does not carry them today. It is future work, not a flag. |
| `sourcekit-lsp` | does not start | A `.sourcekit-lsp/config.json` in the folder is applied after `initializationOptions` and can switch the manifest sandbox off; `Package.swift` is compiled and run (the sandbox allows reading the disk and starting processes), `buildServer.json` and `.bsp/*.json` name a program, and `compile_commands.json` or the repository's own compiler flags can load a compiler plugin (`-load-plugin-executable`). `--default-workspace-type` only reorders the search; no option turns a type off ([configuration file](https://github.com/swiftlang/sourcekit-lsp/blob/main/Documentation/Configuration%20File.md), `DetermineBuildServer.swift`). |
| `typescript-language-server` | starts, safe mode | It takes the `tsserver.js` that is installed with it (`tsserver.path`, an absolute path found beside the server on `PATH`), not the folder's `node_modules/typescript` that it would pick otherwise (`findTypescriptVersion`), `disableAutomaticTypingAcquisition: true` (type acquisition runs `npm install`), and no plugins. The server never passes `--allowLocalPluginLoads`, so `compilerOptions.plugins` loads nothing. Without a `tsserver.js` beside the server it does not start and says so. |
| `pyright-langserver` | starts, safe mode | Pyright imports nothing from the folder. Its one subprocess is the interpreter (`python3` from the supervisor's `PATH`), run with the code that finds its search paths, and `pyrightconfig.json` and `pyproject.toml` cannot choose the interpreter (`pythonPath` is a language-server setting only). On Windows the system looks in the working directory first, so a `python3.exe` in the folder would win: there it does not start. |

A server that did not start has `state: "restricted"` and a `detail` that ends "Trust the folder to turn it
on."; an editor request for it (a symbol, definition, reference or `file.format`) answers `harness_unavailable`
with the same text at once, through one function (`require_language_server`). A trusted folder
starts every server with the options it always had.

**Following a decision.** The server records the trust it was started for. When the answer changes, the
engine closes the server's process, bumps its generation and starts it again the new way if an editor pane
needs it (trusting turns a restricted server on; restricting closes a running one). The same check runs when
the engine starts or reconnects to its supervisor, so a process started for another answer is never adopted.
A checkpoint that does not say what trust it was started for (written by an engine that did not know
project trust: its process may have been started with the folder's own `tsserver` and plugins) is closed and
started again once, the first time the engine looks at it after the upgrade.

The old process is closed with the supervisor's retried close, and a language server process whose instance key
no server owns (a trust that changed while the supervisor was away) is closed when the engine reconciles with
the supervisor, as the agent processes are. A server that cannot start the way its folder's trust asks (no
installed `tsserver.js`, no program on a trusted path) is left `unavailable` with the reason; it never drops the
engine's link to the supervisor.

**Programs and paths.** The server's working directory is the project folder, and a program it starts by
name (`env node` in the launcher of `typescript-language-server`, `python3` for pyright) is found on the search
path it inherits, where an empty or relative entry is the folder itself. So without trust the engine gives the
process a search path of its own (`PATH` in the spawn's environment, part of its identity): the engine's absolute
entries only, each as the system resolves it, none inside the canonical folder. It starts the server by the
absolute path it found on that path, with the canonical folder as the working directory, and refuses a program
that resolves inside the folder (an absolute `node_modules/.bin` entry that a shell hook adds, a link into the
folder). `typescript-language-server` takes its `tsserver.js` only from a `typescript` package that resolves
outside the folder and has a readable `package.json` with a version; a partial install is refused. What a start
or an adoption is made of is worked out once (`Server::launch`), from the folders of the search path (a stat of
each, not a walk). A process the supervisor holds that was started under another identity, a different search
path for one, is closed and started again; it never drops the link to the supervisor.

**What this does not cover.** A folder that was trusted and then changed (a pull, a checkout) is still
trusted; the state is a decision about a place, as in other editors. Programs the person starts in a
terminal or through an agent are not governed by it: it governs what Soyeht starts on its own when a file is
opened.

## 5. For the features that follow

The answer is the one place a feature asks "may this folder's files run?":
`soyeht_engine::project_trust::project_trusted(store, path)`. The repository-defined helpers
(`docs/contracts/17-delegation.md`, `helpers.trust`) require it before a repository type is considered at all;
their per-revision approval is a second, narrower decision on top of the folder's. A feature does not keep a
trust state of its own, does not infer one, and does not set this one.

## 6. Out of scope

Trusting a single file or subfolder; a list of trusted parent folders; trust that expires; an organisation
policy; a server settings file the person writes for a restricted folder.
