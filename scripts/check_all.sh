#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
: "${FORBIDDEN_TERMS:?Provide the restricted vocabulary through the environment}"

# Optional scanner arguments cover the exact PR base and final title/body.
python3 -m unittest discover -s scripts -p 'test_forbidden_terms.py'
python3 scripts/forbidden_terms.py "$@"
unset FORBIDDEN_TERMS

python3 -m unittest discover -s scripts -p 'test_hardening_gate.py'
python3 scripts/hardening_gate.py check-no-sql-outside-store "$@"
python3 scripts/hardening_gate.py check-no-panic-in-production "$@"
python3 scripts/hardening_gate.py check-verification-not-weakened "$@"
python3 scripts/hardening_gate.py check-function-length "$@"

python3 contracts/generate.py --check
python3 -m unittest discover -s contracts -p 'test_*.py'
python3 -m unittest discover -s scripts -p 'test_install_dev.py'

(
    cd engine
    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo build --locked --workspace
    cargo test --locked --workspace
)
python3 scripts/e2e.py --report .cache/e2e.json
python3 scripts/e2e_agents.py --report .cache/e2e-agents.json
python3 scripts/e2e_tasks.py --report .cache/e2e-tasks.json
if [[ "${SOYEHT_REAL_ACP:-}" == 1 ]]; then
    python3 scripts/e2e_real_acp.py --report .cache/e2e-real-acp.json
fi

bash scripts/check_swift.sh
python3 -m unittest discover -s scripts -p 'test_check_mac_text_fonts.py'
python3 scripts/check_mac_text_fonts.py
swift format lint --strict --recursive apps/shared/Sources apps/shared/Tests apps/mac/Sources apps/ios/Sources
swift test --package-path apps/shared
bash scripts/check_mac.sh
bash scripts/check_ios.sh

(
    cd contracts/typescript
    npm ci --ignore-scripts
    npm run check
    npm test
)
echo 'check_all: passed'
