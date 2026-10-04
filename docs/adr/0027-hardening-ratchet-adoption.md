# ADR 0027 · Adopt hardening gates without hiding inherited debt

Status: accepted for hardening round 1.

The hardening objective requires four checks to enter the merge gate before
the inherited violations are removed. Some production functions already exceed
the new absolute ceiling, and production code still contains database access
and panic-capable calls. Enforcing a zero ceiling in the first gate change
would make that change impossible to merge.

Each check therefore measures the pull request base and head. Existing debt is
visible in the output and may only decrease. A new SQL-bearing file fails. The
number of panic-capable calls, functions above 120 lines and functions above
200 lines may not increase, and the longest function may not grow beyond the
inherited maximum. Once a category reaches zero, any recurrence fails without
an allowlist or resettable baseline.

The verification check compares tests, deterministic acceptances and explicit
gate commands. A decrease fails unless the final pull request body contains a
non-empty `Verification change:` line explaining the reviewed replacement.
This exception explains a deliberate verification change; it does not disable
any other check.

Hardening round 1 removes the inherited debt and enables the three Clippy panic
lints as absolute compile-time denials. The comparison base is always the exact
pull request base during a merge gate. A run without `--base` compares the
working tree with the merge-base of `origin/main` and `HEAD`, so committed and
uncommitted regressions on a branch are both visible. When that merge-base
cannot be resolved, the check fails instead of falling back to `HEAD`.
