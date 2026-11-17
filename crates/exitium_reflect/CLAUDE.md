# exitium_reflect

Runtime type info. One `#[derive(Reflect)]` feeds the inspector, serialization, Luau and MCP schemas (design rule 2). Spec: `docs/specs/reflect-model.md`. Read it before editing. The crate is still a stub (slices #31 to #37, #50).

## Public API
- Metadata is read-only. Reflection supplies the primitives (`apply`, `path_mut`, `clone_boxed`) and serializers call them directly. Editor, MCP and Lua mutations built on them go through command-bus commands (design rule 3).
- Reflection stays JSON-free. `TypeInfo` to JSON Schema lives in `exitium_mcp`.
- Re-exported as `exitium::reflect` by the facade (a high-risk surface).
- Not reflectable in v1: sets, `usize`/`isize`, borrowed fields, trait objects. `Handle<T>` has no impl (only `RawHandle`).

## Invariants
- Reflected types are `Send + Sync`. `TypeInfo` is immutable `'static` data. Registration is idempotent and order-independent.
- The schema hash and the per-type `version` are part of the persisted format. Changing what is hashed is a format change. The spec defines what the hash covers.
- Core types (`Num`, `Guid`, `RawHandle`, glam) get their `Reflect` impls here (orphan rule). Per the merged spec, `Num` and `Guid` are `transparent` (Value covers only primitives and `String`), `Entity` is a derived struct `{ index, generation }` in `exitium_ecs`, and pinned engine names are unqualified. **Pending #78 (open) proposes changing this**: `Num` becomes a Value kind and `Entity` is hand-written. Follow the spec until #78 merges.
- `Component` and `Resource` belong to `exitium_ecs`. The derive never implements them.
- Migration decoding of old binary files is decided in `TODO(spec #24)`.

## unsafe
None, and no Miri module. Downcasting uses `std::any::Any`. Leaked `TypeInfo` for generics uses `Box::leak`.

## Dependencies
`exitium_core` only, no external crates, no `serde`. Serializers walk reflection directly. The direction is `ecs`, `serial`, `data`, `script`, `mcp`, `editor` depend on reflect, never the reverse.

## wasm
Must build for `wasm32-unknown-unknown`. The golden-vector tests run natively. A wasm run is an open question.

## Benchmarks
`benches/` per the spec's Performance table: `type_info`, registry lookup by `TypeId` and name, path access, `apply`, `clone_boxed`, leaf walk, `shallow_hash`, `deep_hash`. One bench per row. Conventions: `docs/benchmarks.md`.
