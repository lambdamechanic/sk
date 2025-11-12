#!/usr/bin/env bash
set -euo pipefail

# Mark the PR for the current branch Ready for review via GitHub CLI.

branch=$(git rev-parse --abbrev-ref HEAD)

if command -v gh >/dev/null 2>&1; then
  if gh pr view "$branch" --json number >/dev/null 2>&1; then
    gh pr ready "$branch" || {
      echo "error: failed to mark PR ready; ensure you have permissions." >&2
      exit 1
    }
  else
    echo "error: no PR found for branch '$branch'. Create one first (scripts/pr-draft.sh)." >&2
    exit 1
  fi
else
  echo "warning: 'gh' not found. Open the PR in GitHub and click 'Ready for review'." >&2
  exit 2
fi

