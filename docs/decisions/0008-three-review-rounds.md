# ADR-0008: Three AI review rounds before escalation

- Status: accepted
- Supersedes: ADR-0005 (round count)
- Date: 2026-11-25
- Issue: #176

## Context

[ADR-0005](0005-review-escalation-only.md) made review escalation the only human gate: a PR that fails two AI review rounds gets `needs-human`, and the owner decides. In Stage 0 every spec PR failed two rounds. Most of those second-round findings were real but fixable, so the owner was pulled in for work a third round would have finished.

## Decision

The owner decided on 2026-11-25 that every PR gets **three** AI review rounds. The orchestrator escalates with `needs-human` when round 3 fails. This holds for every PR type: specs, ADRs, code, factory changes and `docs-graph` PRs.

A round is one review pass on one head commit: the `reviewer`, the `test-reviewer`, and `perf` when it runs. Every round that does not end in a merge has failed: one of them requested changes, or the round approved and a gate check then failed on the same head. Each review comment starts with a header that names the round and the head, `round N of 3, head <sha7>`. N is 1 plus the number of distinct earlier heads that got an `AI review` verdict, so a parallel or re-run review of the same head never adds a round. The `review-pr` skill states the exact counting rule.

Only the round count changes. The rest of ADR-0005 stands: `needs-human` is sticky, only the owner's `human-approved` label lifts the gate, and the former gate areas get extra reviewer scrutiny.

PRs already escalated under ADR-0005 stay `needs-human`. Rounds the owner asks for after an escalation, such as round 4 on #171 and #175, run outside the count and never lift the sticky label. [ADR-0009](0009-merging-escalated-prs.md) decides how those post-escalation reviews run, their header, and how an escalated PR merges.

## Alternatives considered

- Keep two rounds: the owner becomes the bottleneck again, as in Stage 0.
- Three rounds for specs only: one rule for every PR type is simpler for the orchestrator to apply and for reviewers to check.
- No limit: a PR that agents cannot get right would loop forever and spend tokens.

## Consequences

Fewer PRs wait on the owner. A PR that agents cannot fix costs one more review and fix cycle before it reaches the owner. `pr-gates.yml` still says "two" in its `human-gate` reason text; the App cannot push workflow files, so the owner changes that wording in #178. The gate logic itself does not count rounds and needs no change.
