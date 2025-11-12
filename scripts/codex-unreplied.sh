#!/usr/bin/env bash
set -euo pipefail

# List unresolved Codex inline review threads without a non-Codex reply.
# Requires: gh (GitHub CLI), jq
# Usage:
#   scripts/codex-unreplied.sh               # scan all open PRs
#   scripts/codex-unreplied.sh <pr-number>   # scan a specific PR

if ! command -v gh >/dev/null 2>&1; then
  echo "error: gh not found" >&2; exit 2
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq not found" >&2; exit 2
fi

repo=$(gh repo view --json nameWithOwner -q .nameWithOwner)
owner=${repo%%/*}
name=${repo##*/}

# Codex bot usernames (override with CODEX_LOGINS="user1,user2")
codex_csv=${CODEX_LOGINS:-chatgpt-codex-connector}
# jq-friendly array from CSV
codex_jq=$(printf '%s' "$codex_csv" | awk -F, '{printf "["; for(i=1;i<=NF;i++){printf (i>1?",":""); printf "\""$i"\""} printf "]"}')

prs=()
if [[ $# -ge 1 ]]; then
  prs=("$@")
else
  mapfile -t prs < <(gh pr list --state open --json number -q '.[].number')
fi

if [[ ${#prs[@]} -eq 0 ]]; then
  echo "No open PRs found."; exit 0
fi

exit_code=0
for pr in "${prs[@]}"; do
  # GraphQL: reviewThreads with comments
  json=$(gh api graphql -f query='query($owner:String!, $name:String!, $number:Int!){
    repository(owner:$owner, name:$name){
      pullRequest(number:$number){
        url
        number
        reviewThreads(first:100){
          nodes{
            isResolved
            path
            comments(first:50){
              nodes{ id url body createdAt author{ login } }
            }
          }
        }
      }
    }
  }' -F owner="$owner" -F name="$name" -F number="$pr")

  echo "$json" | jq --argjson codex "$codex_jq" '
    .data.repository.pullRequest as $pr
    | $pr.reviewThreads.nodes
    | map(select(.isResolved==false))
    | map({
        path: .path,
        comments: .comments.nodes,
        codex_present: (.comments.nodes | any(.author.login as $a | $codex | index($a))),
        non_codex_present: (.comments.nodes | any(.author.login as $a | ($codex | index($a) | not)))
      })
    | map(select(.codex_present and (non_codex_present | not)))
    | if length==0 then empty else {
        pr: $pr.number, url: $pr.url,
        pending: map({path, first: (.comments[0] | {author: .author.login, url: .url, excerpt: (.body|gsub("\n";" ")|.[:140])})})
      } end
  ' | jq -r 'if . then ("PR #" + (.pr|tostring) + " -> " + .url), (.pending[] | "  • " + .path + " — " + .first.author + " — " + .first.url + "\n    " + .first.excerpt) else empty end'

  if echo "$json" | jq --argjson codex "$codex_jq" '.data.repository.pullRequest.reviewThreads.nodes | map(select(.isResolved==false)) | map({comments:.comments.nodes,codex_present:(.comments.nodes|any(.author.login as $a|$codex|index($a))), non_codex_present:(.comments.nodes|any(.author.login as $a|($codex|index($a)|not)))}) | any(.codex_present and ( .non_codex_present|not))' | grep -q true; then
    exit_code=1
  fi
done

exit $exit_code

