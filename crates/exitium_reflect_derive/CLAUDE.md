# exitium_reflect_derive

Proc-macro crate behind `#[derive(Reflect)]`. `exitium_reflect` re-exports it. Spec: `docs/specs/reflect-model.md` ("Derive macro"). Slices #34 (structs) and #35 (enums). The crate is still a stub.

## Public API
- One derive only. No function-like or attribute proc macros, no `macro_rules!` DSL in public API.
- Emits four impl groups and nothing else: `Reflect` plus its kind trait, `Typed`, `FromReflect`, `GetTypeRegistration`. No new public items, hidden fields, helper modules or global state.
- Never implements `Component` or `Resource` (`exitium_ecs` owns them).
- Planned (#34): a hand-written equivalent in `exitium_reflect/tests/manual_impl.rs`, not present yet. Keep it matching the derive once it exists.

## Invariants
- Readable by a basic-level Rust reviewer (root rule 6): at most 800 lines excluding tests, and attributes parsed by hand on `syn`'s `parse_nested_meta` (no `darling`).
- No `unwrap` or `panic`. Errors are `compile_error!` with a span. Unions, lifetimes, borrowed fields and `usize`/`isize` fields are clear compile errors.
- Generated paths are fully qualified. Engine crates (the `ENGINE_CRATES` allow-list, matched on `CARGO_PKG_NAME`) get `::exitium_reflect`. Everything else gets `::exitium::reflect`. `#[reflect(crate = ..)]` overrides this.
- `tests/crate_paths.rs` fails when a new crate under `crates/` is not classified. Do not skip it.
- Spec: pinned engine type names (`Num`, `Vec2`, `Entity`, ...) are unqualified, user types use `module_path!()` and may override with `#[reflect(type_name = ..)]`. Pending #78 (open): restricts unqualified overrides to engine crates. Follow the spec until it merges.

## unsafe
None. No Miri module.

## Dependencies
`syn`, `quote` and `proc-macro2`, used only here, declared once in `[workspace.dependencies]` by #34 (justified in the spec, all `MIT OR Apache-2.0`). Not declared yet. `exitium_reflect` takes no external dependency.

## wasm
In `NATIVE_ONLY` in `xtask/src/ci.rs`: a proc macro runs on the host, so it is excluded from the wasm build. The code it generates must still compile on wasm.

## Benchmarks
None. It runs at compile time and is not hot at runtime. Correctness is covered by the derive tests and the manual-impl parity test.
