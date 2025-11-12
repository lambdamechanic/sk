#!/usr/bin/env bash
set -euo pipefail

# Emits structured JSON about Codex review state for a PR using only `gh`.
#
# Fields:
#   pr, url, headSha, headCommittedAt
#   lastReviewRequestAt (time of latest comment containing "@codex review")
#   reviewRequestedForHead (bool)
#   codexThumbsUp (bool)  # any 👍 by Codex on PR or any PR comment
#   codexEyes (bool)      # any 👀 by Codex on PR or any PR comment
#   codexLatestCommentAt  # latest Codex top-level PR comment time
#   unreplied             # array of unresolved Codex inline threads with no human reply
#   unrepliedCount
#
# Usage:
#   scripts/codex-review-state.sh <pr-number>

if ! command -v gh >/dev/null 2>&1; then echo '{"error":"gh not found"}'; exit 2; fi
if ! command -v jq >/dev/null 2>&1; then echo '{"error":"jq not found"}'; exit 2; fi

if [[ $# -lt 1 ]]; then echo '{"error":"usage: codex-review-state.sh <pr-number>"}'; exit 2; fi

PR_NUM=$1
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
OWNER=${REPO%%/*}
NAME=${REPO##*/}

# Codex actor names; override via CODEX_LOGINS env (CSV)
CODEX_CSV=${CODEX_LOGINS:-chatgpt-codex-connector,chatgpt-codex-connector[bot]}

GQL=$(cat <<'EOF'
query($owner:String!, $name:String!, $number:Int!){
  repository(owner:$owner, name:$name){
    pullRequest(number:$number){
      number url isDraft headRefOid
      commits(last:1){ nodes{ commit{ oid committedDate } } }
      reactionGroups{ content users(first:100){ nodes{ login } } }
      comments(last:100){ nodes{ author{ login } body createdAt reactionGroups{ content users(first:100){ nodes{ login } } } } }
      reviewThreads(first:100){ nodes{ isResolved path comments(first:50){ nodes{ author{ login } body url } } } }
    }
  }
}
EOF
)

RAW=$(gh api graphql -f query="$GQL" -F owner="$OWNER" -F name="$NAME" -F number="$PR_NUM")

# Helper: find thumbs/eyes on PR or comments by Codex
parse_js='def codex: ($codex_csv | split(","));
def has_codex_up(rgs): (rgs // []) | any(.content=="THUMBS_UP" and (.users.nodes | any(.login as $l | codex | index($l))));
def has_codex_eyes(rgs): (rgs // []) | any(.content=="EYES" and (.users.nodes | any(.login as $l | codex | index($l))));

  .data.repository.pullRequest as $pr
  | {
      pr: $pr.number,
      url: $pr.url,
      headSha: ($pr.headRefOid),
      headCommittedAt: ($pr.commits.nodes[0].commit.committedDate),
      lastReviewRequestAt: ($pr.comments.nodes | map(select((.body // "") | contains("@codex review"))) | (max_by(.createdAt) | .createdAt)?),
      codexThumbsUp: (has_codex_up($pr.reactionGroups) or ( $pr.comments.nodes | any(has_codex_up(.reactionGroups)) ) ),
      codexEyes: (has_codex_eyes($pr.reactionGroups) or ( $pr.comments.nodes | any(has_codex_eyes(.reactionGroups)) ) ),
      codexLatestCommentAt: ($pr.comments.nodes | map(select(.author.login as $a | codex | index($a))) | (max_by(.createdAt) | .createdAt)?),
      unreplied: ($pr.reviewThreads.nodes
        | map(select(.isResolved==false))
        | map({path, comments: .comments.nodes, codex_present: (.comments.nodes | any(.author.login as $a | codex | index($a))),
               non_codex_present: (.comments.nodes | any(.author.login as $a | (codex | index($a) | not)))})
        | map(select(.codex_present and (.non_codex_present | not)))
        | map({path, firstAuthor: (.comments[0].author.login), excerpt: (.comments[0].body | gsub("\n";" ") | .[:160]), url: (.comments[0].url)})
      )
    }
'

JSON=$(echo "$RAW" | jq --arg codex_csv "$CODEX_CSV" "$parse_js")

# Derive reviewRequestedForHead
JSON=$(echo "$JSON" | jq '. as $x | $x + { reviewRequestedForHead: ( ($x.lastReviewRequestAt != null) and ((.headCommittedAt != null) and ($x.lastReviewRequestAt >= $x.headCommittedAt)) ) }')

# Add counts
JSON=$(echo "$JSON" | jq '. + {unrepliedCount: (.unreplied | length)}')

echo "$JSON"
