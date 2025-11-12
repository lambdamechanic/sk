#!/usr/bin/env bash
set -euo pipefail

THRESHOLD=${THRESHOLD:-40}

echo "[coverage] threshold: ${THRESHOLD}%"
rustup component add llvm-tools-preview >/dev/null 2>&1 || true
if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  cargo install --locked cargo-llvm-cov@0.6.9
fi

cargo llvm-cov --workspace --fail-under-lines "${THRESHOLD}" --summary-only

