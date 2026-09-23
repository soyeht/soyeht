#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
: "${FORBIDDEN_TERMS:?Provide the restricted vocabulary through the environment}"

run_exact_rust_test() {
    local expected="$1"
    shift
    "$@" 2>&1 | python3 "$repo_root/scripts/require_one_rust_test.py" "$expected"
}

# Optional scanner arguments cover the exact PR base and final title/body.
python3 -m unittest discover -s scripts -p 'test_forbidden_terms.py'
python3 scripts/forbidden_terms.py "$@"
unset FORBIDDEN_TERMS

python3 -m unittest discover -s scripts -p 'test_hardening_gate.py'
python3 -m unittest discover -s scripts -p 'test_require_one_rust_test.py'
python3 scripts/hardening_gate.py check-no-sql-outside-store "$@"
python3 scripts/hardening_gate.py check-no-panic-in-production "$@"
python3 scripts/hardening_gate.py check-verification-not-weakened "$@"
python3 scripts/hardening_gate.py check-function-length "$@"
python3 -m unittest discover -s scripts -p 'test_check_quality.py'
python3 scripts/check_quality.py

python3 contracts/generate.py --check
python3 -m unittest discover -s contracts -p 'test_*.py'
python3 -m unittest discover -s scripts -p 'test_install_dev.py'

(
    cd engine
    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo build --locked --workspace
    cargo test --locked --workspace
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
python3 scripts/e2e.py --report .cache/e2e.json
python3 scripts/e2e_agents.py --report .cache/e2e-agents.json
python3 scripts/e2e_tasks.py --report .cache/e2e-tasks.json
python3 scripts/e2e_editor.py --report .cache/e2e-editor.json
python3 scripts/e2e_web.py --report .cache/e2e-web.json
python3 scripts/e2e_writing.py --report .cache/e2e-writing.json
if [[ "${SOYEHT_REAL_ACP:-}" == 1 ]]; then
    python3 scripts/e2e_real_acp.py --report .cache/e2e-real-acp.json
fi

bash scripts/check_swift.sh
python3 -m unittest discover -s scripts -p 'test_check_mac_text_fonts.py'
python3 scripts/check_mac_text_fonts.py
python3 -m unittest discover -s scripts -p 'test_check_mac_identifier_text.py'
python3 scripts/check_mac_identifier_text.py
python3 -m unittest discover -s scripts -p 'test_check_mac_view_colors.py'
python3 scripts/check_mac_view_colors.py
swift format lint --strict --recursive apps/shared/Sources apps/shared/Tests apps/mac/Sources apps/ios/Sources
swift test --package-path apps/shared
bash scripts/check_mac.sh
python3 -m unittest discover -s scripts -p 'test_check_editor_performance.py'
python3 scripts/check_editor_performance.py
bash scripts/check_ios.sh

(
    cd contracts/typescript
    npm ci --ignore-scripts
    npm run check
    npm test
)
echo 'check_all: passed'
