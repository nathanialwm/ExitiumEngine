# exitium_core

Foundation crate: math (glam), `Num`, handles and `Slab`, `Guid`, `Error`, `Time`. Every other crate sits above it (PLAN § Target architecture).

## Public API
- Games reach it through the `exitium` facade. Engine crates use it directly.
- Math comes from `exitium_core::math`. Nobody else depends on glam (one version in every build).
- Prefer a narrow, crate-local error type. `Error` is for code that spans crates (`error.rs` docs).

## Invariants
- `Num` is always finite and never `-0.0`. Every op goes through `Num::from_f64`. Its field is private: do not give it a derive or a `&mut f64` view. Its `Reflect` impl lives in `exitium_reflect` (amendment: #78).
- Formatting and parsing for `Num` is in PR #89.
- `RawHandle` is 8 bytes with a `NonZeroU32` generation (0 is never valid). Bits from outside go through the fallible `from_bits`. The ECS `Entity` reuses this layout.
- Nothing here reads a clock or a random source (`Instant::now` panics on wasm). `Time` and `FixedClock` are plain data. `Guid`s are minted by the asset database.

## unsafe
None today, and none designated. If a hot path needs it, make a designated module with `// SAFETY:` comments and Miri tests (root CLAUDE.md). Extra reviewer scrutiny applies (ADR-0005).

## Dependencies
`glam` only, with the features set once in the workspace `Cargo.toml`. No dependency on other `exitium_*` crates (the graph points down to core). A new dependency must be justified in the PR.

## wasm
Builds on `wasm32-unknown-unknown` (checked by `cargo xtask ci wasm`). No `std::time::Instant`: it panics there.

## Benchmarks
Conventions: `docs/benchmarks.md`. `benches/num.rs` exists. Touching `Num` ops, `Slab` insert/get/remove or the math types needs a benchmark in the same PR, named `exitium_core/<area>/<case>`. CI fails on a regression over 5%.
