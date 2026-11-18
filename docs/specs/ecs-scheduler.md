# Spec: ECS scheduler, system access, commands, resources and events

- Issue: #22
- Crates: exitium_ecs (depends on exitium_core, exitium_reflect, exitium_tasks and, new, exitium_profiler; see "Intake wiring")
- Status: draft

## Goal

Define how systems declare the data they touch, how the scheduler proves two systems cannot conflict, how order is expressed, how structural changes are deferred (`Commands`), how global data and messages work (resources, double-buffered events), and how an executor runs all of it. The storage spec (`ecs-storage-queries.md`, #21) already provides the primitives this spec builds on: `FilteredAccess`, `UnsafeWorldCell`, `TickWindow` and the cursor and sweep API. The job system spec (`tasks-job-system.md`, #19) provides `scope` and `parallel_for`. Reflection (`reflect-model.md`, #20) provides `TypeData` hooks.

**Jam MVP scope (owner-approved contract, 2026-11-18).** The executor that ships is **single-threaded**. Everything that makes a parallel executor possible is still specified and built now: per-system access sets, the conflict rule, the dependency graph with sync points, and a `System` trait with an unchecked, shared-world entry point. The parallel executor (#48) therefore lands later as a new file with no change to any public API. The MVP/post-MVP split is marked in every section and summarized in "Implementation issues".

Design in one paragraph: a **system** is a plain function whose parameters (`Query`, `Res`, `ResMut`, `Commands`, `EventReader`, `EventWriter`, `Local`) each report an access at build time. A **`Schedule`** collects systems, orders them by `before`/`after`/sets, derives conflict edges from the access sets, inserts **sync points** where deferred `Commands` must apply, and compiles all of that into an immutable `Plan` (a DAG in a stable topological order). An **executor** runs the plan. The MVP executor walks the plan in order on one thread. A **`Schedules`** resource maps a `ScheduleLabel` to a `Schedule`, and the app crate decides which labels exist and in what order they run each frame.

## Non-goals

Not in v1 (each has a stated extension path, so none blocks sidescroller, isometric, pixel art, multiplayer or destruction):

- **A parallel executor in the Jam MVP.** Designed here ("Parallel executor"), delivered by #48 after the MVP.
- **Run conditions** (`run_if`), state machines and `OnEnter`/`OnExit`. State gating is done by running different schedules by label (the app decides). A condition would be a read-only system whose access merges into its target's access.
- **Nested sets, set-level conditions, `ParamSet`, system piping and system input/output values.** Two queries with provably disjoint filters can coexist in one system, which covers most `ParamSet` uses.
- **Non-send resources and main-thread-pinned systems.** See Open question 5.
- **Observers, component hooks, removal events.** Same non-goal as the storage spec.
- **Determinism.** Not required (INTERVIEW #10). See "Determinism".
- **Changing a schedule through MCP.** Systems are Rust code in the MVP (gameplay is Rust only), so no editor feature edits them, and agent parity is not violated.
- **The Phase 4 command bus, the app loop, `Time`, `Transform2d` and the schedule names.** Owned elsewhere ("Boundaries").

## Design

### Boundaries (who owns what)

| Concern | Owner |
|---|---|
| `System`, params, `SystemAccess`, `Schedule`, `Schedules`, `Commands`, resources, events, the executors | `exitium_ecs` (this spec) |
| The set of schedule labels (Startup, FixedUpdate at 60 Hz, Update, render extraction), their per-frame order, `Time`, `Plugin`, `App` | `exitium_app` (its own spec) |
| Typed, serializable, undoable commands used by the editor and MCP | `exitium_command` (the command-bus spec) |
| Span macros and the `profiling` feature | `exitium_profiler` |

The ECS exposes the hooks the app needs (`World::run_schedule`, `World::update_events`, `Schedules`) and never decides frame structure. Example labels below are illustrative; the app spec owns the real ones.

### Module layout

```
exitium_ecs/src/
  resource.rs        Resource, ResourceId, Resources, Res, ResMut                    safe
  event.rs           Event, Events<E>, EventCursor, EventClock, EventRegistry        safe
  command.rs         Command, CommandQueue, Commands, EntityCommands                 safe
  system/
    access.rs        SystemAccess, conflict rule, AccessError                        safe
    mod.rs           System trait, IntoSystem, SystemConfig, exclusive systems       safe
    param.rs         unsafe trait SystemParam + impls (Query, Res, ResMut,           UNSAFE (designated #4)
                     Commands, EventReader, EventWriter, Local)
    function.rs      FunctionSystem, the arity-0..8 macro, run_unchecked forwarding  UNSAFE (designated #4)
  schedule/
    mod.rs           ScheduleLabel, SystemSet, Schedule, ScheduleError, Schedules    safe
    graph.rs         explicit edges, topological sort, cycles, reachability,         safe
                     conflict edges, ambiguities, sync-point levels, Plan
    single.rs        SingleThreadedExecutor                                          safe
    parallel.rs      ParallelExecutor (post-MVP, #48)                                UNSAFE (designated #5)
```

Unsafe additions to the storage spec's inventory (it has three designated modules):

- `world/cell.rs` (designated #2) gains resource accessors (typed `&'w R` and `Mut<'w, R>` from a cell). No new `unsafe` kind: the same "no conflicting access for `'w`" contract.
- **Designated #4: `system/param.rs` and `system/function.rs`** (one invariant set, two files). Invariants are listed under "Unsafe additions".
- **Designated #5: `schedule/parallel.rs`**, post-MVP, with Miri tests of its own.

The MVP therefore adds one designated group (#4) and no `unsafe` to the executor: the single-threaded executor calls the **safe** `System::run(&mut self, &mut World, ..)`, which builds the writable cell internally from the exclusive `&mut World`.

### Systems, parameters and access sets

```rust
pub trait System: Send + 'static {
    fn name(&self) -> &'static str;                      // type_name of the fn item, or the `.named(..)` override
    fn identity(&self) -> TypeId;                        // TypeId of the fn item type, used by after_system/before_system
    fn initialize(&mut self, world: &mut World) -> Result<SystemAccess, AccessError>;   // registers ids, builds QueryStates, fills access
    fn is_exclusive(&self) -> bool;
    fn has_deferred(&self) -> bool;                      // owns a Commands buffer
    fn run(&mut self, world: &mut World, window: TickWindow);          // safe: exclusive world, builds the writable cell inside
    /// # Safety: see "Unsafe additions". Never called by the single-threaded executor.
    unsafe fn run_unchecked(&mut self, world: UnsafeWorldCell<'_>, window: TickWindow);
    fn apply_deferred(&mut self, world: &mut World) -> ApplyReport;
}

pub trait IntoSystem<Marker> { type System: System; fn into_system(self) -> Self::System; }
// Implemented by a macro for FnMut(P0..Pn) with n <= 8 and each Pi: SystemParam, and for fn(&mut World) (exclusive).
```

`SystemParam` is a sealed `unsafe trait` (users cannot add params in v1, so the access claim cannot be forged):

```rust
pub unsafe trait SystemParam: Sized {
    type State: Send + 'static;                          // per-system storage: QueryState, EventCursor, Local<T> value, CommandQueue
    type Item<'w, 's>: SystemParam<State = Self::State>; // what the function receives
    fn init_state(world: &mut World, access: &mut SystemAccess) -> Result<Self::State, AccessError>;   // adds this param's access
    /// # Safety: the access added in `init_state` is exactly what the returned item can touch,
    /// and no conflicting access exists for `'w` (caller proves it, see Unsafe additions).
    unsafe fn get_param<'w, 's>(state: &'s mut Self::State, cell: UnsafeWorldCell<'w>, window: TickWindow) -> Self::Item<'w, 's>;
    fn apply_deferred(_state: &mut Self::State, _world: &mut World) -> ApplyReport { ApplyReport::default() }
}
```

| Param | Access | Notes |
|---|---|---|
| `Query<'w, 's, D, F = ()>` | the query's `FilteredAccess` (reads, writes, tick_reads, with, without) | `iter`, `iter_mut`, `get`, `get_mut`, `single`, `par_iter_mut`, `iter_chunks_mut`, `is_empty`, `count`. Every call goes through the storage `_with` forms with the **run's** `TickWindow` (hand-off 1 below) |
| `Res<'w, R>` / `Option<Res<R>>` | resource read | `Deref`; `is_changed()`, `is_added()` against the run's window. Missing resource: panics naming system and resource; `Option<Res<R>>` yields `None` |
| `ResMut<'w, R>` / `Option<ResMut<R>>` | resource write | wraps the storage `Mut<R>`: stamps `this_run` on `DerefMut` only, `bypass_change_detection()` available |
| `Commands<'w, 's>` | none (reserves entity ids atomically through `&Entities`) | owns the system's `CommandQueue` as state; `has_deferred` is true |
| `EventReader<'w, 's, E>` | read of the resource `Events<E>` | the per-system `EventCursor` is its state |
| `EventWriter<'w, E>` | write of the resource `Events<E>` | two systems writing the same `E` serialize; that is the price of a plain `Vec` push and is fine at game event rates |
| `Local<'s, T: Default + Send + 'static>` | none | per-system value, lives as long as the system |

**Single-entity `Query::get`/`get_mut` use the run's window.** `Query` is built from `QueryState::get_with`, `get_mut_with` and `iter_with` with `TickWindow { last_run, this_run }`, so a system's single-entity write stamps `this_run`, never `world.change_tick()`. This closes hand-off 1 of the storage spec ("Downstream issue adjustments", #22); the end-to-end test is in the Test plan.

**Exclusive systems** are `fn(&mut World)`. They are a safe type (no params, no state beyond the function), their `SystemAccess` is `exclusive = true`, and they conflict with everything. They are not a "run" for tick purposes: their world-level writes stamp `world.change_tick()` like any other world-level write.

**Access data:**

```rust
pub struct SystemAccess {
    queries: Vec<FilteredAccess>,          // one per Query param: component reads/writes/tick_reads and with/without filters (storage spec)
    resources: Access,                     // reads/writes over ResourceId bitsets
    exclusive: bool,
    has_deferred: bool,
    combined: CombinedAccess,              // unions of the above for the O(words) fast-path test
}
```

**The conflict rule (the whole definition).** Systems `a` and `b` **conflict** when any holds:

1. `a.exclusive || b.exclusive`.
2. A resource is written by one and read or written by the other.
3. A query pair `(qa, qb)` with `qa` from `a` and `qb` from `b` has a component that one writes and the other reads or writes **and** the pair is not filter-disjoint. `tick_reads` count as reads (a `Changed<A>` reader never runs beside an `A` writer, as the storage spec requires).

Two queries are **filter-disjoint** iff some component is in `qa.with` and `qb.without`, or in `qb.with` and `qa.without`: no archetype can match both, so they can never touch the same row. `Option<&T>` adds no `with`. The same pairwise rule, applied to the params of **one** system, rejects the system at `initialize` with `AccessError::ParamConflict { system, param_a, param_b, name }` (for example `fn(Query<&mut Pos>, Query<&Pos>)`, or `Res<R>` with `ResMut<R>`), unless the two queries are filter-disjoint. A conflict inside a single `Query<D>` is the storage spec's `QueryAccessError`. Fast path: `combined` unions are tested first with a bitset AND (about 8 words per pair); only a hit pays the pairwise filtered test.

`FilteredAccess` must expose `with`/`without` (or `is_disjoint(&other)`); see "Downstream issue adjustments".

### Resources

```rust
pub trait Resource: Send + Sync + 'static {}              // hand impl, or #[derive(Resource)] (Implementation issues)
pub struct ResourceId(u32);                               // dense, per-process, never persisted (same rule as ComponentId)
pub struct Resources {
    by_type: HashMap<TypeId, ResourceId>,                 // in-crate multiplicative hasher, as the storage spec
    infos: Vec<ResourceInfo>,                             // { name: &'static str, type_id, tracked: bool }
    slots: Vec<Option<Column>>,                           // a Column of capacity 1: value, added/changed TickCell, drop_fn
}
```

- **Storage reuses `Column`** (designated #1, already `Send + Sync`, aligned, with tick arrays and the destructor shim) with `len` 0 or 1. The resource value is a typed `*mut R` from the column's base pointer, so no new `unsafe` kind exists and no `Box<dyn Any>` with its own `Sync` story is needed. Resources always track changes.
- **Ids are registered under `&mut World`**, like components: `World::init_resource_id::<R>()` is implicit in `insert_resource` and in `Res<R>::init_state`. A system can therefore name a resource that is inserted later (a Startup system, `Commands::insert_resource`). A separate id space from `ComponentId`: a type that is both a `Component` and a `Resource` causes no false conflict.
- **Ticks:** `insert_resource` of a new resource stamps `added = changed = world.change_tick()`. Overwriting an existing resource stamps `changed` only (same rule as an overwrite-insert of a component). `ResMut` stamps `this_run` on `DerefMut`. `remove_resource` then `insert_resource` is a fresh `added`. `Res::is_changed()` is `is_newer_than(changed, window.last_run, window.this_run)`; the first run (`last_run == None`) sees every resource as added and changed, which is intended (initial UI binding).
- **Structure changes need `&mut World`** (`insert_resource`, `remove_resource`, id registration), so resource slots never move during a run. Systems insert through `Commands`.
- `AppTypeRegistry` is a `Resource` through `impl Resource for AppTypeRegistry` (#50; reflect-model, "Registry ownership").

```rust
impl World {
    pub fn insert_resource<R: Resource>(&mut self, value: R);
    pub fn init_resource<R: Resource + Default>(&mut self);                 // no-op if present
    pub fn contains_resource<R: Resource>(&self) -> bool;
    pub fn get_resource<R: Resource>(&self) -> Option<&R>;
    pub fn resource<R: Resource>(&self) -> &R;                              // panics, message names R
    pub fn get_resource_mut<R: Resource>(&mut self) -> Option<Mut<'_, R>>;  // stamps world.change_tick() on DerefMut only
    pub fn resource_mut<R: Resource>(&mut self) -> Mut<'_, R>;
    pub fn remove_resource<R: Resource>(&mut self) -> Option<R>;
}
```

### Events (double-buffered)

```rust
pub trait Event: Send + Sync + 'static {}
pub struct Events<E: Event> { old: Vec<E>, new: Vec<E>, old_start: u64, new_start: u64, count: u64 }  // a Resource
pub struct EventId(u64);                                  // the event's index in send order, monotonic, never reused
pub struct EventCursor { next: u64, missed: u64 }         // per reader; state of EventReader, or held by hand
#[derive(Copy, Clone, Eq, PartialEq)] pub enum EventClock { Frame, Fixed }
```

- `send` pushes onto `new` and returns the `EventId`. `count` is the number of events ever sent (`u64`, so it cannot wrap on wasm32).
- **`Events::update()` swaps the buffers:** the old buffer is dropped (its `Vec` is cleared, capacity kept), `new` becomes `old`, `new_start = count`. **Lifetime:** an event sent between update `k` and update `k + 1` is readable until update `k + 2` drops it, that is, for two update intervals. A reader that runs at least once per interval sees every event **exactly once**.
- **Reader cursors:** `EventCursor::read(&mut self, &Events<E>) -> impl Iterator<Item = &E>` yields events with id `>= cursor.next` (old buffer first, then new), advancing `next` per item yielded, so an iterator dropped early leaves the rest for the next call. If `cursor.next < old_start` the events in between were dropped: the cursor jumps to `old_start` and adds the gap to `missed`. `EventReader::{read, len, is_empty, clear, missed}` wrap this; `clear` marks everything as read. A new `EventReader` starts at `next = 0`, so it reads whatever is still buffered.
- **Who calls `update`, and when.** `World::register_event::<E>(clock)` inserts `Events<E>` and records an erased `update` function in an `EventRegistry` resource, tagged with its clock. `World::update_events(clock)` calls every update registered for that clock. **The driver (`exitium_app`) calls `update_events(Frame)` once per rendered frame and `update_events(Fixed)` once after each fixed-step run.** Pitfall this design addresses: with a 60 Hz fixed schedule on a 144 Hz display, a `FixedUpdate` reader runs on some frames only, so an event sent in `Update` could expire before it runs; events that are produced and consumed in the fixed schedule use `Fixed`, and `EventReader::missed()` makes a leftover mismatch visible instead of silent. `Frame` is the default of `App::add_event` (app spec).
- A system that needs an event type that is not registered fails at `initialize` with `AccessError::UnregisteredEvent { name }` (message: call `register_event`), never at the first run.
- `World::send_event(e)` is a convenience over `resource_mut::<Events<E>>().send(e)` for exclusive code, tests, MCP (`ecs_send_event`) and the command bus. Events carry no change ticks.

### Commands (deferred structural changes)

```rust
pub trait Command: Send + 'static { fn apply(self: Box<Self>, world: &mut World); }
pub struct CommandQueue { cmds: Vec<Box<dyn Command>> }          // per system, reused across runs
pub struct Commands<'w, 's> { queue: &'s mut CommandQueue, entities: &'w Entities }
impl Commands<'_, '_> {
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> EntityCommands<'_>;   // reserves the id now: Entities::reserve_entity(&self)
    pub fn spawn_batch<B: Bundle>(&mut self, iter: impl IntoIterator<Item = B>);   // one reserve_entities(n) for n = size_hint
    pub fn entity(&mut self, e: Entity) -> EntityCommands<'_>;
    pub fn despawn(&mut self, e: Entity);
    pub fn insert_resource<R: Resource>(&mut self, value: R);
    pub fn remove_resource<R: Resource>(&mut self);
    pub fn add(&mut self, f: impl FnOnce(&mut World) + Send + 'static);    // custom command
}
impl EntityCommands<'_> { pub fn id(&self) -> Entity; pub fn insert<C: Component>(&mut self, c: C) -> &mut Self;
                          pub fn remove<C: Component>(&mut self) -> &mut Self; pub fn despawn(self); }
```

- **When buffers apply.** Never during a run. A buffer applies at a **sync point** of the schedule (rule below), always under `&mut World`, and always at the **end of the schedule run** (the last node of every plan is a sync point), so no buffer outlives its run.
- **How:** a sync node calls `world.flush_entities()` once, then `System::apply_deferred` on every system **in topological order**, each draining its queue in **recorded order**. So within one system, commands apply in the order they were issued; across systems, in plan order. Entity ids returned by `spawn` are valid for later commands in any buffer applied after the flush.
- **Dead or missing entities.** A command on an entity that no longer exists (despawned earlier in this or another buffer) is **skipped and counted**, not a panic: a crash on wasm aborts the page and an agent-written game needs a recoverable failure. `ApplyReport { applied, skipped_dead_entity }` is summed into `RunReport` and is visible to MCP (`ecs_get_stats` extension in the introspection issue). A skipped `spawn` is impossible because a reserved id is always live after the flush.
- **Cost of reservation.** Each `spawn` reserves one id (an atomic, within the storage spec's 20 ns / 60 ns contended budget); `spawn_batch` reserves all ids with one operation. A per-system reservation cache would cut the atomic further but needs `Entities` to take unused reservations back, which the storage spec does not provide; it is a perf follow-up (Open question 7). `World::spawn_reserved(&mut self, Entity, B)` places a flushed reservation straight into the bundle's archetype instead of an extra move out of the empty archetype (adjustment to #41).
- **Panics.** A panicking command or system abandons the run: remaining queues are cleared (ids that were reserved but never flushed stay unused, a bounded leak of entity-id slots on an abandoned run) and the panic propagates (see "Panic context").
- **Writes stamp the current tick.** Command application is a world-level write (storage spec, stamping rules), so `Changed<T>` observers see a system's own deferred commands in its **next** run, exactly once.

**Boundary with the Phase 4 command bus (different layers).**

| | `Commands` (this spec) | Command bus (`exitium_command`) |
|---|---|---|
| Producers | systems, during a run | editor, MCP, and any host code |
| Form | closure-like, not serializable, no undo | typed, serializable, with undo and history |
| Applies | at sync points **inside** a schedule run | **between** top-level schedule runs, never while a schedule runs |
| Failure | skips dead entities, counts them | returns a typed error to the caller, rolls back |
| Order | recorded order, then plan order | submission order |

The bus applies by calling `World` methods directly under `&mut World`; it does not go through `Commands`, and `Commands` never records undo. A bus command that spawns or despawns entities uses the same reserve/flush rules. Both are world-level writes, so both stamp `world.change_tick()` and bindings observe editor edits and system edits identically.

### Ordering, sets and the schedule graph

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)] pub struct ScheduleLabel(&'static str);   // const fn new; "crate::name" convention
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)] pub struct SystemSet(&'static str);        // flat, no nesting in v1

pub fn system<M>(f: impl IntoSystem<M>) -> SystemConfig;                  // erased, so arrays and Vecs hold mixed systems
impl SystemConfig {
    pub fn named(self, name: &'static str) -> Self;                        // required to add the same fn twice
    pub fn in_set(self, s: SystemSet) -> Self;
    pub fn before_set(self, s: SystemSet) -> Self;   pub fn after_set(self, s: SystemSet) -> Self;
    pub fn before_system<M>(self, f: impl IntoSystem<M>) -> Self;   pub fn after_system<M>(self, f: impl IntoSystem<M>) -> Self;
}
pub fn chain(systems: impl IntoIterator<Item = SystemConfig>) -> Vec<SystemConfig>;   // each runs after the previous one

impl Schedule {
    pub fn new(label: ScheduleLabel) -> Schedule;
    pub fn add_systems(&mut self, systems: impl IntoIterator<Item = SystemConfig>) -> &mut Self;
    pub fn configure_set(&mut self, s: SystemSet) -> SetConfig<'_>;        // .before_set / .after_set between whole sets
    pub fn build(&mut self, world: &mut World) -> Result<(), ScheduleError>;   // initialize new systems, compile the Plan
    pub fn run(&mut self, world: &mut World) -> RunReport;                 // builds first if dirty; panics on a build error
    pub fn report(&self) -> Option<ScheduleReport>;                        // introspection, None before the first build
}
```

Labels and sets are `&'static str` newtypes, not types, on purpose: a basic-level reviewer reads them at a glance, they need no derive or macro, and MCP can list them by name. The `crate::name` convention avoids collisions. `after_system(f)` names a system by the `TypeId` of its fn item type; a closure or a fn added twice needs `.named(..)`.

**Stages.** There are **no stages inside a schedule**. Coarse phases are sets chained with `configure_set(A).before_set(B)`. Coarse frame structure (Startup, fixed update, update, render extraction) is a sequence of **schedules**, which `exitium_app` runs in order. This replaces the "named stages" wording of #47.

**Fixed versus variable tick.** A fixed-rate schedule and a per-frame schedule are the same `Schedule` type. The only difference is cadence, which belongs to the driver: it runs the fixed schedule 0 to N times per frame and the variable one once. What the ECS guarantees for both: every `run` is a run for tick purposes (below), so a fixed schedule that runs three times in a frame sees changes made between its runs, and a schedule that does not run for a while simply has older `last_run`s. The catch-up cap and the `Time` swap are the app's.

**Build.** `build` runs once, lazily at the first `run`, and again only when systems were added. It:

1. Calls `initialize` on each new system against `&mut World` (ids, `QueryState`s, access); errors are `ScheduleError::Access`.
2. Builds the **explicit edges** from `before_*`/`after_*`/`chain`/`configure_set`. An edge to a set expands to every member; a set with no members contributes nothing; `after_system(f)` with `f` not in this schedule is `ScheduleError::UnknownSystem`. Two systems with the same identity and no `.named` is `ScheduleError::DuplicateSystem`.
3. **Topological sort** (Kahn's algorithm with a min-heap on registration index): the order is stable and equals registration order wherever nothing constrains it. A cycle is `ScheduleError::Cycle { systems: Vec<&'static str> }` naming the systems on one cycle (found by DFS over the leftover nodes).
4. **Sync points.** Let `deferred(s) = s.has_deferred()`. Over explicit edges only, define `level(n) = max(level(p) + deferred(p) as u32)` over explicit predecessors `p` (roots are 0). For every level `k >= 1` that contains a node, add `SyncPoint(k)`: it is a successor of every deferred system with level `k - 1` and a predecessor of every node with level `k`. Add a final `SyncPoint(end)` after everything. Meaning: **a system explicitly ordered after a system that has `Commands` sees those commands applied**; unordered systems do not get that guarantee. A sync point and an exclusive system are **barriers** (they conflict with everything).
5. **Reachability** over the explicit DAG (one bitset row per node, filled in reverse topological order).
6. **Conflict edges.** For each pair `(a, b)` with `a` earlier in topological order that conflicts (rule above) and has **no** explicit path from `a` to `b`: add the edge `a -> b`, and, when neither node is a barrier, record an **ambiguity** `(a, b, conflicting names)` (a sync point or exclusive system conflicts with everything by design, so those pairs are ordered but never reported). Conflicting pairs are thus always serialized, oriented by topological position (so by registration order when unconstrained). A pair that is already explicitly ordered adds no edge.
7. Emits the immutable `Plan`.

```rust
struct Plan {
    nodes: Vec<Node>,              // topological order; Node = System(SystemId) | SyncPoint(level)
    preds: Csr, succs: Csr,        // flattened adjacency (offsets + targets, u32)
    dep_count: Vec<u32>,           // preds per node, the initial value of the parallel executor's counters
    segments: Vec<Range<u32>>,     // maximal runs of non-barrier nodes between barriers (parallel executor)
    ambiguities: Vec<Ambiguity>,
}
pub enum AmbiguityCheck { Report, Deny }       // Schedule::set_ambiguity_check; default Report
```

Ambiguities are data, not log lines (the ECS has no logging dependency): `ScheduleReport` lists them, the app may print them, and a test can use `Deny` (build returns `ScheduleError::Ambiguous`) to forbid them. Two systems that conflict and are unordered are legal and deterministic in effect (see "Determinism"), but usually a missing `before`/`after`.

**Adding a system after the first build** marks the schedule dirty. Existing systems keep their state and `last_run`; only the new ones initialize. Removal is not offered in v1 (rebuild the schedule).

### `Schedules` and running by label

```rust
pub struct Schedules { map: HashMap<ScheduleLabel, Schedule> }   // a Resource, created on first use
impl World {
    pub fn schedules_mut(&mut self) -> &mut Schedules;           // add / get_or_insert by label
    pub fn run_schedule(&mut self, label: ScheduleLabel) -> Result<RunReport, ScheduleError>;   // NotFound | AlreadyRunning | build error
    pub fn run_system_once<M>(&mut self, system: impl IntoSystem<M>) -> ApplyReport;   // initialize, run, apply deferred: tests and tools
}
```

`run_schedule` **removes** the schedule from `Schedules` for the duration of the run (so systems can use `ResMut<Schedules>` for other schedules and `run_schedule` of other labels can nest), runs it under `catch_unwind` on native, **always reinserts it**, then resumes any panic. Running the same label re-entrantly is `AlreadyRunning`. A host that owns a `Schedule` value outside `Schedules` calls `schedule.run(&mut world)` directly.

### Executors and the run loop

```rust
pub enum ExecutorKind { SingleThreaded, Parallel }     // Schedule::set_executor; default SingleThreaded in the MVP
pub struct RunReport { pub systems_run: u32, pub sync_points: u32, pub applied: u64, pub skipped_dead_entity: u64, pub sweep_slices: u32 }
```

**Per-system run (both executors, the whole tick contract):**

```
this = world.increment_change_cursor()                          // atomic, returns the pre-increment value
last = entry.last_run.and_then(|c| world.resolve_cursor(c).ok())   // an expired cursor becomes None: everything is seen once
system.run(world, TickWindow { last_run: last, this_run: Tick::from(this) })
entry.last_run = Some(this)                                     // only after the run returned
```

- **`last_run` is a `ChangeCursor` (64-bit), not a `Tick`.** This is a deliberate change from hand-off 2 of the storage spec, which asked the scheduler to clamp per-system `Option<Tick>` values with each `TickSweep`. A 64-bit cursor is resolved by age on every run, exactly like the standalone `QueryState` and external consumers in the storage spec, so there is **nothing to clamp**: it cannot alias after a wrap, it needs no registration, and it stays correct for schedules that are stored outside `Schedules`, run nested, or idle for a long time. An idle system whose cursor is older than 2^30 sees everything once (a false positive only, the same outcome as the clamp). Cost: one `resolve_cursor` per run (two compares). Open question 1 asks the owner to confirm.
- Exclusive systems and sync nodes do not increment: they are world-level (stamp `world.change_tick()`).
- A system's own `Commands` apply at a sync node after its run, stamping the current tick, so its **next** run sees them (the required storage-spec test) and never sees its own direct writes.
- A system that panics does not update `last_run`.

**Sync node:** `world.flush_entities()`; `apply_deferred` on every system with a non-empty queue in plan order; then **sweep polling**: `if world.needs_tick_check() { world.check_change_ticks(SWEEP_SLICE_TICKS) }` with `SWEEP_SLICE_TICKS = 1 << 20` (about 0.6 ms of the storage spec's sweep rate). This meets the host poll contract ("at least once per 2^27 increments"): every schedule run ends in a sync node, and one run adds only as many increments as it has systems. The returned `TickSweep` needs no per-system application (cursors).

**`SingleThreadedExecutor` (the MVP executor, and the permanent fallback).** Walks `plan.nodes` in order. A system node runs the per-system sequence; a sync node does the sync-node work. It is used when the pool has one thread and on `wasm32`, and in the MVP everywhere. It follows the **same plan and the same sync-point rules** as the parallel executor, so a schedule means the same thing on every target. It never calls `run_unchecked` and contains no `unsafe`.

**Panic context.** Before each system the executor stores `(schedule label, system name)` in a `thread_local` (`exitium_ecs::current_system() -> Option<(&'static str, &'static str)>`), so a panic hook (the app installs one) can print the failing system even on `wasm32`, where panics abort and `catch_unwind` does nothing. On native the executor also wraps each system in `catch_unwind` and rethrows `resume_unwind(Box::new(SystemPanic { schedule, system, payload }))`, leaving the original payload reachable. The run is abandoned: buffers are cleared, `last_run`s of unfinished systems are unchanged, and the world satisfies the storage spec's invariants because destructors run after bookkeeping.

### Parallel executor (post-MVP, #48; designed here so no API changes)

- **Partitioning.** `plan.segments` are the maximal runs of non-barrier nodes between barriers (sync points and exclusive systems). The executor walks the plan; a barrier node runs alone on the calling thread with `&mut World` (a sync point does the sync-node work); each segment runs inside **one** `exitium_tasks` `scope` as a dataflow: copy `dep_count` into `Vec<AtomicU32>` counters, spawn every node whose counter is 0, and when a system finishes, `fetch_sub(1, AcqRel)` on each successor, spawning the one that reaches 0 (`Scope::spawn` from inside a job is allowed, tasks spec "Nested jobs"). The end of the `scope` is the barrier. Because conflict edges are in the graph, any two nodes that can run at the same time have no conflict, which is exactly the precondition `run_unchecked` needs. A segment ends before the next barrier starts, so `&mut World` is never needed while a shared cell is live.
- **Cell and `&mut` hand-out.** The executor builds one `UnsafeWorldCell::from_mut(world)` per segment and passes it to each job; each system entry is wrapped in an uncontended `Mutex` so a job obtains its `&mut` without `unsafe` (cost about 15 ns, once per system per run). The only `unsafe` is the `run_unchecked` call and the `Send` wrapper for the cell, in designated #5, with a `// SAFETY:` citing the plan's conflict edges.
- **Nested parallelism.** A system that calls `par_iter_mut` or `parallel_for` runs inside the segment's scope; the tasks spec's help-first waiting runs those chunks on the same workers, so there are no extra threads.
- **Single-thread fallback.** `ExecutorKind::Parallel` resolves to `SingleThreaded` when `current_threads() == 1` or on `wasm32`. The `TaskPool` is the process default installed by the app (tasks spec, "Which pool do the free functions use?").
- **Failure.** On a system panic the executor stops spawning new nodes (a shared `AtomicBool`), lets in-flight jobs finish (the tasks spec has no cancellation), then resumes the panic as `SystemPanic`. Successors of the panicked system never run in that run.

### Determinism

**None is required or promised** (INTERVIEW #10). Specifically: the relative order of two systems with no conflict and no ordering is unspecified; entity ids returned by `Commands::spawn` depend on reservation order, which differs between runs under the parallel executor; `par_iter_mut` row order is unspecified; floating-point reductions across chunks may differ. As an implementation property (not a guarantee, tests may rely on it, games must not): two *conflicting* systems always run in plan order, commands apply in plan order, and the single-threaded executor runs the plan exactly in order, so an MVP run is repeatable on one machine. The parallel and single-threaded executors produce the same results for systems that communicate only through the ECS, up to the unspecified items above.

### Unsafe additions

The workspace deny stays. Designated #4 (`system/param.rs`, `system/function.rs`) **invariants** (each module's `//!` lists them):

1. `get_param` and `run_unchecked` are called only with a cell for which **no conflicting access exists for `'w`**. The caller proves it from the plan's conflict edges (the parallel executor) or from owning `&mut World` (the safe `System::run`, which builds `UnsafeWorldCell::from_mut` itself).
2. The cell is **writable (built by `from_mut`) whenever the system's access contains any write**; a read-only cell is passed only to systems whose access is read-only.
3. For every param, the `SystemAccess` that `init_state` recorded is exactly what the item returned by `get_param` can touch (the reason the trait is `unsafe` and sealed). `Query` calls only the storage `_with` entry points, whose access matches the query's `FilteredAccess`.
4. A param item borrows `state` (`&'s mut`) and the cell (`'w`) and cannot outlive the call: the function bound is higher-ranked over `'w` and `'s`, so a system cannot stash a `Query` or `Res`.
5. `Commands` only reads `Entities` (atomic reservation) and mutates its own queue; it never touches components or resources before the sync point.
6. `QueryState` refresh (`update_archetypes`) happens in `get_param` under the same cell, which is sound because archetypes are only created under `&mut World`; the `WorldId` assert from the storage spec applies on every entry.

Any later PR touching these files is high-risk: Miri tests, and the reviewer checks every `// SAFETY:` argument (ADR-0005). `crates/exitium_ecs/CLAUDE.md` must change from "exactly three" to the real count (done by #46).

## MCP / editor surface

Agent parity (rule 1) and reflection as source of truth (rule 2): components, resources and events are addressed by **stable type name** (reflection, #20); values travel as reflected JSON; no hand-written schema.

**Rust-level introspection (delivered in the MVP, no `exitium_mcp` code in Phase 1):**

- `Schedules::labels()`, `Schedule::report() -> ScheduleReport { label, order, systems[{ id, name, sets, reads, writes, resource_reads, resource_writes, exclusive, deferred, ordered_after }], sync_points[], ambiguities[{ a, b, conflicting }] }`.
- `World::resource_infos()` (name, size, `reflected`, tracked, present), `World::event_infos()` (name, clock, buffered, `count`, per-buffer lengths), `RunReport` counters.
- `ReflectResource` (type data in `exitium_ecs`, registered by `#[reflect(Resource)]`, listed in the reflect spec's type-data table): `contains(&World)`, `get(&World) -> Option<&dyn Reflect>`, `get_mut(&mut World) -> Option<ReflectedMut<'_>>` (stamps in `as_reflect_mut` only, the same guard as `ReflectComponent`), `insert(&mut World, &dyn Reflect)`, `remove(&mut World)`. `ReflectEvent` carries `send(&mut World, &dyn Reflect) -> Result<EventId, ApplyError>`.

**Phase 4 MCP tools (shape fixed here):**

| Tool | Kind | Behavior |
|---|---|---|
| `ecs_list_schedules` | read | `{label, systems, sync_points, ambiguities}` per schedule |
| `ecs_get_schedule` | read | the `ScheduleReport` for one label, access sets by stable component and resource names |
| `ecs_list_resources` | read | `{name, size, reflected, present}` |
| `ecs_get_resource` | read | one resource as reflected JSON, or a field by path |
| `ecs_list_events` | read | `{name, clock, buffered, total_sent}` |
| `ecs_get_stats` | read | extended (the storage spec defines it) with `last_run: { systems_run, sync_points, applied, skipped_dead_entity }` per schedule |

Mutation goes through the command bus (rule 3), each command its own MCP tool, applied between schedule runs with undo (history tools belong to the bus spec):

| Command | MCP tool | Undo |
|---|---|---|
| `InsertResource { type_name, value }` | `ecs_insert_resource` | previous value (or removal) |
| `RemoveResource { type_name }` | `ecs_remove_resource` | re-insert snapshot |
| `SetResourceField { type_name, path, value }` | `ecs_set_resource_field` | previous field value |
| `SendEvent { type_name, value }` | `ecs_send_event` | none (an input, flagged non-undoable; the bus spec decides how history shows it) |

Resource writes stamp `world.change_tick()` and flag only when the write is performed (same guard rule as components), so a rejected `SetResourceField` flags nothing.

**Editor panels** read the same API: a **Systems** panel (schedule order, access sets, sync points, ambiguity warnings) from `ScheduleReport`, a resource inspector from `ReflectResource`, and event counters in the profiler panel. There is no editor action that edits a schedule, so there is no tool for it (Non-goals).

## Performance

Benchmarks live in `crates/exitium_ecs/benches/scheduler.rs` under the existing harness and the 5% gate (`cargo xtask bench-ci`). As in the storage spec, **ratios against baselines are binding; absolute figures are provisional ceilings** for the reference dev PC, re-baselined by the `perf` agent from the first benchmark PR. Single thread unless stated; release, `lto = "thin"`, `profiling` off.

| Metric | Budget |
|---|---|
| Single-threaded dispatch overhead per system run (no-op body; params `Query<&A>`, `Res<R>`, `Local<u32>`; excludes query iteration) | <= 120 ns, of which `increment_change_cursor` <= 20 ns and `resolve_cursor` <= 5 ns |
| 100 no-op systems with 5 sync points, one schedule run | <= 15 us (about 0.1% of a 16.6 ms frame) |
| `Res<R>` / `ResMut<R>` param fetch | <= 5 ns |
| `Query` param fetch, state built, no new archetypes | <= 25 ns, plus O(new archetypes) after one is created |
| `Commands::spawn` (2 components), record only | <= 60 ns |
| `Commands::spawn` round trip (record + apply + world spawn) | <= 250 ns (>= 4 M/s); `spawn_batch` of 1,000: <= 150 ns each |
| `Commands::insert` / `despawn` round trip (cached edge) | <= 350 ns / <= 200 ns |
| Applying an empty queue | <= 2 ns per system |
| `Events::send` | <= 10 ns amortized |
| `EventReader::read` | <= 3 ns per event |
| `Events::update` | O(1) plus the drop of the old buffer; <= 50 ns for an empty old buffer |
| `Schedule::build`, 100 systems, about 200 explicit edges | <= 0.5 ms |
| `Schedule::build`, 1,000 systems | <= 25 ms (O(n^2) pair test on bitsets, once at startup) |
| Memory per system | <= 1 KiB excluding `Local` values and `QueryState` caches; <= 64 B per plan node and 4 B per edge |
| **Parallel executor (post-MVP, binding for #48):** dispatch overhead per system | <= 1.5 us |
| 16 independent systems with 200 us bodies, 4 threads | >= 3.0x speedup over the single-threaded executor (75% efficiency) |
| 16 systems in one `chain`, 4 threads | <= 1.1x the single-threaded time |
| 100 independent no-op systems, 4 threads | <= 60 us total |

**Complexity:** run is O(nodes + edges) plus the systems' own work; build is O(systems^2 / 64 x words) for conflicts plus O(edges x systems / 64) for reachability; `Commands` apply is O(commands).

**Profiling hooks (exitium_profiler, off by default, zero cost when off).** Emitted with `profile_scope!`, named `crate::system` per the profiler's convention:

- `exitium_ecs::schedule::run` around `Schedule::run`, with the schedule label as a field.
- `exitium_ecs::schedule::system` around each system run, with the system name as a field.
- `exitium_ecs::schedule::sync_point` around each sync node (flush, applies, sweep poll).
- `exitium_ecs::schedule::build` around `build`, and `exitium_ecs::events::update` around `update_events`.
- Parallel executor adds `exitium_ecs::schedule::segment` per segment. `exitium_tasks` emits its own `scope` and `parallel_for` spans (#65).

The profiler's macro takes only a literal name today, so a per-system label needs a **field form** of `profile_scope!` (a small change in `exitium_profiler`, first-listed issue below). Until it lands, the system span is emitted unlabeled. Span names are fixed literals; per-call data goes in fields, never in the name.

## Test plan

- **Conflict rule:** a matrix of read/read (no conflict), read/write, write/write, `Changed<A>` reader versus `A` writer (conflict), resource read/write combinations, exclusive versus anything, and filter-disjoint pairs (`With<X>` against `Without<X>` do not conflict; `Option<&T>` adds no `with`); the same matrix inside one system (`ParamConflict`); `Query<(&A, &mut A)>` reported by the storage layer.
- **Graph:** `before`/`after`/sets/`chain` ordering respected in the plan; a set with no members is a no-op; cycle error names the systems; `UnknownSystem` and `DuplicateSystem`; topological order equals registration order when unconstrained; sync-point levels (A `Commands` system, B after A: sync between; unrelated deferred systems share one sync point; final sync always present); a conflicting unordered pair is serialized and reported as an ambiguity, `Deny` makes build fail, an explicitly ordered conflicting pair is not reported; adding a system after the first build keeps existing `last_run`s.
- **Commands:** spawn-then-insert on the reserved id within one buffer; despawn of a reserved entity; ordering across two buffers in plan order; commands on a dead entity are skipped and counted; `spawn_batch`; closure command; the buffer never applies mid-run (a later system in the same segment does not see it unless ordered after); no buffer survives the end of a run; panic clears queues.
- **Tick contract (end to end, from the storage spec's hand-offs):** a system that writes through `Query::get_mut` does not see that write as `Changed` on its next run, while an exclusive world-level write is seen; apply a command buffer after system S, run S again, S sees the command's changes exactly once; `(&mut A, Changed<A>)` does not retrigger itself; first run sees everything; a system idle for more than 2^30 ticks (force the counter) sees everything once; sweep polling: a schedule run with `needs_tick_check()` advances the sweep by one slice per sync node, a full sweep completes across runs, and no stored age exceeds the bound (reuses the storage spec's `force_change_tick` hooks).
- **Resources:** missing resource panics naming system and resource; `Option<Res>` yields `None`; insert-after-system-build works (id registered at `initialize`); `is_changed`/`is_added` for insert, overwrite, `ResMut` write, remove-then-insert; `ResMut` that is never dereferenced mutably flags nothing; a type that is both `Component` and `Resource` causes no conflict.
- **Events:** each event seen exactly once by each of several readers; events live two updates and are dropped at the third; a reader that skips two updates reports `missed`; early-dropped iterator leaves the rest; `Frame` and `Fixed` clocks update independently; unregistered event type fails at `initialize`; `EventId`s are monotonic across updates.
- **Executor (MVP):** runs the plan in order, identically on `ThreadBudget::single()`; panic yields `SystemPanic` with the system and schedule names (native); `current_system()` is set during a run; `run_schedule` removes and always reinserts, `AlreadyRunning` on re-entry, nested `run_schedule` of another label works; `run_system_once`.
- **Parallel (post-MVP, #48):** 1,000 runs of disjoint-component mutation give the correct result (race stress); results equal the single-threaded executor for ECS-only communication; panic in one system stops successors and reports the name; designated #5 is Miri-clean; the single-thread fallback runs on the wasm path.
- **Unsafe group #4:** Miri tests: a `Query` and `ResMut` in one system through a writable cell; two systems with disjoint access on two threads through one cell (use `std::thread::scope` if the tasks crate is not Miri-runnable); read-only cell with read-only systems only.
- **wasm32:** `cargo build --target wasm32-unknown-unknown` for `exitium_ecs`; `cargo xtask ci wasm` also builds it with `profiling`.
- **Benchmarks:** `benches/scheduler.rs` for every row of the Performance table, with the baseline "call the same function bodies by hand in a loop".

## Implementation issues

Each issue edits exactly one crate, is about 600 changed lines or fewer, and carries the footprint below (format from `factory-run`). Existing issues are adjusted rather than replaced. All `exitium_ecs` issues serialize (one crate has at most one issue in flight), so the order below is the intended order. After this spec merges, the architect files the new issues with `file-issue` into the Jam MVP milestone and removes `needs-spec` from #44, #45, #46, #47 and #48.

Common footprint **F-ECS** for `exitium_ecs` issues: `Edits: crates/exitium_ecs/`, `Governs: none`, `Hotspots: none`, `New crate edges: none` (all edges are added by the intake PR). Each such issue also updates `crates/exitium_ecs/CLAUDE.md` where it changes a stated fact. Unless noted, every issue is blocked by the storage chain it needs (#38 to #43) in addition to what is listed.

**MVP slices**

1. **`feat(profiler): field form of profile_scope!`** (new, `exitium_profiler`). `profile_scope!("name", field = expr)`; compiles to nothing with the feature off; doc and test for the macro path. Footprint: `Edits: crates/exitium_profiler/`, no hotspots. Blocked by: none. Replaces the profiler half of #65.
2. **#44 narrowed: resources.** `Resource`, `ResourceId`, `Resources` on `Column`, `World` resource API, `Res`/`ResMut` types, resource ticks, `world/cell.rs` resource accessors. AC: the API above, tick rules tested, no new `unsafe` beyond the cell accessors (Miri test for them). Footprint: F-ECS. Blocked by #40, #41 and #43. ~550 lines.
3. **New: `feat(ecs): double-buffered events`.** `Event`, `Events<E>`, `EventCursor`, `EventClock`, `EventRegistry`, `World::register_event`/`update_events`/`send_event`. AC: lifetime, cursor and missed-event tests above; no `unsafe`. Footprint: F-ECS. Blocked by item 2. ~400 lines.
4. **#45 narrowed: `Commands`.** `Command`, `CommandQueue`, `Commands`, `EntityCommands`, `ApplyReport`, apply rules, dead-entity skip; adds `World::spawn_reserved` (adjusts #41's surface). Footprint: F-ECS. Blocked by item 2 and #41. ~500 lines.
5. **New: `feat(ecs): SystemAccess and the conflict rule`.** `SystemAccess`, `CombinedAccess`, the rule including filter-disjointness, `AccessError`; requires `FilteredAccess::{with, without}` (see adjustments). Safe code, no `unsafe`. Footprint: F-ECS. Blocked by item 2 and #42. ~400 lines.
6. **#46 narrowed: system functions and the core params.** `System`, `IntoSystem` (arity 0 to 8, macro), `SystemParam`, `FunctionSystem`, exclusive systems, `Local`, `Res`, `ResMut`, `Commands` params, `run_system_once`; designated #4 with Miri tests. Footprint: F-ECS. Blocked by items 4 and 5. ~600 lines.
7. **New: `feat(ecs): Query, EventReader and EventWriter params`.** The `Query` param (all methods, run-window routing, `update_archetypes` in `get_param`) and the event params; the system-level window test. Designated #4. Footprint: F-ECS. Blocked by items 3 and 6. ~500 lines.
8. **#47 narrowed: schedule graph and builder.** `ScheduleLabel`, `SystemSet`, `SystemConfig`, `Schedule`, `build`, topological sort, cycle error, reachability, sync-point levels, conflict edges, ambiguities, `Plan`, `ScheduleReport`. Safe code. Footprint: F-ECS. Blocked by item 6. ~600 lines.
9. **New: `feat(ecs): single-threaded executor, Schedules and run_schedule`.** `SingleThreadedExecutor`, the per-system tick sequence, sync nodes with sweep polling, `Schedules`, `World::run_schedule`, `RunReport`, `SystemPanic` and `current_system()`. Safe code. Footprint: F-ECS. Blocked by items 7 and 8. ~500 lines. **The ECS scheduler is usable after this issue.**
10. **New: `feat(ecs): ReflectResource and ReflectEvent`.** The two type-data structs, `#[reflect(Resource)]` registration, `register_reflected_resource`, `ReflectedMut` reuse for resources. Footprint: F-ECS. Blocked by items 2 and 3, and #50. ~350 lines.
11. **New: `feat(ecs): scheduler introspection for MCP`.** `Schedules::labels`, `Schedule::report` wiring, `resource_infos`, `event_infos`, `RunReport` in `WorldStats`. Footprint: F-ECS. Blocked by items 9 and 10. ~300 lines.
12. **New: `feat(ecs): schedule and event spans`.** The spans listed above through `profile_scope!`, the `profiling` feature wiring, a `StatsLayer`-style test that sees the expected span names for a 3-system schedule. Footprint: F-ECS. Blocked by items 1 and 9. ~250 lines. Replaces the ECS half of #65 (its tasks half is outside this spec).
13. **New: `perf(ecs): scheduler and commands benchmarks`.** `benches/scheduler.rs` for the single-threaded rows of the Performance table, with baselines. Footprint: F-ECS. Blocked by item 9. ~350 lines.
14. **New: `feat(ecs): derive(Resource) and derive(Event)`.** Adds the two derives next to `#[derive(Component)]`; the crate that hosts them is whichever #39 chose, so this issue's `Edits` is that crate only (`crates/exitium_ecs/` or its sibling derive crate). Hand impls (`impl Resource for T {}`) work without it. Footprint: as stated (no hotspots, no new edges). Blocked by #39 and items 2 and 3. ~150 lines.
15. **New: `feat(exitium): re-export the scheduler API`** (facade, high-risk surface). `Schedule`, `system`, `chain`, `Commands`, `Query`, `Res`, `ResMut`, `Local`, `EventReader`, `EventWriter`, resources and events. Footprint: `Edits: crates/exitium/`, `Hotspots: crates/exitium/`, no new edges. Blocked by item 9 and the first facade PR (#84). ~150 lines.

**Post-MVP**

16. **#48 adjusted: parallel executor.** Footprint: F-ECS. Segments, dataflow counters, per-entry `Mutex`, `run_unchecked` calls, `SystemPanic`, single-thread fallback; designated #5 with Miri tests; the Performance rows marked parallel. Blocked by item 9, #28, #27 and #29. ~600 lines.
17. **#52 / #67 adjusted**: add the parallel scheduler rows to the 1M-entity benchmark and the headless parallel sim. Existing issues; unchanged scope otherwise.

**Adjustments to existing issues**

- **#44** is split into items 2 and 3 above (resources, events). **#45** keeps its body but gains `spawn_reserved` and the dead-entity-skip rule. **#46** is split into items 6 and 7. **#47** gains the conflict-edge, ambiguity and sync-level rules and drops "named stages" and "parallel batches" (the Plan has segments; there are no stages). **#48** follows item 16.
- **#49 (hierarchy)** is unchanged except that `despawn_recursive` applies through `Commands` and relies on the dead-entity skip rule.
- **#50** is additionally blocked by item 2 (it implements `Resource` for `AppTypeRegistry`).
- **#65** is split by crate: item 1 (profiler), item 12 (ECS); the tasks half belongs to the tasks spec's issues.

## Intake wiring

For the Stage 1 and Stage 2 intake PRs (so no implementation issue touches `Cargo.lock`):

- **Workspace edges:** `exitium_ecs -> exitium_core`, `exitium_ecs -> exitium_reflect`, `exitium_ecs -> exitium_tasks` (already required by the storage spec), and **new** `exitium_ecs -> exitium_profiler`. `exitium_ecs` gets a feature `profiling = ["exitium_profiler/profiling"]`, off by default.
- **crates.io dependencies:** none. Bitsets, the graph, the hasher and `Commands` are in-crate; `Mutex` and atomics come from std. (The `syn`/`quote` derive dependency belongs to #20/#39, not this spec.)
- **Crate graph:** `exitium_ecs` stays a runtime crate that builds for wasm32. It must not depend on `exitium_app`, `exitium_command` or any editor crate; the app, command-bus and ECS-plugin modules depend on it, not the reverse.

## Downstream issue adjustments

Adjustments this spec requires of the storage work (#38 to #43); the architect edits the issue bodies after the spec merges:

- **#42 (queries):** expose `FilteredAccess::with()`/`without()` (or `is_disjoint(&FilteredAccess)`); a public `QueryState::update_archetypes(&mut self, &World)`; `&self` forms that assume a refreshed cache (`iter_prepared`/`get_with`/`get_mut_with`/`par_iter_mut_with` taking a window and a world or cell, with a `debug_assert!` that the cache is current), so a `Query` param can iterate more than once through `&self`; the `UnsafeWorldCell`-taking entry points the storage spec already names.
- **#40/#41:** `Column` usable with capacity 1 for resources (a safe `column_info_of::<R>()` like `drop_fn_of`); `World::spawn_reserved`; `world/cell.rs` resource accessors (item 2 delivers them).
- **#43:** `Tick::from(ChangeCursor)` (already assumed by `DynSince`) is public; `Tick::clamp_last_run` stays available but the scheduler does not use it (cursor `last_run`).

## Open questions

1. **Cursor `last_run` instead of the clamped `Option<Tick>` of hand-off 2.** Chosen because it removes the broadcast-and-clamp problem for schedules held outside `Schedules`, nested runs and long-idle schedules, at one `resolve_cursor` per run. It deviates from the storage spec's hand-off wording. Confirm?
2. **Labels and sets as `&'static str` newtypes** (readable, MCP-friendly, collision risk handled by the `crate::name` convention) versus type-based labels. Confirm?
3. **Sync-point rule.** Deferred buffers apply only at sync points that exist because of an explicit ordering edge, plus at the end of a run, in both executors. The alternative (apply after every system on one thread) is friendlier but makes single-threaded and parallel behave differently. Confirm the strict rule?
4. **Event clocks.** `Frame` and `Fixed` are an ECS concept here; the app spec must call `update_events` for both. If the app wants a single clock, drop `EventClock`.
5. **Non-send resources.** wgpu's WebGPU handles may be `!Send` on wasm32 unless a wgpu feature changes that, which could force `NonSend` resources and main-thread-pinned systems. Extension path: a `main_thread` flag on `SystemAccess` (a barrier node in the plan, run on the calling thread) and a thread-id-checked slot in `Resources` (a new designated unsafe module). The render spec decides whether the MVP needs it.
6. **Run conditions and states.** Cut from v1; state gating is by schedule label. Does Glutton's UI flow need `run_if`?
7. **Per-system entity reservation cache.** Needs an `Entities` API to take unused reservations back (storage spec). Only worth it if the `spawn` round-trip budget is missed.
8. **`Box<dyn Command>` per command** versus a byte arena. The box is simple and safe; an arena would need a new designated unsafe module. Perf follow-up if the 60 ns record budget is missed.
9. **Ambiguity default.** `Report` (data only) versus warning at build in debug builds (needs a logging path the ECS does not have). The app could print `ScheduleReport::ambiguities`.
10. **System enable/disable and removal at runtime** (an editor "pause system" feature) is deferred; it would need a tool and a command when added.
11. **Absolute budgets** are provisional; the `perf` agent confirms them after item 13 on the reference runner and the 4-thread min-spec profile.
