---
name: factory-run
description: Run one pass of the Exitium AI software factory loop - approve and triage issues, start up to 3 implementers in worktrees, drive PRs through test, review and auto-merge. Use when the owner says "run the factory", "/factory-run", or asks to work the backlog.
---

# Factory run

You are the orchestrator. You coordinate subagents; you do not implement. All `gh` API calls go through the App token (CLAUDE.md "GitHub identity"):

```bash
T() { GH_TOKEN="$(scripts/factory/gh-app-token.sh)" gh "$@"; }
```

## 1. Gates before starting new work

- **Usage gate:** run `python -I scripts/factory/usage-gate.py`. Exit 0: continue. Exit 1 (less than 5% left): do not start new issues; skip to step 5 and only advance work already in flight. Exit 2 (unknown): ask the owner once whether to start new issues.
- **Concurrency:** count the in-progress issues with `T issue list --label status:in-progress --json number --jq length`. The number of new slots is `3 - count`.

## 2. Intake

- Issues labelled `status:needs-approval`: spawn `issue-approver` once, with the list.
- `status:approved` issues without a `role:*` label: spawn `triage` once, with the list.

## 3. Pick work

A ready issue is one that:
- is `status:approved`, with a `role:*` label;
- has no open "Blocked by #N" dependencies;
- is not labelled `needs-spec`, or its spec is approved.

Order ready issues by roadmap Project priority, then milestone, then age. Take as many as there are free slots.

## 4. Start implementers

For each picked issue:
1. Relabel it from `status:approved` to `status:in-progress`.
2. Spawn the agent named in its `role:*` label, with `isolation: "worktree"`. The prompt: `Implement issue #N. Follow the implement-issue skill.`

Spawn all of them in one message so they run in parallel. Each one returns a PR URL, head SHA and gate status.

## 5. Drive each open factory PR

1. **Tests:** spawn `test-writer` on the PR branch, unless the implementer already added sufficient tests.
2. **Review:** spawn `reviewer` and `test-reviewer` in parallel. Spawn `perf` too if hot paths or benchmarks changed.
3. **Changes requested:** send the findings to the implementer (SendMessage to the same agent, or spawn the same role on the branch). After **2 failed review rounds**, stop: add `needs-human`, comment a summary that @-mentions `@nathanialwm` with the options (another fix round, owner edits, merge and file follow-ups), and notify the owner. This is the only point where the factory waits on the owner.
4. **Approved:** the reviewer has already posted the `factory/ai-review` success status. Run `scripts/factory/merge-pr.sh <N>`. It merges only when `ci-pass`, `pr-title`, `human-gate` and `factory/ai-review` all pass on the head commit. Exit 2 means checks are still running: move on and retry on the next pass. Exit 1 means a check failed: treat it like changes requested. Never merge any other way; without GitHub Pro there is no branch protection, so this script is the gate.
5. **Merged:** spawn `docs-graph` for merges that changed structure or behavior.

## 6. Report and notify

- Send a push notification (the `PushNotification` tool) to the owner when a PR is escalated with `needs-human`, CI on `main` is broken, an agent is stuck, or the backlog is empty. GitHub notifications are covered separately: the `human-gate` check @-mentions the owner on `needs-human` PRs, and any comment asking the owner for something must @-mention `@nathanialwm`.
- End with a short table of issue, PR, state and next action.
