# Spec: ECS scheduler, system access, commands, resources and events

- Issue: #22
- Crates: exitium_ecs (depends on exitium_core, exitium_reflect, exitium_profiler and, via the derive re-export, exitium_ecs_derive; the `exitium_tasks` edge is added later by #48; see "Intake wiring")
- Status: draft

## Goal

Define how systems declare the data they touch, how the scheduler proves two systems cannot conflict, how order is expressed, how structural changes are deferred (`Commands`), how global data and messages work (resources, double-buffered events), and how an executor runs all of it. The storage spec (`ecs-storage-queries.md`, #21) already provides the primitives this spec builds on: `FilteredAccess`, `UnsafeWorldCell`, `TickWindow` and the cursor and sweep API. The job system spec (`tasks-job-system.md`, #19) provides `scope` and `parallel_for`, used only by the post-MVP parallel executor. Reflection (`reflect-model.md`, #20) provides `TypeData` hooks.

**Jam MVP scope (owner-approved contract, 2026-11-18).** The executor that ships is **single-threaded**, and **`exitium_ecs` does not depend on `exitium_tasks` in the MVP** (the contract cuts the tasks integration; the edge is added by #48, so the ECS lane and the tasks lane can run side by side). Everything that makes a parallel executor possible is still specified and built now: per-system access sets, the conflict rule, the dependency graph with sync points, and a sealed `System` trait with an unchecked, shared-world entry point (the trait seam). The parallel executor (#48) therefore lands later as a new file, a new `ExecutorKind` variant and a new crate edge, with no breaking change to any MVP API (`ExecutorKind` is `#[non_exhaustive]`). The storage spec's `par_iter_mut` is likewise delivered with #48 and is absent from the MVP `Query` param. The MVP/post-MVP split is marked in every section and summarized in "Implementation issues".

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

`System` is **sealed** (a private `sealed::Sealed` supertrait, implemented only by `FunctionSystem` and the exclusive-function system in this crate). Reason: the parallel executor's `// SAFETY:` argument rests on the access set a system reports being exactly what it touches; a downstream `impl System` could return a false `SystemAccess` and be scheduled beside a conflicting system. Users write systems as functions through `IntoSystem`, never by implementing the trait. (`SystemParam` below is sealed for the same reason.)

```rust
pub trait System: sealed::Sealed + Send + 'static {
    fn name(&self) -> &'static str;                      // type_name of the fn item, or the `.named(..)` override
    fn identity(&self) -> TypeId;                        // TypeId of the fn item type, used by after_system/before_system
    fn initialize(&mut self, world: &mut World) -> Result<SystemAccess, AccessError>;   // registers ids, builds QueryStates, fills access
    fn is_exclusive(&self) -> bool;
    fn has_deferred(&self) -> bool;                      // owns a Commands buffer
    fn run(&mut self, world: &mut World, window: TickWindow);          // safe: exclusive world, builds the writable cell inside
    /// # Safety: see "Unsafe additions". Never called by the single-threaded executor.
    /// Panics on an exclusive system (which needs `&mut World`); the parallel executor never calls it for one.
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
| `Query<'w, 's, D, F = ()>` | the query's `FilteredAccess` (reads, writes, tick_reads, with, without) | Read side, `&self`, bounded `D: ReadOnlyQueryData`: `iter`, `get`, `single`, `is_empty`, `count`. Write side, **`&mut self`**, items borrow the param: `iter_mut`, `get_mut`, `single_mut`, `iter_chunks_mut`. (`par_iter_mut` is added by #48.) Every call goes through the storage `_with` forms with the **run's** `TickWindow` (hand-off 1 below). The `&mut self` rule is what keeps two live `Mut<T>` for one entity impossible in safe code (invariant 3 under "Unsafe additions") |
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
pub trait Command: Send + 'static {
    fn apply(self: Box<Self>, world: &mut World);
    fn reserved_entities(&self, _out: &mut Vec<Entity>) {}         // spawn commands report the id they reserved (abandonment, below)
}
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
- **Panics and abandoned runs (no phantom entities).** A panicking command or system abandons the run. `CommandQueue` records the ids it reserved (`reserved: Vec<Entity>`) and `Command::reserved_entities(&self, out)` lets a not-yet-run spawn command report its id. On abandonment the schedule calls `CommandQueue::abandon(&mut self, &mut World)` on every system's queue: it drops the unexecuted commands, then `world.flush_entities()` and **despawns each id those unexecuted spawn commands had reserved** (a spawn that already ran is a normal entity and is left alone). Without this, the next sync node's flush would turn every unflushed reservation into a live, component-less entity visible to `Query<Entity>` and `WorldStats`. Abandonment is done by `Schedule::run`'s unwind path on native; on `wasm32` a panic aborts, so there is nothing to clean. Dropping a `Schedule` that still holds pending buffers (only possible if its run was abandoned without a world) leaks the reserved index slots, a bounded one-off. The panic then propagates (see "Panic context"). Test in the Test plan.
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
3. **Cycle check and levels.** Run Kahn's algorithm over the explicit edges only. A cycle is `ScheduleError::Cycle { systems: Vec<&'static str> }` naming the systems on one cycle (found by DFS over the leftover nodes). The same pass computes `level(n) = max(level(p) + deferred(p) as u32)` over explicit predecessors `p` (roots are 0), where `deferred(s) = s.has_deferred()`.
4. **Sync points.** For every level `k >= 1` that contains a node, add `SyncPoint(k)`: it is a successor of every deferred system with level `k - 1` and a predecessor of every node with level `k`. Add a final `SyncPoint(end)` that is a successor of every other node. Meaning: **a system explicitly ordered after a system that has `Commands` sees those commands applied**; unordered systems do not get that guarantee. A sync point and an exclusive system are **barriers** (they conflict with everything).
5. **Final order (where sync points sit).** Run Kahn again over explicit edges plus sync-point edges with a min-heap on this key: a system node has key `(registration_index, 1)`; `SyncPoint(k)` has key `(smallest registration index among its successors, 0)`, so it is picked immediately before its first-registered successor once its predecessors are done; `SyncPoint(end)` has key `(MAX, 0)`. The result is the plan's node order: stable, equal to registration order wherever nothing constrains it, with every sync point placed as late as its first consumer allows (so unrelated systems registered before that consumer run before the sync point). This order is observable (single-threaded run order, which unrelated systems the barrier waits for) and is pinned by a test.
6. **Reachability** over the explicit DAG (explicit plus sync-point edges; one bitset row per node, filled in reverse order of step 5).
7. **Conflict edges.** For each pair `(a, b)` with `a` earlier in the step 5 order that conflicts (rule above) and has **no** path from `a` to `b`: add the edge `a -> b`, and, when neither node is a barrier, record an **ambiguity** `(a, b, conflicting names)` (a sync point or exclusive system conflicts with everything by design, so those pairs are ordered but never reported). Conflicting pairs are thus always serialized, oriented by plan position (so by registration order when unconstrained). A pair that is already ordered adds no edge. Every added edge points forward in the step 5 order, so the order stays a valid topological order and no second sort is needed.
8. Emits the immutable `Plan`.

```rust
struct Plan {
    nodes: Vec<Node>,              // the step 5 order; Node = System(SystemId) | SyncPoint(level)
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
#[non_exhaustive] pub enum ExecutorKind { SingleThreaded }   // Schedule::set_executor; #48 adds `Parallel`, which falls back to SingleThreaded when the pool has one thread or on wasm32
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

**Sync node:** `world.flush_entities()`; `apply_deferred` on every system with a non-empty queue in plan order; then **sweep polling**: `if world.needs_tick_check() { world.check_change_ticks(SWEEP_SLICE_TICKS) }` with `SWEEP_SLICE_TICKS = 1 << 19` (524,288 stored ticks). At the storage spec's budget of 2.5 ms per 4M ticks that is about 0.33 ms; even at twice that cost it is about 0.65 ms, inside the storage spec's "slices of <= 1 ms per sync point" bound. This meets the host poll contract ("at least once per 2^27 increments"): every schedule run ends in a sync node, and one run adds only as many increments as it has systems. The returned `TickSweep` needs no per-system application (cursors).

**`SingleThreadedExecutor` (the MVP executor, and the permanent fallback).** Walks `plan.nodes` in order. A system node runs the per-system sequence; a sync node does the sync-node work. It is used in the MVP everywhere, and after #48 whenever the pool has one thread and on `wasm32`. It follows the **same plan and the same sync-point rules** as the parallel executor, so a schedule means the same thing on every target. It never calls `run_unchecked` and contains no `unsafe`.

**Panic context.** Before each system the executor pushes `(schedule label, system name)` onto a `thread_local` **stack** and pops it when the system returns (`exitium_ecs::current_system() -> Option<(&'static str, &'static str)>` reads the top), so a panic hook (the app installs one) can print the failing system even on `wasm32`, where panics abort and `catch_unwind` does nothing. A stack, not a single slot, because a system that waits in a nested scope can run another system's job on the same thread (tasks spec, help-first waiting): the push/pop pair restores the outer context when the inner one finishes. The MVP never nests, so the stack has depth 1; the rule is specified now so #48 does not have to change it. On native the executor also wraps each system in `catch_unwind` and rethrows `resume_unwind(Box::new(SystemPanic { schedule, system, payload }))`, leaving the original payload reachable. The run is abandoned: every queue is abandoned as described under "Panics and abandoned runs", `last_run`s of unfinished systems are unchanged, and the world satisfies the storage spec's invariants because destructors run after bookkeeping.

### Parallel executor (post-MVP, #48; designed here so no API changes)

- **Partitioning.** `plan.segments` are the maximal runs of non-barrier nodes between barriers (sync points and exclusive systems). The executor walks the plan; a barrier node runs alone on the calling thread with `&mut World` (a sync point does the sync-node work); each segment runs inside **one** `exitium_tasks` `scope` as a dataflow: copy `dep_count` into `Vec<AtomicU32>` counters, spawn every node whose counter is 0, and when a system finishes, `fetch_sub(1, AcqRel)` on each successor, spawning the one that reaches 0 (`Scope::spawn` from inside a job is allowed, tasks spec "Nested jobs"). The end of the `scope` is the barrier. Because conflict edges are in the graph, any two nodes that can run at the same time have no conflict, which is exactly the precondition `run_unchecked` needs. A segment ends before the next barrier starts, so `&mut World` is never needed while a shared cell is live.
- **Cell and `&mut` hand-out.** The executor builds one `UnsafeWorldCell::from_mut(world)` per segment and passes it to each job; each system entry is wrapped in an uncontended `Mutex` so a job obtains its `&mut` without `unsafe` (cost about 15 ns, once per system per run). `UnsafeWorldCell` is already `Send + Sync` (storage spec, designated #2), so no new `Send` wrapper is needed; the only `unsafe` in designated #5 is the `run_unchecked` call, with a `// SAFETY:` citing the plan's conflict edges. This issue also adds the `exitium_ecs -> exitium_tasks` edge, `ExecutorKind::Parallel` and `Query::par_iter_mut`.
- **Nested parallelism.** A system that calls `par_iter_mut` or `parallel_for` runs inside the segment's scope; the tasks spec's help-first waiting runs those chunks on the same workers, so there are no extra threads.
- **Single-thread fallback.** `ExecutorKind::Parallel` (added by #48, not present in the MVP) resolves to `SingleThreaded` when `current_threads() == 1` or on `wasm32`. The `TaskPool` is the process default installed by the app (tasks spec, "Which pool do the free functions use?"). Before #48 nothing in `exitium_ecs` names a pool.
- **Failure.** On a system panic the executor stops spawning new nodes (a shared `AtomicBool`), lets in-flight jobs finish (the tasks spec has no cancellation), then resumes the panic as `SystemPanic`. Successors of the panicked system never run in that run.

### Determinism

**None is required or promised** (INTERVIEW #10). Specifically: the relative order of two systems with no conflict and no ordering is unspecified; entity ids returned by `Commands::spawn` depend on reservation order, which differs between runs under the parallel executor; `par_iter_mut` row order is unspecified; floating-point reductions across chunks may differ. As an implementation property (not a guarantee, tests may rely on it, games must not): two *conflicting* systems always run in plan order, commands apply in plan order, and the single-threaded executor runs the plan exactly in order, so an MVP run is repeatable on one machine. The parallel and single-threaded executors produce the same results for systems that communicate only through the ECS, up to the unspecified items above.

### Unsafe additions

The workspace deny stays. Designated #4 (`system/param.rs`, `system/function.rs`) **invariants** (each module's `//!` lists them):

1. `get_param` and `run_unchecked` are called only with a cell for which **no conflicting access exists for `'w`**. The caller proves it from the plan's conflict edges (the parallel executor) or from owning `&mut World` (the safe `System::run`, which builds `UnsafeWorldCell::from_mut` itself).
2. The cell is **writable (built by `from_mut`) whenever the system's access contains any write**; a read-only cell is passed only to systems whose access is read-only. This is enforced, not only promised: `FunctionSystem::run_unchecked` takes the cell's `is_writable()` and `debug_assert!`s `cell.is_writable() || !access.has_writes()` before any `get_param`.
3. For every param, the `SystemAccess` that `init_state` recorded is exactly what the item returned by `get_param` can touch (the reason the trait is `unsafe` and sealed). `Query` calls only the storage `_with` entry points, whose access matches the query's `FilteredAccess`.
   **Query aliasing contract.** The storage `_with` forms are `&self` and take a `Copy` cell, so safety against two live `&mut` to one row must come from the param's own signatures: `Query::iter`, `get` and `single` take `&self` and are bounded `D: ReadOnlyQueryData` (no `Mut` or `&mut T` item can exist); `Query::iter_mut`, `get_mut`, `single_mut` and `iter_chunks_mut` take **`&mut self`** and return items that borrow the `Query`, so a second `get_mut` (or an `iter` of the same query) while one is live does not compile. `Query` is not `Clone`/`Copy` and never exposes its cell. Compile-fail doctests pin both rules (Test plan).
4. A param item borrows `state` (`&'s mut`) and the cell (`'w`) and cannot outlive the call: the function bound is higher-ranked over `'w` and `'s`, so a system cannot stash a `Query` or `Res` past the call (compile-fail doctest).
5. `Commands` only reads `Entities` (atomic reservation) and mutates its own queue; it never touches components or resources before the sync point.
6. `QueryState` refresh (`update_archetypes`) happens in `get_param` under the same cell, which is sound because archetypes are only created under `&mut World`; the `WorldId` assert from the storage spec applies on every entry.

Any later PR touching these files is high-risk: Miri tests, and the reviewer checks every `// SAFETY:` argument (ADR-0005). `crates/exitium_ecs/CLAUDE.md` must change from "exactly three" to the real count (done by the first issue that adds designated #4).

## MCP / editor surface

Agent parity (rule 1) and reflection as source of truth (rule 2): components, resources and events are addressed by **stable type name** (reflection, #20); values travel as reflected JSON; no hand-written schema.

**This spec files no issue in `exitium_mcp` or `exitium_editor`.** #110 builds the MCP tools generically from reflection and the command registry, and the editor panels from reflection. What this spec owes #110 is a list of what must be exposed and the Rust API to expose it through; both are below and are delivered by `exitium_ecs` issues.

**Reflected types and read API (delivered in the MVP, in `exitium_ecs`):**

- **Resources.** `ReflectResource`, type data registered by `#[reflect(Resource)]` (already in the reflect spec's type-data table): `contains(&World)`, `get(&World) -> Option<&dyn Reflect>`, `get_mut(&mut World) -> Option<ReflectedMut<'_>>` (stamps in `as_reflect_mut` only, the same guard as `ReflectComponent`), `insert(&mut World, &dyn Reflect)`, `remove(&mut World)`. Every game resource that should be visible to an agent or the inspector is `#[derive(Reflect)]` plus `#[reflect(Resource)]`.
- **Events.** `ReflectEvent` carries `send(&mut World, &dyn Reflect) -> Result<EventId, ApplyError>`. The reflect spec has no `#[reflect(Event)]` (it registers only `Default`, `TextForm`, `Component`, `Resource`), so registration is a **hand-callable** `World::register_reflected_event::<E>()` (which calls `registry.register::<E>()`, registers `ReflectEvent` and `register_event::<E>(clock)`'s `Events<E>`); `App::add_event` (app spec) calls it for reflected events. If the reflect spec later adds an attribute, it only calls this function. No reflect-derive change is required now.
- **Introspection structs.** `ScheduleReport`, `SystemReport`, `Ambiguity`, `ResourceInfo`, `EventInfo` and `RunReport` derive `Reflect` and are returned by plain read functions: `World::schedule_labels()`, `World::schedule_report(label)`, `World::resource_infos()`, `World::event_infos()` and the last `RunReport` per label (`Schedules::last_report(label)`). `ScheduleReport { label, order, systems[{ id, name, sets, reads, writes, resource_reads, resource_writes, exclusive, deferred, ordered_after }], sync_points[], ambiguities[{ a, b, conflicting }] }`. Names are `ComponentInfo`/`ResourceInfo` type names; #110 maps them to stable names through the registry.

**Commands the bus must expose (mutation always goes through the command bus, rule 3, applied between schedule runs, each with undo):**

| Command | Semantics | Undo |
|---|---|---|
| `InsertResource { type_name, value }` | `ReflectResource::insert` | previous value (or removal) |
| `RemoveResource { type_name }` | `ReflectResource::remove` | re-insert snapshot |
| `SetResourceField { type_name, path, value }` | path resolved and value parsed first, then `get_mut` + `as_reflect_mut` only to write | previous field value |
| `SendEvent { type_name, value }` | `ReflectEvent::send` | none (an input, flagged non-undoable; the bus spec decides how history shows it) |

The commands live in the crate the command-bus spec assigns (`exitium_command` depends on `exitium_ecs`, never the reverse). Suggested tool names for #110, `<domain>_<verb>`: `ecs_list_schedules`, `ecs_get_schedule`, `ecs_list_resources`, `ecs_get_resource`, `ecs_list_events`, `ecs_insert_resource`, `ecs_remove_resource`, `ecs_set_resource_field`, `ecs_send_event`; `ecs_get_stats` (storage spec) is extended with the last `RunReport` per schedule. Resource writes stamp `world.change_tick()` and flag only when the write is performed (same guard rule as components), so a rejected `SetResourceField` flags nothing.

**Editor surface.** A Systems panel (schedule order, access sets, sync points, ambiguity warnings) and a resource inspector are built by #110 from the reflected structs and `ReflectResource` above; event counters from `EventInfo` go to the profiler panel. There is no editor action that edits a schedule, so there is nothing to mirror (Non-goals).

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
- Parallel executor adds `exitium_ecs::schedule::segment` per segment. `exitium_tasks` emits its own `scope` and `parallel_for` spans (the tasks half of #65, outside this spec).

The profiler's macro takes only a literal name today, so a per-system label needs a **field form** of `profile_scope!` (a small change in `exitium_profiler`, item 1 below). Until it lands, the system span is emitted unlabeled. Span names are fixed literals; per-call data goes in fields, never in the name.

## Test plan

- **Conflict rule:** a matrix of read/read (no conflict), read/write, write/write, `Changed<A>` reader versus `A` writer (conflict), resource read/write combinations, exclusive versus anything, and filter-disjoint pairs (`With<X>` against `Without<X>` do not conflict; `Option<&T>` adds no `with`); the same matrix inside one system (`ParamConflict`); `Query<(&A, &mut A)>` reported by the storage layer.
- **Graph:** `before`/`after`/sets/`chain` ordering respected in the plan; a set with no members is a no-op; cycle error names the systems; `UnknownSystem` and `DuplicateSystem`; node order equals registration order when unconstrained; **sync-point position is pinned** (`SyncPoint(k)` sits immediately before its first-registered successor, `SyncPoint(end)` is last, unrelated systems registered before that successor run before the sync point); sync-point levels (A `Commands` system, B after A: sync between; unrelated deferred systems share one sync point; final sync always present); a conflicting unordered pair is serialized and reported as an ambiguity, `Deny` makes build fail, an explicitly ordered conflicting pair is not reported; adding a system after the first build keeps existing `last_run`s.
- **Commands:** spawn-then-insert on the reserved id within one buffer; despawn of a reserved entity; ordering across two buffers in plan order; commands on a dead entity are skipped and counted; `spawn_batch`; closure command; the buffer never applies mid-run (a later system in the same segment does not see it unless ordered after); no buffer survives the end of a run; a panic abandons every queue: reserved-but-unexecuted spawn ids are flushed and despawned, so a following sync node finds **no** component-less phantom entity (`Query<Entity>` count and `WorldStats` unchanged), while a spawn that already ran is kept.
- **Tick contract (end to end, from the storage spec's hand-offs):** a system that writes through `Query::get_mut` does not see that write as `Changed` on its next run, while an exclusive world-level write is seen; apply a command buffer after system S, run S again, S sees the command's changes exactly once; `(&mut A, Changed<A>)` does not retrigger itself; first run sees everything; a system idle for more than 2^30 ticks (force the counter) sees everything once; sweep polling: a schedule run with `needs_tick_check()` advances the sweep by one slice per sync node, a full sweep completes across runs, and no stored age exceeds the bound (reuses the storage spec's `force_change_tick` hooks).
- **Resources:** missing resource panics naming system and resource; `Option<Res>` yields `None`; insert-after-system-build works (id registered at `initialize`); `is_changed`/`is_added` for insert, overwrite, `ResMut` write, remove-then-insert; `ResMut` that is never dereferenced mutably flags nothing; a type that is both `Component` and `Resource` causes no conflict.
- **Events:** each event seen exactly once by each of several readers; events live two updates and are dropped at the third; a reader that skips two updates reports `missed`; early-dropped iterator leaves the rest; `Frame` and `Fixed` clocks update independently; unregistered event type fails at `initialize`; `EventId`s are monotonic across updates.
- **Executor (MVP):** runs the plan in order, identically on `ThreadBudget::single()`; panic yields `SystemPanic` with the system and schedule names (native); `current_system()` is set during a run; `run_schedule` removes and always reinserts, `AlreadyRunning` on re-entry, nested `run_schedule` of another label works; `run_system_once`.
- **Parallel (post-MVP, #48):** 1,000 runs of disjoint-component mutation give the correct result (race stress); results equal the single-threaded executor for ECS-only communication; panic in one system stops successors and reports the name; designated #5 is Miri-clean; the single-thread fallback runs on the wasm path.
- **Unsafe group #4 (items 6 to 8):** Miri tests: a `Query` and `ResMut` in one system through a writable cell; two systems with disjoint access on two threads through one cell (`std::thread::scope`); a read-only cell with read-only systems only. `compile_fail` doctests: two live `Query::get_mut` items; `iter` while an `iter_mut` item is live; a `Query` or `Res` stashed past the call; a downstream `impl System`; `Query::iter` on a `&mut T` query. A `debug_assert!` test (`should_panic`, debug only) that a write-access system is rejected on a read-only cell.
- **wasm32:** `cargo build --target wasm32-unknown-unknown` for `exitium_ecs`; `cargo xtask ci wasm` also builds it with `profiling` (the `FEATURE_CHECKS` entry comes from the intake PR).
- **Benchmarks:** `benches/scheduler.rs` for every row of the Performance table, with the baseline "call the same function bodies by hand in a loop".

## Implementation issues

Each issue edits exactly one crate (or, for the facade, only the facade hotspot), is about 600 changed lines or fewer, and has a full labelled footprint (`Edits`, `Governs`, `Hotspots`, `New crate edges`; format from `factory-run`) and acceptance criteria. Existing issues are adjusted rather than replaced. `<intake-1>` is the Stage 1 intake issue (the architect fills in its number once it is filed): **every issue below is `Blocked by <intake-1>`**, which adds all workspace members, edges, dependencies and xtask entries listed in "Intake wiring", so no implementation issue touches `Cargo.lock`, `xtask/` or a manifest's dependency list. All `exitium_ecs` issues serialize (one crate has at most one issue in flight), so the order below is the intended order; the storage chain (#38 to #43) is a further `Blocked by` wherever an issue needs it. After this spec merges, the architect files the new issues with `file-issue` into the Jam MVP milestone and removes `needs-spec` from #44, #45, #46, #47 and #48. Every `exitium_ecs` issue also updates `crates/exitium_ecs/CLAUDE.md` where it changes a stated fact (the "exactly three designated modules" line changes with item 6).

**MVP slices**

1. **`feat(profiler): field form of profile_scope!`** (new). Footprint: Edits `crates/exitium_profiler/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`. ~150 lines.
   - AC: `profile_scope!("name", key = expr)` records the field when `profiling` is on and expands to nothing when off; the literal-only form is unchanged; a doc test and a `compile_fail` test (non-literal name); `benches/scope.rs` shows the off-cost within the 5% gate. Replaces the profiler half of #65.
2. **#44 narrowed: resources.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, #40, #41, #43. ~550 lines.
   - AC: `Resource`, `ResourceId`, `Resources` on `Column`, the `World` resource API, `Res`/`ResMut` types, resource ticks as specified, `world/cell.rs` resource accessors; the tick rules (insert, overwrite, `ResMut` write, remove-then-insert) are tested; a Miri test covers the cell accessors; no `unsafe` outside designated #1 and #2.
3. **New: `feat(ecs): double-buffered events`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, item 2. ~400 lines.
   - AC: `Event`, `Events<E>`, `EventCursor`, `EventClock`, `EventRegistry`, `World::register_event`/`update_events`/`send_event`; the lifetime, cursor and `missed` tests of the Test plan; no `unsafe`.
4. **#45 narrowed: `Commands`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, item 2, #41. ~500 lines.
   - AC: `Command`, `CommandQueue`, `Commands`, `EntityCommands`, `ApplyReport`, recorded-order apply, dead-entity skip and count, `World::spawn_reserved` (adjusts #41's surface), `CommandQueue::abandon` with the no-phantom-entity guarantee and its test; no `unsafe`.
5. **New: `feat(ecs): SystemAccess and the conflict rule`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, item 2, #42. ~400 lines.
   - AC: `SystemAccess`, `CombinedAccess`, the three-clause conflict rule including filter-disjointness, `AccessError::ParamConflict`; the full conflict matrix test (also within one system); no `unsafe`. Needs `FilteredAccess::{with, without}` (see adjustments).
6. **#46 part 1: the `System` trait, function systems and `Local`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, item 5. ~550 lines.
   - AC: sealed `System`, sealed `unsafe trait SystemParam`, `IntoSystem` for arity 0 to 8 and for `fn(&mut World)`, `FunctionSystem`, `Local`, `World::run_system_once`; designated #4 invariants in each file's `//!` docs; the `debug_assert!` that a write-access system gets a writable cell; Miri tests for `Local` and a function system; a `compile_fail` doctest that a downstream `impl System` is rejected.
7. **#46 part 2: `Res`, `ResMut` and `Commands` params.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, items 4 and 6. ~350 lines.
   - AC: the three `SystemParam` impls (`Option<Res>`, `Option<ResMut>` included), a missing-resource panic naming system and resource, `has_deferred`, `apply_deferred` forwarding; Miri test of `ResMut` through a writable cell and of `Res` through a read-only cell.
8. **New: `feat(ecs): Query system param`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, item 6. ~500 lines.
   - AC: the `Query` param with the `&self`/`&mut self` split and `ReadOnlyQueryData` bound, run-window routing through the `_with` forms, `update_archetypes` in `get_param`, `single`/`single_mut`/`count`/`is_empty`; `compile_fail` doctests for two live `get_mut` borrows, `iter` while `iter_mut` is live, and stashing a `Query` past the call; the system-level window test (a write through `Query::get_mut` is not seen on the next run); **Miri tests**: a `Query` and `ResMut` in one system through a writable cell, two systems with disjoint access on two threads through one cell (`std::thread::scope`), a read-only cell with read-only systems only.
9. **New: `feat(ecs): EventReader and EventWriter params`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, items 3 and 6. ~250 lines.
   - AC: both params with the per-system `EventCursor` as state, `AccessError::UnregisteredEvent` at `initialize`, `missed()`; tests per the Test plan.
10. **#47 part 1: schedule graph compiler.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, item 5. ~550 lines.
    - AC: `schedule/graph.rs` over plain node descriptors (registration index, access, deferred flag, explicit edges), so it is tested without real systems: Kahn pass with cycle error naming systems and levels, sync-point insertion, the keyed final order, reachability, conflict edges, ambiguities, `Plan` with CSR adjacency and `segments`; the step 5 order is pinned by tests; no `unsafe`.
11. **#47 part 2: `Schedule` builder API.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, items 6 and 10. ~450 lines.
    - AC: `ScheduleLabel`, `SystemSet`, `system()`, `SystemConfig`, `chain`, `Schedule::{new, add_systems, configure_set, build, set_ambiguity_check}`, `ScheduleError` (`Cycle`, `UnknownSystem`, `DuplicateSystem`, `Ambiguous`, `Access`), `ScheduleReport` built from the `Plan`; tests for ordering, sets, errors, add-after-build keeping `last_run`s; no `unsafe`. Drops #47's "named stages" and "parallel batches".
12. **New: `feat(ecs): single-threaded executor, Schedules and run_schedule`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, items 4, 7, 8 and 11. ~500 lines.
    - AC: `SingleThreadedExecutor`, `#[non_exhaustive] ExecutorKind`, the per-system tick sequence with cursor `last_run`, sync nodes with sweep polling (`SWEEP_SLICE_TICKS = 1 << 19`), `Schedules`, `World::run_schedule` (remove, run under `catch_unwind`, always reinsert), `RunReport`, `SystemPanic`, the `current_system()` stack, queue abandonment on panic; the executor tests of the Test plan; no `unsafe`. **The ECS scheduler is usable after this issue.**
13. **New: `feat(ecs): ReflectResource and ReflectEvent`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, items 2 and 3, #50. ~350 lines.
    - AC: both type-data structs, `#[reflect(Resource)]` registration, `World::register_reflected_resource`, hand-callable `World::register_reflected_event::<E>()`, `ReflectedMut` reuse for resources; a rejected write flags nothing.
14. **New: `feat(ecs): scheduler introspection`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, items 12 and 13, #34. ~300 lines.
    - AC: `ScheduleReport`, `SystemReport`, `Ambiguity`, `ResourceInfo`, `EventInfo`, `RunReport` derive `Reflect`; `World::schedule_labels`, `schedule_report`, `resource_infos`, `event_infos`, `Schedules::last_report`; reflection round-trip test.
15. **New: `feat(ecs_derive): derive(Component), derive(Resource), derive(Event)`.** The derive crate is a new proc-macro crate, `crates/exitium_ecs_derive/` (a proc-macro cannot live in `exitium_ecs`), created as a workspace member by the intake PR. Footprint: Edits `crates/exitium_ecs_derive/`; Governs none; Hotspots none; New crate edges none (the crate has no workspace dependency: generated code names `::exitium_ecs::Component` and friends by absolute path, and the `exitium_ecs -> exitium_ecs_derive` edge is added at intake). Blocked by `<intake-1>`. ~350 lines.
    - AC: `#[derive(Component)]` with `#[component(no_change_detection)]` (storage spec), `#[derive(Resource)]`, `#[derive(Event)]`, generics supported, readable compile errors; unit tests on the generated token streams with `proc-macro2` (no ecs dependency); `#[derive(Reflect)]` never implies any of them. This moves `derive(Component)` out of #39, which ships hand impls and a `declare_component!` helper until this lands.
16. **New: `feat(ecs): re-export the derives`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, items 2, 3 and 15. ~100 lines.
    - AC: `pub use exitium_ecs_derive::{Component, Resource, Event}`; integration tests derive each trait on a type, use it in a schedule and check `no_change_detection` rejects `Changed<T>`.
17. **New: `feat(ecs): schedule and event spans`** (MVP-optional: roadmap PR #113 lists executor spans as cut; if that stands, this item is deferred and nothing else depends on it). Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, items 1 and 12. ~250 lines.
    - AC: the spans listed under Performance via `profile_scope!`; the `profiling` feature wiring; a test with a hand-written `tracing::Subscriber` (the `tracing` dev-dependency is in the intake list; no dependency on `StatsLayer`/#63) asserts the span names for a 3-system schedule; off-cost is covered by item 18's bench. Replaces the ECS half of #65.
18. **New: `perf(ecs): scheduler and commands benchmarks`.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by `<intake-1>`, item 12. ~350 lines.
    - AC: `benches/scheduler.rs` (criterion, `harness = false`, declared by intake) for the single-threaded rows of the Performance table, each against the hand-written-loop baseline, within the 5% gate.
19. **New: `feat(exitium): re-export the scheduler API`** (facade, high-risk surface; the facade is a hotspot). Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges `exitium -> exitium_ecs` (added by intake). Blocked by `<intake-1>`, item 12, #84, and item 16. ~150 lines.
    - AC: re-exports of `Schedule`, `system`, `chain`, `ScheduleLabel`, `SystemSet`, `Commands`, `Query`, `Res`, `ResMut`, `Local`, `EventReader`, `EventWriter`, `Resource`, `Event` and the derives; a facade doc test that builds a one-system schedule through `exitium::` paths only.

**Post-MVP**

20. **#48 adjusted: parallel executor.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges `exitium_ecs -> exitium_tasks` (added by the post-MVP intake step before this issue starts). Blocked by item 12, #27, #28, #29 and that intake step. ~600 lines.
    - AC: segments and dataflow counters, per-entry `Mutex`, `run_unchecked` calls, `ExecutorKind::Parallel` with single-thread fallback, `Query::par_iter_mut`, `SystemPanic` through the stack-based context, the Performance rows marked parallel; designated #5 with Miri tests; the 1,000-run race stress.
21. **#52 adjusted: 1M-entity parallel tick benchmark.** Footprint: Edits `crates/exitium_ecs/`; Governs none; Hotspots none; New crate edges none. Blocked by item 20 and #51. Existing issue; adds the parallel scheduler rows. (#67 spans `exitium_ecs` and `exitium_serial` and belongs to the serial spec's issue list; it is not adjusted here.)

**Adjustments to existing issues**

- **#44** is split into items 2 and 3 above (resources, events). **#45** keeps its body but gains `spawn_reserved`, the dead-entity-skip rule and abandonment. **#46** is split into items 6, 7, 8 and 9. **#47** is split into items 10 and 11 and gains the conflict-edge, ambiguity and sync-level rules; it drops "named stages" and "parallel batches". **#48** follows item 20.
- **#49 (hierarchy)** is unchanged except that `despawn_recursive` applies through `Commands` and relies on the dead-entity skip rule.
- **#50** is additionally blocked by item 2 (it implements `Resource` for `AppTypeRegistry`).
- **#65** is split by crate: item 1 (profiler), item 17 (ECS); the tasks half belongs to the tasks spec's issues.
- **#39** no longer ships `derive(Component)`; it ships the hand impl path and `declare_component!` (item 15 delivers the derive).
- **#43** drops "applies the returned `TickSweep` to every per-system `last_run`" and the completion-time `last_run` clamp: this PR amends the storage spec so that `last_run` is a `ChangeCursor` for schedulers as well, and `Tick::clamp_last_run` is no longer needed by the scheduler (the architect edits the #43 body after this spec merges so the implementer does not build the clamp).
- **#42** no longer delivers `par_iter_mut` (it moves to item 20, with the tasks edge).

## Intake wiring

For the Stage 1 intake PR (the union of every merged spec's list), so that no implementation issue touches `Cargo.lock`, a manifest's dependency list or `xtask/`:

- **Workspace members:** new proc-macro crate `crates/exitium_ecs_derive` (name `exitium_ecs_derive`, `publish = false`, workspace lints, handled by xtask exactly like `exitium_reflect_derive`).
- **Workspace crate edges:**
  - `exitium_ecs -> exitium_core`
  - `exitium_ecs -> exitium_reflect`
  - `exitium_ecs -> exitium_profiler` (new; `exitium_ecs` gets `[features] profiling = ["exitium_profiler/profiling"]`, off by default; `exitium_profiler` has no workspace dependency, so there is no cycle)
  - `exitium_ecs -> exitium_ecs_derive`
  - `exitium -> exitium_ecs` (facade; the facade manifest is a hotspot touched by the intake PR only)
- **Not in the MVP:** `exitium_ecs -> exitium_tasks`. It is added by the post-MVP intake step for #48, so the ECS issues and the tasks issues (#26 to #30) never conflict under the factory-run dependency rule. The storage spec's earlier `exitium_tasks` dependency line is amended in this PR to match.
- **crates.io dependencies:** none for `exitium_ecs` at runtime (bitsets, the graph, the hasher and `Commands` are in-crate; `Mutex` and atomics come from std). `exitium_ecs_derive` uses `syn`, `quote` and `proc-macro2` with the same `[workspace.dependencies]` entries the reflect spec already declares (MIT OR Apache-2.0, compile-time only, no wasm output).
- **Dev-dependencies of `exitium_ecs`:** `criterion` (workspace entry, already used by `exitium_profiler`; items 18 and the storage benches #51/#52) with `[[bench]] name = "scheduler", harness = false` and the storage benchmark's entry; `tracing` (workspace entry, features `std`; item 17's hand-written subscriber). Both are dev-only, licensed MIT or Apache-2.0, and justified because the std alternative (a hand-rolled harness and span collector) would be less reviewable.
- **xtask:** a `("exitium_ecs", "profiling")` entry in `FEATURE_CHECKS` (`xtask/src/ci.rs`), so `cargo xtask ci wasm` builds `exitium_ecs` with `profiling` for wasm32 and the span test runs in CI. `exitium_ecs` is not in `NATIVE_ONLY`.
- **Crate graph rule:** `exitium_ecs` stays a runtime crate that builds for wasm32. It must not depend on `exitium_app`, `exitium_command`, `exitium_mcp`, `exitium_editor` or any other runtime crate (platform, render, physics, audio, assets, ui, scene); cross-crate glue lives in the facade.

## Downstream issue adjustments

Adjustments this spec requires of the storage work (#38 to #43); the architect edits the issue bodies after the spec merges:

- **#42 (queries):** expose `FilteredAccess::with()`/`without()` (or `is_disjoint(&FilteredAccess)`); a public `QueryState::update_archetypes(&mut self, &World)`; `&self` forms that assume a refreshed cache (`iter_prepared`/`get_with`/`get_mut_with` taking a window and a world or cell, with a `debug_assert!` that the cache is current), so a `Query` param can iterate more than once through `&self` (the `Query` param still takes `&mut self` for every mutable form, see invariant 3); the `UnsafeWorldCell`-taking entry points the storage spec already names. `par_iter_mut` moves to item 20.
- **#40/#41:** `Column` usable with capacity 1 for resources (a safe `column_info_of::<R>()` like `drop_fn_of`); `World::spawn_reserved`; `world/cell.rs` resource accessors and `UnsafeWorldCell::is_writable()` (item 2 delivers them).
- **#43:** `Tick::from(ChangeCursor)` (already assumed by `DynSince`) is public; see the `last_run` amendment above.
- **#39:** see "Adjustments to existing issues" (derive moved to item 15).

## Open questions

1. **Cursor `last_run`: resolved in this PR.** Per-system `last_run` is a 64-bit `ChangeCursor`, resolved by age on every run, so nothing is clamped. `ecs-storage-queries.md` is amended in this PR ("Where `last_run` lives", the sweep's `last_run` bullet and the #22 hand-off) so the two specs agree. Residual question: confirm the one `resolve_cursor` per system run (two compares) is acceptable against the 120 ns dispatch budget.
2. **Labels and sets as `&'static str` newtypes** (readable, MCP-friendly, collision risk handled by the `crate::name` convention) versus type-based labels. Confirm?
3. **Sync-point rule.** Deferred buffers apply only at sync points that exist because of an explicit ordering edge, plus at the end of a run, in both executors. The alternative (apply after every system on one thread) is friendlier but makes single-threaded and parallel behave differently. Confirm the strict rule?
4. **Event clocks.** `Frame` and `Fixed` are an ECS concept here; the app spec must call `update_events` for both. If the app wants a single clock, drop `EventClock`.
5. **Non-send resources.** wgpu's WebGPU handles may be `!Send` on wasm32 unless a wgpu feature changes that, which could force `NonSend` resources and main-thread-pinned systems. Extension path: a `main_thread` flag on `SystemAccess` (a barrier node in the plan, run on the calling thread) and a thread-id-checked slot in `Resources` (a new designated unsafe module). The render spec decides whether the MVP needs it.
6. **Run conditions and states.** Cut from v1; state gating is by schedule label. Does Glutton's UI flow need `run_if`?
7. **Per-system entity reservation cache.** Needs an `Entities` API to take unused reservations back (storage spec). Only worth it if the `spawn` round-trip budget is missed.
8. **`Box<dyn Command>` per command** versus a byte arena. The box is simple and safe; an arena would need a new designated unsafe module. Perf follow-up if the 60 ns record budget is missed.
9. **Ambiguity default.** `Report` (data only) versus warning at build in debug builds (needs a logging path the ECS does not have). The app could print `ScheduleReport::ambiguities`.
10. **System enable/disable and removal at runtime** (an editor "pause system" feature) is deferred; it would need a tool and a command when added.
11. **Absolute budgets** are provisional; the `perf` agent confirms them after item 18 on the reference runner and the 4-thread min-spec profile.
