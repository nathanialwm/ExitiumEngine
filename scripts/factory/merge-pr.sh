#!/usr/bin/env bash
# Squash-merges a factory PR once every required check on its head commit has
# passed. This is the factory's merge gate: GitHub branch protection and native
# auto-merge are unavailable on a private repo without GitHub Pro, so the
# orchestrator merges through this script instead of `gh pr merge --auto`.
#
# Usage: scripts/factory/merge-pr.sh <pr-number>
# Exit codes: 0 merged, 1 a check failed, 2 checks still pending.
set -euo pipefail

pr="${1:?usage: merge-pr.sh <pr-number>}"
repo=nathanialwm/ExitiumEngine
required_statuses=(pr-title human-gate factory/ai-review)
required_check=ci-pass

here="$(cd "$(dirname "$0")" && pwd)"
export GH_TOKEN="$("$here/gh-app-token.sh")"

read -r state head base < <(gh pr view "$pr" --repo "$repo" --json state,headRefOid,baseRefName \
  --jq '[.state, .headRefOid, .baseRefName] | join(" ")')
[ "$state" = OPEN ] || { echo "PR #$pr is $state"; exit 1; }
[ "$base" = main ] || { echo "PR #$pr targets $base, not main"; exit 1; }

pending=0
failed=0
statuses=$(gh api "repos/$repo/commits/$head/status" --jq '.statuses[] | "\(.context) \(.state)"')
for context in "${required_statuses[@]}"; do
  result=$(awk -v c="$context" '$1 == c { print $2; exit }' <<< "$statuses")
  case "$result" in
    success) echo "ok       $context" ;;
    pending | "") echo "pending  $context"; pending=1 ;;
    *) echo "FAILED   $context ($result)"; failed=1 ;;
  esac
done

conclusion=$(gh api "repos/$repo/commits/$head/check-runs?check_name=$required_check" \
  --jq '.check_runs | sort_by(.started_at) | last | if . == null then "missing" elif .status != "completed" then "pending" else .conclusion end')
case "$conclusion" in
  success) echo "ok       $required_check" ;;
  pending | missing) echo "pending  $required_check"; pending=1 ;;
  *) echo "FAILED   $required_check ($conclusion)"; failed=1 ;;
esac

[ "$failed" = 0 ] || exit 1
[ "$pending" = 0 ] || exit 2

# --match-head-commit refuses the merge if someone pushed after the checks ran.
gh pr merge "$pr" --repo "$repo" --squash --delete-branch --match-head-commit "$head"
