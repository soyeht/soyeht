#!/bin/bash
# The single list of merge-gate steps. CI workflows call this script and keep
# no step list of their own (scripts/test_gate_list.py enforces it).
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
: "${FORBIDDEN_TERMS:?Provide the restricted vocabulary through the environment}"

run_exact_rust_test() {
    local expected="$1"
    shift
    "$@" 2>&1 | uv run python "$repo_root/scripts/require_one_rust_test.py" "$expected"
}

# Optional scanner arguments cover the exact PR base and final title/body.
uv run python -m unittest discover -s scripts -p 'test_forbidden_terms.py'
uv run python scripts/forbidden_terms.py "$@"
unset FORBIDDEN_TERMS

uv run python -m unittest discover -s scripts -p 'test_hardening_gate.py'
uv run python -m unittest discover -s scripts -p 'test_gate_list.py'
uv run python -m unittest discover -s scripts -p 'test_require_one_rust_test.py'
# Without --base, the ratchets compare with the merge-base of origin/main and
# HEAD and stop the gate when it cannot be resolved.
uv run python scripts/hardening_gate.py check-no-sql-outside-store "$@"
uv run python scripts/hardening_gate.py check-no-panic-in-production "$@"
uv run python scripts/hardening_gate.py check-verification-not-weakened "$@"
uv run python scripts/hardening_gate.py check-function-length "$@"
uv run python -m unittest discover -s scripts -p 'test_check_quality.py'
uv run python -m unittest discover -s scripts -p 'test_check_pane_interior.py'
uv run python -m unittest discover -s scripts -p 'test_check_swallowed_errors.py'
uv run python scripts/check_quality.py
uv run python -m unittest discover -s scripts -p 'test_check_dead_pub.py'
uv run python scripts/check_dead_pub.py

uv run python contracts/generate.py --check
uv run python -m unittest discover -s contracts -p 'test_*.py'
uv run python -m unittest discover -s scripts -p 'test_install_dev.py'

(
    cd engine
    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo build --locked --workspace
    cargo test --locked --workspace
    # Windows-only code has no host build here, so lint it for the Windows target.
    if rustup target list --installed | grep -qx x86_64-pc-windows-msvc; then
        cargo clippy --locked --target x86_64-pc-windows-msvc -p soyeht-platform -p soyeht-process -p soyeht-ptyd --all-targets -- -D warnings
    else
        echo 'check_all: skipped the Windows lint; run: rustup target add x86_64-pc-windows-msvc'
    fi
    run_exact_rust_test \
        terminal_capture::tests::frame_dense_worker_headroom_reports_queue_pressure_and_time_coverage \
        cargo test --locked -p soyeht-engine \
        terminal_capture::tests::frame_dense_worker_headroom_reports_queue_pressure_and_time_coverage \
        -- --ignored --exact --nocapture --test-threads=1
    run_exact_rust_test \
        terminal_capture::tests::capture_cadence_measurement_uses_ten_paired_noisy_samples \
        cargo test --locked -p soyeht-engine \
        terminal_capture::tests::capture_cadence_measurement_uses_ten_paired_noisy_samples \
        -- --ignored --exact --nocapture --test-threads=1
)
(
    cd apps/desktop
    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo test --locked --workspace
)
uv run python -m unittest discover -s scripts -p 'test_tcc_guard.py'
uv run python scripts/tcc_guard.py snapshot .cache/tcc-guard.json
uv run python scripts/e2e.py --report .cache/e2e.json
uv run python scripts/e2e_agents.py --report .cache/e2e-agents.json
uv run python scripts/e2e_tasks.py --report .cache/e2e-tasks.json
uv run python scripts/e2e_editor.py --report .cache/e2e-editor.json
uv run python scripts/e2e_web.py --report .cache/e2e-web.json
uv run python scripts/e2e_writing.py --report .cache/e2e-writing.json
if [[ "${SOYEHT_REAL_ACP:-}" == 1 ]]; then
    uv run python scripts/e2e_real_acp.py --report .cache/e2e-real-acp.json
    uv run python scripts/e2e_real_agent_messaging.py --report .cache/e2e-real-agent-messaging.json
    uv run python scripts/e2e_real_harnesses.py --report .cache/e2e-real-harnesses.json
fi
uv run python scripts/tcc_guard.py check .cache/tcc-guard.json

bash scripts/check_swift.sh
uv run python -m unittest discover -s scripts -p 'test_check_mac_text_fonts.py'
uv run python scripts/check_mac_text_fonts.py
uv run python -m unittest discover -s scripts -p 'test_check_mac_identifier_text.py'
uv run python scripts/check_mac_identifier_text.py
uv run python -m unittest discover -s scripts -p 'test_check_mac_view_colors.py'
uv run python scripts/check_mac_view_colors.py
swift format lint --strict --recursive apps/shared/Sources apps/shared/Tests apps/dictation/Sources apps/dictation/Tests apps/mac/Sources apps/ios/Sources
swift test --package-path apps/shared
swift test --package-path apps/dictation
bash scripts/check_mac.sh
uv run python -m unittest discover -s scripts -p 'test_check_editor_performance.py'
uv run python scripts/check_editor_performance.py
bash scripts/check_ios.sh

(
    cd contracts/typescript
    npm ci --ignore-scripts
    npm run check
    npm test
)
echo 'check_all: passed'
