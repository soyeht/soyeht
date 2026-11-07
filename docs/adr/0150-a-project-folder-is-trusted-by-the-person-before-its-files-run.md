# ADR 0150: A project folder is trusted by the person before its files run

- Status: accepted. Changes a boundary: the engine no longer starts a language server for a folder on its own.
- Date: 2026-11-07

## Context

The engine starts a language server (rust-analyzer, sourcekit-lsp, typescript-language-server, pyright) with
the workspace folder as its working directory as soon as a file of that language is tracked, and also when a
client asks `lsp.status`. These servers read the folder's build description and run what it names: a Cargo
build script and procedural macros, a `.cargo/config.toml` that names the compiler, the program a
`rust-toolchain.toml` points to, a `Package.swift` that is compiled and run, a build server named in
`buildServer.json`, the TypeScript that sits in `node_modules`. Opening a hostile repository therefore ran its
code with the person's rights, before the person had read a line of it. Editors that open other people's
folders ask first ("Do you trust the authors of the files in this folder?"), and the repository-defined helpers
(`docs/contracts/17-delegation.md`) will need the same answer.

## Decision

- **One state per folder, kept by the store.** `undecided`, `restricted` or `trusted`, keyed by the canonical
  folder (`soyeht_platform::fs::canonicalize`), written only by a decision of the person. No row means
  `undecided`, which is treated as `restricted`. It is not derived from anything: not from the parent folder,
  not from a git remote, not from who created the workspace.
- **A human-only method and an event.** `project.trust.set` and the durable `project.trust_changed`; a
  snapshot section `trust` gives each open workspace's state. The method follows the pairing rule: on macOS only
  the signed app may call it (a program a repository started in a terminal cannot), a paired device may, and
  an agent or a chief never can; the dispatcher allow-list does not name it and the handler refuses again.
- **The clients ask once per folder.** The Mac, the iPhone and iPad, and the Windows and Linux app show
  "Trust this folder?" the first time a workspace with an undecided folder is active, with the restricted
  choice as the default; a dismissal defers, it does not answer.
- **Without trust a language server starts only in a mode that cannot run the folder's files.** The rule is
  proof, not a flag: a server is allowed untrusted only if, from its source or documentation and from running
  it where we could, nothing in the folder can make it run a program the folder names, with the options we
  give it. By that rule TypeScript (with the installed `tsserver.js`, no type installs, no plugins) and
  pyright (nothing to switch off; not on Windows) start, and rust-analyzer and sourcekit-lsp do not. The client
  says why (`lsp.status`: `restricted` and the reason) and the editor request answers at once.
  rust-analyzer's own switches for build scripts and proc macros are not enough: with them off, a
  `.cargo/config.toml` rustc wrapper, a `rust-toolchain.toml` toolchain path and a `rust-analyzer.toml`
  formatting command still ran in the test (contract 18, section 4). A start that closes those holes needs
  the supervisor to pass environment variables it does not allow today, the real toolchain binary resolved off
  the lock, and an audit of cargo's other configuration keys; that is a separate change, and until then the
  answer for rust-analyzer is no.
- **Following a decision is a restart.** The server records the trust it was started for; a decision, an
  engine start and a supervisor reconnect make it match the store, closing a process started for another
  answer.
- **Other features reuse the answer.** `project_trusted(store, path)` is the one question. The helpers'
  `helpers.trust` is a narrower per-revision approval on top of it, not a second folder state.

## Consequences

- A folder opened for the first time has no Rust or Swift language features until the person trusts it; the
  editor shows why and offers the question again. Existing workspaces start `undecided` after the upgrade.
- Engine integration tests that need a Rust server trust the folder first.
- The database schema is 33; downgrading to an engine that does not know it is refused as always.
- A folder stays trusted when its contents change. That is a property of the decision, as in other editors; a
  trusted folder is as dangerous as the person's own account.
- Not covered: programs started in terminals or by agents, which the person already runs as themselves.
