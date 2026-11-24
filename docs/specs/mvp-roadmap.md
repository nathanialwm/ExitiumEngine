# Spec: Full MVP roadmap (stages, lanes, restored scope)

- Issue: #129
- Crates: none. This is a roadmap spec: it governs no crate and edits no code. It sequences work across every engine crate, `exitium_command`, `exitium_editor`, `exitium_mcp`, `exitium_cli`, the `exitium` facade and the `examples/` games.
- Status: draft. Supersedes [jam-mvp.md](jam-mvp.md) (#112). Owner-approved (2026-11-24): the contract below. Everything labelled **proposed** is the author's and has not been approved: stage order inside the contract, lane layouts, PR counts, budgets and the rule amendments. The cross-spec rules C1 to C9 are orchestrator rulings, not owner decisions.

## Goal

Record the full MVP plan so that later agents work from the repo and not from chat history.

The full MVP is: an agent builds most kinds of 2D game through MCP plus Rust and Luau (top-down, sidescroller, isometric, pixel art, incremental, and online with saves and leaderboards), a human refines the result in the editor, and it exports to a Windows exe and to the web (wasm). The owner replaced the Jam MVP with this plan on 2026-11-24. The Jam scope cuts are gone, and Glutton is no longer the driver.

The plan has eight stages. Each stage has a lane layout (which crates are edited at the same time), a reason why that layout passes the `factory-run` conflict check, and an exit criterion that can be checked.

| Stage | Name | Exit (short form; the full criteria are in each stage section) |
|---|---|---|
| 0 | Specs and the R5 spike | Intake 1's specs and the Luau-on-wasm spike are merged. The other specs merge before the intake that needs them. |
| 1 | Foundations and standalone backends | Tasks, profiler, reflect, ECS (with the parallel executor), serial (text and binary) merged. Every backend has a Windows example and builds for wasm. |
| 2 | Runtime integration | The sample game runs on Windows and in Chrome, with data assets, prefab variants, gamepad and asset hot reload. |
| 3 | Editor, MCP, Luau and export (Windows tools) | An agent builds a scripted scene through MCP, a human edits it, and `exitium export` produces a Windows exe and a web build of it. |
| 4 | 2D breadth | Four sample scenes (pixel art, top-down, sidescroller, isometric) run with tilemaps, lighting and physics, with editor tooling. |
| 5 | Sim, saves, full UI and editor extras | A sample incremental game with prestige and offline gains saves, migrates and reloads. A heavy-numbers UI mockup holds its budget. |
| 6 | Platform and online | A sample game syncs saves to the backend, posts leaderboards, has offline gains validated by the server, runs on Steam and plays networked. |
| 7 | Rehearsal games in `examples/` | Six games exported to Windows and web, covering every game type and the online path. **This is the full MVP.** |

Size, **proposed**: about 440 PRs, about 190 of them on the critical path, which is 16 to 27 days of factory time at 2 to 3.5 hours per serial PR. These are estimates (plus or minus 30%), not commitments. See "Performance" for what they assume. The Jam plan was about 80 PRs and 4 to 7 days.

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

**Everything else in jam-mvp.md "Scope cuts" is restored:** `exitium_tasks` and the parallel executor, `par_iter_mut`, the 1M-entity benches, the min-spec profile, Tracy, frame stats, the counting allocator, executor spans, data assets and prefab variants, asset hot reload, gamepad, the full UI (markup, themes, binding performance, immediate mode), the editor extras (play-in-editor, Data Asset editor, UI editor, profiler panel), offline MCP tools, the parity test, and physics debug draw and tooling. "Restored scope" below maps each one to a stage.

**Still out:** a WebGL2 fallback, editor/MCP/CLI on Linux and macOS, Firefox and Safari, network scale-out (Phase 9), destruction (epic #11) and the Glutton repo (epic #9).

### Cross-spec rules

Jam's C1 to C6 are restated here so one file is complete. Other specs cite them by number, and the numbers do not change. A spec may refine a rule's detail, but not break it.

- **C1 (amended). No edges between leaf crates, except a closed list.**
  - *Foundation crates:* `exitium_core`, `exitium_profiler`, `exitium_tasks`, `exitium_reflect` (with its derive), `exitium_ecs` (with its derive), `exitium_serial`, `exitium_app`, `exitium_command`. Anything may depend on them, within the graph below.
  - *Leaf crates:* `exitium_platform`, `exitium_render`, `exitium_physics`, `exitium_audio`, `exitium_assets`, `exitium_ui`, `exitium_scene`, `exitium_data`, `exitium_script`, `exitium_tilemap`, `exitium_sim`, `exitium_save`, `exitium_steam`, `exitium_net`. No edge between any two leaf crates, in either direction, normal or dev, except the four below.
  - *Allowed leaf edges (the whole list):* **E1** `exitium_scene -> exitium_data` (**proposed**: prefab variants and data-asset variants share one "base plus overrides" engine; SP10 may remove it by placing that engine in a foundation crate). **E2** `exitium_steam -> exitium_save` (Steam cloud implements the cloud provider trait). **E3** `exitium_backend -> exitium_sim` and `exitium_backend -> exitium_save` (server-side offline validation, save verification). **E4** `exitium_server -> exitium_net` (the server is a layer on net).
  - Cross-crate glue lives in the facade `crates/exitium/` (UI draw data and physics debug draw into the renderer, tilemap into render and physics, scripts and data into the asset server, `DefaultPlugins`). The facade is a hotspot, so its issues list `crates/exitium/` under `Hotspots`, never under `Edits`.
  - **Reason for the amendment:** Jam's list had seven crates. The full MVP has fourteen leaf crates, and the four edges above are the only ones that avoid duplicating real code (a second override engine, a second save verifier, a second sim for validation). Every other pairing still goes through the facade.
- **C2 (kept). The `editor` feature lives in the facade.** `exitium/editor` enables `exitium_editor`.
- **C3 (kept, extended).** `exitium_editor` may depend on any runtime crate and on `exitium_command`. In this plan it gains edges to `exitium_tilemap`, `exitium_data`, `exitium_ui`, `exitium_physics` and `exitium_profiler` (Stages 4 and 5). `exitium_mcp` depends only on `exitium_command`: never on the editor, the facade or a plugin crate, and no dev-dependency on them. The live-control server lives in `exitium_editor`. The end-to-end and parity tests live in `exitium_editor`'s tests and drive the MCP binary as a subprocess.
- **C4 (kept, extended). Reflected surface.** Each per-crate spec lists the reflected surface it exposes (components, resources, commands). The editor panels, the MCP tools **and the Luau bindings** are built generically from that surface. No crate hand-writes a tool, a panel or a binding.
- **C5 (kept). Intake rule.** The intake PRs add the union of the "wiring" sections of the merged specs, plus their crates.io dependencies. This file names the edges known today and is not the complete list. An edge that breaks C1 or C3 is a spec error, not intake input.
- **C6 (amended). Intake blocks the stage.** There are **three** intakes, not two (see "Intake PRs"). Every Stage 1 lane issue is `Blocked by` intake 1, every Stage 2 and 3 lane issue by intake 2, and every Stage 4 to 7 lane issue by intake 3. **Reason:** a fourth boundary would drain the factory once more for little gain, since every spec is merged long before the stage that needs it.
- **C7 (new). ECS and serial are siblings.** `exitium_serial` has no edge to `exitium_ecs`, and `exitium_ecs` has none to `exitium_serial`. **Reason:** such an edge would stop the two longest Stage 1 lanes running side by side (R3). Consequence: issue #61 (ECS world snapshot) cannot live in `exitium_serial`. It is re-homed to `exitium_save`, which depends on both. The text snapshot already belongs to `exitium_scene`. #67 (1M-entity round trip) is re-homed with it.
- **C8 (new). New crates are created at an intake.** A new crate (`exitium_ecs_derive`, `exitium_command`, the example crates) is a Cargo workspace member and a `Cargo.lock` entry, so it is hotspot work. The intake that wires it creates it, with its `CLAUDE.md`, its `ENGINE_CRATES` or `NOT_DERIVE_USERS` entry in `exitium_reflect_derive` (the `crate_paths.rs` drift test fails otherwise) and its `NATIVE_ONLY` or `FEATURE_CHECKS` entry in `xtask`. Intakes therefore list `crates/exitium_reflect_derive/` under `Edits`.
- **C9 (new). Intake needs specs, not the other way round.** An intake is `Blocked by` the specs whose crates it wires. A spec never waits on an intake. A spec issue that needs an ADR lists `docs/decisions/` under `Hotspots`, and at most one such issue runs at a time (R2).

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
sim                      -> core, ecs, reflect, serial | plugin: app
save                     -> core, ecs, reflect, serial | plugin: app
net                      -> core, ecs, reflect, serial, app
steam                    -> core, ecs, app, save                  (E2)

Service crates
server                   -> core, ecs, app, net                   (E4)
backend                  -> core, reflect, serial, sim, save      (E3)

Native-only tools
editor                   -> command, scene, assets, render, platform, serial, app (Stage 3)
                            + tilemap, physics (Stage 4), data, ui, profiler (Stage 5)
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

1. **The facade is a serial tail of every stage.** It depends on every crate, so each facade issue runs alone (R2 and R3). With six stages adding glue, the facade chain is about 45 PRs in total. Open question 5 asks whether to change that.
2. **Stage order is the order of exit checks, not a wall.** Inside one intake's span, a lane starts as soon as it passes the check. For example the Stage 5 UI lane may start beside Stage 4 lanes. The estimates in "Performance" assume this overlap where this file says so, and no other.

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

**Amendments to existing specs (to file).** Each one changes only the named spec file, and also removes that spec's "post-MVP" and "cut" wording for the restored items.

| ID | Amends | What it settles | Governs | Needed by |
|---|---|---|---|---|
| SP2 | `ecs-scheduler.md` and `ecs-storage-queries.md` | **ECS executor.** The parallel executor on `exitium_tasks` (#48), `par_iter_mut`, the `exitium_ecs -> exitium_tasks` edge, executor spans (#65), the 1M-entity benches (#52), the min-spec profile (#66), how script systems run (exclusive, `NonSend`). Also tasks edges (`tasks -> profiler`) and Tracy, frame stats and the counting allocator (#62 to #64) at the profiler level. | `exitium_ecs`, `exitium_tasks`, `exitium_profiler` | Intake 1 |
| SP3 | `app-platform.md` | **Platform gamepad.** Gamepad as one more `InputEvent` source, native and web, mapping, deadzones, the gamepad plugin systems. | `exitium_platform` | Intake 1 |
| SP4 | `assets-scene-export.md` | **Assets hot reload.** File watch (native only) and web behaviour, reload events, how data assets and shaders reload, the `exitium_assets` plugin systems. Scene and CLI parts are untouched. | `exitium_assets` | Intake 1 |
| SP5 | `render-v1.md` | **Render v2 hooks.** What render v1 must leave open so Stage 4 adds no rework: the depth sort key and its width, a normal-map texture slot, ordered extra passes (lights), an offscreen virtual-resolution target, a user material slot. Changes v1's non-goals that now point at Stage 4. | `exitium_render` | Intake 1 |
| SP6 | `ui-mvp.md` | **UI full.** Markup file format and hot reload, themes, the rest of the widget list (epic #5), data binding that updates only on change, the immediate-mode layer, the 10k-node and counter-rebinding benches. Settles the arena shape now, because binding changes the tree layout. | `exitium_ui` | Intake 1 (arena shape), Stage 5 (the rest) |
| SP7 | `physics-2d.md` | **Physics debug draw and tooling.** Debug draw as an editor overlay and MCP query, character controller, tile-collider merging, joints decision. | `exitium_physics` | Intake 3 |
| SP8 | `editor-mcp-mvp.md` | **Editor extras.** Play-in-editor with snapshot restore, Data Asset editor, UI editor, tilemap tool, lighting and camera gizmos, profiler panel, offline MCP tools (docs and API query, schema dump), parity-test growth rule, export and Steam tools. | `exitium_editor`, `exitium_mcp`, `exitium_command` | Intake 3 |

**New specs (to write).**

| ID | Spec file | Governs | Needed by |
|---|---|---|---|
| #24 | `serial-binary-format.md` (binary format, schema hash #36, migrations, compression container) | `exitium_serial` | Intake 1 |
| SP1 | ADR `docs/decisions/NNNN-luau-on-wasm.md` (the **R5 spike**) | none | Luau spec |
| SP9 | `luau-scripting.md` (mlua, bindings from reflection, tiers, hot reload, command-bus API, wasm per SP1) | `exitium_script` | Intake 2 |
| SP10 | `data-assets-prefabs.md` (data assets, variants, prefab variants in scenes) | `exitium_data`, `exitium_scene` | Intake 2 |
| SP11 | `tilemap.md` (all three modes, autotile, chunks, colliders, editor tool surface) | `exitium_tilemap` | Intake 3 |
| SP12 | `render-2d-breadth.md` (pixel-art pipeline, palette swap, Aseprite import, normal-mapped lighting, camera modes, depth sorting) | `exitium_render`, `exitium_assets` | Intake 3 |
| SP13 | `sim.md` (fixed high-rate ticks, producers, modifiers, offline progress, prestige, automation) | `exitium_sim` | Intake 3 |
| SP14 | `saves.md` (versioned saves, migrations, zstd, tamper detection, autosave, cloud provider trait, world snapshot per C7) | `exitium_save` | Intake 3 |
| SP15 | `steam.md` (Steamworks init, achievements, cloud provider, overlay; licence ADR) | `exitium_steam` | Intake 3 |
| SP16 | `net-server.md` (transport for native and web, authoritative client and server, replication through reflection, the headless runtime; transport ADR) | `exitium_net`, `exitium_server` | Intake 3 |
| SP17 | `backend.md` (auth, cloud saves, leaderboards, offline validation, hosting adapters; topology ADR) | `exitium_backend` | Intake 3 |
| SP18 | `export-examples.md` (the `examples/` layout, exporting an in-workspace project, Steam and itch packaging, the six games' acceptance criteria) | `exitium_cli` | Intake 3 |

Spec dependencies (R4): SP9 after SP1. SP10 after SP4. SP11 after SP5. SP12 after SP5 and SP4 (same crates, R1). SP14 after #24. SP16 after #24 and SP1 (ADR order, C9). SP15 after SP14 and SP16, SP17 after SP13, SP14 and SP15, so that ADR holders run one at a time (C9). SP8 after SP6, SP10, SP11 and SP12. SP18 after SP15 and SP17.

## Restored scope

The restore list: every row of jam-mvp.md "Scope cuts", with where it returns. "Spec" is the owning spec or amendment. Issue numbers are the existing issues.

| Jam cut | Returns in | Spec | Notes |
|---|---|---|---|
| Luau, `exitium_script`, mlua, R5 spike | Spike in Stage 0, scripting in Stage 3 | SP1, SP9 | The spike is the first item started. |
| Binary format, compression, migrations (#24, #56 to #60) | Stage 1 serial lane | #24 | |
| Stable schema hash (#36) | Stage 1 reflect lane | #24 | Binary format needs it. |
| ECS world snapshot (#61) | Stage 5, in `exitium_save` | SP14 | Re-homed by C7. Retitle when filing. |
| Parallel executor (#48), `par_iter_mut` | Stage 1, ECS lane tail | SP2 | After the tasks lane. |
| `exitium_tasks` (#26 to #30) | Stage 1, own lane | SP2 | Edge `ecs -> tasks` in intake 1. |
| 1M-entity benches (#52, #67) | #52 Stage 1; #67 Stage 5 | SP2, SP14 | #67 needs ECS and serial, so it moves with #61 (C7). |
| Min-spec bench profile (#66), Tracy (#62) | Stage 1 | SP2 | |
| Frame stats (#63), counting allocator (#64), executor spans (#65) | Stage 1 (#65 at the tail) | SP2 | #64 is a designated `unsafe` module with Miri tests. |
| Data assets and prefab variants | Stage 2 | SP10 | |
| Asset hot reload | Stages 1 and 2 | SP4 | |
| Gamepad | Stages 1 and 2 | SP3 | |
| Tilemaps | Stage 4 | SP11 | |
| Pixel art, lighting, camera and game modes | Stage 4 | SP5, SP12 | |
| Physics debug draw and tooling | Stage 4 | SP7 | Debug draw data (P7, P14) already sit in the physics spec. |
| UI binding performance, markup, themes, widgets, immediate mode | Stage 5 | SP6 | |
| Editor extras, offline MCP tools | Stage 5 | SP8 | |
| Parity test | Stage 3 (XT2), grown every stage | SP8 | |
| Networking, server, backend | Stage 6 | SP16, SP17 | Scale-out stays out (epic #11). |
| Sim | Stage 5 | SP13 | |
| Saves | Stage 5 | SP14 | |
| Steam | Stage 6 | SP15 | |
| itch.io packaging, release workflows | Stage 6 | SP18 | Workflow files are pushed by the owner (open question 2). |
| Web threads | Open question 1 | none | Proposed: out. |

**Still out, unchanged:** WebGL2 fallback; editor, MCP and CLI on Linux and macOS; Firefox and Safari; network scale-out (epic #11, Phase 9); destruction (epic #11); the Glutton repo (epic #9). For the first two and Firefox/Safari the path stays open because `NATIVE_ONLY` already separates native-only crates. Open chores (#84, #96 to #102, #105, #125, #126) stay on the normal backlog and are placed into gaps.

**Epic mapping.** Epic #3 is Stage 1. Epic #4 is Stages 2 and 3, with the R5 spike in Stage 0. Epic #5 is Stages 1 and 5. Epic #6 is Stages 3 and 5. Epic #7 is Stage 5. Epic #8 is Stage 6. Epic #10 is Stage 4. Epic #9 (Glutton) and epic #11 stay out.

## Stages

### Intake PRs

There are three. Each runs alone at a stage boundary, holds the hotspots `Cargo.toml`, `Cargo.lock` and `deny.toml`, and adds everything its span needs, so lane issues never touch them.

| Intake | Span | Adds (known today; the union of the merged specs' wiring is authoritative, C5) | Blocked by |
|---|---|---|---|
| 1 | Stage 1 | Crates.io dependencies of Stage 1 (wgpu, winit, taffy, rapier2d, kira, image, font and text crates, `gilrs`, the file-watch choice, `tracy-client`, the `triple_buffer` licence exception and its ADR). The new crate `exitium_ecs_derive` (C8). The edges in the Stage 1 graph. **`exitium_ecs -> exitium_tasks`** and `exitium_ecs -> exitium_profiler`. The `profiling`, `tracy` and allocator features of `exitium_profiler`. | #24, SP2 to SP6, and the Jam-era Stage 1 specs (all merged) |
| 2 | Stages 2 and 3 | Dependencies of Stages 2 and 3 (egui family, rmcp, tokio, serde, `mlua` per SP1, the wasm-bindgen additions, zip). The new crate `exitium_command` (C8). `exitium_mcp` in `NATIVE_ONLY`. The plugin-feature edges, `scene -> data` (E1), the script, command, editor and mcp edges, and the facade edges. | Intake 1, every Stage 1 lane issue, #110, SP9, SP10 |
| 3 | Stages 4 to 7 | Dependencies of Stages 4 to 7 (an Aseprite parser, compression, authenticated encryption, Steam bindings, a net transport, the server and backend stack). The edges `tilemap`, `sim`, `save`, `steam`, `net`, `server`, `backend` and the editor additions (C3). The six example crates and the `examples/*` workspace members (C8). Any `NATIVE_ONLY` or `FEATURE_CHECKS` entries. Any ADR exceptions. | Intake 2, every Stage 2 and 3 lane issue, SP7, SP8, SP11 to SP18 |

### Stage 0: specs and the R5 spike

**Work.** Nineteen spec issues: the amendments SP2 to SP8, the new specs #24, SP1, SP9 to SP18, and this file (#129). Jam's nine Stage 0 issues are all merged.

**Lane layout.** Three waves, ordered by `Blocked by`. Within a wave all issues are docs-only and edit one exact file.

- **Wave 1 (eight at once):** SP1 (the spike, started first), #24, SP2, SP3, SP4, SP5, SP6 and SP13.
- **Wave 2:** SP9 (after SP1), SP10 (after SP4), SP11 (after SP5), SP12 (after SP4 and SP5), SP14 and SP16 (after #24), SP7.
- **Wave 3:** SP15, then SP17 (the ADR holders in order, C9), SP8 (after SP6, SP10, SP11, SP12), then SP18.

**The spike (SP1).** Luau is C++, and `wasm32-unknown-unknown` has no C or C++ standard library, so `mlua` with a vendored Luau may not build for the web. The spike's code is throwaway, kept outside the repo. Its PR is an ADR with measurements:

1. Does a documented recipe build `mlua` (Luau) for `wasm32-unknown-unknown` on stable Rust?
2. If not, which of these works: a wasm clang target with a minimal libc and libc++ subset; a separate Luau wasm module bridged from JS; another route.
3. For the winner: gzip size, a hello-world script running in Chrome, and the cost of one Lua to Rust call.

If no route works, the ADR says so and lists the choices: scripts native-only for the MVP (a scope cut that needs the owner), or a different VM. That decision is the owner's (open question 3). Nothing that depends on Luau is filed before this ADR merges.

**Why it passes.**
- R1: each issue edits one exact file under `docs/specs/` (SP1 edits one new ADR). `Governs` sets in the same wave are pairwise disjoint: wave 1 governs none, serial, ecs+tasks+profiler, platform, assets, render, ui, sim. Wave 2: script, data+scene, tilemap, render+assets, save, net+server, physics. Wave 3: steam, backend, editor+mcp+command, cli.
- R2: only SP1, SP15, SP16 and SP17 touch a hotspot (`docs/decisions/`). They are ordered by `Blocked by` (SP1 in wave 1, SP16 in wave 2, SP15 then SP17 in wave 3), so never two at once.
- R3: no workspace crate depends on another workspace crate today (no `Cargo.toml` has an `exitium_` dependency), and every spec issue declares `New crate edges: none`.
- R4: the only dependencies are the `Blocked by` lines under "Spec dependencies", which is why they sit in later waves. SP12 is after SP4 and SP5 by R1 as well (same crates).

**Stage 0 is rolling.** Intake 1 needs only #24 and SP2 to SP6, plus the spike running. Intake 2 needs SP9 and SP10. Intake 3 needs the rest. Stage 1 starts when intake 1's specs have merged.

**Exit.** #24 and SP1 to SP6 merged, and the other specs merged before the intake that needs them. The `needs-spec` labels are lifted from the dependent issues as each spec merges (#48, #54, #56 to #59, #61 and the issues each new spec files).

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
- R2: after intake 1 no lane issue changes a dependency table. Examples and benches live in the crate directory. The two hotspot items, #66 (`xtask/`) and #65 (a feature edge in `Cargo.toml`), run one at a time, never beside intake 1 or each other.
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

**Work.** The command spine, then four lanes beside each other, then the facade hook and the end-to-end tests:

1. **Spine: the command bus and its wire types** (`exitium_command`, CB1 to CB12, about 12 PRs).
2. **Editor lane** (`exitium_editor`, ED1 to ED13, about 13): the live-control server, headless runner, scene and asset tools, play and pause, the egui shell, the inspector from reflection.
3. **MCP lane** (`exitium_mcp`, MC1 to MC3, then the export tool MC4, about 4): the protocol client, live tools, ops tools. Tools are built from the schema the running game reports (C4).
4. **Script lane** (`exitium_script`, about 12): the Luau VM wrapper, the reflection bindings, the safe and privileged tiers, the command-bus API, hot reload, the wasm build per SP1, benches. Script systems run exclusively (the Lua state is not `Sync`).
5. **CLI lane** (`exitium_cli`, C1 to C5, about 5): `exitium new`, export staging, `export --target windows`, `export --target web`, `serve`.
6. **Command surfaces** (about 8, one small PR per crate after the spine): physics P15 and P16, UI U13, and the attribute PRs for `exitium_app`, `exitium_render`, `exitium_audio`, `exitium_assets` and `exitium_platform`.
7. **Facade hook** (about 3): drain the command bus, the `editor` feature and headless mode, the script glue (sources through the asset server, reload events).
8. **End-to-end and parity tests** (3, in `exitium_editor`'s tests): headless end-to-end through the MCP subprocess, the parity test, a scripted-scene test.

**Why it passes.**
- Spine: editor, MCP, script and the command surfaces all depend on `exitium_command`, so by R3 and R4 they wait for the whole spine. The spine is a serial chain in one crate.
- Lanes: editor, MCP, script and CLI are separate crate directories (R1) with no hotspot (R2: egui, rmcp, mlua and the edges came in intake 2). `exitium_mcp` depends only on command, the editor does not depend on MCP (C3), script has no edge to editor or MCP (C1), and the CLI depends on serial only, so none edits a crate that depends on another's (R3). The editor and MCP share only the wire types frozen in the spine (R4).
- The editor and MCP lanes stay independent only if adding an editor command never requires an edit to `exitium_mcp`. This is why tools are built at runtime from the schema (C4). The same holds for script bindings.
- Command surfaces: each edits one plugin crate that depends on command. They run after the spine, beside the editor lane (no edge from the editor to physics or ui yet in Stage 3: intake 2 declares those edges in intake 3).
- Facade hook: the facade depends on the editor, MCP-free, so it runs alone after the editor lane. Tests live in the editor crate, so they run after the editor lane, and start the MCP binary as a subprocess (no dev-dependency).

Critical path: command spine (12), editor lane (13), facade hook (2), tests (2): about 29 serial PRs. The script and CLI lanes are shorter than the editor lane.

**Exit.** An agent builds a scene through MCP (spawn entities, set components, attach a Luau script, save the scene, take a screenshot). A human opens the same scene in the editor, edits it and saves it. The script hot-reloads on Windows. The scripted sample runs in Chrome with Luau, or the spike's fallback is recorded and accepted by the owner. `exitium export --target windows|web` and the MCP export tool produce a Windows exe and a web build of the Stage 2 sample, and the web build runs in Chrome. The parity test runs in CI.

### Stage 4: 2D breadth

**Work.** First intake 3, then lanes:

1. **Intake 3** (see the table above).
2. **Render v2 lane** (`exitium_render`, about 14): virtual resolution and integer scaling, sub-pixel camera, palette swap and indexed colour, pixel-perfect rotation, normal-mapped lights, top-down, sidescroller and isometric camera modes with depth sorting and height levels, goldens.
3. **Aseprite import** (`exitium_assets`, about 3).
4. **Tilemap lane** (`exitium_tilemap`, about 10): chunk storage, autotile rules, the three modes, the plugin, collider output, text round trip, benches.
5. **Physics tooling** (`exitium_physics`, about 3): character controller, tile-collider merging, debug draw surfaces.
6. **Editor breadth** (`exitium_editor`, about 6): the tilemap tool, light and camera gizmos, physics debug overlay, Aseprite preview, and the matching commands. Tools come through the generic MCP path (C4); one small MCP PR if a tool needs an ops entry.
7. **Facade glue** (about 6): tilemap into render and physics, lights, camera modes, depth sorting, re-exports.
8. **Four sample scenes** in `crates/exitium/examples/` (about 4): pixel art, top-down, sidescroller, isometric. Each is a facade-lane PR.

**Why it passes.**
- Intake 3 runs alone at the Stage 3 to Stage 4 boundary (R1 to R3, as before) and creates the example crates (C8).
- Render v2, assets, tilemap and physics tooling are four crate directories (R1) with no edge among them (C1; tilemap talks to render and physics only through the facade). So they run together.
- The editor depends on render, assets, physics and tilemap (C3), so editor breadth waits until those lanes finish (R3). Facade glue and samples wait for the editor lane (R3).
- UI full (Stage 5) has no edge to these four crates, so it may start now (stage-order note above).

Critical path: intake 3 (1), render v2 (14), editor breadth (6), facade glue (6), samples (4): about 31 serial PRs.

**Exit.** The four sample scenes run on Windows and in Chrome. Each uses a tilemap in the right mode, lighting where it fits (not the pixel-art scene's palette-swapped sprites), physics, and the matching camera mode. A tilemap is built by an agent through MCP and edited in the editor with the tilemap tool. Render goldens pass. The budgets below hold.

### Stage 5: sim, saves, full UI and editor extras

**Work.**

1. **Sim lane** (`exitium_sim`, about 10): fixed high-rate ticks, producer and consumer patterns, modifier pipelines, offline progress (analytic and chunked fast-forward, time cap, validation hooks), prestige layers, automation, benches.
2. **Save lane** (`exitium_save`, about 11): the container, versioned saves, migration chains, zstd, authenticated tamper detection, autosave, the cloud provider trait, the ECS world snapshot (#61) and the 1M-entity round trip (#67), corruption tests.
3. **UI full lane** (`exitium_ui`, about 14): markup format and hot reload, themes, the remaining widgets, change-only data binding, the immediate-mode layer, the 10k-node and rebinding benches.
4. **Editor extras** (`exitium_editor`, about 12): play-in-editor with snapshot restore, the Data Asset editor, the UI editor, the profiler panel.
5. **MCP offline tools** (`exitium_mcp`, about 3): docs and API query, schema dump.
6. **Facade glue** (about 5) and **two samples** (2): an incremental sample with prestige, offline gains and saves, and the heavy-numbers UI mockup.

**Why it passes.**
- Sim, save and UI are three crates with no edge among them (C1). Sim and save depend on ecs and serial (frozen); neither edits a crate the other depends on. They run together and beside the editor lane only where the editor has no edge to them (it has none to sim or save).
- Editor extras depend on `exitium_ui` and `exitium_data` (C3), so they start after the UI full lane. The profiler panel needs `exitium_profiler`, frozen since Stage 1.
- MCP offline tools edit `exitium_mcp`, which depends only on command, so they run beside the editor lane.
- Facade glue and samples run alone at the end.

Critical path, counted from the end of Stage 4 and assuming the UI lane overlapped Stage 4: editor extras (12), facade glue (5), samples (2): about 19 serial PRs. Without that overlap, add 14.

**Exit.** The incremental sample (prestige, offline gains) saves, migrates through two schema versions, and reloads correctly, on Windows and in Chrome. A 1M-entity world round-trips through both formats (#67). The UI mockup with 10,000 live counters holds its budget. The editor opens a Data Asset and a UI layout, edits them, and plays in the editor. The offline MCP tools answer from a cold start. The parity test covers every new command.

### Stage 6: platform and online

**Work.**

1. **Steam lane** (`exitium_steam`, about 6): Steamworks init, achievements, Steam cloud as a save provider (E2), overlay, a test app.
2. **Net lane** (`exitium_net`, about 10): the transport on native and web, an authoritative client and server, replication of components marked through reflection, interpolation, input commands.
3. **Server lane** (`exitium_server`, about 6): the headless runtime (E4), stateless and horizontally scalable, hosting adapters.
4. **Backend lane** (`exitium_backend`, about 14): auth, cloud saves, leaderboards, server-side offline-gain validation reusing `exitium_sim` (E3), a web-capable client, load tests.
5. **CLI packaging** (`exitium_cli`, about 3): the Steam depot layout and the itch.io zip, from the export staging.
6. **Facade glue** (about 6): the net client and server plugins, Steam and backend plugins, `ServerPlugins`.
7. **Release workflows** (2): written by an agent, **pushed by the owner** (the factory App cannot push `.github/workflows/`). Open question 2.

**Why it passes.**
- Steam, net, server, backend and the CLI are five crate directories (R1). The only edges are E2 (steam to save), E3 (backend to sim and save) and E4 (server to net). Save and sim are frozen after Stage 5, and `exitium_net` is not touched by the server lane's peers except server itself, so the net lane runs before the server lane (R3).
- Steam, backend and net start together once Stage 5 has merged. The server lane waits for net.
- The CLI depends on serial only (R3), so packaging runs beside all of them.
- Facade glue runs alone at the end (R2, R3).

Critical path: net (10), server (6), facade glue (6): about 22 serial PRs. The backend lane (14) runs beside it.

**Exit.** A sample game syncs its save to the backend, posts a leaderboard score, and has its offline gains validated by the server, from Windows and from Chrome. A networked sample runs with a headless server and at least two clients, with replicated and interpolated entities. The Steam path works by hand on Windows with Valve's test app: init, one achievement, one cloud save round trip. The server and backend run from a documented command on a clean checkout. The Steam depot layout and the itch zip are produced by the CLI.

### Stage 7: rehearsal games in `examples/`

**Work.** Intake 3 created six crates in the repo root. Each game is built by an agent through MCP, Rust and Luau, refined by a human in the editor, and exported to a Windows exe and to the web:

| Example | Covers |
|---|---|
| `examples/topdown` | top-down camera, tilemap, lighting, gamepad, Luau gameplay |
| `examples/sidescroller` | sidescroller camera, tilemap, character controller, audio |
| `examples/isometric` | isometric tilemap, depth sorting, height levels |
| `examples/pixel_art` | virtual resolution, integer scaling, palette swap, Aseprite animation |
| `examples/incremental` | sim, prestige, offline gains, saves, full UI, **cloud saves, leaderboards, offline validation, Steam** |
| `examples/netplay` | `exitium_net`, `exitium_server`, interpolation, a web client |

About 4 PRs per game, about 24 in total. Engine-side output is gap issues and fixes, about 12, each an ordinary filed issue that goes through the same check.

**Why it passes.** Each example crate depends only on the facade (C1). Two examples have no edge to each other (R3), and an example never edits an engine crate. A gap fix edits an engine crate, so it conflicts with every example (R3, through the facade) and runs alone. Example PRs edit `examples/<name>/`. Under today's factory rules that path is a hotspot (R2), so examples would run one at a time. The housekeeping PR "treat `examples/<name>/` as an Edits entry" lifts that and lets three run together. With it the stage is about 15 serial PRs, without it about 34.

**Exit = full MVP.** All six games exist as a Windows exe and as a web build that runs in Chrome. Each was built through MCP, scripted in Luau and refined by a human in the editor. The online path works (cloud saves, leaderboards, offline validation, networked play) from both targets. The exports were produced by the CLI and by the MCP tool on a clean checkout. Every gap issue the rehearsal found is merged or filed with `status:needs-approval`.

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

- **ECS.** The scheduler has a single-threaded executor and a parallel executor on `exitium_tasks` (restored). The min-spec profile runs 4 threads. The web build is single-threaded (the tasks wasm fallback, #29), whatever open question 1 decides.
- **Script.** The Luau state lives on the main thread. Script systems are exclusive (no parallel system runs beside them). Hot reload swaps a chunk at a frame boundary.
- **Editor.** The live-control server is an extra OS thread. It never touches the world. It queues requests, and the main loop drains them at one fixed point in the frame.
- **Assets, saves, audio.** Native worker threads decode assets and write saves. Autosave compression runs off the main thread, and on the web in slices (no threads).
- **Net and server.** The transport runs on its own threads or runtime, and hands packets to the main loop at one fixed point, the same way as the live-control server. The server is the main thread plus a worker pool.
- **MCP** is a separate process. The backend is a separate process. Neither shares the game's world or frame budget.

## MCP / editor surface

This is a roadmap, so it adds no tool itself. It fixes when each part of the agent-parity rule is met:

- Stages 1 and 2 add **no editor feature**, so they add no MCP tool. Components and resources become visible to MCP automatically, because they derive `Reflect` (design rule 2).
- Stage 3 delivers parity: every mutation is a command on the bus (design rule 3), the editor, MCP and Lua are three front ends of the same registry, and the tools, panels and bindings are built generically from the reflected surface (C4). The parity test enforces the rule in CI from then on, and every later stage grows it.
- Stage 3 also adds the export capability and its MCP tool together (CLI and `exitium_mcp`).
- Stage 4: tilemap paint and fill, light and camera settings, palette and Aseprite import are commands. Their editor tools and MCP tools arrive together.
- Stage 5: sim inspection, save list, load, export and migrate, Data Asset edit, UI layout edit, play-in-editor control and the offline tools are commands or queries, with matching editor panels.
- Stage 6: net status, server control, backend login and leaderboard queries, Steam status are commands or queries. Secrets are never exposed by a tool.
- Any editor feature added later ships with its MCP tool, as CLAUDE.md requires.

## Performance

**Every number in this section is a proposal.** The owner approved the plan, not these figures. Open question 7 asks the owner to confirm them. Crate specs own the micro-budgets and may only tighten these. Any hot-path PR ships with a benchmark, and CI fails on a regression over 5% (design rule 4). "Not optimized" (decision 4) means the 2D-breadth budgets are floors to reach, not targets to beat.

**Factory budgets (proposed).**

| Item | Budget |
|---|---|
| PR size | about 600 changed lines or fewer |
| In flight | at most 8, and only conflict-free (CLAUDE.md) |
| Total | about 440 PRs (plus or minus 30%) |
| Critical path | about 190 serial PRs |
| Elapsed | 16 to 27 days of factory time |

Allocation of PRs (an estimate; the specs' own "Implementation issues" sections replace these counts once filed):

| Stage | PRs | Shape |
|---|---|---|
| 0 | about 20 | 19 spec issues, plus this file |
| 1 | about 112 | intake 1, ECS 28, serial 17, reflect group 7, tasks 5, profiler 4 plus #65, platform 7, render 13, physics 7, audio 3, assets 9, UI 10, xtask 1 |
| 2 | about 58 | intake 2, spine 4, data 6, scene 8, plugins about 22, facade about 17 |
| 3 | about 60 | command 12, editor 13, MCP 4, script 12, CLI 5, surfaces 8, facade 3, tests 3 |
| 4 | about 48 | intake 3, render v2 14, Aseprite 3, tilemap 10, physics 3, editor 6, facade 6, samples 4, MCP 1 |
| 5 | about 57 | sim 10, save 11, UI 14, editor 12, MCP 3, facade 5, samples 2 |
| 6 | about 47 | Steam 6, net 10, server 6, backend 14, CLI 3, facade 6, workflows 2 |
| 7 | about 38 | six games 24, gap fixes about 12, docs 2 |
| Total | about 440 | |

Critical path, in serial PRs (each stage counted from its own intake or spine):

| Stage | Serial PRs | How |
|---|---|---|
| 0 | 5 | #24, SP16, SP15, SP17, SP18 (ADR order) |
| 1 | 37 | intake 1, reflect group 7, ECS 28, #65 |
| 2 | 32 | intake 2, data then scene 14, facade 17 |
| 3 | 29 | command 12, editor 13, facade 2, tests 2 |
| 4 | 31 | intake 3, render v2 14, editor 6, facade 6, samples 4 |
| 5 | 19 | editor 12, facade 5, samples 2 (UI overlaps Stage 4) |
| 6 | 22 | net 10, server 6, facade 6 |
| 7 | 15 | three games at a time (34 without the factory-rule change) |
| Total | about 190 | |

The 16 to 27 day estimate (about 380 to 660 hours) holds only if one serial PR (implement, test, review, merge) takes 2 to 3.5 hours. If PRs take longer, elapsed time grows with the critical path, not with the PR count. Review rounds, CI queueing and the 5% usage gate are not modelled, so treat the estimate as a floor.

**Runtime exit budgets (proposed ceilings; min-spec 4 cores and 8 GB; Windows and Chrome).** The first block carries over from Jam.

| Where | Budget |
|---|---|
| Sample game (Stage 2) | at least 60 FPS, p99 frame at most 20 ms, with 2,000 sprites, 200 dynamic bodies and 100 UI nodes |
| Sample game | first frame at most 3 s on Windows (release), at most 5 s in Chrome from a warm cache |
| Web build | wasm at most 10 MB gzipped without scripting, at most 13 MB with Luau, assets excluded |
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
| Lighting | 32 visible normal-mapped lights at 1080p at most 4 ms GPU on the reference GPU (open question 8) |
| Depth sorting | 10,000 sprites sorted by depth key at most 0.5 ms |
| UI full | 10,000-node cold layout at most 6 ms (UI spec); rebinding 10,000 counters changing every frame at most 1.5 ms CPU; 100 changed bindings at most 20 us; markup parse of 1,000 nodes at most 3 ms; markup hot reload at most 100 ms |
| Editor | 60 FPS with 10,000 entities in the hierarchy (virtualised); an inspector edit applied and acknowledged in at most 16 ms |
| Sim | 100,000 producers ticked at most 3 ms per tick on one thread, 1.2 ms on 4 threads; 8 h offline catch-up at most 50 ms (analytic) or 1.5 s (chunked), default time cap 24 h |
| Saves | encode 1 MB of reflected state at most 5 ms, compress at most 8 ms, load and migrate at most 15 ms; autosave never makes a frame longer than 20 ms |
| Steam | init at most 500 ms; an achievement unlock call at most 0.1 ms on the main thread |
| Net | a 32-client, 200-entity world ticks at most 4 ms on the server at 60 Hz; at most 20 KB/s per client at 20 snapshots/s; default interpolation delay 100 ms |
| Server | idle memory at most 50 MB; startup at most 1 s |
| Backend | on 2 vCPUs, leaderboard submit at most 50 ms p95 and top-100 read at most 30 ms p95 at 500 requests/s; offline validation of 8 h at most 100 ms |
| Examples (Stage 7) | each game at least 60 FPS, p99 frame at most 20 ms, on Windows and Chrome; wasm at most 13 MB gzipped |

## Test plan

Each stage has one exit check, and the stage is not done until it passes:

- **Stage 0:** each spec merges after the AI review. The spike's ADR contains the measurements listed above.
- **Stage 1:** `cargo xtask ci all` green on `main`. Each backend example runs on Windows and its PR records that. The ECS, tasks and format benchmarks show no regression. The golden files for the text and binary formats pass. The min-spec profile runs.
- **Stage 2:** the sample game runs on Windows for a fixed number of frames, with a render-to-texture screenshot compared to a stored image. It also runs once by hand in Chrome. The PR records both. The facade tests that plugin components and a prefab variant round-trip through a scene, and that a changed texture on disk reloads.
- **Stage 3:** tests in `exitium_editor` start a headless app with the live-control server and the `exitium_mcp` binary as a subprocess, build a scene with a script through MCP tool calls, save it, load it back and compare. The parity test runs in CI. The two exports run on a clean checkout and the web build runs in Chrome.
- **Stage 4:** render goldens for each camera mode, lights, palette swap and integer scaling. The four sample scenes run on Windows and in Chrome. A tilemap built through MCP equals one built in the editor.
- **Stage 5:** a save written by schema version 1 loads in version 3 through two migrations. Corruption and tamper tests reject bad saves. The 1M-entity round trip (#67). The UI and sim benches hold their budgets. Play-in-editor restores the world on stop.
- **Stage 6:** an integration test starts the backend and a headless server on localhost, then runs two clients (one native, one headless test client) through login, a cloud save, a leaderboard post and a validated offline claim. Steam is checked by hand (Valve's client is required, so CI cannot run it). A load test records the backend budgets.
- **Stage 7:** the rehearsal is the test. All six exports are produced by the CLI and by the MCP tool, started on a clean checkout, and all of them run in Chrome and on Windows.

The wasm build covers the runtime crates in CI (`cargo build --workspace --target wasm32-unknown-unknown`, excluding `NATIVE_ONLY`). `exitium_steam`, `exitium_server` and the server half of `exitium_backend` go into `NATIVE_ONLY`, and the backend's web client must still build for wasm (open question 6). Running in a browser is checked by hand in Chrome only.

## Open questions

Each is for the architect to settle when the named spec merges, or for the owner. None blocks this PR.

1. **Web threads (owner).** The Jam cut list named "web threads". They need `-C target-feature=+atomics` and `build-std`, which is nightly, and CLAUDE.md pins stable Rust. Proposed: out of the MVP. The web build stays single-threaded (tasks wasm fallback). Confirm.
2. **Release workflows (owner).** Steam depot upload and the itch butler push are GitHub workflows. The factory App cannot push `.github/workflows/`, so the owner pushes them. Proposed: Stage 6 delivers packaging in the CLI and drafts the workflows in a PR, and the owner pushes them. Confirm that these are in the MVP.
3. **R5 fallback (owner).** If the spike finds no way to run Luau on `wasm32-unknown-unknown`, which is acceptable: scripts native-only (web games run without scripts, and the web exports of the scripted examples change), a different VM, or a bridge to a separate Luau wasm module? SP1 gives the data. The owner decides.
4. **Steam licence (owner, SP15).** The Steamworks SDK is proprietary and cannot be redistributed in the repo. SP15 writes an ADR: how the SDK is supplied, whether CI builds without it, and `deny.toml` handling of the bindings crate.
5. **Facade as the bottleneck.** The facade chain is about 45 PRs across the stages, each running alone with the whole factory drained. Mitigation in this plan: one facade tail per stage and batching glue per stage. A larger change would add "glue crates" (say, one per pair of leaf crates) so glue stops being a hotspot. That breaks the C1 model and is not proposed. Confirm the cost is acceptable.
6. **Backend client on the web.** `exitium_backend` has a server half (native) and a client half (must build for wasm). SP17 decides one crate with features, or two crates. Two crates add a stub in intake 3.
7. **Proposals for the owner to confirm.** The PR counts, the 16 to 27 day estimate and every budget in "Performance". In particular the sim, net and backend budgets, which have no earlier basis.
8. **Reference GPU.** The lighting budget needs a named reference GPU (min-spec is stated as 4 cores and 8 GB, with no GPU). SP12 proposes one.
9. **`scene -> data` (E1).** SP10 may remove the edge by putting the override engine in `exitium_reflect` or `exitium_serial`. If it does, the data and scene lanes run side by side and Stage 2 is about 6 PRs shorter.
10. **`docs-graph` cadence.** Running it after every merge serializes a lane's next issue. Proposal (carried from Jam): at the end of each lane and at each stage boundary. That is a factory-rule change, so its own PR.
11. **Windows CI.** The editor, CLI, Steam and server crates are `NATIVE_ONLY`. Confirm CI runs their tests on a Windows runner. Workflow changes are owner-pushed.
12. **Zstd on wasm.** The `zstd` crate wraps C code and shares R5's toolchain trouble. SP14 chooses between it, a pure-Rust decoder, and LZ4 on the web.

## Implementation issues

This roadmap has no code. Its work items are the issues below. Each is PR-sized (about 600 changed lines or fewer), has a `## Footprint`, acceptance criteria and `Blocked by` lines, and goes into the MVP milestone. After this spec merges, the architect files the missing ones with the `file-issue` skill. The three intake issues are the only ones allowed to edit more than one crate.

**Already filed.**
- Stage 0: #24 and this file (#129). Jam's #22, #23, #106 to #112 are merged.
- Stage 1, Jam-era (now blocked by intake 1 once filed): reflect #31 to #33, #36, #37; derive #34, #35; ECS #38 to #52; serial #53 to #61; profiler #62 to #65; xtask #66; #67; tasks #26 to #30.

**To file now (from this spec).**

1. **Spec issues SP1 to SP18** (table below). Common Footprint: `Hotspots: none` and `New crate edges: none`, except SP1, SP15, SP16 and SP17, which list `Hotspots: docs/decisions/`. `Edits` is one exact file (SP1: its new ADR).

   | ID | Title | Edits | Governs | Blocked by |
   |---|---|---|---|---|
   | SP1 | docs(adr): Luau on wasm, the R5 spike | `docs/decisions/NNNN-luau-on-wasm.md` | none | none (start first) |
   | SP2 | docs(spec): ECS parallel executor, tasks, profiler amendments | `docs/specs/ecs-scheduler.md`, `docs/specs/ecs-storage-queries.md` | `crates/exitium_ecs/`, `crates/exitium_tasks/`, `crates/exitium_profiler/` | none |
   | SP3 | docs(spec): platform gamepad amendment | `docs/specs/app-platform.md` | `crates/exitium_platform/` | none |
   | SP4 | docs(spec): assets hot reload amendment | `docs/specs/assets-scene-export.md` | `crates/exitium_assets/` | none |
   | SP5 | docs(spec): render v2 hooks amendment | `docs/specs/render-v1.md` | `crates/exitium_render/` | none |
   | SP6 | docs(spec): full UI amendment | `docs/specs/ui-mvp.md` | `crates/exitium_ui/` | none |
   | SP7 | docs(spec): physics debug draw and tooling amendment | `docs/specs/physics-2d.md` | `crates/exitium_physics/` | none |
   | SP8 | docs(spec): editor extras and offline MCP amendment | `docs/specs/editor-mcp-mvp.md` | `crates/exitium_editor/`, `crates/exitium_mcp/`, `crates/exitium_command/` | SP6, SP10, SP11, SP12 |
   | SP9 | docs(spec): Luau scripting | `docs/specs/luau-scripting.md` | `crates/exitium_script/` | SP1 |
   | SP10 | docs(spec): data assets and prefab variants | `docs/specs/data-assets-prefabs.md` | `crates/exitium_data/`, `crates/exitium_scene/` | SP4 |
   | SP11 | docs(spec): tilemap | `docs/specs/tilemap.md` | `crates/exitium_tilemap/` | SP5 |
   | SP12 | docs(spec): pixel art, lighting and camera modes | `docs/specs/render-2d-breadth.md` | `crates/exitium_render/`, `crates/exitium_assets/` | SP4, SP5 |
   | SP13 | docs(spec): incremental sim | `docs/specs/sim.md` | `crates/exitium_sim/` | none |
   | SP14 | docs(spec): saves | `docs/specs/saves.md` | `crates/exitium_save/` | #24 |
   | SP15 | docs(spec): Steam | `docs/specs/steam.md` (and an ADR) | `crates/exitium_steam/` | SP14, SP16 |
   | SP16 | docs(spec): net and server | `docs/specs/net-server.md` (and an ADR) | `crates/exitium_net/`, `crates/exitium_server/` | #24, SP1 |
   | SP17 | docs(spec): backend | `docs/specs/backend.md` (and an ADR) | `crates/exitium_backend/` | SP13, SP14, SP15 |
   | SP18 | docs(spec): export of `examples/` | `docs/specs/export-examples.md` | `crates/exitium_cli/` | SP15, SP17 |

   #24 also needs its Footprint filled: `Edits: docs/specs/serial-binary-format.md`, `Governs: crates/exitium_serial/`, `Hotspots: none`, `New crate edges: none`.

2. **Intake 1: chore(deps): dependencies and the Stage 1 crate graph.** Blocked by #24, SP2 to SP6.
   ```
   ## Footprint
   - Edits: crates/exitium_ecs/, crates/exitium_ecs_derive/, crates/exitium_reflect/, crates/exitium_reflect_derive/, crates/exitium_serial/, crates/exitium_tasks/, crates/exitium_profiler/, crates/exitium_platform/, crates/exitium_render/, crates/exitium_physics/, crates/exitium_audio/, crates/exitium_assets/, crates/exitium_ui/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock, deny.toml, xtask/, docs/decisions/, crates/exitium/
   - New crate edges: the union of the wiring sections of the merged Stage 1 specs (C5). At least the Stage 1 graph above, including exitium_ecs -> exitium_tasks and exitium_ecs -> exitium_profiler
   ```
   Acceptance: every Stage 1 crates.io dependency is in `[workspace.dependencies]` with a one-line comment (licence, wasm, why not std); `exitium_ecs_derive` exists (C8); `cargo deny check` passes; `cargo xtask ci all` passes; no edge among leaf crates (C1) and none from `exitium_serial` to `exitium_ecs` (C7); every later Stage 1 issue can be done without editing `Cargo.toml` or `Cargo.lock`.

3. **Intake 2: chore(deps): dependencies and the Stage 2 and 3 crate graph.** Blocked by intake 1, every Stage 1 lane issue, #110, SP9, SP10.
   ```
   ## Footprint
   - Edits: crates/exitium_app/, crates/exitium_platform/, crates/exitium_render/, crates/exitium_physics/, crates/exitium_audio/, crates/exitium_assets/, crates/exitium_ui/, crates/exitium_scene/, crates/exitium_data/, crates/exitium_script/, crates/exitium_command/, crates/exitium_editor/, crates/exitium_mcp/, crates/exitium_cli/, crates/exitium_reflect_derive/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock, deny.toml, xtask/src/ci.rs, crates/exitium/
   - New crate edges: the union of the wiring sections of the merged specs (C5). At least the known edges in "Stage 2", including E1, the facade to every runtime crate and `exitium_command`, and the optional `exitium_editor`
   ```
   Intake 2 holds `crates/exitium/` first. After it merges, each facade issue holds it in turn. Acceptance: creates `exitium_command` and its `CLAUDE.md` (C8); adds `exitium_mcp` to `NATIVE_ONLY`; no leaf edge outside E1 (C1); no edge from `exitium_mcp` to the editor, the facade or a plugin crate, nor from the editor to `exitium_mcp` (C3); `cargo xtask ci all` passes.

4. **Intake 3: chore(deps): dependencies, the Stage 4 to 7 crate graph and the example crates.** Blocked by intake 2, every Stage 2 and 3 lane issue, SP7, SP8, SP11 to SP18.
   ```
   ## Footprint
   - Edits: crates/exitium_tilemap/, crates/exitium_sim/, crates/exitium_save/, crates/exitium_steam/, crates/exitium_net/, crates/exitium_server/, crates/exitium_backend/, crates/exitium_render/, crates/exitium_assets/, crates/exitium_physics/, crates/exitium_ui/, crates/exitium_editor/, crates/exitium_mcp/, crates/exitium_cli/, crates/exitium_reflect_derive/
   - Governs: none
   - Hotspots: Cargo.toml, Cargo.lock, deny.toml, xtask/, docs/decisions/, crates/exitium/, examples/
   - New crate edges: the union of the wiring sections of the merged specs (C5). At least the known edges in "Crate graph" for tilemap, sim, save, steam, net, server, backend and the editor additions, including E2, E3 and E4
   ```
   Acceptance: the six example crates exist as stubs depending on `exitium` only, with `examples/*` as workspace members (C8); `NATIVE_ONLY` and `FEATURE_CHECKS` entries for Steam, server and the backend (C8); any ADR exception recorded; no leaf edge outside E1 to E4 (C1); `cargo xtask ci all` passes.

5. **Per-spec implementation issues.** Each spec's own "Implementation issues" section lists them. Expected lanes: Stage 1 backends, serial, ECS, tasks, profiler; Stage 2 spine, data, scene, plugins, facade chain; Stage 3 command, editor, MCP, script, CLI, surfaces, tests; Stage 4 render v2, tilemap, physics, editor breadth, samples; Stage 5 sim, save, UI full, editor extras; Stage 6 Steam, net, server, backend, packaging; Stage 7 the six games. Their `Blocked by` lines follow the lane orders above, and each is `Blocked by` its intake (C6). Facade issues list `crates/exitium/` under `Hotspots` and `Edits: none`.

6. **Housekeeping.**
   - **Re-milestone** the restored issues #26 to #30, #36, #48, #52, #56 to #67, and #24, to the MVP milestone.
   - Add `Blocked by <intake 1>` to the existing Stage 1 issues (#26 to #67 except the xtask and profiler tail items, which are also `Blocked by` the lane they follow).
   - **Retitle and re-scope** #61 (ECS world snapshot, now in `exitium_save`, Stage 5) and #67 (1M-entity round trip, Stage 5), per C7. Footprint `crates/exitium_save/`. Trim #65 to its real footprint (`exitium_ecs`, `exitium_tasks`, `exitium_profiler`).
   - Undo the Jam trims: #42 and #51 keep their sequential scope, `par_iter_mut` stays with #48 (jam-mvp.md open question 4).
   - Set Project priorities: the reflect group, tasks and profiler above ECS, and ECS above serial, in Stage 1.
   - **Factory-rule PR** `chore(factory): treat examples/<name>/ as an Edits entry` in `factory-run`, so examples run side by side. Ordinary PR, same AI review.
   - **Docs follow-up PR** (a hotspot: PLAN.md, INTERVIEW.md, the epics). This spec does not edit them. The follow-up updates: PLAN.md phase order, the "Glutton critical path" wording and the roadmap table, so the MVP is first and Glutton is later; INTERVIEW.md decisions that named Glutton as the driver; the epics' checklists (#3 to #11) to match "Epic mapping"; and a pointer from CLAUDE.md "Orient cheaply" if the owner wants one. Run it as a `docs-graph` pass after Stage 0 merges.
   - Lift `needs-spec` as each spec merges.

**Dependency candidates (crates.io, for the intake PRs).** The versions, licences, wasm support and why-not-std argument for each belong to the owning spec. The intake PR records them in the workspace manifest. Nothing here is a decision. Licences below are from memory and must be checked by the owning spec. Items marked "R5" share the C or C++ toolchain risk.

| Crate | Used by | Intake | Native | wasm32 | Notes |
|---|---|---|---|---|---|
| `wgpu`, `winit`, `taffy`, `rapier2d`, `kira`, `png`, `hound`, `lewton`, `ttf-parser`, `web-time` | Stage 1 backends | 1 | yes | yes (WebGPU only) | As in the merged specs. |
| `gilrs` | `exitium_platform` (gamepad) | 1 | yes (XInput, WGI) | yes, via `wasm-bindgen` | MIT OR Apache-2.0. SP3 decides. |
| `notify` or polling with std | `exitium_assets` (hot reload) | 1 | yes | no (web has no file watch) | CC0-1.0. Check `deny.toml`. Polling at 4 Hz needs no dependency. SP4 decides. |
| `tracy-client` | `exitium_profiler` (`tracy` feature) | 1 | yes | not required | Off by default. Builds C++ (native only). |
| `mlua` with `luau` and `vendored` | `exitium_script` | 2 | yes | **R5**: Luau is C++ | MIT. The spike decides the route. |
| `egui` family, `rmcp`, `tokio`, `serde`, `serde_json`, `zip` | editor, MCP, CLI | 2 | yes | not required | As in editor-mcp-mvp.md and assets-scene-export.md. |
| An Aseprite parser (for example `asefile`) | `exitium_assets` | 3 | yes | yes (pure Rust) | Check licence and maintenance. SP12 decides. |
| `zstd` (C) or `ruzstd` (pure Rust), `lz4_flex` | `exitium_serial`, `exitium_save` | 1 and 3 | yes | `zstd`: **R5-like**; `ruzstd`: decode complete, encoder limited; `lz4_flex`: yes | #24 and SP14 decide, including the web. |
| An authenticated cipher and hash (`chacha20poly1305`, `blake3` or `sha2` and `hmac`), `getrandom` | `exitium_save` | 3 | yes | yes (`getrandom` needs its wasm feature) | Tamper detection is a deterrent: a key shipped in the client is not secret. The server validates. |
| `steamworks` (Steamworks bindings) | `exitium_steam` | 3 | Windows | no | **Proprietary SDK** (Valve). Needs an ADR (open question 4). Never in `wasm`. |
| A net transport: `renet` or `quinn` (native), `tungstenite` or WebTransport (web) | `exitium_net` | 3 | yes | web needs WebSocket or WebTransport, not UDP | SP16 and an ADR decide. |
| `axum`, `hyper`, `tokio`, `sqlx` or `rusqlite`, password and token crates, `reqwest` or `ureq` (client) | `exitium_server`, `exitium_backend` | 3 | yes | client only: fetch through `web-sys` or `reqwest` | SP17 decides. The server stack is native only. |
| A seeded RNG (`rand_pcg` or a hand-written PCG) | `exitium_sim`, `exitium_net` | 3 | yes | yes | A 20-line PCG avoids the dependency. |

The ECS, reflect, serial, tasks, tilemap, data and command crates add no external dependency beyond the above, unless their specs justify one. Anything not in this table needs the owning spec to justify it, and goes into the intake PR before its stage starts.
