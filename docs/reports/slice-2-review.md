# Slice 2 review · 2026-09-11

Reviewed on `main` at a0c860a against `docs/contracts/00-slice-2-objective.md`
and contracts 04, 06, 07 and 08. Read-only; every claim points at a file
and line. Verdict: **accepted with follow-ups**. Nothing found is fake or
silently missing; the follow-ups below are required before slice 3 starts.

## Acceptance
All ten deterministic acceptances are real: launchd jobs, a real SQLite
database, the deterministic ACP peer as a separate process, real `SIGKILL`
of the engine and of the supervisor (`scripts/e2e_agents.py:135-314`,
`engine/soyeht-engine/tests/runtime.rs`). Acceptance 11 (physical iPhone
over mobile data) is documented and remains with Caio, as the report says.
The orphan code from the audit is now wired: `soyeht-acp` is a dependency of
the engine (`engine/soyeht-engine/Cargo.toml:10`) and the agent store is
called from `core.rs:60,66`, `panes.rs:126`, `agents/*`. Protocol, vectors
and the three generated targets share the same schema sha256; the fourteen
new methods and eighteen new events match contract 08 by name and field.
The Mac and iPhone apps render the normalized transcript, answer
permissions (with a two-client test, `PhoneConnectionTests.swift:144-167`),
and use generated types.

## Follow-ups required before slice 3 (one PR set, small)
1. **Stale contracts, same PR as the code that changed them.**
   `docs/contracts/04-acp-session.md` §2.4 (`mcpServers` is empty in this
   slice, `soyeht-acp/src/client.rs:87`) and §9 (`id` → `requestId`);
   `docs/contracts/08-engine-and-clients.md` §5 (`new_session` vs
   `new_session_with_package`, `agents/responses.rs:261`; add
   `pane.queue.paused|resumed|steered`).
2. **Dead protocol surface.** `pane.queue.steered` is in the schema and the
   Swift client with no emitter in the engine (`grep steer engine/*/src`
   empty). Implement the steer path from the contract 04 addendum or remove
   the event until it exists.
3. **Silent fallback on a security value.** `agents/configuration.rs:32`
   maps an unknown permission mode to `Ask` without an event. Return
   `invalid_params` or emit `pane.config_failed`.
4. **`expect` on the snapshot path served to clients**,
   `engine/soyeht-engine/src/dispatch.rs:302-303`. Convert to typed errors.
5. **Permission deadline duplicated**: `agents/questions.rs:15` hardcodes
   `30*60*1000` instead of the generated `limits::PERMISSION_TIMEOUT_SEC`.
6. **Evidence of the gate run.** The report says the full gate passed but no
   artifact or hash is recorded (`.cache/` is ignored). Record in the report
   the date, `main` hash and the summary lines of `.cache/e2e-agents.json`
   and `.cache/e2e-real-acp.json`, as slice 1 did.
7. **Closed enum**: `ModelListResult.source` is an open string in the schema,
   forcing `"not_probed"` by hand in Swift (`AgentReplica.swift:37,276`).
   Contract 08 §1.5 requires closed enums.
8. **Transcript paging** ignores `nextCursor` (`AgentReplica.swift:215`).
9. **iPhone**: the sign-in card leads to a read-only terminal pane
   (`AgentReplica.swift:404-412`); either hide the card on iPhone or route
   sign-in to the Mac. Model and effort change on iPhone (contract 06 §1.7)
   is missing; move it to slice 8 explicitly or add it.
10. **ADR headings** 0022–0025 use a different format from the others.

## Architecture debt (from the 2026-09-11 audit), measured again
| Item | Before (cdf44b7) | Now (a0c860a) |
|---|---|---|
| engine files with raw SQL | 8 | 11 (15 import the database crate) |
| `unwrap`/`expect` in engine `src` | 98 | 197 (127 outside test modules) |
| `pub trait` in the workspace | 0 | 0 |
| duplicated NDJSON decoders | 2 | 2, now with different behaviour on oversized lines (`ptyd/src/pending.rs:20-42` drops silently, `acp/src/rpc.rs:138-175` errors) |
| longest function in ptyd | 242 lines | 263 lines (`server.rs:182`) |
| limits as arithmetic | yes | fixed (`x-limits` generated) except `questions.rs:15` |

These are not follow-ups of slice 2; they are the pending decision on the
hardening round. The numbers show the debt grows with every slice.

## Notes
- The README and design file changes inside the slice window were made by
  the reviewer, not by the slice; no rule was broken by the executor.
- No AI attribution in any commit since cdf44b7. Four older commits (PRs
  12, 15, 16, 17) carry a `Co-authored-by` trailer with a non-existent
  address; removing it needs a history rewrite, decided separately.
- The restricted-vocabulary scan checks file content only against the
  private list; the fixed list in the script applies to commit and PR text.
