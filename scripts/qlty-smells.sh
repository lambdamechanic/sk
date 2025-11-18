#!/usr/bin/env bash
set -o pipefail

QLTY_CMD="${QLTY:-qlty}"
out="$("$QLTY_CMD" smells --all --no-snippets)"
printf '%s\n' "$out"

# Adjust patterns as needed
if printf '%s\n' "$out" | grep -qE 'Function with high complexity|High total complexity|Found [0-9]+ lines of similar code'; then
  echo "qlty smells: smells found, failing build"
  exit 1
fi
