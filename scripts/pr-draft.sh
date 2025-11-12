#!/usr/bin/env bash
set -euo pipefail

# Open or update a Draft PR for the current branch using GitHub CLI if available.
# Usage:
#   scripts/pr-draft.sh "Title for PR" [body-file] [base]

TITLE=${1:-}
BODY_FILE=${2:-}
BASE_BRANCH=${3:-main}

if [[ -z "$TITLE" ]]; then
  echo "usage: scripts/pr-draft.sh \"Title\" [body-file] [base]" >&2
  exit 2
fi

branch=$(git rev-parse --abbrev-ref HEAD)
remote_url=$(git config --get remote.origin.url || true)

# Ensure branch is pushed
git push -u origin "$branch" >/dev/null 2>&1 || git push -u origin "$branch"

if command -v gh >/dev/null 2>&1; then
  if gh pr view "$branch" --json number >/dev/null 2>&1; then
    # Update title/body if provided
    if [[ -n "$BODY_FILE" && -f "$BODY_FILE" ]]; then
      gh pr edit "$branch" --title "$TITLE" --body-file "$BODY_FILE"
    else
      gh pr edit "$branch" --title "$TITLE"
    fi
  else
    if [[ -n "$BODY_FILE" && -f "$BODY_FILE" ]]; then
      gh pr create --head "$branch" --base "$BASE_BRANCH" --title "$TITLE" --body-file "$BODY_FILE" --draft
    else
      gh pr create --head "$branch" --base "$BASE_BRANCH" --title "$TITLE" --draft
    fi
  fi
else
  echo "warning: 'gh' not found. Please open a draft PR manually." >&2
  # Best-effort GitHub compare URL
  if [[ "$remote_url" =~ github.com[:/](.+)/(.+)(\.git)?$ ]]; then
    owner=${BASH_REMATCH[1]}
    repo=${BASH_REMATCH[2]}
    echo "Open: https://github.com/$owner/$repo/compare/$BASE_BRANCH...$branch?expand=1" >&2
  fi
fi

