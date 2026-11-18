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
- `status:approved` issues without a `role:*` label, without a `## Footprint`, or with a footprint step 3 judged vague: spawn `triage` once, with the list. For each vague footprint, name the vague entries in the prompt.

## 3. Pick work

A ready issue is one that:
- is `status:approved`, with a `role:*` label;
- has no open "Blocked by #N" dependencies;
- is not labelled `needs-spec`, or its spec is approved;
- has a concrete `## Footprint` section (see below). No footprint, or a vague one, means not ready: list it for `triage` in step 2 of the next pass.

Order ready issues by roadmap Project priority, then milestone, then age. Walk that list and start an issue only if it passes the conflict check against every in-progress issue, every running `docs-graph` pass, **and** every issue already picked in this pass. Stop when the slots run out.

**No starvation:** if the top-priority ready issue was skipped for a conflict on 3 passes in a row, start nothing new that conflicts with it until it has started. Passes share no memory, so keep the count on the issue: each pass that skips it, comment `starvation-skip: N` on it, where N is one more than in its latest `starvation-skip` comment (1 if none). If a later pass finds another ready issue ranked above it, the streak is broken: if its latest count is above 0, comment `starvation-skip: 0` on it.

### Footprint

Triage writes it into the issue body:

```
## Footprint
- Edits: crates/exitium_ecs/
- Governs: none
- Hotspots: none
- New crate edges: none
```

- `Edits` holds two kinds of entry: a crate directory (`crates/<name>/`, which covers every file in it, benches and tests included), or an exact file under `docs/specs/`. Any other path is a hotspot. The facade `crates/exitium/` looks like a crate directory but is a hotspot: list it under `Hotspots`, never `Edits`.
- `Governs` (spec issues only) names the crate directories the spec decides. A spec issue counts as editing those crates for every rule below, so a spec amendment never runs beside work on the crate it governs.
- `Hotspots` lists every path the change touches outside `Edits`, or `none`.
- `New crate edges` lists dependencies the change adds between workspace crates (`exitium_render -> exitium_assets`), or `none`.

**Hotspots** are every path that is not inside one crate directory and not a spec file: the root `Cargo.toml`, `Cargo.lock`, `crates/exitium/` (the facade, which depends on every crate), `xtask/`, `scripts/`, `ci/`, `.cargo/`, `.claude/`, `.github/`, `docs/decisions/` (ADR numbers collide), `docs/benchmarks.md`, every root file (`CLAUDE.md`, `PLAN.md`, `INTERVIEW.md`, `README.md`, `deny.toml`, `rustfmt.toml`, `rust-toolchain.toml` and so on), and any path added later outside `crates/`. Any change to a crate's `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` or features rewrites `Cargo.lock`, so it holds the `Cargo.lock` hotspot. A new crate adds a package to `Cargo.lock` and a member to the workspace the root `Cargo.toml` defines, so it holds both hotspots.

### Conflict check

Two issues may run at the same time only if **all** of these hold. If any answer is "probably" or "unsure", the answer is no.

1. **Disjoint edits:** no entry of one issue's `Edits` (plus `Governs`) equals or contains an entry of the other's. One crate has at most one issue in flight.
2. **No shared hotspot:** at most one in-flight item touches any hotspot at all.
3. **No crate dependency:** neither issue edits a crate that depends, directly or transitively, on a crate the other edits. Use the graph as it will be after both land: the current graph plus both issues' `New crate edges`. Read the current graph with `cargo tree -i <crate> --workspace --target all --all-features -e normal,build,dev --prefix none`, which lists every workspace crate that depends on `<crate>` on any target and feature set. This rules out semantic conflicts, where both PRs pass CI alone but break `main` together.
4. **No issue dependency:** neither is blocked by the other, and neither needs an API the other is adding.

Spec files that govern no crate (`Governs: none`) only need rules 1, 2 and 4.

An in-progress issue with no `## Footprint` counts as editing every crate and touching every hotspot until it has one, so nothing new starts beside it.

## 4. Start implementers

For each picked issue:
1. Relabel it from `status:approved` to `status:in-progress`.
2. Spawn the agent named in its `role:*` label, with `isolation: "worktree"`. The prompt: `Implement issue #N. Follow the implement-issue skill. Stay inside the issue's Footprint.` If the issue already has a branch (`git ls-remote --heads origin '<N>-*'`), left by a failed footprint extension in step 5, use the continue prompt from step 5 item 0 instead.

Spawn all of them in one message so they run in parallel. Each one returns a PR URL, head SHA and gate status.

## 5. Drive each open factory PR

0. **Footprint extension:** if an implementer, `test-writer` or `perf` stopped with `blocked: footprint <paths>`, rerun the conflict check for the issue with those paths added. If it passes, update the issue's Footprint and resume: SendMessage to the same agent if it is still alive, otherwise spawn the same role with `Continue issue #N on branch <branch>. Follow the implement-issue skill. Stay inside the issue's Footprint.` If it fails, update the Footprint anyway and relabel the issue `status:approved`, which frees its slot so two blocked issues can never deadlock. Leave the branch and any open PR. The rest of this step skips that PR until its issue is `status:in-progress` again, because an approved issue holds no footprint. Step 3 picks the issue up again like any other, and step 4 continues the existing branch.
1. **Tests:** spawn `test-writer` on the PR branch, unless the implementer already added sufficient tests.
2. **Review:** spawn `reviewer` and `test-reviewer` in parallel. Spawn `perf` too if hot paths or benchmarks changed.
3. **Changes requested:** send the findings to the implementer (SendMessage to the same agent, or spawn the same role on the branch). After **2 failed review rounds**, stop: add `needs-human`, comment a summary that @-mentions `@nathanialwm` with the options (another fix round, owner edits, merge and file follow-ups), and notify the owner. The label is sticky, so even after a clean extra round the owner must approve the final head. This is the only point where the factory waits on the owner.
4. **Approved:** the reviewer has already posted the `factory/ai-review` success status. Run `scripts/factory/merge-pr.sh <N>`. It merges only when `ci-pass`, `pr-title`, `human-gate` and `factory/ai-review` all pass on the head commit. Exit 2 means checks are still running: move on and retry on the next pass. Exit 1 means a check failed: treat it like changes requested. Never merge any other way; without GitHub Pro there is no branch protection, so this script is the gate.
5. **Merged:** spawn `docs-graph` for merges that changed structure or behavior. A `docs-graph` pass is an in-flight item: its footprint is the crate directories the merged PR edited, plus any hotspot docs it must update. Spawn it, with `isolation: "worktree"`, only when that footprint passes the conflict check (otherwise on a later pass), and give it the footprint in its prompt. It opens its own PR (see the `docs-graph` agent), which this step drives like any factory PR. The pass stays in flight until that PR merges, or until it returns `no changes`.

## 6. Report and notify

- Send a push notification (the `PushNotification` tool) to the owner when a PR is escalated with `needs-human`, CI on `main` is broken, an agent is stuck, or the backlog is empty. GitHub notifications are covered separately: the `human-gate` check @-mentions the owner on `needs-human` PRs, and any comment asking the owner for something must @-mention `@nathanialwm`.
- End with a short table of issue, PR, state and next action.
