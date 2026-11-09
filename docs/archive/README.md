# Archive

Historical material from the first months of the rewrite. It records how Soyeht was planned,
built and checked. **It is not current documentation**: names, commands, file paths and
procedures in it may no longer exist, and `scripts/check_doc_names.py` does not check it.
For what is true today read `AGENTS.md`, `CONTRIBUTING.md`, `docs/contracts/` and `docs/adr/`.

| Path | What it is |
| --- | --- |
| `contracts/00-slice-plan.md` | The plan that split the work into ten vertical slices. |
| `contracts/00-slice-*-objective.md`, `00-slice-1.md` | The brief given for each slice: scope, acceptances, what "done" meant. |
| `contracts/00-hardening-1-objective.md`, `00-design-alignment-1-objective.md`, `00-fix-batch-before-slice-9-objective.md` | The briefs of three cleanup passes between slices. |
| `manual-acceptance-slice-1.md` to `-4.md` | The manual device procedures written for slices 1 to 4. |
| `reports/slice-N.md` and `reports/slice-N/` | The closing report of each slice, with the screenshots that compared the app with its design. |
| `reports/hardening-1.md`, `audit-2026-09-11.md`, `acceptance-11.md`, `fix-batch-before-slice-9.md`, `design-alignment-1*` | Reports of the cleanup passes and an audit. |
| `reports/human-mode-spike.md` | A decision spike on driving a web view with system events (ADR 0055). The spike's code was removed. |
| `reports/usage-reader-*.md`, `multi-terminal-cost.md`, `issue-413-timeout-investigation.md`, `new-pane-chooser-floor.md` | One-off investigations and measurements. |
| `reports/phone-writing-*.md`, `writing-layout-policy.md`, `responsive-layout-blocks-a-c.md` | Design and layout studies for the writing panes and the responsive layout. |

The reports that a check, a source comment or an ADR still points to stay in `docs/reports/`.
