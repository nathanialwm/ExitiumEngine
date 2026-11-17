# exitium_profiler

Intended to be the engine's single instrumentation API (PLAN § Target architecture: tracing, Tracy, frame stats). **On main the crate is a stub**: `src/lib.rs` is one doc line, `[dependencies]` is empty, there are no features and no benches.

**Pending PR #87** (not merged: re-check main before relying on any of this) proposes `profile_scope!`, an off-by-default `profiling` feature, an optional `tracing` dependency, `benches/scope.rs` and a "Span naming convention" in `src/lib.rs`. Until it merges, none of these exist. If you touch this crate after it merges, replace this paragraph with the real state.

## Public API
- Today: nothing. Other crates must not invent their own instrumentation macros. Wait for #87 or coordinate with it.
- Pending #87: other crates mark work with `profile_scope!` and never name `tracing`, Tracy or the editor profiler. The span naming rules live in `src/lib.rs` once it lands. Link to them, do not copy them here.

## Invariants
- Pending #87: `profiling` off means the macro expands to nothing and shipped builds pay zero.

## unsafe
None.

## Dependencies
None today. A new dependency must be justified in the PR body (ADR-0005) and declared once in `[workspace.dependencies]`.

## wasm
Builds for `wasm32-unknown-unknown` today (`cargo xtask ci wasm`; it is not in `NATIVE_ONLY`). Pending #87: its xtask change also builds the crate with `profiling` for wasm32.

## Benchmarks
None today. Pending #87: `benches/scope.rs`. Conventions: `docs/benchmarks.md`.
