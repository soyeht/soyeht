#!/bin/bash
# The single list of merge-gate steps. CI workflows call this script and keep
# no step list of their own (scripts/test_gate_list.py enforces it).
#
#   check_all.sh [--tier fast|full] [--only STEP_ID[,STEP_ID...]] [--list] [scanner arguments]
#
# Every step is one `step AREA TIERS ID command...` line (ADR 0089):
#   AREA   always | gate_scripts | engine | desktop | swift | dictation | mac_app | ios_app
#          | typescript | e2e | editor_perf | timing
#   TIERS  both  runs in the fast and the full tier
#          full  runs only in the full tier
#          fast  runs only in the fast tier (a cheaper stand-in for a full step)
# The full tier (the default) runs every `both` and `full` step, as it always did. The
# fast tier, which runs before a merge, is cheap (a few minutes on a warm cache): the
# `always` steps plus format, lint, a build and the tests of the `both` and `fast` steps of
# the areas that scripts/gate_plan.py selects from the changed paths. Those tests are the unit
# tests of the engine and Swift, the whole `cargo test` of the desktop, and the e2e suites
# when the change reaches the engine's programs. The engine integration tests, the Swift
# tests, the Mac and iOS app builds, the editor measurement and the timing tests are `full`
# steps. The unit tests of the gate scripts are the `gate_scripts` area: a change under
# scripts/, contracts/*.py or .githooks/ selects it (and the full tier runs it). The full
# tier runs on main after the merge
# (scripts/post_merge.py, scripts/main_watch.py), on a remote Linux host for the areas
# that run there (--remote, scripts/remote_gate.sh). A fast step that stands in for a
# full one is named `<full id>-changed`, `<full id>-fast` or `<full id>-build`
# (test_gate_list.py checks that). A change to the scripts that run the gate upgrades a
# fast run to the full tier. Exit codes: 64 bad usage, 70 unknown tier in a step,
# 71 unknown area in a step, 78 --only names no step.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

tier=full
only=""
list=0
base=""
gate_args=()
while (($#)); do
    case "$1" in
        --tier) tier="${2:?--tier needs fast or full}"; shift 2 ;;
        --tier=*) tier="${1#--tier=}"; shift ;;
        --only) only="${2:?--only needs a step id}"; shift 2 ;;
        --list) list=1; shift ;;
        --base) base="${2:?--base needs a commit}"; gate_args+=("$1" "$2"); shift 2 ;;
        --base=*) base="${1#--base=}"; gate_args+=("$1"); shift ;;
        *) gate_args+=("$1"); shift ;;
    esac
done
case "$tier" in fast | full) ;; *) echo "check_all: --tier must be fast or full" >&2; exit 64 ;; esac
known_areas=" always gate_scripts engine desktop swift dictation mac_app ios_app typescript e2e editor_perf timing "
set -- ${gate_args[@]+"${gate_args[@]}"}
# A contributor without the vocabulary sets SOYEHT_SKIP_FORBIDDEN_TERMS=1: the scan prints
# that it was skipped (scripts/forbidden_terms.py), the run is recorded as not merge-grade, and
# a maintainer runs the real scan before the merge. Supplied terms always mean a real scan.
skipped_vocabulary=0
if ((!list)); then
    if [[ "${SOYEHT_SKIP_FORBIDDEN_TERMS:-}" != 1 || -n "${FORBIDDEN_TERMS:-}" ]]; then
        : "${FORBIDDEN_TERMS:?Provide the restricted vocabulary through the environment (outside contributors: SOYEHT_SKIP_FORBIDDEN_TERMS=1, see CONTRIBUTING.md)}"
    else
        skipped_vocabulary=1
    fi
fi

# A shared, content-addressed compile cache: safe across worktrees, unlike a shared
# target directory. Incremental crates are not cached, so the gate turns them off.
if [[ -z "${RUSTC_WRAPPER:-}" ]] && command -v sccache >/dev/null 2>&1; then
    export RUSTC_WRAPPER=sccache
    if [[ "$(uname)" == Darwin ]]; then
        export SCCACHE_DIR="${SCCACHE_DIR:-$HOME/Library/Caches/soyeht-sccache}"
    else
        export SCCACHE_DIR="${SCCACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/soyeht-sccache}"
    fi
    export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
    # A broken cache must not fail every cargo step: build without it. (--show-stats starts
    # the server when it is not running; --start-server fails when it already runs.)
    if ((!list)) && ! sccache --show-stats >/dev/null 2>&1; then
        echo 'check_all: warning: sccache does not start; building without it' >&2
        unset RUSTC_WRAPPER SCCACHE_DIR
    fi
fi

# Per-step wall times: each step appends a line; the EXIT trap writes the JSON.
timings_out="${SOYEHT_GATE_TIMINGS:-$repo_root/.cache/gate-timings.json}"
step_log="$(mktemp "${TMPDIR:-/tmp}/soyeht-gate-steps.XXXXXX")"
GATE_AREAS="always gate_scripts engine desktop swift dictation mac_app ios_app typescript e2e editor_perf timing"
GATE_ENGINE_TEST_ARGS="--workspace"
matched=0

finish() {
    local code=$?
    trap - EXIT
    if [[ -s "$step_log" ]]; then
        local label="$tier"
        [[ -z "$only" ]] || label="only:$only"
        ((!skipped_vocabulary)) || label="$label+vocabulary-skipped"
        uv run python scripts/gate_timings.py finalize --steps "$step_log" \
            --tier "$label" --exit-code "$code" \
            --areas "$GATE_AREAS" --out "$timings_out" || echo 'check_all: could not write the timings' >&2
    fi
    rm -f "$step_log"
    if [[ "${RUSTC_WRAPPER:-}" == sccache && $code -eq 0 ]] && ((!list)); then
        sccache --show-stats 2>/dev/null | grep -E '^(Compile requests|Cache hits  |Cache misses  )' || true
    fi
    exit "$code"
}
trap finish EXIT

now() { perl -MTime::HiRes=time -e 'printf "%.2f\n", time' 2>/dev/null || date +%s; }

area_selected() {
    local area="$1"
    [[ "$area" == always || " $GATE_AREAS " == *" $area "* ]]
}

should_run() {
    local area="$1" tiers="$2" id="$3"
    case "$tiers" in
        both | full | fast) ;;
        *) echo "check_all: unknown tiers '$tiers' for $id" >&2; exit 70 ;;
    esac
    if [[ "$known_areas" != *" $area "* ]]; then
        echo "check_all: unknown area '$area' for $id" >&2
        exit 71
    fi
    if [[ -n "$only" ]]; then
        [[ ",$only," == *",$id,"* ]]
        return
    fi
    case "$tiers" in
        full) [[ "$tier" == full ]] || return 1 ;;
        fast) [[ "$tier" == fast ]] || return 1 ;;
    esac
    area_selected "$area"
}

# step AREA TIERS ID command...
# The command runs with errexit off (it is the left side of `||`), so a function that runs
# several commands must chain them with `&&` or check each one; the last status is the step's.
step() {
    local area="$1" tiers="$2" id="$3"
    shift 3
    should_run "$area" "$tiers" "$id" || return 0
    matched=1
    if ((list)); then
        printf '%-11s %-5s %s\n' "$area" "$tiers" "$id"
        return 0
    fi
    echo "check_all: [$area] $id"
    local started status=0
    started="$(now)"
    "$@" || status=$?
    printf '%s\t%s\t%s\t%s\n' "$id" "$area" "$(perl -e 'printf "%.2f", $ARGV[1] - $ARGV[0]' "$started" "$(now)")" "$status" >>"$step_log"
    if ((status)); then
        echo "check_all: FAILED step $id (area $area, exit $status)" >&2
        exit "$status"
    fi
}

in_dir() {
    local directory="$1"
    shift
    (cd "$directory" && "$@")
}

run_exact_rust_test() {
    local expected="$1"
    shift
    "$@" 2>&1 | uv run python "$repo_root/scripts/require_one_rust_test.py" "$expected"
}

# Windows-only code has no host build here, so lint it for the Windows target.
lint_windows() {
    if ! rustup target list --installed | grep -qx x86_64-pc-windows-msvc; then
        echo 'check_all: skipped the Windows lint; run: rustup target add x86_64-pc-windows-msvc'
        return 0
    fi
    local packages=(-p soyeht-platform -p soyeht-process -p soyeht-ptyd -p soyeht-mcp)
    if windows_c_compiler; then
        packages+=(-p soyeht-engine -p soyeht-cli)
    else
        echo 'check_all: skipped the Windows lint of the engine and cli: their SQLite and tree-sitter build scripts need a C compiler that targets Windows (clang-cl with the Windows SDK, for instance through cargo-xwin)'
    fi
    cargo clippy --locked --target x86_64-pc-windows-msvc "${packages[@]}" --all-targets -- -D warnings
}

# Whether the C compiler the `cc` crate would pick can build for Windows. The engine's
# dependencies compile C, and a host without a Windows sysroot fails in their build scripts.
windows_c_compiler() {
    local compiler="${CC_x86_64_pc_windows_msvc:-${TARGET_CC:-${CC:-cc}}}"
    printf '#include <stdlib.h>\n' |
        $compiler --target=x86_64-pc-windows-msvc -fsyntax-only -x c - >/dev/null 2>&1
}

# The engine's contract checks (ADR 0090) are on in every build and test run of the gate, so
# each engine test and end-to-end run also checks the events and results against the contract.
# A changed-crates run turns them on whenever it tests the engine crate.
engine_test_changed() {
    local features=()
    case " $GATE_ENGINE_TEST_ARGS " in
        *" --workspace "* | *" -p soyeht-engine "*) features=(--features soyeht-engine/contract-checks) ;;
    esac
    # shellcheck disable=SC2086
    cargo test --locked $GATE_ENGINE_TEST_ARGS ${features[@]+"${features[@]}"} "$@"
}

capture_test() {
    local test="$1"
    run_exact_rust_test "$test" cargo test --locked -p soyeht-engine "$test" \
        -- --ignored --exact --nocapture --test-threads=1
}

GATE_UPGRADE=""
if [[ -z "$only" ]]; then
    # The plan comes first in the log. A base that cannot be resolved stops the gate.
    # SOYEHT_GATE_FILES_FROM names a file of changed paths (for tests); git is not read then.
    plan_env="$(uv run python scripts/gate_plan.py --tier "$tier" --format shell ${base:+--base "$base"} ${SOYEHT_GATE_FILES_FROM:+--files-from "$SOYEHT_GATE_FILES_FROM"})"
    eval "$plan_env"
    if [[ "$tier" == fast && -n "$GATE_UPGRADE" ]]; then
        echo "check_all: the gate scripts changed ($GATE_UPGRADE); a partial run cannot vouch for them, running the full tier" >&2
        tier=full
        plan_env="$(uv run python scripts/gate_plan.py --tier full --format shell)"
        eval "$plan_env"
    fi
fi

# Optional scanner arguments cover the exact PR base and final title/body.
step always both forbidden-terms:unit uv run python -m unittest discover -s scripts -p 'test_forbidden_terms.py'
step always both forbidden-terms:scan uv run python scripts/forbidden_terms.py "$@"
unset FORBIDDEN_TERMS

step always both hardening:unit uv run python -m unittest discover -s scripts -p 'test_hardening_gate.py'
step gate_scripts both gate-list:unit uv run python -m unittest discover -s scripts -p 'test_gate_list.py'
step always both gate-plan:unit uv run python -m unittest discover -s scripts -p 'test_gate_plan.py'
step gate_scripts both gate-timings:unit uv run python -m unittest discover -s scripts -p 'test_gate_timings.py'
step gate_scripts both post-merge:unit uv run python -m unittest discover -s scripts -p 'test_post_merge.py'
step gate_scripts both post-merge-waiver:unit uv run python -m unittest discover -s scripts -p 'test_post_merge_waiver.py'
step gate_scripts both main-watch:unit uv run python -m unittest discover -s scripts -p 'test_main_watch.py'
step always both require-one-rust-test:unit uv run python -m unittest discover -s scripts -p 'test_require_one_rust_test.py'
# Without --base, the ratchets compare with the merge-base of origin/main and
# HEAD and stop the gate when it cannot be resolved.
step always both hardening:no-sql-outside-store uv run python scripts/hardening_gate.py check-no-sql-outside-store "$@"
step always both hardening:no-panic-in-production uv run python scripts/hardening_gate.py check-no-panic-in-production "$@"
step always both hardening:verification-not-weakened uv run python scripts/hardening_gate.py check-verification-not-weakened "$@"
step always both hardening:function-length uv run python scripts/hardening_gate.py check-function-length "$@"
step gate_scripts both quality:unit uv run python -m unittest discover -s scripts -p 'test_check_quality.py'
step always both pane-interior:unit uv run python -m unittest discover -s scripts -p 'test_check_pane_interior.py'
step always both swallowed-errors:unit uv run python -m unittest discover -s scripts -p 'test_check_swallowed_errors.py'
step always both wire-names:unit uv run python -m unittest discover -s scripts -p 'test_check_wire_names.py'
step always both wire-names:check uv run python scripts/check_wire_names.py
step always both no-test-backdoors:unit uv run python -m unittest discover -s scripts -p 'test_check_no_test_backdoors.py'
step always both no-test-backdoors:check uv run python scripts/check_no_test_backdoors.py
step always both repo-files:unit uv run python -m unittest discover -s scripts -p 'test_check_repo_files.py'
# Like the ratchets, it compares with the merge-base unless --base is passed.
step always both repo-files:check uv run python scripts/check_repo_files.py "$@"
step always both doc-names:unit uv run python -m unittest discover -s scripts -p 'test_check_doc_names.py'
step always both doc-names:check uv run python scripts/check_doc_names.py
step always both layers:unit uv run python -m unittest discover -s scripts -p 'test_check_layers.py'
step always both layers:check uv run python scripts/check_layers.py
step always both quality:check uv run python scripts/check_quality.py
step always both dead-pub:unit uv run python -m unittest discover -s scripts -p 'test_check_dead_pub.py'
step always both dead-pub:check uv run python scripts/check_dead_pub.py
step always both e2e:unit uv run python -m unittest discover -s scripts -p 'test_e2e_platform.py'
step always both dead-public-swift:unit uv run python -m unittest discover -s scripts -p 'test_check_dead_public_swift.py'
step always both dead-public-swift:check uv run python scripts/check_dead_public_swift.py
step always both logging:unit uv run python -m unittest discover -s scripts -p 'test_check_logging.py'
step always both logging:check uv run python scripts/check_logging.py
step always both desktop-tokens:unit uv run python -m unittest discover -s scripts -p 'test_check_desktop_tokens.py'
step always both desktop-tokens:check uv run python scripts/check_desktop_tokens.py
step always both event-literals:unit uv run python -m unittest discover -s scripts -p 'test_check_event_literals.py'
step always both event-literals:check uv run python scripts/check_event_literals.py
step always both legacy-isolation:unit uv run python -m unittest discover -s scripts -p 'test_check_legacy_isolation.py'
step always both legacy-isolation:check uv run python scripts/check_legacy_isolation.py

step always both contracts:generate-check uv run python contracts/generate.py --check
step always both contracts:unit uv run python -m unittest discover -s contracts -p 'test_*.py'
step always both install-dev:unit uv run python -m unittest discover -s scripts -p 'test_install_dev.py'
step always both agent-precheck:unit uv run python -m unittest discover -s scripts -p 'test_agent_precheck.py'

step engine both engine:fmt in_dir engine cargo fmt --all --check
step engine both engine:clippy in_dir engine cargo clippy --locked --workspace --all-targets -- -D warnings
# A shipping build checks nothing against the contract; lint it and the checked one.
step engine both engine:clippy-contract-checks in_dir engine cargo clippy --locked --workspace --all-targets --features soyeht-engine/contract-checks -- -D warnings
# The engine is built with the checks on (ADR 0090), so the tests that start it and the
# end-to-end runs also check its events and results against the contract.
step engine both engine:build in_dir engine cargo build --locked --workspace --features soyeht-engine/contract-checks
# Integration tests (the crates' tests/ directories, which start sibling binaries) run here only.
step engine full engine:test in_dir engine cargo test --locked --workspace --features soyeht-engine/contract-checks
# Unit tests (src/ of libraries and binaries) of the changed crates and their reverse dependencies.
step engine fast engine:test-changed in_dir engine engine_test_changed --lib --bins
# The shipping configuration has the checks off: the one test that says an unchecked build
# validates nothing runs without the feature, as the timing steps below do.
step engine both engine:test-unchecked in_dir engine run_exact_rust_test \
    model::tests::without_the_checks_nothing_is_validated \
    cargo test --locked -p soyeht-engine --lib model::tests::without_the_checks_nothing_is_validated -- --exact
step engine both engine:clippy-windows in_dir engine lint_windows
step timing full engine:timing-frame-dense in_dir engine capture_test \
    terminal_capture::tests::frame_dense_worker_headroom_reports_queue_pressure_and_time_coverage
step timing full engine:timing-capture-cadence in_dir engine capture_test \
    terminal_capture::tests::capture_cadence_measurement_uses_ten_paired_noisy_samples

step desktop both desktop:fmt in_dir apps/desktop cargo fmt --all --check
step desktop both desktop:clippy in_dir apps/desktop cargo clippy --locked --workspace --all-targets -- -D warnings
# The design-audit tools (`dev-tools`) are not in the shipping build: lint and test them too.
step desktop both desktop:clippy-all-features in_dir apps/desktop cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
# The whole test run, `tests/` directories included: they drive the client against a scripted
# engine in seconds and start no sibling binary.
step desktop both desktop:test in_dir apps/desktop cargo test --locked --workspace --all-features

step always both tcc-guard:unit uv run python -m unittest discover -s scripts -p 'test_tcc_guard.py'
# The engine programs that the suites start must be the shipped build, and every `cargo test` of
# the engine workspace (engine:test, engine:test-changed, engine:test-unchecked and the timing
# steps, all above) links target/debug/soyeht-engine from the library with the test-hooks feature
# on (the engine lists itself as a development dependency). So these two builds come after every
# one of them and just before the suites: the product programs without the hooks (the contract
# checks stay on, ADR 0090), then the engine with the test hooks (ADR 0091), which the editor
# acceptance starts. No product build has it.
step e2e both engine:build-product in_dir engine cargo build --locked --workspace --features soyeht-engine/contract-checks
step e2e both engine:build-test-engine in_dir engine cargo build --locked -p soyeht-engine --features test-hooks,contract-checks --bin soyeht-engine-test
step e2e both e2e:tcc-snapshot uv run python scripts/tcc_guard.py snapshot .cache/tcc-guard.json
step e2e both e2e:core uv run python scripts/e2e.py --report .cache/e2e.json
step e2e both e2e:agents uv run python scripts/e2e_agents.py --report .cache/e2e-agents.json
step e2e both e2e:tasks uv run python scripts/e2e_tasks.py --report .cache/e2e-tasks.json
step e2e both e2e:editor uv run python scripts/e2e_editor.py --report .cache/e2e-editor.json
step e2e both e2e:web uv run python scripts/e2e_web.py --report .cache/e2e-web.json
step e2e both e2e:writing uv run python scripts/e2e_writing.py --report .cache/e2e-writing.json
if [[ "${SOYEHT_REAL_ACP:-}" == 1 ]]; then
    step e2e full e2e:real-acp uv run python scripts/e2e_real_acp.py --report .cache/e2e-real-acp.json
    step e2e full e2e:real-agent-messaging uv run python scripts/e2e_real_agent_messaging.py --report .cache/e2e-real-agent-messaging.json
    step e2e full e2e:real-harnesses uv run python scripts/e2e_real_harnesses.py --report .cache/e2e-real-harnesses.json
fi
step e2e both e2e:tcc-check uv run python scripts/tcc_guard.py check .cache/tcc-guard.json

step swift full swift:check-swift bash scripts/check_swift.sh
step always both mac-text-fonts:unit uv run python -m unittest discover -s scripts -p 'test_check_mac_text_fonts.py'
step always both mac-text-fonts:check uv run python scripts/check_mac_text_fonts.py
step always both mac-environment:unit uv run python -m unittest discover -s scripts -p 'test_check_mac_environment.py'
step always both mac-environment:check uv run python scripts/check_mac_environment.py
step always both mac-identifier-text:unit uv run python -m unittest discover -s scripts -p 'test_check_mac_identifier_text.py'
step always both mac-identifier-text:check uv run python scripts/check_mac_identifier_text.py
step always both mac-view-colors:unit uv run python -m unittest discover -s scripts -p 'test_check_mac_view_colors.py'
step always both mac-view-colors:check uv run python scripts/check_mac_view_colors.py
step swift both swift:format-lint swift format lint --strict --recursive apps/shared/Sources apps/shared/Tests apps/dictation/Sources apps/dictation/Tests apps/mac/Sources apps/ios/Sources
# EngineConnectionTests and PhoneConnectionTests launch the engine binaries from engine/target,
# so the Swift tests need them built (with the contract checks on, like every engine build).
step swift full swift:engine-binaries in_dir engine cargo build --locked --workspace --features soyeht-engine/contract-checks
step swift full swift:test-shared swift test --package-path apps/shared
# The fast stand-in compiles the shared package without running its tests.
step swift fast swift:test-shared-build swift build --package-path apps/shared
step dictation full swift:test-dictation swift test --package-path apps/dictation
step mac_app full mac-app:build bash scripts/check_mac.sh
step always both editor-performance:unit uv run python -m unittest discover -s scripts -p 'test_check_editor_performance.py'
# The Rust half of the editor check is an engine step: the post-merge run sends it to the quiet
# Linux host when one is given, because a loaded host trips its latency limits (ADR 0092).
step engine full editor-performance:rust uv run python scripts/check_editor_performance.py --rust
step editor_perf full editor-performance:check uv run python scripts/check_editor_performance.py
step ios_app full ios-app:build bash scripts/check_ios.sh

step typescript both typescript:install in_dir contracts/typescript npm ci --ignore-scripts
step typescript both typescript:check in_dir contracts/typescript npm run check
step typescript both typescript:test in_dir contracts/typescript npm test
if [[ -n "$only" && $matched -eq 0 ]]; then
    echo "check_all: no step with id $only" >&2
    exit 78
fi
if ((!list)); then
    if ((skipped_vocabulary)); then
        echo "check_all: passed (tier $tier; vocabulary scan SKIPPED, not merge-grade)"
    else
        echo "check_all: passed (tier $tier)"
    fi
fi
