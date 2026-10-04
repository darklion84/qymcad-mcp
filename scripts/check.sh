#!/usr/bin/env bash
# The gate every commit must pass: formatting, lints, all tests.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
echo "check.sh: OK"
