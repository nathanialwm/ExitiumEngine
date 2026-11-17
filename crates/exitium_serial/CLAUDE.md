# exitium_serial

Text format (human-editable, diff-friendly) and custom binary format (schema-hashed, aligned, zero-copy bulk arrays, zstd/LZ4) (PLAN § Target architecture). The crate is still a stub.

**Specs are not written yet.** Text format: `TODO(spec #23)`. Binary format, schema hashing and migrations: `TODO(spec #24)`. Do not implement from this file. Read the specs first. If they do not exist, stop and ask.

## Public API
- Serializers walk reflection directly (`exitium_reflect`, `docs/specs/reflect-model.md`). Never hand-write a parallel schema (design rule 2).
- Format versions, headers and schema tables: `TODO(spec #24)`.

## Invariants
- Both formats are persisted data. A change to either is a format change and gets extra reviewer scrutiny (ADR-0005).
- The reflect spec fixes what the schema hash covers and that `version` is never hashed. #24 decides how an old binary file decodes for migrations (#78, #59).
- Values cross MCP in the text format (#23).

## unsafe
None designated. Zero-copy and aligned bulk arrays may want it: `TODO(spec #24)`. If a spec adds a designated module, it follows the root rules (`// SAFETY:` comments, Miri tests).

## Dependencies
`exitium_core`, `exitium_reflect`. Compression crates (zstd, LZ4) are new dependencies. Justify each in the PR body (ADR-0005) and add a single entry in `[workspace.dependencies]`. Which ones: `TODO(spec #24)`.

## wasm
The crate is not in `NATIVE_ONLY` (`xtask/src/ci.rs`), so it must build for `wasm32-unknown-unknown` today. `TODO(spec #24)`: wasm status of the format and compression crates.

## Benchmarks
Serializing a large world (the reflect spec budgets for 1M entities) is the hot path. Benchmarks, sizes and budgets: `TODO(spec #23/#24)`. Follow `docs/benchmarks.md`: `exitium_serial/<area>/<case>`, 1k, 100k and 1M sizes.
