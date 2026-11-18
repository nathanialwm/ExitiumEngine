# Exitium Engine — Plan

Companion to `INTERVIEW.md`, which holds the answers and risks this plan is built on.

## Overview

Build order:

1. **Stand up the AI software factory first.** This means the repo, CI, agent roster, gates and the graph.
2. **Have the factory build the engine in dependency order**, prioritizing what **Glutton** needs: foundations → runtime → UI → editor/MCP → incremental sim and saves → platform and backend.
3. **Ship the Glutton Steam demo and itch.io build together.**
4. **Broaden the engine:** pixel art, camera modes, tilemaps, physics, multiplayer scale-out, destruction groundwork.

Phase 0 is done by hand: me, in this session or the next few, with the owner. From Phase 1 onward, the Architect agent turns each phase into epics and issues on the GitHub Project, and the factory executes them.

---

## Target architecture

### Workspace layout (Cargo workspace, crates prefixed `exitium_`)

| Layer | Crate | Purpose |
|---|---|---|
| Foundation | `exitium_core` | Math (glam), `Num` (f64 newtype with incremental formatting), IDs/handles, time, errors |
| | `exitium_tasks` | Work-stealing job system, frame/tick scheduler, thread budgets (4-thread min-spec mode, single-thread web fallback) |
| | `exitium_ecs` | Custom archetype SoA ECS, parallel system scheduler with access-conflict detection, change detection, events, hierarchy (Parent/Children) |
| | `exitium_reflect` (+ `_derive`) | Runtime type info. One derive powers inspector, serialization, Luau, MCP schemas and migrations |
| | `exitium_serial` | Text format (human-editable, diff-friendly) ↔ custom binary format (schema-hashed, aligned, zero-copy bulk arrays, zstd/LZ4) |
| | `exitium_profiler` | `tracing` spans → Tracy and the in-editor profiler; memory and frame stats |
| Runtime | `exitium_app` | App/plugin model, main loop, fixed and variable tick, headless mode |
| | `exitium_platform` | Window/input (winit), filesystem, platform traits (desktop/web now, mobile later) |
| | `exitium_assets` | Asset database, stable GUIDs, import pipeline, hot reload (assets, shaders, data), packed builds |
| | `exitium_data` | **Data Assets** (scriptable objects): typed definitions, prototype → variant inheritance, ID references |
| | `exitium_scene` | Scenes, prefabs, prefab variants, hierarchy serialization |
| | `exitium_render` | wgpu renderer: sprite batching, high-res image path (portraits), text (cosmic-text/glyphon), render graph, camera modes, later the pixel pipeline and 2D lighting |
| | `exitium_ui` | Retained widget tree, flex layout (taffy), themes/styles, data binding, UI markup files, animation/tweening; immediate-mode layer for debug and tools |
| | `exitium_audio` | kira-based audio, buses, streaming |
| | `exitium_script` | Luau via mlua. Layered API (safe tier vs. privileged tier, for future modding), hot reload |
| Game systems | `exitium_sim` | Incremental sim toolkit: fixed high-rate ticks, offline progress (analytic + fast-forward), prestige/reset layers, automation and scheduling |
| | `exitium_save` | Versioned saves, migration chains, compression, authenticated encryption, cloud provider trait |
| | `exitium_physics` | rapier2d integration (later phase) |
| | `exitium_tilemap` | Tilemaps for top-down, sidescroller and isometric (depth sort, height levels) — later phase |
| | `exitium_destruct` | Groundwork: chunked grids, cell-sim interface, GPU compute hooks, dirty regions, collider regeneration interface — later phase |
| Online | `exitium_net` | Transport, authoritative replication, client prediction, compact wire format |
| | `exitium_server` | Headless server runtime, stateless and horizontally scalable |
| | `exitium_backend` | Cloud saves, leaderboards, offline-gain validation service, auth; adapters for external hosting providers |
| | `exitium_steam` | Steamworks: achievements, cloud, overlay, demo/app IDs |
| Tooling | `exitium_editor` | Standalone egui editor: hierarchy, inspector, asset browser, UI editor, data asset editor, play-in-editor, profiler panel |
| | `exitium_mcp` | MCP server (rmcp): docs/API query, live editor/game control, screenshots, builds/tests/benchmarks |
| | `exitium_cli` | `exitium new/build/run/pack/bench` |
| | `exitium` | Facade crate that games depend on |
| | `xtask` | Repo automation: CI entry points, push wrapper, bench compare, schema export |

### Cross-cutting rules

- **Command bus:** every mutation from the editor, MCP or Lua is a typed, serializable command. Undo/redo, audit logs and agent parity all come from this one mechanism.
- **Reflection first:** a type the editor can't see through reflection doesn't exist to agents. MCP tool schemas are generated from reflection, not written by hand.
- **Performance gates:** criterion/divan benchmarks per hot system; `xtask bench-compare` fails CI on a regression over 5%; a min-spec profile runs with 4 threads.
- **`unsafe` policy:** only in designated modules, every block gets a `// SAFETY:` comment, Miri tests run in CI, and changes to these modules are flagged for owner review.

---

## AI software factory design

### Flow

```
Issue (owner / agent-filed)
  → [agent-filed? Issue Approver (Sonnet 5.5, medium) approves]
  → Triage: labels, size, specialization, Project placement
  → [systems-level? Architect writes spec/ADR → AI review → merge]
  → Orchestrator picks it up
      (≤8 concurrent, provably conflict-free footprints, only if >5% session usage remains)
  → Implementer (Opus 5.5 xhigh, specialized) in its own worktree/branch
  → Test Writer adds tests → Test Reviewer checks for redundancy and gaps
  → Perf Agent runs benches when hot paths changed
  → PR opened (Conventional Commit title, linked issue)
  → CI (all jobs required) + Reviewer (Sonnet 5.5 high) posts `factory/ai-review` status
  → human-gate check: only PRs escalated with `needs-human` (two failed review rounds) need the owner's `human-approved` (ADR-0005)
  → auto-merge (squash)
  → Docs & Graph Maintainer refreshes docs, MCP docs and the graph
  → notifications (GitHub + Claude push)
```

The Token Optimizer runs on a schedule, e.g. after every N merged PRs or weekly. It reads transcripts and outcomes, then either:
- opens PRs against `.claude/` rules, skills and prompts, or
- for large or unsafe changes, posts a suggestion in chat instead.

### Repo artifacts

- `CLAUDE.md` at the root: mission, design rules, definition of done, escalation rule, graphify usage. Each crate also gets its own `CLAUDE.md`/`AGENTS.md` with local invariants.
- `.claude/agents/*.md`: one file per role, with model and effort set in frontmatter.
- `.claude/skills/`:
  - `factory-run`: the orchestrator loop (pick issue, check usage, cap 3, spawn worktree)
  - `implement-issue`
  - `review-pr`
  - `write-spec`
  - `bench-compare`
  - `file-issue`
- `.claude/settings.json`: permissions and hooks (graph refresh, usage check).
- `.github/ISSUE_TEMPLATE/`: feature, bug, perf, spec/RFC, agent-filed.
- `.github/workflows/`: `ci.yml`, `human-gate.yml`, `bench.yml`, `macos.yml`, `release.yml`.
- `docs/decisions/` (ADRs), `docs/specs/`.
- `graphify-out/`, built locally by git hooks and gitignored (ADR-0004 supersedes the committed graph).
- Labels:
  - Status: `status:needs-approval`, `status:approved`, `status:in-progress`
  - Area: `area:*`
  - Role: `role:*`
  - Escalation: `needs-human`, `human-approved`
  - Size: `size:*`

### Human gates, enforced by the `human-gate` CI check

*Superseded by ADR-0005: the only human gate is a PR that fails two AI review rounds (`needs-human`). The areas below now get extra reviewer scrutiny instead.*

The check fails unless the owner has applied `human-approved` when a PR touches any of:

| Gate | What triggers it |
|---|---|
| New dependencies | `Cargo.toml`/`Cargo.lock` adds a dependency |
| CI/factory config | `.github/`, `.claude/`, `CLAUDE.md`, `AGENTS.md`, `xtask` |
| Save/serialization formats | `exitium_serial`, `exitium_save` format modules |
| Licensing | `LICENSE*`, `deny.toml` |
| Public API | `cargo-semver-checks` / public-API diff reports changes on the facade crate |
| Secrets | workflows or files that reference secrets |
| Steam releases | release workflows |

---

## Step-by-step plan

### Phase 0 — Repository and factory bootstrap (manual, with owner)

1. ~~Resolve R1–R3~~. Decided 2026-11-16 (INTERVIEW.md §4): native Windows runner + Linux Docker runners; macOS only on path-matched PRs; GitHub App `exitium-factory` for agent identity.
2. Update stable Rust. Add `rust-toolchain.toml` (pinned stable, plus the `wasm32` target and `clippy`, `rustfmt` and `miri` components where available).
3. `git init`, create the private repo `nathanialwm/ExitiumEngine` with `gh`, and push the initial commit containing PROMPT, INTERVIEW and PLAN.
4. Create the Cargo workspace skeleton: empty crates from the layout above, the facade crate, `xtask`, shared lints (`[workspace.lints]`), and `deny.toml`.
5. Pick the license structure (proprietary now, structured so it can become MIT/Apache-2.0 later) and record it in an ADR.
6. Write the root `CLAUDE.md`, the ADR template and ADR-0001 (architecture overview), and the spec template.
7. Create `.claude/agents/` for every role in the roster (INTERVIEW §3) and `.claude/skills/` for the factory skills.
8. Build the self-hosted CI:
   - A Docker image for the Linux runner (Rust, wasm tools, cargo-xwin, lavapipe for headless wgpu tests). Register it as a GitHub Actions runner and run it with docker compose on this PC.
   - The native Windows host runner (R1).
   - The GitHub-hosted macOS job: it always runs, but only builds and tests when platform, render or windowing paths changed, and otherwise reports success. No nightly run (R2).
   - The GitHub App `exitium-factory`, created by the owner (R3). Agents authenticate as the App, and `human-gate` verifies that `human-approved` was applied by `nathanialwm`.
9. Write `ci.yml`: fmt, clippy `-D warnings`, tests (nextest), WASM build, cargo-deny, Miri on the `unsafe` modules, `bench-compare` (5% gate), and the platform build matrix. Add `human-gate.yml` and the `factory/ai-review` required status.
10. Configure branch protection on `main`: PR-only, all checks required, squash merge, auto-merge enabled, Conventional Commit title check.
11. Set up graphify: local graph build from git hooks (ADR-0004, superseding R9) and `graphify claude install` for the CLAUDE.md integration.
12. Set up GitHub Projects (the roadmap board, with phases as milestones), labels and issue templates.
13. Implement and verify the usage-gate mechanism (R4) and notifications (GitHub + Claude push).
14. Dry run: take one trivial issue through the whole pipeline end to end, then fix whatever breaks.
15. The Architect agent turns Phases 1–9 into epics and issues on the board, and the owner approves the roadmap.

**Exit:** an issue labelled `status:approved` goes from pickup to auto-merge with no manual steps.

### Phase 1 — Foundations

1. `exitium_core`: math types, `Num` with formatting (scientific, engineering, suffix, letter notations), handles/IDs.
2. `exitium_tasks`: job system, scoped parallel-for, thread budget config, web single-thread fallback.
3. `exitium_ecs`: archetype storage, queries, commands, parallel scheduler, change detection, events, hierarchy. Benchmarks against baselines (iteration, add/remove churn, 1M+ entities).
4. `exitium_reflect` + derive macro: fields, enums, attributes (range, tooltip, hidden), generic containers.
5. `exitium_serial`: text format + binary format, schema hashing, versioned migration hooks, fuzz tests on round-trips.
6. `exitium_profiler`: tracing → Tracy integration and frame stats.
7. Bench harness + `xtask bench-compare` wired into CI with a stored baseline.

**Exit:** a headless ECS sim with 1M entities ticks in parallel, round-trips through both formats, and has profiled benchmarks gating CI.

### Phase 2 — Runtime

1. `exitium_app`: plugin model, main loop, fixed/variable ticks, headless mode.
2. `exitium_platform`: winit window and input (keyboard, mouse, gamepad, touch-ready), desktop and web.
3. `exitium_render` v1: wgpu device and surface, sprite batching, high-res image rendering with mipmaps (portraits), text rendering, cameras, render-to-texture (for screenshots and MCP).
4. `exitium_assets`: GUIDs, import pipeline, file-watch hot reload (textures, shaders, data), pack builder.
5. `exitium_data`: Data Assets with prototype/variant inheritance and inspector-ready reflection.
6. `exitium_scene`: scenes, prefabs, prefab variants.
7. `exitium_audio`: kira integration.
8. `exitium_script`: Luau via mlua, the layered API, hot reload, and the WASM spike (R5).

**Exit:** a windowed and web sample renders portraits and text, loads data assets and scenes, plays audio, runs Luau, and hot-reloads assets.

### Phase 3 — Game UI (Glutton critical path)

1. Retained widget tree on top of the ECS or a dedicated arena (decided by ADR), with taffy flex layout.
2. Core widgets: panel, label, rich text, image, button, toggle, slider, progress bar, scroll view, list/virtualized list, grid, tooltip, modal, tabs, number display (bound to `Num`).
3. Styling/theme system, 9-slice images, transitions/tweens.
4. Data binding: widgets bind to ECS components or resources and update only on change, so thousands of live counters stay cheap.
5. UI markup file format (text ↔ binary) with hot reload.
6. Immediate-mode layer for debug overlays and quick tools.
7. UI perf benchmarks: layout of 10k nodes, rebinding counters at high tick rates.

**Exit:** a Glutton-style UI mockup with heavy live numbers runs at full frame rate on min-spec settings.

### Phase 4 — Editor and MCP

1. The command bus with undo/redo, shared by editor, MCP and Lua.
2. Editor shell (egui): docking layout, hierarchy, inspector (generated from reflection), asset browser, console/log.
3. Editors: Data Asset editor (with variant overrides shown), UI layout editor (WYSIWYG over the retained UI), scene editor.
4. Play-in-editor, pause/step, profiler panel.
5. `exitium_mcp`:
   - Offline tools: docs/API query, schema dump.
   - Live tools: entities, components, assets, data, UI, commands, screenshots.
   - Ops tools: build, test, bench, run headless.
   - Tool schemas generated from reflection.
6. A parity test that fails if an editor command has no MCP equivalent.

**Exit:** an agent builds a small UI screen and its data assets entirely through MCP, and a human can then edit the same result in the editor.

### Phase 5 — Incremental sim and saves

1. `exitium_sim`: high-rate fixed ticks, producer/consumer patterns, modifiers/multipliers pipelines, automation and schedulers.
2. Offline progress: analytic solvers where possible, chunked fast-forward otherwise, with a time cap and validation hooks.
3. Prestige/reset layers framework.
4. `exitium_save`: versioned saves, migration chains, zstd, authenticated encryption/tamper detection, autosave, a cloud provider trait.
5. Stress benchmarks: maximum sim complexity on min-spec settings.

**Exit:** a sample incremental game with prestige and offline gains saves, migrates and reloads correctly.

### Phase 6 — Platform, online and backend

1. Web build pipeline (wasm-bindgen, threads when cross-origin isolated, WebGL2 fallback) and itch.io packaging.
2. `exitium_steam`: Steamworks init, achievements, Steam cloud as a save provider, overlay, demo app.
3. `exitium_net` v1: transport, authoritative client/server, replication of components marked via reflection.
4. `exitium_server`: headless runtime built for stateless, horizontal scaling.
5. `exitium_backend`: auth, cloud saves, leaderboards, server-side offline-gain validation (reusing `exitium_sim`), external hosting/orchestration adapters, load tests.
6. Release workflow: Steam depot upload and itch butler push.

**Exit:** a sample game syncs saves to the backend, posts to leaderboards, has offline gains validated by the server, and ships to both Steam (private branch) and itch.

### Phase 7 — Glutton (separate repo)

1. Create the `Glutton` repo depending on the `exitium` facade, with its own factory config inherited from the engine's templates.
2. Build the vertical slice → demo content. Engine gaps found here become issues on the engine board.
3. Release the Steam demo and itch build together.

### Phase 8 — Engine breadth

1. Pixel art pipeline: virtual resolution with integer scaling, sub-pixel camera, palette swap/indexed color, pixel-perfect rotation, Aseprite import (tags, slices, animations).
2. Normal-mapped 2D lighting.
3. Camera/game modes: top-down, sidescroller, isometric with depth sorting and height levels.
4. `exitium_tilemap` for all three modes, with an editor tilemap tool.
5. `exitium_physics`: rapier2d integration, debug draw, editor tooling.

### Phase 9 — Scale and destruction groundwork

1. Network scale-out: interest management, sharding hooks, gateway/edge design, load testing toward large concurrency targets.
2. `exitium_destruct` groundwork: chunked cell grids, GPU compute sim interface, dirty-region rendering, collider regeneration from cell and polygon changes, a fracture interface.
3. Full destruction engine spec (an ADR with the owner), then implementation as its own roadmap.

---

## Immediate next steps

1. The owner decides R1, R2 and R3 (INTERVIEW.md §4).
2. Run Phase 0 steps 2–4: toolchain, git, the GitHub repo and the workspace skeleton.
3. Continue Phase 0 through the dry run, then hand the roadmap to the factory.
