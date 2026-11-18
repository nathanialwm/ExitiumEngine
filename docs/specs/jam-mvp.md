# Spec: Jam MVP roadmap (stages, lanes, exit criteria)

- Issue: #112
- Crates: none. This is a roadmap spec: it governs no crate and edits no code. It sequences work across every runtime crate, `exitium_command`, `exitium_editor`, `exitium_mcp`, `exitium_cli` and the `exitium` facade.
- Status: draft. The plan in it was approved by the owner on 2026-11-18.

## Goal

Record the owner-approved plan for the **Jam MVP** so that later agents work from the repo and not from chat history. The MVP is: an agent builds a 2D game through MCP plus Rust code, a human refines it in the editor, and it exports to a Windows exe and to the web (wasm).

The plan has five stages. Each stage has a lane layout (which crates are edited at the same time), a reason why that layout passes the `factory-run` conflict check, and an exit criterion that can be checked.

| Stage | Name | Exit |
|---|---|---|
| 0 | Specs | All nine spec issues merged (#22, #23, #106 to #112). |
| 1 | Foundations and standalone backends | ECS, reflect and serial merged. Every backend has a Windows example and builds for wasm. |
| 2 | Runtime integration | The sample game runs on Windows and in a browser. |
| 3 | Editor and MCP (Windows only) | An agent builds a scene through MCP, and a human edits it in the editor. |
| 4 | Export and rehearsal | `exitium new`, `export --target windows\|web` and the MCP export tool work, and a rehearsal mini game ships to both targets. **This is the Jam MVP.** |

Size: about 80 PRs. Rough estimate: 4 to 7 days of factory time (see "Performance" for what that assumes).

## Non-goals

- Re-deciding anything in the contract. The owner decisions below are inputs.
- Designing a crate. Each crate's design belongs to its spec (index below). If this file and a crate spec disagree about a crate's design, the crate spec wins. If they disagree about order, lanes or hotspots, this file wins.
- Replacing the epics #3 to #11. They stay the long-term roadmap. This file says which slice of them the MVP takes and where the rest returns (see "Scope cuts").
- Editing PLAN.md (a hotspot). The epics' checklists and PLAN.md are updated by a follow-up `docs-graph` pass after the stages land, not by this PR.

## Design

### The contract (owner decisions, 2026-11-18)

- **Rust only.** No Luau in the MVP. Gameplay is Rust.
- **Game UI** is a retained tree on taffy.
- **Web rendering** uses wgpu's WebGPU backend. There is no WebGL2 fallback in the MVP. Windows uses wgpu's native backends.
- **`exitium_mcp`** is a separate stdio process. It talks to the running editor over localhost.
- **The editor** is a feature of the game binary (`--features editor`), built on egui.
- **Windows only where it expedites.** The editor, MCP and CLI are native Windows only (`NATIVE_ONLY` in `xtask/src/ci.rs`). The runtime crates (ecs, reflect, serial, app, platform, render, physics, audio, assets, scene, ui) must build for wasm32. Testing covers Windows and Chrome only.
- **Crate graph.** In Stage 1 the backend crates (platform, render, physics, audio, assets, ui) depend on no workspace crate except `exitium_core`, and not on each other. In Stage 2 their ECS integration lands in a `plugin` module of the same crate, which depends on `exitium_ecs` and `exitium_app`.
- **Ownership.** `exitium_app` owns `App`, `Plugin`, the schedules (Startup, FixedUpdate at 60 Hz by default, Update, render extraction), `Time`, and `Transform2d` / `GlobalTransform2d` with hierarchy propagation. The command bus lives in a new runtime crate, `exitium_command`, so it is usable without the editor. `exitium_mcp` must not depend on `exitium_editor`.
- **Dependencies.** Every spec lists the crates.io dependencies its crates need. The two intake PRs add them all at once, so implementation issues never touch `Cargo.lock`.

### Spec index

Every Jam MVP spec, by issue. Files that do not exist yet are linked by the path in the issue's Footprint, and are marked (planned).

| Issue | Spec | Governs | Needed by |
|---|---|---|---|
| #19 | [Job system](tasks-job-system.md) (merged) | `exitium_tasks` | Not on the MVP path (see cuts). |
| #20 | [Reflection model](reflect-model.md) (merged) | `exitium_reflect`, `exitium_reflect_derive` | Stage 1 reflect lane |
| #21 | [ECS storage and queries](ecs-storage-queries.md) (merged) | `exitium_ecs` | Stage 1 ECS lane |
| #22 | [ECS scheduler](ecs-scheduler.md) (planned) | `exitium_ecs` | Stage 1 ECS lane. Blocks #106, #110. |
| #23 | [Text serialization format](serial-text-format.md) (planned) | `exitium_serial` | Stage 1 serial lane. Blocks #111. |
| #106 | [App loop, plugins, transforms, platform](app-platform.md) (planned) | `exitium_app`, `exitium_platform` | Stages 1 and 2 |
| #107 | [Render v1](render-v1.md) (planned) | `exitium_render` | Stages 1 and 2 |
| #108 | [2D physics](physics-2d.md) (planned) | `exitium_physics` | Stages 1 and 2 |
| #109 | [Game UI MVP](ui-mvp.md) (planned) | `exitium_ui` | Stages 1 and 2 |
| #110 | [Command bus, editor, MCP](editor-mcp-mvp.md) (planned) | `exitium_command`, `exitium_editor`, `exitium_mcp` | Stage 3 |
| #111 | [Assets, scenes, audio, export](assets-scene-export.md) (planned) | `exitium_assets`, `exitium_scene`, `exitium_audio`, `exitium_cli` | Stages 1, 2 and 4 |
| #112 | This file: [Jam MVP roadmap](jam-mvp.md) | none | All stages |

### How the conflict check applies

`factory-run` step 3 lets two issues run at once only if all four rules hold. This file refers to them as R1 to R4:

- **R1, disjoint edits.** No `Edits` entry (plus `Governs`, for spec issues) of one issue equals or contains one of the other. One crate has at most one issue in flight.
- **R2, one hotspot.** At most one in-flight item touches any hotspot. Hotspots are everything outside a crate directory or a spec file: root `Cargo.toml`, `Cargo.lock`, `crates/exitium/` (the facade), `xtask/`, `deny.toml`, `docs/decisions/`, root files and so on. Any change to a `[dependencies]` table rewrites `Cargo.lock`, so it is a hotspot change.
- **R3, no crate dependency.** Neither issue edits a crate that depends, directly or transitively, on a crate the other edits, using the graph after both land (the current graph plus both issues' `New crate edges`, dev-dependencies included).
- **R4, no issue dependency.** Neither is blocked by the other, and neither needs an API the other is adding.

Three consequences shape the whole plan:

1. **Crate edges are what limit parallelism.** Two crates run side by side only if neither depends on the other, and no edge joins them through a third crate that is also being edited. That is why Stage 1 keeps the backends dependent on `exitium_core` only.
2. **The intake PRs hold the hotspot so nothing else has to.** They add every crates.io dependency and every workspace edge for the stage in one PR. After one lands, a lane issue edits only its crate directory, which is how lanes avoid R2.
3. **The intake PRs run alone.** They edit the `Cargo.toml` of every crate they wire, so R1 excludes any other issue in those crates, and R2 excludes any other hotspot holder. Each runs at a stage boundary, when nothing else is in flight.

An edge declared in an intake PR is in the graph for every later R3 check. The edges are chosen so that the lanes below still pass. Where a spec would need an edge that breaks a lane, see "Open questions".

### Stage 0: specs

**Work.** Nine spec issues. #22 (ECS scheduler), #23 (text serial), #106 (app and platform), #107 (render v1), #108 (physics), #109 (UI), #110 (command bus, editor, MCP), #111 (assets, scenes, audio, export), and #112 (this roadmap). #106 and #110 are blocked by #22. #111 is blocked by #23.

**Lane layout.** Two waves. Wave 1: #22, #23, #107, #108, #109, #112 (six at once). Wave 2: #106 and #110 after #22 merges, and #111 after #23 merges.

**Why it passes.**
- R1: each issue edits one exact file under `docs/specs/`, and the `Governs` sets are pairwise disjoint: #22 `exitium_ecs`; #23 `exitium_serial`; #106 `exitium_app` and `exitium_platform`; #107 `exitium_render`; #108 `exitium_physics`; #109 `exitium_ui`; #110 `exitium_editor`, `exitium_mcp` and `exitium_command`; #111 `exitium_assets`, `exitium_scene`, `exitium_audio` and `exitium_cli`; #112 none.
- R2: every spec declares `Hotspots: none`.
- R3: a `Governs` crate counts as edited, so R3 applies. No workspace crate has a workspace dependency today (every `[dependencies]` table is empty except `glam` in core and `tracing` in the profiler), and every spec declares `New crate edges: none`. So no governed crate depends on another.
- R4: the only dependencies are the three `Blocked by` lines above, which is exactly why they are in wave 2.

**Exit.** All nine merged. The remaining `needs-spec` labels are lifted from the dependent issues, per the `write-spec` skill (#44 to #47, #53, #54, and the implementation issues each new spec files).

Stage 1 needs only seven of the nine (#22, #23, #106, #107, #108, #109, #111) before its intake PR can start. #110 and #112 govern crates and files that no Stage 1 issue edits and that gain no edge in intake 1, so they may still be open.

### Stage 1: foundations and standalone backends

**Work.** First the **dependency and crate-graph intake PR** (intake 1), then the lanes:

| Lane | Crate(s) | Issues | Notes |
|---|---|---|---|
| ECS | `exitium_ecs` | #38 to #47, #49, #50, #51 (13 PRs) | **The critical path.** Order: #38, #39, #40, #41, #42, #43, #44, #45, #46, #47, #49, #50, #51. |
| Reflect | `exitium_reflect` | #31, #32, #33, #37 | |
| Reflect derive | `exitium_reflect_derive` | #34, #35 | |
| Serial text | `exitium_serial` | #53, #54, #55 | |
| Platform | `exitium_platform` | filed from #106 | winit window and input. |
| Render | `exitium_render` | filed from #107 | wgpu device, sprites, camera, text, screenshots. |
| Physics | `exitium_physics` | filed from #108 | rapier2d. |
| Audio | `exitium_audio` | filed from #111 | |
| Assets | `exitium_assets` | filed from #111 | |
| UI layout core | `exitium_ui` | filed from #109 | taffy tree and layout, no ECS yet. |

Each backend lane ends with a **Windows example** (`crates/<crate>/examples/`, inside the crate's own footprint) and a clean `cargo build --target wasm32-unknown-unknown`. An example opens its own window with a dev-dependency declared in intake 1. It does not use `exitium_platform`, because that would add a sibling edge.

**Intake 1.**
- Adds every crates.io dependency the Stage 1 crates need to `[workspace.dependencies]`, and to each crate's `Cargo.toml`.
- Wires the Stage 1 workspace edges: `exitium_ecs` to `exitium_core` and `exitium_reflect`; `exitium_reflect` to `exitium_core` and `exitium_reflect_derive`; `exitium_serial` to `exitium_core` and `exitium_reflect`; and `exitium_core` only for platform, render, physics, audio, assets and ui. It does **not** add `exitium_ecs` to `exitium_tasks` (see cuts).
- Updates `deny.toml` if a licence needs allowing.
- Holds the hotspots `Cargo.toml`, `Cargo.lock` and `deny.toml`. It is the **only hotspot holder** in Stage 1.

**Why it passes.** After intake 1 the graph is:

```
exitium_core  <-  exitium_reflect_derive  <-  exitium_reflect  <-  exitium_ecs
                                                    ^
                                                    +----------  exitium_serial
exitium_core  <-  platform, render, physics, audio, assets, ui    (each only on core)
```

- R1: every lane is one crate directory, so lanes are disjoint, and within a lane one issue runs at a time.
- R2: after intake 1 no lane issue changes a `Cargo.toml` dependency table, so none holds a hotspot. Examples and benches live inside the crate directory.
- R3, using the graph above:
  - The six backends depend on core only and on nothing in each other, and nobody in a lane edits core. So each backend runs beside every other lane.
  - `exitium_ecs` and `exitium_serial` do not depend on each other, so they run beside each other.
  - `exitium_reflect` and `exitium_reflect_derive` are **a dependency of both** ECS and serial, and of each other. So the reflect group (reflect, then derive, one at a time) **never runs beside the ECS or serial lanes**. They are ordered, not parallel.
- R4: cross-lane needs are all `Blocked by` lines: #34 blocks #50 and #53, and #50 needs #41. Both sides of each such pair are already ordered by R3.

**Schedule that follows from R3.** Run the reflect group first: it is short (six PRs), and finishing it unblocks both #53 and #50. After it, ECS and serial overlap, and the ECS chain is the critical path. The Project priority must put the reflect lane above the ECS lane for this to hold. Ordering them the other way (ECS first) is slower: the reflect group would then run alone after the ECS chain, and the critical path grows by about two serial PRs.
- Wave 1: reflect group plus the six backends (7 in flight, under the cap of 8).
- Wave 2: ECS, serial and the backends still running (8 in flight, exactly the cap).

**Core is frozen.** Every crate depends on `exitium_core`, so any issue that edits it conflicts with everything. No MVP issue edits core. A needed core change is filed narrowly and the orchestrator runs it at a lane boundary, alone. The same goes for unrelated chores that touch core, the facade or a hotspot (#84, #102): they run in Stage 0 or between stages.

**`docs-graph` passes** are in-flight items whose footprint is the crate directories the merged PR edited. A pass after every ECS merge would stop the next ECS issue from starting, since one crate has one item in flight. See open question 3.

**Exit.** ECS, reflect and serial are merged. Every backend has a Windows example that runs and builds for wasm. `cargo xtask ci all` is green on `main`. No benchmark regression over 5% (#51 sets the ECS baselines).

### Stage 2: runtime integration

**Work.** First **intake 2**, then the spine, then a fan-out, then scene, then the facade:

1. **Intake 2.** Adds the crates.io dependencies of Stage 2, **and of Stages 3 and 4** (egui, the MCP and wasm-bindgen families, whatever the specs name), so Stages 3 and 4 never touch `Cargo.lock`. Wires the Stage 2 edges. Creates the empty `exitium_command` crate (a new crate rewrites `Cargo.lock` too) with its `CLAUDE.md`, and wires the Stage 3 and 4 edges. Adds `exitium_mcp` to `NATIVE_ONLY` (a change to the hotspot `xtask/`).
2. **Spine: `exitium_app`.** First `App`, `Plugin`, the schedules and `Time`; then `Transform2d` / `GlobalTransform2d` with propagation over the Stage 1 hierarchy (#49).
3. **Fan-out: one plugin per crate.** A `plugin` module in each of `exitium_platform`, `exitium_render`, `exitium_physics`, `exitium_audio`, `exitium_assets` and `exitium_ui`. Six lanes, one crate each.
4. **Scene.** `exitium_scene`: scene load and save in the text format, with entity references.
5. **Facade.** `exitium` with `DefaultPlugins`, and the sample game.

**Wiring after intake 2.**

```
app                     -> core, ecs
platform, render, physics,
audio, assets, ui       -> core, ecs, app          (plugin modules; no edge among them)
scene                   -> core, ecs, reflect, serial, app   (plus assets if #111 needs it)
command                 -> core, ecs, reflect
editor                  -> command, app, ecs, reflect, render, platform, ui  (egui; the set is #110's)
mcp                     -> command, core, reflect            (no edge to editor, and none from editor)
cli                     -> the crates its export step needs  (#111's)
exitium (facade)        -> every runtime crate
```

This is the expected shape; the specs are authoritative, and the intake issue is built from them.

**Why it passes.**
- Intake 2 runs alone at the Stage 1 to Stage 2 boundary, for the reasons above. It holds the hotspots `Cargo.toml`, `Cargo.lock`, `deny.toml` and `xtask/`. Stage 1's last lane issues have merged by then.
- **Spine.** Every plugin crate, scene and the facade depend on `exitium_app`, so R3 forbids any of them running beside an `exitium_app` issue. R4 says the same: plugins need the `Plugin` trait and `Transform2d`, which the spine is adding. So the spine runs alone, as a serial chain in one crate (R1). That is the point of making it the spine.
- **Fan-out.** Six lanes, six crate directories (R1). No hotspot, since edges and dependencies came in intake 2 (R2). Each crate depends on core, ecs and app, all frozen by then, and on none of the others (R3), so the six may run together within the cap of 8. Each needs only the merged spine (R4). Cross-crate behaviour goes through types the lower crates own (ECS components and resources, `exitium_core` handles), and lands in scene or the facade, not as an edge between lanes.
- **Scene** runs after the fan-out, not beside it. It is one crate (R1), but its round-trip test needs the plugin components registered, and it may depend on `exitium_assets` (R3, R4).
- **Facade.** `crates/exitium/` is a hotspot and depends on every crate, so by R2 and R3 it runs alone, last. Its issues are one chain. The sample game lives in `crates/exitium/examples/`, inside the facade crate, so it needs no new top-level path.

**Exit.** The sample game (`DefaultPlugins`, a scene loaded from a text file, sprites, text, physics, UI, audio) runs on Windows and in Chrome via WebGPU. The web run uses a documented `wasm-bindgen` recipe stored with the sample. Productising it is Stage 4.

### Stage 3: editor and MCP (Windows only)

**Work.** The spine, then two lanes and a test:

1. **Spine, part 1: the command bus** (`exitium_command`). Typed, serializable commands with undo and redo, a registry, and schemas derived from reflection.
2. **Spine, part 2: the live-control server.** A localhost server in the running game, on its own OS thread, that hands requests to the main world at one fixed point in the frame. Which crate hosts it is #110's decision.
3. **Editor lane** (`exitium_editor`, egui): shell, hierarchy, inspector generated from reflection, scene save and load, an asset list, console.
4. **MCP lane** (`exitium_mcp`): live tools (entities, components, commands, assets, scene, screenshots) and ops tools (build, test, run).
5. **Parity test:** fails if an editor action has no MCP equivalent.

**Why it passes.**
- Spine: `exitium_command` is depended on by the editor and by MCP, and the server is used by both. So by R3 and R4 they wait until the spine is merged. The spine is a serial chain.
- Lanes: `exitium_editor` and `exitium_mcp` are separate crate directories (R1). Neither holds a hotspot, since egui, the MCP dependencies, `NATIVE_ONLY` and the edges came in intake 2 (R2). By the contract `exitium_mcp` does not depend on `exitium_editor`, and the editor does not link `exitium_mcp` (it is a separate process), so neither edits a crate that depends on the other (R3).
- The lanes stay independent **only if** adding an editor command never requires an edit to `exitium_mcp`. This needs the MCP tool list to be built at runtime from the command registry that the live server exposes, not from a compile-time list in the MCP crate. It is a requirement on #110 (open question 2). If #110 chooses otherwise, the two lanes serialize and the plan still holds, only slower.
- Parity test: it needs both lanes' output (R4), so it runs after both. It must not create an edge between the editor and MCP crates, even as a dev-dependency (R3). It checks the registry: every registered command has an MCP tool, and every editor action dispatches a registered command.

**Exit.** An agent builds a scene through MCP (spawn entities, set components, save the scene, take a screenshot), and a human opens the same scene in the editor, edits it, and saves it.

### Stage 4: export and rehearsal

**Work.**
1. `exitium new`, then `exitium export --target windows`, then `exitium export --target web`: three PRs in `exitium_cli`, one after another (R1: one crate).
2. The **MCP export tool** (`exitium_mcp`). It starts the `exitium` binary as a process, like the ops tools start cargo, so there is no `exitium_mcp` to `exitium_cli` edge. Blocked by both export PRs (R4).
3. The **rehearsal mini game**: an agent builds a small game with `exitium new`, MCP and Rust, then exports it to an exe and to the web. Engine-side output is gap issues and fixes, as with Glutton (epic #9). The game lives outside the engine workspace (open question 5).

**Why it passes.** The CLI chain is one crate. The MCP tool is a different crate with no edge to the CLI, and its R4 dependency is a `Blocked by`. The rehearsal changes no engine crate until it finds a gap, and each gap is an ordinary filed issue that goes through the same check.

**Exit = Jam MVP.** The rehearsal game exists as a Windows exe and as a web build that runs in Chrome. Its scene was built by an agent through MCP and refined by a human in the editor. The export was started through the MCP tool.

### Public API sketch (stage handoffs)

Names come from the contract. The signatures are each spec's; this only fixes who hands what to whom.

```rust
// Stage 1 handoff: each backend crate has a plain API and an example, no ECS.
// Stage 2: each adds `plugin::XPlugin: exitium_app::Plugin`.
// Facade (Stage 2), what the sample game and `exitium new` use:
exitium::App::new()
    .add_plugins(exitium::DefaultPlugins)   // platform, render, assets, audio, physics, ui, scene
    .add_systems(exitium::FixedUpdate, my_rules)
    .run();
```

### Threading

- The executor is single-threaded in the MVP, with an API a parallel executor can implement later. Nothing in the MVP depends on `exitium_tasks`.
- FixedUpdate runs at 60 Hz by default. Web is single-threaded by construction (no web threads in the MVP).
- The live-control server is the only extra thread in the game process. It never touches the world. It queues requests, and the main loop drains them at one fixed point in the frame.
- MCP is a separate process, so it never shares the game's world or frame budget.

## MCP / editor surface

This is a roadmap, so it adds no tool itself. It fixes when each part of the agent-parity rule is met:

- Stages 1 and 2 add **no editor feature**, so they add no MCP tool. Components and resources become visible to MCP automatically, because they derive `Reflect` (design rule 2).
- Stage 3 delivers parity: every mutation is a command on the bus (design rule 3), the editor and MCP are two front ends of the same registry, and the parity test enforces the rule in CI from then on.
- Stage 4 adds the export capability and its MCP tool together (one in `exitium_cli`, one in `exitium_mcp`).
- Any editor feature added after Stage 3 ships with its MCP tool, as CLAUDE.md requires.

## Performance

**Factory budgets (planning numbers).**

| Item | Budget |
|---|---|
| PR size | about 600 changed lines or fewer |
| In flight | at most 8, and only conflict-free |
| Total | about 80 PRs |
| Elapsed | 4 to 7 days of factory time |

Planning allocation of the 80 PRs (an estimate of plus or minus 25%; the "Implementation issues" sections of the specs replace these counts once filed):

| Stage | PRs | Shape |
|---|---|---|
| 0 | 9 | the nine spec issues |
| 1 | about 35 | intake 1, ECS 13, reflect group 6, serial 3, backends and UI about 12 |
| 2 | about 18 | intake 2, spine about 4, plugins about 8, scene about 2, facade about 3 |
| 3 | about 14 | command bus 3, server 1, editor 5, MCP 4, parity 1 |
| 4 | about 5 | CLI 3, MCP export tool 1, rehearsal 1 |

Critical path, in serial PRs: Stage 1 about 19 (reflect group 6, then ECS 13), Stage 2 about 12, Stage 3 about 10, Stage 4 about 6; about 47 in all. The 4 to 7 day estimate holds only if one serial PR (implement, test, review, merge) takes about 2 to 3.5 hours. If a lane's PRs take longer, the elapsed time grows with the critical path, not with the PR count.

**Runtime exit budgets (proposed ceilings, min-spec 4 cores and 8 GB, Windows and Chrome).** Crate specs own the micro-budgets and may only tighten these. Any hot-path PR ships with a benchmark, and CI fails on a regression over 5% (design rule 4).

| Where | Budget |
|---|---|
| Sample game (Stage 2) | at least 60 FPS, p99 frame at most 20 ms, with 2,000 sprites, 200 dynamic physics bodies and 100 UI nodes on screen |
| Sample game | first frame at most 3 s on Windows (release), at most 5 s in Chrome from a warm cache |
| Web build | wasm at most 10 MB gzipped, assets excluded |
| MCP round trip (Stage 3) | one command applied and acknowledged in at most 50 ms (p95); a 1280x720 screenshot in at most 250 ms |
| Export (Stage 4) | cold Windows export at most 10 min, cold web export at most 15 min, on 4 cores |
| ECS (Stage 1) | no regression over 5% against the #51 baselines; absolute budgets are in the ECS specs |

## Test plan

Each stage has one exit check, and the stage is not done until it passes:

- **Stage 1:** `cargo xtask ci all` green on `main` (fmt, clippy, tests, wasm). Each backend example is run on Windows and its PR records that. The ECS benchmarks (#51) show no regression.
- **Stage 2:** the sample game is run on Windows for a fixed number of frames, with a render-to-texture screenshot compared to a stored image. It is also run once by hand in Chrome. The PR records both runs.
- **Stage 3:** an integration test starts a headless game and the `exitium_mcp` binary, builds a scene through MCP tool calls, saves it, loads it back and compares it. The parity test runs in CI.
- **Stage 4:** the rehearsal is the test. Both exports are produced by the CLI and by the MCP tool, started on a clean checkout, and both run.

The wasm build covers the runtime crates in CI (`cargo build --workspace --target wasm32-unknown-unknown`, excluding `NATIVE_ONLY`). Running in a browser is checked by hand in Chrome only.

## Open questions

Each of these is for the architect to settle when the named spec merges, or for the owner. None blocks this PR.

1. **Sibling edges in the fan-out.** The plan assumes no edge among `platform`, `render`, `physics`, `audio`, `assets` and `ui`. Likely pressure points: render needing asset handles, and ui drawing through render and reading input from platform. If #107, #109 or #111 needs such an edge, intake 2 declares it and that part of the fan-out is layered (the depended-on crate first, then its dependents). The plan still holds. Only the "six at once" figure drops.
2. **MCP tool list from the live registry** (Stage 3). #110 should make the MCP crate build its tools from the registry the running game exposes. If #110 uses a compile-time catalog in `exitium_command`, the editor and MCP lanes serialize through that crate. Also: the crate that hosts the server, and whether `exitium_command` builds for wasm with the server behind a cfg, or goes into `NATIVE_ONLY` in intake 2.
3. **`docs-graph` cadence.** Running it after every merge in a lane serializes that lane's next issue. Proposal: run it at the end of each lane and at each stage boundary. This is a change to factory rules, so it is its own PR, not part of this one.
4. **ECS issues that mention `exitium_tasks`.** The ECS storage spec (#21) lists `par_iter_mut` on `exitium_tasks`. The MVP does not add that edge. When filing or amending #42 and #51, trim them to sequential iteration, and keep `par_iter_mut` with #48. Also confirm that the scheduler spec (#22) specifies a single-threaded executor with an API a parallel one can implement.
5. **Rehearsal game home and the engine reference.** Recommended: a separate repository created from `exitium new`, so it proves the external-consumer path as Glutton will. That needs the owner to create the repository, and a decision on how a generated project refers to the engine (a git dependency on this repository, since nothing is published). Alternative: `crates/exitium/examples/`.
6. **Budgets.** The runtime ceilings in "Performance" are proposals. The owner may change them. The crate specs refine them.
7. **Offline MCP tools** (docs and API query, schema dump) are not in the approved Stage 3 plan, which names live and ops tools. They stay with epic #6 unless #110 pulls them in cheaply.

## Scope cuts and where they return

Everything below is **deferred, not dropped.** The "Returns to" column names the issue or epic that owns it afterwards. "Path stays open" is what the MVP does so that design rule 5 (never block later goals) holds.

| Deferred | Returns to | Path stays open because |
|---|---|---|
| Luau (`exitium_script`, mlua, the wasm spike R5) | Epic #4 (Phase 2, `exitium_script`) | Commands are typed and serializable (the command bus is the shared Lua entry), and reflection is the only schema source. |
| Binary serial format, compression, migrations: #24, #56 to #60, and #36 (schema hash) | Epic #3 (Phase 1) | The text format is written through reflection with no format assumptions. Binary adds a second writer and reader over the same `TypeInfo`. |
| ECS world snapshot in both formats (#61) | Epic #3. The text snapshot for scenes is #111's. | Scenes use reflection, which is what #61 uses. |
| Parallel executor (#48) and `exitium_tasks` integration (#26 to #30, `par_iter_mut`) | Epic #3 | The single-threaded executor has an API that a parallel one can implement. The `exitium_ecs` to `exitium_tasks` edge is added with #48. |
| 1M-entity benchmarks (#52, #67), min-spec bench profile (#66) | Epic #3 | #51 baselines run on every ECS PR. |
| Tracy (#62), frame stats (#63), counting allocator (#64), executor spans (#65) | Epic #3 | `exitium_profiler` spans already exist. |
| Data assets and prefab variants (`exitium_data`, prefab variants in scene) | Epic #4 | Scenes use `AssetRef`-style ids, not paths. |
| Asset hot reload | Epic #4 (`exitium_assets`) | The asset id and registry sit in front of the loader. |
| Gamepad (input) | Epic #4 (`exitium_platform`) | Input goes through one event type, which new devices extend. |
| Tilemaps (`exitium_tilemap`), pixel-art pipeline, lighting, camera and game modes | Epic #10 (Phase 8) | Render v1 has no assumption of orthographic-only content. |
| Physics debug draw and editor tooling (beyond what #108 covers) | Epic #10 | Colliders are reflected components, so the inspector sees them. |
| UI data-binding performance (10k-node layout, counter rebinding), markup file format, theme and widget set beyond #109, immediate-mode layer | Epic #5 (Phase 3) | Widget and style properties are reflected. |
| Editor extras (Data Asset editor, UI WYSIWYG editor, play-in-editor, profiler panel) and offline MCP tools | Epic #6 (Phase 4) | The command bus is the one mutation path for every future editor. |
| The sim (`exitium_sim`) and saves (`exitium_save`) | Epic #7 (Phase 5). This is Glutton's critical path, so it is the first epic to resume after the MVP, if the owner agrees. | They need only core, ecs, reflect and serial. Neither is in a lane, so the MVP builds nothing that conflicts. |
| Networking (`exitium_net`, `exitium_server`, `exitium_backend`) | Epic #8 (Phase 6); scale-out in Epic #11 (Phase 9) | Replication marks components through reflection. |
| WebGL2 fallback, web threads, itch.io packaging, Steam, release workflows | Epic #8 | The web build already goes through wasm-bindgen. |
| Editor, MCP and CLI on Linux and macOS; Firefox and Safari | Epic #8 (platform breadth); file an issue under it when wanted | Native-only crates are split from runtime crates by `NATIVE_ONLY`. |
| The Glutton repository and the vertical slice | Epic #9 (Phase 7) | The rehearsal game exercises the same facade. |
| Destruction groundwork | Epic #11 | No cell or collider assumption is baked into physics or render. |

Open chores that are not part of the MVP (#84, #96 to #102, #105) stay on the normal backlog. The orchestrator places those that conflict with a lane into a gap.

## Implementation issues

This roadmap has no code. Its work items are the issues below. Each is PR-sized (about 600 changed lines or fewer), has a `## Footprint`, acceptance criteria and `Blocked by` lines, and goes into the Jam MVP milestone. After this spec merges, the architect files the missing ones with the `file-issue` skill. The two intake issues are the only ones allowed to edit more than one crate, since wiring is their whole purpose.

**Already filed.**
- Stage 0: #22, #23, #106, #107, #108, #109, #110, #111, #112.
- Stage 1: ECS #38 to #47, #49, #50, #51. Reflect #31, #32, #33, #37. Derive #34, #35. Serial #53, #54, #55.

**To file now (from this spec).**

1. **chore(deps): intake 1, dependencies and Stage 1 crate graph.** Blocked by #22, #23, #106, #107, #108, #109, #111.
   ```
   ## Footprint
   - Edits: crates/exitium_ecs/, crates/exitium_reflect/, crates/exitium_reflect_derive/, crates/exitium_serial/, crates/exitium_platform/, crates/exitium_render/, crates/exitium_physics/, crates/exitium_audio/, crates/exitium_assets/, crates/exitium_ui/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock, deny.toml
   - New crate edges: exitium_ecs -> exitium_core, exitium_ecs -> exitium_reflect, exitium_reflect -> exitium_core, exitium_reflect -> exitium_reflect_derive, exitium_serial -> exitium_core, exitium_serial -> exitium_reflect, and exitium_core only for platform, render, physics, audio, assets and ui
   ```
   Acceptance: every Stage 1 crates.io dependency is in `[workspace.dependencies]` with a one-line comment (licence, wasm, why not std); `cargo deny check` passes; `cargo xtask ci all` passes; every later Stage 1 issue can be done without editing `Cargo.toml` or `Cargo.lock`.
2. **chore(deps): intake 2, dependencies and the Stage 2 to 4 crate graph.** Blocked by #110, the Stage 1 exit, and every Stage 1 lane merged.
   ```
   ## Footprint
   - Edits: crates/exitium_app/, crates/exitium_platform/, crates/exitium_render/, crates/exitium_physics/, crates/exitium_audio/, crates/exitium_assets/, crates/exitium_ui/, crates/exitium_scene/, crates/exitium_command/, crates/exitium_editor/, crates/exitium_mcp/, crates/exitium_cli/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock, deny.toml, xtask/src/ci.rs
   - New crate edges: as in "Wiring after intake 2", from the merged specs
   ```
   Acceptance: creates the `exitium_command` stub and its `CLAUDE.md`; adds `exitium_mcp` to `NATIVE_ONLY`; no edge between `exitium_mcp` and `exitium_editor` in either direction; `cargo xtask ci all` passes.
3. **Per-spec implementation issues.** Each spec's own "Implementation issues" section lists them. Expected lanes: platform, render, physics, audio, assets and UI layout core (Stage 1); the `exitium_app` spine, Transform, six plugins, scene and the facade with the sample (Stage 2); command bus, server, editor lane, MCP lane and parity test (Stage 3); `exitium new`, both exports, the MCP export tool and the rehearsal (Stage 4). Their `Blocked by` lines follow the lane orders above.
4. **Housekeeping.** Add `Blocked by` and priorities so the Project board puts the reflect group above the ECS lane in Stage 1. Lift `needs-spec` as each spec merges.

**Dependency list (crates.io, for the intake PRs).** The versions, licences, wasm support and why-not-std argument for each belong to the owning spec, and the intake PR records them in the workspace manifest. The candidates named so far by the owner's decisions, PLAN.md and the epics:

| Crate | Used by | Stage | Native | wasm32 |
|---|---|---|---|---|
| `wgpu` (native backends and WebGPU) | `exitium_render` | 1 | yes | yes (WebGPU backend only) |
| `winit` | `exitium_platform` (and example dev-dependencies) | 1 | yes | yes |
| `taffy` | `exitium_ui` | 1 | yes | yes |
| `rapier2d` | `exitium_physics` | 1 | yes | yes |
| `kira` | `exitium_audio` | 1 | yes | yes |
| `syn`, `quote`, `proc-macro2` | `exitium_reflect_derive` | 1 | yes | n/a (proc-macro, native `NATIVE_ONLY`) |
| `wasm-bindgen` family | platform, render, audio, export | 1 and 4 | n/a | yes |
| `egui` family | `exitium_editor` | 3 (added in intake 2) | yes | not required |
| MCP, JSON and stdio-server crates | `exitium_mcp` | 3 (added in intake 2) | yes | not required |
| image, font and text-shaping crates | render, assets | 1 | yes | yes |

The ECS, reflect, serial and command crates add no external dependency. `glam` and `tracing` are already declared. Anything not in this table needs the owning spec to justify it, and goes into the intake PR before its stage starts.
