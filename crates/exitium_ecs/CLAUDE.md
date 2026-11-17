# exitium_ecs

Archetype SoA ECS: entities, components, queries, change detection. Spec: `docs/specs/ecs-storage-queries.md`. Read it before editing. The crate is still a stub. Hottest and riskiest code in the engine (INTERVIEW R11).

## Public API
- Game-facing types go through the `exitium` facade. What is re-exported is decided in the first facade PR (a high-risk surface).
- `Component: Send + Sync`. `#[derive(Component)]` is provided by `exitium_ecs` (crate placement, shared with the Reflect derive or a sibling, is open in the spec). `#[derive(Reflect)]` never implements it. `ReflectComponent` lives here too (hook: #50).
- `QueryData` and `QueryFilter` are sealed. No sparse sets, hooks or `Or<>` in v1.
- Scheduler, `Commands`, resources, events: `TODO(spec #22)`.

## Invariants
- `Entity` is 8 bytes (`RawHandle` layout, `NonZeroU32` generation), so `Option<Entity>` is 8 bytes. Generation 0 is rejected.
- Structural change needs `&mut World` and happens at sync points. **Columns never reallocate during a parallel phase.**
- A parallel task builds references only for its own rows. Nothing returns `&mut Column` or a whole-column `&mut [T]`.
- Ticks: `Mut<T>` flags only on `DerefMut`, and `get*` burn no tick. Pending amendments, not merged (`get_mut` window, sweep polling, MCP `as_of`): #79. `Entity` reflection: pending #78 (the merged spec has it as a derived struct).

## unsafe
Exactly three designated modules: `storage/column.rs`, `world/cell.rs`, `query/fetch.rs`. Everything else keeps the deny. `// SAFETY:` on every block. Each module's `//!` lists its invariants. The whole crate's tests run under Miri, so keep them Miri-friendly. Extra reviewer scrutiny (ADR-0005).

## Dependencies
`exitium_core`, `exitium_tasks` (`parallel_for`) and `exitium_reflect` for the hook. No external crates.

## wasm
Must build for `wasm32-unknown-unknown`. `par_iter_mut` falls back to sequential through `exitium_tasks`.

## Benchmarks
`benches/ecs.rs` (#51, #52), per the spec's Performance table. Compare against a plain `Vec` AoS loop and a hand-written SoA baseline. Cases: iterate 1 to 4 components, `Changed`, `par_iter_mut` (4 threads), spawn/despawn, add/remove, `get`, tick sweep. Memory is tracked against the 4-thread, 8 GB min-spec profile (#66).
