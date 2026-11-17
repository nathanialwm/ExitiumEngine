# Graph Report - exitium  (2026-11-16)

## Corpus Check
- 110 files · ~15,729 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 8 file(s) not represented in the graph (top: .toml 4, (none) 4)

## Summary
- 395 nodes · 349 edges · 107 communities (21 shown, 86 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 3 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `4eaa12aa`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- bench.rs
- ci.rs
- jobs
- Exitium Engine — Interview Results
- Step-by-step plan
- volumes
- Exitium Engine — Agent Guide
- Spec: <feature>
- Factory run
- ADR-NNNN: <title>
- ADR-0001: Architecture overview
- ADR-0002: License
- PROMPT.md
- gh-app-token.sh
- file-issue/SKILL.md
- implement-issue/SKILL.md
- review-pr/SKILL.md
- write-spec/SKILL.md
- README.md
- exitium
- exitium_app
- exitium_assets
- exitium_audio
- exitium_backend
- exitium_cli
- exitium_core
- exitium_data
- exitium_destruct
- exitium_ecs
- exitium_editor
- exitium_mcp
- exitium_net
- exitium_physics
- exitium_platform
- exitium_profiler
- exitium_reflect
- exitium_reflect_derive
- exitium_render
- exitium_save
- exitium_scene
- exitium_script
- exitium_serial
- exitium_server
- exitium_sim
- exitium_steam
- exitium_tasks
- exitium_tilemap
- exitium_ui
- xtask
- pr-gates.yml
- ci.yml
- bug.yml
- epic.yml
- task.yml
- env
- usage-gate.py
- CI runners
- pull_request_template.md
- merge-pr.sh
- entrypoint.sh

## God Nodes (most connected - your core abstractions)
1. `Step-by-step plan` - 11 edges
2. `Exitium Engine — Agent Guide` - 10 edges
3. `jobs` - 9 edges
4. `run` - 8 edges
5. `Spec: <feature>` - 8 edges
6. `uses` - 7 edges
7. `volumes` - 7 edges
8. `bench_ci()` - 7 edges
9. `step()` - 7 edges
10. `main()` - 7 edges

## Surprising Connections (you probably didn't know these)
- `main()` --calls--> `bench_ci()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/bench.rs
- `main()` --calls--> `compare()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/bench.rs
- `main()` --calls--> `push()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/git.rs
- `main()` --calls--> `install_hooks()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/git.rs
- `main()` --calls--> `step()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/ci.rs

## Import Cycles
- None detected.

## Communities (107 total, 86 thin omitted)

### Community 0 - "bench.rs"
Cohesion: 0.20
Nodes (15): bench_ci(), cargo_bench(), Change, collect_changes(), compare(), DEFAULT_THRESHOLD_PERCENT, has_bench_targets(), number_after() (+7 more)

### Community 1 - "ci.rs"
Cohesion: 0.13
Nodes (16): clippy(), dir_contains(), fmt(), miri(), NATIVE_ONLY, step(), test(), unsafe_crates() (+8 more)

### Community 2 - "jobs"
Cohesion: 0.07
Nodes (38): if, runs-on, steps, outputs, runs-on, steps, if, needs (+30 more)

### Community 3 - "Exitium Engine — Interview Results"
Cohesion: 0.18
Nodes (10): 1. Project summary (my understanding), 2. Answers — Round 1 (engine), 3. Answers — Round 2 (AI software factory and follow-ups), 4. Tensions, risks and open questions, 5. Design principles derived from the interview, A. Factory operating model, Agent roster, B. CI and infrastructure (+2 more)

### Community 4 - "Step-by-step plan"
Cohesion: 0.09
Nodes (21): AI software factory design, Cross-cutting rules, Exitium Engine — Plan, Flow, Human gates, enforced by the `human-gate` CI check, Immediate next steps, Overview, Phase 0 — Repository and factory bootstrap (manual, with owner) (+13 more)

### Community 5 - "volumes"
Cohesion: 0.08
Nodes (29): REPO_URL, RUNNER_LABELS, RUNNER_NAME, RUNNER_TOKEN, build, environment, image, restart (+21 more)

### Community 6 - "Exitium Engine — Agent Guide"
Cohesion: 0.18
Nodes (10): Code rules, Definition of done, Design rules (non-negotiable), Exitium Engine — Agent Guide, Factory operating rules, GitHub identity, graphify, Human gates (+2 more)

### Community 7 - "Spec: <feature>"
Cohesion: 0.22
Nodes (8): Design, Goal, MCP / editor surface, Non-goals, Open questions, Performance, Spec: <feature>, Test plan

### Community 8 - "Factory run"
Cohesion: 0.25
Nodes (7): 1. Gates before starting new work, 2. Intake, 3. Pick work, 4. Start implementers, 5. Drive each open factory PR, 6. Report and notify, Factory run

### Community 9 - "ADR-NNNN: <title>"
Cohesion: 0.33
Nodes (5): ADR-NNNN: <title>, Alternatives considered, Consequences, Context, Decision

### Community 10 - "ADR-0001: Architecture overview"
Cohesion: 0.40
Nodes (4): ADR-0001: Architecture overview, Consequences, Context, Decision

### Community 11 - "ADR-0002: License"
Cohesion: 0.40
Nodes (4): ADR-0002: License, Consequences, Context, Decision

### Community 12 - "PROMPT.md"
Cohesion: 0.50
Nodes (3): Features, The Codebase, The project

### Community 94 - "pr-gates.yml"
Cohesion: 0.11
Nodes (18): concurrency, cancel-in-progress, group, env, GH_TOKEN, HEAD_SHA, OWNER_LOGIN, PR (+10 more)

### Community 95 - "ci.yml"
Cohesion: 0.13
Nodes (15): concurrency, concurrency, cancel-in-progress, group, env, CARGO_INCREMENTAL, CARGO_TERM_COLOR, name (+7 more)

### Community 96 - "bug.yml"
Cohesion: 0.14
Nodes (13): description, label, value, body, attributes, id, type, validations (+5 more)

### Community 97 - "epic.yml"
Cohesion: 0.14
Nodes (13): description, label, value, body, attributes, id, type, validations (+5 more)

### Community 98 - "task.yml"
Cohesion: 0.14
Nodes (13): description, label, value, body, attributes, id, type, validations (+5 more)

### Community 99 - "env"
Cohesion: 0.22
Nodes (11): ACTION, TITLE, runs-on, steps, jobs, human-gate, pr-title, runs-on (+3 more)

### Community 101 - "CI runners"
Cohesion: 0.40
Nodes (4): CI runners, Linux runners, Toolchain bumps, Windows runner

### Community 102 - "pull_request_template.md"
Cohesion: 0.40
Nodes (4): Changes, Gates, Summary, Verification

## Knowledge Gaps
- **31 isolated node(s):** `GH_TOKEN`, `exitium`, `exitium_app`, `exitium_assets`, `exitium_audio` (+26 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 268 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **86 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `jobs` connect `jobs` to `ci.yml`?**
  _High betweenness centrality (0.011) - this node is a cross-community bridge._
- **What connects `GH_TOKEN`, `exitium`, `exitium_app` to the rest of the system?**
  _31 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `ci.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.12648221343873517 - nodes in this community are weakly interconnected._
- **Why does `bench` connect `jobs` to `ci.yml`?**
  _High betweenness centrality (0.003) - this node is a cross-community bridge._
- **Should `jobs` be split into smaller, more focused modules?**
  _Cohesion score 0.07254623044096728 - nodes in this community are weakly interconnected._
- **Should `Step-by-step plan` be split into smaller, more focused modules?**
  _Cohesion score 0.09090909090909091 - nodes in this community are weakly interconnected._
- **Should `volumes` be split into smaller, more focused modules?**
  _Cohesion score 0.08275862068965517 - nodes in this community are weakly interconnected._