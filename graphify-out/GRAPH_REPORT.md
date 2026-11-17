# Graph Report - exitium  (2026-11-16)

## Corpus Check
- 95 files · ~12,697 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 7 file(s) not represented in the graph (top: .toml 4, (none) 3)

## Summary
- 211 nodes · 146 edges · 94 communities (13 shown, 81 thin omitted)
- Extraction: 98% EXTRACTED · 2% INFERRED · 0% AMBIGUOUS · INFERRED: 3 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `1c795920`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- bench.rs
- ci.rs
- main
- Exitium Engine — Interview Results
- Exitium Engine — Plan
- Step-by-step plan
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

## God Nodes (most connected - your core abstractions)
1. `Step-by-step plan` - 11 edges
2. `Exitium Engine — Agent Guide` - 9 edges
3. `Spec: <feature>` - 8 edges
4. `run()` - 7 edges
5. `main()` - 7 edges
6. `Factory run` - 7 edges
7. `bench_ci()` - 6 edges
8. `Exitium Engine — Interview Results` - 6 edges
9. `Exitium Engine — Plan` - 6 edges
10. `compare()` - 5 edges

## Surprising Connections (you probably didn't know these)
- `main()` --calls--> `bench_ci()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/bench.rs
- `main()` --calls--> `compare()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/bench.rs
- `main()` --calls--> `run()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/ci.rs
- `main()` --calls--> `push()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/git.rs
- `main()` --calls--> `install_hooks()`  [EXTRACTED]
  xtask/src/main.rs → xtask/src/git.rs

## Import Cycles
- None detected.

## Communities (94 total, 81 thin omitted)

### Community 0 - "bench.rs"
Cohesion: 0.21
Nodes (14): bench_ci(), cargo_bench(), Change, collect_changes(), compare(), DEFAULT_THRESHOLD_PERCENT, number_after(), parse_mean_change() (+6 more)

### Community 1 - "ci.rs"
Cohesion: 0.24
Nodes (9): clippy(), dir_contains(), fmt(), miri(), NATIVE_ONLY, run(), test(), unsafe_crates() (+1 more)

### Community 2 - "main"
Cohesion: 0.22
Nodes (6): install_hooks(), MAX_ATTEMPTS, push(), RETRY_MARKER, main(), USAGE

### Community 3 - "Exitium Engine — Interview Results"
Cohesion: 0.18
Nodes (10): 1. Project summary (my understanding), 2. Answers — Round 1 (engine), 3. Answers — Round 2 (AI software factory and follow-ups), 4. Tensions, risks and open questions, 5. Design principles derived from the interview, A. Factory operating model, Agent roster, B. CI and infrastructure (+2 more)

### Community 4 - "Exitium Engine — Plan"
Cohesion: 0.18
Nodes (10): AI software factory design, Cross-cutting rules, Exitium Engine — Plan, Flow, Human gates, enforced by the `human-gate` CI check, Immediate next steps, Overview, Repo artifacts (+2 more)

### Community 5 - "Step-by-step plan"
Cohesion: 0.18
Nodes (11): Phase 0 — Repository and factory bootstrap (manual, with owner), Phase 1 — Foundations, Phase 2 — Runtime, Phase 3 — Game UI (Glutton critical path), Phase 4 — Editor and MCP, Phase 5 — Incremental sim and saves, Phase 6 — Platform, online and backend, Phase 7 — Glutton (separate repo) (+3 more)

### Community 6 - "Exitium Engine — Agent Guide"
Cohesion: 0.20
Nodes (9): Code rules, Definition of done, Design rules (non-negotiable), Exitium Engine — Agent Guide, Factory operating rules, GitHub identity, Human gates, Orient cheaply (+1 more)

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

## Knowledge Gaps
- **30 isolated node(s):** `exitium`, `exitium_app`, `exitium_assets`, `exitium_audio`, `exitium_backend` (+25 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 163 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **81 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `main()` connect `main` to `bench.rs`, `ci.rs`?**
  _High betweenness centrality (0.012) - this node is a cross-community bridge._
- **What connects `exitium`, `exitium_app`, `exitium_assets` to the rest of the system?**
  _30 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Why does `Step-by-step plan` connect `Step-by-step plan` to `Exitium Engine — Plan`?**
  _High betweenness centrality (0.007) - this node is a cross-community bridge._
- **Why does `Exitium Engine — Plan` connect `Exitium Engine — Plan` to `Step-by-step plan`?**
  _High betweenness centrality (0.007) - this node is a cross-community bridge._