# Spec: Full MVP roadmap (stages, lanes, restored scope)

- Issue: #129
- Crates: none. This is a roadmap spec: it governs no crate and edits no code. It sequences work across every engine crate, `exitium_command`, `exitium_editor`, `exitium_mcp`, `exitium_cli`, the `exitium` facade and the `examples/` games.
- Status: draft. Supersedes [jam-mvp.md](jam-mvp.md) (#112). Owner-approved (2026-11-24): the contract below, and the owner decisions made in review of PR #130 (web threads, release workflows, the Luau fallback, the Steamworks SDK, the PR counts and budgets, the facade chain). The lane layouts are the architect's and the cross-spec rules C1 to C9 are orchestrator rulings, both reviewed in that PR. Anything still labelled **proposed** is a detail left to a named spec.

## Goal

Record the full MVP plan so that later agents work from the repo and not from chat history.

The full MVP is: an agent builds most kinds of 2D game through MCP plus Rust and Luau (top-down, sidescroller, isometric, pixel art, incremental, and online with saves and leaderboards), a human refines the result in the editor, and it exports to a Windows exe and to the web (wasm). The owner replaced the Jam MVP with this plan on 2026-11-24. The Jam scope cuts are gone, and Glutton is no longer the driver.

The plan has eight stages. Each stage has a lane layout (which crates are edited at the same time), a reason why that layout passes the `factory-run` conflict check, and an exit criterion that can be checked.

| Stage | Name | Exit (short form; the full criteria are in each stage section) |
|---|---|---|
| 0 | Specs, ADRs and the R5 spike | Intake 1's specs are merged (#24, SP2 to SP7, SP12 and the licence ADR AD1). The Luau-on-wasm spike and the other specs merge before the intake that needs them. |
| 1 | Foundations and standalone backends | Tasks, profiler, reflect, ECS (with the parallel executor), serial (text and binary) merged. Every backend has a Windows example and builds for wasm. |
| 2 | Runtime integration | The sample game runs on Windows and in Chrome, with data assets, prefab variants, gamepad and asset hot reload. |
| 3 | Editor, MCP, Luau and export (Windows tools) | An agent builds a scripted scene through MCP, a human edits it, and `exitium export` produces a Windows exe and a web build of it. |
| 4 | 2D breadth | Four sample scenes (pixel art, top-down, sidescroller, isometric) run with tilemaps, lighting and physics, with editor tooling. |
| 5 | Sim, saves, full UI and editor extras | A sample incremental game with prestige and offline gains saves, migrates and reloads. A heavy-numbers UI mockup holds its budget. |
| 6 | Platform and online | A sample game syncs saves to the backend, posts leaderboards, has offline gains validated by the server, runs on Steam and plays networked. |
| 7 | Rehearsal games in `examples/` | Six games exported to Windows and web, covering every game type and the online path. **This is the full MVP.** |

Size (accepted by the owner, 2026-11-24): about 450 PRs, about 200 of them on the critical path (198 summed in "Performance"), which is 17 to 29 days of factory time at 2 to 3.5 hours per serial PR. These are estimates (plus or minus 30%), not commitments. See "Performance" for what they assume. The Jam plan was about 80 PRs and 4 to 7 days.

## Non-goals

- Re-deciding anything in the contract. The owner decisions below are inputs.
- Designing a crate. Each crate's design belongs to its spec (index below). If this file and a crate spec disagree about a crate's design, the crate spec wins. If they disagree about order, lanes, hotspots or the cross-spec rules, this file wins.
- Replacing the epics #3 to #11. They stay as the long-term checklists. This file says which stage takes which epic's items (see "Restored scope").
- Editing PLAN.md, INTERVIEW.md or the epics (hotspots). A follow-up docs PR does that (see "Implementation issues", housekeeping).
- Optimising. Decision 4 says "not optimized yet": correct, tested and benchmarked to a baseline, with no tuning beyond the budgets below.

## Design

### Data layout

None. This is a roadmap with no data structures. The nearest thing is the crate graph, shown below and per stage.

### The contract (owner decisions, 2026-11-24)

**Goal.** An agent builds most kinds of 2D game through MCP plus Rust and Luau: top-down, sidescroller, isometric, pixel art, incremental, and online with saves and leaderboards. A human refines the result in a full standalone editor, and it exports to a Windows exe and to the web.

**Carried over from the Jam contract.**

- **Game UI** is a retained tree on taffy.
- **`exitium_mcp`** is a separate stdio process. It talks to the running editor over localhost. The editor is a feature of the game binary, so "the running editor" is the game process started with `--features editor`.
- **The editor** is a feature of the game binary (`--features editor`), built on egui.
- **The command bus** lives in `exitium_command`.
- **`exitium_app`** owns `App`, `Plugin`, the schedules (Startup, FixedUpdate at 60 Hz by default, Update, render extraction), `Time`, and `Transform2d` / `GlobalTransform2d` with hierarchy propagation.
- **Dependencies.** Every crates.io dependency arrives through an intake PR, so implementation issues never touch `Cargo.lock`.
- **Cross-spec rules C1 to C6** still apply, except where this file amends them. Each amendment is marked below with the reason.

**Changed by the owner.**

1. **Luau is back.** `exitium_script` is in the MVP. It runs on mlua, with bindings generated from reflection, hot reload, and every mutation going through the command bus. The R5 wasm spike comes first, because it is the biggest unknown.
2. **Web is WebGPU-only.** wgpu's WebGPU backend only. No WebGL2 fallback (unchanged).
3. **Editor, MCP and CLI stay Windows-only** (`NATIVE_ONLY`). Linux and macOS come after the MVP. Testing covers Windows and Chrome.
4. **2D breadth is in, but not optimized yet:**
   - tilemaps (`exitium_tilemap`, all three modes, with an editor tool);
   - the pixel-art pipeline (virtual resolution, integer scaling, palette swap, Aseprite import);
   - normal-mapped 2D lighting;
   - top-down, sidescroller and isometric camera modes with depth sorting.

   "Not optimized" means correct, tested and benchmarked to a baseline (design rule 4), with no tuning work beyond the stated budgets.
5. **Sim, saves, Steam, net, server and backend are in.** That covers `exitium_sim` (offline progress and prestige), `exitium_save` (versioned, migrations, zstd, tamper detection, a cloud provider trait), `exitium_steam`, `exitium_net` v1, `exitium_server` and `exitium_backend` (auth, cloud saves, leaderboards, server-side offline validation). It also requires the binary serial format, schema hash, migrations and compression.
6. **Rehearsal games live in the repo root `examples/`.** A real game is built in a new folder after the MVP is tested. Glutton is no longer the driver.

**Everything else in jam-mvp.md "Scope cuts" is restored:** `exitium_tasks` and the parallel executor, `par_iter_mut`, the 1M-entity benches, the min-spec profile, Tracy, frame stats, the counting allocator, executor spans, data assets and prefab variants, asset hot reload, gamepad, the full UI (markup, themes, binding performance, immediate mode), the editor extras (Data Asset editor, UI editor, profiler panel and the play-in-editor extensions beyond Stage 3's `play_*` tools), offline MCP tools, the parity test, and physics debug draw and tooling. "Restored scope" below maps each one to a stage.

**Still out:** web threads (they need nightly Rust), a WebGL2 fallback, editor/MCP/CLI on Linux and macOS, Firefox and Safari, network scale-out (Phase 9), destruction (epic #11) and the Glutton repo (epic #9).

**Owner decisions on review (2026-11-24, PR #130).** The owner answered the questions this file first raised. They are inputs, like the contract above.

1. **Web threads stay out of the MVP.** They need `-C target-feature=+atomics` and `build-std`, which is nightly, and CLAUDE.md pins stable Rust. The web build stays single-threaded (the tasks wasm fallback, #29).
2. **The Steam and itch release workflows are in the MVP.** The owner pushes the `.github/workflows/` files, because the factory App cannot. The plan is written so that no factory PR ever touches that directory: the CLI crate holds the drafts, and an issue assigned to the owner asks for the copy (Stage 6, item 7).
3. **If the Luau-on-wasm spike (SP1) fails, scripts are native-only** and web builds run without Luau. What that means for the plan, which is written to hold whether the spike passes or fails:
   - SP9 keeps a cfg-gated path. `exitium_script` always builds for `wasm32-unknown-unknown`: with the real VM if the spike passed, with a no-op stub if not. It is therefore never in `NATIVE_ONLY`, and every runtime crate keeps its wasm build.
   - Rust gameplay must be enough to run every web-exported example. An example's Luau is an extra layer for behaviour that may be missing on the web, and the exit criteria name which scripted parts are native-only (Stage 3 exit, Stage 7).
   - AD2 records the outcome either way, so nothing waits for the owner.
4. **The proprietary Steamworks SDK is accepted.** SP15's ADR still records how: an optional feature, native-only, the redistribution terms, kept out of the wasm and default builds.
5. **The PR counts and the sim, net and backend budgets are accepted.** So are the elapsed-time estimate and every other figure in "Performance"; crate specs may only tighten them.
6. **The facade chain of about 45 serial PRs is accepted.** Glue stays in the facade, one tail per stage. Glue crates are not adopted, because they would break C1.

### Cross-spec rules

Jam's C1 to C6 are restated here so one file is complete. Other specs cite them by number, and the numbers do not change. A spec may refine a rule's detail, but not break it.

- **C1 (amended). No edges between leaf crates, except a closed list.**
  - *Foundation crates:* `exitium_core`, `exitium_profiler`, `exitium_tasks`, `exitium_reflect` (with its derive), `exitium_ecs` (with its derive), `exitium_serial`, `exitium_app`, `exitium_command`. Anything may depend on them, within the graph below.
  - *Leaf crates:* `exitium_platform`, `exitium_render`, `exitium_physics`, `exitium_audio`, `exitium_assets`, `exitium_ui`, `exitium_scene`, `exitium_data`, `exitium_script`, `exitium_tilemap`, `exitium_sim`, `exitium_save`, `exitium_steam`, `exitium_net`. No edge between any two leaf crates, in either direction, normal or dev, except the four below.
  - *Allowed leaf edges (the whole list):* **E1** `exitium_scene -> exitium_data` (**proposed**: prefab variants and data-asset variants share one "base plus overrides" engine; SP10 may remove it by placing that engine in a foundation crate). **E2** `exitium_steam -> exitium_save` (Steam cloud implements the cloud provider trait). **E3** `exitium_backend -> exitium_sim` and `exitium_backend -> exitium_save` (server-side offline validation, save verification). **E4** `exitium_server -> exitium_net` (the server is a layer on net).
  - Cross-crate glue lives in the facade `crates/exitium/` (UI draw data and physics debug draw into the renderer, tilemap into render and physics, scripts and data into the asset server, `DefaultPlugins`). The facade is a hotspot, so its issues list `crates/exitium/` under `Hotspots`, never under `Edits`.
  - **Commands register through `exitium_command`.** Every crate with commands (`exitium_tilemap`, `exitium_physics`, `exitium_ui`, `exitium_sim`, `exitium_save`, `exitium_net`, `exitium_steam`, `exitium_server` and the backend client) gets an `exitium_command` edge under its `plugin` feature. That is a leaf-to-foundation edge, so C1 allows it, and the editor needs no edge to see these commands: it reads the registry at run time (C4). Intake 3 adds the edges for Stages 4 to 6. `exitium_command` is frozen after Stage 3: a later change is filed narrowly and runs alone at a lane boundary, like `exitium_core`.
  - **Reason for the amendment:** Jam's list had seven crates. The full MVP has fourteen leaf crates, and the four edges above are the only ones that avoid duplicating real code (a second override engine, a second save verifier, a second sim for validation). Every other pairing still goes through the facade.
- **C2 (kept). The `editor` feature lives in the facade.** `exitium/editor` enables `exitium_editor`.
- **C3 (kept, extended).** `exitium_editor` may depend on any runtime crate and on `exitium_command`, but gains an edge only when a stage needs it. Intake 2 gives it `command`, `scene`, `assets`, `render`, `platform`, `serial` and `app`. Intake 3 adds `exitium_tilemap` (Stage 4) and the frozen crates `exitium_data` and `exitium_profiler` (used in Stage 5; an edge to a frozen crate costs no parallelism). The edge to `exitium_ui` is **not** in an intake: an edge PR adds it in Stage 5, after the UI lane (see C5), so the UI lane can overlap Stage 4. **SP8 overrides** the line in editor-mcp-mvp.md's wiring section that says the editor has no edge to `exitium_physics`, `exitium_ui` or `exitium_audio`: it is replaced by the list above. Physics and audio stay edge-free: physics debug draw reaches the viewport through the facade glue and reflected resources, and SP7 and SP8 may add an edge PR if they decide otherwise. `exitium_mcp` depends only on `exitium_command`: never on the editor, the facade or a plugin crate, and no dev-dependency on them. The live-control server lives in `exitium_editor`. **Where tests live.** A test that needs only crates the editor depends on lives in `exitium_editor`'s tests and drives the `exitium_mcp` binary as a subprocess: the Stage 3 headless end-to-end test, and the Stage 3 parity test over those crates' commands. A test that needs any other crate is a **facade test**, because the facade depends on every runtime crate: the scripted-scene test needs `exitium_script`, and the parity test beyond Stage 3 needs sim, save, net, steam, server, backend, ui, physics and audio. A facade test also starts the `exitium_mcp` binary as a subprocess, so the facade gains no edge to `exitium_mcp`. Parity grows in the facade-hosted test, as part of each stage's glue PRs.
- **C4 (kept, extended). Reflected surface.** Each per-crate spec lists the reflected surface it exposes (components, resources, commands). The editor panels, the MCP tools **and the Luau bindings** are built generically from that surface. No crate hand-writes a tool, a panel or a binding.
- **C5 (kept). Intake rule.** The intake PRs add the union of the "wiring" sections of the merged specs, plus their crates.io dependencies. This file names the edges known today and is not the complete list. An edge that breaks C1 or C3 is a spec error, not intake input. **Edge PRs.** An edge that would stop two live lanes running together (R3) is not put in an intake. It is added by an *edge PR*: one crate, `Hotspots: Cargo.toml, Cargo.lock`, `Blocked by` the lane it would conflict with. An edge PR is an ordinary issue under R1 to R4 (lane issues touch no hotspot, so it can run beside lanes it does not conflict with). This plan has one: `exitium_editor -> exitium_ui` in Stage 5.
- **C6 (amended). Intake blocks the stage.** There are **three** intakes, not two (see "Intake PRs"). Every Stage 1 lane issue is `Blocked by` intake 1, every Stage 2 and 3 lane issue by intake 2, and every Stage 4 to 7 lane issue by intake 3. The edge PR is not an intake and does not drain the factory. **Reason:** a fourth boundary would drain the factory once more for little gain, since every spec is merged long before the stage that needs it.
- **C7 (new). ECS and serial are siblings.** `exitium_serial` has no edge to `exitium_ecs`, and `exitium_ecs` has none to `exitium_serial`. **Reason:** such an edge would stop the two longest Stage 1 lanes running side by side (R3). Consequences: issue #61 (ECS world snapshot) cannot live in `exitium_serial`, so it splits. Its **text half** is already the scene spec's (SC2 to SC4 in `exitium_scene`, Stage 2, through `TextRefs`; see serial-text-format.md, "Downstream issue adjustments") and needs no new work. Its **binary half** moves to `exitium_save`, which depends on ecs, reflect and serial. `exitium_save` has no edge to `exitium_scene` (C1), so it cannot reuse the scene text path. #67 (the 1M-entity round trip) therefore becomes a **facade test and bench** in Stage 5: the facade depends on ecs, tasks, profiler, serial, scene and save, so one test can run the text path through scene, the binary path through save, the parallel executor and the frame stats together. It holds the facade hotspot and runs alone.
- **C8 (new). New crates are created at an intake.** A new crate (`exitium_ecs_derive`, `exitium_command`, the example crates) is a Cargo workspace member and a `Cargo.lock` entry, so it is hotspot work. The intake that wires it creates it, with its `CLAUDE.md`, its `ENGINE_CRATES` or `NOT_DERIVE_USERS` entry in `exitium_reflect_derive` (the `crate_paths.rs` drift test fails otherwise) and its `NATIVE_ONLY` or `FEATURE_CHECKS` entry in `xtask`. Intakes therefore list `crates/exitium_reflect_derive/` under `Edits`.
- **C9 (new). Intake needs specs, not the other way round.** An intake is `Blocked by` the specs whose crates it wires. A spec never waits on an intake. An ADR is an issue with `Hotspots: docs/decisions/` and `Edits: none` (the ADR file is not a spec file), or a spec issue that also writes one. R2 allows at most one issue holding `docs/decisions/` at a time, so the factory serialises them without any `Blocked by` line between them; a `Blocked by` is added only for a real dependency. An ADR never rides in an intake PR: an exception an intake needs (a `deny.toml` licence entry) is recorded by its own ADR PR first, and the intake is `Blocked by` that PR, so no intake holds `docs/decisions/`. Long-running work (the spike) is split from its ADR so it never holds `docs/decisions/` for days.

### Crate graph

"A -> B" means A depends on B. This is the graph after all three intakes. Each intake adds its part (see the stages). The union of the merged specs' wiring sections is authoritative (C5).

```
Foundation
exitium_core             -> glam
exitium_profiler         -> (none)           (#65 later adds `exitium_tasks -> exitium_profiler`)
exitium_tasks            -> core
exitium_reflect_derive   -> (none)
exitium_reflect          -> core, reflect_derive
exitium_ecs_derive       -> (none)
exitium_ecs              -> core, reflect, profiler, ecs_derive, tasks    (tasks: intake 1)
exitium_serial           -> core, reflect                                 (no ecs: C7)
exitium_app              -> core, ecs, reflect
exitium_command          -> core, ecs, reflect, serial

Leaf crates (Stage 1 style standalone use needs core only; a `plugin` feature adds ecs, app, reflect)
platform, render, audio  -> core | plugin: ecs, app, reflect | dev: serial (render, audio)
physics, tilemap         -> core | plugin: ecs, app, reflect, command
ui                       -> core, taffy | plugin: ecs, app, reflect, command | dev: serial
assets                   -> core | manifest: reflect, serial | plugin: ecs, app
data                     -> core, reflect, serial        (SP10: no `app` edge if it can be avoided)
scene                    -> core, ecs, reflect, serial, data      (E1)
script                   -> core, ecs, reflect, app, command
sim, save                -> core, ecs, reflect, serial | plugin: app, command
net                      -> core, ecs, reflect, serial, app | plugin: command
steam                    -> core, ecs, app, command, save         (E2)

Service crates
server                   -> core, ecs, app, command, net          (E4)
backend                  -> core, reflect, serial, sim, save      (E3)
                            | client half (SP17: same crate or its own): ecs, app, command

Native-only tools
editor                   -> command, scene, assets, render, platform, serial, app (intake 2, Stage 3)
                            + tilemap (intake 3, Stage 4), data, profiler (intake 3, frozen, used in Stage 5)
                            + ui (an edge PR in Stage 5, after the UI lane: C5)
                            no edge to physics or audio (C3)
mcp                      -> command
cli                      -> serial
exitium (facade)         -> every runtime crate; editor optional (C2)
examples/*               -> exitium only
```

### How the conflict check applies

`factory-run` step 3 lets two issues run at once only if all four rules hold. This file refers to them as R1 to R4:

- **R1, disjoint edits.** No `Edits` entry (plus `Governs`, for spec issues) of one issue equals or contains one of the other. One crate has at most one issue in flight.
- **R2, one hotspot.** At most one in-flight item touches any hotspot: root `Cargo.toml`, `Cargo.lock`, `crates/exitium/`, `xtask/`, `deny.toml`, `docs/decisions/`, `examples/`, root files, and any path outside `crates/` and `docs/specs/`. Any change to a dependency table rewrites `Cargo.lock`.
- **R3, no crate dependency.** Neither issue edits a crate that depends, directly or transitively, on a crate the other edits, using the graph after both land. Optional dependencies and dev-dependencies count.
- **R4, no issue dependency.** Neither is blocked by the other, and neither needs an API the other is adding.

The three consequences in jam-mvp.md still hold: crate edges are what limit parallelism, intake PRs hold the hotspots so lanes do not, and intake PRs run alone at a stage boundary. Two more matter for this plan:

1. **The facade is a serial tail of every stage.** It depends on every crate, so each facade issue runs alone (R2 and R3). With six stages adding glue, the facade chain is about 45 PRs in total. The owner accepted that cost (2026-11-24): glue stays in the facade, one tail per stage, and glue crates are not adopted because they would break C1.
2. **Stage order is the order of exit checks, not a wall.** Inside one intake's span, a lane starts as soon as it passes the check. For example the Stage 5 UI lane may start beside Stage 4 lanes, if it ends before Stage 4's facade glue starts (the facade depends on `exitium_ui`). The estimates in "Performance" assume this overlap where this file says so, and no other.

### Spec index

Every MVP spec, by issue. A spec issue that edits an existing spec amends it in place, and its Footprint names that file. The "Needed by" column says which intake or stage waits for it.

**Existing specs (all merged).**

| Issue | Spec | Governs | Used by |
|---|---|---|---|
| #19 | [Job system](tasks-job-system.md) | `exitium_tasks` | Stage 1 tasks lane (restored) |
| #20 | [Reflection model](reflect-model.md) | `exitium_reflect`, `exitium_reflect_derive` | Stage 1 |
| #21 | [ECS storage and queries](ecs-storage-queries.md) | `exitium_ecs` | Stage 1 |
| #22 | [ECS scheduler](ecs-scheduler.md) | `exitium_ecs` | Stage 1 |
| #23 | [Text serialization format](serial-text-format.md) | `exitium_serial` | Stage 1 |
| #106 | [App loop, plugins, transforms, platform](app-platform.md) | `exitium_app`, `exitium_platform` | Stages 1, 2 |
| #107 | [Render v1](render-v1.md) | `exitium_render` | Stages 1, 2 |
| #108 | [2D physics](physics-2d.md) | `exitium_physics` | Stages 1, 2, 3 |
| #109 | [Game UI MVP](ui-mvp.md) | `exitium_ui` | Stages 1, 2, 3 |
| #110 | [Command bus, editor, MCP](editor-mcp-mvp.md) | `exitium_command`, `exitium_editor`, `exitium_mcp` | Stage 3 |
| #111 | [Assets, scenes, audio, export](assets-scene-export.md) | `exitium_assets`, `exitium_scene`, `exitium_audio`, `exitium_cli` | Stages 1, 2, 3 |
| #112 | [Jam MVP roadmap](jam-mvp.md) | none | Superseded by this file |
| #129 | This file | none | All stages |

**Amendments to existing specs (to file).** Each one changes only the named spec file, and also removes that spec's "post-MVP" and "cut" wording for the restored items. SP3 to SP7 also carry the spec halves of the editor spec's amendment list (editor-mcp-mvp.md, "Downstream adjustments"), so no second issue edits the same file: AM1 is in SP5, AM2 and AM3 in SP3, AM5 in SP7, AM6 in SP6 and AM9 in SP4. **AM8 is dropped:** it edits jam-mvp.md, which this file supersedes, and this file replaces its Stage 3 counts. AM4, AM7 and the code halves of AM1 to AM3 are code issues, scheduled in the Stage 3 pre-editor lane. None of AM1 to AM9 has been filed.

| ID | Amends | What it settles | Governs | Needed by |
|---|---|---|---|---|
| SP2 | `ecs-scheduler.md` and `ecs-storage-queries.md` | **ECS executor.** The parallel executor on `exitium_tasks` (#48), `par_iter_mut`, the `exitium_ecs -> exitium_tasks` edge, executor spans (#65), the 1M-entity benches (#52), the min-spec profile (#66), how script systems run (exclusive, `NonSend`). Also tasks edges (`tasks -> profiler`) and Tracy, frame stats and the counting allocator (#62 to #64) at the profiler level. | `exitium_ecs`, `exitium_tasks`, `exitium_profiler` | Intake 1 |
| SP3 | `app-platform.md` | **Platform gamepad** (one more `InputEvent` source, native and web, mapping, deadzones, the gamepad plugin systems) **and the editor hooks** (AM2, AM3): `raw_event` for every `WindowEvent`, the hook signature, a wake handle, the reflected `SimulationMode`, and the supersession of the `app_*` and `input_inject` presets. | `exitium_platform`, `exitium_app` | Intake 1 |
| SP4 | `assets-scene-export.md` | **Assets hot reload** (file watch, native only, and web behaviour, reload events, how data assets and shaders reload, the plugin systems) **and the editor and MCP surface** (AM9): the `audio_*` tools are not built, no Export menu, the "no undo" declaration. It also **moves the `triple_buffer` ADR out of intake 1** into its own PR (AD1); intake 1 keeps only the `deny.toml` entry. | `exitium_assets`, `exitium_audio`, `exitium_scene`, `exitium_cli` | Intake 1 |
| SP5 | `render-v1.md` | **Render v2 hooks** (what render v1 must leave open so Stage 4 adds no rework: the depth sort key and its width, a normal-map texture slot, ordered extra passes for lights, an offscreen virtual-resolution target, a user material slot) **and the surface overlay** (AM1: `SurfaceOverlay`, `set_overlay`, `draw_cameras_to_surface`). It is written against render-v1.md's text. The Jam amendment's blocker on I16 and I17 applies to the code PR, not to the spec. | `exitium_render` | Intake 1 |
| SP6 | `ui-mvp.md` | **UI full** (the whole design is decided now, because binding changes the arena shape): markup file format and hot reload, themes, the rest of the widget list (epic #5), data binding that updates only on change, the immediate-mode layer, the 10k-node and counter-rebinding benches. **And the command bus** (AM6): U13 registers its commands and queries. | `exitium_ui` | Intake 1 |
| SP7 | `physics-2d.md` | **Physics debug draw and tooling** (debug draw as data for the facade glue and as an MCP query, character controller, tile-collider merging, joints decision) **and the command bus** (AM5): `SetPhysicsConfig` and `SetDebugDraw` are not built, P15 and P16 are re-scoped. | `exitium_physics` | Intake 1 (physics has Stage 1 to 4 lanes; P15 and P16 need AM5 in Stage 3) |
| SP8 | `editor-mcp-mvp.md` | **Editor extras:** the Data Asset editor, the UI editor, the tilemap tool, light and camera gizmos, the physics debug toggle, the profiler panel, the play-in-editor contract that the script, sim, save and backend plugins implement, and its extensions (see Stage 5), the offline MCP tools (docs and API query, schema dump), the parity-test growth rule, and the override of the wiring line named in C3. The export and Steam tools are **not** here: SP18 and SP15 own those surfaces (C4). | `exitium_editor`, `exitium_mcp`, `exitium_command` | Intake 2 |

**New specs and ADRs (to write).**

| ID | File | Governs | Needed by |
|---|---|---|---|
| #24 | `serial-binary-format.md` (binary format, schema hash #36, migrations, compression container) | `exitium_serial` | Intake 1 |
| SP1 | `luau-wasm-spike.md` (the **R5 spike** report, with measurements; no hotspot) | none | AD2 |
| AD1 | ADR: the `triple_buffer` MPL-2.0 licence exception (the owner's decision in assets-scene-export.md, recorded as its own PR) | none | Intake 1 |
| AD2 | ADR: Luau on wasm, the outcome of SP1: the route that works, or native-only scripts with a cfg-gated stub (owner decision 3) | none | Intake 2, SP9 |
| SP9 | `luau-scripting.md` (mlua, bindings from reflection, tiers, hot reload, command-bus API, wasm per AD2 with a cfg-gated stub path, its part in play-in-editor per SP8) | `exitium_script` | Intake 2 |
| SP10 | `data-assets-prefabs.md` (data assets, variants, prefab variants in scenes) | `exitium_data`, `exitium_scene` | Intake 2 |
| SP11 | `tilemap.md` (all three modes, autotile, chunks, colliders, editor tool surface) | `exitium_tilemap` | SP8, so intake 2 |
| SP12 | `render-2d-breadth.md` (pixel-art pipeline, palette swap, Aseprite import, normal-mapped lighting, camera modes, depth sorting) | `exitium_render`, `exitium_assets` | Intake 1 (both crates have Stage 1 lanes) |
| SP13 | `sim.md` (fixed high-rate ticks, producers, modifiers, offline progress, prestige, automation, its part in play-in-editor per SP8) | `exitium_sim` | Intake 3 |
| SP14 | `saves.md` (versioned saves, migrations, compression, tamper detection, autosave, cloud provider trait, the binary world snapshot per C7, its part in play-in-editor per SP8) | `exitium_save` | Intake 3 |
| SP15 | `steam.md` (Steamworks init, achievements, cloud provider, overlay, depot layout; the ADR records how the proprietary SDK is supplied: an optional native-only feature, the redistribution terms, kept out of the wasm and default builds) | `exitium_steam` | Intake 3 |
| SP16 | `net-server.md` (transport for native and web, authoritative client and server, replication through reflection, the headless runtime; transport ADR) | `exitium_net`, `exitium_server` | Intake 3 |
| SP17 | `backend.md` (auth, cloud saves, leaderboards, offline validation, hosting adapters, its part in play-in-editor per SP8; topology ADR) | `exitium_backend` | Intake 3 |
| SP18 | `export-examples.md` (the `examples/` layout, exporting an in-workspace project, itch packaging, the six games' acceptance criteria) | `exitium_cli` | Intake 3 |

Spec dependencies (R4): AD2 after SP1. SP9 after AD2 and SP8. SP10 after SP4. SP11 after SP5. SP12 after SP4 and SP5 (same crates, R1). SP13 after SP8. SP14 after #24 and SP8. SP16 after #24. SP15 after SP14 and SP16. SP17 after SP8, SP13, SP14 and SP15. SP8 after SP6, SP7, SP10, SP11 and SP12. SP18 after SP15, SP17 and the last Stage 3 CLI lane issue. SP8 owns the play-in-editor contract, so SP9, SP13, SP14 and SP17 are ordered after it, and each carries one acceptance bullet (below). The ADR holders (AD1, AD2, SP15, SP16, SP17) need no `Blocked by` among themselves: R2 lets only one hold `docs/decisions/` at a time (C9).

## Restored scope

The restore list: every row of jam-mvp.md "Scope cuts", with where it returns. "Spec" is the owning spec or amendment. Issue numbers are the existing issues.

| Jam cut | Returns in | Spec | Notes |
|---|---|---|---|
| Luau, `exitium_script`, mlua, R5 spike | Spike in Stage 0, scripting in Stage 3 | SP1, SP9 | The spike is the first item started. If it fails, scripts are native-only and the crate builds for wasm as a stub (owner decision 3). |
| Binary format, compression, migrations (#24, #56 to #60) | Stage 1 serial lane | #24 | |
| Stable schema hash (#36) | Stage 1 reflect lane | #24 | Binary format needs it. |
| ECS world snapshot (#61) | Text half: scene SC2 to SC4, Stage 2. Binary half: Stage 5, in `exitium_save` | SP14 | Split by C7. Retitle when filing. |
| Parallel executor (#48), `par_iter_mut` | Stage 1, ECS lane tail | SP2 | After the tasks lane. |
| `exitium_tasks` (#26 to #30) | Stage 1, own lane | SP2 | Edge `ecs -> tasks` in intake 1. |
| 1M-entity benches (#52, #67) | #52 Stage 1; #67 Stage 5, a facade test | SP2, SP14 | #67 needs ECS, tasks, profiler, scene and save together, so only the facade can host it (C7). |
| Min-spec bench profile (#66), Tracy (#62) | Stage 1 | SP2 | |
| Frame stats (#63), counting allocator (#64), executor spans (#65) | Stage 1 (#65 at the tail) | SP2 | #64 is a designated `unsafe` module with Miri tests. |
| Data assets and prefab variants | Stage 2 | SP10 | |
| Asset hot reload | Stages 1 and 2 | SP4 | |
| Gamepad | Stages 1 and 2 | SP3 | |
| Tilemaps | Stage 4 | SP11 | |
| Pixel art, lighting, camera and game modes | Stage 4 | SP5, SP12 | |
| Physics debug draw and tooling | Stage 4 | SP7 | Debug draw data (P7, P14) already sit in the physics spec. |
| UI binding performance, markup, themes, widgets, immediate mode | Stage 5 | SP6 | |
| Editor extras, offline MCP tools | Stage 5 (`play_*` already ships in Stage 3) | SP8 | |
| Parity test | Stage 3 (XT2), grown every stage | SP8 | |
| Networking, server, backend | Stage 6 | SP16, SP17 | Scale-out stays out (epic #11). |
| Sim | Stage 5 | SP13 | |
| Saves | Stage 5 | SP14 | |
| Steam | Stage 6 | SP15 | |
| itch.io packaging, release workflows | Stage 6 | SP18 | In the MVP (owner decision 2). Drafted inside `exitium_cli`; the owner pushes the `.github/workflows/` files. |
| Web threads | Out of the MVP (owner decision 1) | none | They need nightly Rust. |

**Still out:** web threads (they need nightly Rust; owner decision 1); WebGL2 fallback; editor, MCP and CLI on Linux and macOS; Firefox and Safari; network scale-out (epic #11, Phase 9); destruction (epic #11); the Glutton repo (epic #9). For the first two and Firefox/Safari the path stays open because `NATIVE_ONLY` already separates native-only crates. Open chores (#84, #96 to #102, #105, #125, #126) stay on the normal backlog and are placed into gaps.

**Epic mapping.** Epic #3 is Stage 1. Epic #4 is Stages 2 and 3, with the R5 spike in Stage 0. Epic #5 is Stages 1 and 5. Epic #6 is Stages 3 and 5. Epic #7 is Stage 5. Epic #8 is Stage 6. Epic #10 is Stage 4. Epic #9 (Glutton) and epic #11 stay out.

## Stages

### Intake PRs

There are three. Each runs alone at a stage boundary, holds the hotspots `Cargo.toml`, `Cargo.lock` and `deny.toml`, and adds everything its span needs, so lane issues never touch them.

| Intake | Span | Adds (known today; the union of the merged specs' wiring is authoritative, C5) | Blocked by |
|---|---|---|---|
| 1 | Stage 1 | Crates.io dependencies of Stage 1: `wgpu`, `winit`, `taffy`, `rapier2d`, `kira`, `png`, `hound`, `lewton`, `ttf-parser`, `glyphon`, `bytemuck`, `web-time`, `gilrs`, the file-watch choice, `tracy-client`, and the compression and zero-copy crates that #24 names for the binary container. The `deny.toml` entry for `triple_buffer` (its ADR is AD1, a separate PR). The new crate `exitium_ecs_derive` (C8). The edges in the Stage 1 graph, **`exitium_ecs -> exitium_tasks`** and `exitium_ecs -> exitium_profiler` among them. The `profiling`, `tracy` and allocator features of `exitium_profiler`. | #24, SP2 to SP7, SP12, AD1, and the Jam-era Stage 1 specs (all merged) |
| 2 | Stages 2 and 3 | Dependencies of Stages 2 and 3 (egui family, rmcp, tokio, serde, `mlua` per AD2, the wasm-bindgen additions, zip). The new crate `exitium_command` (C8). `exitium_mcp` in `NATIVE_ONLY`. The plugin-feature edges, `scene -> data` (E1), the script, command, editor and mcp edges, and the facade edges. | Intake 1, every Stage 1 lane issue, #110, SP1, AD2, SP8 to SP11 |
| 3 | Stages 4 to 7 | Dependencies of Stages 4 to 7 (an Aseprite parser, compression, authenticated encryption, Steam bindings, a net transport, the server and backend stack). The edges for `tilemap`, `sim`, `save`, `steam`, `net`, `server` and `backend`, including their `exitium_command` edges (C1), and `exitium_editor -> exitium_tilemap, exitium_data, exitium_profiler` (C3; the `exitium_ui` edge is a separate edge PR, C5). The six example crates and the `examples/*` workspace members (C8). Any `NATIVE_ONLY` or `FEATURE_CHECKS` entries. The `deny.toml` entries for exceptions whose ADR is already on `main` (the intake holds no `docs/decisions/`, C9). | Intake 2, every Stage 2 and 3 lane issue, SP13 to SP18, and the ADR PR of each exception it records |

### Stage 0: specs, ADRs and the R5 spike

**Work.** Twenty-one issues, plus this file (#129): the amendments SP2 to SP8, the new specs #24, SP1 (the spike report) and SP9 to SP18, and two ADRs (AD1, the `triple_buffer` licence, and AD2, Luau on wasm). Jam's nine Stage 0 issues are all merged. The editor spec's amendment list (AM1 to AM9) is folded in: see "Spec index".

**When each one runs.** A spec that governs a crate counts as editing it (R1, R3), so no spec may run while a lane issue is in flight in a crate it governs. The windows below are fixed by group, so the picker never has to guess:

| Group | Issues (and what they govern) | Window | Why it is safe |
|---|---|---|---|
| A | #24 (serial), SP2 (ecs, tasks, profiler), SP3 (platform, app), SP4 (assets, audio, scene, cli), SP5 (render), SP6 (ui), SP7 (physics), SP12 (render, assets), AD1 (none) | Merged **before intake 1**. Intake 1 is `Blocked by` all of them. | These crates have lanes in Stages 1 to 4, and intake 1 edits most of them. If a spec started after intake 1, R1 would hold it back until a lane gap. Doing them first means the overlap never happens. |
| B | SP1 (none), AD2 (none), SP9 (script), SP10 (data, scene), SP11 (tilemap), SP8 (editor, mcp, command) | Run **beside Stage 1**. All merged **before intake 2**. SP8 follows SP6, SP7, SP10, SP11 and SP12, and SP9 follows SP8 and AD2. | No lane touches these crates until Stage 2 (data, scene), Stage 3 (script, editor, mcp, command) or Stage 4 (tilemap), so R1 holds. In the Stage 1 graph none of them depends on a crate a Stage 1 lane edits, so R3 holds. |
| C | SP13 (sim), SP14 (save), SP15 (steam), SP16 (net, server), SP17 (backend) | Before **intake 3**, beside Stages 1 to 3. SP13, SP14 and SP17 start after SP8 (group B). | No lane in these crates before Stage 5 or 6, and they are in no graph until intake 3. |
| D | SP18 (cli) | After the **last Stage 3 CLI lane issue**, SP15 and SP17. Before intake 3. | The CLI has a Stage 3 lane, so SP18 waits for it to end. The CLI is not edited again until Stage 6. |

SP5 then SP12 is the only chain in front of intake 1: **two serial PRs** open Stage 1. Everything else in groups B to D runs off the critical path.

**Order inside the windows.**
- **Wave 1 (eight at once, no blockers):** SP1 (started first, it runs for days), #24, SP2, SP3, SP4, SP5, SP6, SP7.
- **Wave 2 (as slots free):** AD1 (short, goes first), SP12 (after SP4, SP5), SP10 (after SP4), SP11 (after SP5), SP16 (after #24), AD2 (when SP1 merges).
- **Wave 3:** SP8 (after SP6, SP7, SP10, SP11, SP12), then SP9 (after SP8, AD2), SP13 and SP14 (after SP8; SP14 also after #24), SP15 (after SP14, SP16), SP17 (after SP8, SP13, SP14, SP15).
- **Later:** SP18 (group D).

**The spike (SP1 and AD2).** Luau is C++, and `wasm32-unknown-unknown` has no C or C++ standard library, so `mlua` with a vendored Luau may not build for the web. The spike's code is throwaway, kept outside the repo. SP1's PR is a report, `docs/specs/luau-wasm-spike.md`, with measurements:

1. Does a documented recipe build `mlua` (Luau) for `wasm32-unknown-unknown` on stable Rust?
2. If not, which of these works: a wasm clang target with a minimal libc and libc++ subset; a separate Luau wasm module bridged from JS; another route.
3. For the winner: gzip size, a hello-world script running in Chrome, and the cost of one Lua to Rust call.

SP1 edits one spec file and holds no hotspot, so it can run for days without blocking any ADR. AD2 is the short ADR that records the route. If no route fits the budgets, the owner has already decided (decision 3): scripts are **native-only** and web builds run without Luau. AD2 records the outcome either way, so nothing waits for the owner. Nothing that depends on Luau is filed before AD2 merges.

**The plan holds whether the spike passes or fails.** Only these things differ:
- `exitium_script` builds for `wasm32-unknown-unknown` in both cases: SP9 puts the VM behind a cfg, and the other arm is a no-op stub plugin that logs once that scripts are native-only. The crate is never in `NATIVE_ONLY`, the workspace wasm build still covers it, and every runtime crate keeps its wasm build.
- Every web-exported example runs on its Rust gameplay alone (Stage 7). Luau in an example is an extra layer that may be absent on the web.
- The web size budget is 13 MB with Luau, 10 MB without (see "Performance").
- AD2 lists which examples' scripted parts are native-only. Everything else (MCP script tools, hot reload, the command-bus API) is native either way.

**Why it passes.**
- R1: each issue edits one exact file under `docs/specs/` (SP2 edits two; SP1 its report; AD1 and AD2 edit no spec file, their ADR is a hotspot entry). `Governs` sets that can run at the same time are pairwise disjoint. Wave 1: none, serial, ecs+tasks+profiler, platform+app, assets+audio+scene+cli, render, ui, physics. Wave 2: render+assets (after SP4 and SP5 have merged), data+scene (after SP4), tilemap, net+server. Wave 3: editor+mcp+command first, then script, sim, save, steam and backend (each after SP8, and after its other blockers). Every pair that shares a crate (SP4 and SP10 on scene, SP4 and SP5 with SP12 on assets and render) is ordered by `Blocked by`.
- R2: AD1, AD2, SP15, SP16 and SP17 hold `docs/decisions/`. R2 itself keeps them to one at a time, so they need no `Blocked by` among themselves. No intake holds `docs/decisions/` (AD1 carries intake 1's ADR, and SP4 records the move; an ADR for an intake 3 exception is its own PR, C9), so no intake overlaps an ADR holder.
- R3: no `Cargo.toml` has an `exitium_` dependency today, and every spec issue declares `New crate edges: none`. After intake 1 lands, the graph has only the Stage 1 edges (ecs, serial, reflect, tasks, profiler). Groups B to D govern no crate in it and none depends on one, so they pass beside Stage 1 lane issues. Group A is merged before intake 1.
- R4: the only dependencies are the `Blocked by` lines under "Spec dependencies".

**Stage 0 is rolling.** Intake 1 needs group A only. The spike and group B run beside Stage 1. Intake 2 needs group B. Intake 3 needs group C and SP18.

**Exit.** Intake 1 may start when #24, SP2 to SP7, SP12 and AD1 have merged (nothing in Stage 1 waits for the spike). Intake 2 may start when SP1, AD2 and SP8 to SP11 have merged. Intake 3 may start when SP13 to SP18 have merged. The `needs-spec` labels are lifted from the dependent issues as each spec merges (#48, #54, #56 to #59, #61 and the issues each new spec files).

### Stage 1: foundations and standalone backends

**Work.** First intake 1, then the lanes:

| Lane | Crate(s) | Issues | PRs | Notes |
|---|---|---|---|---|
| Reflect group | `exitium_reflect`, `exitium_reflect_derive` | #31 to #33, #36, #37 (reflect); #34, #35 (derive) | 7 | Short. Runs first. |
| Tasks | `exitium_tasks` | #26 to #30 | 5 | Restored. Must finish before the ECS lane. |
| Profiler | `exitium_profiler` | field form of `profile_scope!` (ECS item 1), #62 (Tracy), #63 (frame stats), #64 (counting allocator) | 4 | Before ECS. #65 is later. |
| ECS | `exitium_ecs`, `exitium_ecs_derive` | #38 to #51, the scheduler items, #48, #52 | about 28 | **The critical path.** |
| Serial | `exitium_serial` | text S1 to S12 (#53 to #55 are three of them), binary #56 to #60 | about 17 | Binary needs #36. No world snapshot (C7). |
| Platform | `exitium_platform` | PL1 to PL5, gamepad | about 7 | |
| Render | `exitium_render` | I1 to I13 | 13 | Leaves the SP5 hooks open. |
| Physics | `exitium_physics` | P1 to P7 | 7 | |
| Audio | `exitium_audio` | AU1 to AU3 | 3 | |
| Assets | `exitium_assets` | A1 to A7, hot reload standalone | about 9 | |
| UI layout core | `exitium_ui` | U1 to U10 | 10 | Arena shaped per SP6. |
| Tail | `exitium_profiler`, `exitium_ecs`, `exitium_tasks` | #65 (executor and job spans) | 1 | Runs alone after the ECS lane. |
| xtask | `xtask/` | #66 (min-spec bench profile) | 1 | A hotspot item. Runs in a gap after the tasks lane. |

Every issue in the table is `Blocked by` intake 1 (C6). Each backend ends with a Windows example and a clean `cargo build --target wasm32-unknown-unknown`. An example opens its own window with a dev-dependency from intake 1, not `exitium_platform` (C1). The gamepad lane ends with a native and a web example. The assets lane ends with a hot-reload example.

**Known graph after intake 1** (the Stage 1 part of the graph above):

```
ecs -> core, reflect, profiler, ecs_derive, tasks      serial -> core, reflect
reflect -> core, reflect_derive                         tasks -> core
profiler, ecs_derive, reflect_derive -> (none)          platform, render, physics, audio, assets, ui -> core
```

**Why it passes.**
- R1: every lane is one crate directory (or one derive and its parent, ordered), so lanes are disjoint. Within a lane one issue runs at a time.
- R2: after intake 1 no lane issue changes a dependency table, and none holds `docs/decisions/`. Examples and benches live in the crate directory. The two hotspot items, #66 (`xtask/`) and #65 (a feature edge in `Cargo.toml`), run one at a time, never beside intake 1 or each other.
- R3, using the graph above:
  - `exitium_ecs` depends on `reflect`, `reflect_derive`, `profiler`, `ecs_derive` and `tasks`, so the ECS lane never runs beside any of them. The reflect group, tasks and profiler lanes are independent of each other, so they run **together in wave 1**.
  - `exitium_serial` depends on `reflect` and `reflect_derive` only, so it runs beside ECS in wave 2.
  - The six backends depend on core alone, so they run beside everything.
  - `exitium_tasks -> exitium_profiler` is **not** in intake 1. #65 adds it, at the tail, alone. Until then tasks and profiler are independent.
- R4: cross-lane needs are `Blocked by` lines: #34 blocks #50 and #53; #36 blocks the binary writer; #48 needs the tasks lane (#26 to #29); #52 needs #48; #65 needs #48.

**Schedule.** Wave 1 (eight in flight): the reflect group, tasks, profiler and five backends. Wave 2: ECS, serial and the six backends still running (eight in flight). The Project priority must put the reflect group, tasks and profiler above ECS, and ECS above serial. Tasks (5) and profiler (4) finish before the reflect group (7), so adding them to wave 1 costs nothing on the critical path.

Critical path: intake 1 (1), reflect group (7), ECS (28), #65 (1): about 37 serial PRs.

**Core is frozen.** No MVP issue edits `exitium_core`. A needed core change is filed narrowly and run alone at a lane boundary.

**Exit.**
- `cargo xtask ci all` is green on `main`, including the wasm build.
- ECS, reflect, serial, tasks and profiler are merged.
- The parallel tick of 1M entities (#52) meets its budget at 4 threads, and the min-spec profile (#66) runs.
- Text and binary formats round-trip reflected values with golden files. (The ECS **world** round trip is Stage 5, C7.)
- Every backend has a Windows example that runs, builds for wasm, and is recorded in its PR.
- No benchmark regression over 5% (#51 sets the ECS baselines).

### Stage 2: runtime integration

**Work.** First intake 2, then the spine and the data lane side by side, then a fan-out, then scene, then the facade:

1. **Intake 2** (see the table above).
2. **Spine: `exitium_app`** (A1 to A4: `App`, `Plugin`, schedules, `Time`, `Transform2d` with propagation, benches).
3. **Data lane: `exitium_data`** (about 6 PRs): data assets with prototype and variant inheritance, the registry, reflection, text round trip. SP10 keeps the crate free of an `exitium_app` edge if it can, so this lane does not wait for the spine.
4. **Fan-out: one plugin per crate** in `exitium_platform` (3: platform plugin, windowed runner, gamepad), `exitium_render` (4), `exitium_physics` (7: P8 to P14), `exitium_audio` (1), `exitium_assets` (4: manifest, plugin, edit API, hot reload) and `exitium_ui` (3: U11, U12, UA1). Command surfaces (P15, P16, U13 and the attributes) wait for Stage 3.
5. **Scene lane: `exitium_scene`** (about 8: SC1 to SC5 and three prefab-variant PRs), after the data lane (E1).
6. **Facade** (about 17, one serial chain): window and UI glue, asset feeding, scene loading, render, physics and data glue, gamepad glue, hot-reload glue, `DefaultPlugins`, the re-exports and the sample game.

**Known graph after intake 2** (new edges only):

```
app -> core, ecs, reflect                     data -> core, reflect, serial
platform, render, physics, audio, assets, ui -> core, ecs, app, reflect (plugin features)
scene -> core, ecs, reflect, serial, data     command -> core, ecs, reflect, serial
script -> core, ecs, reflect, app, command    physics, ui -> command (plugin features)
editor -> command, scene, assets, render, platform, serial, app     mcp -> command
exitium (facade) -> every runtime crate, command, editor (optional)
```

**Why it passes.**
- Intake 2 runs alone at the Stage 1 to Stage 2 boundary. It holds `Cargo.toml`, `Cargo.lock`, `deny.toml`, `xtask/` and `crates/exitium/`. All Stage 1 lane issues have merged by then.
- **Spine.** Every plugin crate depends on `exitium_app`, so R3 forbids any of them beside a spine issue. The spine is a serial chain in one crate (R1).
- **Data and scene.** Data depends on core, reflect and serial, all frozen, and has no edge to `app` or to the fan-out crates, so it runs beside the spine (R1, R3). Scene depends on data (E1), so scene waits for the data lane (R3, R4). Scene has no edge to the fan-out crates (C1), so it overlaps them.
- **Fan-out.** Six lanes, six crate directories (R1). No hotspot, since edges came in intake 2 (R2). Each depends on core, ecs and app, frozen by then, and on none of the others (C1), so all six run together within the cap of 8 (R3). Each needs only the merged spine (R4).
- **Facade.** `crates/exitium/` is a hotspot and depends on every crate, so each facade issue runs alone, after the fan-out, data and scene (R2, R3). Facade issues list `crates/exitium/` under `Hotspots` and `Edits: none`.

Critical path: intake 2 (1), the data lane (6) then scene (8) as the longer of the two paths, the facade (17): about 32 serial PRs.

**Exit.** The sample game (`DefaultPlugins`, a scene loaded from a text file, a data asset driving unit stats, a prefab variant, sprites, text, physics, UI, audio, gamepad input) runs on Windows and in Chrome via WebGPU. Editing a texture on disk shows in the running Windows build within the hot-reload budget. The web run uses a documented `wasm-bindgen` recipe stored with the sample.

### Stage 3: editor, MCP, Luau and export (Windows tools)

**Work.** The command spine and the pre-editor lane run side by side, then the editor and its neighbours, then the facade hook and the tests:

1. **Spine: the command bus and its wire types** (`exitium_command`, CB1 to CB12, about 12 PRs).
2. **Pre-editor lane** (8 PRs, beside the spine). These are the code PRs the editor waits for, from editor-mcp-mvp.md "Downstream adjustments". Their spec halves are already merged in SP3 and SP5:
   - `exitium_app`: `SimulationMode` in the frame driver (AM3, blocks ED6), then the read-only attribute PR (AM4). 2 PRs, first.
   - `exitium_platform`: `raw_event` for every `WindowEvent`, the hook signature and the wake handle (AM2, blocks ED8 and FH1), then its attribute PR. 2 PRs.
   - `exitium_render`: `SurfaceOverlay`, `set_overlay` and `draw_cameras_to_surface` (AM1, blocks ED8 and FH1), then its attribute PR. 2 PRs.
   - `exitium_assets` and `exitium_audio`: one attribute PR each. 2 PRs.
3. **Editor lane** (`exitium_editor`, ED1 to ED13, about 13): the live-control server, headless runner, scene and asset tools, play and pause (`play_start`, `play_stop` with snapshot restore, ED6), the egui shell, the inspector from reflection.
4. **MCP lane** (`exitium_mcp`, MC1 to MC3, then the export tool MC4, about 4): the protocol client, live tools, ops tools. Tools are built from the schema the running game reports (C4).
5. **Script lane** (`exitium_script`, about 12): the Luau VM wrapper, the reflection bindings, the safe and privileged tiers, the command-bus API, hot reload, the wasm build per AD2 (the real VM, or a cfg-gated stub if scripts are native-only), benches. Script systems run exclusively (the Lua state is not `Sync`).
6. **CLI lane** (`exitium_cli`, about 6): `exitium new`, export staging, `export --target windows`, `export --target web`, `serve`, and the `.mcp.json` template (AM7). SP18 runs after its last PR.
7. **Surface PRs** (4, after the spine): `exitium_physics` attribute PR, P15 and P16 (one at a time), and UI U13 (`exitium_ui`).
8. **Facade hook** (about 3): drain the command bus, the `editor` feature and headless mode, the script glue (sources through the asset server, reload events).
9. **End-to-end and parity tests** (3: two in `exitium_editor`'s tests, one facade test). The editor tests start a headless app with the live-control server and the `exitium_mcp` binary as a subprocess: a headless end-to-end test (scene and asset tools), and the Stage 3 parity test over the commands of the crates the editor depends on. The third, the **scripted-scene test**, is a facade test, because it needs `exitium_script`, which the editor has no edge to (C3): it builds a scene with a script through MCP tool calls, saves it, loads it back and compares, and checks parity for the script, physics and ui commands. It holds `crates/exitium/` and runs alone.

**Why it passes.**
- **Spine.** Editor, MCP, script, physics and ui all depend on `exitium_command`, so by R3 and R4 they wait for the whole spine. The spine is a serial chain in one crate (R1).
- **Beside the spine.** Only crates that do not depend on `exitium_command` run: `exitium_app` first (its dependents are the plugin crates, so R3 keeps them waiting for its two PRs), then platform, render, assets and audio (four crate directories, no edge among them by C1, none depends on command, and command depends on none of them). The CLI lane depends on serial only. No hotspot is touched (R2): the attributes and hooks are inside the crates, and the edges came in intake 2. That is at most six issues in flight beside the spine.
- **Editor lane start.** The editor depends on app, platform, render, assets, scene and command (C3), so R3 forbids it beside any of their PRs, and R4 makes ED6 wait for the app PR and ED8 for the platform and render PRs. It starts when the spine and the pre-editor lane have both merged. The pre-editor lane is at most four PRs deep (app, then platform or render), so the spine is the longer wait and the lane adds PRs but no time.
- **Beside the editor lane.** MCP (depends on command only), script (app, command), the CLI lane, and the physics and ui surface PRs (command, app). None of them is depended on by the editor, none depends on a crate the editor lane edits, and they edit separate directories. The editor and MCP lanes stay independent only if adding an editor command never requires an edit to `exitium_mcp`, which is why tools are built at run time from the schema (C4). The same holds for script bindings.
- **Facade hook and tests.** The facade depends on every crate, so it runs alone, after the editor, script, MCP and surface lanes. The two editor tests edit `exitium_editor`, which the facade depends on (optional, C2), so by R3 they never run beside a facade issue: they follow the facade hook, one at a time. The scripted-scene test is a facade issue (R2, R3: alone) and goes last (R4: it needs the hook's script glue). It starts the `exitium_mcp` binary as a subprocess, so the facade needs no dependency on `exitium_mcp`.
- **After Stage 3.** `exitium_command` is frozen (C1).

Critical path: command spine (12), editor lane (13), facade hook (3), tests (3: two editor, one facade): about 31 serial PRs. The script (12) and CLI (6) lanes are shorter than the editor lane.

**Exit.** An agent builds a scene through MCP (spawn entities, set components, attach a Luau script, save the scene, take a screenshot). A human opens the same scene in the editor, edits it and saves it. The script hot-reloads on Windows. The scripted sample runs in Chrome with Luau. If AD2 recorded native-only scripts, its web build instead runs the sample's Rust gameplay without the script, and the PR says which part is native-only. `exitium export --target windows|web` and the MCP export tool produce a Windows exe and a web build of the Stage 2 sample, and the web build runs in Chrome. The Stage 3 parity test (in the editor tests) and the facade scripted-scene test run in CI.

### Stage 4: 2D breadth

**Work.** First intake 3, then lanes:

1. **Intake 3** (see the table above).
2. **Render v2 lane** (`exitium_render`, about 14): virtual resolution and integer scaling, sub-pixel camera, palette swap and indexed colour, pixel-perfect rotation, normal-mapped lights, top-down, sidescroller and isometric camera modes with depth sorting and height levels, goldens.
3. **Aseprite import** (`exitium_assets`, about 3).
4. **Tilemap lane** (`exitium_tilemap`, about 10): chunk storage, autotile rules, the three modes, the plugin, collider output, text round trip, benches.
5. **Physics tooling** (`exitium_physics`, about 3): character controller, tile-collider merging, debug draw surfaces.
6. **Editor breadth** (`exitium_editor`, about 6): the tilemap tool, light and camera gizmos, the physics debug toggle (the lines are drawn through the facade glue, so the editor needs no physics edge, C3), Aseprite preview, and the matching commands. Tools come through the generic MCP path (C4); one small MCP PR if a tool needs an ops entry.
7. **Facade glue** (about 6): tilemap into render and physics, lights, camera modes, depth sorting, re-exports.
8. **Four sample scenes** in `crates/exitium/examples/` (about 4): pixel art, top-down, sidescroller, isometric. Each is a facade-lane PR.

**Why it passes.**
- Intake 3 runs alone at the Stage 3 to Stage 4 boundary (R1 to R3, as before) and creates the example crates (C8).
- Render v2, assets, tilemap and physics tooling are four crate directories (R1) with no edge among them (C1; tilemap talks to render and physics only through the facade). So they run together.
- The editor depends on render, assets and tilemap (C3), so editor breadth waits until those lanes finish (R3). It has no edge to physics. Facade glue and samples wait for the editor lane (R3).
- The UI full lane (Stage 5) may start now: `exitium_ui` has no edge to render, assets, tilemap or physics, and the editor has no ui edge until the Stage 5 edge PR (C5). Its 14 serial PRs fit inside render v2 (14) plus editor breadth (6), which both end before the facade glue starts. The facade depends on `exitium_ui`, so facade glue must not overlap it (R3).

Critical path: intake 3 (1), render v2 (14), editor breadth (6), facade glue (6), samples (4): about 31 serial PRs.

**Exit.** The four sample scenes run on Windows and in Chrome. Each uses a tilemap in the right mode, lighting where it fits (not the pixel-art scene's palette-swapped sprites), physics, and the matching camera mode. A tilemap is built by an agent through MCP and edited in the editor with the tilemap tool. Render goldens pass. The budgets below hold.

### Stage 5: sim, saves, full UI and editor extras

**Work.**

1. **Sim lane** (`exitium_sim`, about 10): fixed high-rate ticks, producer and consumer patterns, modifier pipelines, offline progress (analytic and chunked fast-forward, time cap, validation hooks), prestige layers, automation, benches.
2. **Save lane** (`exitium_save`, about 11): the container, versioned saves, migration chains, compression, authenticated tamper detection, autosave, the cloud provider trait, the **binary half** of the ECS world snapshot (#61; the text half is scene's SC2 to SC4, C7), and corruption tests.
3. **UI full lane** (`exitium_ui`, about 14): markup format and hot reload, themes, the remaining widgets, change-only data binding, the immediate-mode layer, the 10k-node and rebinding benches. It **starts in Stage 4** (see there) and must end before Stage 4's facade glue.
4. **Edge PR** (1): `exitium_editor -> exitium_ui` (C5). The first item of Stage 5.
5. **Editor extras** (`exitium_editor`, about 12): the Data Asset editor, the UI editor, the profiler panel, and the play-in-editor extensions below.
6. **MCP offline tools** (`exitium_mcp`, about 3): docs and API query, schema dump.
7. **Facade glue** (about 6) and **two samples** (2): glue for sim, save, UI full and the editor extras, with the 1M-entity round-trip test and bench (#67, C7) and the growth of the facade parity test for the new commands (C3); an incremental sample with prestige, offline gains and saves; and the heavy-numbers UI mockup.

**Play-in-editor: what Stage 5 adds.** Stage 3 already ships `play_start`, `play_pause`, `play_resume`, `play_step`, `play_advance` and `play_stop` (ED6, editor-mcp-mvp.md "Play, pause, step, stop"). They snapshot the scene text, the resources marked `custom(play_snapshot = true)` and the live-entity set, and restore them on Stop. That covers authoring data. ED6 lists what Stop does not restore. Stage 5 adds exactly these, decided in SP8:
- Data Asset edits made while playing are reverted on Stop (the registry is part of the snapshot).
- The UI tree is rebuilt from its layout file on Stop (ED6's documented gap: `UiTree` is not a scene member).
- The Luau state is reset on Stop, through a reflected event that the script plugin handles, so the editor needs no script edge.
- Sim and save resources carry `play_snapshot`, and autosave and backend writes are off while playing in the editor (a reflected flag the plugins read).

**Why it passes.**
- Sim, save and UI full are three crates with no edge among them (C1), and their edges go only to frozen crates (core, ecs, reflect, serial, app, command), so the three lanes run together. The editor has no edge to sim or save, so the editor lane runs beside them.
- **UI lane in Stage 4.** `exitium_ui` depends on frozen crates only, and none of Stage 4's lane crates (render, assets, tilemap, physics, editor) depends on it before the edge PR. So it runs beside them. It must end before the Stage 4 facade glue, because the facade depends on `exitium_ui` (R3).
- **Edge PR.** It edits the editor crate and holds `Cargo.toml` and `Cargo.lock`, so by R2 it runs alone among hotspot holders. It is `Blocked by` the last UI lane issue, because after it lands the editor depends on ui and R3 forbids the editor beside any ui issue. It cannot overlap a facade issue either (the facade depends on both crates), so it runs after Stage 4's samples and before the editor extras.
- **Editor extras.** They need the ui edge and the data edge (intake 3), so they start after the edge PR. They run beside sim, save and MCP offline.
- **MCP offline tools** edit `exitium_mcp`, which depends only on command.
- Facade glue and samples run alone at the end.

Critical path, counted from the end of Stage 4 and assuming the UI lane ran beside Stage 4: edge PR (1), editor extras (12), facade glue (6), samples (2): about 21 serial PRs. Without that overlap, add 14.

**Exit.** The incremental sample (prestige, offline gains) saves, migrates through two schema versions, and reloads correctly, on Windows and in Chrome. A 1M-entity world round-trips through both formats (#67). The UI mockup with 10,000 live counters holds its budget. The editor opens a Data Asset and a UI layout, edits them, and plays in the editor with the extensions above. The offline MCP tools answer from a cold start. The facade parity test covers every new command (C3).

### Stage 6: platform and online

**Work.**

1. **Steam lane** (`exitium_steam`, about 6): Steamworks init behind an optional, native-only feature (owner decision 4; SP15's ADR records how), achievements, Steam cloud as a save provider (E2), overlay, a test app.
2. **Net lane** (`exitium_net`, about 10): the transport on native and web, an authoritative client and server, replication of components marked through reflection, interpolation, input commands.
3. **Server lane** (`exitium_server`, about 6): the headless runtime (E4), stateless and horizontally scalable, hosting adapters.
4. **Backend lane** (`exitium_backend`, about 14): auth, cloud saves, leaderboards, server-side offline-gain validation reusing `exitium_sim` (E3), a web-capable client, load tests.
5. **CLI packaging** (`exitium_cli`, about 3): the Steam depot layout and the itch.io zip, from the export staging.
6. **Facade glue** (about 6): the net client and server plugins, Steam and backend plugins, `ServerPlugins`.
7. **Release workflow drafts** (2, in the CLI crate; owner decision 2): the Steam depot upload and the itch butler push, as YAML files under `crates/exitium_cli/release/`, which is an ordinary `Edits` path of the CLI lane. **No factory PR touches `.github/workflows/`**, which the App cannot push. One issue assigned to the owner asks them to copy the two files there. The drafts are thin: they call `exitium` packaging subcommands, so a later logic change is a CLI PR and never a workflow PR.

**Why it passes.**
- Steam, net, server, backend and the CLI are five crate directories (R1). The only edges are E2 (steam to save), E3 (backend to sim and save) and E4 (server to net). Save and sim are frozen after Stage 5, and `exitium_net` is not touched by the server lane's peers except server itself, so the net lane runs before the server lane (R3).
- Steam, backend and net start together once Stage 5 has merged. The server lane waits for net.
- The CLI depends on serial only (R3), so packaging runs beside all of them. The workflow drafts are CLI-crate files, so they sit in the same lane (5 serial PRs with packaging, off the critical path).
- All five lanes also depend on `exitium_command` and `exitium_app`, frozen since Stage 3 and Stage 2, so those edges cost no parallelism.
- Facade glue runs alone at the end (R2, R3).

Critical path: net (10), server (6), facade glue (6): about 22 serial PRs. The backend lane (14) runs beside it.

**Exit.** A sample game syncs its save to the backend, posts a leaderboard score, and has its offline gains validated by the server, from Windows and from Chrome. A networked sample runs with a headless server and at least two clients, with replicated and interpolated entities. The Steam path works by hand on Windows with Valve's test app: init, one achievement, one cloud save round trip. The server and backend run from a documented command on a clean checkout. The Steam depot layout and the itch zip are produced by the CLI, and the two workflow drafts are in the CLI crate (the owner pushes them; the stage does not wait for that). The facade parity test covers the Stage 6 commands, with secrets never exposed.

### Stage 7: rehearsal games in `examples/`

**Work.** Intake 3 created six crates in the repo root. Each game is built by an agent through MCP, Rust and Luau, refined by a human in the editor, and exported to a Windows exe and to the web:

| Example | Covers |
|---|---|
| `examples/topdown` | top-down camera, tilemap, lighting, gamepad, Luau gameplay scripts on top of the Rust rules |
| `examples/sidescroller` | sidescroller camera, tilemap, character controller, audio |
| `examples/isometric` | isometric tilemap, depth sorting, height levels |
| `examples/pixel_art` | virtual resolution, integer scaling, palette swap, Aseprite animation |
| `examples/incremental` | sim, prestige, offline gains, saves, full UI, **cloud saves, leaderboards, offline validation, Steam** |
| `examples/netplay` | `exitium_net`, `exitium_server`, interpolation, a web client |

**Rust first (owner decision 3).** Each example's rules run in Rust systems, so its web build plays whether or not Luau runs on wasm. The Luau in an example is limited to behaviour that may be missing on the web, and the example's issue lists it. About 4 PRs per game, about 24 in total. Engine-side output is gap issues and fixes, about 12, each an ordinary filed issue that goes through the same check.

**Why it passes.** Each example crate depends only on the facade (C1). Two examples have no edge to each other (R3), and an example never edits an engine crate. A gap fix edits an engine crate, so it conflicts with every example (R3, through the facade) and runs alone. Example PRs edit `examples/<name>/`. Under today's factory rules that path is a hotspot (R2), so examples would run one at a time. The housekeeping PR "treat `examples/<name>/` as an Edits entry" lifts that and lets three run together. Serial count: the 24 game PRs, the 12 gap fixes (each runs alone) and 2 docs PRs. With the rule change, three games run together, so the games take 2 batches of 4 PRs = 8 serial PRs, and the stage is 8 + 12 + 2 = 22. Without it, 24 + 12 + 2 = 38.

**Exit = full MVP.** All six games exist as a Windows exe and as a web build that runs in Chrome. Each was built through MCP (Rust, and Luau where the table lists it) and refined by a human in the editor. If AD2 recorded native-only scripts, the web builds run without their Luau layer (today `topdown`'s gameplay scripts) and still play, because their Rust rules are complete. The online path works (cloud saves, leaderboards, offline validation, networked play) from both targets. The exports were produced by the CLI and by the MCP tool on a clean checkout. Every gap issue the rehearsal found is merged or filed with `status:needs-approval`.

### Public API sketch (stage handoffs)

Names are placeholders. The signatures are each spec's. This only fixes who hands what to whom.

```rust
// Stage 1: each backend crate has a plain API and an example, no ECS.
// Stage 2: each adds `plugin::XPlugin: exitium_app::Plugin`.
// Facade, what the samples, `exitium new` and the examples use:
exitium::App::new()
    .add_plugins(exitium::DefaultPlugins)          // Stage 2: platform, render, assets, audio, physics, ui, scene, data
    .add_plugins(exitium::ScriptPlugin)            // Stage 3
    .add_plugins((exitium::TilemapPlugin, exitium::LightingPlugin)) // Stage 4
    .add_plugins((exitium::SimPlugin, exitium::SavePlugin))         // Stage 5
    .add_plugins((exitium::NetClientPlugin, exitium::BackendPlugin, exitium::SteamPlugin)) // Stage 6
    .add_systems(exitium::FixedUpdate, my_rules)
    .run();
// With `--features editor` the facade also adds exitium_editor's plugin (C2).
```

### Threading

- **ECS.** The scheduler has a single-threaded executor and a parallel executor on `exitium_tasks` (restored). The min-spec profile runs 4 threads. The web build is single-threaded (the tasks wasm fallback, #29), (web threads are out, owner decision 1).
- **Script.** The Luau state lives on the main thread. Script systems are exclusive (no parallel system runs beside them). Hot reload swaps a chunk at a frame boundary.
- **Editor.** The live-control server is an extra OS thread. It never touches the world. It queues requests, and the main loop drains them at one fixed point in the frame.
- **Assets, saves, audio.** Native worker threads decode assets and write saves. Autosave compression runs off the main thread, and on the web in slices (no threads).
- **Net and server.** The transport runs on its own threads or runtime, and hands packets to the main loop at one fixed point, the same way as the live-control server. The server is the main thread plus a worker pool.
- **MCP** is a separate process. The backend is a separate process. Neither shares the game's world or frame budget.

## MCP / editor surface

This is a roadmap, so it adds no tool itself. It fixes when each part of the agent-parity rule is met:

- Stages 1 and 2 add **no editor feature**, so they add no MCP tool. Components and resources become visible to MCP automatically, because they derive `Reflect` (design rule 2).
- Stage 3 delivers parity: every mutation is a command on the bus (design rule 3), the editor, MCP and Lua are three front ends of the same registry, and the tools, panels and bindings are built generically from the reflected surface (C4). The parity test enforces the rule in CI from then on, and every later stage grows it in a facade-hosted test (C3).
- Stage 3 also adds the export capability and its MCP tool together (CLI and `exitium_mcp`).
- Stage 4: tilemap paint and fill, light and camera settings, palette and Aseprite import are commands. Their editor tools and MCP tools arrive together.
- Stage 5: sim inspection, save list, load, export and migrate, Data Asset edit, UI layout edit and the offline tools are commands or queries, with matching editor panels. Play-in-editor control already exists in Stage 3 (`play_*`); Stage 5 only widens what Stop restores.
- Stage 6: net status, server control, backend login and leaderboard queries, Steam status are commands or queries. Secrets are never exposed by a tool.
- Sim, save, net, steam, server and backend commands register through `exitium_command` (the edges come in intake 3, C1). The editor and MCP see them in the registry at run time, so no editor edge is needed for parity (C4).
- Any editor feature added later ships with its MCP tool, as CLAUDE.md requires.

## Performance

**Every number in this section is accepted by the owner (2026-11-24)**: the PR counts, the elapsed-time estimate and the budgets, including the sim, net and backend budgets. Crate specs own the micro-budgets and may only tighten these. Any hot-path PR ships with a benchmark, and CI fails on a regression over 5% (design rule 4). "Not optimized" (decision 4) means the 2D-breadth budgets are floors to reach, not targets to beat.

**Factory budgets (accepted).**

| Item | Budget |
|---|---|
| PR size | about 600 changed lines or fewer |
| In flight | at most 8, and only conflict-free (CLAUDE.md) |
| Total | about 450 PRs (plus or minus 30%) |
| Critical path | about 200 serial PRs (198 summed below) |
| Elapsed | 17 to 29 days of factory time |

Allocation of PRs (an estimate; the specs' own "Implementation issues" sections replace these counts once filed):

| Stage | PRs | Shape |
|---|---|---|
| 0 | about 22 | 21 spec and ADR issues, plus this file |
| 1 | about 112 | intake 1, ECS 28, serial 17, reflect group 7, tasks 5, profiler 4 plus #65, platform 7, render 13, physics 7, audio 3, assets 9, UI 10, xtask 1 |
| 2 | about 58 | intake 2, spine 4, data 6, scene 8, plugins about 22, facade about 17 |
| 3 | about 65 | command 12, pre-editor lane 8, editor 13, MCP 4, script 12, CLI 6, surfaces 4, facade 3, tests 3 (2 editor, 1 facade) |
| 4 | about 48 | intake 3, render v2 14, Aseprite 3, tilemap 10, physics 3, editor 6, facade 6, samples 4, MCP 1 |
| 5 | about 59 | sim 10, save 11, UI 14 (runs in Stage 4), edge PR 1, editor 12, MCP 3, facade 6 (with #67), samples 2 |
| 6 | about 47 | Steam 6, net 10, server 6, backend 14, CLI 3, facade 6, workflow drafts 2 (CLI crate) |
| 7 | about 38 | six games 24, gap fixes about 12, docs 2 |
| Total | about 450 | |

Critical path, in serial PRs (each stage counted from its own intake or spine):

| Stage | Serial PRs | How |
|---|---|---|
| 0 | 2 | SP5, SP12 (the only chain in front of intake 1; the ADR chain AD1, AD2, SP16, SP15, SP17 runs beside Stages 1 to 3) |
| 1 | 37 | intake 1, reflect group 7, ECS 28, #65 |
| 2 | 32 | intake 2, data then scene 14, facade 17 |
| 3 | 31 | command 12, editor 13, facade hook 3, tests 3 (2 editor, 1 facade; the pre-editor lane runs beside the spine) |
| 4 | 31 | intake 3, render v2 14, editor 6, facade 6, samples 4 |
| 5 | 21 | edge PR 1, editor 12, facade 6, samples 2 (UI overlaps Stage 4) |
| 6 | 22 | net 10, server 6, facade 6 |
| 7 | 22 | 2 batches of three games x 4 = 8, gap fixes 12, docs 2 (38 without the factory-rule change) |
| Total | about 198 | |

The 17 to 29 day estimate (about 400 to 690 hours) holds only if one serial PR (implement, test, review, merge) takes 2 to 3.5 hours. If PRs take longer, elapsed time grows with the critical path, not with the PR count. Review rounds, CI queueing and the 5% usage gate are not modelled, so treat the estimate as a floor.

**Runtime exit budgets (accepted ceilings; min-spec 4 cores and 8 GB; Windows and Chrome).** The first block carries over from Jam.

| Where | Budget |
|---|---|
| Sample game (Stage 2) | at least 60 FPS, p99 frame at most 20 ms, with 2,000 sprites, 200 dynamic bodies and 100 UI nodes |
| Sample game | first frame at most 3 s on Windows (release), at most 5 s in Chrome from a warm cache |
| Web build | wasm at most 10 MB gzipped without scripting, at most 13 MB with Luau, assets excluded. The 13 MB applies only if the spike passes: if scripts are native-only, the web build has no Luau and the 10 MB limit holds |
| MCP round trip | one command applied and acknowledged in at most 50 ms (p95); a 1280x720 screenshot in at most 250 ms |
| Export | cold Windows export at most 10 min, cold web export at most 15 min, on 4 cores |
| ECS | no regression over 5% against the #51 baselines; absolute budgets in the ECS specs |

New budgets for the restored and new areas:

| Area | Budget |
|---|---|
| Tasks | 16 empty jobs on 4 threads at most 8 us (tasks spec); `parallel_for` and `par_iter_mut` over 1M items with a 100 ns body at least 3.2x on 4 threads |
| ECS 1M entities | one parallel tick of 1M entities x 2 components with a 100 ns body at most 35 ms on 4 threads; spawn of 1M at most 100 ms (storage spec) |
| Profiler | with features off, zero cost (spans compile out); frame stats at most 0.05 ms per frame; counting allocator adds at most 20 ns per allocation |
| Binary format | encode and decode of a 32 MB bulk-array world at least 500 MB/s; text round trip of the same at least 20 MB/s; zstd level 3 decode at least 800 MB/s |
| Reflection and Luau | a reflected field get or set from Lua at most 200 ns; 10,000 scripted entities running a 10-line `update` at most 4 ms per frame; hot reload of a 500-line script at most 50 ms; Luau on wasm at most 2.5x slower than native |
| Data assets | a 5-level variant chain resolves in at most 5 us; 10,000 data assets load and resolve in at most 150 ms |
| Asset hot reload | file change to new texture on screen at most 500 ms on Windows; no frame over 20 ms while it reloads |
| Gamepad | event to `InputState` in the same frame; no allocation per frame |
| Tilemap | 3,000 visible tiles at 1080p at 60 FPS with lighting on; a 32x32 chunk rebuild at most 0.2 ms; `set_tile` at most 200 ns; a 1,000x1,000 map loads in at most 300 ms |
| Pixel art | the integer-scale pass adds at most 0.3 ms GPU at 1080p; palette swap adds at most 5% to the sprite pass |
| Lighting | 32 visible normal-mapped lights at 1080p at most 4 ms GPU on the reference GPU (open question 2) |
| Depth sorting | 10,000 sprites sorted by depth key at most 0.5 ms |
| UI full | 10,000-node cold layout at most 6 ms (UI spec); rebinding 10,000 counters changing every frame at most 1.5 ms CPU; 100 changed bindings at most 20 us; markup parse of 1,000 nodes at most 3 ms; markup hot reload at most 100 ms |
| Editor | 60 FPS with 10,000 entities in the hierarchy (virtualised); an inspector edit applied and acknowledged in at most 16 ms |
| Sim | 100,000 producers ticked at most 3 ms per tick on one thread, 1.2 ms on 4 threads; 8 h offline catch-up at most 50 ms (analytic) or 1.5 s (chunked), default time cap 24 h |
| Saves | encode 1 MB of reflected state at most 5 ms, compress at most 8 ms, load and migrate at most 15 ms; autosave never makes a frame longer than 20 ms |
| Steam | init at most 500 ms; an achievement unlock call at most 0.1 ms on the main thread |
| Net | a 32-client, 200-entity world ticks at most 4 ms on the server at 60 Hz; at most 20 KB/s per client at 20 snapshots/s; default interpolation delay 100 ms |
| Server | idle memory at most 50 MB; startup at most 1 s |
| Backend | on 2 vCPUs, leaderboard submit at most 50 ms p95 and top-100 read at most 30 ms p95 at 500 requests/s; offline validation of 8 h at most 100 ms |
| Examples (Stage 7) | each game at least 60 FPS, p99 frame at most 20 ms, on Windows and Chrome; wasm at most 13 MB gzipped (10 MB if scripts are native-only) |

## Test plan

Each stage has one exit check, and the stage is not done until it passes:

- **Stage 0:** each spec and ADR merges after the AI review. The spike's report (SP1) contains the measurements listed above, and AD2 records the route.
- **Stage 1:** `cargo xtask ci all` green on `main`. Each backend example runs on Windows and its PR records that. The ECS, tasks and format benchmarks show no regression. The golden files for the text and binary formats pass. The min-spec profile runs.
- **Stage 2:** the sample game runs on Windows for a fixed number of frames, with a render-to-texture screenshot compared to a stored image. It also runs once by hand in Chrome. The PR records both. The facade tests that plugin components and a prefab variant round-trip through a scene, and that a changed texture on disk reloads.
- **Stage 3:** two tests in `exitium_editor` start a headless app with the live-control server and the `exitium_mcp` binary as a subprocess: a scene end-to-end test, and the parity test over the commands of the crates the editor depends on. A facade test does the same with the script plugin: it builds a scene with a script through MCP tool calls, saves it, loads it back and compares, and checks parity for the script, physics and ui commands. All run in CI. `cargo build --workspace --target wasm32-unknown-unknown` passes with the script crate as the real VM or as the stub. A `play_start` then `play_stop` round trip restores the scene and the opted-in resources. The two exports run on a clean checkout and the web build runs in Chrome (with Luau if the spike passed, without it if not).
- **Stage 4:** render goldens for each camera mode, lights, palette swap and integer scaling. The four sample scenes run on Windows and in Chrome. A tilemap built through MCP equals one built in the editor.
- **Stage 5:** a save written by schema version 1 loads in version 3 through two migrations. Corruption and tamper tests reject bad saves. The 1M-entity round trip (#67, a facade test). The UI and sim benches hold their budgets. Stop restores the extras Stage 5 adds (Data Asset edits, the UI tree, the Luau state, sim and save resources). The facade parity test covers the new commands.
- **Stage 6:** an integration test starts the backend and a headless server on localhost, then runs two clients (one native, one headless test client) through login, a cloud save, a leaderboard post and a validated offline claim. Steam is checked by hand (Valve's client is required, so CI cannot run it). A load test records the backend budgets.
- **Stage 7:** the rehearsal is the test. All six exports are produced by the CLI and by the MCP tool, started on a clean checkout, and all of them run in Chrome and on Windows.

The wasm build covers the runtime crates in CI (`cargo build --workspace --target wasm32-unknown-unknown`, excluding `NATIVE_ONLY`). `exitium_steam`, `exitium_server` and the server half of `exitium_backend` go into `NATIVE_ONLY`, and the backend's web client must still build for wasm (open question 1). `exitium_script` is not in `NATIVE_ONLY`: it builds for wasm as the VM or as the stub (SP9). Running in a browser is checked by hand in Chrome only.

## Open questions

The owner answered the first questions this file raised (web threads, release workflows, the Luau fallback, the Steamworks licence, the facade chain, the PR counts and budgets) on 2026-11-24: see "Owner decisions on review". These remain. Each is for the architect to settle when the named spec merges, or for the owner. None blocks this PR.

1. **Backend client on the web.** `exitium_backend` has a server half (native) and a client half (must build for wasm). SP17 decides one crate with features, or two crates. Two crates add a stub in intake 3.
2. **Reference GPU.** The lighting budget needs a named reference GPU (min-spec is stated as 4 cores and 8 GB, with no GPU). SP12 proposes one.
3. **`scene -> data` (E1).** SP10 may remove the edge by putting the override engine in `exitium_reflect` or `exitium_serial`. If it does, the data and scene lanes run side by side and Stage 2 is about 6 PRs shorter.
4. **`docs-graph` cadence.** Running it after every merge serializes a lane's next issue. Proposal (carried from Jam): at the end of each lane and at each stage boundary. That is a factory-rule change, so its own PR.
5. **Windows CI.** The editor, CLI, Steam and server crates are `NATIVE_ONLY`. Confirm CI runs their tests on a Windows runner. Workflow changes are owner-pushed.
6. **Zstd on wasm.** The `zstd` crate wraps C code and shares R5's toolchain trouble. SP14 chooses between it, a pure-Rust decoder, and LZ4 on the web.

## Implementation issues

This roadmap has no code. Its work items are the issues below. Each is PR-sized (about 600 changed lines or fewer), has a `## Footprint`, acceptance criteria and `Blocked by` lines, and goes into the MVP milestone. After this spec merges, the architect files the missing ones with the `file-issue` skill. The three intake issues are the only ones allowed to edit more than one crate.

**Already filed.**
- Stage 0: #24 and this file (#129). Jam's #22, #23, #106 to #112 are merged.
- Stage 1, Jam-era (now blocked by intake 1 once filed): reflect #31 to #33, #36, #37; derive #34, #35; ECS #38 to #52; serial #53 to #61; profiler #62 to #65; xtask #66; #67; tasks #26 to #30.

**To file now (from this spec).**

1. **Spec and ADR issues** (table below). `Hotspots: none` and `New crate edges: none`, except AD1, AD2, SP15, SP16 and SP17, which list `Hotspots: docs/decisions/` because they write an ADR. `Edits` is one exact spec file (SP2: two, AD1 and AD2: none). The editor spec's AM1 to AM3, AM5, AM6 and AM9 are not filed separately: SP3 to SP7 carry them. AM8 is dropped.

   | ID | Title | Edits | Governs | Blocked by |
   |---|---|---|---|---|
   | SP1 | docs(spec): Luau on wasm, the R5 spike report | `docs/specs/luau-wasm-spike.md` | none | none (start first) |
   | AD1 | docs(adr): `triple_buffer` licence exception | none | none | none |
   | AD2 | docs(adr): Luau on wasm, the route chosen | none | none | SP1 |
   | SP2 | docs(spec): ECS parallel executor, tasks, profiler amendments | `docs/specs/ecs-scheduler.md`, `docs/specs/ecs-storage-queries.md` | `crates/exitium_ecs/`, `crates/exitium_tasks/`, `crates/exitium_profiler/` | none |
   | SP3 | docs(spec): platform gamepad and editor hooks amendment | `docs/specs/app-platform.md` | `crates/exitium_platform/`, `crates/exitium_app/` | none |
   | SP4 | docs(spec): assets hot reload and editor surface amendment | `docs/specs/assets-scene-export.md` | `crates/exitium_assets/`, `crates/exitium_audio/`, `crates/exitium_scene/`, `crates/exitium_cli/` | none |
   | SP5 | docs(spec): render v2 hooks and surface overlay amendment | `docs/specs/render-v1.md` | `crates/exitium_render/` | none |
   | SP6 | docs(spec): full UI and command bus amendment | `docs/specs/ui-mvp.md` | `crates/exitium_ui/` | none |
   | SP7 | docs(spec): physics debug draw, tooling and command bus amendment | `docs/specs/physics-2d.md` | `crates/exitium_physics/` | none |
   | SP8 | docs(spec): editor extras and offline MCP amendment | `docs/specs/editor-mcp-mvp.md` | `crates/exitium_editor/`, `crates/exitium_mcp/`, `crates/exitium_command/` | SP6, SP7, SP10, SP11, SP12 |
   | SP9 | docs(spec): Luau scripting | `docs/specs/luau-scripting.md` | `crates/exitium_script/` | AD2, SP8 |
   | SP10 | docs(spec): data assets and prefab variants | `docs/specs/data-assets-prefabs.md` | `crates/exitium_data/`, `crates/exitium_scene/` | SP4 |
   | SP11 | docs(spec): tilemap | `docs/specs/tilemap.md` | `crates/exitium_tilemap/` | SP5 |
   | SP12 | docs(spec): pixel art, lighting and camera modes | `docs/specs/render-2d-breadth.md` | `crates/exitium_render/`, `crates/exitium_assets/` | SP4, SP5 |
   | SP13 | docs(spec): incremental sim | `docs/specs/sim.md` | `crates/exitium_sim/` | SP8 |
   | SP14 | docs(spec): saves | `docs/specs/saves.md` | `crates/exitium_save/` | #24, SP8 |
   | SP15 | docs(spec): Steam | `docs/specs/steam.md` | `crates/exitium_steam/` | SP14, SP16 |
   | SP16 | docs(spec): net and server | `docs/specs/net-server.md` | `crates/exitium_net/`, `crates/exitium_server/` | #24 |
   | SP17 | docs(spec): backend | `docs/specs/backend.md` | `crates/exitium_backend/` | SP8, SP13, SP14, SP15 |
   | SP18 | docs(spec): export of `examples/` | `docs/specs/export-examples.md` | `crates/exitium_cli/` | SP15, SP17, the last Stage 3 CLI lane issue |

   **Acceptance additions.** SP9, SP13, SP14 and SP17 each add: the spec states how the crate takes part in play-in-editor, as SP8's contract defines it (what is snapshotted, reset or suppressed on Stop), and its implementation issues include that behaviour. SP9 also keeps the cfg-gated path: the crate builds for wasm as the real VM or as a stub (owner decision 3). SP15's ADR records how the proprietary SDK is supplied: an optional native-only feature, the redistribution terms, kept out of the wasm and default builds (owner decision 4).

   #24 also needs its Footprint filled: `Edits: docs/specs/serial-binary-format.md`, `Governs: crates/exitium_serial/`, `Hotspots: none`, `New crate edges: none`.

2. **Intake 1: chore(deps): dependencies and the Stage 1 crate graph.** Blocked by #24, SP2 to SP7, SP12, AD1.
   ```
   ## Footprint
   - Edits: crates/exitium_ecs/, crates/exitium_ecs_derive/, crates/exitium_reflect/, crates/exitium_reflect_derive/, crates/exitium_serial/, crates/exitium_tasks/, crates/exitium_profiler/, crates/exitium_platform/, crates/exitium_render/, crates/exitium_physics/, crates/exitium_audio/, crates/exitium_assets/, crates/exitium_ui/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock, deny.toml, xtask/, crates/exitium/
   - New crate edges: the union of the wiring sections of the merged Stage 1 specs (C5). At least the Stage 1 graph above, including exitium_ecs -> exitium_tasks and exitium_ecs -> exitium_profiler
   ```
   Acceptance: every Stage 1 crates.io dependency, including the compression and zero-copy crates #24 names, is in `[workspace.dependencies]` with a one-line comment (licence, wasm, why not std); the `deny.toml` entry for `triple_buffer` is present and AD1 is on `main`; `exitium_ecs_derive` exists (C8); `cargo deny check` passes; `cargo xtask ci all` passes; no edge among leaf crates (C1) and none from `exitium_serial` to `exitium_ecs` (C7); every later Stage 1 issue can be done without editing `Cargo.toml` or `Cargo.lock`.

3. **Intake 2: chore(deps): dependencies and the Stage 2 and 3 crate graph.** Blocked by intake 1, every Stage 1 lane issue, #110, SP1, AD2, SP8 to SP11.
   ```
   ## Footprint
   - Edits: crates/exitium_app/, crates/exitium_platform/, crates/exitium_render/, crates/exitium_physics/, crates/exitium_audio/, crates/exitium_assets/, crates/exitium_ui/, crates/exitium_scene/, crates/exitium_data/, crates/exitium_script/, crates/exitium_command/, crates/exitium_editor/, crates/exitium_mcp/, crates/exitium_cli/, crates/exitium_reflect_derive/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock, deny.toml, xtask/src/ci.rs, crates/exitium/
   - New crate edges: the union of the wiring sections of the merged specs (C5). At least the known edges in "Stage 2", including E1, the facade to every runtime crate and `exitium_command`, and the optional `exitium_editor`
   ```
   Intake 2 holds `crates/exitium/` first. After it merges, each facade issue holds it in turn. Acceptance: creates `exitium_command` and its `CLAUDE.md` (C8); adds `exitium_mcp` to `NATIVE_ONLY`; no leaf edge outside E1 (C1); no edge from `exitium_mcp` to the editor, the facade or a plugin crate, nor from the editor to `exitium_mcp` (C3); no editor edge beyond the Stage 3 list in C3; `cargo xtask ci all` passes.

4. **Intake 3: chore(deps): dependencies, the Stage 4 to 7 crate graph and the example crates.** Blocked by intake 2, every Stage 2 and 3 lane issue, SP13 to SP18, and the ADR PR of each exception it records (it holds no `docs/decisions/`, C9).
   ```
   ## Footprint
   - Edits: crates/exitium_tilemap/, crates/exitium_sim/, crates/exitium_save/, crates/exitium_steam/, crates/exitium_net/, crates/exitium_server/, crates/exitium_backend/, crates/exitium_render/, crates/exitium_assets/, crates/exitium_physics/, crates/exitium_ui/, crates/exitium_editor/, crates/exitium_mcp/, crates/exitium_cli/, crates/exitium_reflect_derive/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock, deny.toml, xtask/, crates/exitium/, examples/
   - New crate edges: the union of the wiring sections of the merged specs (C5). At least the known edges in "Crate graph" for tilemap, sim, save, steam, net, server and backend, including E2, E3, E4 and their exitium_command edges, and exitium_editor -> exitium_tilemap, exitium_data, exitium_profiler. Not exitium_editor -> exitium_ui (the edge PR)
   ```
   Acceptance: the six example crates exist as stubs depending on `exitium` only, with `examples/*` as workspace members (C8); `NATIVE_ONLY` and `FEATURE_CHECKS` entries for Steam, server and the backend (C8); every `deny.toml` exception points to an ADR already on `main`; no leaf edge outside E1 to E4 (C1); `cargo xtask ci all` passes.

   **Edge PR: chore(editor): add the editor to ui edge.** Blocked by the last UI full lane issue and the last Stage 4 sample issue (it runs before the Stage 5 editor extras).
   ```
   ## Footprint
   - Edits: crates/exitium_editor/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock
   - New crate edges: exitium_editor -> exitium_ui
   ```
   Acceptance: only the dependency line is added; no other edge changes; `cargo xtask ci all` passes.

5. **Per-spec implementation issues.** Each spec's own "Implementation issues" section lists them. Expected lanes: Stage 1 backends, serial, ECS, tasks, profiler; Stage 2 spine, data, scene, plugins, facade chain; Stage 3 command, editor, MCP, script, CLI, surfaces, tests; Stage 4 render v2, tilemap, physics, editor breadth, samples; Stage 5 sim, save, UI full, editor extras; Stage 6 Steam, net, server, backend, packaging; Stage 7 the six games. Their `Blocked by` lines follow the lane orders above, and each is `Blocked by` its intake (C6). Facade issues list `crates/exitium/` under `Hotspots` and `Edits: none`.

6. **Housekeeping.**
   - **Re-milestone** the restored issues #26 to #30, #36, #48, #52, #56 to #67, and #24, to the MVP milestone.
   - Add `Blocked by <intake 1>` to the existing Stage 1 issues (#26 to #67 except the xtask and profiler tail items, which are also `Blocked by` the lane they follow).
   - **Retitle and re-scope**, per C7: #61 becomes the binary world snapshot in `exitium_save` (Stage 5; Footprint `crates/exitium_save/`; the text half is scene's SC2 to SC4). #67 becomes a facade test and bench (Stage 5; `Edits: none`, `Hotspots: crates/exitium/`). Trim #65 to its real footprint (`exitium_ecs`, `exitium_tasks`, `exitium_profiler`).
   - **Editor-spec amendments.** Do not file AM1 to AM3, AM5, AM6, AM8 or AM9 as separate issues (SP3 to SP7 carry them; AM8 is dropped). File AM4, AM7 and the code halves of AM1 to AM3 as Stage 3 lane issues with the footprints in editor-mcp-mvp.md "Downstream adjustments", each `Blocked by` intake 2.
   - Undo the Jam trims: #42 and #51 keep their sequential scope, `par_iter_mut` stays with #48 (jam-mvp.md open question 4).
   - Set Project priorities: the reflect group, tasks and profiler above ECS, and ECS above serial, in Stage 1.
   - **Factory-rule PR** `chore(factory): treat examples/<name>/ as an Edits entry` in `factory-run`, so examples run side by side. Ordinary PR, same AI review.
   - **Docs follow-up PR** (a hotspot: PLAN.md, INTERVIEW.md, the epics). This spec does not edit them. The follow-up updates: PLAN.md phase order, the "Glutton critical path" wording and the roadmap table, so the MVP is first and Glutton is later; INTERVIEW.md decisions that named Glutton as the driver; the epics' checklists (#3 to #11) to match "Epic mapping"; and a pointer from CLAUDE.md "Orient cheaply" if the owner wants one. Run it as a `docs-graph` pass after Stage 0 merges.
   - **Release workflows (owner decision 2).** File the two drafts as CLI-lane issues (`Edits: crates/exitium_cli/`). File one issue assigned to the owner: copy `crates/exitium_cli/release/*.yml` to `.github/workflows/`. No factory issue lists `.github/workflows/` in its Footprint.
   - Lift `needs-spec` as each spec merges.

**Dependency candidates (crates.io, for the intake PRs).** The versions, licences, wasm support and why-not-std argument for each belong to the owning spec. The intake PR records them in the workspace manifest. Nothing here is a decision. Licences below are from memory and must be checked by the owning spec. Items marked "R5" share the C or C++ toolchain risk.

| Crate | Used by | Intake | Native | wasm32 | Notes |
|---|---|---|---|---|---|
| `wgpu`, `winit`, `taffy`, `rapier2d`, `kira`, `png`, `hound`, `lewton`, `ttf-parser`, `glyphon`, `bytemuck`, `web-time` | Stage 1 backends | 1 | yes | yes (WebGPU only) | As in the merged specs (`png`, not the `image` crate: assets-scene-export.md). |
| `gilrs` | `exitium_platform` (gamepad) | 1 | yes (XInput, WGI) | yes, via `wasm-bindgen` | MIT OR Apache-2.0. SP3 decides. |
| `notify` or polling with std | `exitium_assets` (hot reload) | 1 | yes | no (web has no file watch) | CC0-1.0. Check `deny.toml`. Polling at 4 Hz needs no dependency. SP4 decides. |
| `tracy-client` | `exitium_profiler` (`tracy` feature) | 1 | yes | not required | Off by default. Builds C++ (native only). |
| `mlua` with `luau` and `vendored` | `exitium_script` | 2 | yes | **R5**: Luau is C++ | MIT. The spike decides the route. If none works, wasm gets a stub (owner decision 3). |
| `egui` family, `rmcp`, `tokio`, `serde`, `serde_json`, `zip` | editor, MCP, CLI | 2 | yes | not required | As in editor-mcp-mvp.md and assets-scene-export.md. |
| An Aseprite parser (for example `asefile`) | `exitium_assets` | 3 | yes | yes (pure Rust) | Check licence and maintenance. SP12 decides. |
| A compression crate for the binary container (`lz4_flex`, `zstd` (C) or `ruzstd` (pure Rust)) and a zero-copy cast crate (`bytemuck`) | `exitium_serial` (#24, Stage 1) | 1 | yes | `zstd`: **R5-like**; `ruzstd`: decode complete, encoder limited; `lz4_flex`: yes | #24 names them, so intake 1 adds them. |
| Any further compression or codec crate | `exitium_save` | 3 | yes | as above | SP14 decides, including the web. |
| An authenticated cipher and hash (`chacha20poly1305`, `blake3` or `sha2` and `hmac`), `getrandom` | `exitium_save` | 3 | yes | yes (`getrandom` needs its wasm feature) | Tamper detection is a deterrent: a key shipped in the client is not secret. The server validates. |
| `steamworks` (Steamworks bindings) | `exitium_steam` | 3 | Windows | no | **Proprietary SDK** (Valve), accepted by the owner. SP15's ADR records how: an optional native-only feature, the redistribution terms, kept out of the wasm and default builds. |
| A net transport: `renet` or `quinn` (native), `tungstenite` or WebTransport (web) | `exitium_net` | 3 | yes | web needs WebSocket or WebTransport, not UDP | SP16 and an ADR decide. |
| `axum`, `hyper`, `tokio`, `sqlx` or `rusqlite`, password and token crates, `reqwest` or `ureq` (client) | `exitium_server`, `exitium_backend` | 3 | yes | client only: fetch through `web-sys` or `reqwest` | SP17 decides. The server stack is native only. |
| A seeded RNG (`rand_pcg` or a hand-written PCG) | `exitium_sim`, `exitium_net` | 3 | yes | yes | A 20-line PCG avoids the dependency. |

The ECS, reflect, serial, tasks, tilemap, data and command crates add no external dependency beyond the above, unless their specs justify one. Anything not in this table needs the owning spec to justify it, and goes into the intake PR before its stage starts.
