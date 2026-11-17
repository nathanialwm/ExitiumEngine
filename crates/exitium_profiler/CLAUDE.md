# exitium_profiler

The engine's single instrumentation API: `profile_scope!` and the `profiling` feature (PR #87). Tracy and the in-editor profiler panel are later backends (PLAN § Target architecture).

## Public API
- Other crates mark work with `profile_scope!` and never name `tracing`, Tracy or the editor profiler. Backends can then change without touching instrumented code.
- **Span names follow the "Span naming convention" in `src/lib.rs`** (`crate::system`, real crate names, literal names, no per-call data). Link to it. Do not copy it here or invent another.

## Invariants
- `profiling` is off by default. Off, the macro expands to nothing and `tracing` is not compiled: shipped builds pay zero.
- On, each scope is a `tracing` span at `INFO` with target `exitium`. Cargo unifies features, so one crate enabling it turns spans on everywhere.

## unsafe
None.

## Dependencies
`tracing` only, optional behind `profiling`, default features off. A new dependency (a Tracy client, for example) must be justified in the PR (license, wasm support, why not std) and added to `[workspace.dependencies]` once.

## wasm
Must build for `wasm32-unknown-unknown` (`cargo xtask ci wasm`). Wasm support of the `profiling` backends is not decided yet.

## Benchmarks
`benches/scope.rs` measures `profile_scope!` overhead per item. Feature off, `scoped` must match `baseline` (the zero-cost claim, and what the CI gate runs). Feature on, with no subscriber, shows the cost of a disabled callsite. Conventions: `docs/benchmarks.md`.
