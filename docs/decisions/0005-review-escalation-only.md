# ADR-0005: The owner is involved only when AI review fails twice

- Status: accepted; round count superseded by ADR-0008 (three rounds)
- Date: 2026-11-17

## Context

INTERVIEW.md A3 made several areas permanent human gates: new dependencies, CI and factory config, the facade API, save formats, licensing, secrets and releases. `pr-gates.yml` enforced them by path, by dependency and `unsafe` detection, and by a sticky `gate:human` label. In practice almost every early PR was gated: every spec, the job system, and every factory change. The owner became the bottleneck, and the factory's own two-round review limit already catches PRs that agents cannot get right.

## Decision

The only human gate is review escalation. When a PR fails two AI review rounds, the orchestrator labels it `needs-human`, comments a summary for the owner, and stops. `human-gate` requires the owner's `human-approved` only on PRs that have ever carried `needs-human` (the label stays sticky). Every other PR merges through `merge-pr.sh` once CI and `factory/ai-review` pass.

The former gate areas get extra reviewer scrutiny instead (`review-pr` step 3.6), and new dependencies must be justified in the PR body. Old `gate:human` labels no longer have any effect.

## Alternatives considered

- Keep a short list of hard gates (secrets, releases, licensing) — the owner chose a single escalation rule.
- Relax gates per area over time — slower, and it kept specs on the owner's critical path.

## Consequences

Throughput no longer waits on the owner except for escalations. The cost is that high-risk changes rely on the AI reviewer alone. Two safeguards remain outside the factory's control: `pr-gates.yml` runs from `main` (`pull_request_target`), so a PR cannot weaken its own gate, and the factory App lacks the Workflows permission, so changes to `.github/workflows/` still need the owner to push them.
