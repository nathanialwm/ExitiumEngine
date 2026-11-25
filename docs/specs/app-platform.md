# Spec: App loop, plugins, transforms and platform (window, input)

- Issue: #106. Amended by #139 (SP3 of `mvp-roadmap.md`, 2026-11-24): platform gamepad, the editor hooks (`editor-mcp-mvp.md` AM2 and AM3) and the supersession of the control presets. Changed sections: Non-goals, Boundaries, Module layout, the frame, "Executors and the thread pool" (from SP2, #138), "`SimulationMode`" (including the rule that engine glue never lives in `Update`), Input, "Gamepad", Window and the event loop, Stage 2, Threading, "MCP / editor surface", Performance, Test plan, Open questions, Implementation issues, Intake wiring and Cross-spec resolutions.
- Crates: `exitium_app` and `exitium_platform`. `exitium_app` depends on `exitium_core`, `exitium_ecs` and `exitium_reflect` (and, behind its `profiling` feature, `exitium_profiler`) and has no external dependency. `exitium_platform` is Stage 1 standalone (`exitium_core`, `winit` and `web-time`, plus `web-sys` and `wasm-bindgen` on wasm32, plus `gilrs` on Windows and wasm32). Its `plugin` module (Stage 2) adds `exitium_ecs`, `exitium_app` and `exitium_reflect`. Neither crate has an edge to render, physics, audio, assets, ui, scene, command, editor or mcp, and `exitium_app` has no edge to `exitium_platform` (C1). Cross-crate glue lives in the facade (see "Facade glue").
- Status: draft. The owner-approved contract (2026-11-18) fixes the ownership of `App`, `Plugin`, the schedules, `Time` and `Transform2d`, and fixes Windows and Chrome as the only tested targets. Everything else is the author's proposal. The cross-spec rules C1 to C6 are orchestrator rulings (`jam-mvp.md`).

## Goal

Specify the two crates every other runtime crate plugs into.

- **`exitium_app`** owns the plugin model, the `App` builder, the frame loop (Startup, fixed-rate ticks, variable update, render extraction), `Time`, the headless mode, and `Transform2d` / `GlobalTransform2d` with parent/child propagation. It maps those schedules onto the ECS scheduler of #22.
- **`exitium_platform`** owns the winit window and event loop on Windows and on the web, resize and high DPI, keyboard, mouse and gamepad state, the editor's raw-event hook and the wake handle. In Stage 1 it is a plain library with an example. In Stage 2 a `plugin` module makes it a `PlatformPlugin` for `exitium_app`.

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

- **Gamepad force feedback and gyro, pad-driven UI navigation, and touch.** Gamepad input itself is in ("Gamepad" below). `InputEvent` is `#[non_exhaustive]`, so rumble (gilrs offers it), gyro and touch each add variants and a state type without changing anything else. Spatial UI navigation by pad is `ui-mvp.md`'s call (SP6).
- **Text input and logical (layout-dependent) keys in the game.** `KeyCode` names the key's position, as the W3C `code` does, and the platform delivers no text. The editor's egui gets text and IME from `raw_event`, which now sees every `WindowEvent` ("The editor's raw-event hook"). A text field in the game UI is `ui-mvp.md`'s call and, if it needs text events, adds `InputEvent` variants. The platform side of text input (committed text, IME, clipboard) is a separate amendment, #177, which waits for this PR.
- **Multiple windows, window decorations and cursor grab.** One primary window. The resource names (`PrimaryWindow`) leave room for more.
- **Render interpolation of non-physics movers.** A game system that moves a `Transform2d` in `FixedUpdate` is drawn at the tick rate. The path is an opt-in `PreviousTransform2d` plus a `PostUpdate` blend by `Time::fixed_alpha` (open question 6). Physics interpolates its own bodies (#108).
- **Keeping the world pose when reparenting.** `set_parent` keeps the local transform, so the world pose changes. A `reparent_keep_world` helper is additive.
- **A real-time headless runner.** The default headless runner runs frames back to back with simulated time. A sleeping runner needs `std::thread::sleep`, which wasm32 lacks, so it is native only and belongs to the server runtime of `net-server.md` (SP16), which uses `run_frame(seconds)` as its entry point (open question 8).
- **State machines and system enable/disable.** State gating is by schedule label (#22). Play-in-editor is `editor-mcp-mvp.md`'s, built on `TimeConfig` and `SimulationMode` (below).
- **Changing the loop through MCP beyond data.** Systems are Rust (the contract). Agents control the loop through the `TimeConfig` and `SimulationMode` fields (below) and, in an editor build, the `play_*` tools.

## Design

### Boundaries (who owns what)

| Concern | Owner |
|---|---|
| `System`, `Schedule`, `Schedules`, `Commands`, resources, events, `run_schedule`, `update_events` | `exitium_ecs` (#22) |
| `App`, `Plugin`, schedule labels and sets, the frame loop, `Time`, `TimeConfig`, `SimulationMode`, `AppExit`, headless mode, `Transform2d`, `GlobalTransform2d`, `CursorWorld` (the slot) | `exitium_app` (this spec) |
| `FixedClock` (the accumulator and the tick cap), the plain `Time` snapshot | `exitium_core` (frozen, reused) |
| winit window and loop, `InputEvent`, `InputState`, `GamepadState`, `GamepadConfig`, `WindowInfo`, `PrimaryWindow`, `WakeHandle`, the raw-event hook slot, `PlatformPlugin` | `exitium_platform` (this spec) |
| Window to renderer, cursor to world, platform input to `UiInputFrame`, `DefaultPlugins`, the default task pool (`ThreadingPlugin`) | the facade `crates/exitium/` (a hotspot, C1) |
| Command bus, live-control server, the `play_*` tools (they set `SimulationMode` and `TimeConfig`), the egui raw-event hook | `exitium_command`, `exitium_editor` (#110, `editor-mcp-mvp.md`) |

### Module layout

```
exitium_app/src/
  lib.rs         re-exports, crate docs
  app.rs         App, AppError, Runner, boundary hooks                        safe
  plugin.rs      Plugin, PluginGroup, PluginGroupBuilder, Plugins             safe
  schedule.rs    schedule labels (unit structs), sets (consts), IntoSystemConfigs   safe
  frame.rs       run_frame, FrameReport, AppExit, the headless runner         safe
  time.rs        Time, TimeConfig, FrameStats, SimulationMode                 safe
  cursor.rs      CursorWorld (data only)                                      safe
  transform/mod.rs        Transform2d, GlobalTransform2d, hierarchy commands  safe
  transform/propagate.rs  ensure_global_transforms, propagate_transforms      safe

exitium_platform/src/
  lib.rs
  input/key.rs     KeyCode, MouseButton                                       safe
  input/button.rs  ButtonInput<K>, EdgeView                                   safe
  input/event.rs   InputEvent, PlatformEvent, InjectedInput                      safe
  input/state.rs   InputState, InputSnapshot                                  safe
  input/gamepad.rs GamepadId, GamepadButton, GamepadAxis, GamepadState, GamepadConfig, GamepadSnapshot   safe
  gamepad/translate.rs  gilrs values to InputEvent, pure functions            safe
  gamepad/backend.rs    GamepadPoller over gilrs (cfg windows or wasm; a stub elsewhere)   safe
  window/mod.rs    WindowConfig, WindowInfo                                   safe
  window/handle.rs PlatformWindow (raw handle traits), PrimaryWindow          safe
  window/wake.rs   WakeHandle                                                 safe
  window/translate.rs  winit values to PlatformEvent, pure functions          safe
  window/route.rs  WindowEventKind, classify, translate_after_hook (the raw_event consumption table)   safe
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
| `Update` | every frame, **except while `SimulationMode::Frozen`** | Game systems that follow the frame rate (animation, camera, UI logic). Never engine glue that must run while authoring (see "`SimulationMode`"). |
| `PostUpdate` | every frame | Systems that write `Transform2d` from other state (physics interpolation), then transform propagation, then the UI's layout and draw list (set `UI`, owned by `ui-mvp.md`) and the asset request scan. |
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
  0. open the `exitium_app::frame` profiler span           // feature `profiling`, see "Executors and the thread pool"
  1. each boundary hook runs, outside any schedule        // the live-control seam, below
  2. if not started: run PreStartup, Startup; started = true
  3. Time update (see "Time" for the arithmetic) -> ticks, alpha    // SimulationMode::Frozen: ticks = 0, delta = 0
  4. run First
  5. repeat `ticks` times:
       Time.tick_count += 1                               // so a tick sees a count that includes itself
       run FixedFirst, FixedUpdate, FixedLast
       world.update_events(Fixed)
  6. Time.fixed_alpha = clock.remainder() / clock.step()
  7. run Update (not when Frozen), PostUpdate, Extract, Last
  8. read AppExit events; world.update_events(Frame)
     close the `exitium_app::frame` span; exitium_profiler::mark_frame()   // feature `profiling`
  9. return FrameReport { ticks, dropped_seconds, systems_run, applied, skipped_dead_entity, exit }
```

Each "run X" is `world.run_schedule(X)` (#22). A `ScheduleError` (a cycle, an access conflict, a missing resource id) is returned as `AppError::Schedule` and is a programmer error: the native runner prints it and exits with code 1, and `App::update` panics with it. `App::build()` compiles every schedule early, so the same error shows at startup and not on the first frame (an agent runs it to check a game without a window).

**Event clocks (resolves #22 open question 4).** Both clocks stay. The driver calls `update_events(Fixed)` after each tick and `update_events(Frame)` once per frame, exactly as #22 specified. `App::add_event::<E>()` uses `Frame`, `add_fixed_event::<E>()` uses `Fixed`. An event sent in `Update` and read in `FixedUpdate` can expire if two consecutive frames run no tick (possible at 144 Hz display and 60 Hz fixed rate), so events produced and consumed in the fixed schedules use `Fixed`, and `EventReader::missed()` reports the rest. Plugins may use ECS events for per-tick delivery (this answers physics open question 3).

**Executors and the thread pool** (amended, #139, from the SP2 amendment #138, which makes the parallel executor and `par_iter_mut` MVP scope). SP2's `ExecutorKind::Parallel` and `par_iter_mut` do nothing unless a pool is installed and a schedule asks for the executor, and `ecs-scheduler.md` leaves both decisions to this spec ("Open question 12").

- **Which schedules run `Parallel`.** `Update` and `PostUpdate`. `Parallel` runs the same plan as `SingleThreaded` with the same sync points, and it resolves to `SingleThreaded` when the pool has one thread and on `wasm32`, so `exitium_app` needs no thread-count test and no `cfg`. `PreStartup`, `Startup`, `First`, `Extract` and `Last` stay `SingleThreaded`: they run once, or hold the platform ingest, the render submit and the facade glue, and only the main thread may own those. **`FixedFirst`, `FixedUpdate` and `FixedLast` stay `SingleThreaded`:** the simulation steps are the ones that should repeat (netcode, replays, the offline catch-up), and the parallel executor's order between two independent systems is unspecified. `SingleThreaded` fixes only the order of the **systems**: they run one at a time on the main thread in plan order. It does **not** make a system's own parallel work repeatable. A fixed-step system that calls `par_iter_mut` or `exitium_tasks::parallel_for` (physics may, inside its own step, which is its spec's decision) still runs that work on the process default pool, because the executor does not scope it (`ecs-scheduler.md`, "Determinism"). On a multi-thread default pool its row order and its floating-point reduction order are then unspecified. See "Repeatable runs" below for what a replay needs.
- **Who installs the pool.** The facade, not the app: a `ThreadingPlugin` in `crates/exitium/` (F4, in `DefaultPlugins` before every other plugin) builds the pool and calls `exitium_tasks::TaskPool::set_default(Arc<TaskPool>)` in its `build`, before any schedule runs. That keeps `exitium_app` free of an `exitium_tasks` edge (the app only names `exitium_ecs::ExecutorKind::Parallel`), and `exitium_tasks` has no edge to the app. `ThreadingPlugin { budget: Option<ThreadBudget> }`: `None` resolves `ThreadBudget::detect()` (environment variable, then the default detection of the tasks spec), and `Some(budget)` is explicit. The `SetThreadBudget` command of the tasks spec rebuilds the pool at a sync point and calls `set_default` again; that is the tasks spec's, not the app's.
- **Without the plugin, nothing is parallel, and nothing breaks.** A headless test or tool that builds an `App` with no `ThreadingPlugin` has no default pool, so the tasks crate's inline pool (one thread) runs every `Parallel` schedule exactly like `SingleThreaded`.
- **Repeatable runs.** `SingleThreaded` is not enough on its own, and pinning a pool on one schedule does not help: `Schedule::set_task_pool` only changes the pool the parallel executor's own segments use, and does not pin the nested `par_iter_mut` or `parallel_for` calls that a system makes (`ecs-scheduler.md`, "Determinism"; the fixed schedules are `SingleThreaded`, so it has no effect on them at all). A test, benchmark, replay, server or offline catch-up that needs the same result every run needs **one of two things**, the two options of the ECS spec:
  1. **A one-thread default pool:** `ThreadingPlugin { budget: Some(ThreadBudget::single()) }` (or `EXITIUM_THREADS=1`, or no `ThreadingPlugin` at all). Every nested call then runs inline in a fixed order, and every `Parallel` schedule runs as `SingleThreaded`. This is what the test suite and the baselines of the Performance table use.
  2. **Fixed-step bodies that do not depend on cross-row order:** disjoint writes, `Batching::Fixed(n)` for reproducible chunk boundaries, and per-chunk partial results that are combined in chunk order. Those systems give the same result on any pool, and a game that wants a repeatable multi-threaded build writes them this way.

  The engine promises neither by default: a shipped game on a multi-thread pool is not bit-repeatable unless its fixed-step systems follow option 2. A netcode or replay feature (a later spec) must pick one and test it.
- **Profiling hooks.** `run_frame` opens one `exitium_profiler` span named `exitium_app::frame` (the profiler's `crate::name` convention, `profile_scope!("exitium_app::frame")`) at step 0 (before the boundary hooks) and closes it after step 8, so the span covers hooks, ticks, `Update`, `Extract` (the submit and present) and `Last`. It calls `exitium_profiler::mark_frame()` once as the last thing before step 9, so every `run_frame` is one frame mark: a windowed frame, a headless frame and a `Frozen` frame alike. Both are behind `exitium_app`'s `profiling` feature (default off, so the span expands to nothing and `mark_frame` is not called); the feature adds the optional edge `exitium_app -> exitium_profiler` ("Intake wiring"). The profiler's `tracy` feature and `init` belong to the profiler spec; this spec only places the two calls.

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

- **`App::new()`** creates the `World`, inserts `AppTypeRegistry` (reflect-model, "Registry ownership": the app only inserts it), `Time`, `TimeConfig`, `SimulationMode` (`Running`), `FrameStats`, `CursorWorld`, and registers `AppExit` as a `Frame` reflected event. It creates all ten schedules, each with the executor of "Executors and the thread pool" below (`Update` and `PostUpdate` get `ExecutorKind::Parallel`, the other eight stay `SingleThreaded`), applies the set ordering above, and adds `ensure_global_transforms` and `propagate_transforms` to `PostUpdate`. It registers `Transform2d`, `GlobalTransform2d`, `Time`, `TimeConfig`, `SimulationMode`, `FrameStats`, `CursorWorld` and `AppExit` with the registry. The app therefore works headless with no plugin.
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
- **Pause and step.** `paused` stops the tick loop and zeroes `delta`; `Update`, `PostUpdate` and `Extract` still run every frame, so a paused game is still drawn and the editor can still pick. `step_ticks = n` while paused runs exactly `min(n, max_ticks)` ticks next frame, with `delta` still 0 and `fixed_step` unchanged, then resets itself. An agent debugs by pausing, stepping one tick and reading the world. This is two fields and no command (see the surface section). In an editor build the `play_*` tools own these two fields, together with `SimulationMode` below and the play snapshot.
- **No `Time` swap.** #22 left "the `Time` swap" to the app. This spec does not swap. Core's `Time` already says fixed-rate systems use `fixed_step` and `tick_count` and never `delta`, so one resource serves both schedules and a system reads the same type everywhere. Physics reads `Time::fixed_step` and `Time::fixed_alpha` (this resolves physics open question 2 and the "open dependency on #106" in its fixed-step section).

**`FrameStats`** (reflected resource, written by the loop at the end of each frame, so it is stale inside a frame): `frame_count: u64`, `last_ticks: u32`, `last_systems_run: u32`, `dropped_seconds_total: f64`, `stalled_frames: u32` (frames that hit either clamp), `config_clamped: u32`, `frozen_frames: u64` (frames run with `SimulationMode::Frozen`). These are the "last RunReport per schedule" aggregates #22 promises to MCP; the per-schedule detail stays in `Schedules::last_report` (#22).

**`AppExit`** is `#[derive(Event, Reflect)] struct AppExit { pub code: u8 }`, `Frame` clock. Any system, hook or agent sends it (`world.send_event(AppExit { code: 0 })` or the generic `SendEvent` command). The frame that sees it completes, including `Last`, and `run_frame` returns it in `FrameReport::exit`. The runner then stops.

### `SimulationMode` (amended, #139; the app half of `editor-mcp-mvp.md` AM3)

`TimeConfig.paused` stops the ticks and leaves `Update` and `PostUpdate` running, so a paused game is still drawn and its frame-rate systems (animation, camera, UI logic) still move. That is right for a human pausing a game and wrong for the editor's Edit mode, where a game that mutates the world in `Update` would change the scene being authored. `SimulationMode` is the second switch: it freezes the simulation but not the frame.

```rust
#[derive(Resource, Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Resource, Default, type_name = "exitium_app::SimulationMode")]
pub enum SimulationMode { #[default] Running, Frozen }
```

| `SimulationMode` | `TimeConfig.paused` | Ticks | `Update` | `First`, `PostUpdate`, `Extract`, `Last` |
|---|---|---|---|---|
| `Running` | false | per `Time` | runs | run |
| `Running` | true | none, except `step_ticks` | runs | run |
| `Frozen` | any | none; `step_ticks` is not consumed | **skipped** | run |

- **Read once per frame**, at step 3, after the boundary hooks. A hook that sets the mode applies to the same frame.
- **What Frozen does at step 3.** `ticks = 0`, `delta = 0`, `dropped_seconds = 0`. `elapsed`, `tick_count`, `step_ticks`, `fixed_alpha` and the `FixedClock` remainder are left untouched, and `real_delta` is still computed. Leaving Frozen therefore never causes a catch-up burst: no time accrued while frozen. Step 5 is skipped, and step 7 runs `PostUpdate`, `Extract` and `Last` but not `Update`. `FrameStats::frozen_frames` counts these frames.
- **What still runs, and why.** `First` (platform ingest, UI input, the facade glue), so the editor still gets input and the cursor; `PostUpdate`, so a `Transform2d` the editor wrote through the bus is propagated and drawn the same frame; `Extract`, so the frame is rendered; `Last`, so input edges clear. `PreStartup` and `Startup` still run once on the first frame in either mode, so the scene's setup exists when Edit mode starts. A system that must run while authoring goes in `First`, `PostUpdate` or a boundary hook, never in `Update`.
- **Engine glue never lives in `Update`** (the rule). Frozen skips `Update`, but merged specs put engine glue there that the editor needs in Edit mode: with the asset feeding glue (`assets-scene-export.md` FA1) in `Update`, a texture would never reach the renderer, and a scene load (`LoadScene`, FA3) would never complete. So **every engine system that must run while authoring runs in `First` or `PostUpdate`**, or in another schedule that Frozen still runs, and a **game** keeps `Update` for rules that should freeze with the world. The systems this moves, named so that no issue has to guess:

  | System | Today (merged spec) | Moves to | Owner of the fix |
  |---|---|---|---|
  | Asset delivery to render, UI and audio: `take_loaded(Image)`, `take_loaded(Font)` and the retry rule (FA1), and the hot reload swap (FA5) | `Update`, after the assets plugin's `ASSETS` set | `First`, after `ASSETS` (`assets-scene-export.md` places `ASSETS` itself in `First`, after `INPUT`) | SP4, #140 |
  | The change-driven request scan for `Sprite`, `Text` and UI image and font `Guid`s (FA1) | `Update` | `PostUpdate`, after the systems that write those components (it needs no `First` slot; it runs before `Extract`, which is a later schedule) | SP4, #140 |
  | Audio feeding glue (FA2, `take_loaded(Audio)` into `ClipStore`) | `Update` | `First`, after `ASSETS` | SP4, #140 |
  | `apply_audio_settings` and `audio_events` (`AudioPlugin`: `PlayAudio`, `StopAllAudio`, `PlayMusic`, `StopMusic`) | `Update` | `First`, after `ASSETS` (without `audio_events` the editor's play button would be silent in Edit mode) | SP4, #140 |
  | Scene loading glue (`ScenePlugin`: `LoadScene`, `SceneLoaded`, `SceneLoadFailed`), FA3 | `Update` | `First`, after `ASSETS`, so the scene opens in Edit mode and the `PostUpdate` of the same frame propagates the new transforms | SP4, #140 |
  | The UI plugin's `Headless` driver (layout and draw list, `ui-mvp.md`) | the end of `Update` | `PostUpdate`, in the set `UI`, which `ui-mvp.md` declares and orders `after_set(PROPAGATE)` (so the driver sees the frame's global transforms) | SP6, #142 |

  Unchanged and already outside `Update`: the autoplay unlock (FA2) stays in `First`, after `INPUT`; `audio_update` stays in `Last`; `ui_input` is in `First`; the physics and transform systems are in `PostUpdate`.

  **What this section decides, and what it does not.** This section owns the **rule** (engine glue never in `Update`, always in `First` or `PostUpdate`). The **schedule and set each system lands in** is its owning spec's choice: SP4 (#166) has chosen `First` after `ASSETS` for delivery, the swap, scene loading and the audio systems, and `PostUpdate` for the request scan, and this table records those choices so that the two specs read the same. Where a spec picks another slot that is also `First` or `PostUpdate`, that spec wins and this table is edited to match. A spec that leaves a system in `Update` breaks the rule, and the rule wins ("Cross-spec resolutions"). `UI`, `ASSETS` and the other sets named here are declared by their own specs (`exitium_app::sets` lists only the five of "Schedules and the frame loop"). A game system that wants to run in Edit mode also goes in `PostUpdate` (open question 13).
- **Input edges while Frozen** (the fix for the stale edge after `play_start`). No tick runs while Frozen, so `end_tick` never fires, and a key or pad button pressed in Edit mode would still read as `just_pressed` in the first `FixedUpdate` after `play_start`. So the platform plugin's `Last` system calls `end_tick` on `InputState` and `GamepadState` on every frame it sees `SimulationMode::Frozen` (before `end_frame`). The Fixed edge sets are therefore empty on the first Running frame. A press made on the Running frame itself, even before the tick, is kept as usual. Only `Frozen` does this: a human pause (`paused = true`, `Running`) keeps the edges, so a press during a pause is seen by the `step_ticks` tick that follows.
- **Events.** A `Frame`-clock event sent while Frozen and read only by `Update` or fixed-schedule systems expires unread. That is intended: the world is frozen.
- **Who sets it.** Nothing in `exitium_app` or `exitium_platform` does. The default is `Running`, so every game, every headless test and every shipped build is unchanged. The editor sets `Frozen` on entering Edit (the start and `play_stop`) and `Running` on `play_start` and `play_resume`; a human pause (`play_pause`) stays `Running` with `paused = true` (`editor-mcp-mvp.md`, D9).
- **Why a resource the driver reads, and not a system run condition or a schedule.** The ECS gates by schedule label and has no per-system run condition (#22). One resource read in `run_frame` costs nothing and adds no ECS concept. The schedule list and the set ordering are unchanged.

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
    GamepadConnected { pad: GamepadId },                 // the four gamepad variants are defined under "Gamepad" below
    GamepadDisconnected { pad: GamepadId },
    GamepadButton { pad: GamepadId, button: GamepadButton, pressed: bool },
    GamepadAxis { pad: GamepadId, axis: GamepadAxis, value: f32 },   // raw: sticks in [-1, 1] with y up, triggers in [0, 1]
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
    pub fn apply(&mut self, e: &InputEvent);                     // the single mutation path for keyboard and mouse; a CursorMoved while the cursor is None sets it with no delta; the gamepad variants are ignored here and go to `GamepadState`
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

### Gamepad (`exitium_platform`) (amended, #139)

Gamepad is one more source of `InputEvent`, on the same road as keyboard and mouse: **device, then `GamepadPoller`, then `PlatformEvent::Input` (or an agent's `InjectedInput`), then `GamepadState::apply` in `First`.** Everything else in the input design carries over: edges are relative to the reading schedule (the same `ButtonInput` and `EdgeView`), the platform never masks, and focus loss releases everything.

**Dependency decision: `gilrs` 0.11.2** (crates.io, 2026-11-24), the only new crates.io dependency of this amendment. Licence `Apache-2.0 OR MIT` (`gilrs-core` 0.6.8 the same). MSRV 1.84, under the pinned toolchain. The intake PR records the rest ("Intake wiring").

- **Why not std or hand-written.** std has no gamepad API. Native Windows device access is WGI (`Windows.Gaming.Input`) or XInput, both FFI, and `unsafe` is denied workspace-wide; a bespoke wrapper would need a designated unsafe module and Miri tests for no gain. The browser side is safe code over `web_sys` but would still need a button and axis mapping layer. `gilrs` does both behind one polling API. The alternatives are worse here: the SDL bindings (`sdl2`, `sdl3`) need a C library (the R5 toolchain risk) and do not build for `wasm32-unknown-unknown`, and an XInput-only crate is Windows-only.
- **Backends used.** Windows: the `wgi` feature (Windows 10 and later). The `xinput` feature is not enabled: it is the older API, needs two more crates, and WGI covers Xbox-layout pads. **`wgi` and `xinput` are mutually exclusive** in `gilrs-core` (enabling both is a `compile_error!`), so there is no fallback between them; a later wish for XInput replaces `wgi` and does not add to it. Web: the browser Gamepad API through `web_sys` (`gilrs-core` enables the `web-sys` features it needs by feature unification, so the workspace `web-sys` entry does not change). Features: `default-features = false, features = ["wgi"]` (the default is `wgi`; the entry says so explicitly). `serde-serialize` stays off, because the engine reflects its own types.
- **Target gating.** `gilrs-core` on Linux links `libudev` through `libudev-sys` and needs the system library to build, which the CI image does not have and the tested targets (Windows, Chrome) do not need. So the dependency is target-specific, `cfg(any(windows, target_family = "wasm"))`, and on every other target `gamepad/backend.rs` is a stub whose poller never reports a device. The public API has no `cfg` (only that one file does), so Linux CI and the wasm build compile the same types. Linux and macOS pads come with the Linux and macOS ports (roadmap, owner decision 3).
- **A thread that is not ours, and only one.** On Windows `gilrs-core`'s WGI backend starts one background thread that reads the devices every 8 ms and sends events over a channel. It touches no engine data: our poller only drains the channel on the main thread ("Threading"). **Force feedback is switched off** (`GilrsBuilder::with_force_feedback(false)`, see "The poller"): gilrs enables it by default, and it starts a second, permanent thread that the engine would never use (rumble is a non-goal). On the web `next_event` calls `navigator.getGamepads()` and there is no thread.
- **Web behavior to document and verify in the example.** `getGamepads()` is polled, so connect and disconnect events appear at the first poll that sees the change; the browser exposes a pad to a page only after the user presses a button on it, and `gilrs-core` warns that the Gamepad API may be unavailable outside a secure context (HTTPS or `localhost`). `gilrs-core`'s web backend calls `getGamepads()` whenever its event cache is empty, so a drain that reads to `None` costs **one call on an idle frame and two on a frame that had events** (the refill, then the terminating empty read). The poller adds no call of its own and does not poll twice in one drain, and GP2b records the measured cost; there is no way to drop the second call without a change inside `gilrs-core`, because it does not report how many events are cached.
- **Logging.** gilrs and `gilrs-core` report problems (a failed `getGamepads()`, an unreadable device) through the `log` crate. **They are not bridged to `tracing` by this spec.** The platform installs no logger and names neither `log` nor `tracing`, so those records go nowhere until a logger exists. The profiler spec's `init` may install `tracing_log::LogTracer` and then they appear with no change here. The poller's own failure to start gilrs is the one message it prints itself (`eprintln!` on native, `console.error` on the web), once.

**Types** (`input/gamepad.rs`):

```rust
pub const MAX_GAMEPADS: usize = 4;                   // local players; a fifth pad is ignored until a slot frees
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GamepadId(u8);                            // a slot, 0..MAX_GAMEPADS (not gilrs's id); `new(u8) -> Option<Self>`, `index() -> usize`

#[non_exhaustive] #[repr(u8)] pub enum GamepadButton {
    South, East, West, North,                        // position names as in the W3C standard mapping; South is A on an Xbox pad
    LeftBumper, RightBumper,
    LeftTrigger, RightTrigger,                       // digital, derived from the trigger axes; a device never sends these two
    Select, Start, Mode,                             // Back, Start, Guide
    LeftStick, RightStick,                           // the stick clicks
    DPadUp, DPadDown, DPadLeft, DPadRight }          // 17 variants
#[non_exhaustive] #[repr(u8)] pub enum GamepadAxis { LeftStickX, LeftStickY, RightStickX, RightStickY, LeftTrigger, RightTrigger }
```

`GamepadButton` and `GamepadAxis` are the platform's own enums, like `KeyCode`: no gilrs type is public. Sticks are y up, like the rest of the engine. gilrs already normalizes stick Y to up-positive on both backends (the WGI backend and the web backend both report it through gilrs's standard layout), so the poller passes the value through with **no sign flip and no constant**; GP2b asserts it on both targets (stick up reads positive) and a later gilrs version that changed it would fail that run. An `InputEvent::GamepadButton` for `LeftTrigger` or `RightTrigger` is accepted when injected, and sets that button only.

**`GamepadState`** (a plain-data resource, `Send + Sync`, no heap, at most 1 KiB). It is separate from `InputState` so that `InputState` keeps its 512-byte budget:

```rust
pub struct GamepadState { pads: [PadState; MAX_GAMEPADS], pub ignored: u32, dirty: bool }
struct PadState { connected: bool, buttons: ButtonInput<GamepadButton>, raw: [f32; 6], left: Vec2, right: Vec2, triggers: [f32; 2] }
impl GamepadState {
    pub fn apply(&mut self, e: &InputEvent, cfg: &GamepadConfig);   // gamepad variants only; anything else is ignored
    pub fn reprocess(&mut self, cfg: &GamepadConfig);               // recompute sticks and triggers after a config change
    pub fn is_connected(&self, pad: GamepadId) -> bool;  pub fn connected(&self) -> impl Iterator<Item = GamepadId> + '_;
    pub fn buttons(&self, pad: GamepadId) -> &ButtonInput<GamepadButton>;     // pressed, just_pressed, just_released, per schedule view; an unconnected slot reads as nothing held
    pub fn left_stick(&self, pad: GamepadId) -> Vec2;  pub fn right_stick(&self, pad: GamepadId) -> Vec2;   // deadzone applied, length <= 1, y up
    pub fn left_trigger(&self, pad: GamepadId) -> f32;  pub fn right_trigger(&self, pad: GamepadId) -> f32;  // deadzone applied, 0..=1
    pub fn set_view(&mut self, v: EdgeView);  pub fn end_frame(&mut self);  pub fn end_tick(&mut self);
    pub fn release_all(&mut self);                                   // every held button released with edges, axes zeroed, connection kept
    pub fn snapshot(&self, out: &mut GamepadSnapshot);
}
```

- **Connect and disconnect.** `GamepadConnected` marks the slot connected with a fresh state. `GamepadDisconnected` releases everything with edges (a held button is seen released once, so a pulled cable never leaves one stuck), zeroes the axes and marks the slot not connected. Any other event for a slot that is not connected, and any event whose `GamepadId` is out of range (reflection can build a `GamepadId(200)` inside an injected event), is dropped and counted in `ignored`.
- **Mapping to the layout.** The table is the poller's pure translation (`gamepad/translate.rs`). gilrs reports through the standard layout on both backends, so no per-device table ships.

| gilrs event | Becomes |
|---|---|
| `Connected`, `Disconnected` | `GamepadConnected`, `GamepadDisconnected` for the pad's slot |
| `ButtonPressed` or `ButtonReleased` of `South`, `East`, `West`, `North`, `Select`, `Start`, `Mode` and `DPadUp` to `DPadRight` | `GamepadButton` of the same name, with `pressed` |
| `ButtonPressed` or `ButtonReleased` of `LeftTrigger`, `RightTrigger` (gilrs's names for the bumpers), `LeftThumb`, `RightThumb` | `GamepadButton` `LeftBumper`, `RightBumper`, `LeftStick`, `RightStick`, with `pressed` |
| `ButtonChanged(LeftTrigger2 or RightTrigger2, v)` | `GamepadAxis` `LeftTrigger` or `RightTrigger` with `value = v` |
| `ButtonPressed` or `ButtonReleased` of `LeftTrigger2` or `RightTrigger2` | dropped (the digital trigger button is derived from the axis, so one trigger pull gives one edge) |
| `AxisChanged` of `LeftStickX`, `LeftStickY`, `RightStickX`, `RightStickY` | `GamepadAxis` of the same name, the value unchanged (Y is already up-positive) |
| `AxisChanged(DPadX or DPadY)`, `LeftZ`, `RightZ`, `C`, `Z`, every `Unknown`, `ButtonRepeated`, `ButtonChanged` of any other button, `Dropped`, `ForceFeedbackEffectCompleted` | dropped (only Linux evdev reports the D-pad as axes, and that backend is not built) |

- **Deadzones** (applied in `apply` and `reprocess`, so reads are plain loads). A stick is processed as a pair, because per-axis deadzones turn a diagonal into a square and let a drifting axis steer. With `m = |(x, y)|` of the raw pair: `m <= stick_deadzone` gives `(0, 0)`, otherwise the result keeps the direction and has length `min((m - stick_deadzone) / (stick_outer - stick_deadzone), 1)`. A trigger reads `clamp((raw - trigger_deadzone) / (1 - trigger_deadzone), 0, 1)`. The digital `LeftTrigger` and `RightTrigger` buttons use hysteresis on the processed value `v`: **press** when `v >= trigger_press`, **release** when the button is down and `v <= release_at`, with `release_at = max(trigger_press - 0.1, 0)`. So a trigger resting at the threshold does not chatter. Because `trigger_press` can be as low as 0.05, `release_at` can be 0; a trigger at rest processes to exactly 0 (it is inside the deadzone), so the button always releases and can never stay down. `release_at` is always below `trigger_press`, so there is no value at which a down button is both pressed and released.
- **Non-finite and out-of-range values.** A non-finite axis value is ignored, and a finite one is clamped to its range (`[-1, 1]` for sticks, `[0, 1]` for triggers), as `InputState::apply` does for scroll.
- **Focus.** `PlatformEvent::Focused(false)` calls `GamepadState::release_all` with `InputState::release_all`. While the window is unfocused, the driver still drains the poller (so the queue cannot grow) and drops button and axis events; connect and disconnect events always pass, so the slot table stays right.

**`GamepadConfig`** (reflected resource, read and write; the loop clamps it when read, like `TimeConfig`: a non-finite value is the default, and `stick_outer` is raised to at least `stick_deadzone + 0.05`):

```rust
#[derive(Resource, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Resource, Default, type_name = "exitium_platform::GamepadConfig")]
pub struct GamepadConfig {
    #[reflect(range(min = 0.0, max = 0.9))]  pub stick_deadzone: f32,    // 0.15
    #[reflect(range(min = 0.1, max = 1.0))]  pub stick_outer: f32,       // 0.95: the corners of a real stick rarely reach 1
    #[reflect(range(min = 0.0, max = 0.9))]  pub trigger_deadzone: f32,  // 0.05
    #[reflect(range(min = 0.05, max = 1.0))] pub trigger_press: f32,     // 0.5
}
```

The defaults are the author's starting values. The sample game tunes them with a real pad. **`GamepadSnapshot`** (reflected, read-only, built by `snapshot` into a reused buffer, like `InputSnapshot`) lists, for each connected pad, `pad`, the `buttons` held, both processed sticks and both processed triggers. It exists for the same reason: bitsets do not reflect usefully.

**The poller** (`gamepad/backend.rs`). `GamepadPoller::new(&WindowConfig) -> Option<GamepadPoller>` returns `None` when `WindowConfig::gamepad` is false (a new field, default true, read at creation like `size`) or when gilrs fails to start (the failure is printed once and the game runs without a pad). It builds gilrs in one function, `build_gilrs()`, with `GilrsBuilder::new().with_default_filters(false).set_update_state(false).add_env_mappings(false).with_force_feedback(false)`: raw events, because the deadzone is ours and reflected; no cached gilrs state, because `GamepadState` is the state; and **no force feedback**, because gilrs turns it on by default and it starts a second permanent thread (rumble is a non-goal). The included SDL mapping database stays on (the default). It keeps `[Option<gilrs::GamepadId>; MAX_GAMEPADS]`: `Connected` takes the lowest free slot (a fifth pad is ignored until a slot frees) and `Disconnected` **frees** the slot at once. **Nothing remembers a pad after it leaves**: a pad that returns is a new connection and takes the lowest free slot, which is its old one when the other pads did not move, and not otherwise. A game that wants "player 2 stays player 2" keeps its own map from the order in which it saw `GamepadConnected`.

- **When it runs.** In the driver, on every `RedrawRequested`, immediately before `handler.frame`: it drains `next_event` and calls `handler.event(window, PlatformEvent::Input(..))` for each result, at most 256 per frame (the rest wait in gilrs's queue for the next frame, so a chatty device cannot stall a frame). On the web a drain makes one `getGamepads()` call on an idle frame and two on a frame with events (see "Web behavior"); the poller adds no call of its own. The events reach `GamepadState` in `First` of the same frame, which is the roadmap's "event to state in the same frame". Gamepad events are not winit events, so they never pass through `raw_event`.
- **Translation is pure and tested without a device**, the way winit's is: `translate_button(gilrs::Button, pressed)`, `translate_trigger(gilrs::Button, f32)` and `translate_axis(gilrs::Axis, f32)` take decomposed values (gilrs's `Event` carries an opaque `Code` that a test cannot build). A table test covers every gilrs `Button` and `Axis` variant.
- **Injection.** `InjectedInput` carries an `InputEvent`, so the four gamepad variants are injectable with no new event: an agent can connect a virtual pad, press `South`, push a stick and read the result, in a headless app with no device. That is the agent-parity story for gamepad. **Injected and real pads share the slot table of `GamepadState` only, not the poller's.** An injected `GamepadConnected` for slot 0 is applied as sent, and a real pad that later connects takes the poller's lowest free slot, which can be slot 0: that real `GamepadConnected` resets slot 0 to a fresh state (the poller does not know the slot was taken by injection). **Real events win; injection is for headless and agent use**, where no device is present, and a test or agent that mixes the two on one slot gets the real device's state from then on.

**Facade glue.** None beyond the re-exports (F3) is needed: a game reads `GamepadState` and `GamepadConfig` directly, and `DefaultPlugins` already adds `PlatformPlugin` with gamepad on. If the owner wants a pad to drive UI focus, that is an amendment to `ui-mvp.md`, which lists it as a non-goal.

### Window and the event loop (`exitium_platform`, Stage 1)

```rust
pub struct WindowConfig { pub title: String, pub size: UVec2 /* logical px, 1280x720 */, pub resizable: bool, pub fullscreen: bool /* borderless */,
                          pub cursor_visible: bool, pub canvas_id: Option<String> /* web, "exitium-canvas" */, pub prevent_default: bool /* web, true */,
                          pub max_fps: Option<u32> /* native, None */, pub gamepad: bool /* true; read at creation: starts the gamepad poller */ }

pub struct WindowInfo { pub physical_size: UVec2, pub scale_factor: f32, pub focused: bool }
impl WindowInfo { pub fn logical_size(&self) -> Vec2; pub fn is_minimized(&self) -> bool /* either side is 0 */; }

/// Anything wgpu accepts as a surface target. Wraps `Arc<winit::window::Window>`.
#[derive(Clone)] pub struct PlatformWindow(/* Arc<winit::window::Window> */);
impl raw_window_handle::HasWindowHandle for PlatformWindow { .. }      // from `winit::raw_window_handle`, delegating
impl raw_window_handle::HasDisplayHandle for PlatformWindow { .. }
impl PlatformWindow { pub fn physical_size(&self) -> UVec2; pub fn scale_factor(&self) -> f32; pub fn request_redraw(&self);
                      pub fn set_title(&self, t: &str); pub fn set_cursor_visible(&self, v: bool); pub fn set_fullscreen(&self, f: bool);
                      pub fn as_winit(&self) -> &winit::window::Window;     // the escape hatch for the editor's egui-winit
                      pub fn wake_handle(&self) -> WakeHandle; }            // see "The wake handle"

/// A cheap handle that any thread can use to make the event loop run a frame soon. `Clone + Send + Sync + 'static`.
#[derive(Clone)] pub struct WakeHandle(/* Arc<WakeInner> */);
impl WakeHandle { pub fn wake(&self); }          // never blocks; many calls before the next frame coalesce into one

pub trait WindowHandler: 'static {
    fn ready(&mut self, window: &PlatformWindow);                           // once, when the window exists
    fn event(&mut self, window: &PlatformWindow, e: PlatformEvent);
    fn frame(&mut self, window: &PlatformWindow, f: FrameInfo) -> Flow;     // once per redraw
    fn raw_event(&mut self, _w: &PlatformWindow, _e: &winit::event::WindowEvent) -> bool { false }   // called first for EVERY WindowEvent; true = consumed, which only matters for keyboard, mouse and cursor events (editor)
}
pub struct FrameInfo { pub seconds: f64 /* since the previous frame; 0 on the first */ }
pub enum Flow { Continue, Exit }
pub fn run(config: WindowConfig, handler: impl WindowHandler) -> Result<(), PlatformError>;   // blocks on Windows; returns at once on the web
```

**What the platform exposes for the renderer glue (answers render D2, C1-safe).** `PlatformWindow` implements `HasWindowHandle` and `HasDisplayHandle` and is `Clone + Send + Sync + 'static`, so it satisfies wgpu's blanket `From<T> for SurfaceTarget<'static>`. Render's `Renderer::new(window: impl Into<wgpu::SurfaceTarget<'static>>, size, scale_factor)` takes `platform_window.clone()` as is. The platform plugin (Stage 2) inserts the resources `PrimaryWindow(PlatformWindow)` (not reflected) and `WindowInfo` (reflected) before `PreStartup`, and sends `PlatformEvent::Resized` on every size or scale change. **The facade glue** (`PreStartup`: copy `PrimaryWindow` into render's window resource; `First`, after `INPUT`: copy `WindowInfo` into it each frame) is the only place that names both crates. `exitium_platform` never names `wgpu` or `exitium_render`; `exitium_render` never names `winit` or `exitium_platform`. `PlatformWindow` must be `Send + Sync` on wasm32 too, because the ECS `Resource` trait requires it: PL3 adds a compile-time assertion, built for wasm32. If winit's web window is not `Send + Sync`, the fallback is the ECS's non-send extension path (#22 open question 5), and this is the only point that would need it.

**The loop (`run`) on both targets.**

- winit's `ApplicationHandler` is the only driver. The window is created in `resumed` (winit 0.30 forbids creating it earlier), then `ready` is called, then `Poll` control flow runs. Each `about_to_wait` calls `window.request_redraw()`, and **each `RedrawRequested` is one frame**: measure seconds with `web_time::Instant` (a drop-in for `std::time::Instant`, which panics on wasm32-unknown-unknown), drain the gamepad poller into `event` calls ("Gamepad"), then call `frame`. On native the loop is `ApplicationHandler<WakeUp>`, so `WakeHandle` can reach it (below). On the web `request_redraw` is `requestAnimationFrame`, so frames follow the display. On Windows, frames follow the renderer's present (vsync), or spin when there is no renderer or present is immediate; `max_fps: Some(n)` switches to `WaitUntil` for that case.
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
- **Translation is pure and tested without a window.** winit's `KeyEvent` has a private field and cannot be built in a test (nor can `ScaleFactorChanged`'s `InnerSizeWriter`), so `translate.rs` takes decomposed values, and the consumption rules take an engine-owned kind (below): `translate_key(PhysicalKey, ElementState) -> Option<InputEvent>`, `translate_mouse_button`, `translate_scroll(MouseScrollDelta)`, `translate_cursor(PhysicalPosition<f64>)`. All of those are public winit types that a test can construct. A table test covers every `KeyCode` variant (winit code to ours and back by name).
- **The editor's raw-event hook** (amended, #139; the platform half of `editor-mcp-mvp.md` AM2). egui-winit (#110) needs the winit `WindowEvent` and must see all of them (focus, IME, modifiers, scale factor, occlusion, resize), and the game must not see a click the editor took.
  - **Delivery.** `WindowHandler::raw_event` is called **first, for every `WindowEvent`** the driver receives, including the ones the platform translates itself (`Resized`, `ScaleFactorChanged`, `Focused`, `CloseRequested`) and the ones it ignores (`Ime`, `ModifiersChanged`, `Occluded`, `RedrawRequested`, `Touch`).
  - **Consumption.** The return value (`true` = consumed) changes behavior for **keyboard, mouse-button, mouse-wheel and cursor-move events only** (`KeyboardInput`, `MouseInput`, `MouseWheel`, `CursorMoved`): a consumed one is not translated, so `InputState` never sees it. For every other event the return value is ignored, so the editor can never swallow a resize, a scale change, a focus change or a close.
  - **State-clearing events are never consumed.** A key or button **release** and `CursorLeft` are translated even when `raw_event` returns true. Otherwise a drag that starts in the viewport and ends over an editor panel would leave the game's button held for good; the game ignores a release for a button it never saw pressed (`ButtonInput::release`), so delivering it is harmless. This narrows the rule above on purpose, and the acceptance of the code issue says so: a consumed press, wheel or cursor move never reaches `InputState`, and a release always does. **This spec wins over `editor-mcp-mvp.md` where they differ** (that spec still says "consumed events never reach the game" at its line 284 and in the acceptance of ED8 at line 796 and of its AM2 code issue at line 968), until SP8 (#149) patches the editor spec; "Cross-spec resolutions" names the three lines.
  - **A consumed cursor move is the cursor leaving the game.** If the hook consumes `CursorMoved` and the game has not been told, the last translated position would stay in `InputState`, `CursorWorld` would point at a stale spot under an editor panel, and the first unconsumed move after the cursor returned to the viewport would produce a `cursor_delta` spike. So the first consumed `CursorMoved` after an unconsumed one is translated as **`InputEvent::CursorLeft`** (the cursor is `None`, `CursorWorld` is `None`, no delta), and further consumed moves are dropped. `InputState::apply` of a `CursorMoved` that arrives while the cursor is `None` **sets the position and adds no delta**, so the return to the viewport never spikes. Held buttons are untouched: their releases still arrive.
  - **The routing is a pure function over engine-owned types** (so it is testable: winit's `KeyEvent` has a private field and `ScaleFactorChanged` carries an `InnerSizeWriter`, and a test cannot build either). The driver's `window_event` does three things in order: `raw_event(window, &event)`, then `classify(&event)`, then `translate_after_hook(kind, consumed, cursor_known)`. Both functions live in `window/route.rs` and name no winit type except as `classify`'s argument:

    ```rust
    pub(crate) enum WindowEventKind {            // one variant per WindowEvent the platform treats differently
        KeyPress, KeyRelease, MousePress, MouseRelease, Wheel, CursorMoved, CursorLeft,
        Resized, ScaleFactorChanged, Focused, CloseRequested, Occluded, Other,           // Other: Ime, ModifiersChanged, Touch, RedrawRequested, ..
    }
    pub(crate) enum Translate { Yes, No, AsCursorLeft }
    pub(crate) fn classify(e: &winit::event::WindowEvent) -> WindowEventKind;            // a plain match, one arm per variant, no logic
    pub(crate) fn translate_after_hook(kind: WindowEventKind, consumed: bool, cursor_known: bool) -> Translate;   // pure: the whole consumption table
    ```

    `translate_after_hook` is the table: `KeyPress`, `MousePress`, `Wheel` give `No` when consumed, else `Yes`; `CursorMoved` gives `AsCursorLeft` when consumed and `cursor_known`, `No` when consumed and not, else `Yes`; **every other kind gives `Yes` whatever `consumed` says** (releases, `CursorLeft`, `Resized`, `ScaleFactorChanged`, `Focused`, `CloseRequested`, `Occluded`, `Other`). The driver keeps one `cursor_known: bool`, set by a translated `CursorMoved` and cleared by a translated `CursorLeft`. A test enumerates the whole `kind x consumed x cursor_known` table against this text. `classify` is tested with every winit value a test can construct (`MouseInput`, `MouseWheel`, `CursorMoved` and `CursorLeft` through winit 0.30's safe `DeviceId::dummy()`, and `Resized`, `Focused`, `CloseRequested`, `Occluded`, `ModifiersChanged`, `RedrawRequested`); its `KeyboardInput` and `ScaleFactorChanged` arms cannot be constructed, so they are one-line arms checked by review, and the fake driver feeds `WindowEventKind` values directly for those two.
  - **Not offered:** gamepad events are not winit events and never reach the hook.
  - **Where the hook is installed.** `PlatformPlugin::with_raw_event_hook(self, hook: impl FnMut(&PlatformWindow, &winit::event::WindowEvent) -> bool + 'static) -> Self` (Stage 2, below). The hook is not `Send`: it runs on the main thread, inside winit's event callback, before the world is touched. It must not borrow the `World`; the editor shares state with its boundary hooks through the `Rc<RefCell<..>>` it captured and takes the egui input at step 1 of the next frame (`editor-mcp-mvp.md`, "The frame, in a windowed editor").
  - **Public winit types.** The crate still has exactly two, both deliberate and both for the editor: this hook (`&winit::event::WindowEvent`) and the escape hatch `PlatformWindow::as_winit` (`&winit::window::Window`, which egui-winit also needs). `egui-winit` 0.37 needs `winit ^0.30.13`, the same version.
- **The wake handle** (amended, #139; the other half of AM2). The editor's live-control server runs on its own thread and queues requests that the boundary hook drains at step 1 of a frame. In a visible window `Poll` runs a frame per vsync, so a request waits at most one frame. A minimized or occluded window may produce no `RedrawRequested` at all, and `max_fps: Some(n)` with a small `n` stretches the wait, so the server needs a way to ask for a frame.
  - **API.** `PlatformWindow::wake_handle() -> WakeHandle`. `WakeHandle::wake()` is callable from any thread, never blocks and coalesces: a flag (one `AtomicBool`) ensures at most one wake is pending, so a burst of requests is one frame. The editor takes the handle from `PrimaryWindow` in `Startup` and gives it to the server thread.
  - **Native.** The loop is built with `EventLoop::<WakeUp>::with_user_event()`; the handle holds the `EventLoopProxy<WakeUp>` and the flag. `wake()` swaps the flag to true and, only if it was false, sends `WakeUp` (a send to a loop that has gone is ignored). The driver's `user_event` clears the flag and then either calls `window.request_redraw()`, or, when the window is minimized (zero size) or occluded (`WindowEvent::Occluded(true)` seen), runs one frame directly, because no redraw will come. **On Windows `Occluded` is never sent** (winit does not support it there, and a covered window keeps getting redraws), so on Windows only the minimized test (zero `inner_size`) can fire; the occluded arm is the same code and is reached on the platforms and browsers that do report it. The code has no `cfg` for this, and the manual run of the AM2 issue minimizes the window. The renderer already skips presenting a minimized window (`Skipped(Minimized)`), so the frame runs the hooks and the simulation and draws nothing.
  - **Web.** `wake()` does nothing. Frames follow `requestAnimationFrame`, and there is no live-control server on the web (the editor is Windows-only). The type exists on both targets so shared code compiles unchanged.
  - **What a wake is not.** It carries no data and runs no hook itself. It only makes a frame happen, and the frame's step 1 does the work.

### Stage 1 and Stage 2

- **Stage 1 (`exitium_platform`, `exitium_core`, `winit` and `web-time`; `web-sys` and `wasm-bindgen` on wasm32; `gilrs` on Windows and wasm32).** Input types and state, the gamepad types, state and poller, window config and info, `PlatformWindow`, translation, `run` on both targets, the console panic hook and two examples (window, gamepad). No ECS, no app. The examples are the proof (below). The wake handle and the every-event `raw_event` delivery land in Stage 3's pre-editor lane (the AM2 code issue), because only the editor needs them; the contract is fixed here.
- **Stage 1 for `exitium_app` does not exist.** `exitium_app` depends on `exitium_ecs`, so it is the Stage 2 spine (roadmap).
- **Stage 2, `exitium_app`:** the spine in the order A1 to A4 below.
- **Stage 2, `exitium_platform::plugin` (feature `plugin`, default on, like physics):**
  - `PlatformPlugin { window: WindowConfig, exit_on_close: bool (true), install_panic_hook: bool (true) }`, plus a private slot for the raw-event hook. `PlatformPlugin::with_raw_event_hook(self, hook: impl FnMut(&PlatformWindow, &winit::event::WindowEvent) -> bool + 'static) -> Self` stores the hook; `build(&self, ..)` takes it out of its `Cell` and moves it into the runner, so a plugin builds once (it is unique anyway). The editor installs its egui hook here (`PlatformPlugin::default().with_raw_event_hook(..)`, handed to `DefaultPlugins::set`), and a game never calls it.
  - **In `build`:** insert `InputState`, `InputSnapshot`, `GamepadState`, `GamepadConfig` (default), `GamepadSnapshot`, `WindowConfig`, `WindowInfo` (zero size) and register two `Frame` events: `PlatformEvent` (sent only by the runner, **not reflected**, so MCP cannot send a resize or a close) and `InjectedInput(InputEvent)` (reflected, the injection event; its `InputEvent` now includes the gamepad variants). Register all types with the reflection registry (Stage 1 types gain `cfg_attr(feature = "reflect", derive(Reflect))`; the `reflect` feature is added by intake 2). Install the panic hook. Set the runner.
  - **Systems.** `First`, set `INPUT`: `apply_platform_events` reads `EventReader<PlatformEvent>` and `EventReader<InjectedInput>`, sends each `InputEvent` to `InputState::apply` or, for the four gamepad variants, to `GamepadState::apply` with the current `GamepadConfig`, updates `WindowInfo` on `Resized` and `Focused` (with `release_all` on both states on focus loss), and sends `AppExit` on `CloseRequested` when `exit_on_close`. Then `reprocess_gamepad` (`INPUT`, after it) compares `GamepadConfig` with its `Local` copy of last frame's value and calls `GamepadState::reprocess` when they differ (value comparison, like `propagate_transforms`, because the ECS has no change flag it can rely on here). `FixedFirst`: set the `Fixed` view on both states. `FixedLast`: `end_tick` on both, set the `Frame` view. `Last`: **while `SimulationMode` is `Frozen`, `end_tick` on both first** (no tick will ever clear those edges; see "`SimulationMode`"), then `end_frame` on both, refresh `InputSnapshot` and `GamepadSnapshot` (each only when its state is `dirty`), and apply a changed `WindowConfig` (title, cursor visibility and fullscreen apply live; `size`, `resizable`, `canvas_id` and `gamepad` apply only at creation).
  - **The runner** implements `WindowHandler`: `ready` inserts `PrimaryWindow` and `WindowInfo` (so they exist before `PreStartup`); `event` does `world.send_event(PlatformEvent)`; `frame` calls `app.run_frame(seconds)` and returns `Flow::Exit` when the report holds an `AppExit`; `raw_event` forwards to the hook installed with `PlatformPlugin::with_raw_event_hook` and returns `false` when there is none.
  - **Input injection and winit input take one path into the input states:** the runner sends `PlatformEvent::Input` (winit and gamepad input alike), an agent sends `InjectedInput`, both between frames and both consumed in `First` by `apply_platform_events`, which dispatches to `InputState::apply` or `GamepadState::apply`. Injection is limited to `InputEvent` on purpose: a reflected `Resized`, `Focused` or `CloseRequested` would let an MCP call feed a huge size to the renderer or close the game. `InputState::apply` itself ignores a non-finite `CursorMoved` or `Scroll` and clamps one `Scroll` to 1,000 lines, so a hostile injected value cannot poison the state.

### Threading

- Game code runs on the main thread and on the `exitium_tasks` pool. `Update` and `PostUpdate` use the parallel executor, so their systems may run on pool workers (see "Executors and the thread pool"); every other schedule runs its systems on the main thread (a fixed-step system's own `par_iter_mut` still uses the default pool, so repeatable runs need `ThreadBudget::single()` or order-independent bodies), and the web has no threads (the pool is one inline thread there, and `Parallel` resolves to `SingleThreaded`). The pool is created by the facade's `ThreadingPlugin`, not by this spec's crates: `exitium_app` and `exitium_platform` create no thread, take no lock and use no `unsafe`. A system must follow the tasks spec's job rule (never block on another system), and `PrimaryWindow` is `Send + Sync` for that reason. `exitium_app` uses no atomic. `exitium_platform` has exactly one, the `AtomicBool` that coalesces `WakeHandle::wake` calls (native only in effect; the web `wake` is a no-op). The `WakeHandle` also holds winit's `EventLoopProxy`, which is thread-safe by itself.
- The winit event loop owns the main thread on Windows (required by winit on every desktop OS) and the JS event loop on the web. `App` is not `Send`.
- Besides the pool, two other threads can exist in a game process, neither of them ours to schedule. #110's live-control server never touches the world: it queues requests, and its boundary hook drains them at step 1 of the frame (it calls `WakeHandle::wake` to get a frame). On Windows, `gilrs-core`'s WGI backend runs one reader thread that sends device events over a channel (force feedback is off, so it is the only gilrs thread); the platform only drains that channel on the main thread ("Gamepad").
- `World` is `Send`, so a future runner on another thread is possible, but nothing in the MVP does it.

### Facade glue (listed here, owned by the facade)

C1 forces these into `crates/exitium/` (a hotspot, one issue at a time). They are not edits to `exitium_app` or `exitium_platform`.

1. **Window to render:** `PreStartup` copies `PrimaryWindow` into render's window resource, and `First` (after `INPUT`) copies `WindowInfo` into it each frame (render D2). The systems take `Option<Res<PrimaryWindow>>` and `Option<Res<WindowInfo>>` and do nothing when they are `None`, so a headless app (no window, no renderer: the MCP end-to-end test) does not panic.
2. **Cursor to world:** `First`, after `INPUT`: use render's camera resolve to fill `CursorWorld` from `InputState::cursor_logical` (logical pixels, as render's `screen_to_world` takes) and the primary camera's view. Like the window hand-off it reads `Option<Res<PrimaryWindow>>` and `Option<Res<..>>` of render's resources, so a headless app (no window, no renderer) skips it instead of panicking, and `DefaultPlugins` adds it only with `PlatformPlugin`.
3. **UI input and viewport:** a `First` system, `after_set(INPUT)` and ordered before the UI plugin's `ui_input` system (which is in `UI_INPUT`), converts `InputState` (frame view) into the UI plugin's `UiInputFrame` **resource** (UI-logical pixels = `cursor_logical / ui_scale`) and syncs `UiViewport` from `WindowInfo`. `ui_input` only reads that resource and stores `UiCapture`; it names no platform type, because C1 forbids a UI to platform edge. `ui-mvp.md` is amended in this PR to say so (U11). With no platform plugin the resource keeps its default (no pointer, nothing pressed), so a headless or UI-only app works. The slot (`UI_INPUT` after `INPUT`, in `First`, before every tick) is this spec's.
4. **The default task pool:** `ThreadingPlugin` (F4), which builds the pool and installs it with `TaskPool::set_default` ("Executors and the thread pool"). It also reports `ResolvedBudget::env_warning`, because the tasks spec leaves that print to the app side and the facade is the side that resolves the budget.
5. **`DefaultPlugins`** adds `ThreadingPlugin` first and `PlatformPlugin` second among the engine's plugins, and re-exports the app, platform and threading API (`ThreadBudget` included) through the facade.

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
| `TimeConfig` | read, write | `fixed_hz`, `max_ticks_per_frame`, `max_frame_seconds`, `time_scale`, `paused`, `step_ticks`. The generic `SetResourceField` (#22) writes it ("slow down" is `time_scale`). In an editor build, pause, resume and step go through the `play_*` tools (below), which also own the play snapshot. |
| `SimulationMode` | read, write | `Running` or `Frozen`. Written by the editor's `play_*` tools. A hand write with `ecs_set_resource_field` works and skips the snapshot, so an agent in an editor build uses `play_*`. |
| `GamepadConfig` | read, write | Stick and trigger deadzones and the trigger press threshold. Takes effect at the next `First` (`reprocess_gamepad`). |
| `GamepadSnapshot` | read | The connected pads, their held buttons, and processed sticks and triggers. |
| `FrameStats` | read | Frames, ticks, stalls, dropped time. |
| `CursorWorld` | read | |
| `WindowInfo` | read | Physical and logical size, scale factor, focus. The agent's window info. |
| `WindowConfig` | read, write | Title, cursor visibility, fullscreen apply live. `size`, `resizable`, `canvas_id` and `gamepad` apply only at creation. |
| `InputSnapshot` | read | What is pressed now and where the cursor is. |

**Reflected events** (sent through the generic `SendEvent` command, #22; each is registered with `add_reflected_event`):

| Event | Use |
|---|---|
| `InjectedInput` | **Input injection.** An agent sends `InjectedInput(Key { key: Space, pressed: true })`, then the release, or `CursorMoved` and `MouseButton`, or a virtual pad (`GamepadConnected`, `GamepadButton`, `GamepadAxis`). It reaches `InputState::apply` or `GamepadState::apply` in `First`, `INPUT`, the same as real input, so a game cannot tell the difference, and the edge rules above apply. It carries an `InputEvent` only; window events (`Resized`, `Focused`, `CloseRequested`) are not injectable. This is the whole of the `input_inject` preset (see "Tool names"): the call is `ecs_send_event`. |
| `AppExit` | Graceful quit. |

**Tool names** (amended, #139; the `<domain>_<verb>` convention of the ECS spec). The generic tools already cover everything above (`ecs_get_resource`, `ecs_set_resource_field`, `ecs_send_event`), and **no preset is built**. The first draft of this spec proposed four presets (`app_pause`, `app_resume`, `app_step`, `input_inject`); `editor-mcp-mvp.md` (D9, AM2 and AM3) supersedes them:

| Draft preset | Replaced by | Why |
|---|---|---|
| `app_pause`, `app_resume`, `app_step` | the editor's `play_start`, `play_pause`, `play_resume`, `play_step`, `play_advance` and `play_stop` | They write `TimeConfig.paused` and `step_ticks` and `SimulationMode`, and they **also own the play snapshot** (scene text, opted-in resources, the live entity set) that `play_stop` restores. A bare field write skips the snapshot, so a preset that only wrote the fields would be a trap. |
| `input_inject` | `ecs_send_event` of `exitium_platform::InjectedInput` | The generic event tool already sends any reflected event, and `InjectedInput` is one. A preset would add a name and no behavior. |

Without the editor (a headless game) the generic tools are the surface: write `TimeConfig` with `ecs_set_resource_field`, send `InjectedInput` with `ecs_send_event`. A tool that "advances N frames and returns" in a **windowed** live game is `play_advance` (a `Wait` on `Time::tick_count`, `editor-mcp-mvp.md`). **`play_advance` works in the headless host too**: a headless app with the live-control plugin (the editor spec's end-to-end test, XT1) runs the same `Wait`, and each frame of the headless runner is one tick, so the reply comes after that many frames. A bare headless `App` with no live-control plugin calls `App::run_frames` directly.

**Editor surface** (amended, #139). The editor builds on these pieces and no others:
- It **reads `PrimaryWindow` and `WindowInfo`** (the egui screen size, the scale factor, focus) and gets a `WakeHandle` from `PrimaryWindow` in `Startup` for its server thread. It creates egui-winit's `State` from `PlatformWindow::as_winit`.
- It **installs its hook through `PlatformPlugin::with_raw_event_hook`**, which gives egui every `WindowEvent` first (above). #110 owns the egui side; the platform provides the hook and the wake handle.
- Its toolbar (play, pause, step, stop, speed) is the `play_*` tools and `TimeConfig.time_scale`; `play_*` set `SimulationMode` (`Frozen` in Edit, `Running` in Play) and `TimeConfig.paused`.
- The transform gizmo writes `Transform2d`; an input and window panel reads `InputSnapshot`, `GamepadSnapshot` and `WindowInfo`.

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
| `GamepadState::apply` (one event; a stick event recomputes one pair with a square root) | <= 30 ns, no allocation |
| `GamepadState` `end_frame` / `end_tick` (4 pads) | <= 60 ns each |
| `GamepadState::snapshot`, one pad with 4 buttons down | <= 300 ns, no allocation after warm-up |
| `GamepadState` size | <= 1 KiB, no heap |
| gilrs value to `InputEvent` translation, per event | <= 50 ns, no allocation |
| `GamepadPoller::poll`, nothing pending (not CI-gated, recorded by the gamepad example) | <= 5 us native; on the web <= 100 us per `getGamepads()` call, one call on an idle frame and two on a frame with events (a drain, see "Gamepad") |
| Gamepad event to `GamepadState`, latency | the same frame: polled before `frame`, applied in `First` |
| `Frozen` frame, all schedules empty (`First`, `PostUpdate`, `Extract`, `Last`: 4 schedule runs, time update) | <= 6 us |
| Frame driver overhead with `Update` and `PostUpdate` set to `Parallel` on a one-thread (inline) pool, all schedules empty | within 5% of the `SingleThreaded` row above (the executor choice costs nothing when no worker exists) |
| `exitium_app::frame` span and `mark_frame` with the `profiling` feature off | no measurable change to the frame-driver rows (the span expands to nothing) |
| `WakeHandle::wake` while a wake is already pending (one atomic swap) | <= 50 ns |
| app and platform share of the Stage 2 sample frame (2,000 sprites) | <= 0.5% of 16.6 ms (83 us) |
| Window created to first `frame` call (not CI-gated, recorded by the examples) | <= 150 ms native, <= 100 ms on the web after the wasm has loaded |

**Complexity.** The loop is O(ticks) plus the systems. Propagation is O(roots + hierarchy nodes) per frame. Input is O(1) per event and per query. The driver keeps no per-frame `Vec`; `FrameReport` is `Copy`, and the transform stack is a reused `Local`. The no-allocation claims are reviewed until the counting allocator of the profiler spec (#64, a designated `unsafe` module with Miri tests; restored by `mvp-roadmap.md`) lands. Then the `apply`, `snapshot` and translation rows gain a counting assertion in the same benchmark files.

## Test plan

- **Time and loop (headless, `exitium_app`).** 60 Hz with frames of 6.9 ms (144 Hz) run 0 or 1 ticks (never 2: a frame is shorter than the 16.7 ms step) and, with frames of 33 ms (30 Hz), 1 or 2 ticks, and `fixed_alpha` stays in `[0, 1)` in both; a 2 s frame is clamped by `max_frame_seconds` and by the tick cap, and `dropped_seconds` and `stalled_frames` report it; NaN and negative `frame_seconds` act as 0; `time_scale` 0.5 and 2.0; `paused` runs no ticks and zero `delta` while `Update` still runs; `step_ticks` runs exactly that many and resets; **`SimulationMode::Frozen`** (a recording test over all four combinations of the mode table): `FixedFirst`, `FixedUpdate`, `FixedLast` and `Update` do not run, `First`, `PostUpdate`, `Extract` and `Last` run once each; `elapsed`, `tick_count`, `step_ticks`, `fixed_alpha` and the clock remainder are unchanged and `frozen_frames` counts; a `Transform2d` written by a boundary hook is propagated and seen in `Extract` the same frame; `Startup` runs once on a first frame that is already Frozen; a hook that sets the mode takes effect in the same frame; unfreezing after a long real-time gap runs no catch-up burst; `Frozen` plus `step_ticks` runs no tick and keeps the step; the type reflects, round-trips and has its pinned name; `run_frames` honors the mode; a hostile `TimeConfig` (zero, NaN, huge) is clamped, counted, and never panics; changing `fixed_hz` mid-run rebuilds the clock; `tick_count` inside a tick includes it.
- **Schedule order.** A recording test registers a system in every schedule and asserts the exact order for 0, 1 and 3 ticks, `Startup` once, `PreStartup` before `Startup`, and `UI_INPUT` after `INPUT` and before the first `FixedFirst`; `EXTRACT` before `RENDER`; a `PostUpdate` system `before_set(PROPAGATE)` is seen by the propagation of the same frame; `Commands` from `FixedUpdate` are applied before `FixedLast`.
- **Threads and repeatability (A1 and F4).** A recording test over the executor choice: `Update` and `PostUpdate` report `Parallel`, the other eight `SingleThreaded`. On a pinned four-thread pool two disjoint `Update` systems run on two threads, and every `FixedUpdate` system runs only on the calling thread, in plan order. **Determinism test (F4):** a `FixedUpdate` system with `par_iter_mut` and an order-dependent floating-point body is bit-identical over 20 runs with `ThreadBudget::single()`; the same system with an order-independent body (disjoint writes, fixed batching, partials combined in chunk order) matches the single-thread result on a four-thread default pool. The order-dependent body on four threads is not asserted to differ. `Schedule::set_task_pool` on `FixedUpdate` is shown not to pin the nested call (the documented limit of "Executors and the thread pool").
- **Events.** An event sent in `FixedUpdate` with the `Fixed` clock is read once by a `FixedUpdate` reader; a `Frame`-clock event sent in `Update` is read in the next `First` and in a `FixedUpdate` of the next frame when a tick runs; `AppExit` ends `run_frame` with the frame completed, including `Last`.
- **Plugins.** Registration order; a duplicate unique plugin panics with its name; `is_unique == false` allows two; `PluginGroupBuilder::set`, `disable`; a plugin inserting resources in `build` is visible to a game `Startup` system.
- **`App` API.** `build()` reports a cycle and an access conflict before any frame; `add_systems` with a function, a tuple and `chain`; `run_frames` returns the last report; the headless runner exits on `AppExit`; boundary hooks run first, in order, before `Startup` on the first frame and outside any schedule (a hook may call `world.run_schedule` on a label of its own without `AlreadyRunning`).
- **Transforms.** Identity and composition (scale, rotation, translation order); shear from non-uniform parent scale; a three-level tree; reparent through the #49 helper and detach through `remove_child` both update, and a stale `Children` entry (a child listed by a former parent) is ignored and does not make `Changed<GlobalTransform2d>` fire every frame; direct write to a child; a root with no `GlobalTransform2d` gets one in the same frame; a grouping entity with `Children` and no `Transform2d` gets an identity transform, and its transformed children propagate through it; a despawned child in `Children` is skipped; a dangling `Parent` keeps its last global; `Changed<GlobalTransform2d>` is false after a no-op `Transform2d` write and true after a real change; reflect round trip of both types, the pinned type names, and a `DynamicStruct` with `scale` missing rebuilt through `FromReflect` gets `Vec2::ONE` (scene loading itself is #111's).
- **Input (Stage 1, no window).** Press, release, repeat press ignored; edge rules in both views (a press on a frame with no tick is visible to the next tick once; a press and release inside one interval gives both edges and `pressed` false; two ticks in a frame give the edge to the first only); `end_frame` and `end_tick` clear only their own set; `release_all` emits edges; cursor physical to logical with scale 1.0, 1.5 and 2.0; scroll accumulation and the pixel-to-line constant; `unmapped` counts an unknown key; a property test applies random event sequences and checks that `pressed` equals the last event per button.
- **Translation.** Every `KeyCode` against winit's by name; mouse buttons; both `MouseScrollDelta` forms; fractional physical positions.
- **Gamepad (Stage 1, no device).** Connect, disconnect and reconnect: a held button gives one release edge on disconnect; an event for an unconnected slot or an out-of-range `GamepadId` is dropped and counted in `ignored`; edge rules in both views for pad buttons (the keyboard table, repeated for a pad); the stick deadzone table (inside the deadzone is `(0, 0)`, at `stick_outer` is length 1, past it clamps, the direction is kept on a diagonal, the result never exceeds length 1); trigger deadzone and scale, and the digital trigger button with its hysteresis (a value oscillating around the threshold gives one press and one release; a `trigger_press` of 0.05, 0.08 and 0.1 each releases when the trigger returns to 0, and a button never stays down at rest); a non-finite axis is ignored and an out-of-range one is clamped; `reprocess` after a config change; a property test (random event sequences: `pressed` equals the last event per button, stick length at most 1); a table test of every gilrs `Button` and `Axis` variant through the translation functions; the poller's slot table with fake gilrs ids (lowest free slot, a disconnect frees the slot at once, a returning pad takes the lowest free slot, a fifth pad is ignored until a slot frees); a consumed-cursor sequence into `InputState` (`CursorLeft` makes the position `None`, the next `CursorMoved` sets it with no `cursor_delta`).
- **Raw event and wake (fake driver, no window).** The consumption rules are tested on engine-owned types, because winit's `KeyEvent` and `ScaleFactorChanged` cannot be built: **(a)** `translate_after_hook` over the whole `kind x consumed x cursor_known` table (a consumed `KeyPress`, `MousePress` and `Wheel` give `No`; a consumed `CursorMoved` gives `AsCursorLeft` once and `No` after; every release, `CursorLeft`, `Resized`, `ScaleFactorChanged`, `Focused`, `CloseRequested`, `Occluded` and `Other` give `Yes` whatever `consumed` says); **(b)** `classify` against every winit value a test can construct, with the `KeyboardInput` and `ScaleFactorChanged` arms checked by review; **(c)** a fake driver that is fed `WindowEventKind` values (so it covers the two arms of (b) that no test can construct) and records that `raw_event` is called once per event, first, for every kind (focus, IME, modifiers, scale, occlusion, resize, close, redraw), and that the resulting `PlatformEvent`s match (a). The hook compiles with the exact signature of `with_raw_event_hook`; `WakeHandle: Clone + Send + Sync + 'static` (compile assertion, also for wasm32); a burst of `wake()` calls sends one event (the flag logic is tested against a fake sender); the driver's `user_event` runs a frame directly when the window is minimized or occluded and requests a redraw otherwise (the fake window reports both); the web `wake` is a no-op.
- **Window and loop (manual, recorded in each PR).** The example on Windows (125% and 100% scale, drag a window across monitors if available, resize, minimize, restore, alt-tab with a key held) and in Chrome (canvas by id, created canvas, browser zoom, window resize, hidden tab and return, the panic hook on a forced panic).
- **Gamepad (manual, recorded in the GP2b PR).** The gamepad example on Windows with an Xbox-layout pad (connect, hot-unplug and replug (the pad takes the lowest free slot, its old one when no other pad moved), both sticks at the centre, the edge and a diagonal, both triggers, every button, alt-tab with a button held) and in Chrome at `localhost` (the pad appears after a button press; stick up reads positive on both targets, which GP2b asserts; Process Explorer on Windows shows no force-feedback thread, only gilrs's one reader thread). The PR body names any non-Xbox pad tried and what it did. A `compile`-only assertion that `PlatformWindow` is `Send + Sync`, built for wasm32.
- **Plugin (Stage 2, headless where possible).** Injected gamepad events (a virtual pad connected, a button pressed, a stick pushed) drive `GamepadState` through `First` in a `FixedUpdate` system and an `Update` system, with the edge rules of both views; `Focused(false)` releases pad buttons too; a changed `GamepadConfig` is reprocessed the same frame; `GamepadSnapshot` refreshes only when dirty; `with_raw_event_hook` reaches the runner's `raw_event` and the runner returns false when no hook is set. Injected `InjectedInput` events drive `InputState` through `First` (a non-finite cursor and an oversized scroll are ignored or clamped, and a `Resized` cannot be sent through reflection); `Focused(false)` releases keys; `CloseRequested` sends `AppExit` when `exit_on_close`; a changed `WindowConfig` is applied only for the live fields; the runner's `WindowHandler` is tested with a fake window (the handler logic is separate from the winit loop).
- **wasm32 and Linux.** `cargo build --target wasm32-unknown-unknown` for both crates (and `--examples` for the platform, proposed in the wiring). Neither crate is in `NATIVE_ONLY`. The platform crate also builds and tests on the Linux CI image with no `libudev`, because `gilrs` is target-gated and the gamepad backend is a stub there.
- **Benchmarks.** Every row of the Performance table, each against a hand-written baseline (a bare loop over the same data).

## Open questions

1. **`Time` duplicates the core `Time`.** Core is frozen, cannot derive `Resource` or `Reflect`, and has no alpha. The app's `Time` has the same four fields plus three. When core may change (a later chore), core's `Time` can gain `fixed_alpha` and the app's resource can wrap it. Until then there are two types of the same name in different crates; the facade exports only the app's.
2. **Full propagation versus changed-only.** This spec visits every root and hierarchy node each frame (value compare) so that detaching a child is always correct without a hook. If a game has tens of thousands of parented entities, the optimization is the changed-only pass with a `Changed<Parent>` check and a removal-aware helper in #49. Confirm the budgets are acceptable for the MVP.
3. **Skipping derived components on save.** This spec marks `GlobalTransform2d` with `custom(derived = true)` and expects the scene spec (#111) to skip such components. If #111 chooses another rule (a list, or not registering `ReflectComponent`), only the attribute on `GlobalTransform2d` changes. `custom` on a type must be allowed by the reflect derive (reflect-model lists `custom` among the attributes, without saying where); if not, #111 owns the marker.
4. **Two winit types are public.** `WindowHandler::raw_event` (and `PlatformPlugin::with_raw_event_hook`, which takes the same `&WindowEvent`) exposes `winit::event::WindowEvent` and `PlatformWindow::as_winit` exposes `winit::window::Window`, both for the editor's egui-winit. Settled by #139: egui-winit needs every winit event and the window (IME and modifier events are not in `PlatformEvent`), so both stay. They are the only two, and the editor spec's X2 relies on them.
5. **Windows modal resize loop.** Frames stall while the title bar is held. A fix needs a timer-driven redraw during the modal loop (winit has no portable hook for it). Accepted for the MVP: the game stays correct, because the next frame's `Time::delta` is clamped.
6. **Interpolation of non-physics movers** (render open question 3). Not in the MVP. The additive path is an opt-in `PreviousTransform2d` (saved in `FixedFirst`) and a `PostUpdate` blend into `GlobalTransform2d` by `Time::fixed_alpha`. It touches no signature in this spec.
7. **`Plugins` for tuples and groups** is a small marker-trait trick (as `IntoSystem`). If a basic-level reviewer finds it too clever, the alternative is `add_plugin` and `add_plugin_group` as two methods. Same behavior.
8. **A real-time headless runner** (the dedicated server) needs a native sleep. It belongs to the server runtime of `net-server.md` (SP16), which is in the roadmap. The `run_frame(seconds)` entry point already accepts any measured time, so that runner is a loop around it and needs no change here.
9. **Defaults for `max_ticks_per_frame` (8) and `max_frame_seconds` (0.25).** They are the author's. Lower values drop simulation time sooner on a slow machine; higher values risk longer stalls. The owner may tune them with the sample's p99 frame budget.
10. **ECS items to confirm.** `ScheduleLabel::new` and `SystemSet::new` must be `const fn` (the sets here are `const`s). `register_reflected_event::<E>` takes the clock (#22 item 13 lists it without one). `Children` exposes its slice (`as_slice` or `Deref`) and `Parent` its entity (`get`) (#49).
11. **Gamepad: non-Xbox pads and the web read cost.** gilrs normalizes stick Y to up-positive on both backends, so the engine passes it through, and GP2b asserts it on both targets. Pads that do not use the standard layout (some generic HID pads, a web pad whose browser mapping is empty) may report buttons the translator drops; the example run records which pads were tried. A per-device remap table is additive and not specified here. The web backend's second `getGamepads()` call on a frame with events cannot be removed without a change inside `gilrs-core` ("Gamepad", web behavior); GP2b records its cost in Chrome, and the owner may then choose to poll every other frame.
12. **Gamepad on Linux and macOS.** `gilrs` needs `libudev` on Linux and IOKit on macOS, and neither is a tested target, so the backend is a stub there. The Linux and macOS ports enable it by widening the target `cfg` in the manifest and installing `libudev-dev` on the CI image; no API changes.
13. **`Update` systems that should run while authoring.** Frozen skips all of `Update`, including a game camera that follows a target, so in Edit mode that camera does not move on its own. The editor's scene view is its own camera (`SceneView`), so this is intended. If a game has a frame-rate system that must run in Edit mode, it moves to `PostUpdate`; a finer split (a second schedule that ignores Frozen) is additive. **Engine glue is not in this category:** it must never be in `Update` at all ("`SimulationMode`", the rule and the table of systems that move).
14. **Who moves the glue systems.** The rule that engine glue runs in `First` or `PostUpdate` is stated here, and the systems it moves are owned by the SP4 (#140, assets, scene and audio; PR #166 already does it) and SP6 (#142, UI) amendments, which also choose the exact schedule and set. If either amendment keeps a system in `Update`, the editor's Edit mode shows stale textures or a scene that never loads; the table in "`SimulationMode`" and the cross-spec rows are the checklist for their reviewers.
15. **Parallel `PostUpdate`.** `PostUpdate` runs `Parallel`, so two independent `PostUpdate` systems (physics interpolation and the facade's glue, for instance) may run in either order. Where order matters the systems already declare it (`before_set(PROPAGATE)`), and the transform propagation is itself an ordered chain. If the sample game finds a `PostUpdate` ordering bug, the remedy is an explicit edge, or `SingleThreaded` for that one schedule (a one-line change in `App::new`); the owner confirms the choice after the first parallel sample run.

## Implementation issues

After this spec merges the architect files these into the Jam MVP milestone with the `file-issue` skill. The labels `PL1` to `PL4a`, `PL4b`, `PL5`, `GP1a`, `GP1b`, `GP2a`, `GP2b` (Stage 1 gamepad), `A1` to `A4`, `PP1`, `PP2`, `GP3` (Stage 2 gamepad) and `F1` to `F4` are placeholders, replaced by issue numbers when filed. `AM2` and `AM3` are the two code halves of the editor spec's amendments, filed in Stage 3's pre-editor lane (see the end of this section). `intake-1` and `intake-2` are the intake PRs of the roadmap. Each issue edits one crate (or, for `F*`, only the facade hotspot), has about 600 changed lines or fewer (the estimate is next to it), a full labelled footprint and acceptance criteria. No issue is filed in `exitium_mcp` or `exitium_editor` (ruling b).

**Dependency rules.** Every Stage 1 issue is `Blocked by intake-1`. Every Stage 2 issue is `Blocked by intake-2` (rule d, C6). `exitium_platform` issues share a crate, so the factory runs one at a time and the list is in dependency order; the same holds for `exitium_app`. The two chains are independent of each other until the plugin issues. The `exitium_ecs` issues named below are the numbered items of `ecs-scheduler.md`.

**Common acceptance criteria:** `cargo xtask ci all` passes; new behavior has tests; each hot-path change ships its benchmark with the numbers in the PR body; every module has `//!` docs; no `unsafe`; no winit type is public except `WindowHandler::raw_event` (with `with_raw_event_hook`) and `PlatformWindow::as_winit`, and no gilrs type is public; the crate builds for `wasm32-unknown-unknown` and on the Linux CI image (gamepad stub, no `libudev`).

**Stage 1: `exitium_platform` (depends on `exitium_core`, `winit` and `web-time`).**

**PL1. `feat(platform): KeyCode, MouseButton and ButtonInput`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `input/key.rs`, `input/button.rs`, and the crate `CLAUDE.md` (module map, the two-public-winit-types rule, the one-path-for-input rule). The benchmark and its `[[bench]]` stanza are in PL2, to keep this issue under the size limit.
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

**PL4a. `feat(platform): event routing and the raw_event consumption table`** (about 300 lines; split from PL4 in the round 2 fix, because the router, its table test and the fake driver pushed the loop issue past 600 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `window/route.rs`: `WindowEventKind`, `classify`, `translate_after_hook` and the whole consumption table (a consumed keyboard press, mouse press or wheel is not translated; a consumed cursor move becomes one `CursorLeft`; a release and `CursorLeft` always are; the other events are unaffected by the return value), with the table test and the minimal fake driver that feeds `WindowEventKind` values (PL4b's loop tests reuse it).
- Acceptance:
  - [ ] `translate_after_hook` is tested over the whole `kind x consumed x cursor_known` table, and `classify` against every winit value a test can construct (its `KeyboardInput` and `ScaleFactorChanged` arms cannot be built and are reviewed; the fake driver covers them with `WindowEventKind` values).
  - [ ] The module docs state the consumption rules and the `cursor_known` bookkeeping.
- Blocked by: intake-1, PL3.

**PL4b. `feat(platform): event loop, frame timing and web canvas`** (about 500 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `run.rs`, `run/native.rs`, `run/web.rs`, `web.rs`: `WindowHandler`, `FrameInfo`, `Flow`, `run`, `PlatformError`; `resumed` creates the window; redraw-driven frames with `web_time::Instant`; `max_fps` through `WaitUntil` on native; canvas lookup, creation and `prevent_default` on the web; `Resized` emission from the window's own size; the driver calls `classify` and `translate_after_hook` from PL4a; `install_console_panic_hook`. PL4b offers `raw_event` the keyboard, mouse and cursor events and the translated window events; delivering **every** `WindowEvent` and the wake handle are the AM2 code issue's.
- Acceptance:
  - [ ] The handler's logic (event order, `Resized` after scale change, exit on `Flow::Exit`) is tested with PL4a's fake driver, which needs no window; the winit glue is a thin layer around it.
  - [ ] The crate builds for Windows and `wasm32-unknown-unknown`; the web driver uses `spawn_app` and returns at once.
  - [ ] The Windows modal-resize behavior is documented in the module docs (open question 5).
- Blocked by: intake-1, PL4a.

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
- Blocked by: intake-1, PL4b.

**GP1a. `feat(platform): gamepad types, InputEvent variants and GamepadConfig`** (about 300 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `input/gamepad.rs` (types only): `MAX_GAMEPADS`, `GamepadId` (with `new` and `index`), `GamepadButton`, `GamepadAxis`, `GamepadConfig` (data, `Default` and the clamping function), the `GamepadSnapshot` data type; the four `InputEvent::Gamepad*` variants in `input/event.rs`; `InputState::apply` ignoring them. No state and no gilrs yet. The crate `CLAUDE.md` gains the gamepad rules (separate state, the poller sits in the driver, deadzones live in `GamepadConfig`).
- Acceptance:
  - [ ] `GamepadId::new` rejects 4 and above; the two enums have the listed variants (17 and 6) with a dense index; `GamepadConfig::clamped` follows the rules of "Gamepad" (non-finite is the default, `stick_outer >= stick_deadzone + 0.05`).
  - [ ] `InputState::apply` ignores the four gamepad variants (tested) and a `CursorMoved` that arrives while the cursor is `None` sets it with no delta (tested).
- Blocked by: intake-1, PL2.

**GP1b. `feat(platform): GamepadState and gamepad benchmarks`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `GamepadState` in `input/gamepad.rs`: `apply`, `reprocess`, the readers, the per-view edge handling (`ButtonInput<GamepadButton>`), the deadzones, the trigger hysteresis, `release_all`, `snapshot`; the gamepad rows added to `benches/input.rs`.
- Acceptance:
  - [ ] Every gamepad test of the Test plan that needs no backend passes: connect and disconnect edges, ignored events counted, edge rules in both views, the stick deadzone table, trigger scaling and hysteresis (including `trigger_press` of 0.05 to 0.1), non-finite and out-of-range values, `reprocess`, and the property test.
  - [ ] `GamepadState::apply` ignores every non-gamepad `InputEvent` (tested).
  - [ ] The benchmarks cover `apply` (button, stick and trigger events), `end_frame`, `end_tick` and `snapshot` within the budgets; `GamepadState` is at most 1 KiB (asserted by a size test) and `apply` and `snapshot` allocate nothing after warm-up (reviewed).
- Blocked by: intake-1, GP1a.

**GP2a. `feat(platform): gilrs backend, translation and hot-plug slots`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `gamepad/translate.rs` (pure functions) and `gamepad/backend.rs` (cfg `any(windows, target_family = "wasm")`, a stub otherwise): `build_gilrs()` with the builder chain of "The poller", `GamepadPoller::new(&WindowConfig)`, `poll`, the slot table, the 256-event cap, and `WindowConfig::gamepad`. The `gilrs` dependency itself is added by intake 1 ("Intake wiring"). No driver call and no example yet.
- Acceptance:
  - [ ] The translation table test covers every gilrs `Button` and `Axis` variant (stick Y passes through unchanged); the slot-table tests of the Test plan pass with fake ids.
  - [ ] `build_gilrs()` is the only place that builds gilrs, and its chain is exactly `with_default_filters(false)`, `set_update_state(false)`, `add_env_mappings(false)` and `with_force_feedback(false)`; a comment names why force feedback is off (a second permanent thread). A test cannot see inside gilrs, so the review checks the chain against the spec.
  - [ ] The crate builds for Windows, for `wasm32-unknown-unknown` and on the Linux CI image with no `libudev` (the stub); no gilrs type is public.
  - [ ] `GamepadPoller::new` with `WindowConfig::gamepad = false` returns `None` and costs nothing per frame.
- Blocked by: intake-1, GP1b.

**GP2b. `feat(platform): gamepad driver calls and example for Windows and Chrome`** (about 400 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- The driver calls in `run/native.rs` and `run/web.rs` (drain at most 256 events before `frame`, drop button and axis events while unfocused), and `examples/gamepad.rs` (prints connect and disconnect, buttons, processed sticks and triggers; `println!` on Windows, `console.log` on the web; reuses PL5's `examples/web/index.html`; exits on Escape or the `Start` button; on the web it also prints the time of each drain, to record the `getGamepads()` cost).
- Acceptance:
  - [ ] The PR body records the manual run of the Test plan on Windows and in Chrome, **asserts that stick up reads positive on both targets** (and says what happened if it did not), records the web drain cost (one call idle, two with events), shows that Windows has no force-feedback thread (gilrs's one reader thread only), and names any non-Xbox pad tried.
  - [ ] The example builds for Windows and for `wasm32-unknown-unknown`.
  - [ ] Starting with no pad connected, and with `WindowConfig::gamepad = false`, is silent and costs nothing per frame.
- Blocked by: intake-1, GP2a, PL4b, PL5.

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
  - [ ] `App::new()` creates all ten schedules with the set ordering and registers its types, with `ExecutorKind::Parallel` on `Update` and `PostUpdate` and `SingleThreaded` on the other eight (asserted through `Schedule`'s executor getter). A test with a pinned four-thread pool (`Schedule::set_task_pool`) runs two disjoint `Update` systems on two threads and a `FixedUpdate` system only on the calling thread; with `ThreadBudget::single()` the results are identical, and with no pool installed at all the app runs. (This tests system placement. Repeatability of a system's own `par_iter_mut` is F4's determinism test.)
  - [ ] The labels convert into `ScheduleLabel`; a label name is unique and `crate::name`.
  - [ ] The `INPUT`, `UI_INPUT`, `PROPAGATE`, `EXTRACT` and `RENDER` ordering is tested.
- Blocked by: intake-2, the ECS executors (item 12 and the parallel executor of #48, which `ecs-scheduler.md` (SP2) lists as item 20), the ECS derives re-export (item 16), `ReflectResource` and `ReflectEvent` (item 13).

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
  - [ ] With the `profiling` feature, `run_frame` opens the `exitium_app::frame` span (step 0) before the hooks and closes it after step 8 and calls `exitium_profiler::mark_frame()` once per call, including a `Frozen` frame (a test with a recording `tracing` subscriber; the profiler's `testing::capture` helper (#168 item 1) is an alternative and not a blocker); with the feature off, neither exists (checked by `cargo build` without the feature and by a `cfg` test). The frame-driver benchmark with the feature off is A4's.
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
  - [ ] The frame-driver rows are measured with the `profiling` feature off, so the A2 span and `mark_frame` calls are compiled out, and stay within the 5% gate of their hand-written baseline (the `exitium_app::frame` row of the Performance table).
- Blocked by: intake-2, A3.

**Stage 2: `exitium_platform::plugin` (depends on `exitium_ecs`, `exitium_app`, `exitium_reflect`).**

**PP1. `feat(platform): PlatformPlugin, platform events and input systems`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/mod.rs`, `plugin/systems.rs`; `cfg_attr(feature = "reflect", derive(Reflect))` with pinned type names on `KeyCode`, `MouseButton`, **`GamepadId`, `GamepadButton`, `GamepadAxis`** (the field types of the gamepad `InputEvent` variants, so `InputEvent` can derive `Reflect` here and not one issue later), `InputEvent` (all variants, gamepad included), `InjectedInput`, `WindowConfig`, `WindowInfo`, `InputSnapshot` (`PlatformEvent` is not reflected); until GP3 the gamepad variants reach `InputState::apply`, which ignores them; `PlatformPlugin`, the resources, the reflected event registration, `apply_platform_events`, the view switch, `end_tick`, `end_frame`, `InputSnapshot` refresh.
- Acceptance:
  - [ ] Injected `InjectedInput` events drive `InputState` through `First` in a headless app; a non-finite cursor and an oversized scroll are ignored or clamped; `PlatformEvent` has no reflection registration; the edge rules hold in a `FixedUpdate` system on a frame with no tick and on a frame with two ticks.
  - [ ] `Focused(false)` releases keys; `CloseRequested` sends `AppExit` only when `exit_on_close`.
  - [ ] Every type in the surface list reflects and round-trips, **including `GamepadId`, `GamepadButton`, `GamepadAxis` and an `InputEvent::GamepadButton`**; an out-of-range `GamepadId` built through reflection round-trips and is not rejected by the type (GP3's state drops it); the injected event schema is readable by a generic tool.
  - [ ] The `Last` system calls `end_tick` on `InputState` while `SimulationMode` is `Frozen`, before `end_frame` (a key pressed on a Frozen frame is not `just_pressed` in the first `FixedUpdate` after the mode changes to `Running`); this needs `SimulationMode`, so PP1 is blocked by AM3.
  - [ ] `INPUT` is a set the UI glue can order after (a test orders a system `after_set(INPUT)`).
  - [ ] With `default-features = false` the crate still builds with no ECS dependency (the Stage 1 build).
- Blocked by: intake-2, PL5, GP1a, A2, AM3.

**PP2. `feat(platform): windowed runner, window config and panic hook`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/runner.rs`, the live `WindowConfig` application in `Last`, the panic hook with `current_system()` on native and web, `PlatformPlugin::with_raw_event_hook`, and `examples/plugin_window.rs` (`App::new().add_plugins(PlatformPlugin::default())` with systems that print input in `Update` and `FixedUpdate`, so both edge views are visible).
- Acceptance:
  - [ ] The runner's `WindowHandler` is tested with a fake window: `ready` inserts `PrimaryWindow` and `WindowInfo` before `PreStartup`; `frame` returns `Exit` on `AppExit`; `with_raw_event_hook` has the exact signature `FnMut(&PlatformWindow, &winit::event::WindowEvent) -> bool + 'static`, the runner's `raw_event` forwards to it, and it returns false when none is installed.
  - [ ] A forced panic in a system prints the schedule and system names on native and in the Chrome console.
  - [ ] The PR body records a run of the example on Windows and in Chrome, including pressing a key on a frame with no tick (hold the display at a rate above the fixed rate) and seeing it once in `FixedUpdate`.
  - [ ] `PrimaryWindow` implements `Resource` and is the only window resource; its handle satisfies wgpu's surface target bound (asserted without depending on wgpu).
- Blocked by: intake-2, PP1, A2.

**GP3. `feat(platform): gamepad plugin systems, GamepadConfig and GamepadSnapshot`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/` additions: `GamepadState`, `GamepadConfig` and `GamepadSnapshot` inserted in `build`; `cfg_attr(feature = "reflect", derive(Reflect))` with the pinned type names on `GamepadConfig` and `GamepadSnapshot` (the id, button and axis types and the gamepad variants of `InputEvent` are reflected by PP1); `apply_platform_events` dispatching to `GamepadState::apply`; `reprocess_gamepad`; the view, `end_tick` and `end_frame` calls (including `end_tick` in `Last` while `SimulationMode` is `Frozen`) and the dirty-gated snapshot refresh; `release_all` on focus loss; `examples/plugin_window.rs` gains a gamepad printout so both edge views are visible for a pad.
- Acceptance:
  - [ ] Injected gamepad events drive `GamepadState` through `First` in a headless app, seen by an `Update` system and by a `FixedUpdate` system with the specified edges, on a frame with no tick and on a frame with two ticks.
  - [ ] `Focused(false)` releases pad buttons; a changed `GamepadConfig` is reprocessed the same frame; `GamepadSnapshot` refreshes only when dirty; a pad button pressed on a `Frozen` frame is not `just_pressed` in the first `FixedUpdate` after the mode changes to `Running`.
  - [ ] `GamepadConfig` and `GamepadSnapshot` reflect and round-trip; an `InjectedInput` carrying a gamepad variant is readable and sendable by a generic tool; an out-of-range `GamepadId` sent that way is dropped and counted.
  - [ ] With `default-features = false` the crate still builds with no ECS dependency (the gamepad types and state are Stage 1).
- Blocked by: intake-2, GP2b, PP1, PP2, A2, AM3.

**AM2 and AM3: the code halves of the editor spec's amendments** (Stage 3 pre-editor lane; filed with the other lane issues from `editor-mcp-mvp.md` "Downstream adjustments", each `Blocked by` intake 2, listed here so the contract has one home). Neither adds a crate edge.

**AM2. `feat(platform): raw_event for every WindowEvent, the hook signature and a wake handle`** (blocks ED8 and FH1)
- Footprint:
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- The driver offers every `WindowEvent` to `raw_event` first (PL4b and PP2 already deliver the input events and own the signature); `WakeHandle`, `PlatformWindow::wake_handle`, the `EventLoop::<WakeUp>` user event and the `user_event` rule of "The wake handle"; the web no-op.
- Acceptance:
  - [ ] The pure translation tests still pass; a fake driver fed every `WindowEventKind` calls `raw_event` first, once, for each (focus, IME, modifier, scale, occlusion, resize, close and redraw included).
  - [ ] The consumption table of PL4a (`window/route.rs`) still passes with the fake driver now offering every kind, and the driver behaves as it says: a consumed keyboard press, mouse press or wheel never reaches `InputState`; a consumed cursor move becomes one `CursorLeft` and then nothing, and the next unconsumed move sets the cursor with no delta; a consumed release and `CursorLeft` still reach it; a consumed `Resized`, `ScaleFactorChanged`, `Focused` or `CloseRequested` is still processed. The `KeyboardInput` and `ScaleFactorChanged` arms of `classify` cannot be built in a test and are reviewed (the fake driver covers them with `WindowEventKind` values).
  - [ ] `WakeHandle` is `Clone + Send + Sync`, builds for wasm32, coalesces a burst into one event, and `user_event` runs a frame directly when the window is minimized or occluded (on Windows only the minimized case can occur: `Occluded` is not sent there). The PR body records a manual run on Windows: minimize the window, `wake()` from another thread, see the frame run.
- Blocked by: intake-2, PP2.

**AM3. `feat(app): SimulationMode in the frame driver`** (**blocks ED6**, which sets it, and PP1, whose `Last` system reads it)
- Footprint:
  - Edits: `crates/exitium_app/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `SimulationMode` in `time.rs`, its registration in `App::new`, the step 3 and step 7 changes of "`SimulationMode`", `FrameStats::frozen_frames`, and its row in `benches/frame.rs`.
- Acceptance:
  - [ ] Every `SimulationMode` test of the Test plan passes.
  - [ ] `SimulationMode` is reflected with its pinned name; the frame-driver benchmark holds for a `Running` frame (the new read costs nothing measurable) and the `Frozen` row is within budget.
- Blocked by: intake-2, A2, A4 (`benches/frame.rs` and its `[[bench]]` stanza are created by A4; PP1 and GP3 already wait on AM3, so there is no cycle).

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
- Re-export `App`, `Plugin`, `PluginGroup`, the ten schedule labels, `Time`, `TimeConfig`, `SimulationMode`, `Transform2d`, `GlobalTransform2d`, `AppExit`, `KeyCode`, `MouseButton`, `InputState`, `WindowInfo`, `GamepadState`, `GamepadConfig`, `GamepadId`, `GamepadButton`, `GamepadAxis`, `ThreadBudget` and `ThreadingPlugin` (F4); add `ThreadingPlugin` and then `PlatformPlugin` to `DefaultPlugins`, first and second.
- Acceptance:
  - [ ] A game that depends only on `exitium` builds a window app with a `FixedUpdate` system that reads `ButtonInput`.
  - [ ] A facade doc test builds a headless app and runs one frame through `exitium::` paths only.
  - [ ] A game that depends only on `exitium` names `exitium::ThreadBudget` and builds `ThreadingPlugin { budget: Some(ThreadBudget::single()) }`; `DefaultPlugins` lists `ThreadingPlugin` first and `PlatformPlugin` second (asserted by a test of the group's order).
  - [ ] The facade API change gets the extra reviewer scrutiny of ADR-0005 (high-risk area).
- Blocked by: intake-2, A3, PP2, F4, and the facade issue that creates `DefaultPlugins`.

**F4. `feat(exitium): ThreadingPlugin, the default task pool`** (about 150 lines; amended, #139, from the SP2 review)
- Footprint:
  - Edits: none
  - Governs: none
  - Hotspots: `crates/exitium/` (the facade)
  - New crate edges: none (`exitium -> exitium_tasks` is named in intake-2: the facade depends on every runtime crate)
- `ThreadingPlugin { budget: Option<ThreadBudget> }`: in `build`, resolve the budget (`ThreadBudget::detect()` when `None`), build `exitium_tasks::TaskPool::from_resolved(..)` and install it once with `TaskPool::set_default(Arc<TaskPool>)`, before any schedule runs; no system, no resource. When the resolved budget carries an `env_warning` (`EXITIUM_THREADS` was present but invalid), the plugin prints it once with `eprintln!` (the tasks crate prints nothing, and the tasks spec leaves this print to the side that resolves the budget, which is the facade here and not `exitium_app`). It is the first plugin of `DefaultPlugins` (F3).
- Acceptance:
  - [ ] After the plugin builds, `exitium_tasks::current_threads()` equals the resolved budget on a thread outside the pool, and a `Parallel` `Update` schedule runs two disjoint systems on two threads on a machine with at least two (the test skips with a message on one).
  - [ ] `budget: Some(ThreadBudget::single())` makes every schedule run inline, and the app's results equal the multi-threaded run for ECS-only communication (a small generated app).
  - [ ] **Determinism test** (the Test plan's "Threads and repeatability" row): a `FixedUpdate` system that calls `par_iter_mut` with an order-dependent body (a floating-point running sum over a column) gives bit-identical results on 20 runs under `ThreadBudget::single()`; the same system with an order-independent body (disjoint per-row writes, `Batching::Fixed(n)`, partials combined in chunk order) gives the single-thread result on a four-thread pool. The test does not assert that the order-dependent body differs on four threads (it may not on a given run); the module docs say why.
  - [ ] An invalid `EXITIUM_THREADS` is reported once and does not stop the app (the print is checked against the plugin's own message sink in a test).
  - [ ] An `App` without the plugin runs every schedule on the calling thread and does not panic (the inline pool of the tasks crate).
  - [ ] It builds for `wasm32-unknown-unknown`, where the pool is one inline thread.
- Blocked by: intake-2, A1 (and through it the ECS parallel executor and `par_iter_mut`, items 20a and 20b of `ecs-scheduler.md`), the `exitium_tasks` pool issue that adds `TaskPool::set_default`.
- The facade API change gets the extra reviewer scrutiny of ADR-0005 (high-risk area), like F3.

**Adjustments to existing issues** (the architect edits the bodies after this spec merges):

- **#49 (hierarchy):** its Context says transform propagation is "scene crate, Phase 2". It is `exitium_app` (A3). Add the ECS-side needs: `Children` exposes its slice and `Parent` its entity, and `despawn_recursive` and the helpers keep `Children` consistent, since propagation trusts them.
- **#22 items 11 and 13 (the ECS scheduler):** `ScheduleLabel::new` and `SystemSet::new` are `const fn`; `register_reflected_event::<E>` takes the clock. Both are one line in the issue bodies.
- **UI (#109), U11:** `ui_input` takes a UI-owned `UiInputFrame` resource and stores `UiCapture`, and names no platform type (`exitium_ui` has no edge to `exitium_platform`, C1). F2 fills the resource from `InputState`. U11's headless tests write a `UiInputFrame` into the resource. `ui-mvp.md` is amended in this PR to match (the Stage 2 text, open questions 3 to 5 and U11).
- **Physics (#108) and UI (#109) issues:** their plugin issues are `Blocked by` A1 and A2 and use the `FixedFirst`, `FixedLast`, `PostUpdate` (before `PROPAGATE`) and `UI_INPUT` slots; resources are inserted in `build`.
- **Render (#107) issues:** `Extract` with the `EXTRACT` and `RENDER` sets, `Startup` for renderer init, `PreStartup` for the facade's window hand-off; no interpolation in v1.
- **`editor-mcp-mvp.md` AM2 and AM3.** Their spec halves are this amendment; their code halves are the `AM2` and `AM3` entries above. ED6 is `Blocked by` AM3, ED8 and FH1 by AM2. The editor spec's own sentence "(the supersession of the presets is recorded by AM2/AM3)" is satisfied by "MCP / editor surface, Tool names".
- **`editor-mcp-mvp.md` lines that this spec contradicts, until SP8 (#149) patches them** (#149's acceptance gets the patch). All three say a consumed event never reaches the game, and this spec narrows that: releases and `CursorLeft` always reach `InputState`, and a consumed cursor move becomes one `CursorLeft`. The lines (at the head of this PR's base): **line 284** (the event-flow sketch, "consumed events never reach the game"), **line 796** (the acceptance of ED8, "an event over an egui panel is consumed and never reaches `InputState`"), and **line 968** (the acceptance of the AM2 code issue, "a consumed keyboard or pointer event never reaches `InputState`"). **On a conflict this spec wins**, and the `AM2` entry above is the issue text to implement; an implementer who finds the older sentence in ED8 or in the editor's AM2 text follows this one.
- **SP4 (#140) and SP6 (#142) move their `Update` glue.** The rule and the table of systems are in "`SimulationMode`" (engine glue never lives in `Update`): the asset delivery and hot reload swap (FA1, FA5), the clip feeding (FA2), `LoadScene` (FA3), `apply_audio_settings` and `audio_events` move to `First` after `ASSETS` (the request scan to `PostUpdate`), and the UI `Headless` driver moves to `PostUpdate`, set `UI`, after `PROPAGATE`. #140 (PR #166) already makes the first move and cites this spec; the architect of #142 is told to make the second. The rule is this spec's; the schedule each system lands in is its owner's.
- **SP2 (#138, `ecs-scheduler.md`):** its parallel executor and `par_iter_mut` are only useful if a pool is installed and a schedule asks for it. "Executors and the thread pool" decides both: `Update` and `PostUpdate` are `Parallel`, everything else is `SingleThreaded`, the facade's `ThreadingPlugin` (F4) installs the pool with `TaskPool::set_default`, and `exitium_app` opens the `exitium_app::frame` span and calls `exitium_profiler::mark_frame()` (feature `profiling`). SP2's open question 12 is answered by that section, and its text that `exitium_app` installs the pool is changed to "the facade installs it" when #168 and this PR are both merged (a one-line edit in `ecs-scheduler.md`, owned by SP2).
- **After this spec merges:** `needs-spec` is lifted from the gamepad, AM2 and AM3 issues and from the platform and app issues that waited on it.

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
| `gilrs` (amended, #139) | `0.11.2` | `default-features = false`, `["wgi"]` | `Apache-2.0 OR MIT` (`gilrs-core` 0.6.8 the same) | yes (browser Gamepad API through `web-sys`; polling) | std has no gamepad API; native WGI and XInput are FFI and `unsafe` is denied; the SDL bindings need a C library and have no wasm32 build. Windows backend: `wgi` (Windows 10 and later). `xinput` and `serde-serialize` stay off. MSRV 1.84 (the pinned toolchain is newer). It starts one reader thread on Windows with force feedback off (see "Gamepad"). It also pulls the `log` crate (a dependency of `gilrs` and `gilrs-core`; the intake records whether the lock already had it through winit or wgpu). `wgi` and `xinput` are mutually exclusive. |

- `crates/exitium_platform/Cargo.toml`: `[dependencies] exitium_core.workspace = true, winit.workspace = true`; `web-time.workspace = true` (all targets); `[target.'cfg(target_family = "wasm")'.dependencies] web-sys.workspace = true, wasm-bindgen.workspace = true`; **`[target.'cfg(any(windows, target_family = "wasm"))'.dependencies] gilrs.workspace = true`** (target-gated because `gilrs-core` needs `libudev` to build on Linux, and Windows and Chrome are the tested targets); `[dev-dependencies] criterion.workspace = true`. **No other dev-dependency:** the examples use the crate's own dependencies and never `exitium_render` (C1). The `[[bench]]` stanza for `benches/input.rs` is added by PL2 (PL1 adds none), and A4 adds the one for `benches/frame.rs`.
- **`gilrs` checks for the intake PR (C5, licensing is a high-risk area).** `cargo deny check` (the file has `all-features = true` and checks every target, so the Linux and macOS crates in `gilrs-core`'s tree are checked even though they are not built): every transitive licence must be on `deny.toml`'s allow list, and an advisory on a transitive crate (an unmaintained one, for example) is handled in `deny.toml` with a stated reason, or this decision is reopened. The PR body records the `Cargo.lock` diff, the `windows` crate version the resolver picks (`gilrs-core` accepts `>=0.44, <=0.62`; `multiple-versions` is `warn`) and that the Linux CI image still builds the platform crate with no `libudev`. `gilrs-core` enables `web-sys` features (`Gamepad`, `GamepadButton`, `GamepadMappingType`, `Window`, `Navigator`, `DomException`) by unification, so the workspace `web-sys` entry does not change. No new crate edge.
- `raw-window-handle` is **not** a direct dependency: `winit` re-exports `raw_window_handle` (0.6) under `rwh_06`, which is the version `wgpu` 30 uses.
- `deny.toml`: no change expected (every licence is MIT or Apache-2.0; winit's Windows tree is `windows-sys` and similar). Intake runs `cargo deny check`.
- `xtask`: no `NATIVE_ONLY` entry for either crate. Proposed, optional: the wasm step also runs `cargo build --examples --target wasm32-unknown-unknown -p exitium_platform`, so the example's web build is checked in CI and not only by hand (this is a change to the `xtask/` hotspot, so it belongs in the intake PR).
- `exitium_app` gains **no** Stage 1 change: it has no workspace dependency until intake 2.

**Intake 2 (Stage 2 to 4).**

- Workspace crate edges: `exitium_app -> exitium_core`, `exitium_app -> exitium_ecs`, `exitium_app -> exitium_reflect`, and (amended, #139) the **optional** edge `exitium_app -> exitium_profiler`, behind the `profiling` feature (default off; the `frame` span and `mark_frame`). **No** edge `exitium_app -> exitium_tasks`: the facade installs the pool (F4), and the app only names `exitium_ecs::ExecutorKind`. `exitium_platform -> exitium_ecs`, `-> exitium_app`, `-> exitium_reflect`, as optional dependencies behind features. Facade: `exitium -> exitium_app`, `exitium -> exitium_platform` and (spelled out, amended in the round 2 fix) `exitium -> exitium_tasks`, which F4's `ThreadingPlugin` needs (the facade depends on every runtime crate, so the roadmap's intake already covers it).
- `crates/exitium_platform/Cargo.toml`: optional dependencies `exitium_ecs`, `exitium_app`, `exitium_reflect`; features `reflect = ["dep:exitium_reflect"]`, `plugin = ["reflect", "dep:exitium_ecs", "dep:exitium_app"]`, `default = ["plugin"]`. Standalone (Stage 1 style) use is `default-features = false`; xtask should check that build too (a `--no-default-features` check for the platform crate, if the intake finds that `FEATURE_CHECKS` cannot express it, a note in the PR).
- `crates/exitium_app/Cargo.toml`: `[dependencies] exitium_core.workspace = true, exitium_ecs.workspace = true, exitium_reflect.workspace = true`; `[dev-dependencies] criterion.workspace = true`; `[dependencies] exitium_profiler = { workspace = true, optional = true }` and `[features] profiling = ["dep:exitium_profiler"]` (amended, #139; `FEATURE_CHECKS` gets a `profiling` entry for `exitium_app`, like the ECS one in SP2, and `cargo xtask ci wasm` builds it). **No crates.io dependency.** The ECS derives (`Resource`, `Component`, `Event`) are re-exported by `exitium_ecs` (item 16), and `Reflect` by `exitium_reflect`, so no derive crate is named.
- **Editor hooks and `SimulationMode` need no wiring.** `PlatformPlugin::with_raw_event_hook` and `WakeHandle` add no dependency and no crate edge: the editor reuses this workspace's `winit` entry (below) and reaches the hook through `exitium_platform`'s own API. `SimulationMode` is inside `exitium_app`. The editor's `egui-winit` entry is #110's intake-2 list.
- **Not added, on purpose:** the `xinput` feature of `gilrs` (and with it `rusty-xinput` and `winapi`), `gilrs`'s `serde-serialize`, `libudev` on Linux, any edge from `exitium_app` or `exitium_platform` to `exitium_render`, `exitium_physics`, `exitium_audio`, `exitium_assets`, `exitium_ui`, `exitium_scene`, `exitium_command`, `exitium_mcp` or `exitium_editor` (C1, C3); an edge from `exitium_app` to `exitium_platform` (the platform plugin depends on the app, never the reverse); an edge from `exitium_app` to `exitium_tasks` (the facade installs the default pool); `console_error_panic_hook` (a 20-line function replaces it); `wasm-bindgen-futures` and `js-sys` (nothing here is async); `raw-window-handle` (through winit); `egui-winit` and a direct `winit` entry for the editor (#110's intake list; it reuses this workspace entry, which keeps one winit version).
- **Crate graph rule:** `exitium_app` and `exitium_platform` stay runtime crates that build for `wasm32-unknown-unknown` and are not in `NATIVE_ONLY`. A change that would break C1 or C3 is a spec error, not intake input.

**Known graph after both intakes** ("A -> B" means A depends on B):

```
exitium_app             -> exitium_core, exitium_ecs, exitium_reflect, exitium_profiler (optional, feature `profiling`)
exitium_platform        -> exitium_core                                   (Stage 1)
exitium_platform        -> exitium_core, exitium_ecs, exitium_app, exitium_reflect   (Stage 2, feature `plugin`)
exitium (facade)        -> exitium_app, exitium_platform, exitium_tasks, ... (every runtime crate)
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
| Roadmap (`jam-mvp.md`) | `exitium_app` ownership list; C1; C5 and C6; Stage 1 platform lane; Windows and Chrome only | As above; the lane ends with PL5 and GP2b, which run on both. |
| Roadmap (`mvp-roadmap.md`) SP3 | Platform gamepad: one more `InputEvent` source, native and web, mapping, deadzones, the plugin systems | "Gamepad": `gilrs` 0.11.2 decided, GP1a to GP3. The roadmap's gamepad budget ("event to `InputState` in the same frame, no allocation") is the Performance rows. |
| Editor (`editor-mcp-mvp.md`) X2 and AM2 | `raw_event` for every `WindowEvent`, the exact hook signature, a wake handle | "The editor's raw-event hook" and "The wake handle"; `PlatformPlugin::with_raw_event_hook(impl FnMut(&PlatformWindow, &WindowEvent) -> bool + 'static)`; the `AM2` issue. |
| Editor X3 and AM3 | A way to freeze `Update` in Edit mode | "`SimulationMode`"; the `AM3` issue. |
| Assets (SP4, #140; `assets-scene-export.md` FA1, FA2, FA3, FA5 and `AudioPlugin`'s `apply_audio_settings` and `audio_events`) | Frozen skips `Update`, where the asset feeding glue, the audio feeding glue, `LoadScene`, `apply_audio_settings` and `audio_events` run today | "`SimulationMode`", engine glue never lives in `Update`: delivery, the hot reload swap, FA2, FA3, `apply_audio_settings` and `audio_events` move to `First` after `ASSETS`, and the FA1 request scan to `PostUpdate`; the autoplay unlock stays in `First` and `audio_update` in `Last`. #140 (PR #166) makes the move and cites this section; its schedule choices are the ones in the table. |
| UI (SP6, #142; `ui-mvp.md` `UiPlugin` `Headless` driver) | The headless driver runs at the end of `Update` | The same rule: it moves to `PostUpdate`, in the set `UI` (declared by `ui-mvp.md`, ordered `after_set(PROPAGATE)`). #142 makes the move. |
| Editor (`editor-mcp-mvp.md` lines 284, 796 and 968) | Say a consumed keyboard or pointer event never reaches `InputState` (the event-flow sketch, the ED8 acceptance, the editor's AM2 code issue) | **This spec wins** until SP8 (#149) patches those three lines: a consumed press, wheel or cursor move never reaches `InputState`, a release and `CursorLeft` always do, and a consumed cursor move becomes one `CursorLeft` ("The editor's raw-event hook"). |
| ECS scheduler (SP2, #138; `ecs-scheduler.md` parallel executor, "Open question 12" and the `exitium_tasks` process pool) | Who installs `TaskPool::set_default`, which schedules are `Parallel`, where `mark_frame` is called and the `frame` span opened | "Executors and the thread pool": the facade's `ThreadingPlugin` (F4) installs the pool; `Update` and `PostUpdate` are `Parallel`, the other eight `SingleThreaded`; `run_frame` opens the `exitium_app::frame` span and calls `mark_frame()` once per call (feature `profiling`, edge `exitium_app -> exitium_profiler`, no edge to `exitium_tasks`). |
| Editor D9 and "MCP / editor surface" | The `app_*` and `input_inject` presets are replaced by `play_*` and `ecs_send_event` of `InjectedInput` | "MCP / editor surface, Tool names": no preset is built. |
| Editor, "Other edits to existing issue bodies" | The editor reads `PrimaryWindow` and `WindowInfo` and installs the hook through `PlatformPlugin` | "MCP / editor surface, Editor surface". |
