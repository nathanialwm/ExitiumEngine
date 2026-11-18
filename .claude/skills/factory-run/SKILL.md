---
name: factory-run
description: Run one pass of the Exitium AI software factory loop - approve and triage issues, start up to 8 conflict-free implementers in worktrees, drive PRs through test, review and auto-merge. Use when the owner says "run the factory", "/factory-run", or asks to work the backlog.
---

# Factory run

You are the orchestrator. You coordinate subagents; you do not implement. All `gh` API calls go through the App token (CLAUDE.md "GitHub identity"):

```bash
T() { GH_TOKEN="$(scripts/factory/gh-app-token.sh)" gh "$@"; }
```

## 1. Gates before starting new work

- **Usage gate:** run `python -I scripts/factory/usage-gate.py`. Exit 0: continue. Exit 1 (less than 5% left): do not start new issues; skip to step 5 and only advance work already in flight. Exit 2 (unknown): ask the owner once whether to start new issues.
- **Concurrency:** count the in-progress issues with `T issue list --label status:in-progress --json number --jq length`. The number of new slots is `8 - count`. Free slots are a ceiling, not a target: the conflict check in step 3 decides what actually starts.

## 2. Intake

- Issues labelled `status:needs-approval`: spawn `issue-approver` once, with the list.
- `status:approved` issues without a `role:*` label: spawn `triage` once, with the list.

## 3. Pick work

A ready issue is one that:
- is `status:approved`, with a `role:*` label;
- has no open "Blocked by #N" dependencies;
- is not labelled `needs-spec`, or its spec is approved;
- has a concrete `## Footprint` section (see below). No footprint, or a vague one, means not ready: send it back to `triage`.

Order ready issues by roadmap Project priority, then milestone, then age. Walk that list and start an issue only if it passes the conflict check against every in-progress issue **and** every issue already picked in this pass. Stop when the slots run out.

### Footprint

Triage writes it into the issue body:

```
## Footprint
- Edits: crates/exitium_ecs/
- Hotspots: none
```

`Edits` lists the crate directories, or for non-crate work the exact files, that the change creates or modifies. `Hotspots` names any hotspot it touches, or `none`.

**Hotspots** are files that almost any change can collide on: the root `Cargo.toml`, `Cargo.lock` (so adding or bumping any dependency), `crates/exitium/` (the facade), `xtask/`, `scripts/`, `.claude/`, `.github/`, `CLAUDE.md`, `PLAN.md`, `deny.toml`, `rust-toolchain.toml`, and the next ADR number in `docs/decisions/`.

### Conflict check

Two issues may run at the same time only if **all** of these hold. If any answer is "probably" or "unsure", the answer is no.

1. **Disjoint edits:** no crate directory or file appears in both `Edits` lists. One crate has at most one issue in flight.
2. **No shared hotspot:** at most one in-flight issue touches any hotspot at all.
3. **No crate dependency:** neither issue edits a crate that depends, directly or transitively (normal, build or dev), on a crate the other edits. Check with `cargo tree -i <crate> --workspace -e normal,build,dev --prefix none`, which lists every workspace crate that depends on `<crate>`. This rules out semantic conflicts, where both PRs pass CI alone but break `main` together.
4. **No issue dependency:** neither is blocked by the other, and neither needs an API the other is adding.

Docs-only issues (specs, crate docs outside a code crate) only need rules 1, 2 and 4.

## 4. Start implementers

For each picked issue:
1. Relabel it from `status:approved` to `status:in-progress`.
2. Spawn the agent named in its `role:*` label, with `isolation: "worktree"`. The prompt: `Implement issue #N. Follow the implement-issue skill. Stay inside the issue's Footprint.`

Spawn all of them in one message so they run in parallel. Each one returns a PR URL, head SHA and gate status.

## 5. Drive each open factory PR

0. **Footprint extension:** if an implementer stopped with `blocked: footprint <paths>`, rerun the conflict check with those paths added. If it passes, add them to the issue's `Edits` and resume the implementer. If not, leave it in progress and retry on the next pass.
1. **Tests:** spawn `test-writer` on the PR branch, unless the implementer already added sufficient tests.
2. **Review:** spawn `reviewer` and `test-reviewer` in parallel. Spawn `perf` too if hot paths or benchmarks changed.
3. **Changes requested:** send the findings to the implementer (SendMessage to the same agent, or spawn the same role on the branch). After **2 failed review rounds**, stop: add `needs-human`, comment a summary that @-mentions `@nathanialwm` with the options (another fix round, owner edits, merge and file follow-ups), and notify the owner. The label is sticky, so even after a clean extra round the owner must approve the final head. This is the only point where the factory waits on the owner.
4. **Approved:** the reviewer has already posted the `factory/ai-review` success status. Run `scripts/factory/merge-pr.sh <N>`. It merges only when `ci-pass`, `pr-title`, `human-gate` and `factory/ai-review` all pass on the head commit. Exit 2 means checks are still running: move on and retry on the next pass. Exit 1 means a check failed: treat it like changes requested. Never merge any other way; without GitHub Pro there is no branch protection, so this script is the gate.
5. **Merged:** spawn `docs-graph` for merges that changed structure or behavior.

## 6. Report and notify

- Send a push notification (the `PushNotification` tool) to the owner when a PR is escalated with `needs-human`, CI on `main` is broken, an agent is stuck, or the backlog is empty. GitHub notifications are covered separately: the `human-gate` check @-mentions the owner on `needs-human` PRs, and any comment asking the owner for something must @-mention `@nathanialwm`.
- End with a short table of issue, PR, state and next action.
