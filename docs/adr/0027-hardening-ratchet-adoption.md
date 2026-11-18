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

Amendment (the test count and the waiver). The count of tests never bound: in 156 post-merge runs
it never dropped, and 23 of the 26 pull requests that carried a `Verification change:` line said
"none", which waived every part of the check at once. The check no longer counts tests; deleting a
useless test is welcome, and the gate prints the name of each test the head no longer has. It
compares acceptances by name and gate steps by the id of the `step` line of `check_all.sh`, not by
the words of their commands. A waiver names what it waives and why: `Verification change: removes
step <id> because <reason>`, `removes acceptance <name> because <reason>` or `ignores <test>
because <reason>`, one line per thing, with a reason of at least three words. A line that
names nothing waives nothing. A growth limit takes the place of the count: a change may add at most
400 net lines of test code unless the body carries `Test growth allowance: <N> lines because
<reason>` (`scripts/growth_gate.py`).

Hardening round 1 removes the inherited debt and enables the three Clippy panic
lints as absolute compile-time denials. The comparison base is always the exact
pull request base during a merge gate. A run without `--base` compares the
working tree with the merge-base of `origin/main` and `HEAD`, so committed and
uncommitted regressions on a branch are both visible. When that merge-base
cannot be resolved, the check fails instead of falling back to `HEAD`.
