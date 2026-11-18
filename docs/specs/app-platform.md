# Spec: App loop, plugins, transforms and platform (window, input)

- Issue: #106
- Crates: `exitium_app` and `exitium_platform`. `exitium_app` depends on `exitium_core`, `exitium_ecs` and `exitium_reflect` and has no external dependency. `exitium_platform` is Stage 1 standalone (`exitium_core`, `winit` and `web-time`, plus `web-sys` and `wasm-bindgen` on wasm32). Its `plugin` module (Stage 2) adds `exitium_ecs`, `exitium_app` and `exitium_reflect`. Neither crate has an edge to render, physics, audio, assets, ui, scene, command, editor or mcp, and `exitium_app` has no edge to `exitium_platform` (C1). Cross-crate glue lives in the facade (see "Facade glue").
- Status: draft. The owner-approved contract (2026-11-18) fixes the ownership of `App`, `Plugin`, the schedules, `Time` and `Transform2d`, and fixes Windows and Chrome as the only tested targets. Everything else is the author's proposal. The cross-spec rules C1 to C6 are orchestrator rulings (`jam-mvp.md`).

## Goal

Specify the two crates every other runtime crate plugs into.

- **`exitium_app`** owns the plugin model, the `App` builder, the frame loop (Startup, fixed-rate ticks, variable update, render extraction), `Time`, the headless mode, and `Transform2d` / `GlobalTransform2d` with parent/child propagation. It maps those schedules onto the ECS scheduler of #22.
- **`exitium_platform`** owns the winit window and event loop on Windows and on the web, resize and high DPI, and keyboard and mouse state. In Stage 1 it is a plain library with an example. In Stage 2 a `plugin` module makes it a `PlatformPlugin` for `exitium_app`.

The spec decides these things so that the implementation issues need no further design:

- **One fixed schedule order**, with a schedule for each phase (`First`, `FixedFirst`, `FixedUpdate`, `FixedLast`, `Update`, `PostUpdate`, `Extract`, `Last`) instead of sets inside one schedule. Physics, UI input and render extraction each get an unambiguous slot.
- **`Time` is an app resource** that carries the fixed-step alpha, and the loop clamps both a long frame and the number of catch-up ticks.
- **Input edges are relative to the schedule that reads them.** `just_pressed` inside `FixedUpdate` means "since the previous tick", and inside `Update` it means "since the previous frame", so a jump pressed on a frame with no tick is not lost.
- **World space is y up**, rotation is counter-clockwise radians, and `Transform2d` is the only transform an author writes.
- **No winit type appears in the public API** except two deliberate ones for the editor (`WindowHandler::raw_event` and `PlatformWindow::as_winit`), and **`exitium_platform` hands the renderer a window handle** through a type that implements the raw handle traits, without depending on render.
- **Everything an agent needs is reflected data and events.** This spec adds no command to the bus.

Versions were checked on 2026-11-18 (crates.io): `winit` 0.30.13 stable (0.31.0-beta.4 is the newest and is not used), `web-sys` 0.3.107, `wasm-bindgen` 0.2.130, `web-time` 1.1.0.

## Non-goals

Each has a stated path, so none blocks sidescroller, isometric, pixel art, multiplayer or destruction (design rule 5).

- **Gamepad and touch.** `InputEvent` is `#[non_exhaustive]`; a device adds variants and a `ButtonInput` field. Nothing else changes.
- **Text input, IME and logical (layout-dependent) keys.** The MVP has physical keys only (`KeyCode` names the key's position, as the W3C `code` does). The UI has no text field in the MVP.
- **Multiple windows, window decorations and cursor grab.** One primary window. The resource names (`PrimaryWindow`) leave room for more.
- **Render interpolation of non-physics movers.** A game system that moves a `Transform2d` in `FixedUpdate` is drawn at the tick rate. The path is an opt-in `PreviousTransform2d` plus a `PostUpdate` blend by `Time::fixed_alpha` (open question 6). Physics interpolates its own bodies (#108).
- **Keeping the world pose when reparenting.** `set_parent` keeps the local transform, so the world pose changes. A `reparent_keep_world` helper is additive.
- **A real-time headless runner.** The default headless runner runs frames back to back with simulated time. A sleeping runner needs `std::thread::sleep`, which wasm32 lacks, so it is native only and is a later addition (open question 8).
- **Play-in-editor, state machines and system enable/disable.** The editor is #110's. State gating is by schedule label (#22).
- **Changing the loop through MCP.** Systems are Rust (the contract). Agents control the loop through `TimeConfig` fields (below).

## Design

### Boundaries (who owns what)

| Concern | Owner |
|---|---|
| `System`, `Schedule`, `Schedules`, `Commands`, resources, events, `run_schedule`, `update_events` | `exitium_ecs` (#22) |
| `App`, `Plugin`, schedule labels and sets, the frame loop, `Time`, `TimeConfig`, `AppExit`, headless mode, `Transform2d`, `GlobalTransform2d`, `CursorWorld` (the slot) | `exitium_app` (this spec) |
| `FixedClock` (the accumulator and the tick cap), the plain `Time` snapshot | `exitium_core` (frozen, reused) |
| winit window and loop, `InputEvent`, `InputState`, `WindowInfo`, `PrimaryWindow`, `PlatformPlugin` | `exitium_platform` (this spec) |
| Window to renderer, cursor to world, platform input to `UiInputFrame`, `DefaultPlugins` | the facade `crates/exitium/` (a hotspot, C1) |
| Command bus, live-control server | `exitium_command`, `exitium_editor` (#110) |

### Module layout

```
exitium_app/src/
  lib.rs         re-exports, crate docs
  app.rs         App, AppError, Runner, boundary hooks                        safe
  plugin.rs      Plugin, PluginGroup, PluginGroupBuilder, Plugins             safe
  schedule.rs    schedule labels (unit structs), sets (consts), IntoSystemConfigs   safe
  frame.rs       run_frame, FrameReport, AppExit, the headless runner         safe
  time.rs        Time, TimeConfig, FrameStats                                 safe
  cursor.rs      CursorWorld (data only)                                      safe
  transform/mod.rs        Transform2d, GlobalTransform2d, hierarchy commands  safe
  transform/propagate.rs  ensure_global_transforms, propagate_transforms      safe

exitium_platform/src/
  lib.rs
  input/key.rs     KeyCode, MouseButton                                       safe
  input/button.rs  ButtonInput<K>, EdgeView                                   safe
  input/event.rs   InputEvent, PlatformEvent, InjectedInput                      safe
  input/state.rs   InputState, InputSnapshot                                  safe
  window/mod.rs    WindowConfig, WindowInfo                                   safe
  window/handle.rs PlatformWindow (raw handle traits), PrimaryWindow          safe
  window/translate.rs  winit values to PlatformEvent, pure functions          safe
  run.rs           WindowHandler, FrameInfo, Flow, run()                      safe
  run/native.rs, run/web.rs   the two event-loop drivers (cfg target_family)  safe
  web.rs           canvas lookup, console panic hook (cfg wasm32)             safe
  plugin/          PlatformPlugin, systems, runner (Stage 2, feature `plugin`) safe
```

Neither crate contains `unsafe`. There is no designated unsafe module, so no Miri target and no `#[allow(unsafe_code)]`.

### Schedules and the frame loop

**Labels.** Ten schedules, each a unit struct that converts into the ECS's `ScheduleLabel` (`impl From<Update> for ScheduleLabel`), so a game writes `app.add_systems(FixedUpdate, my_rules)` as the contract's sketch shows. `ScheduleLabel` is the `&'static str` newtype of #22 (`"exitium_app::update"` and so on).

| Label | Runs | What goes here |
|---|---|---|
| `PreStartup` | once, before the first frame | Facade glue that hands runtime-created resources between crates (the window to the renderer). |
| `Startup` | once, after `PreStartup` | Game setup, and plugin init that needs the window (renderer init). |
| `First` | every frame, before the tick loop | Platform ingest, UI input. |
| `FixedFirst` | every tick | Physics `reconcile`; input edge view switch. |
| `FixedUpdate` | every tick (60 Hz by default) | Game rules. |
| `FixedLast` | every tick | Physics `sync_in`, `step`, `write_back`; input tick edge clear. |
| `Update` | every frame | Game systems that follow the frame rate (animation, camera, UI logic). |
| `PostUpdate` | every frame | Systems that write `Transform2d` from other state (physics interpolation), then transform propagation. |
| `Extract` | every frame, last before `Last` | Render extraction, facade glue that appends draw data, then the render submit. |
| `Last` | every frame | Clear per-frame input state, apply window config. |

**Why a schedule per phase and not sets in one schedule.** The ECS has flat sets (#22), and a set cannot mean "after every system I did not name", which is what physics needs ("after the game's `FixedUpdate` systems"). Separate schedules give a total order for free, and each one ends in a sync point, so `Commands` from one phase are applied before the next phase starts. The cost is 8 schedule runs per frame at one tick, budgeted below.

**Sets (consts, `exitium_app::sets`).** Plugins that must be ordered inside one schedule use these. The ECS `SystemSet` is a `&'static str` newtype, so each is a `const`.

| Set | Schedule | Ordering (configured by `App::new`) |
|---|---|---|
| `INPUT` | `First` | Platform ingest: events into `InputState`, window info. |
| `UI_INPUT` | `First` | After `INPUT`. The UI input system and the facade glue that feeds it. |
| `PROPAGATE` | `PostUpdate` | Transform propagation. Systems that write `Transform2d` go `before_set(PROPAGATE)`. |
| `EXTRACT` | `Extract` | Systems that read the world and fill render lists. Before `RENDER`. |
| `RENDER` | `Extract` | Submit and present. After `EXTRACT`. |

`ensure_global_transforms` and `propagate_transforms` are in `PROPAGATE`, in that order.

**The frame, exactly** (`App::run_frame`, one call per displayed frame; `frame_seconds` is measured by the caller, because `exitium_core::time` reads no clock):

```
run_frame(frame_seconds):
  1. each boundary hook runs, outside any schedule        // the live-control seam, below
  2. if not started: run PreStartup, Startup; started = true
  3. Time update (see "Time" for the arithmetic) -> ticks, alpha
  4. run First
  5. repeat `ticks` times:
       Time.tick_count += 1                               // so a tick sees a count that includes itself
       run FixedFirst, FixedUpdate, FixedLast
       world.update_events(Fixed)
  6. Time.fixed_alpha = clock.remainder() / clock.step()
  7. run Update, PostUpdate, Extract, Last
  8. read AppExit events; world.update_events(Frame)
  9. return FrameReport { ticks, dropped_seconds, systems_run, applied, skipped_dead_entity, exit }
```

Each "run X" is `world.run_schedule(X)` (#22). A `ScheduleError` (a cycle, an access conflict, a missing resource id) is returned as `AppError::Schedule` and is a programmer error: the native runner prints it and exits with code 1, and `App::update` panics with it. `App::build()` compiles every schedule early, so the same error shows at startup and not on the first frame (an agent runs it to check a game without a window).

**Event clocks (resolves #22 open question 4).** Both clocks stay. The driver calls `update_events(Fixed)` after each tick and `update_events(Frame)` once per frame, exactly as #22 specified. `App::add_event::<E>()` uses `Frame`, `add_fixed_event::<E>()` uses `Fixed`. An event sent in `Update` and read in `FixedUpdate` can expire if two consecutive frames run no tick (possible at 144 Hz display and 60 Hz fixed rate), so events produced and consumed in the fixed schedules use `Fixed`, and `EventReader::missed()` reports the rest. Plugins may use ECS events for per-tick delivery (this answers physics open question 3).

**Boundary hooks (the seam for #110's live-control server).** `App::add_boundary_hook(f: impl FnMut(&mut World) + 'static)`. Hooks run at the start of every frame, in registration order, **outside** any schedule run. That is the one point at which the command bus may apply commands and the live-control server may drain its queue (the bus applies between schedule runs, #22, and the roadmap fixes "one fixed point in the frame"). A hook is not `Send`; the server thread talks to it over a channel the hook owns. The headless runner keeps calling `run_frame`, so hooks keep running without a window.

### Plugin model and the `App` builder

```rust
pub trait Plugin: 'static {
    fn build(&self, app: &mut App);
    fn name(&self) -> &str { std::any::type_name::<Self>() }
    fn is_unique(&self) -> bool { true }        // a second instance panics unless this returns false
}
pub trait PluginGroup { fn build(self) -> PluginGroupBuilder; }
pub struct PluginGroupBuilder { /* ordered Vec of (name, Box<dyn Plugin>) */ }
impl PluginGroupBuilder {
    pub fn add<P: Plugin>(self, p: P) -> Self;
    pub fn set<P: Plugin>(self, p: P) -> Self;        // replaces the instance of P; panics if P is absent
    pub fn disable<P: Plugin>(self) -> Self;
}
pub trait Plugins<Marker> { fn add_to(self, app: &mut App); }   // implemented for P: Plugin, G: PluginGroup, tuples of up to 8

pub struct App { /* World, runner, hooks, plugin names, FixedClock, started flag, AppExit cursor */ }
impl App {
    pub fn new() -> App;                                     // see below
    pub fn add_plugins<M>(&mut self, p: impl Plugins<M>) -> &mut Self;
    pub fn add_systems<M>(&mut self, label: impl Into<ScheduleLabel>, s: impl IntoSystemConfigs<M>) -> &mut Self;
    pub fn configure_set(&mut self, label: impl Into<ScheduleLabel>, set: SystemSet) -> SetConfig<'_>;
    pub fn insert_resource<R: Resource>(&mut self, r: R) -> &mut Self;
    pub fn init_resource<R: Resource + Default>(&mut self) -> &mut Self;
    pub fn register_type<T: GetTypeRegistration>(&mut self) -> &mut Self;     // components and resources: #[reflect(Component)] / #[reflect(Resource)] register their type data here
    pub fn add_event<E: Event>(&mut self) -> &mut Self;                         // Frame clock
    pub fn add_fixed_event<E: Event>(&mut self) -> &mut Self;                   // Fixed clock
    pub fn add_reflected_event<E: Event + Reflectable>(&mut self) -> &mut Self;        // Frame clock, also registers ReflectEvent
    pub fn add_reflected_fixed_event<E: Event + Reflectable>(&mut self) -> &mut Self;
    pub fn add_boundary_hook(&mut self, f: impl FnMut(&mut World) + 'static) -> &mut Self;
    pub fn set_runner(&mut self, r: impl FnOnce(App) -> AppExit + 'static) -> &mut Self;
    pub fn world(&self) -> &World;   pub fn world_mut(&mut self) -> &mut World;
    pub fn build(&mut self) -> Result<(), AppError>;                           // compile every schedule now
    pub fn run_frame(&mut self, frame_seconds: f64) -> Result<FrameReport, AppError>;
    pub fn update(&mut self);                                                   // run_frame(fixed_step) and panic on error: one tick per call, for tests
    pub fn run_frames(&mut self, n: u32, frame_seconds: f64) -> Result<FrameReport, AppError>;   // headless: the last report
    pub fn run(self) -> AppExit;                                                // the runner, or the headless runner
}
```

- **`App::new()`** creates the `World`, inserts `AppTypeRegistry` (reflect-model, "Registry ownership": the app only inserts it), `Time`, `TimeConfig`, `FrameStats`, `CursorWorld`, and registers `AppExit` as a `Frame` reflected event. It creates all ten schedules with `ExecutorKind::SingleThreaded`, applies the set ordering above, and adds `ensure_global_transforms` and `propagate_transforms` to `PostUpdate`. It registers `Transform2d`, `GlobalTransform2d`, `Time`, `TimeConfig`, `FrameStats`, `CursorWorld` and `AppExit` with the registry. The app therefore works headless with no plugin.
- **`IntoSystemConfigs<M>`** is implemented for a plain function (`IntoSystem`), for `SystemConfig`, `Vec<SystemConfig>` (what `chain` returns), `[SystemConfig; N]` and tuples of up to 8 of those. #22 offers `system(f)` and `Schedule::add_systems(IntoIterator<SystemConfig>)`; this trait is the sugar that lets `add_systems(Update, (a, b))` read like the contract's sketch. It adds no concept.
- **A plugin inserts its resources and registers its types in `build`**, because the world already exists. `Startup` is for systems that need another plugin's data or the window. This avoids the race where a game's `Startup` system runs before a plugin's `Startup` system has inserted the resource it reads. (Physics and UI draft their resource inserts as `Startup` systems; inserting in `build` satisfies every reader and is stricter, so their issues use `build`.)
- **Duplicate plugins.** A second instance of a unique plugin panics at `add_plugins` with `plugin <name> added twice`. Panics at startup are the right failure for an agent-written game: the message names the plugin and the log shows it.
- **Plugin order is registration order.** `DefaultPlugins` (a `PluginGroup` in the facade) fixes the order of the engine's own plugins. Ordering between systems is by schedule and set, never by plugin order.
- **Run.** `App::run` calls the runner set by `set_runner`, or the headless runner if none was set. `PlatformPlugin` sets the winit runner (below). `App` is not `Send` (hooks are plain closures); it needs only `'static`, which is all that winit's web `spawn_app` requires.

**Runner contract and the web.** A runner is `FnOnce(App) -> AppExit`. On Windows it blocks until the app exits and returns the exit code. On the web the browser owns the loop: the runner hands the app to winit with `spawn_app` and **returns `AppExit { code: 0 }` at once**. `main` must return after `run()`, and must not touch the app. This is the one visible difference of the two targets in the app API.

**The headless runner** (the default, with no window). It loops `run_frame(1.0 / fixed_hz)` until a frame reports an `AppExit`, so each frame runs exactly one tick and no wall clock is read. A headless app with no system that sends `AppExit` never returns by design (it is a server). `run_frames(n, seconds)` is the bounded form for tests and for an MCP "advance N frames" tool.

### `Time`, `TimeConfig` and the fixed step

`exitium_core::Time` has `delta`, `elapsed`, `tick_count` and `fixed_step`, and no alpha. `exitium_core` is frozen for the MVP, and it cannot derive `Resource` or `Reflect` (it has no `exitium_ecs` or `exitium_reflect` edge). So `exitium_app` defines its own reflected `Time` resource with the same four fields **and** the three that the loop needs. `FixedClock` from core is reused as the accumulator, so its spiral-of-death clamp and its tests are not rewritten.

```rust
#[derive(Resource, Reflect, Clone, Copy, Debug)]
#[reflect(Resource, type_name = "exitium_app::Time")]
pub struct Time {
    pub delta: f64,             // seconds this frame, clamped and scaled; 0 while paused. Variable-rate systems read this.
    pub real_delta: f64,        // seconds this frame, clamped, not scaled, not zeroed by pause (UI and menu animation)
    pub elapsed: f64,           // sum of every frame's `delta`
    pub tick_count: u64,        // fixed ticks run, including the current one inside a tick
    pub fixed_step: f64,        // seconds per tick (1 / fixed_hz). Fixed-rate systems read this, never `delta`
    pub fixed_alpha: f32,       // fraction of the next tick already accumulated, in [0, 1). Valid in Update and later; in First it is last frame's value
    pub ticks_this_frame: u32,
    pub dropped_seconds: f64,   // time discarded this frame by the tick cap (0 on a healthy frame)
}
impl Time { pub fn fixed_step_f32(&self) -> f32; pub fn fixed_alpha(&self) -> f32; }   // `From<&Time> for exitium_core::Time` for code that wants the core snapshot

#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource, Default, type_name = "exitium_app::TimeConfig")]
pub struct TimeConfig {
    #[reflect(range(min = 1.0, max = 1000.0))] pub fixed_hz: f64,              // 60.0
    #[reflect(range(min = 1, max = 64))]       pub max_ticks_per_frame: u32,   // 8
    #[reflect(range(min = 0.001, max = 5.0))]  pub max_frame_seconds: f64,     // 0.25
    #[reflect(range(min = 0.0, max = 100.0))]  pub time_scale: f64,            // 1.0
    pub paused: bool,                                                          // false: no ticks run, `delta` is 0
    pub step_ticks: u32,                                                       // 0: while paused, run this many ticks and reset to 0
}
```

**Arithmetic (step 3 of the frame):**

```
raw        = if frame_seconds is finite and >= 0 { frame_seconds } else { 0 }
real_delta = min(raw, max_frame_seconds)                 // a tab that slept for minutes cannot owe thousands of ticks
delta      = if paused { 0 } else { real_delta * time_scale }
elapsed   += delta
if fixed_hz or max_ticks_per_frame changed since last frame: clock = FixedClock::new(1 / fixed_hz, max_ticks)   // the remainder resets
advance    = if paused { ticks = min(step_ticks, max_ticks); step_ticks -= ticks; dropped = 0 } else { clock.advance(delta) }
ticks_this_frame = advance.ticks;  dropped_seconds = advance.dropped
```

- **Spiral of death.** Two clamps: `max_frame_seconds` bounds one hitch, and `max_ticks_per_frame` (through `FixedClock`) bounds the work per frame. Past either, the simulation runs slower than real time instead of freezing, and `dropped_seconds` and `FrameStats` show it. With the defaults the simulation falls behind only below 8 frames per second (8 ticks of 1/60 s cover 133 ms).
- **Configuration is data, validated by the loop.** A writable reflected resource can be set to anything (reflection's `range` is a hint and does not clamp, reflect-model). The loop clamps each field into its documented range every frame, treats a non-finite value as the default, and counts frames where it had to in `FrameStats::config_clamped`. It never panics on a bad config set through MCP.
- **Pause and step.** `paused` stops the tick loop and zeroes `delta`; `Update`, `PostUpdate` and `Extract` still run every frame, so a paused game is still drawn and the editor can still pick. `step_ticks = n` while paused runs exactly `min(n, max_ticks)` ticks next frame, with `delta` still 0 and `fixed_step` unchanged, then resets itself. An agent debugs by pausing, stepping one tick and reading the world. This is two fields and no command (see the surface section).
- **No `Time` swap.** #22 left "the `Time` swap" to the app. This spec does not swap. Core's `Time` already says fixed-rate systems use `fixed_step` and `tick_count` and never `delta`, so one resource serves both schedules and a system reads the same type everywhere. Physics reads `Time::fixed_step` and `Time::fixed_alpha` (this resolves physics open question 2 and the "open dependency on #106" in its fixed-step section).

**`FrameStats`** (reflected resource, written by the loop at the end of each frame, so it is stale inside a frame): `frame_count: u64`, `last_ticks: u32`, `last_systems_run: u32`, `dropped_seconds_total: f64`, `stalled_frames: u32` (frames that hit either clamp), `config_clamped: u32`. These are the "last RunReport per schedule" aggregates #22 promises to MCP; the per-schedule detail stays in `Schedules::last_report` (#22).

**`AppExit`** is `#[derive(Event, Reflect)] struct AppExit { pub code: u8 }`, `Frame` clock. Any system, hook or agent sends it (`world.send_event(AppExit { code: 0 })` or the generic `SendEvent` command). The frame that sees it completes, including `Last`, and `run_frame` returns it in `FrameReport::exit`. The runner then stops.

### `Transform2d` and `GlobalTransform2d`

**Conventions (resolves physics and render open questions on axes).** World space is **y up, +x right**. Rotation is **counter-clockwise radians**. One world unit is whatever the game says (texels for pixel art, meters for a physics game); `exitium_app` assigns no meaning, the camera (#107) and `units_per_meter` (#108) do. There is no z: draw order is a `layer` and `z` on the sprite (#107), and 2D physics has none.

```rust
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default, type_name = "exitium_app::Transform2d")]
pub struct Transform2d {
    pub translation: Vec2,
    pub rotation: f32,                                    // radians, counter-clockwise
    #[reflect(default = "Transform2d::default_scale")] pub scale: Vec2,   // Vec2::ONE, so a scene may omit it
}
impl Transform2d { pub const IDENTITY: Self; pub fn from_xy(x: f32, y: f32) -> Self; pub fn with_rotation(self, r: f32) -> Self;
                   pub fn with_scale(self, s: Vec2) -> Self; pub fn to_affine(&self) -> Affine2; }   // scale, then rotate, then translate

#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Component, Default, type_name = "exitium_app::GlobalTransform2d", custom(derived = true))]
pub struct GlobalTransform2d(pub Affine2);                // world from local; derived, never written by gameplay
impl GlobalTransform2d { pub fn translation(&self) -> Vec2; pub fn affine(&self) -> Affine2; }
```

`Vec2` and `Affine2` already reflect as structs of their components (reflect-model). The type names are **pinned**, because scenes persist `Transform2d` and a module move must not change the schema. `GlobalTransform2d` carries the `derived` marker so that the scene spec (#111) skips it on save and the inspector shows it read-only (open question 3). Saving one would be harmless: propagation recomputes it on load.

**Hierarchy** comes from the ECS (#49): `Parent(Entity)` and `Children(Vec<Entity>)`, kept consistent by `Commands` helpers there. **Reparenting goes through those helpers** (`set_parent`, `add_child`, `remove_child`, and the bus's `Reparent`, which uses them). A raw overwrite or removal of `Parent` that leaves `Children` stale is unsupported: propagation trusts `Children` to find nodes and checks `Parent` to confirm them (below). A child's global is `parent_global * child_local.to_affine()`. A non-uniform parent scale with a rotated child produces shear, which an `Affine2` holds exactly, so `GlobalTransform2d` is an affine and not a decomposed transform.

**Systems (`PostUpdate`, set `PROPAGATE`).**

1. `ensure_global_transforms`: two queries and `Commands`. (a) `(With<Transform2d>, Without<GlobalTransform2d>)` gets `GlobalTransform2d::default()`. (b) `(With<Children>, Without<Transform2d>)` gets `Transform2d::IDENTITY` and `GlobalTransform2d::default()` together. **An entity that has children is spatial:** a grouping entity with no transform, common in scenes, would otherwise leave its children unreachable (a node is reached only through a transformed parent). The ordered sync point (#22) applies the inserts before step 2, so a freshly spawned entity or group has a correct global in the same frame. An entity with `Children` that is not meant to be spatial (a non-spatial grouping) gets an identity transform it ignores; that cost is accepted.
2. `propagate_transforms`: a full top-down pass, with **value comparison instead of change flags**.
   - **Roots** (no `Parent`): `Query<(&Transform2d, &mut GlobalTransform2d, Option<&Children>), Without<Parent>>`. The new global is `local.to_affine()`. It is **written only if it differs bitwise from the stored value**. A root with `Children` goes on a scratch stack together with its global.
   - **Descent:** pop `(entity, global)`, read its `Children`, and for each child look up `(&Transform2d, &Parent)`. **Skip the child unless its `Parent` is the entity being expanded** (one compare per node). Otherwise compute `global * local.to_affine()`, write it if it differs, and push the child if it has children. So a stale `Children` entry (a child listed by a former parent) is ignored, and a node can never be visited from two parents, which would write its global twice a frame and make `Changed<GlobalTransform2d>` fire every frame. A child with no `Transform2d` is skipped (not spatial). A child despawned since the parent's `Children` was updated is skipped (`get` returns `None`). A child with a dangling `Parent` (parent despawned without `despawn_recursive`) is unreachable and keeps its last global (the helpers of #49 prevent this). The scratch stack is a `Local<Vec<..>>` reused across frames. A debug build asserts the step count stays at or below the number of transformed entities (a `Children` cycle would loop forever; #49 rejects cycles).
   - **Queries:** the write queries are `Without<Parent>` (roots) and `With<Parent>` (nodes, `&mut GlobalTransform2d`), so they are filter-disjoint and the system is accepted (#22, conflict rule 3). Read-only `Query<(&Transform2d, &Parent)>` and `Query<&Children>` supply the other reads.
3. **Why compare values and not use `Changed<Transform2d>`.** The ECS cannot observe a component removal (#21). A child detached with `remove_child` (which removes `Parent` and edits `Children`) becomes a root, and the root pass recomputes it because its stored global no longer equals its local affine, with no change flag needed. Direct writes to either transform need none either. The price is a visit per entity per frame instead of per changed entity; the budget below accepts it for the MVP. The optimization (visit only changed subtrees: `Changed<Transform2d>` and `Changed<Parent>`, topmost-dirty-ancestor first) is additive and does not change a signature.
4. **The `Changed<GlobalTransform2d>` contract.** Because a global is written only when its value changes, `Changed<GlobalTransform2d>` means "this entity moved in world space". Physics relies on this to re-place static colliders (#108, "Moved static things are re-placed here"). A test pins it.

`propagate_transforms` runs in `PostUpdate`, after the systems that write `Transform2d` (`before_set(PROPAGATE)`) and before `Extract`, so the renderer and the next frame's physics `reconcile` read a consistent global (render D1, physics "Hierarchy and `GlobalTransform2d`").

### `CursorWorld`

`CursorWorld { position: Option<Vec2> }` is a reflected resource defined in `exitium_app` (world space is app's) and **filled by facade glue**, because converting a cursor needs the camera and render has no edge to platform or app's input (C1). The glue runs in `First`, after `INPUT`, and uses the camera as of the previous frame's propagation, so it maps the cursor onto what is on screen. It passes `InputState::cursor_logical` (never the physical value) to render's `screen_to_world`, which takes logical pixels; a physical value would be wrong by the scale factor on a 1.5x display. It is `None` when the cursor is outside the window or no camera contains it. Screen space (logical pixels, top-left origin, y down, which is render's `Screen` space) is `InputState::cursor_logical`.

### Input (`exitium_platform`, Stage 1)

**Types.**

```rust
#[non_exhaustive] #[repr(u8)] pub enum KeyCode { KeyA, /* .. KeyZ */, Digit0, /* .. Digit9 */, F1, /* .. F12 */, ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Space, Enter, Escape, Tab, Backspace, Delete, Insert, Home, End, PageUp, PageDown,
    ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight, SuperLeft, SuperRight, CapsLock,
    Minus, Equal, BracketLeft, BracketRight, Backslash, Semicolon, Quote, Backquote, Comma, Period, Slash,
    Numpad0, /* .. Numpad9 */, NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide, NumpadEnter, NumpadDecimal }   // about 120 variants, at most 192
#[non_exhaustive] pub enum MouseButton { Left, Right, Middle, Back, Forward }

#[non_exhaustive]
pub enum InputEvent {                                    // the only way input enters the engine; real and injected input take this path
    Key { key: KeyCode, pressed: bool },
    MouseButton { button: MouseButton, pressed: bool },
    CursorMoved { physical: Vec2 },                      // device pixels, top-left origin, y down
    CursorLeft,
    Scroll { lines: Vec2 },                              // +y is away from the user (up); pixel deltas are divided by PIXELS_PER_LINE = 40
}
#[non_exhaustive]
pub enum PlatformEvent { Input(InputEvent), Resized { physical: UVec2, scale_factor: f32 }, Focused(bool), CloseRequested }
```

`KeyCode` is the platform's own enum, not winit's, so no winit type is public and the winit 0.31 migration touches one file. Variant names are the W3C `KeyboardEvent.code` strings (`KeyA`, `Digit1`, `ArrowLeft`, `Space`), which are also winit's names, so an MCP caller and a browser agree. A key outside the list is dropped by the translator and counted in `InputState::unmapped`.

**`ButtonInput<K>`: pressed, just pressed, just released, per schedule.** Edges are relative to the schedule that reads them. This is what makes jump input work in `FixedUpdate` on a 144 Hz display: a press on a frame that runs no tick is kept until the next tick instead of being cleared at the end of the frame.

```rust
pub struct ButtonInput<K: ButtonId> {
    down: [u64; 4],                       // 256 buttons, `K::COUNT <= 256` asserted at compile time
    frame_pressed: [u64; 4], frame_released: [u64; 4],    // edges since the last end_frame
    fixed_pressed: [u64; 4], fixed_released: [u64; 4],    // edges since the last end_tick
    view: EdgeView,                       // Frame (default) or Fixed
}
impl<K: ButtonId> ButtonInput<K> {
    pub fn pressed(&self, k: K) -> bool;          // the level, the same in both views
    pub fn just_pressed(&self, k: K) -> bool;     // an edge since the previous run of the *current view's* schedule
    pub fn just_released(&self, k: K) -> bool;
    pub fn any_pressed(&self) -> bool;  pub fn iter_pressed(&self) -> impl Iterator<Item = K>;
    pub fn press(&mut self, k: K);  pub fn release(&mut self, k: K);   // ignored when already in that state (OS key repeat)
    pub fn release_all(&mut self);                // releases every held button, with edges (focus loss)
    pub fn set_view(&mut self, v: EdgeView);  pub fn end_frame(&mut self);  pub fn end_tick(&mut self);
}
```

The platform plugin sets the view to `Fixed` in `FixedFirst` and back to `Frame` in `FixedLast` (which also calls `end_tick`), and calls `end_frame` in `Last`. A press and release inside one tick interval gives `just_pressed` and `just_released` both true and `pressed` false: nothing is lost and nothing is invented. If a frame runs two ticks, the first sees the edge and the second does not. A game never selects the view; `just_pressed` is correct in whichever schedule it runs.

**`InputState`** (Stage 1 resource-to-be, plain data, `Send + Sync`):

```rust
pub struct InputState {
    pub keys: ButtonInput<KeyCode>,  pub mouse: ButtonInput<MouseButton>,
    cursor_physical: Option<Vec2>,   scale_factor: f32,          // set from WindowInfo; 1.0 until a window reports
    cursor_delta: Vec2, scroll: Vec2,                            // frame quantities, cleared by end_frame; read them in Update
    pub unmapped: u32, dirty: bool,
}
impl InputState {
    pub fn apply(&mut self, e: &InputEvent);                     // the single mutation path
    pub fn cursor_physical(&self) -> Option<Vec2>;               // device pixels, top-left origin
    pub fn cursor_logical(&self) -> Option<Vec2>;                // physical / scale_factor: render's Screen space
    pub fn cursor_delta(&self) -> Vec2;  pub fn scroll_lines(&self) -> Vec2;
    pub fn set_scale_factor(&mut self, s: f32);  pub fn snapshot(&self, out: &mut InputSnapshot);
    pub fn end_frame(&mut self);  pub fn end_tick(&mut self);  pub fn release_all(&mut self);
}
```

- **Cursor in screen and world space.** `cursor_physical` and `cursor_logical` are screen space. The world position needs a camera, so it is `CursorWorld` (above), filled by glue.
- **Consumption (answers UI open questions 3 and 4).** The platform never masks input. The UI reads `InputState` in the `Frame` view in `First` (`cursor_logical`, `mouse.pressed(Left)`, `keys.just_pressed(Tab | Enter | Space | Escape)`, `keys.pressed(ShiftLeft | ShiftRight)`, `scroll_lines`) and publishes `UiCapture`. Gameplay checks `UiCapture`. Masking in the shared state would make `pressed` mean different things to different readers and would need a UI edge (C1).
- **Focus loss.** `PlatformEvent::Focused(false)` calls `release_all`, so alt-tab does not leave a key stuck. Releasing emits the edges, so a held key is seen as released once.
- **`InputSnapshot`** (reflected, built by `snapshot` into a reused buffer) lists what is down: `keys: Vec<KeyCode>`, `buttons: Vec<MouseButton>`, `cursor_physical`, `cursor_logical`, `scroll_lines`. It exists because bitsets do not reflect usefully. The plugin refreshes the resource in `Last` only when `dirty`, so a still keyboard costs nothing.

### Window and the event loop (`exitium_platform`, Stage 1)

```rust
pub struct WindowConfig { pub title: String, pub size: UVec2 /* logical px, 1280x720 */, pub resizable: bool, pub fullscreen: bool /* borderless */,
                          pub cursor_visible: bool, pub canvas_id: Option<String> /* web, "exitium-canvas" */, pub prevent_default: bool /* web, true */,
                          pub max_fps: Option<u32> /* native, None */ }

pub struct WindowInfo { pub physical_size: UVec2, pub scale_factor: f32, pub focused: bool }
impl WindowInfo { pub fn logical_size(&self) -> Vec2; pub fn is_minimized(&self) -> bool /* either side is 0 */; }

/// Anything wgpu accepts as a surface target. Wraps `Arc<winit::window::Window>`.
#[derive(Clone)] pub struct PlatformWindow(/* Arc<winit::window::Window> */);
impl raw_window_handle::HasWindowHandle for PlatformWindow { .. }      // from `winit::raw_window_handle`, delegating
impl raw_window_handle::HasDisplayHandle for PlatformWindow { .. }
impl PlatformWindow { pub fn physical_size(&self) -> UVec2; pub fn scale_factor(&self) -> f32; pub fn request_redraw(&self);
                      pub fn set_title(&self, t: &str); pub fn set_cursor_visible(&self, v: bool); pub fn set_fullscreen(&self, f: bool);
                      pub fn as_winit(&self) -> &winit::window::Window; }   // the escape hatch for the editor's egui-winit

pub trait WindowHandler: 'static {
    fn ready(&mut self, window: &PlatformWindow);                           // once, when the window exists
    fn event(&mut self, window: &PlatformWindow, e: PlatformEvent);
    fn frame(&mut self, window: &PlatformWindow, f: FrameInfo) -> Flow;     // once per redraw
    fn raw_event(&mut self, _w: &PlatformWindow, _e: &winit::event::WindowEvent) -> bool { false }   // true = consumed (editor)
}
pub struct FrameInfo { pub seconds: f64 /* since the previous frame; 0 on the first */ }
pub enum Flow { Continue, Exit }
pub fn run(config: WindowConfig, handler: impl WindowHandler) -> Result<(), PlatformError>;   // blocks on Windows; returns at once on the web
```

**What the platform exposes for the renderer glue (answers render D2, C1-safe).** `PlatformWindow` implements `HasWindowHandle` and `HasDisplayHandle` and is `Clone + Send + Sync + 'static`, so it satisfies wgpu's blanket `From<T> for SurfaceTarget<'static>`. Render's `Renderer::new(window: impl Into<wgpu::SurfaceTarget<'static>>, size, scale_factor)` takes `platform_window.clone()` as is. The platform plugin (Stage 2) inserts the resources `PrimaryWindow(PlatformWindow)` (not reflected) and `WindowInfo` (reflected) before `PreStartup`, and sends `PlatformEvent::Resized` on every size or scale change. **The facade glue** (`PreStartup`: copy `PrimaryWindow` into render's window resource; `First`, after `INPUT`: copy `WindowInfo` into it each frame) is the only place that names both crates. `exitium_platform` never names `wgpu` or `exitium_render`; `exitium_render` never names `winit` or `exitium_platform`. `PlatformWindow` must be `Send + Sync` on wasm32 too, because the ECS `Resource` trait requires it: PL3 adds a compile-time assertion, built for wasm32. If winit's web window is not `Send + Sync`, the fallback is the ECS's non-send extension path (#22 open question 5), and this is the only point that would need it.

**The loop (`run`) on both targets.**

- winit's `ApplicationHandler` is the only driver. The window is created in `resumed` (winit 0.30 forbids creating it earlier), then `ready` is called, then `Poll` control flow runs. Each `about_to_wait` calls `window.request_redraw()`, and **each `RedrawRequested` is one frame**: measure seconds with `web_time::Instant` (a drop-in for `std::time::Instant`, which panics on wasm32-unknown-unknown), then call `frame`. On the web `request_redraw` is `requestAnimationFrame`, so frames follow the display. On Windows, frames follow the renderer's present (vsync), or spin when there is no renderer or present is immediate; `max_fps: Some(n)` switches to `WaitUntil` for that case.
- **Native:** `EventLoop::run_app(handler)` blocks until `Flow::Exit` calls `event_loop.exit()`. Windows' title-bar drag and edge resize run an OS-owned modal loop, so frames stall while the mouse is held on the title bar and redraws come only from `Resized`. The loop's `max_frame_seconds` clamp absorbs the catch-up. Accepted for the MVP (open question 5).
- **Web differences** (`run/web.rs`):
  1. The loop cannot block: `EventLoopExtWebSys::spawn_app(handler)` moves the handler into the browser and `run` returns at once. `Flow::Exit` stops the loop but leaves the canvas.
  2. **Canvas.** The canvas is found with `document.getElementById(canvas_id)` (default `exitium-canvas`) and given to winit with `with_canvas`. If it does not exist, winit creates one and appends it to `<body>` (`with_append(true)`). `WindowConfig::size` applies only to a canvas the engine creates; an existing canvas keeps its CSS size, and the page's CSS (for example `width: 100%; height: 100%`) decides the window size.
  3. **Async init.** There is none in this crate: window creation is synchronous even on the web. The only async step is the renderer's `request_adapter` and `request_device`, which render handles with its own poll system (#107).
  4. **High DPI.** `scale_factor` is `devicePixelRatio`; browser zoom changes it and winit reports `ScaleFactorChanged`. Both targets give consumers **physical** size and `scale_factor`; logical size is derived. On Windows winit makes the process per-monitor DPI aware. A `Resized` event is emitted after every `Resized` and `ScaleFactorChanged` from winit, **reading `inner_size()` and `scale_factor()` from the window at emission time**, so the pair is always consistent.
  5. **Size can be zero at startup** (the canvas has not been laid out). `WindowInfo::is_minimized` is true, and consumers treat zero as "not ready", as render's `Skipped(Minimized)` does.
  6. **Hidden tab.** `requestAnimationFrame` stops, so no frames run, and the first frame after return has a huge `seconds`. `max_frame_seconds` clamps it.
  7. **Default key handling.** `prevent_default: true` (so arrow keys do not scroll the page); `with_focusable(true)` so the canvas can receive keys. Set it to `false` for a page that wants the browser's shortcuts.
  8. **Panics** abort the page silently by default. `web::install_console_panic_hook()` (about 20 lines over `web_sys::console::error_1`) prints the message to the console. The plugin wraps it with `exitium_ecs::current_system()` so the failing system is named (#22, "Panic context"). The `console_error_panic_hook` crate is not used: the function is 20 lines and one crate fewer in the lock.
- **Translation is pure and tested without a window.** winit's `KeyEvent` has a private field and cannot be built in a test, so `translate.rs` takes decomposed values: `translate_key(PhysicalKey, ElementState) -> Option<InputEvent>`, `translate_mouse_button`, `translate_scroll(MouseScrollDelta)`, `translate_cursor(PhysicalPosition<f64>)`. All of those are public winit types that a test can construct. A table test covers every `KeyCode` variant (winit code to ours and back by name).
- **The editor's raw-event hook.** egui-winit (#110) needs the winit `WindowEvent`, and the game must not see a click the editor took. `WindowHandler::raw_event` is called first for keyboard, mouse and cursor events; if it returns true the translation is skipped. Resize, scale, focus and close are never consumed. The crate has exactly two public winit types, both deliberate and both for the editor: this hook (`&winit::event::WindowEvent`) and the escape hatch `PlatformWindow::as_winit` (`&winit::window::Window`, which egui-winit also needs). `egui-winit` 0.37 needs `winit ^0.30.13`, the same version.

### Stage 1 and Stage 2

- **Stage 1 (`exitium_platform`, `exitium_core`, `winit` and `web-time`; `web-sys` and `wasm-bindgen` on wasm32).** Input types and state, window config and info, `PlatformWindow`, translation, `run` on both targets, the console panic hook and an example. No ECS, no app. The example is the proof (below).
- **Stage 1 for `exitium_app` does not exist.** `exitium_app` depends on `exitium_ecs`, so it is the Stage 2 spine (roadmap).
- **Stage 2, `exitium_app`:** the spine in the order A1 to A4 below.
- **Stage 2, `exitium_platform::plugin` (feature `plugin`, default on, like physics):**
  - `PlatformPlugin { window: WindowConfig, exit_on_close: bool (true), install_panic_hook: bool (true) }`.
  - **In `build`:** insert `InputState`, `InputSnapshot`, `WindowConfig`, `WindowInfo` (zero size) and register two `Frame` events: `PlatformEvent` (sent only by the runner, **not reflected**, so MCP cannot send a resize or a close) and `InjectedInput(InputEvent)` (reflected, the injection event). Register all types with the reflection registry (Stage 1 types gain `cfg_attr(feature = "reflect", derive(Reflect))`; the `reflect` feature is added by intake 2). Install the panic hook. Set the runner.
  - **Systems.** `First`, set `INPUT`: `apply_platform_events` reads `EventReader<PlatformEvent>` and `EventReader<InjectedInput>`, applies input events to `InputState`, updates `WindowInfo` on `Resized` and `Focused` (with `release_all` on focus loss), and sends `AppExit` on `CloseRequested` when `exit_on_close`. `FixedFirst`: set the `Fixed` view. `FixedLast`: `end_tick`, set the `Frame` view. `Last`: `end_frame`, refresh `InputSnapshot`, and apply a changed `WindowConfig` (title, cursor visibility and fullscreen apply live; `size`, `resizable` and `canvas_id` apply only at creation).
  - **The runner** implements `WindowHandler`: `ready` inserts `PrimaryWindow` and `WindowInfo` (so they exist before `PreStartup`); `event` does `world.send_event(PlatformEvent)`; `frame` calls `app.run_frame(seconds)` and returns `Flow::Exit` when the report holds an `AppExit`; `raw_event` forwards to an optional hook set with `PlatformPlugin::with_raw_event_hook`.
  - **Input injection and winit input take one path into `InputState::apply`:** the runner sends `PlatformEvent::Input`, an agent sends `InjectedInput`, both between frames and both consumed in `First`. Injection is limited to `InputEvent` on purpose: a reflected `Resized`, `Focused` or `CloseRequested` would let an MCP call feed a huge size to the renderer or close the game. `InputState::apply` itself ignores a non-finite `CursorMoved` or `Scroll` and clamps one `Scroll` to 1,000 lines, so a hostile injected value cannot poison the state.

### Threading

- Everything runs on the main thread. The ECS executor is single-threaded in the MVP, and the web has no threads. `exitium_app` and `exitium_platform` create no thread and use no lock, atomic or `unsafe`.
- The winit event loop owns the main thread on Windows (required by winit on every desktop OS) and the JS event loop on the web. `App` is not `Send`.
- The only other thread in a game process is #110's live-control server. It never touches the world. It queues requests, and its boundary hook drains them at step 1 of the frame.
- `World` is `Send`, so a future runner on another thread is possible, but nothing in the MVP does it.

### Facade glue (listed here, owned by the facade)

C1 forces these into `crates/exitium/` (a hotspot, one issue at a time). They are not edits to `exitium_app` or `exitium_platform`.

1. **Window to render:** `PreStartup` copies `PrimaryWindow` into render's window resource, and `First` (after `INPUT`) copies `WindowInfo` into it each frame (render D2). The systems take `Option<Res<PrimaryWindow>>` and `Option<Res<WindowInfo>>` and do nothing when they are `None`, so a headless app (no window, no renderer: the MCP end-to-end test) does not panic.
2. **Cursor to world:** `First`, after `INPUT`: use render's camera resolve to fill `CursorWorld` from `InputState::cursor_logical` (logical pixels, as render's `screen_to_world` takes) and the primary camera's view. Like the window hand-off it reads `Option<Res<PrimaryWindow>>` and `Option<Res<..>>` of render's resources, so a headless app (no window, no renderer) skips it instead of panicking, and `DefaultPlugins` adds it only with `PlatformPlugin`.
3. **UI input and viewport:** a `First` system, `after_set(INPUT)` and ordered before the UI plugin's `ui_input` system (which is in `UI_INPUT`), converts `InputState` (frame view) into the UI plugin's `UiInputFrame` **resource** (UI-logical pixels = `cursor_logical / ui_scale`) and syncs `UiViewport` from `WindowInfo`. `ui_input` only reads that resource and stores `UiCapture`; it names no platform type, because C1 forbids a UI to platform edge. `ui-mvp.md` is amended in this PR to say so (U11). With no platform plugin the resource keeps its default (no pointer, nothing pressed), so a headless or UI-only app works. The slot (`UI_INPUT` after `INPUT`, in `First`, before every tick) is this spec's.
4. **`DefaultPlugins`** adds `PlatformPlugin` first among the engine's plugins, and re-exports the app and platform API through the facade.

## MCP / editor surface

Reflection is the source of truth (rule 2) and mutation is by command (rule 3). **This spec files no issue in `exitium_mcp` or `exitium_editor`.** It lists the surface that #110 builds the generic tools and panels from (C4), and adds **no new command** to the bus.

**Reflected components.**

| Type | Access | Notes |
|---|---|---|
| `Transform2d` | read, write | Edited through the ECS component-field and insert commands (storage spec). The editor's gizmo writes it. |
| `GlobalTransform2d` | read only (`derived`) | Shown in the inspector, skipped by scene save (open question 3). |

**Reflected resources.**

| Type | Access | Notes |
|---|---|---|
| `Time` | read | The loop rewrites it every frame, so a write would be overwritten. Marked read-only by its docs. |
| `TimeConfig` | read, write | `fixed_hz`, `max_ticks_per_frame`, `max_frame_seconds`, `time_scale`, `paused`, `step_ticks`. The generic `SetResourceField` (#22) is the "pause", "resume", "slow down" and "step N ticks" command. |
| `FrameStats` | read | Frames, ticks, stalls, dropped time. |
| `CursorWorld` | read | |
| `WindowInfo` | read | Physical and logical size, scale factor, focus. The agent's window info. |
| `WindowConfig` | read, write | Title, cursor visibility, fullscreen apply live. |
| `InputSnapshot` | read | What is pressed now and where the cursor is. |

**Reflected events** (sent through the generic `SendEvent` command, #22; each is registered with `add_reflected_event`):

| Event | Use |
|---|---|
| `InjectedInput` | **Input injection.** An agent sends `InjectedInput(Key { key: Space, pressed: true })`, then the release, or `CursorMoved` and `MouseButton`. It reaches `InputState::apply` in `First`, `INPUT`, the same as real input, so a game cannot tell the difference, and the edge rules above apply. It carries an `InputEvent` only; window events (`Resized`, `Focused`, `CloseRequested`) are not injectable. |
| `AppExit` | Graceful quit. |

**Tool names for #110** (the `<domain>_<verb>` convention of the ECS spec). The generic tools already cover everything above (`ecs_get_resource`, `ecs_set_resource_field`, `ecs_send_event`). Two thin presets make agent use easy and need no new command: `app_pause` / `app_resume` / `app_step` (writes `TimeConfig.paused` and `step_ticks`), and `input_inject` (sends `InjectedInput`). Whether they are presets or just documentation is #110's call. A tool that "advances N frames and returns" in a **windowed** live game needs the boundary hook to wait on `Time::tick_count`; in a **headless** app `App::run_frames` does it directly.

**Editor surface.** A time control (play, pause, step, speed) bound to `TimeConfig`; the transform gizmo writing `Transform2d`; an input and window panel from `InputSnapshot` and `WindowInfo`. The editor takes input before the game through `WindowHandler::raw_event` (above); #110 owns that wiring, and the platform provides the hook.

## Performance

Min-spec is 4 cores and 8 GB (CLAUDE.md). Benchmarks live in `crates/exitium_app/benches/frame.rs` and `crates/exitium_platform/benches/input.rs` under the 5% gate (`cargo xtask bench-ci`). **As in the ECS specs, ratios against baselines are binding and the absolute figures are provisional ceilings**, re-baselined by the `perf` agent from the first benchmark PR. Single thread, release, `lto = "thin"`.

| Metric | Budget |
|---|---|
| Frame driver overhead, all schedules empty, 1 tick (8 schedule runs, time update, two `update_events`) | <= 10 us |
| One schedule run, empty (remove, sync node, reinsert) | <= 1 us |
| Time update and `FixedClock::advance`, per frame | <= 150 ns |
| Extra tick in a catch-up frame (3 empty schedules and `update_events(Fixed)`) | <= 3 us |
| `propagate_transforms`, flat roots, per entity (compare, write only if different) | <= 6 ns; 2,000 sprites <= 12 us; 20,000 <= 120 us |
| `propagate_transforms`, hierarchy node (depth 3, per node) | <= 40 ns; 5,000 nodes <= 200 us |
| `ensure_global_transforms`, nothing to add | <= 0.5 us (archetype-level, no row scan) |
| `ButtonInput::pressed` / `just_pressed` | <= 3 ns |
| `InputState::apply` (one event) | <= 20 ns, no allocation |
| `end_frame` / `end_tick` | <= 15 ns each |
| `InputState::snapshot` with 10 keys down | <= 200 ns, no allocation after warm-up |
| winit to `PlatformEvent` translation, per event | <= 100 ns, no allocation |
| `InputState` size | <= 512 bytes, no heap |
| app and platform share of the Stage 2 sample frame (2,000 sprites) | <= 0.5% of 16.6 ms (83 us) |
| Window created to first `frame` call (not CI-gated, recorded by the examples) | <= 150 ms native, <= 100 ms on the web after the wasm has loaded |

**Complexity.** The loop is O(ticks) plus the systems. Propagation is O(roots + hierarchy nodes) per frame. Input is O(1) per event and per query. The driver keeps no per-frame `Vec`; `FrameReport` is `Copy`, and the transform stack is a reused `Local`. The no-allocation claims are reviewed, not enforced: a counting allocator needs `unsafe` and the roadmap cuts it (#64).

## Test plan

- **Time and loop (headless, `exitium_app`).** 60 Hz with frames of 6.9 ms (144 Hz) run 0 or 1 ticks (never 2: a frame is shorter than the 16.7 ms step) and, with frames of 33 ms (30 Hz), 1 or 2 ticks, and `fixed_alpha` stays in `[0, 1)` in both; a 2 s frame is clamped by `max_frame_seconds` and by the tick cap, and `dropped_seconds` and `stalled_frames` report it; NaN and negative `frame_seconds` act as 0; `time_scale` 0.5 and 2.0; `paused` runs no ticks and zero `delta` while `Update` still runs; `step_ticks` runs exactly that many and resets; a hostile `TimeConfig` (zero, NaN, huge) is clamped, counted, and never panics; changing `fixed_hz` mid-run rebuilds the clock; `tick_count` inside a tick includes it.
- **Schedule order.** A recording test registers a system in every schedule and asserts the exact order for 0, 1 and 3 ticks, `Startup` once, `PreStartup` before `Startup`, and `UI_INPUT` after `INPUT` and before the first `FixedFirst`; `EXTRACT` before `RENDER`; a `PostUpdate` system `before_set(PROPAGATE)` is seen by the propagation of the same frame; `Commands` from `FixedUpdate` are applied before `FixedLast`.
- **Events.** An event sent in `FixedUpdate` with the `Fixed` clock is read once by a `FixedUpdate` reader; a `Frame`-clock event sent in `Update` is read in the next `First` and in a `FixedUpdate` of the next frame when a tick runs; `AppExit` ends `run_frame` with the frame completed, including `Last`.
- **Plugins.** Registration order; a duplicate unique plugin panics with its name; `is_unique == false` allows two; `PluginGroupBuilder::set`, `disable`; a plugin inserting resources in `build` is visible to a game `Startup` system.
- **`App` API.** `build()` reports a cycle and an access conflict before any frame; `add_systems` with a function, a tuple and `chain`; `run_frames` returns the last report; the headless runner exits on `AppExit`; boundary hooks run first, in order, before `Startup` on the first frame and outside any schedule (a hook may call `world.run_schedule` on a label of its own without `AlreadyRunning`).
- **Transforms.** Identity and composition (scale, rotation, translation order); shear from non-uniform parent scale; a three-level tree; reparent through the #49 helper and detach through `remove_child` both update, and a stale `Children` entry (a child listed by a former parent) is ignored and does not make `Changed<GlobalTransform2d>` fire every frame; direct write to a child; a root with no `GlobalTransform2d` gets one in the same frame; a grouping entity with `Children` and no `Transform2d` gets an identity transform, and its transformed children propagate through it; a despawned child in `Children` is skipped; a dangling `Parent` keeps its last global; `Changed<GlobalTransform2d>` is false after a no-op `Transform2d` write and true after a real change; reflect round trip of both types, the pinned type names, and a `DynamicStruct` with `scale` missing rebuilt through `FromReflect` gets `Vec2::ONE` (scene loading itself is #111's).
- **Input (Stage 1, no window).** Press, release, repeat press ignored; edge rules in both views (a press on a frame with no tick is visible to the next tick once; a press and release inside one interval gives both edges and `pressed` false; two ticks in a frame give the edge to the first only); `end_frame` and `end_tick` clear only their own set; `release_all` emits edges; cursor physical to logical with scale 1.0, 1.5 and 2.0; scroll accumulation and the pixel-to-line constant; `unmapped` counts an unknown key; a property test applies random event sequences and checks that `pressed` equals the last event per button.
- **Translation.** Every `KeyCode` against winit's by name; mouse buttons; both `MouseScrollDelta` forms; fractional physical positions.
- **Window and loop (manual, recorded in each PR).** The example on Windows (125% and 100% scale, drag a window across monitors if available, resize, minimize, restore, alt-tab with a key held) and in Chrome (canvas by id, created canvas, browser zoom, window resize, hidden tab and return, the panic hook on a forced panic). A `compile`-only assertion that `PlatformWindow` is `Send + Sync`, built for wasm32.
- **Plugin (Stage 2, headless where possible).** Injected `InjectedInput` events drive `InputState` through `First` (a non-finite cursor and an oversized scroll are ignored or clamped, and a `Resized` cannot be sent through reflection); `Focused(false)` releases keys; `CloseRequested` sends `AppExit` when `exit_on_close`; a changed `WindowConfig` is applied only for the live fields; the runner's `WindowHandler` is tested with a fake window (the handler logic is separate from the winit loop).
- **wasm32.** `cargo build --target wasm32-unknown-unknown` for both crates (and `--examples` for the platform, proposed in the wiring). Neither crate is in `NATIVE_ONLY`.
- **Benchmarks.** Every row of the Performance table, each against a hand-written baseline (a bare loop over the same data).

## Open questions

1. **`Time` duplicates the core `Time`.** Core is frozen, cannot derive `Resource` or `Reflect`, and has no alpha. The app's `Time` has the same four fields plus three. When core may change (a later chore), core's `Time` can gain `fixed_alpha` and the app's resource can wrap it. Until then there are two types of the same name in different crates; the facade exports only the app's.
2. **Full propagation versus changed-only.** This spec visits every root and hierarchy node each frame (value compare) so that detaching a child is always correct without a hook. If a game has tens of thousands of parented entities, the optimization is the changed-only pass with a `Changed<Parent>` check and a removal-aware helper in #49. Confirm the budgets are acceptable for the MVP.
3. **Skipping derived components on save.** This spec marks `GlobalTransform2d` with `custom(derived = true)` and expects the scene spec (#111) to skip such components. If #111 chooses another rule (a list, or not registering `ReflectComponent`), only the attribute on `GlobalTransform2d` changes. `custom` on a type must be allowed by the reflect derive (reflect-model lists `custom` among the attributes, without saying where); if not, #111 owns the marker.
4. **Two winit types are public.** `WindowHandler::raw_event` exposes `winit::event::WindowEvent` and `PlatformWindow::as_winit` exposes `winit::window::Window`, both for the editor's egui-winit. If #110 can route egui from the translated `PlatformEvent`s and needs no window reference, both go away. egui-winit needs the winit event and window, so they are expected to stay.
5. **Windows modal resize loop.** Frames stall while the title bar is held. A fix needs a timer-driven redraw during the modal loop (winit has no portable hook for it). Accepted for the MVP: the game stays correct, because the next frame's `Time::delta` is clamped.
6. **Interpolation of non-physics movers** (render open question 3). Not in the MVP. The additive path is an opt-in `PreviousTransform2d` (saved in `FixedFirst`) and a `PostUpdate` blend into `GlobalTransform2d` by `Time::fixed_alpha`. It touches no signature in this spec.
7. **`Plugins` for tuples and groups** is a small marker-trait trick (as `IntoSystem`). If a basic-level reviewer finds it too clever, the alternative is `add_plugin` and `add_plugin_group` as two methods. Same behavior.
8. **A real-time headless runner** (a dedicated server, #8 in the roadmap's later epics) needs a native sleep and is deferred. The `run_frame(seconds)` entry point already accepts any measured time, so a server runner is a loop around it.
9. **Defaults for `max_ticks_per_frame` (8) and `max_frame_seconds` (0.25).** They are the author's. Lower values drop simulation time sooner on a slow machine; higher values risk longer stalls. The owner may tune them with the sample's p99 frame budget.
10. **ECS items to confirm.** `ScheduleLabel::new` and `SystemSet::new` must be `const fn` (the sets here are `const`s). `register_reflected_event::<E>` takes the clock (#22 item 13 lists it without one). `Children` exposes its slice (`as_slice` or `Deref`) and `Parent` its entity (`get`) (#49).

## Implementation issues

After this spec merges the architect files these into the Jam MVP milestone with the `file-issue` skill. The labels `PL1` to `PL5`, `A1` to `A4`, `PP1`, `PP2` and `F1` to `F3` are placeholders, replaced by issue numbers when filed. `intake-1` and `intake-2` are the intake PRs of the roadmap. Each issue edits one crate (or, for `F*`, only the facade hotspot), has about 600 changed lines or fewer (the estimate is next to it), a full labelled footprint and acceptance criteria. No issue is filed in `exitium_mcp` or `exitium_editor` (ruling b).

**Dependency rules.** Every Stage 1 issue is `Blocked by intake-1`. Every Stage 2 issue is `Blocked by intake-2` (rule d, C6). `exitium_platform` issues share a crate, so the factory runs one at a time and the list is in dependency order; the same holds for `exitium_app`. The two chains are independent of each other until the plugin issues. The `exitium_ecs` issues named below are the numbered items of `ecs-scheduler.md`.

**Common acceptance criteria:** `cargo xtask ci all` passes; new behavior has tests; each hot-path change ships its benchmark with the numbers in the PR body; every module has `//!` docs; no `unsafe`; no winit type is public except `WindowHandler::raw_event` and `PlatformWindow::as_winit`; the crate builds for `wasm32-unknown-unknown`.

**Stage 1: `exitium_platform` (depends on `exitium_core`, `winit` and `web-time`).**

**PL1. `feat(platform): KeyCode, MouseButton and ButtonInput`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `input/key.rs`, `input/button.rs`, and the crate `CLAUDE.md` (module map, the two-public-winit-types rule, the one-path-for-input rule). The benchmark is in PL2, to keep this issue under the size limit.
- Acceptance:
  - [ ] `KeyCode` has the variants listed, with `name()` and `from_name()` on the W3C strings and a dense index; every variant round-trips by name; `K::COUNT <= 256` is asserted at compile time.
  - [ ] `ButtonInput` implements the two edge sets and the `EdgeView` rules exactly as specified, including repeat ignore, `release_all` and the per-view clears; the table and property tests of the Test plan.
- Blocked by: intake-1.

**PL2. `feat(platform): InputEvent, InjectedInput, InputState and input benchmarks`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `input/event.rs`, `input/state.rs`, `benches/input.rs` (the `[[bench]]` stanza is added here and does not change `Cargo.lock`): `InputEvent`, `PlatformEvent`, `InjectedInput`, `apply` (ignoring non-finite cursor and scroll values and clamping one scroll to 1,000 lines), cursor physical and logical, scroll, `cursor_delta`, `unmapped`, `snapshot` into a reused `InputSnapshot`, the scale factor.
- Acceptance:
  - [ ] Every `InputEvent` variant applies as specified; the cursor converts for scales 1.0, 1.5 and 2.0; `Scroll` accumulates and `end_frame` clears it.
  - [ ] The property test (random sequences, `pressed` equals the last event per button) passes; NaN and infinite cursor and scroll values leave the state unchanged, and an oversized scroll is clamped.
  - [ ] The benchmark covers `pressed`, `just_pressed`, `press`, the clears, `apply` and `snapshot`, within the budgets; `apply` and `snapshot` are allocation-free after warm-up (reviewed).
- Blocked by: intake-1, PL1.

**PL3. `feat(platform): window config, info, PlatformWindow and winit translation`** (about 500 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `window/mod.rs`, `window/handle.rs`, `window/translate.rs`: `WindowConfig`, `WindowInfo`, `PlatformWindow` with the two raw handle impls and `as_winit`, the pure translation functions.
- Acceptance:
  - [ ] Every `KeyCode` is produced from winit's `KeyCode` by a table test, with the reverse by name; unknown keys return `None`.
  - [ ] Both `MouseScrollDelta` forms, all mouse buttons and fractional positions translate.
  - [ ] `PlatformWindow: HasWindowHandle + HasDisplayHandle + Clone + Send + Sync + 'static` is asserted at compile time and the assertion builds for `wasm32-unknown-unknown` (the Send and Sync assumption on the web is checked here, not assumed).
  - [ ] `WindowInfo::logical_size` and `is_minimized` have tests, including zero size.
- Blocked by: intake-1, PL2.

**PL4. `feat(platform): event loop, frame timing and web canvas`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `run.rs`, `run/native.rs`, `run/web.rs`, `web.rs`: `WindowHandler`, `FrameInfo`, `Flow`, `run`, `PlatformError`; `resumed` creates the window; redraw-driven frames with `web_time::Instant`; `max_fps` through `WaitUntil` on native; canvas lookup, creation and `prevent_default` on the web; `Resized` emission from the window's own size; `raw_event` consumption rules; `install_console_panic_hook`.
- Acceptance:
  - [ ] The handler's logic (event order, `Resized` after scale change, `raw_event` consuming only input events, exit on `Flow::Exit`) is tested with a fake driver that does not need a window; the winit glue is a thin layer around it.
  - [ ] The crate builds for Windows and `wasm32-unknown-unknown`; the web driver uses `spawn_app` and returns at once.
  - [ ] The Windows modal-resize behavior is documented in the module docs (open question 5).
- Blocked by: intake-1, PL3.

**PL5. `feat(platform): window example for Windows and Chrome`** (about 300 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `examples/window.rs` and `examples/web/index.html`. The example opens a window with `run`, prints the window info, key and mouse events, the cursor in physical and logical pixels, resize and scale changes and `ButtonInput` edges (`println!` on Windows, `console.log` on the web through `web-sys`), caps at 60 frames per second on native, and exits on Escape. A header comment gives the exact commands: `cargo run --example window` on Windows, and for Chrome `cargo build --example window --target wasm32-unknown-unknown --release`, then `wasm-bindgen --target web` with the CLI at the **locked** `wasm-bindgen` version (`cargo install wasm-bindgen-cli --version <locked version>`), then a static file server on `examples/web/`. It uses only this crate's own dependencies, never `exitium_render`, `exitium_ecs` or `exitium_app` (C1).
- Acceptance:
  - [ ] The PR body records a run on Windows (scale 100% and a higher scale, resize, minimize, alt-tab with a key held) and in Chrome (an existing canvas and a created canvas, browser zoom, window resize, a forced panic shown in the console).
  - [ ] `cargo build --example window` and `cargo build --example window --target wasm32-unknown-unknown` both pass.
  - [ ] The wasm-bindgen recipe is reproducible from the header comment alone.
- Blocked by: intake-1, PL4.

**Stage 2: `exitium_app` (the spine; depends on `exitium_core`, `exitium_ecs`, `exitium_reflect`).**

**A1. `feat(app): App, Plugin, PluginGroup, schedule labels and sets`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_app/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `app.rs`, `plugin.rs`, `schedule.rs`; the crate `CLAUDE.md` (the schedule order table, "plugins insert resources in `build`", the set table). The ten labels, the five sets, `IntoSystemConfigs`, `Plugins`, `PluginGroupBuilder`, `App` construction and the registration methods, `AppTypeRegistry` insertion, `build()`, and `run` with `set_runner` and the headless runner (`run_frames`).
- Acceptance:
  - [ ] Every `App` method of the sketch exists and is tested, including duplicate-plugin panics, group `set` and `disable`, `add_systems` with a function, a tuple and `chain`, and `build()` reporting a cycle and an access conflict.
  - [ ] `App::new()` creates all ten schedules with the set ordering and registers its types.
  - [ ] The labels convert into `ScheduleLabel`; a label name is unique and `crate::name`.
  - [ ] The `INPUT`, `UI_INPUT`, `PROPAGATE`, `EXTRACT` and `RENDER` ordering is tested.
- Blocked by: intake-2, the ECS single-threaded executor (item 12), the ECS derives re-export (item 16), `ReflectResource` and `ReflectEvent` (item 13).

**A2. `feat(app): Time, TimeConfig and the frame driver`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_app/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `time.rs`, `frame.rs`, `cursor.rs`: `Time`, `TimeConfig`, `FrameStats`, `AppExit`, `CursorWorld`, `run_frame`, `FrameReport`, boundary hooks, `update`, the headless runner.
- Acceptance:
  - [ ] The frame order of the spec holds for 0, 1 and 3 ticks (the recording test); `Startup` runs once; `tick_count` inside a tick includes it.
  - [ ] Every time test of the Test plan passes: the 0-or-1 tick pattern at 144 Hz and the 1-or-2 pattern at 30 Hz, both clamps, scale, pause, step, config clamping and counting, clock rebuild on `fixed_hz` change.
  - [ ] Both event clocks update at the specified points; `AppExit` completes the frame.
  - [ ] Boundary hooks run first, outside any schedule.
  - [ ] All new resources and `AppExit` are reflected with the pinned type names and ranges.
- Blocked by: intake-2, A1.

**A3. `feat(app): Transform2d, GlobalTransform2d and propagation`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_app/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `transform/mod.rs`, `transform/propagate.rs`: the two components, `ensure_global_transforms` (both queries), `propagate_transforms` with the disjoint write queries, the `Parent` check in the descent and the reused scratch stack, registration in `App::new`.
- Acceptance:
  - [ ] Every transform test of the Test plan passes, including detach through `remove_child`, reparent through the #49 helper, a stale `Children` entry ignored, a transform-less grouping parent, shear, the dangling `Parent`, and the `Changed<GlobalTransform2d>` contract.
  - [ ] Both types are reflected, the type names are pinned, and a `DynamicStruct` with `scale` missing rebuilt through `FromReflect` gets `Vec2::ONE` (no scene file and no `exitium_serial` dependency; scene loading is #111's).
  - [ ] The system is accepted by the scheduler's conflict check (the filter-disjoint queries), and its `SystemAccess` is asserted by a test.
- Blocked by: intake-2, A2, #49 (hierarchy), #50 (the reflect hook for components).

**A4. `perf(app): frame driver and propagation benchmarks`** (about 300 lines)
- Footprint:
  - Edits: `crates/exitium_app/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `benches/frame.rs` (the `[[bench]]` stanza is added here) for the frame-overhead, schedule-run, time-update, catch-up and propagation rows of the Performance table, each against a hand-written baseline.
- Acceptance:
  - [ ] Every row has a benchmark within the 5% gate and the numbers are in the PR body.
  - [ ] The flat 20,000-entity and 5,000-node hierarchy cases are included, with a case where every transform changes and one where none does.
- Blocked by: intake-2, A3.

**Stage 2: `exitium_platform::plugin` (depends on `exitium_ecs`, `exitium_app`, `exitium_reflect`).**

**PP1. `feat(platform): PlatformPlugin, platform events and input systems`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/mod.rs`, `plugin/systems.rs`; `cfg_attr(feature = "reflect", derive(Reflect))` with pinned type names on `KeyCode`, `MouseButton`, `InputEvent`, `InjectedInput`, `WindowConfig`, `WindowInfo`, `InputSnapshot` (`PlatformEvent` is not reflected); `PlatformPlugin`, the resources, the reflected event registration, `apply_platform_events`, the view switch, `end_tick`, `end_frame`, `InputSnapshot` refresh.
- Acceptance:
  - [ ] Injected `InjectedInput` events drive `InputState` through `First` in a headless app; a non-finite cursor and an oversized scroll are ignored or clamped; `PlatformEvent` has no reflection registration; the edge rules hold in a `FixedUpdate` system on a frame with no tick and on a frame with two ticks.
  - [ ] `Focused(false)` releases keys; `CloseRequested` sends `AppExit` only when `exit_on_close`.
  - [ ] Every type in the surface list reflects and round-trips; the injected event schema is readable by a generic tool.
  - [ ] `INPUT` is a set the UI glue can order after (a test orders a system `after_set(INPUT)`).
  - [ ] With `default-features = false` the crate still builds with no ECS dependency (the Stage 1 build).
- Blocked by: intake-2, PL5, A2.

**PP2. `feat(platform): windowed runner, window config and panic hook`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/runner.rs`, the live `WindowConfig` application in `Last`, the panic hook with `current_system()` on native and web, `PlatformPlugin::with_raw_event_hook`, and `examples/plugin_window.rs` (`App::new().add_plugins(PlatformPlugin::default())` with systems that print input in `Update` and `FixedUpdate`, so both edge views are visible).
- Acceptance:
  - [ ] The runner's `WindowHandler` is tested with a fake window: `ready` inserts `PrimaryWindow` and `WindowInfo` before `PreStartup`; `frame` returns `Exit` on `AppExit`; the raw hook consumes input events only.
  - [ ] A forced panic in a system prints the schedule and system names on native and in the Chrome console.
  - [ ] The PR body records a run of the example on Windows and in Chrome, including pressing a key on a frame with no tick (hold the display at a rate above the fixed rate) and seeing it once in `FixedUpdate`.
  - [ ] `PrimaryWindow` implements `Resource` and is the only window resource; its handle satisfies wgpu's surface target bound (asserted without depending on wgpu).
- Blocked by: intake-2, PP1, A2.

**Facade glue (not an edit to either crate).**

**F1. `feat(exitium): window and cursor glue for the renderer`** (about 200 lines)
- Footprint:
  - Edits: none
  - Governs: none
  - Hotspots: `crates/exitium/` (the facade)
  - New crate edges: none (`exitium -> exitium_app`, `exitium -> exitium_platform` and `exitium -> exitium_render` are in intake-2)
- The `PreStartup` and `First` systems that fill render's window resource from `PrimaryWindow` and `WindowInfo`, and the `First` system that fills `CursorWorld` from the cursor and the primary camera.
- Acceptance:
  - [ ] A window opens, the renderer takes the surface, and a resize reaches the renderer within one frame.
  - [ ] `CursorWorld` is correct for a camera with zoom, offset and rotation, and for scale factors 1.0 and 1.5 (it is built from `cursor_logical`); it is `None` outside the window.
  - [ ] A headless app (no `PrimaryWindow`, no renderer) runs a frame with the glue installed and does not panic; the glue systems read `Option<Res<..>>`.
  - [ ] Nothing in the glue is wasm-gated except what the render and platform APIs already gate.
- Blocked by: intake-2, PP2, A2, the render plugin issue that adds the window resource and the camera resolve (#107's issues).

**F2. `feat(exitium): UI input and viewport glue`** (about 200 lines)
- Footprint:
  - Edits: none
  - Governs: none
  - Hotspots: `crates/exitium/` (the facade)
  - New crate edges: none
- The `First` system (`after_set(INPUT)`, before the UI plugin's `ui_input`) that turns `InputState` (frame view) into the UI plugin's `UiInputFrame` resource, and syncs `UiViewport` from `WindowInfo`. The UI's own `ui_input` (set `UI_INPUT`) then processes it before any tick.
- Acceptance:
  - [ ] A button click through injected `InjectedInput` events is seen by a game `Update` system and by a `FixedUpdate` system in the same frame (this resolves the UI spec's "previous-frame capture" case).
  - [ ] `UiCapture.pointer` is true on the press, the held and the release frames of a UI press, and false for a gameplay press.
  - [ ] With no platform plugin the `UiInputFrame` resource keeps its default and no UI event fires; the glue reads `Option<Res<InputState>>`.
- Blocked by: intake-2, F1, PP2, the UI plugin issue U11 (#109's issues).

**F3. `feat(exitium): app and platform re-exports and DefaultPlugins entry`** (about 150 lines)
- Footprint:
  - Edits: none
  - Governs: none
  - Hotspots: `crates/exitium/` (the facade)
  - New crate edges: none
- Re-export `App`, `Plugin`, `PluginGroup`, the ten schedule labels, `Time`, `TimeConfig`, `Transform2d`, `GlobalTransform2d`, `AppExit`, `KeyCode`, `MouseButton`, `InputState` and `WindowInfo`; add `PlatformPlugin` to `DefaultPlugins` first.
- Acceptance:
  - [ ] A game that depends only on `exitium` builds a window app with a `FixedUpdate` system that reads `ButtonInput`.
  - [ ] A facade doc test builds a headless app and runs one frame through `exitium::` paths only.
  - [ ] The facade API change gets the extra reviewer scrutiny of ADR-0005 (high-risk area).
- Blocked by: intake-2, A3, PP2, and the facade issue that creates `DefaultPlugins`.

**Adjustments to existing issues** (the architect edits the bodies after this spec merges):

- **#49 (hierarchy):** its Context says transform propagation is "scene crate, Phase 2". It is `exitium_app` (A3). Add the ECS-side needs: `Children` exposes its slice and `Parent` its entity, and `despawn_recursive` and the helpers keep `Children` consistent, since propagation trusts them.
- **#22 items 11 and 13 (the ECS scheduler):** `ScheduleLabel::new` and `SystemSet::new` are `const fn`; `register_reflected_event::<E>` takes the clock. Both are one line in the issue bodies.
- **UI (#109), U11:** `ui_input` takes a UI-owned `UiInputFrame` resource and stores `UiCapture`, and names no platform type (`exitium_ui` has no edge to `exitium_platform`, C1). F2 fills the resource from `InputState`. U11's headless tests write a `UiInputFrame` into the resource. `ui-mvp.md` is amended in this PR to match (the Stage 2 text, open questions 3 to 5 and U11).
- **Physics (#108) and UI (#109) issues:** their plugin issues are `Blocked by` A1 and A2 and use the `FixedFirst`, `FixedLast`, `PostUpdate` (before `PROPAGATE`) and `UI_INPUT` slots; resources are inserted in `build`.
- **Render (#107) issues:** `Extract` with the `EXTRACT` and `RENDER` sets, `Startup` for renderer init, `PreStartup` for the facade's window hand-off; no interpolation in v1.

## Intake wiring

For the Stage 1 and Stage 2 intake PRs (C5: the union of every merged spec's list), so that no implementation issue touches `Cargo.lock`, a manifest's dependency list or features, or `xtask/`.

**Intake 1 (Stage 1).**

- Workspace crate edge: `exitium_platform -> exitium_core`. No other edge. In particular `exitium_platform` has no edge to `exitium_ecs`, `exitium_app`, `exitium_reflect` or any other runtime crate in Stage 1.
- `[workspace.dependencies]` (declared once; versions checked 2026-11-18):

| Crate | Version | Features | Licence | wasm32 | Why not std |
|---|---|---|---|---|---|
| `winit` | `0.30.13` | `default-features = false`, `["rwh_06", "x11"]` | Apache-2.0 (allowed by `deny.toml`) | yes (`spawn_app`, canvas) | std has no window or event loop. 0.31 is beta. The version matches what `wgpu` 30 and `egui-winit` 0.37 need. `x11` is there so the Linux CI build compiles: winit 0.30 needs one of `x11` or `wayland` on Linux, and `x11` loads libX11 at run time (`x11-dl`), so no system library is needed to build, and the Linux runners never open a window. Intake verifies that the Linux image builds it, and falls back to `wayland` if it does not. |
| `web-sys` | `0.3.107` | `Window`, `Document`, `Element`, `HtmlCanvasElement`, `console` | MIT OR Apache-2.0 | wasm only | The canvas lookup and the console panic hook need the DOM. winit already depends on `web-sys` with all of these features, so the lock gains no package and no feature that is not already built. |
| `wasm-bindgen` | `0.2.130` | default | MIT OR Apache-2.0 | wasm only | `JsCast` for the canvas element, and the `wasm-bindgen` CLI recipe of the examples. Also already in winit's wasm tree. The CLI used by the recipe must equal the locked version. |
| `web-time` | `1.1.0` | default | MIT OR Apache-2.0 | yes (it re-exports `std::time` on native, so one code path serves both) | `std::time::Instant::now()` panics on `wasm32-unknown-unknown`. Also already in winit's wasm tree. |

- `crates/exitium_platform/Cargo.toml`: `[dependencies] exitium_core.workspace = true, winit.workspace = true`; `web-time.workspace = true` (all targets); `[target.'cfg(target_family = "wasm")'.dependencies] web-sys.workspace = true, wasm-bindgen.workspace = true`; `[dev-dependencies] criterion.workspace = true`. **No other dev-dependency:** the example uses the crate's own dependencies and never `exitium_render` (C1). The `[[bench]]` stanza is added by PL1.
- `raw-window-handle` is **not** a direct dependency: `winit` re-exports `raw_window_handle` (0.6) under `rwh_06`, which is the version `wgpu` 30 uses.
- `deny.toml`: no change expected (every licence is MIT or Apache-2.0; winit's Windows tree is `windows-sys` and similar). Intake runs `cargo deny check`.
- `xtask`: no `NATIVE_ONLY` entry for either crate. Proposed, optional: the wasm step also runs `cargo build --examples --target wasm32-unknown-unknown -p exitium_platform`, so the example's web build is checked in CI and not only by hand (this is a change to the `xtask/` hotspot, so it belongs in the intake PR).
- `exitium_app` gains **no** Stage 1 change: it has no workspace dependency until intake 2.

**Intake 2 (Stage 2 to 4).**

- Workspace crate edges: `exitium_app -> exitium_core`, `exitium_app -> exitium_ecs`, `exitium_app -> exitium_reflect`. `exitium_platform -> exitium_ecs`, `-> exitium_app`, `-> exitium_reflect`, as optional dependencies behind features. Facade: `exitium -> exitium_app` and `exitium -> exitium_platform` (the facade depends on every runtime crate).
- `crates/exitium_platform/Cargo.toml`: optional dependencies `exitium_ecs`, `exitium_app`, `exitium_reflect`; features `reflect = ["dep:exitium_reflect"]`, `plugin = ["reflect", "dep:exitium_ecs", "dep:exitium_app"]`, `default = ["plugin"]`. Standalone (Stage 1 style) use is `default-features = false`; xtask should check that build too (a `--no-default-features` check for the platform crate, if the intake finds that `FEATURE_CHECKS` cannot express it, a note in the PR).
- `crates/exitium_app/Cargo.toml`: `[dependencies] exitium_core.workspace = true, exitium_ecs.workspace = true, exitium_reflect.workspace = true`; `[dev-dependencies] criterion.workspace = true`. **No crates.io dependency.** The ECS derives (`Resource`, `Component`, `Event`) are re-exported by `exitium_ecs` (item 16), and `Reflect` by `exitium_reflect`, so no derive crate is named.
- **Not added, on purpose:** any edge from `exitium_app` or `exitium_platform` to `exitium_render`, `exitium_physics`, `exitium_audio`, `exitium_assets`, `exitium_ui`, `exitium_scene`, `exitium_command`, `exitium_mcp` or `exitium_editor` (C1, C3); an edge from `exitium_app` to `exitium_platform` (the platform plugin depends on the app, never the reverse); `console_error_panic_hook` (a 20-line function replaces it); `wasm-bindgen-futures` and `js-sys` (nothing here is async); `raw-window-handle` (through winit); `egui-winit` and a direct `winit` entry for the editor (#110's intake list; it reuses this workspace entry, which keeps one winit version).
- **Crate graph rule:** `exitium_app` and `exitium_platform` stay runtime crates that build for `wasm32-unknown-unknown` and are not in `NATIVE_ONLY`. A change that would break C1 or C3 is a spec error, not intake input.

**Known graph after both intakes** ("A -> B" means A depends on B):

```
exitium_app             -> exitium_core, exitium_ecs, exitium_reflect
exitium_platform        -> exitium_core                                   (Stage 1)
exitium_platform        -> exitium_core, exitium_ecs, exitium_app, exitium_reflect   (Stage 2, feature `plugin`)
exitium (facade)        -> exitium_app, exitium_platform, ... (every runtime crate)
```

## Cross-spec resolutions

Each open dependency that another merged or open spec names on this one, and where it is resolved here.

| From | Dependency | Resolution |
|---|---|---|
| Physics (#108) | Ordering before and after the game's `FixedUpdate` systems, and late in `Update` before propagation | `FixedFirst` (`reconcile`), `FixedLast` (`sync_in`, `step`, `write_back`), `PostUpdate` with `before_set(PROPAGATE)` (`interpolate`, `reconcile`, `clear_events`, `debug_draw`). See "Schedules and the frame loop". |
| Physics | Fixed-step alpha | `Time::fixed_alpha`, valid in `Update` and later; `Time::fixed_step`. |
| Physics | `Transform2d` and `GlobalTransform2d` shape | Translation `Vec2`, rotation radians, scale `Vec2`; the global is an `Affine2`, written only when it changes. |
| Physics | Open question 1 (units and the y axis) | y up, counter-clockwise radians; units belong to the game, the camera and `units_per_meter`. |
| Physics | Open question 3 (event channel) | ECS events with the `Fixed` clock serve per-tick delivery; the driver updates both clocks. |
| UI (#109) | `ui_input` after platform input and before `FixedUpdate` | Sets `INPUT` then `UI_INPUT` in `First`, which runs before every tick. The conversion is facade glue (F2) that fills a UI-owned `UiInputFrame` resource, because C1 forbids a UI to platform edge; `ui-mvp.md` is amended to match. |
| UI | Open question 3 (platform input shape) | `InputState` in the `Frame` view: `cursor_logical`, `mouse`, `keys`, `scroll_lines`. |
| UI | Open question 4 (consumption) | The platform never masks. `UiCapture` is the contract. |
| UI | Open question 5 (ordering, extraction glue position) | `UI_INPUT` in `First`; facade glue in `Extract`, set `EXTRACT`. |
| Render (#107) D1 | `GlobalTransform2d` as a 2D affine, propagated before extraction; `render_frame` after `extract_*` and glue | `GlobalTransform2d(Affine2)`; propagation in `PostUpdate`, which runs before `Extract`; sets `EXTRACT` before `RENDER`. |
| Render D2 | A window resource for the surface, size, scale factor and resize events | `PlatformWindow` (raw handle traits), `PrimaryWindow`, `WindowInfo`, `PlatformEvent::Resized`. Facade glue (F1) fills render's own resource (C1). |
| Render | Open question 3 (interpolation) | Not in the MVP; additive path in open question 6. |
| ECS (#22) | The labels, the per-frame order, the `Time` swap, `update_events` calls | Ten labels and the order above; no swap; both clocks driven by the loop. |
| Roadmap (`jam-mvp.md`) | `exitium_app` ownership list; C1; C5 and C6; Stage 1 platform lane; Windows and Chrome only | As above; the lane ends with PL5, which runs on both. |
