#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
default_dumbwaiter_root="${repo_root}/../dumbwaiter"
dumbwaiter_root="${DUMBWAITER_ROOT:-$default_dumbwaiter_root}"

if [[ ! -d "$dumbwaiter_root" ]]; then
  echo "Dumbwaiter repo not found at '$dumbwaiter_root'. Set DUMBWAITER_ROOT to its path." >&2
  exit 1
fi

cd "$dumbwaiter_root"
exec dumbwaiter-mcp "$@"
