# exitium_core

Foundation crate: math (glam), `Num`, handles and `Slab`, `Guid`, `Error`, `Time`. Every other crate sits above it (PLAN § Target architecture).

## Public API
- Games reach it through the `exitium` facade. Engine crates use it directly.
- Math comes from `exitium_core::math`. Nobody else depends on glam (one version in every build).
- Prefer a narrow, crate-local error type. `Error` is for code that spans crates (`error.rs` docs).

## Invariants
- `Num` is always finite and never `-0.0`. Every op goes through `Num::from_f64`. Its field is private. Besides `from_f64`, only the constants and the derived `Default` (0.0) build one. Do not add derives or views that expose or construct the inner `f64` bypassing `from_f64` (e.g. a field-level `Deserialize`/`Reflect`, a `&mut f64`). Its `Reflect` impl lives in `exitium_reflect`: the merged spec makes it `transparent`, and pending #78 (open) proposes a Value kind.
- Pending PR #89 (open): formatting and parsing for `Num`. Not on main.
- `RawHandle` is 8 bytes with a `NonZeroU32` generation (0 is never valid). Bits from outside go through the fallible `from_bits`. The ECS `Entity` reuses this layout.
- Nothing here reads a clock or a random source (`Instant::now` panics on wasm). `Time` and `FixedClock` are plain data. `Guid`s are minted by the asset database.

## unsafe
None today, and none designated. If a hot path needs it, make a designated module with `// SAFETY:` comments and Miri tests (root CLAUDE.md). Extra reviewer scrutiny applies (ADR-0005).

## Dependencies
`glam` only, with the features set once in the workspace `Cargo.toml`. No dependency on other `exitium_*` crates (the graph points down to core). A new dependency must be justified in the PR.

## wasm
Builds on `wasm32-unknown-unknown` (checked by `cargo xtask ci wasm`). No `std::time::Instant`: it panics there.

## Benchmarks
Conventions: `docs/benchmarks.md`. `benches/num.rs` exists. A hot-path change (design rule 4: e.g. `Num` ops, `Slab` insert/get/remove, `Rect`/`Color`) needs a benchmark in the same PR, named `exitium_core/<area>/<case>`. CI fails on a regression over 5%.
