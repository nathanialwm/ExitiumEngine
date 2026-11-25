# ADR-0009: How an escalated PR is reviewed and merged

- Status: accepted
- Date: 2026-11-25
- Issue: #131

## Context

Under [ADR-0005](0005-review-escalation-only.md) and [ADR-0008](0008-three-review-rounds.md), a PR whose third AI review round fails gets the sticky `needs-human` label, and the owner decides. The escalation comment offers the owner three options: another fix round, owner edits, or merge as is with follow-up issues.

`merge-pr.sh` required `factory/ai-review` success on the head of every PR, escalated or not. Two of the three options leave the final head without that status: owner edits give a head with no review status, and merge as is keeps the failing one. So the gate could never merge them, even with the owner's `human-approved` on the head. PR #130 got stuck this way and needed an improvised "verification pass" to post the status.

The owner can also ask for more AI review after escalation, as on #171 and #175 (fix, then round 4). ADR-0008 keeps those rounds outside the three-round count but left their header and their effect on the merge to this decision. The ADR-0008 header would read `round 4 of 3`.

## Decision

**The owner's approval stands in for the AI review on an escalated PR.** On a PR labelled `needs-human`, `merge-pr.sh` does not require `factory/ai-review` success. It still requires `ci-pass`, `pr-title` and `human-gate`, plus the owner's approval in both parts it already checks: the `human-approved` label on the PR now, and the `human-approved` success status on the head. A push still makes a new head without that status, so it still revokes approval, and the labels are still re-read just before the merge, which still uses `--match-head-commit`.

**One exception: the owner must have seen the latest verdict.** If the newest `factory/ai-review` status on the head is not success and was posted at or after the newest approval, `merge-pr.sh` waits (exit 2). This happens when a post-escalation round fails after the owner approved the same head. The owner reads the verdict and then re-applies `human-approved` (remove the label and add it again), which posts a newer approval. A failing status posted before the approval does not block, because the owner approved having seen it.

**Post-escalation review rounds run only when the owner asks.** The orchestrator spawns the `reviewer` alone, scoped to the fixes the owner asked for and the diff since the escalated head. It still runs the definition-of-done checks, and it posts `factory/ai-review` as usual. The verdict goes to the owner. It never lifts `needs-human` and never starts a fix round by itself.

**Header for those rounds:** `Post-escalation AI review, round <N>, head <sha7>`. `<N>` is 1 plus the number of distinct earlier heads that got a verdict in either a counted round or a post-escalation round, so the first post-escalation round after round 3 is round 4. The header has no "of 3", because these rounds have no limit: the owner asks for each one. It does not start with `AI review`, so the ADR-0008 round rule never counts it, and no copy of that rule needs to change. The verdict line is the same as in a counted round.

## Alternatives considered

- Require a scoped verification pass that posts `factory/ai-review` success (the other option in #131, used ad hoc on #130): it makes an AI pass a precondition of the owner's decision. It cannot express "merge as is" without the reviewer approving a head whose findings are not fixed, and it costs a review after every owner edit.
- Skip `factory/ai-review` on escalated PRs with no exception: simpler, but a post-escalation round that fails after the owner approved the same head would not stop the merge.
- Continue the counted header (`round 4 of 3`), or give post-escalation rounds an `AI review` header the round rule must exclude: the first reads as an error, and the second would change the round rule in every copy (`review-pr`, `factory-run`, `test-reviewer`).

## Consequences

The owner can merge an escalated PR in any of the three offered ways, and nothing on that path needs an AI review to pass. The owner's approval now carries the review for escalated PRs, which is what ADR-0005 intended: after escalation, the owner decides.

`merge-pr.sh` reads the labels before the status checks and compares status timestamps. An approval given before a failing post-escalation verdict needs one more remove-and-re-add of `human-approved`. The orchestrator must not run `merge-pr.sh` on an escalated PR while a fix round the owner asked for is still in progress, because an approval on the old head would merge it; the `factory-run` skill says so.

Post-escalation rounds use the `reviewer` only, so `test-reviewer` and its `Test review, round <R> of 3` header take part only in counted rounds.
