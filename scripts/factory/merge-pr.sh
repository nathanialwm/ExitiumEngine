#!/usr/bin/env bash
# Squash-merges a factory PR once every required check on its head commit has
# passed. This is the factory's merge gate: GitHub branch protection and native
# auto-merge are unavailable on a private repo without GitHub Pro, so the
# orchestrator merges through this script instead of `gh pr merge --auto`.
#
# A check only counts if it was posted by the expected author. The factory App
# can post statuses itself, so a status it wrote under a CI context is ignored.
# github-actions[bot] statuses can only be forged by a PR that edits
# .github/workflows, and the factory App lacks the Workflows permission, so only
# the owner can push such changes.
#
# Usage: scripts/factory/merge-pr.sh <pr-number>
# Exit codes: 0 merged, 1 a check failed, 2 checks still pending (or a
# needs-human PR waiting for the owner's human-approved label and status).
set -euo pipefail

pr="${1:?usage: merge-pr.sh <pr-number>}"
repo=nathanialwm/ExitiumEngine
actions_bot='github-actions[bot]'
factory_bot='exitium-factory[bot]'

here="$(cd "$(dirname "$0")" && pwd)"
# Assign before export: `export X="$(cmd)"` would hide a failed token fetch, and
# gh would silently fall back to the owner's own login.
token="$("$here/gh-app-token.sh")"
[ -n "$token" ] || { echo "could not get a factory App token"; exit 1; }
export GH_TOKEN="$token"

read -r state head base < <(gh pr view "$pr" --repo "$repo" --json state,headRefOid,baseRefName \
  --jq '[.state, .headRefOid, .baseRefName] | join(" ")')
[ "$state" = OPEN ] || { echo "PR #$pr is $state"; exit 1; }
[ "$base" = main ] || { echo "PR #$pr targets $base, not main"; exit 1; }

pending=0
failed=0

# Sets $newest to the newest state of a context from the expected creator, or
# "missing". The API lists statuses newest first, and --paginate reads every
# page, so a busy head can't push the status off the first page. gh's --jq runs
# on each page in turn, so the matching states print newest first.
# gh's built-in --jq is used because jq itself is not installed everywhere.
newest_state() {
  local context="$1" creator="$2" states
  states=$(gh api --paginate "repos/$repo/commits/$head/statuses?per_page=100" --jq \
    ".[] | select(.context == \"$context\" and .creator.login == \"$creator\") | .state")
  newest=$(head -n 1 <<< "$states")
  [ -n "$newest" ] || newest=missing
}

check_status() {
  local context="$1"
  newest_state "$context" "$2"
  case "$newest" in
    success) echo "ok       $context" ;;
    pending | missing) echo "pending  $context"; pending=1 ;;
    *) echo "FAILED   $context ($newest)"; failed=1 ;;
  esac
}
check_status pr-title "$actions_bot"
check_status human-gate "$actions_bot"
check_status factory/ai-review "$factory_bot"

conclusion=$(gh api "repos/$repo/commits/$head/check-runs?check_name=ci-pass&filter=latest" \
  --jq '[.check_runs[] | select(.app.slug == "github-actions")] | first
        | if . == null then "missing" elif .status != "completed" then "pending" else .conclusion end')
case "$conclusion" in
  success) echo "ok       ci-pass" ;;
  pending | missing) echo "pending  ci-pass"; pending=1 ;;
  *) echo "FAILED   ci-pass ($conclusion)"; failed=1 ;;
esac

# Prints the PR's current labels, one per line. Callers assign the output, so
# under `set -e` a failed read stops the script (exit 1): it fails closed.
pr_labels() {
  gh pr view "$pr" --repo "$repo" --json labels --jq '.labels[].name'
}

# An escalated (needs-human) PR needs the owner's approval, in two parts that
# must both hold:
# - The human-approved status on this head. pr-approval.yml posts `success` when
#   the owner applies the label. A push makes a new head with no status, so it
#   revokes approval. Checked directly so a lagging human-gate success is not
#   trusted.
# - The human-approved label on the PR now. No workflow posts a failure status
#   when the owner removes the label: pr-approval.yml runs on `labeled` only,
#   and human-gate stays success while any success approval exists on the head.
#   So a success status outlives a withdrawn approval with no time limit, and
#   the current label is the only withdrawal signal. Keep this check.
# A withdrawn approval, like a missing one, leaves the PR waiting for the owner:
# pending.
labels=$(pr_labels)
needs_human=no
if grep -qx needs-human <<< "$labels"; then
  needs_human=yes
  newest_state human-approved "$actions_bot"
  if ! grep -qx human-approved <<< "$labels"; then
    echo "pending  human-approved (label not on PR, status $newest)"; pending=1
  elif [ "$newest" = success ]; then
    echo "ok       human-approved"
  else
    echo "pending  human-approved ($newest)"; pending=1
  fi
fi

[ "$failed" = 0 ] || exit 1
[ "$pending" = 0 ] || exit 2

# Nothing in `gh pr merge` checks labels, so read them again just before it:
# the owner may have removed human-approved while the checks above ran. If
# needs-human came or went, stop too, so the gate is never decided on stale
# labels. Only the gap between this read and the merge call remains.
labels=$(pr_labels)
needs_human_now=no
grep -qx needs-human <<< "$labels" && needs_human_now=yes
if [ "$needs_human_now" != "$needs_human" ]; then
  echo "pending  needs-human label changed during checks"; exit 2
fi
if [ "$needs_human" = yes ] && ! grep -qx human-approved <<< "$labels"; then
  echo "pending  human-approved (label removed during checks)"; exit 2
fi

# --match-head-commit refuses the merge if someone pushed after the checks ran.
# The remote branch is deleted by the repo's delete-branch-on-merge setting.
gh pr merge "$pr" --repo "$repo" --squash --match-head-commit "$head"
