# Exitium Engine — Interview Results

Date: 2026-11-16
Interviewee: project owner (sole developer)
Source documents: `PROMPT.md`, two interview rounds held in chat

This document records the answers given during the interview and my understanding of what they imply for the project. Section 4 lists the tensions, risks and open questions that need a decision or a spike before or during Phase 0.

---

## 1. Project summary (my understanding)

**Exitium** is a 2D game engine written in Rust. Its main priority is performance. Its primary user is **an AI agent**; the human developer/designer mostly reviews, edits and cleans up what agents produce. Even so, the engine must ship a complete standalone visual editor that humans can use for all work.

The repository runs as an **AI software factory**. Claude agents with defined roles take approved GitHub Issues, implement them in parallel worktrees, open PRs, review them, and auto-merge when every gate passes. The owner intervenes only when a PR fails two AI review rounds (ADR-0005).

The first consumer was planned as **Glutton**, an incremental roguelite in a separate repository. It is UI-heavy and uses static (non-animated) hand-drawn portraits. It has **no pixel art**. It needs cloud saves, leaderboards and server-side offline gains, and it releases a Steam demo and an itch.io build at the same time.

*Updated 2026-11-24: Glutton is no longer the driver. The full MVP (`docs/specs/mvp-roadmap.md`) is proven by six rehearsal games in the repo root `examples/`. A real game is built in a new folder after the MVP is tested.*

Later consumers:
- High-performance multiplayer games. The long-term target is an architecture that can scale toward 100M concurrent players.
- A 2D game built on a heavy destruction engine. The destruction engine will be specified later; early versions only need the groundwork.

---

## 2. Answers — Round 1 (engine)

| # | Topic | Answer | Implication |
|---|---|---|---|
| 1 | First milestone | Ship a full incremental roguelite (Glutton): heavy UI, static hand-drawn portraits | The **UI system** and the **incremental sim/save stack** are the critical path. Pixel art, isometric, physics and destruction are not on Glutton's path. Superseded 2026-11-24 (`docs/specs/mvp-roadmap.md`): Glutton no longer drives priority, and 2D breadth and physics are in the MVP (destruction is not). |
| 2 | Editor | Standalone editor app | Separate `exitium_editor` binary. Shipped games contain no editor code. |
| 3 | Platforms | Windows, Linux, macOS, Web now; mobile later | Use wgpu. Updated 2026-11-24: the web build uses wgpu's WebGPU backend only; a WebGL2 fallback is out of the MVP, not dropped (`docs/specs/mvp-roadmap.md`, `docs/specs/render-v1.md`). In the MVP the editor, MCP and CLI are Windows-only, and testing covers Windows and Chrome. Linux and macOS for those tools come after the MVP. Keep platform code behind traits so mobile can be added later. |
| 4 | Audience | Owner only for now; make it extensible toward a public release | No doc-maintenance burden now, but keep public API boundaries clean and use a permissive-ready license structure. |
| 5 | Rust experience / feel | Basic Rust. Take the best ideas from Godot, Unity, Unreal and Bevy, with its own identity | The code must stay readable and well commented at module boundaries. Agents do most of the writing, and the owner needs to be able to review it. |
| 6 | ECS | Custom archetype ECS (recommended) | Build `exitium_ecs`: SoA archetype storage, parallel system scheduling, change detection, built-in reflection hooks. |
| 7 | Renderer | wgpu | |
| 8 | Scripting | Rust + Lua | Lua flavor: **Luau** (answer C19). |
| 9 | Multithreading | From day one | Job system and parallel scheduler are core foundations, not retrofits. |
| 10 | Determinism | Not necessary | Use f64 freely. Lockstep and rollback are out of scope. The server validates within tolerances. |
| 11 | Number range | f64 (up to ~1e308) is enough | No BigNum. Use a `Num` newtype over f64 with incremental-style formatting, so the backing type could still be swapped later. |
| 12 | Sim features | All: fixed-tick sim, offline progress, prestige/reset layers, automation | Build `exitium_sim` as a first-class engine module. |
| 13 | Scale | First game is modest, but the engine is optimized for maximum complexity: many parallel processes, very high tick rates. Minimum spec is a 4-core / 8 GB desktop. The dev PC is far stronger. | Benchmarks must also run with a 4-thread cap and memory budgets. Perf gates in CI. |
| 14 | Serialization | Recommended: dual format | Human-readable text for editing and git diffs, compiled to a compact custom binary for shipping. |
| 15 | Scriptable objects | Recommended: Rust structs + derive, editable in the inspector, referenced by stable ID. **Yes to inheritance/variants.** | "Data Assets" with prototype → variant overrides, similar to Unity prefab variants or Godot inherited resources. |
| 16 | Hot reload | All: assets, data objects, shaders, game code | Rust game-code hot reload goes through a dynamic library or Lua. See risk R6. |
| 17 | Saves | All: versioning/migration, compression, encryption/tamper resistance, cloud hooks | Build `exitium_save` with schema versions, migration chains, zstd, an authenticated encryption layer, and a provider trait for Steam cloud and our own backend. |
| 18 | UI | Both a retained widget tree and immediate mode. The editor uses egui. | The game UI is a custom retained tree (flex layout, theming, data binding, markup files) plus an immediate-mode layer for debug and tools. The editor is egui. |
| 19 | Pixel art | All features matter | Virtual resolution with integer scaling, sub-pixel camera, palette swap/indexed color, Aseprite import, pixel-perfect rotation, normal-mapped 2D lighting. Scheduled after Glutton's needs. Superseded 2026-11-24: in the MVP as Stage 4, built correct and tested but not yet optimized (`docs/specs/mvp-roadmap.md`). |
| 20 | Camera modes / tiles / physics | All three modes, depth-sorted iso with height levels, tilemaps for all modes, Rapier | Use `rapier2d`. |
| 21 | Networking | Authoritative server + client. Concurrent players in the thousands now, offline gains for many more. Architecture optimized toward 100M concurrent. | Engine provides netcode, a headless server runtime, and horizontally scalable/stateless server design. See C18. Updated 2026-11-24: netcode, the server and the backend are in the MVP. Network scale-out (Phase 9) is not. |
| 22 | Destruction | Both cellular (Noita-like) and polygon fracture; specifics later | Groundwork only: chunked grid storage, GPU compute pipeline, dirty-region tracking, dynamic collider regeneration interface. Updated 2026-11-24: out of the MVP (Phase 9). |
| 23 | MCP | All of it: docs/API queries, live editor/game control, builds/tests/benchmarks. **The engine assumes the main user is an AI agent.** | Every editor capability must be reachable through MCP. This is a core design rule, not an add-on. |
| 24 | Graphify | Git hook on each push; graph output committed | `graphify-out/` lives in the repo. A pre-push hook regenerates it. See R9. Superseded by ADR-0004: the graph is local and gitignored. |
| 25 | GitHub | New private repo `nathanialwm/ExitiumEngine` | |
| 26 | CI | No runners yet. Self-hosted Docker runners. **Every check blocks merge.** | fmt, clippy, tests, bench regression, WASM build, plus the platform build matrix. |
| 27 | Toolchain | Stable Rust only | No portable SIMD; use `wide` or `std::arch` behind cfg. Pin the toolchain with `rust-toolchain.toml`. |
| 28 | Other systems | Audio and profiler needed early | Audio via `kira`. Profiler via `tracing` + Tracy, plus an in-editor profiler panel. |

---

## 3. Answers — Round 2 (AI software factory and follow-ups)

### A. Factory operating model

| # | Topic | Answer |
|---|---|---|
| A1 | Main loop | Confirmed: Issue → plan → implement on a branch → PR → CI → AI review → merge → graph/docs update. |
| A2 | Autonomy | **(b)**: auto-merge when CI is green and the AI review passes. The owner reviews only flagged PRs. |
| A3 | Human gates (always need owner approval) | Public API changes, new dependencies, CI/factory config, save-format/serialization changes, licensing, anything touching secrets, Steam releases. Superseded by ADR-0005: the owner is involved only when a PR fails two AI review rounds. |
| A4 | Agent roles | See the role table below. |
| A5 | AI vendor | Claude only. |
| A6 | Where agents run | **(b)**: on the dev machine through T3 Code with git worktrees. The Docker runners are for CI only. |
| A7 | Parallelism | One branch/worktree per issue. **Cap: 3 concurrent agents.** Raised to 8 by #103, with a footprint conflict check (`factory-run` step 3). |
| A8 | Budget | **Do not start a new issue when less than 5% of session usage remains.** Work already in flight may finish. |
| A9 | Work intake | GitHub Issues; GitHub Projects for the roadmap. Agents may file issues, but **each agent-filed issue must be approved by a review agent (Sonnet, medium effort)** before work begins. |
| A10 | Specs | Spec/ADR required for systems-level work; not for small tasks. |
| A11 | Definition of done | fmt, clippy `-D warnings`, tests, no bench regression above **5%**, WASM build, docs and MCP schema updated, graphify regenerated. |
| A12 | Notifications | GitHub notifications and Claude (push) notifications. |
| A13 | Memory/conventions | Root `CLAUDE.md` + per-crate `CLAUDE.md`/`AGENTS.md`, the graphify graph as the shared map, and a `docs/decisions/` ADR log. |
| A14 | Self-improvement | Agents may edit the factory (prompts, workflows, rules, skills), **but every such change requires owner approval**. Superseded by ADR-0005: factory changes are reviewed like code; the owner is involved only after two failed review rounds. |

#### Agent roster

| Role | Model / effort | Responsibility |
|---|---|---|
| Architect | Sonnet 5.5, high | Breaks epics into issues, writes specs/ADRs, keeps the dependency order on the roadmap. |
| Implementer, specialized by domain | **Opus 5.5, max** | Writes the code. Specializations I propose: **Engine Core Developer** (ECS, tasks, reflect, serial), **Rendering/GPU Engineer**, **UI Developer/Designer** (widgets, layout, themes, UI markup), **Editor & Tools Developer** (egui editor, MCP tools, CLI), **Gameplay/Sim Developer** (incremental sim, saves, data assets), **Network & Backend Engineer**, **Scripting Integrator** (Luau bindings). |
| Reviewer | Sonnet 5.5, high | Adversarial code review. Must not be the model that wrote the code. Approves or requests changes. |
| Test Writer | Sonnet 5.5, medium | Writes unit, integration, property and fuzz tests. |
| Test Reviewer | Sonnet 5.5, medium | Reviews tests for redundancy, weak assertions and missing cases. |
| Perf Agent | Opus 5.5, high | Writes and maintains benchmarks, investigates regressions, profiles hot paths. |
| Docs & Graph Maintainer | Sonnet 5.5, high | Keeps rustdoc, crate READMEs, MCP tool docs, ADR index and the graphify output current. |
| Triage | Sonnet 5.5, high | Labels, deduplicates, sizes and routes issues to the right implementer specialization. |
| Issue Approver | Sonnet 5.5, medium | Approves or rejects agent-filed issues before work starts (A9). |
| Token Optimizer | Sonnet 5.5, high | Reviews agent transcripts and process, then improves rules, skills and prompts to raise quality and cut token use. Small, safe changes go through a PR (A14; reviewed like code since ADR-0005). **Large or unsafe changes are left as a suggestion in chat** instead of a PR. |

### B. CI and infrastructure

| # | Topic | Answer |
|---|---|---|
| B15 | Runner host | This PC: Windows 11 Home, Docker 28.4 installed. |
| B16 | macOS | GitHub-hosted macOS runner for that one job. |
| B17 | Branching | `main` is protected; PR-only, squash merge, Conventional Commits. |

### C. Follow-ups

| # | Topic | Answer |
|---|---|---|
| C18 | Backend scope | The engine provides netcode, the headless server runtime, and optimized **hookups** for external hosting/orchestration services (e.g. Multiplay-style providers). The engine's own backend must be able to run Glutton's online features: cloud saves, leaderboards, server-side offline gains. Superseded 2026-11-24: Glutton no longer drives the plan; the same features are proven by `examples/incremental` (`docs/specs/mvp-roadmap.md`). |
| C19 | Lua | Luau. Full engine access. **Modding is a future goal.** |
| C20 | Glutton art | No pixel art; UI plus hand-drawn assets only. |
| C21 | Steam | Steamworks integration lives in the engine. The itch.io build and the Steam demo release together. In the MVP, Steam works by hand with Valve's test app, and the CLI produces the depot layout and the itch zip. The owner pushes the release workflows. |
| C22 | Game repo | Separate repo. Working title **Glutton**. Superseded 2026-11-24: rehearsal games live in the repo root `examples/`, and a real game is built in a new folder after the MVP is tested. The Glutton repo is out of the MVP. |

---

## 4. Tensions, risks and open questions

These came out of the answers and need a decision, a spike, or at least awareness.

**R1. Windows Home and Docker runners.** Docker Desktop on Windows 11 Home only runs Linux containers (through WSL2); Windows containers need Pro or Enterprise. Windows builds and tests therefore cannot run inside Docker on this PC.
*Proposal:* Linux containers handle Linux, WASM and cross-compiled Windows checks (`cargo-xwin`). One native (non-Docker) runner on the Windows host runs Windows tests and Windows GPU tests.
**Decision (2026-11-16): accepted.**

**R2. macOS minutes on a private repo.** GitHub-hosted macOS minutes are billed at a high multiplier on private repos. Making the macOS job block every PR (A11/26) could burn through the free allowance quickly.
*Options:* (a) run it on every PR anyway; (b) run it only on PRs that touch platform, render or windowing code, plus nightly on `main`.
**Decision (2026-11-16):** run macOS only on PRs that touch platform, render or windowing code. No nightly. The macOS job is mainly insurance for when the engine goes public, so it stays low priority until then. Implementation note: a path-filtered *required* check would sit pending forever on PRs it skips. The workflow must therefore always run and report success when no relevant paths changed.

**R3. Bot identity for gates.** Agents act through the owner's GitHub account. GitHub doesn't let an author approve their own PR, and a "human-approved" label added by an agent would look identical to one added by the owner.
*Proposal:* create a GitHub App (`exitium-factory`) for agent actions. The AI review is then posted as a required status check, and the `human-gate` check can verify that the approval label was applied by `nathanialwm`, not the app.
**Decision (2026-11-16): accepted.** The owner creates the GitHub App (a manual step); its credentials are stored as a local secret for agents and as a repo secret for CI.

**R4. The 5% usage gate (A8).** This needs a reliable way to read remaining session usage. The candidates are Claude Code's status-line/usage data or the `/usage` output. Which one is exposed programmatically must be verified in Phase 0. Fallback: the orchestrator asks before starting each new issue once usage is uncertain.

**R5. Web target constraints.**
- Threads on the web need `SharedArrayBuffer`, which requires cross-origin isolation headers, and also `-C target-feature=+atomics` with `build-std`, which is nightly Rust. **Decided 2026-11-24:** web threads are out of the MVP, and the web build stays single-threaded.
- WebGPU isn't available in every browser. The web build uses wgpu's WebGPU backend only, so browsers without WebGPU are unsupported for now. A WebGL2 fallback is out of the MVP, not dropped, and would limit compute shaders on the web (`docs/specs/mvp-roadmap.md`, `docs/specs/render-v1.md`).
- Building Luau for WASM through `mlua` needs an early spike. **Decided 2026-11-24:** the spike runs first (SP1 in the roadmap). If it fails, scripts are native-only and web builds run without Luau, and `exitium_script` still builds for wasm as a no-op stub. Rust gameplay must be enough to play every web-exported example.

The engine must degrade gracefully, e.g. a single-threaded scheduler and no compute path on the web.

**R6. Hot-reloading Rust game code.** Stable Rust has no first-class support for this. Options are a dylib reload boundary (fragile across struct layout changes) or routing iteration-heavy logic through Luau and data assets. *Proposal:* Luau + data assets + shaders + assets get true hot reload; Rust game code gets a fast incremental rebuild with state-preserving restart (save → rebuild → restore). A dylib path can be an experimental opt-in later.

**R7. "100M concurrent" vs. no determinism.** That scale is a backend topology problem (sharding, stateless gateways, distributed storage, edge routing) more than an engine problem. The engine's job is a lean, horizontally scalable headless server, efficient wire formats, and a pluggable backend/hosting interface. Offline-gain validation on the server reuses the same `exitium_sim` code. Since determinism isn't required, validation uses tolerances rather than exact replay.

**R8. Full Lua access vs. future modding.** Full engine access is fine for first-party scripts. Modding later will need a sandboxed capability tier, so the Lua API should be layered from the start (a `core` safe tier vs. a `privileged` tier) so modding doesn't force a rewrite.

**R9. Graphify on push.** *(Superseded by ADR-0004: the graph is no longer committed.)* A `pre-push` hook can't add a commit to the push that is already running. *Proposal:* the hook runs `graphify update .`; if the output changed, it commits `chore(graph): refresh` and aborts with "graph updated, push again". Agents push through an `xtask push` wrapper that retries automatically. `graph.json` merge conflicts between parallel branches are handled by graphify's union merge driver, configured in `.gitattributes`.

**R10. Toolchain age.** The installed toolchain is Rust 1.89 (2025). Phase 0 will update stable and pin a current version in `rust-toolchain.toml`.

**R11. Basic Rust experience with heavy `unsafe` pressure.** A custom high-performance ECS needs some `unsafe`. Rule: `unsafe` is confined to a small number of audited modules. Each `unsafe` block carries a `// SAFETY:` justification. Miri tests run on those modules in CI. Changes to them are flagged for owner review.

---

## 5. Design principles derived from the interview

1. **AI-first parity.** Anything a human can do in the editor, an agent can do through MCP. Both go through the same command layer.
2. **One reflection source of truth.** A single `#[derive(Reflect)]` drives inspector UI, text/binary serialization, Luau bindings, MCP schemas and save migration.
3. **Command bus for all mutation.** Editor actions, MCP calls and Lua edits are typed, serializable commands. This gives undo/redo, audit logs, replays and agent safety from one mechanism.
4. **Performance is measured, never assumed.** Every hot system has a benchmark, and CI blocks any regression over 5%. Minimum-spec profiles (4 threads, 8 GB) are part of the bench suite.
5. **The rehearsal games drive priority; the engine stays general.** Build what the six `examples/` games need (`docs/specs/mvp-roadmap.md`), without shortcuts that block multiplayer or destruction later. *(Originally "Glutton drives priority"; Glutton is no longer the driver.)*
6. **Readable for a basic-level Rust reviewer.** Clear module boundaries, minimal macro magic in user-facing APIs, and documented invariants.
