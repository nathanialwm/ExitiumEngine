# Spec: ECS storage, entities, queries and change detection

- Issue: #21
- Crates: exitium_ecs (depends on exitium_core, exitium_tasks; reflection hooks per #20)
- Status: draft (revised after review round 2)

## Goal

Define how `exitium_ecs` stores entities and components, how systems read and write them through queries, and how changes are detected, before any code is written. The ECS is the engine's hottest and riskiest code (INTERVIEW R11), so this spec fixes the data layout, the exact places `unsafe` is allowed, the borrow rules and the performance budgets up front. It covers PLAN Phase 1 item 3 for storage, entities, queries and change detection only. The scheduler, `Commands`, resources and events are in #22. Reflection (`Reflect`, `TypeRegistry`, `ReflectComponent`) is in #20 and only referenced here.

Design in one paragraph: entities are 8-byte generational ids (the `exitium_core` `RawHandle` layout). Components are grouped into **archetypes**, one per distinct set of component types. Each archetype is a table with one type-erased, densely packed column per component plus a parallel column of entity ids. Add/remove component moves the entity's row to another archetype along a cached **archetype graph** edge. Queries are tuples of `&T` / `&mut T` / `Option<..>` / `Entity` with `With`/`Without`/`Added`/`Changed` filters, resolved once per query into a cached list of matching archetypes and then iterated column by column. Change detection is a pair of `u32` ticks per tracked component per row, stored in separate interior-mutable arrays so iteration that ignores them never touches that memory.

## Non-goals

Explicitly **not supported in v1** (each can be added later without breaking the layout, see "Extensibility"):

- Sparse-set (non-table) component storage. Every component is table-stored.
- Component lifecycle hooks / observers (`on_add`, `on_remove`) and `RemovedComponents<T>`. Removal is not observable by queries (a removal log can be added later without changing the tick API, see "Extensibility").
- `Or<(..)>` filters, `Has<T>` query data, and user-implemented `QueryData` / `QueryFilter` (the traits are sealed).
- Non-`Send` / non-`Sync` components. (Such state, e.g. a Luau VM handle, lives outside the ECS or as a scheduler-managed non-send resource, see #22.)
- Relations beyond `Parent`/`Children` (no generic entity-to-entity relationship tables).
- Archetype garbage collection (archetypes are never freed; see Risks).
- Cross-world entity transfer, world cloning, and serialization (see #61).
- Scheduling, `Commands`, events, resources (#22) and the Phase 4 command bus.
- Determinism guarantees (INTERVIEW #10: not required). Iteration order is specified below only as far as needed for correctness.

## Design

### Module layout

```
exitium_ecs/src/
  lib.rs
  entity.rs          Entity, Entities (allocator + location table)        safe
  component.rs       Component, ComponentId, ComponentInfo, Components    safe
  tick.rs            Tick, TickWindow, ChangeCursor, wraparound math      safe
  bundle.rs          Bundle trait + tuple impls (macro, arity <= 8)       safe
  change.rs          Mut<T>, change-detection helpers                     safe
  storage/
    column.rs        Column, TickCell: type-erased aligned growable array UNSAFE (designated #1)
                     and its relaxed-atomic tick arrays
    archetype.rs     Archetype, Archetypes, archetype graph               safe
  world/
    mod.rs           World, WorldId: spawn/despawn/insert/remove/get      safe
    cell.rs          UnsafeWorldCell: raw-pointer world access            UNSAFE (designated #2)
  query/
    access.rs        Access / FilteredAccess bitsets, conflict checks     safe
    filter.rs        With, Without, Added, Changed                        safe
    state.rs         QueryState: world-bound ids + matching-archetype cache  safe
    fetch.rs         QueryData impls, row -> &T / Mut<T>, Send/Sync impls UNSAFE (designated #3)
    dynamic.rs       DynamicQuery: runtime-id archetype scan (MCP/editor) safe
```

Exactly three modules may carry `#[allow(unsafe_code)]`; everything else keeps the workspace-wide deny. Macros are limited to one small `macro_rules!` per tuple impl (`Bundle`, `QueryData`, `QueryFilter`), each documented, arity 1 to 8.

### Entities

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Entity { index: u32, generation: NonZeroU32 }   // 8 bytes, Option<Entity> is 8 bytes
```

- Same layout as `exitium_core::RawHandle { index, generation }`, whose generation is `NonZeroU32` (first generation is 1; merged in #81), so `Option<Entity>` has a niche; `Entity` converts to and from it at the boundary.
- **Generation 0 is never a valid entity.** Every conversion from an untrusted `u32`/`u64`/string is fallible: `Entity::from_bits(u64) -> Option<Entity>` returns `None`; parsing `"42v0"` (or any malformed text) returns `Err(InvalidEntity)`, which the MCP layer reports as `invalid_entity`. Both directions between `Entity` and `RawHandle` are infallible, because `RawHandle` already holds a `NonZeroU32` generation.
- `Entity::to_bits() -> u64` / `from_bits` for the binary format. **`to_bits() = (generation << 32) | index`**: the index is the low half and the generation the high half, so `to_bits().to_le_bytes()` equals the little-endian bytes of the reflected structure `{ index: u32, generation: u32 }` that the reflect spec's schema hash describes (index first). This is deliberately **not** `RawHandle::to_bits`, which puts the index in the high half so that packed values order by index; do not compare `Entity::to_bits` values for ordering (`Ord` is the derived index-then-generation order). `from_bits` returns `None` when the high half (the generation) is 0. Wire/debug text form is `"{index}v{generation}"` (e.g. `"42v1"`); JSON/MCP carries it as that string, never as a bare u64 (not safe in JS numbers).
- `Entity` has a **hand-written `Reflect` + `ReflectTextForm` impl in `entity.rs`** (safe code that depends on `exitium_reflect`), not a derive: the real layout has a `NonZeroU32` generation and private fields, and reflection must rebuild it through the checked constructor (generation 0 is rejected by `from_reflect`/`set`/`apply`). See `reflect-model.md`, "Entity".
- `Entities` owns a `Vec<EntityMeta>` indexed by entity index:

```rust
struct EntityMeta { generation: NonZeroU32, location: EntityLocation }   // 12 bytes
struct EntityLocation { archetype: ArchetypeId /*u32*/, row: u32 }
```

  `ArchetypeId::INVALID` (u32::MAX) marks a free or reserved-but-unflushed slot, so a lookup is `meta[index]` then a generation compare: O(1), no hashing.
- **Allocation:** free indices are reused LIFO (cache-friendly) from a `Vec<u32>` free list; `free` bumps the generation. On generation overflow (2^32 - 1 frees of one slot) the slot is **retired**, never reused, never wrapped silently; the count is exposed as `Entities::retired()` and in `WorldStats`. Index space is `u32`, so at most 2^32 - 1 live + retired slots.
- **Reservation for deferred commands (#22):** `Entities::reserve_entity(&self) -> Entity` is lock-free (an atomic cursor over the free list, then an atomic fresh-index counter) so systems running in parallel can hand out ids from `Commands::spawn()` without `&mut World`. `Entities::reserve_entities(&self, n) -> impl Iterator<Item = Entity>` takes `n` ids with one atomic operation per source (free list, fresh counter); `Commands` buffers should reserve in batches because a single shared atomic does not scale across threads (see Performance). Reserved entities have `ArchetypeId::INVALID` until `World::flush_entities(&mut self)` places them in the empty archetype. `contains` is `false` for reserved-unflushed entities; command application flushes first.
- **Despawn ordering** (full rule in "Panic safety"): the row is detached from its archetype, the moved-in entity's location is fixed up, **the despawned slot is marked `INVALID` with its generation bumped and pushed on the free list, and only then do destructors run.** `Entities` is therefore consistent with the columns at every point where user code (a `Drop`) can run.

### Component registry

```rust
pub trait Component: 'static + Send + Sync {
    const TRACK_CHANGES: bool = true;          // see "Change detection: opt-out"
}

pub struct ComponentId(u32);                  // dense, per-process, NOT stable across runs
pub struct ComponentInfo {
    id: ComponentId, name: &'static str,      // std::any::type_name; stable name comes from reflection (#20)
    type_id: TypeId, layout: Layout,
    drop_fn: Option<unsafe fn(*mut u8)>,      // None when !needs_drop
    track_changes: bool,                      // copied from Component::TRACK_CHANGES
    storage: StorageType,                     // single variant `Table` in v1
}
pub struct Components { /* Vec<ComponentInfo> + HashMap<TypeId, ComponentId> */ }
```

- Registration is **on first use, through `&mut Components`** (`World::init_component::<T>()`, called implicitly by `spawn`/`insert`/`query_state`). No locks, no atomics: ids are assigned only where `&mut World` exists. Systems resolve their component ids when the schedule is built or initialised against `&mut World` (#22); deferred commands register at apply time, which also has `&mut World`.
- `ComponentId` is dense so it can index bitsets and `Vec`s. It is **never** put in a file, command payload or MCP message; those use the stable type name from reflection (#20). `Components::id_of(TypeId) -> Option<ComponentId>` lets the dynamic query path (below) turn a reflection-registry type into an id.
- Zero-sized types are supported: their columns store no bytes and only track `len` (marker components cost 0 bytes of data, and 0 bytes of ticks when they set `TRACK_CHANGES = false`).
- The `drop_fn` shim (`unsafe fn drop_in_place_erased::<T>(*mut u8)`) is defined in `storage/column.rs` and passed to the registry by a safe function `column::drop_fn_of::<T>() -> Option<unsafe fn(*mut u8)>`. This keeps `component.rs` free of `unsafe` (clarifies #39: the fn pointer is created in the designated module, called only there).
- `Component` is implemented by hand (`impl Component for T {}`) or with **`#[derive(Component)]`, which `exitium_ecs` provides itself** (a proc-macro crate that reuses the `syn`/`quote` dependencies #20's derive already brings, so it adds no new external dependency; whether it shares a crate with `#[derive(Reflect)]` is left to the implementing issue). `#[derive(Reflect)]` does **not** implement `Component`, and `#[reflect(Component)]` (#20's spelling) only registers the `ReflectComponent` type data for a type that already implements `Component`; a missing impl is a compile error, never an implicit derive. Opting out of change tracking is an ECS concern and lives on the ECS derive, never in `#[reflect(..)]`: `#[derive(Component)] #[component(no_change_detection)] struct Marker;` expands to `impl Component for Marker { const TRACK_CHANGES: bool = false; }`, and by hand it is `const TRACK_CHANGES: bool = false`. The derive is delivered with the registry (#39); until it lands, types opt in by hand or with a one-line `declare_component!` `macro_rules!` helper.

### Archetypes and columns (the data layout)

```rust
pub struct Archetype {
    id: ArchetypeId,                       // dense u32, assigned in creation order, never freed
    components: Box<[ComponentId]>,        // sorted ascending, the archetype's identity; immutable after creation
    set: BitSet,                           // membership test, absent word = not present
    entities: Vec<Entity>,                 // row -> entity, len == every column's len
    columns: Box<[Column]>,                // same order as `components`; never resized
    edges: Edges,                          // archetype graph, see below
}
pub struct Column {                        // designated unsafe module
    data: NonNull<u8>, len: usize, cap: usize,
    info: ColumnInfo,                      // layout, drop_fn, TypeId (copied from ComponentInfo)
    ticks: Option<TickArrays>,             // None when the component opted out of change tracking
}
struct TickArrays { added: Vec<TickCell>, changed: Vec<TickCell> }   // each len == column len
#[repr(transparent)] struct TickCell(AtomicU32);                      // relaxed-atomic cell, safe get/set, see below
```

- **SoA:** one contiguous allocation per component per archetype, plus the entity list and two tick arrays per tracked component. A query that reads `&A` and `&mut B` touches exactly the `A` array and the `B` array (and ticks only if it uses `Added`/`Changed`/`Mut`).
- **Interior mutability of ticks (`TickCell` API).** `Mut<T>` and chunk stamping write ticks through handles derived from a shared `&World`-level cell, so tick storage cannot be plain `Tick`s. `TickCell` is `repr(transparent)` over `AtomicU32` with a **safe** API: `get(&self) -> Tick` (`Ordering::Relaxed` load) and `set(&self, Tick)` (`Ordering::Relaxed` store). A relaxed store compiles to the same plain 4-byte store, and because `AtomicU32` is already `Send + Sync` there is no `unsafe impl Sync` and no safe `&self` setter on a hand-asserted `Sync` type: a shared-reference setter is sound by construction. This keeps `change.rs` (`Mut`) and the public tick getters in safe code. Bulk loops that must vectorize (chunk stamp fill, age sweep, row-filter scans) instead take a raw `*mut u32` range from `AtomicU32::as_ptr` (the arrays stay contiguous), in designated modules only and **only while exclusivity is already established** (the sweep and relocation clamps under `&mut World`; the chunk stamp under the writable cell for that chunk's rows; filter scans under the access-set rule), so atomic and plain accesses to one location never overlap in time. A row filter of a query that also writes the same component (`(&mut A, Changed<A>)`) reads through a per-row raw `*const u32`, never a `&[u32]` over a range the same query writes. Cost: per-row `Mut` stores are scalar relaxed stores (not auto-vectorized); that is already inside the `Mut<T>`-writing budget below. The component data array is a raw allocation (`NonNull<u8>`), whose pointer is stored in the column and copied out without creating a reference to it.
- **Column accessors never yield `&mut Column`.** `&self` accessors return raw base pointers (`NonNull<u8>` for data, `*mut Tick` for each tick array) or `&TickCell`; only `&mut self` methods (push, swap-remove, reserve, row move) can resize or move a column, and those are reachable only from `World` structural operations that hold `&mut World`.
- **Alignment:** the column base is aligned to `max(align_of::<T>(), 16)` so 128-bit SIMD loads (via `wide` / `std::arch`, INTERVIEW #27) on the leading element are aligned. Element alignment is always satisfied because element stride is a multiple of `align_of::<T>()`. ZST columns allocate nothing.
- **Growth:** amortized doubling, `new_cap = max(cap * 2, 4)` (elements), applied to every column of an archetype in lock-step (`Archetype::reserve(additional)` reserves all columns and the entity list). `World::reserve_entities::<Bundle>(n)` and `spawn_batch` pre-size so bulk spawn never reallocates mid-loop. `World::shrink_to_fit()` is explicit; nothing shrinks automatically. Columns are contiguous (never chunked) so a column is always one valid `&[T]`, which `par_iter`, SIMD and zero-copy binary serialization (#57) depend on. Cost: a growth step copies the whole column; the transient peak is old + new for one column (see Memory).
- **Swap-remove:** removing row `r` moves the **last** row of every column and of `entities` into `r`, then pops. The entity that moved has its `EntityLocation.row` updated in `Entities`. Order within an archetype is therefore not insertion order after any removal.
- **Moving between archetypes** (insert/remove component, or `Parent` added): the destination row is allocated in every destination column; components present in both archetypes are moved by byte copy with **no drop** and **their ticks copied unchanged**; the removed component (if any) is moved out into a local; the source row is swap-removed without dropping moved-out elements (`Column::swap_remove_forget` / `move_row_to`); the newly added component gets `added = changed = current tick`; the destination's moved-in `Entity` and the swapped-in source entity both get their locations fixed up; **only then** is the removed component's local dropped (see Panic safety).
- **Insert of a component the entity already has** overwrites in place: the old value is moved out into a local, the new value is written, `changed = current tick` is stamped (not `added`), and **then** the old value is dropped. No archetype change.
- **Archetype graph:**

  ```rust
  struct Edges { add: HashMap<ComponentId, ArchetypeId>, remove: HashMap<ComponentId, ArchetypeId> }
  ```

  The first transition `A --(+C)--> B` computes the sorted component set, finds or creates `B` through `Archetypes::by_set: HashMap<Box<[ComponentId]>, ArchetypeId>`, and caches the edge on both archetypes. Later transitions are a single hash lookup. Integer-keyed maps use a small in-crate multiplicative hasher (no new dependency; HashDoS is irrelevant for engine-internal ids). Archetype 0 is always the empty archetype (entities with no components).
- **Cached-index contract (append-only archetypes, fixed column order).** `Archetypes` only grows; an archetype's `components`, `set` and `columns` slice never change after creation, so `ArchetypeId` and each component's column index are permanent for the life of the world. Only rows and column buffers move. `QueryState` caches `(ArchetypeId, column index)` pairs and `World::bundles` caches column indices on exactly this guarantee; any future archetype GC or column reordering must introduce a world-level epoch that invalidates every cache and is out of scope for v1.
- **Bundles:** `trait Bundle` with tuple impls up to arity 8 (small macro, documented). `World` keeps `bundles: HashMap<TypeId, BundleInfo>` caching, per bundle type, its sorted component ids and the resolved archetype and column indices, so `spawn(bundle)` is one `TypeId` hash plus column pushes. Duplicate component types in one bundle (`(A, A)`) panic at first registration with the component name.
- **Panic safety.** The only operations that run user code while the world is mid-operation are component destructors and the user iterator pulled by `spawn_batch`. Structural operations are ordered **bookkeeping first, destructors last**, and `spawn_batch` completes every row before it pulls the next element:
  1. Detach the row: swap-remove it from every column and from `entities`, moving the removed values *out* into a local `DetachedRow` (a guard type in `column.rs`), so all column lengths already match `entities.len()`.
  2. Fix `Entities`: set the moved-in (previously last) entity's `location.row`; set the despawned entity's meta to `ArchetypeId::INVALID`, bump its generation (or retire the slot) and push the index on the free list.
  3. Only then run destructors, via `DetachedRow::drop`. If one destructor panics, the guard keeps running the remaining destructors of that row and then resumes the panic (the same behaviour as slice drop glue); a second panic during unwinding aborts, as in std.
  
  **Invariant at every user-code call point:** (a) for each live entity `e`, `entities[e.index].location` is `(archetype, row)` with `archetype.entities[row] == e`; (b) every despawned entity's meta is `INVALID` with a newer generation, so `get(dead)` is `None` and can never alias the entity that was swapped into its row; (c) all column and entity-list lengths in an archetype are equal. A panicking `Drop` can therefore leak the remaining destructors of its own row at worst, and never double-drops, never leaves stale locations and never leaves mismatched column lengths. Overwrite-insert, `remove`/archetype move and `World::drop` follow the same ordering (their destructor runs after all bookkeeping; `World::drop` clears archetype by archetype and uses the same guard). Allocation failure aborts (`handle_alloc_error`), it does not unwind. Covered by Miri and `catch_unwind` tests (see Test plan).

  **`spawn_batch` and its iterator.** `iter.next()` (and any `Bundle` construction inside it) is user code and can panic. The loop is: pull one bundle, reserve if needed, write it into every column, push the entity, set its location; the iterator is advanced only between complete rows. Reservation uses `size_hint().0` and only grows capacity, never `len`, before initialization. A panic in `next()` therefore leaves every previously spawned row fully formed and live (they remain in the world, as with `Vec::extend`; their ids are lost with the unreturned `Vec` but `WorldStats` still counts them) and no half-written row, so invariants (a) to (c) hold at the panic.
- **Lookup:** `World::get::<T>(entity)`: `Entities` lookup, generation check, `archetype.column_index(id)` (binary search over the sorted `components`, off the per-row hot path), bounds-checked row. Typed access checks the column's `TypeId` once per call (not per row on slice paths).

### Hierarchy, sparse and rare components

- `Parent(Entity)` and `Children(Vec<Entity>)` (#49) are **ordinary table components**. The storage layer has no special knowledge of them. `World::despawn` never cascades; `despawn_recursive` lives in the hierarchy module and walks `Children` with an explicit stack (no recursion), applying through `Commands` (#22).
- Consequence worth knowing: a root entity has no `Parent`, a child does, so the two populations live in different archetypes, and reparenting from "no parent" to "has parent" is one archetype move (budgeted below).
- Entity ids stored inside components (e.g. `Parent`) are plain `Entity` values. Remapping them on load/instantiate (`MapEntities`) belongs to serialization and scene (#61, Phase 2) and is not designed here. A dangling `Parent` after a parent despawn is the hierarchy module's responsibility (consistent maintenance goes through its helpers, not through storage hooks, since hooks are a non-goal).
- **Rare or frequently toggled components** (a marker added and removed every frame) are the known weak spot of archetype storage: each toggle copies the entity's whole row and fragments entities across more archetypes. v1 guidance: model toggles as a field inside a component (`enum`/`bool`), not as add/remove, and watch `WorldStats.archetype_count` (exposed to agents, see below). `ComponentInfo.storage: StorageType` has the single variant `Table` now so a `SparseSet` variant can be added later (see Extensibility).
- Archetypes are never freed. Worst case count is bounded by the number of distinct component sets the game actually builds; the iteration budget below allows ~1,000 archetypes. A debug warning fires at 4,096 archetypes.

### Queries

```rust
// Reusable form, used by systems (#22 / #46): resolved once, cached, bound to the world that built it.
let mut state: QueryState<(Entity, &Position, &mut Velocity), With<Alive>> = world.query_state();
for (e, pos, mut vel) in state.iter_mut(&mut world) { vel.x += pos.x; }  // &mut T yields Mut<T>, see change detection

// Read-only: shared borrow, only for D: ReadOnlyQueryData (no Mut items can exist).
let mut ro: QueryState<(&Position, &Velocity)> = world.query_state();
for (pos, vel) in ro.iter(&world) { /* ... */ }
let item = ro.get(&world, entity);                        // &self; Result<Item, QueryEntityError>, D: ReadOnlyQueryData; burns no tick
let item = state.get_mut(&mut world, entity);             // &self; mutable form takes &mut World
for (pos, vel) in world.query_ref::<(&Position, &Velocity)>() { /* throwaway state, &World */ }

state.par_iter_mut(&mut world, batch_size, |item| ..);    // exitium_tasks, see Threading
state.iter_chunks_mut(&mut world, |chunk| { let (a, b): (&[A], &mut [B]) = chunk.slices(); .. });  // SoA slices
state.iter_with(&world, TickWindow { last_run, this_run });  // &mut self (refreshes the matched-archetype cache only); explicit change window, see Change detection
```

- **Data (`QueryData`)**: `&T`, `&mut T` (yielding `Mut<T>`), `Option<&T>`, `Option<&mut T>`, `Entity`, and tuples of these up to arity 8. Sealed `unsafe` trait; implementations live in `query/fetch.rs`.
- **`ReadOnlyQueryData`:** a sealed marker `unsafe trait ReadOnlyQueryData: QueryData`, implemented only for `&T`, `Option<&T>`, `Entity` and tuples whose members are all read-only; `&mut T`/`Option<&mut T>` never implement it. Every entry point that takes `&World` (`iter`, `get`, `iter_chunks`, `iter_with`, `world.query_ref`) is bounded `D: ReadOnlyQueryData`; every entry point that can yield `Mut`/`&mut` (`iter_mut`, `get_mut`, `par_iter_mut`, `iter_chunks_mut`, `world.query`) takes `&mut World` or an `UnsafeWorldCell` built from one. A world that is only shared can therefore never produce a mutable reference.
- **Filters (`QueryFilter`)**: `With<T>`, `Without<T>`, `Added<T>`, `Changed<T>`, and tuples (logical AND). `Option<&T>` does not constrain archetype matching; `With`/`Without` read no data. `Added`/`Changed` on a component with `TRACK_CHANGES = false` panic at query creation (`try_` returns `QueryAccessError::UntrackedComponent`).
- **World binding:** each `World` has a `WorldId` (process-unique `u64` from a global atomic counter, never reused; `World` is not `Clone`). `QueryState` stores the `WorldId` it was built against and **every entry point asserts it** (`assert_eq!`, panic message names both ids); `get`/`get_mut` return `Err(QueryEntityError::WorldMismatch)` instead. The cached column indices and archetype ids are only meaningful in the world that produced them, so this check is what makes the safe API sound; it is one `u64` compare per call, not per row, and is a listed invariant of module #3.
- **Matching:** `QueryState` stores the `WorldId`, resolved `ComponentId`s, its `FilteredAccess`, and `matched: Vec<MatchedArchetype { id, column_offset }>` plus a flat `columns: Vec<u32>` (one column index per data term per matched archetype; no `SmallVec`). A world-level `archetype_generation: u32` increments when an archetype is created; each state remembers the last generation it scanned and, on next use, tests **only the new archetypes** (bitset subset test for required ids, disjointness for `Without`). Cost is paid once per archetype per query, not per iteration. The cache is valid because archetypes are append-only with fixed column order (cached-index contract above).
- **Iteration order guarantee:** matched archetypes are visited in ascending `ArchetypeId` (creation order); rows within an archetype in ascending row index. Order is stable **for the duration of one iteration** (no structural change is possible during it) and is explicitly **not stable across structural changes** (swap-remove and archetype moves reorder rows), is not sorted by `Entity`, and parallel iteration visits row ranges in unspecified order. Code that needs an order must sort or use a dedicated component/system.
- **Structural changes during iteration are impossible by construction:** iterators borrow `&World` or `&mut World`, and `spawn`/`despawn`/`insert`/`remove` need `&mut World`. Systems defer them through `Commands` (#22), applied at sync points.
- **Borrow rules (aliasing) - two layers:**
  1. *Within one query:* `QueryState::new` builds `Access` bitsets over `ComponentId`: `reads`, `writes` (component data) and `tick_reads` (the `added`/`changed` arrays read by `Added`/`Changed`). A component that appears in `writes` **and** (`reads` or a second write) is a conflict: `(&A, &mut A)`, `(&mut A, &mut A)`, `(&mut A, Option<&A>)`. `world.query_state()` panics with `"query requests conflicting access to component Position (read and write)"`; `world.try_query_state()` returns `Err(QueryAccessError::Conflict { component, name })` for generic callers that want a `Result` (tests, tooling). **`(&mut A, Changed<A>)` and `(&mut A, Added<A>)` are allowed:** for any row the filter reads `changed[row]` (or `added[row]`) before that row's item is yielded and `Mut<A>` writes `changed[row]` afterwards, on the same row and thread, and no other row's tick is touched, so the two never alias. This keeps the common "react to A's change and write A back" binding idiom expressible in one query. `tick_reads` is therefore not counted against the same query's own `writes`, but it **is** counted as a read of the component for the scheduler-level check below. Runtime/MCP callers do not use `QueryState` at all (see the dynamic path in the MCP section).
  2. *Between queries/systems:* with `&mut World` Rust's borrow checker guarantees exclusivity. Under a shared world cell (parallel systems), the scheduler compares systems' `FilteredAccess` sets (with `tick_reads` merged into `reads`, so a `Changed<A>` reader never runs beside an `A` writer) and never runs conflicting ones together (#22). `QueryState` exposes `access() -> &FilteredAccess` for that purpose; whether two queries with provably archetype-disjoint filters (`With<X>` vs `Without<X>`) may overlap is decided in #22, the data model here supports it.
- **Row filters and chunks:** `iter_chunks[_mut]` is only offered when the filter tuple contains no row-level filter (`Added`/`Changed`), because those make the surviving rows non-contiguous; `With`/`Without` are archetype-level and fine. Enforced by a `QueryFilter::ARCHETYPE_ONLY` associated const and a `where` bound (compile-fail doctest).
- **Single-entity `get`/`get_mut`:** location lookup, generation check, then the matched-archetype test against the query's required/excluded sets; errors are `NoSuchEntity`, `QueryDoesNotMatch`, `WorldMismatch`. Row-level filters apply too (`get` of an unchanged entity is `QueryDoesNotMatch`), evaluated against the read-only window described under "Where `last_run` lives".
- **Hot-loop shape:** per matched archetype, `QueryData::init_fetch` resolves raw column base pointers (and tick base pointers, only if the query uses them) once; the row loop is then `for row in 0..len { fetch(row) }` with no hash lookups, no bounds checks beyond the one `len`, and no dynamic dispatch. `for_each`-style internal iteration is provided because a nested loop optimizes better than a flattened iterator. `iter_chunks[_mut]` exposes the per-archetype typed slices directly so inner loops can autovectorize. A `Fetch` is a `Copy` bundle of raw pointers with `unsafe impl Send + Sync` (module #3), justified by `Component: Send + Sync` plus the disjoint-row rule in Threading.

### Change detection

```rust
#[derive(Copy, Clone, Eq, PartialEq)] pub struct Tick(u32);          // per-row stamp, wrapping
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)] pub struct ChangeCursor(u64);   // world counter that never wraps
#[derive(Copy, Clone)] pub struct TickWindow { pub last_run: Option<Tick>, pub this_run: Tick }
```

**Counter and stamping rules (the whole contract):**

- The world keeps one monotonic `u64` counter; `World::change_tick() -> Tick` is its low 32 bits (the **current tick**), `World::change_cursor() -> ChangeCursor` is the full value. It starts at 1. **Invariant: no stored stamp is ever greater than the counter** (every stamp is either a run's `this_run`, which is below the counter once its increment has happened, or the counter itself).
- `World::increment_change_cursor(&self) -> ChangeCursor` is atomic (`fetch_add(1)`, so parallel systems can call it under a shared cell) and **returns the value from before the increment**; `increment_change_tick(&self) -> Tick` is the same operation returning the low 32 bits. That value is the caller's `this_run`; after the call the current tick is `this_run + 1`. Consequently the current tick is always strictly greater than the `this_run` of every run that has already started.
- **Every run starts by incrementing.** A system run (scheduler, #22), a standalone `QueryState::iter*`/`par_iter_mut`/`iter_chunks*` call, and the first page of an MCP query session each call `increment_change_cursor()` (or `_tick()`) once to obtain `this_run`, and afterwards remember `this_run` as their next `last_run`. `get`/`get_mut` are **not** runs: they burn no tick and never touch `last_run` (see "Where `last_run` lives").
- **Stamping:** a write made inside a run (`Mut<T>`, chunk stamp) stamps that run's `this_run`. A write made outside any run at world level (`World::spawn`/`insert`, a write through the `Mut` that `World::get_mut` returns, command application at a sync point, command-bus and MCP edits) stamps the **current tick** `world.change_tick()`, i.e. at least `this_run + 1` of every run that already started. So a world-level write can never carry a tick equal to a previous run's `this_run`, and that run's next `last_run` (`== its this_run`) is strictly older than the stamp: the change is seen. The next run to start has `this_run` equal to (or beyond) the stamp, and its `last_run` is strictly older. Required test: apply a command buffer after system S, then run S again and assert it sees the command's changes (and one with no `increment` between a standalone `iter`, a `spawn`, and the next `iter`).
- **Own writes:** a run's direct writes carry its own `this_run`, and its next `last_run` is that same value, so a system does not see its own direct writes as `Changed` on its next run (this is what makes `(&mut A, Changed<A>)` not retrigger itself). Everything else is seen exactly once: other systems' writes, and the system's own deferred `Commands` (stamped at the sync point, after its run).
- **Flag on write, on every path.** Every mutable access to a tracked component has `Mut<T>` semantics: it stamps only when the mutable reference is actually taken (`DerefMut`), never when the handle is created. `World::get_mut` returns `Mut<'_, T>` whose `this_run` is `world.change_tick()` (it is not a run), and flags nothing if never dereferenced mutably, exactly like a query item. The reflected path (#50) is built on it: `ReflectComponent::get_mut` returns a `ReflectedMut` wrapper around that `Mut<T>` whose `as_reflect_mut(&mut self) -> &mut dyn Reflect` performs the stamp at that call. The `SetField` command (Phase 4) resolves the path and parses and type-checks the new value first and calls `as_reflect_mut` only to perform the write, so a rejected or failed command flags nothing; a write of an equal value still flags (there is no equality check). Read-only reflection (`get`) never flags. The matching wording in `reflect-model.md` uses the same guard name and stamping point (aligned by #78).
- **Spawn/insert:** spawn writes `added = changed = stamp` for every tracked component of the row. Insert of a new component on an existing entity does the same for that component (the moved components keep their ticks). Insert over an existing component and every `Mut<T>` write set `changed = stamp` (not `added`).
- **Per-row storage:** `added[row]`, `changed[row]` per tracked column (8 bytes per component per entity), separate from the data array, stored as relaxed-atomic `TickCell`s.
- **Opt-out (decided in this spec):** `Component::TRACK_CHANGES = false` (default `true`; spelled `#[component(no_change_detection)]` on the ECS `#[derive(Component)]`, never inside `#[reflect(..)]`) gives a column with `ticks: None`: no tick memory, no stamping, `Added`/`Changed` on it rejected at query creation. `Mut<T>` holds an optional tick handle (`None` for untracked); `DerefMut` stores only when it is `Some`, a per-archetype-invariant branch the optimizer unswitches out of the row loop. `Mut::is_changed()`/`is_added()` on an untracked component conservatively return `true`. This changes the `Column` layout and `Mut<T>`, so it is fixed now rather than retrofitted. Memory figures below assume the default (tracked); ZST markers and large write-once components should opt out.
- **`Mut<'w, T>`:** the item type for `&mut T` and `Option<&mut T>`. It holds `&'w mut T`, `Option<&'w TickCell>` (the row's `changed` slot) and `this_run`. `Deref` is free. `DerefMut` stores `this_run` into the slot (one relaxed 4-byte store through `TickCell::set`) and returns `&mut T`. **A `Mut<T>` that is never dereferenced mutably flags nothing.** `Mut::bypass_change_detection(&mut self) -> &mut T` skips the store for hot paths that knowingly don't change data. `Mut::is_changed()` / `is_added()` are available on query items.
- **Chunk API:** `iter_chunks_mut` yields raw `&mut [T]` slices over the chunk's rows. Obtaining a mutable chunk slice conservatively stamps `changed = this_run` for **all rows of the chunk** up front (a vectorizable `fill` of a `u32` range, about 0.25 ns/row); `chunk.slices_untracked()` skips that. This is the documented price of raw mutable slices; its cost is budgeted separately below.
- **Filters and windows:** `Added<T>` is true iff `added[row]` is newer than `window.last_run`; `Changed<T>` iff `changed[row]` is newer. Newer means: `last_run == None` → always true (**"never ran" is an explicit state, not a magic tick value**, so the first run sees everything no matter how old the world is); otherwise `is_newer_than(stamp, last_run, this_run) = this_run.wrapping_sub(stamp) < this_run.wrapping_sub(last_run)`. Because spawn/insert stamp both, `Added<T>` implies `Changed<T>`. Row-level evaluation: one `u32` load and compare per row on a dedicated tick array (the `None` case is hoisted out of the loop), no archetype-level skipping in v1 (see Open Questions for chunk-level summaries).
- **Where `last_run` lives:** the scheduler stores `Option<Tick>` per system (`None` until its first run) and builds the `TickWindow` for each run (#22). **A standalone `QueryState` owns its own `last_run: Option<ChangeCursor>`, initially `None`** (a 64-bit cursor, so the age sweep never has to find or clamp standalone states). `iter`/`iter_mut`/`par_iter_mut`/`iter_chunks*` take `&mut self` (they refresh the matched-archetype cache), obtain `this_run` from `increment_change_cursor()`, resolve `last_run` with `resolve_cursor` (an expired cursor, older than 2^30, is treated as `None`: everything is seen once, a false positive only), evaluate against it, then set `last_run = Some(this_run)` (the window is consumed when the call is made, not when the iterator is drained). **`get`/`get_mut` are not runs (`&self`):** they evaluate row filters against `TickWindow { last_run: <the state's, resolved, read-only>, this_run: world.change_tick() }`, never increment the counter and never write `last_run`. A loop of a million `get`s therefore burns no ticks, and a `get` between two `iter` calls cannot swallow changes the second `iter` must see. A `get_mut` write stamps `world.change_tick()` like any world-level write. `iter_with(&mut self, world, TickWindow)` (and a `_with` form of every entry point) takes an **arbitrary caller-owned window**; the `&mut self` is needed only to refresh the matched-archetype cache, and it touches neither `last_run` nor the counter. This is the primitive for retry-after-error, replication and MCP. The throwaway `world.query`/`query_ref` convenience has a fresh state, so `Added`/`Changed` there see everything; use a `QueryState` or `iter_with` for incremental use.
- **External consumers (MCP `changed_since`, per-client replication acks, INTERVIEW #21):** hold a `ChangeCursor` (u64, carried in JSON as a string). **The only valid cursor is the value returned by `World::increment_change_cursor()` at the start of the read, handed back to the consumer as its next `changed_since`.** A plain `change_cursor()` read is diagnostic only (it appears in `ecs_get_stats`) and must never be stored as a cursor.
  - *Why it is exact.* Let the read's increment return `c`. Every stamp that exists at that moment is `<= c` (counter invariant), and the read's `this_run` is `c`, so the read sees all of them. The counter is now `c + 1` and monotonic, so every later write, world-level (`change_tick()`) or in a later run (`this_run >= c + 1`), stamps `>= c + 1` and can never equal `c` (as 32-bit ticks it would need a full 2^32 wrap, excluded by the 2^30 maximum cursor age). The next read with `last_run = c` therefore sees every later write and none of the earlier ones. A cursor taken *without* incrementing would equal the stamp of the next world-level write and lose it; that is the round-1 bug this rule closes.
  - `World::resolve_cursor(c) -> Result<Tick, CursorExpired>` returns the low 32 bits if `c <= change_cursor()` and its age `change_cursor() - c` is at most `Tick::MAX_AGE` (2^30); otherwise `CursorExpired` (a cursor from the future, e.g. minted by another world, is expired too), on which the consumer discards incremental state and does a full resync (MCP: `cursor_expired`; replication: a full snapshot). Because the cursor is 64-bit and the world only ever compares it by age, it cannot alias after a wrap, and no registration is needed: the **documented maximum age of an externally held cursor is 2^30 ticks** (about 24 h at 200 systems x 60 Hz), checked on every use.
  - A read builds `TickWindow { last_run: Some(resolve_cursor(prev)?), this_run: Tick::from(c) }` and passes it inside `DynamicQuerySpec` (below) or to `iter_with`. A paged read takes **one increment for the whole session**: the first page increments and returns `c`; every later page presents `c` back (`as_of`) and reuses it as `this_run`, so all pages evaluate one window and a write landing mid-session is simply seen by the next session.
  - Cursors younger than 2^30 are exact: rows they care about have age at most the cursor's age and are never clamped by the sweep (a row is clamped only when its age exceeds 2^30 relative to a sweep `ref`, which would put it older than any valid cursor's `last_run`).
- **Semantics:**
  - "Seen once": a system that runs every tick sees a change in the run after it happened, then not again, because its `last_run` advances.
  - A run with `last_run == None` (the first run) sees every tracked row as added/changed; this is intended (initial UI binding).
  - Despawn+respawn of an entity gives fresh ticks.
  - Component **removal is not observable** (non-goal): a system can notice absence only by `Without<T>`.
- **Tick wraparound and the age sweep:** ticks are `u32` with wrapping arithmetic, correct while every stored age (current tick minus stamp) is below 2^31. The sweep uses a **signed age**: `age(t, ref) = ref.wrapping_sub(t) as i32`, so a stamp made after `ref` has a *negative* age and a stamp at or before `ref` has a non-negative one. The world keeps the bound true with an **incremental age sweep**:
  - A sweep *starts* once the counter has advanced 2^29 since the previous sweep started (`World::needs_tick_check()`); the scheduler polls it at every sync point. It fixes `ref` = current tick at start. `World::check_change_ticks(&mut self, budget_ticks: usize) -> Option<TickSweep>` clamps at most `budget_ticks` stored ticks per call and returns `Some(TickSweep { reference: ref })` when the sweep completes. Its cursor is `(archetype id, column, tick array, offset)` over contiguous `Tick` ranges (branch-free select, so it vectorizes), and it walks the **live** archetype list, so archetypes created mid-sweep are visited too.
  - **Row clamp rule:** a stored tick `t` with `age(t, ref) > 2^30` is set to `ref - 2^30`. Every other tick, **including every tick with a negative age (written after `ref`), is left untouched.** A write made after `ref` therefore is never clamped, wherever the cursor is, and a system that ran after `ref` is never made to miss it.
  - **Rows relocated across the cursor.** While a sweep is active, every operation that copies a row's ticks to another position passes each moved tick through the same clamp **unconditionally**: the swap-remove tail move and the archetype move ("moved components keep their ticks"). Clamping is idempotent, so no comparison with the cursor is needed, and a not-yet-swept row cannot escape into the already-swept region or into an archetype behind the cursor. Cost: two ticks per tracked component per moved row, paid only while a sweep is active (one world-level `sweep_active` test in structural operations that already cost >= 80 ns). If the archetype under the cursor shrinks, the cursor offset is clamped to its new length. So every row that existed at `ref` is visited in place, clamped when moved, or despawned, and rows created after `ref` carry negative-age stamps that need nothing: **when the sweep completes no stored tick has an age above 2^30 relative to `ref`.**
  - **`last_run`s (`Some` values) are clamped only on completion**, using `Tick::clamp_last_run(&mut Option<Tick>, sweep)` with the same signed rule: `age(l, ref) > 2^30` → `ref - 2^30 - 1`, strictly older than every clamped row. A `last_run` set during the sweep (any system that ran after `ref`) has a non-positive age, so it is untouched and the system does not re-see everything. Clamping idle `last_run`s earlier than the rows would make an old-but-real change look older than the clamped `last_run` (a false negative); at completion every clamp error is a false positive: a system idle for more than 2^30 ticks sees some already-seen rows as changed once. It never misses a change: a row is clamped only when its age exceeds 2^30, so a `last_run` that is not itself clamped (age <= 2^30) is at least as new as the clamped row and still treats it as unchanged, and a clamped `last_run` is older than every clamped row and treats them as changed.
  - Bound check: a sweep must finish within 2^27 ticks of starting (at a slice of about 1 ms per sync point it takes tens of sync points; if the bound is hit, the next call ignores the budget and finishes, an exceptional hitch). After a sweep every tick has age <= 2^30 relative to its `ref`; the next sweep starts 2^29 later and completes within 2^27 more, so the maximum stored age is below 2^30 + 2^29 + 2^27 + one schedule run, under 2^31. The relocation clamp is what keeps this bound true for rows that move during a sweep.
  - `None` windows never need clamping, which is why a never-run system can never be stranded by the sweep. Standalone `QueryState`s hold 64-bit cursors and need no clamping either.
- **Cost per write (budget summary):** a tracked write is one 4-byte store; an untouched mutable access costs nothing beyond constructing the `Mut` (optimized away when inlined). Memory is 8 bytes/tracked component/entity.

### Threading model

- `World: Send + Sync`. Shared `&World` reads are always safe. Structural mutation (spawn, despawn, insert, remove, `init_component`, archetype creation, anything that can reallocate a column) needs `&mut World` and therefore happens only at sync points when no query is live. **Columns never reallocate during a parallel phase.**
- `&mut T` writes are plain stores and `Mut` tick stores are relaxed atomic stores. This is sound because a writer holds exclusive access to that column's access-set entry (`writes`); `Changed`/`Added` readers count as `reads` of the same component at the scheduler level and so cannot run concurrently with a writer; within one query the same-row rule above applies. The tick storage is `TickCell` (`AtomicU32`), so stores through shared handles are data-race free by construction, and the plain bulk-range paths run only under established exclusivity (see the `TickCell` API above).
- `Entities::reserve_entity(&self)` and `World::increment_change_cursor(&self)` / `increment_change_tick(&self)` are the only shared-state mutations through `&self` (atomics), plus the relaxed `TickCell` stores of `Mut`; everything else needs `&mut`.
- **`UnsafeWorldCell<'w>`** (designated module #2) is a `Copy` handle used by queries and by the parallel executor (#22). It stores a raw `*const World` (read-only) or a `*mut World` derived from an `&mut World` (writable), with two **separate constructors**: `UnsafeWorldCell::from_ref(&'w World)` yields a **read-only** cell whose accessors only expose `&Archetypes`/`&Archetype` and `*const` data and tick pointers; `UnsafeWorldCell::from_mut(&'w mut World)` yields the only **writable** cell. The mutable-pointer accessors are `unsafe fn`, `debug_assert!` that the cell was built by `from_mut`, and are unreachable from safe code through a `&World` because of the `ReadOnlyQueryData` bound. **No accessor returns `&mut Column`, `&mut Archetype` or `&mut [T]` over a whole column.** They return `&'w Archetype` (metadata and tick cells only), raw `NonNull<u8>` data base pointers and `*mut Tick` tick base pointers. Their contract: "no conflicting access exists for `'w`", discharged by the access-set check (within a query) and the scheduler's conflict detection (between systems); and **no `&mut Column`/`&mut Archetype`/`&mut World` exists while any cell-derived pointer is live** (structural methods need `&mut World`, which cannot coexist with the cell's lifetime).
- **Parallel iteration:** `par_iter_mut` / `par_for_each` splits matched archetypes into disjoint row ranges of `batch_size` and runs them through `exitium_tasks::parallel_for` (#19, #28). The `Fetch` for each matched archetype (raw base pointers) is built once on the calling thread and copied into the tasks. **Each task materializes references only for its own rows**: item `&mut T` is `&mut *base.add(row)`, `&TickCell` is `&*tick_base.add(row)`, and a chunk slice is `slice::from_raw_parts_mut(base.add(start), len)` over `[start, start + len)` only. No task ever holds a `&mut Column` or a mutable slice over the whole column, so disjoint row ranges give disjoint references and overlapping `&mut` are never created. Default `batch_size = clamp(total_rows / (4 x threads), 256, 16_384)`; the caller may override. On wasm32 the tasks crate falls back to sequential execution, so the same call is valid with no `cfg` in ECS code. Min-spec (4 threads) is a benchmark profile (#52, #66).
- Because `Component: Send + Sync`, column memory may be accessed from any thread. `Column` gets `unsafe impl Send/Sync` (`TickCell` is an `AtomicU32` and is `Send + Sync` automatically), and the raw-pointer `Fetch` types of every `QueryData` get `unsafe impl Send/Sync` (raw pointers are `!Send`, so `parallel_for` would not compile otherwise); all are justified by that bound plus the disjoint-row rule and listed in the unsafe inventory.

### `unsafe` inventory and Miri plan

R11 rule applied: three designated modules, no `unsafe` anywhere else in `exitium_ecs` (`query/state.rs`, `query/dynamic.rs`, `world/mod.rs`, `archetype.rs`, `bundle.rs`, `change.rs`, `entity.rs`, `component.rs` stay safe; the workspace deny stays). Every `unsafe` block carries `// SAFETY:`; each module's `//!` docs list its invariants. Any later PR touching these modules is labelled `gate:human`.

| # | Module | Unsafe operations | Invariants the module must uphold |
|---|---|---|---|
| 1 | `storage/column.rs` | `std::alloc::{alloc, realloc, dealloc}`; `ptr::copy_nonoverlapping`/`copy` for push, swap-remove, cross-column row move; `ptr::drop_in_place` via erased `drop_fn`; `slice::from_raw_parts[_mut]` for typed views and tick range fills; `AtomicU32::as_ptr` plain `*mut u32` ranges for the bulk tick loops (age sweep, relocation clamp, chunk stamp fill); `unsafe impl Send/Sync for Column`; the `drop_in_place_erased::<T>` shim; the `DetachedRow` destructor guard | `data` valid for `cap * size` bytes at the stored alignment; elements `0..len` initialized, `len..cap` not; each tick array has `len == column len`; `TypeId`/layout checked on every typed entry point; ZST path never allocates and uses `NonNull::dangling` aligned; a row is detached (len updated, values moved into `DetachedRow`) before any destructor runs; the row move/forget API never double-drops; tick storage is written only through `TickCell::set` (relaxed atomic) or, for bulk ranges, through plain pointers taken only while exclusivity is established (sweep and relocation clamps under `&mut World`), so atomic and plain accesses never overlap in time; **no `&mut Column` exists while any cell-derived pointer into it is live**; **archetypes are append-only and a column's index and archetype's column order are fixed for the life of the world** |
| 2 | `world/cell.rs` | Constructing `UnsafeWorldCell` (`from_ref(&World)` read-only, `from_mut(&mut World)` writable); `unsafe fn` accessors returning `&'w Archetype` and raw data/tick base pointers (never `&mut Column`); `unsafe impl Send/Sync` | Lifetime `'w` never outlives the borrow it came from; the writable constructor takes only `&mut World` and the read-only one can never yield a mutable pointer (`debug_assert!` plus the `ReadOnlyQueryData` bound); every mutable pointer used has no overlapping access, proven by the caller via the access set (documented contract on each fn); no structural change while a cell-derived pointer is live; the world's `WorldId` is readable from the cell so entry points can assert it |
| 3 | `query/fetch.rs` | Raw column pointer to `&'w T` / `&'w mut T` per row (`ptr.add(row)`); `&'w TickCell` per row for `Mut<T>` and tick filters; `slice::from_raw_parts_mut` for chunk row ranges; sealed `unsafe trait QueryData` / `ReadOnlyQueryData`; **`unsafe impl Send/Sync` for each `QueryData::Fetch` and `QueryFilter` state type**; the `unsafe` tick-range stamp for `iter_chunks_mut` | Item for (archetype, column, row) is yielded at most once per live iteration, and a parallel task only touches rows in its own range; `row < len` (the archetype's `len`, read once); the `Access` reported by each `QueryData` is exactly what its `fetch` touches (this is why the trait is `unsafe` and sealed); **the `QueryState` the pointers came from was built on this world (`WorldId` asserted by `state.rs` before any `init_fetch`)**; `ReadOnlyQueryData` is implemented only by types whose items are shared references or `Copy`; `Option<&mut T>` yields `None` without touching memory when the column is absent; a mutable whole-column slice exists only in sequential iteration under an exclusive cell |

**Miri plan.** `cargo xtask ci miri` already runs `cargo +nightly miri test -p exitium_ecs` once the crate contains `allow(unsafe_code)`, i.e. the **whole crate's** tests run under Miri. Therefore:

- Tests are written Miri-friendly: heavy loops and 1M-entity cases are `#[cfg(not(miri))]` or scale down under `cfg(miri)`.
- A dedicated `miri_*` test group per module covers: push/grow/swap-remove/drop with `Drop`-counting types; ZST columns; over-aligned types (`#[repr(align(64))]`); cross-column row move with no double drop; **panicking `Drop` during despawn, remove, overwrite-insert and world drop, caught with `catch_unwind`, asserting afterwards that locations are consistent, the dead entity is gone and no other entity's components changed**; `Mut` write (data and `TickCell`) through a pointer obtained from a writable `UnsafeWorldCell`; two disjoint `par` row ranges of the **same column** on two threads writing data and ticks (the tasks crate must itself be Miri-runnable; if not, use `std::thread::scope` in the test); `Option<&mut T>` on absent columns; tick base pointers from a `&World`-derived cell used only for reads.
- Miri runs with the default Stacked Borrows model in CI. **Recommended follow-up (xtask change, gated, not part of the ECS PRs):** also run `MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"` on this crate, since the column code is raw-pointer heavy and tree-borrows is the model Rust is converging on. Listed in Open Questions.
- Fuzz-style property test (safe, runs in normal CI too): a random op sequence (`spawn`, `despawn`, `insert`, `remove`, overwrite) against a model `HashMap<Entity, HashMap<TypeId, Box<dyn Any>>>`, asserting after every op that entity locations, column lengths and `get::<T>` agree. This validates the swap-remove/location bookkeeping that Miri cannot see.

### Public API sketch (v1 surface)

```rust
// exitium_ecs
impl World {
    pub fn new() -> World;
    pub fn id(&self) -> WorldId;
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity;
    pub fn spawn_batch<B: Bundle>(&mut self, iter: impl IntoIterator<Item = B>) -> Vec<Entity>;
    pub fn despawn(&mut self, entity: Entity) -> bool;
    pub fn insert<C: Component>(&mut self, entity: Entity, value: C) -> Result<(), NoSuchEntity>;
    pub fn remove<C: Component>(&mut self, entity: Entity) -> Result<Option<C>, NoSuchEntity>;
    pub fn get<C: Component>(&self, entity: Entity) -> Option<&C>;
    pub fn get_mut<C: Component>(&mut self, entity: Entity) -> Option<Mut<'_, C>>;   // stamps world.change_tick() on DerefMut only
    pub fn contains(&self, entity: Entity) -> bool;
    pub fn reserve_entities<B: Bundle>(&mut self, additional: usize);
    pub fn shrink_to_fit(&mut self);

    pub fn query<D: QueryData>(&mut self) -> QueryIter<'_, D, ()>;          // convenience, panics on conflict
    pub fn query_filtered<D: QueryData, F: QueryFilter>(&mut self) -> QueryIter<'_, D, F>;
    pub fn query_ref<D: ReadOnlyQueryData>(&self) -> QueryIter<'_, D, ()>;
    pub fn query_state<D: QueryData, F: QueryFilter>(&mut self) -> QueryState<D, F>;
    pub fn try_query_state<D: QueryData, F: QueryFilter>(&mut self) -> Result<QueryState<D, F>, QueryAccessError>;

    // Change detection (see the contract above)
    pub fn change_tick(&self) -> Tick;                                      // current tick: stamped by world-level writes
    pub fn increment_change_tick(&self) -> Tick;                            // atomic; returns the PRE-increment value = this_run
    pub fn increment_change_cursor(&self) -> ChangeCursor;                  // same atomic op, full 64 bits, PRE-increment value: the ONLY valid external cursor
    pub fn change_cursor(&self) -> ChangeCursor;                            // diagnostic read (stats); NOT a valid changed_since cursor
    pub fn resolve_cursor(&self, c: ChangeCursor) -> Result<Tick, CursorExpired>;
    pub fn needs_tick_check(&self) -> bool;
    pub fn check_change_ticks(&mut self, budget_ticks: usize) -> Option<TickSweep>;

    // Introspection: what MCP and the editor read (see next section)
    pub fn stats(&self) -> WorldStats;
    pub fn components(&self) -> &Components;
    pub fn archetypes(&self) -> &Archetypes;
    pub fn entity(&self, e: Entity) -> Option<EntityRef<'_>>;               // EntityRef::component_ids(), component_ticks(id)
    pub fn query_dynamic(&self, spec: &DynamicQuerySpec, after: Option<u32>, limit: usize) -> DynamicPage;
}

impl Archetype { pub fn entities(&self) -> &[Entity]; pub fn component_ids(&self) -> &[ComponentId];
                 pub fn added_tick(&self, id: ComponentId, row: usize) -> Option<Tick>;     // None if absent or untracked
                 pub fn changed_tick(&self, id: ComponentId, row: usize) -> Option<Tick>; }
```

`World::query` takes `&mut World` because a `&mut T` item may be requested; `query_ref` takes `&self` and requires `D: ReadOnlyQueryData`. The `exitium` facade re-export of this API is a separate, `gate:human` PR (public API of the facade).

## MCP / editor surface

Agent parity (rule 1) and reflection as source of truth (rule 2): the ECS exposes no hand-written schema. Components are addressed by **stable type name** (from reflection, see #20), never `ComponentId`; values travel as reflected JSON; entities travel as the `"{index}v{generation}"` string; change cursors travel as a decimal string (`ChangeCursor`).

**Phase 1 deliverable (this spec's scope):** the Rust-level introspection API above (`stats`, `components`, `archetypes`, `Archetype::entities`/tick getters, `entity`/`EntityRef::component_ids`/`component_ticks`, `increment_change_cursor`/`resolve_cursor`, `query_dynamic`), plus the dynamic per-component access in #50 (`ReflectComponent`: `get`, `get_mut` (flags on actual write, see Change detection), `insert`, `remove`, enumerate-all-components as `&dyn Reflect`; hook defined in #20). These are what every MCP tool and editor panel below is built on. No `exitium_mcp` code is written in Phase 1.

**Dynamic query path (why it exists).** `QueryState<D, F>` is generic over Rust types and cannot be built from runtime type names, so MCP and editor panels use `DynamicQuery` in `query/dynamic.rs` (safe code only; it reads through the safe `&World` accessors above and never creates `&mut`):

```rust
pub struct DynamicQuerySpec {
    pub with: Vec<ComponentId>, pub without: Vec<ComponentId>,       // resolved from stable names via #20 + Components::id_of
    pub since: Option<DynSince>,                                     // None = no tick filter
}
pub enum DynSince { Changed(TickWindow), Added(TickWindow) }         // full window: last_run = resolve_cursor(prev), this_run = the session's increment_change_cursor(); matches if ANY of `with` (or any component of the entity when `with` is empty) qualifies
pub struct DynamicPage { pub entities: Vec<Entity>, pub next: Option<u32> }
```

`query_dynamic` scans `Archetypes` once (bitset subset/disjointness test per archetype, O(archetypes), a few microseconds at 100 archetypes), then walks entity **indices** in ascending order starting after `after`, keeping entities whose location is in a matching archetype and (if `since` is set) whose tick test passes, until `limit` is reached; `next` is the last visited index or `None` when done. Runtime conflicts do not arise because it only reads. Because the full `TickWindow` is part of the spec, `query_dynamic(&self, ..)` takes no counter, never increments, and evaluates `is_newer_than` exactly like a typed `iter_with`.

**Paging on a live world.** The cursor is an **entity-index keyset** (`after`), not an archetype/row position, so swap-remove and archetype moves cannot invalidate it and no `structural_version` exists or is needed. MCP requests are served at a sync point between schedule runs, so each page sees a frozen world; across pages the guarantee is *weakly consistent*: every entity that stays alive and matching for the whole paging session appears exactly once, entities spawned or despawned in between may be missed or seen once. Entity indices are stable while an entity is alive, so a 1M-entity world pages in `1M / limit` calls even if structure changes every frame. Worst-case scan per page is O(entity-index space) when matches are rare (about 1 ms per 1M metas; budgeted below), which is acceptable for a tooling call.

**Phase 4 MCP tools (defined here so the ECS API is shaped for them):**

| Tool | Kind | Behavior |
|---|---|---|
| `ecs_get_stats` | read | `WorldStats`: entity count, retired slots, archetype count, component count, `change_cursor` (diagnostic only, not a `changed_since` token), per-component `{name, rows, bytes_len, bytes_capacity, tracked}` |
| `ecs_list_components` | read | registered components `{name, size, align, reflected, storage, tracked}` |
| `ecs_list_archetypes` | read | `{id, components[name], entity_count, capacity}` |
| `ecs_query_entities` | read | `{with[], without[], changed_since?: cursor, added_since?: cursor, as_of?: cursor, limit, after?}` to `{entities[], next?, change_cursor}` via `query_dynamic`; optionally their reflected components. The first page (no `after`) calls `increment_change_cursor()` and returns that **pre-increment** value as `change_cursor`, which is the value to pass as the next session's `changed_since`; later pages of the session (`after` set) must send it back as `as_of` (required when a `*_since` is set) so every page evaluates one window and the same `change_cursor` is returned. A cursor older than 2^30 ticks returns `cursor_expired` and the caller does a full read |
| `ecs_get_entity` | read | every component of an entity as reflected JSON, plus `parent`/`children` when present and, on request, per-component `{added, changed}` ticks |
| `ecs_get_component` | read | one component of an entity, or a reflected field by path |

**Mutation always goes through the Phase 4 command bus** (rule 3), never directly. Typed, serializable commands with undo, all applied with `&mut World` between schedule runs (a different layer from the `Commands` buffer in #22, whose boundary #22 states). **Each command is exposed as its own MCP tool** (a thin wrapper that builds the typed command, whose schema is derived from reflection and submits it to the bus); the generic bus tools `command_undo`, `command_redo` and `command_get_history` (`<domain>_<verb>`) belong to the command-bus spec (Phase 4) and cover undo for every row below:

| Command | MCP tool | Undo |
|---|---|---|
| `SpawnEntity { components: [(type_name, value)] }` (returns `Entity`) | `ecs_spawn_entity` | `DespawnEntity` of the returned entity |
| `DespawnEntity { entity }` | `ecs_despawn_entity` | re-spawn from a reflected snapshot of all components; the new entity's id differs, so the history keeps an **old-to-new entity remap** applied to later undo/redo entries |
| `InsertComponent { entity, type_name, value }` | `ecs_insert_component` | previous value (or removal if absent) |
| `RemoveComponent { entity, type_name }` | `ecs_remove_component` | re-insert snapshot |
| `SetField { entity, type_name, path, value }` | `ecs_set_field` | previous field value |
| `Reparent { entity, parent: Option<Entity> }` (#49) | `ecs_reparent` | previous parent |

Command-bus writes are world-level writes and stamp the **current tick** (see the stamping rules), so `Changed<T>` bindings (UI data binding, Phase 3) observe editor and agent edits identically.

**Editor panels** (all read from the API above): hierarchy view = query `(Entity, Option<&Name>, Option<&Parent>, Option<&Children>)` (`Name` is a scene-crate component, Phase 2, not defined here); inspector = `EntityRef` + `ReflectComponent` enumeration; an archetype/memory view in the profiler panel from `WorldStats`.

## Extensibility

Each non-goal can be added without breaking the layout or the public tick/cursor API:

- **Removal and despawn log (multiplayer).** A `RemovedComponents<T>` / despawn log is an append-only `Vec<(Entity, Tick)>` per component, filtered with the same `is_newer_than(last_run, this_run)` test and clamped by the same age sweep. It needs no change to `Tick`, `TickWindow`, `ChangeCursor`, `increment_change_tick` semantics, `query_dynamic`'s `since`, or the per-client acknowledged-tick scheme replication will use: a consumer's cursor already selects both changed rows and removals newer than itself.
- **Sparse-set storage and hooks:** `ComponentInfo.storage` gains a `SparseSet` variant; `on_add`/`on_remove` hooks run through the same bookkeeping-before-destructors ordering.
- **Archetype GC:** requires the world-level epoch noted in the cached-index contract.
- **`Or` filters, `Has<T>`:** new sealed `QueryData`/`QueryFilter` impls; the `Access` model already carries the needed bitsets.

## Performance

Benchmarks live in `crates/exitium_ecs/benches/ecs.rs` (#51, #52) under the harness from #18 and run through `cargo xtask bench-ci` on the 5% regression gate. **The ratios against baselines are the binding contract** (hardware-independent). **Absolute figures are provisional ceilings** for the reference dev PC, to be re-baselined from the first benchmark PR (#51) by the `perf` agent; if a ceiling is missed, #51 files a perf issue instead of widening its scope.

Workload: `Position { x: f32, y: f32 }` (8 B), `Velocity` (8 B), further components 8 B; one archetype unless stated; single thread unless stated; release profile with `lto = "thin"`.

**Baselines compared against** (same file): (a) plain `Vec<(Pos, Vel)>` AoS loop; (b) hand-written SoA, two `Vec` zipped. External ECS comparison (`hecs`, `bevy_ecs`) would be new dev-dependencies, a gated follow-up, not part of v1.

| Metric | Budget |
|---|---|
| Iterate 1M entities x 1 component (read) | <= 1.25x hand-written SoA; <= 0.8 ms |
| Iterate 1M x 2 components (`&A`, `&mut B`, `bypass_change_detection`) | <= 1.25x hand-written SoA; <= 1.5 ms |
| Iterate 1M x 2 components with `Mut<T>` writing every row | <= 1.5x the bypass run; <= 2.2 ms |
| Iterate 1M x 4 components | <= 1.3x hand-written SoA; <= 3.0 ms |
| `Changed<T>` filter, 1M rows, no row matches | <= 1.2 ms (one u32 compare per row) |
| `Changed<T>` filter, 1M rows, all match, `&A` read | <= 2.0 ms |
| `(&mut A, Changed<A>)` read-modify-write, 1M rows, all match | <= 1.3x the `Mut<T>` writing-every-row run |
| `iter_chunks_mut` with `slices_untracked()` vs hand-written SoA | within 5% (the zero-overhead path) |
| `iter_chunks_mut` tracked (up-front stamp of every row's `changed`, ~0.25 ns/row), 1M x 2 | <= 1.3x hand-written SoA; <= 1.9 ms (the stamp is a deliberate, separate cost) |
| Per-matched-archetype setup (`init_fetch` + loop entry) | `init_fetch` <= 30 ns plus loop entry <= 10 ns, so 1,000 archetypes x 1,000 rows costs <= 40 us = 5% of the 0.8 ms single-component 1M-row read (`init_fetch` alone: 1,000 x 30 ns = 30 us = 3.75%) |
| `QueryState` entry-point `WorldId` assert | <= 1 ns per call, not per row; not measurable in the row loop |
| `par_iter_mut`, 1M x 2 comps, ~100 ns body | >= 3.2x speedup on 4 threads (80% efficiency); trivial body must not regress more than 10% vs single thread |
| Spawn, bundle of 2 components, `spawn_batch` pre-reserved | <= 100 ns each (>= 10 M/s); 1M in <= 100 ms |
| Spawn, one at a time, no pre-reserve | <= 150 ns amortized |
| Despawn (2 components), sequential order | <= 80 ns each; random order <= 150 ns |
| Add/remove component, cached edge (24 B row move) | <= 250 ns (>= 4 M/s) |
| First transition creating an archetype | <= 10 us |
| `get::<T>(entity)` random access, 1M entities | <= 40 ns |
| `QueryState` creation against 100 archetypes | <= 20 us; incremental update = O(new archetypes) |
| `Entities::reserve_entity` | <= 20 ns single-threaded; <= 60 ns per call with 4 threads contending on one `Entities` (a shared atomic does not scale linearly, so no scaling is claimed) |
| `Entities::reserve_entities(n = 64)` | <= 25 ns amortized per entity, 4 threads contending |
| `increment_change_tick` (atomic) | <= 20 ns single-threaded; <= 60 ns contended on 4 threads (once per system run, so negligible) |
| Tick age sweep | <= 2.5 ms per 1M rows x 2 tracked components (4M ticks = 16 MB read + 16 MB write, bandwidth-bound; vectorized clamp), executed in slices of <= 1 ms per sync point; a 10M-row world sweeps in about 25 ms total spread over >= 25 sync points, never as a single hitch. Starts once per 2^29 ticks. Relocation clamp while a sweep is active: <= 10 ns extra per moved row |
| `query_dynamic` page, 1M entity indices scanned | <= 2 ms (rare-match worst case); typical pages scan far fewer |
| `World::new()` | <= 10 us and <= 4 KiB |

**Memory:** fixed overhead per entity <= 20 B (12 B `EntityMeta` + 8 B `Entity` in the row list) and <= 8 B per **tracked** component per entity (two ticks; 0 for components with `TRACK_CHANGES = false`). Reference shape, 1M entities x 2 x 8 B tracked components after `shrink_to_fit`: <= 52 MB (16 data + 16 ticks + 20 overhead); with doubling slack (capacity 2^20) <= 64 MB.

**10M entities x 2 tracked 8 B components.** Per entity: 16 B data + 16 B ticks + 8 B in the row list + 12 B `EntityMeta` = 52 B.

- **With `reserve_entities`/`spawn_batch` pre-sizing: 10M x 52 B = 520 MB, budget <= 600 MB**, no growth transient. This is the supported way to hold 10M entities on the 8 GB min-spec profile.
- **Without pre-sizing,** doubling rounds capacity to 2^24 = 16.8M: columns and row list (16 + 16 + 8) B x 16.8M = 671 MB plus `Vec<EntityMeta>` 12 B x 16.8M = 201 MB = **about 870 MB steady state**, plus a transient of up to about 130 MB while the largest array is reallocated (old + new copy), so a **peak of about 1.0 GB**. The earlier "within 1 GB" figure ignored this slack and is replaced: the unreserved budget is **<= 1.1 GB peak**, measured in #66.

Memory is reported through the min-spec profile (#66) and `WorldStats`.

**Complexity:** `spawn`/`despawn`/`get`: O(1) amortized. Structural transition with a cached edge: O(row bytes). Query iteration: O(matched archetypes + rows). `Changed` filter: O(rows) (no skipping, see Open Questions). Query matching: O(new archetypes) per refresh. Tick sweep: O(rows x tracked components), amortized over sync points.

## Test plan

- **Unit, safe layer:** entity alloc/free churn with generation bump and stale detection; generation-overflow retirement (force the counter); generation 0 rejected by every conversion; reserve-then-flush and batched reserve; component registry id stability, ZST, `drop_fn` presence only for `needs_drop` types; archetype lookup by set, graph edge caching, archetypes append-only with stable column index; bundle arity 1..8 and duplicate rejection.
- **Unsafe modules (Miri-clean, see plan above):** column push/grow/swap-remove/drop/ZST/over-alignment/moved-without-drop; **panicking `Drop` during despawn, remove, overwrite-insert and world drop with entity-table consistency asserted afterwards (stale `get(dead)` is `None`, the swapped-in entity's components are intact)**; **`spawn_batch` with an iterator that panics at element k (rows 0..k are complete and live, no half-written row, invariants (a) to (c) hold)**; `Drop`-counting types prove every component is dropped exactly once, including on world drop; fetch aliasing cases; `Mut` write path through `TickCell`; parallel disjoint ranges of one column.
- **Query behavior:** every filter combination and empty results; `Option` present/absent; conflict rejection for each conflicting pair above (panic message text asserted, `try_` returns `Err`), and acceptance of `(&mut A, Changed<A>)`/`(&mut A, Added<A>)`; **a `QueryState` used on a second world panics (iteration) or returns `WorldMismatch` (`get`)**; compile-fail doctests that `iter(&world)`/`get(&world, ..)` reject `&mut T` queries and `iter_chunks` rejects `Added`/`Changed` filters; `get(entity)` for stale/non-matching entities; iteration-order guarantee (ascending archetype, ascending row) and the documented non-stability after swap-remove; archetype created after a state was built is matched on next use.
- **Change detection:** change seen exactly once per system run; spawn counts as added; overwrite counts as changed, not added; untouched `Mut` flags nothing; chunk mutable access flags the whole chunk, `slices_untracked` does not; moved components keep their ticks across archetype transitions; **world-level write after a run is seen by that run's next execution (command-application case)**; **standalone `QueryState`: `iter`, then `spawn` with no explicit increment, then `iter` sees the spawn**; own direct writes are not seen on the next run; `(&mut A, Changed<A>)` does not retrigger itself; `None` window sees everything; **never-run system after a world older than 2^30 ticks (force the counter) still sees every old row as added**; **wraparound:** advance the tick to near `u32::MAX`, run the incremental sweep (including partial, interleaved with runs) and assert `is_newer_than` correct and no false negatives across the clamp (property test over random stamps and `last_run`s); sweep completion deadline; `ChangeCursor` valid below 2^30 age, `CursorExpired` above; `untracked` component rejected by `Added`/`Changed` and `Mut::is_changed()` returns true.
- **Incremental sweep (signed-age rule, relocation):** force the counter near `u32::MAX` (test-only hook). (1) *Post-`ref` write is never clamped:* write a row, start a sweep, write a second row at `ref + 5` in a not-yet-swept range, let a later slice pass it, and assert system S with `last_run = ref` sees it (the round-2 miss) and its stamp is unchanged. (2) *Completion-time `last_run`:* a system that ran at `ref + 3` during the sweep keeps its `last_run` unclamped and does not re-see old rows after completion; an idle system with `last_run` older than `ref - 2^30` is clamped and sees rows only as false positives. (3) *Relocation:* write rows, run a partial sweep, then mid-sweep (a) despawn a swept-region row so the unswept tail row swap-removes into it, (b) `insert` a component to move an unswept-archetype row into an already-swept archetype, (c) create a new archetype by a move, then finish the sweep and assert no stored tick has age above 2^30 relative to `ref`, nothing was missed for any `last_run`, and the maximum stored age stayed below 2^31. (4) Property test over random stamps, `last_run`s and interleaved ops: every change newer than a not-clamped `last_run` is seen, before, during and after the sweep.
- **Cursors (`changed_since`):** page, then a world-level write (spawn, `insert`, a `get_mut` write), then page with the returned cursor: the write is seen exactly once and the earlier rows are not seen again; a cursor read with plain `change_cursor()` (no increment) is shown to lose that write, which is why only the `increment_change_cursor()` value is valid; a multi-page session with a write between pages evaluates one `as_of` window and the write appears in the next session; `resolve_cursor` rejects a cursor older than 2^30 and a cursor from the future; `DynSince` with a full `TickWindow` agrees with a typed `iter_with` on the same data.
- **`get` and `iter_with` ticks:** a million `state.get(..)` calls advance the counter by 0; `iter`, `get`, `iter` on a `Changed` state does not lose a change that the second `iter` should see (`get` leaves `last_run` alone); `iter_with` changes neither `last_run` nor the counter; an expired standalone `last_run` cursor behaves as `None`.
- **Mutation flag paths:** `World::get_mut` and `ReflectComponent::get_mut` that are never dereferenced mutably flag nothing; a write through either flags `world.change_tick()`; a `SetField` with a bad path or a value that fails to parse flags nothing and a successful one flags once; a component with `#[component(no_change_detection)]` rejects `Added`/`Changed` and its `#[reflect(Component)]` registration still compiles; `#[reflect(Component)]` on a type without `Component` is a compile-fail doctest.
- **Dynamic query / MCP support:** `query_dynamic` matches the typed query on the same data; keyset paging returns each stable entity exactly once while another thread of test code spawns/despawns between pages; `since` filter agrees with `Changed`.
- **Property/model test:** random op sequence vs a model (above), 10k ops x many seeds in normal CI, reduced under Miri.
- **Parallel:** `par_iter_mut` visits each row exactly once (counter array); results equal the sequential run; runs on the wasm single-thread fallback path.
- **Benchmarks:** #51/#52 per the Performance table; CI gate at 5% via `xtask bench-compare`.
- **wasm32:** `cargo build --target wasm32-unknown-unknown` for `exitium_ecs`.

## Downstream issue adjustments

Dependent issues were written before this spec; where it diverges they should follow the spec:

- **#15 (handles/`RawHandle`):** generation is `NonZeroU32` (first generation 1) so `Option<Entity>` is 8 bytes; its AC currently says a plain `u32`. Conversion from a raw handle with generation 0 is fallible (`TryFrom`), never defaulted or wrapped. #15 is still `status:needs-approval`, so the change should be made in its AC before approval.
- **#42 (queries):** requires `unsafe` in two more designated modules (`world/cell.rs`, `query/fetch.rs`), not "only the column module". Its PR touches designated unsafe modules and must be `gate:human` with Miri tests. Delivers `WorldId` binding, `ReadOnlyQueryData` and the read-only/writable `UnsafeWorldCell` constructors, the `Access` split (`tick_reads`), and `(&mut A, Changed<A>)`. Consider splitting `fetch.rs` + `cell.rs` (unsafe) from `state.rs` + `filter.rs` + `access.rs` (safe) to stay near 600 lines.
- **#43 (change detection):** `Added`/`Changed` count as reads of `T` for scheduler conflicts; chunk-API stamping rule; the `TickWindow`/`Option<Tick>` model, `increment_change_cursor`/`resolve_cursor`, the stamping contract (flag on `DerefMut` only, `get*` burn no tick) and the **incremental** age sweep (`check_change_ticks(budget)`, signed-age clamp, relocation clamp while active, completion-time `last_run` clamp) are part of its scope, as is the `TRACK_CHANGES` opt-out and the relaxed-atomic `TickCell`.
- **#39 (registry):** also delivers `#[derive(Component)]` with `#[component(no_change_detection)]` (a proc-macro crate reusing #20's `syn`/`quote`; `#[derive(Reflect)]` does not imply `Component`); `drop_fn` shim comes from `storage::column::drop_fn_of::<T>()`; registration is `&mut`-only (no locking); adds `ComponentInfo.storage`, `ComponentInfo.track_changes` and `Components::id_of(TypeId)`.
- **#41 (World):** also delivers the introspection API (`stats`, `archetypes`, `components`, `entity`, `Archetype::entities`/tick getters), `query_dynamic`/`DynamicQuerySpec`, `reserve_entities`, `spawn_batch`, `shrink_to_fit`; `World::despawn` does not cascade and follows the bookkeeping-before-destructors ordering.
- **#38 (entities):** `NonZeroU32` generation, `reserve_entity(&self)` and batched `reserve_entities` lock-free paths, location table, fallible generation-0 conversions.
- **#50 (reflection hook):** needs `EntityRef::component_ids()`; reflection is built from typed closures (`world.get::<T>`), so no type-erased public API in the ECS is required; `#[reflect(Component)]` (as in #20) only registers `ReflectComponent` type data for a type that already implements `Component` (via the ECS `#[derive(Component)]`, not `#[derive(Reflect)]`); `ReflectComponent::get_mut` returns a `ReflectedMut` wrapping `Mut<T>` that flags only when `as_reflect_mut` is called (see Change detection). The matching `reflect-model.md` wording is aligned (#78).
- **#51/#52 (benches):** use the budgets above and add the baselines (a), (b).
- **#22 (scheduler spec):** takes `FilteredAccess` (with `tick_reads` merged into `reads`), the tick contract (`increment_change_tick(&self)` returns the pre-increment `this_run`; `last_run: Option<Tick>`, `None` until first run; sweep polling at sync points, slice budget and the completion-time `last_run` clamp with the signed-age rule, so `last_run`s set during a sweep are untouched; `get*` are not runs) and `UnsafeWorldCell` (writable only from `&mut World`) from this spec.

## Open questions

1. **Row-level vs chunk-level ticks.** v1 is per-row (exact). Per-64-row-block `max_changed` summaries would let `Changed<T>` skip untouched blocks and cut tick writes, at the cost of false positives. Defer to a perf follow-up after #51 measures `Changed` scan cost?
2. **Sparse-set storage and component hooks (`on_add`/`on_remove`, `RemovedComponents`).** Needed for robust hierarchy maintenance (dangling `Parent`) and for observing removal. Deferred to a later spec (see Extensibility); is the `StorageType` placeholder and the hierarchy-via-helpers approach acceptable for Glutton?
3. **Archetype fragmentation policy.** Are the diagnostic (`archetype_count`, warning at 4,096) and the "toggle as a field" guidance enough, or should a hard cap/GC of empty archetypes be designed now?
4. **Query data via `&mut T` yielding `Mut<T>`** (always tracked unless the component opts out) versus plain `&mut T` plus explicit tracking. The spec chooses `Mut<T>` for correctness (no silent missed changes) and offers `bypass_change_detection`; confirm the ergonomics are acceptable for the basic-level-reviewer rule.
5. **Panic vs `Result` for query conflicts.** Spec: `query_state` panics, `try_query_state` returns `Err` (for generic callers wanting a `Result`; runtime/MCP callers use `DynamicQuery`). Confirm.
6. **Identity across despawn-undo.** The command history remaps old-to-new entities. Should editor-visible persistent identity instead be a `Guid` component owned by `exitium_scene` (Phase 2)? Does not block Phase 1.
7. **Miri model and nightly pinning.** Add `-Zmiri-strict-provenance -Zmiri-tree-borrows` runs and pin a nightly date in `xtask ci miri`/the runner image (currently unpinned `+nightly`). This is an `xtask`/CI-config change (gated) and independent of the ECS PRs.
8. **`#[derive(Component)]` crate placement.** Decided: `exitium_ecs` provides `#[derive(Component)]` with `#[component(no_change_detection)]`; `#[derive(Reflect)]` does not implement `Component` and `#[reflect(Component)]` only registers `ReflectComponent` type data. Open: does the Component derive share a proc-macro crate with #20's Reflect derive or live in a sibling? An implementation detail for #39/#20; either way no new external dependency.
9. **Facade exposure.** Which of `World`, `Query*`, `Mut`, `Tick`, `ChangeCursor`, `EntityRef` are re-exported through `exitium` in the first facade PR (gated)?
10. **Absolute budgets** are provisional (see Performance); the `perf` agent should confirm or adjust after #51 on the reference runner and the 4-thread min-spec profile.

Resolved in review round 2: the sweep uses a signed age with a relocation clamp, the external cursor is the pre-increment `increment_change_cursor()` value with a full `TickWindow` in `DynSince`, `Component` is derived by `exitium_ecs` (not by `Reflect`) with the opt-out on that derive, and every mutable path flags on write only.

Resolved in review round 1: the change-tracking opt-out (formerly Open Question 1) is **adopted in v1** as `Component::TRACK_CHANGES` (see Change detection), because it alters the `Column` and `Mut<T>` layout and cannot be retrofitted cheaply.
