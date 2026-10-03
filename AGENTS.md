# Rules for agents working in this repository

These rules are enforced by the merge gate where a check exists. Where the gate
cannot see a rule yet, the rule still applies. If code and this file disagree,
fix one of them in the same PR.

## Architecture layers

Each layer depends only on the layers listed above it.

1. `contracts/`: `protocol.json` (wire methods, events, limits, error codes) and
   `profiles.json` (installed profile values). This is the only place where wire
   values are defined.
2. `engine/soyeht-protocol`: generated wire models, limits and framing. It has no
   Soyeht dependencies.
3. Engine building blocks, each depending only on `soyeht-protocol`:
   - `soyeht-ptyd`: the process supervisor that holds pseudoterminals.
   - `soyeht-acp`: the ACP v1 adaptation. Process ownership and durable effects
     belong to the engine.
   - `soyeht-store`: the only crate with SQL or `rusqlite`
     (`hardening_gate.py check-no-sql-outside-store`).
   - Pure, bounded domain crates: `soyeht-editor`, `soyeht-writing`,
     `soyeht-web`, `soyeht-pane-state`, `soyeht-usage`, `soyeht-lsp`, and
     `soyeht-vcs` (which has no Soyeht dependencies).
4. `engine/soyeht-engine`: assembles the crates above behind one local socket
   and WebSocket API. `soyeht-cli` and `soyeht-mcp` are clients of the protocol.
5. `apps/shared` (`SoyehtClient`): the shared Swift client. It holds the engine
   client, replicas, presentation policies, layout math and the theme. Put logic
   here so `swift test --package-path apps/shared` covers it.
6. `apps/mac/Sources` and `apps/ios/Sources`: thin views over `SoyehtClient`.
   They render and forward input. They do not own state rules or wire handling.

Every feature starts in `docs/contracts/`. Decisions live in `docs/adr/`; add an
ADR when you change a boundary.

## Single source of truth

- `contracts/protocol.json` and `contracts/profiles.json` are the sources.
  `uv run python contracts/generate.py` writes the Rust, Swift, TypeScript and
  Python outputs. A file whose first line starts with `// Generated from
  contracts/` (or `# Generated from contracts/`) is an output: never edit it by
  hand. Commit the outputs with their source; the gate runs `--check`.
- Once a generated form exists, use it. Do not hand-type a method or event name,
  a limit, or an error code. Examples: `EventName`, `METHODS`, `error_code()`,
  `limits.rs` and `ProtocolLimits`.
- One rule, one place. Before you add a helper, a constant or a policy, search
  for an existing one and reuse or extend it.

## No duplication and no god files

`scripts/check_quality.py` must match `scripts/quality_baseline.json` exactly:

- jscpd clones and duplicated lines per area may not grow.
- A new file may not exceed 800 lines. A file already above 800 lines may not
  grow past its recorded cap. Split it instead.
- Loose Swift padding, corner-radius, font-size and font-name literals, and Rust
  `4096`/`512` literals, may not grow per file. Use a token, a policy or the
  generated limits.
- Engine production code has no `unwrap`/`expect`/`panic!` calls. The limit is
  0, and no baseline edit or written reason can raise it. Test code is exempt:
  crate `tests/` and `benches/`, inline `#[cfg(test)]` and
  `#[cfg(all(test, ...))]` items, and modules declared `#[cfg(test)] mod x;`,
  including their submodules. Clippy denies these calls through the workspace
  lints too.
- When a number improves, lower the baseline in the same PR:
  `uv run python scripts/check_quality.py --print-baseline > scripts/quality_baseline.json`.
  The check fails until you do.
- Raising any other baseline entry needs a written reason for the exact new value in
  `scripts/quality_baseline_justifications.json`. `review_report.py` shows each
  reason to the reviewer.

`scripts/hardening_gate.py` also fails when the number of functions over 120 or
over 200 lines grows, when an existing long function grows, or when tests,
acceptances or gate steps drop. A drop needs a `Verification change:` line in the
PR body.

## Design

- Colors come only from the palette: `SoyehtThemeColors`/`MacTheme`, backed by
  `ThemePalette.swift` (ADR 0065). Do not add literal or system colors, and do not
  add new colors. `check_mac_view_colors.py` enforces this.
- Pane interiors are flat. `paneShell()` is the only raised (neumorphic) surface.
  Inside a pane, use no shadows, no raised or inset modifiers, and palette roles
  only. `check_pane_interior.py` enforces this.
- Use only the approved fonts (`check_mac_text_fonts.py`).

## Commands

- Python: always `uv run python ...`, never a bare `python3`.
- Fast check, also run by the pre-push hook
  (`git config core.hooksPath .githooks`): `uv run python scripts/check_quality.py`.
- Merge gate, before every merge:
  `FORBIDDEN_TERMS=... bash scripts/check_all.sh`. Get the vocabulary from outside
  the repository. `scripts/check_all.sh` is the only list of gate steps. A CI
  workflow calls it and never lists steps of its own (`test_gate_list.py`).
- The ratchets compare with the merge-base of `origin/main` and `HEAD`. They fail
  if it cannot be resolved, so run `git fetch origin main` first. Pass
  `--base FULL_OID` to override it.
- Rust: `export PATH="$HOME/.cargo/bin:$PATH"; (cd engine && cargo build --workspace)`.

## Repository hygiene

- Write everything in the repository in English: code, comments, docs, commits
  and PRs.
- Add no AI attribution in commits, PR titles or PR bodies: no co-author
  trailers and no "generated by" lines. `forbidden_terms.py` rejects them.
- Never commit `benchmark/`. It is private local analysis and is ignored.
- Never commit secrets or machine-specific values: tokens, keys, team IDs, VM
  addresses or the restricted vocabulary. Pass them through the environment.
