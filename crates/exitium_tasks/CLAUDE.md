# exitium_tasks

Job system: one worker pool, `scope`/`spawn`, `parallel_for`, `par_chunks[_mut]`, `join`, and `ThreadBudget`. Spec: `docs/specs/tasks-job-system.md`. Read it before editing. The crate is still a stub (slices #26 to #30).

## Public API
- Safe API only. The same signatures on every target, `Send`/`Sync` bounds included.
- A job must not block on anything except another scope or job (the job rule in the spec).
- Not a system scheduler. Ordering, access-conflict batching and the tick loop belong to the ECS scheduler spec (`TODO(spec #22)`) and `exitium_app`.

## Invariants
- `threads` in `ThreadBudget` includes the calling thread. `EXITIUM_THREADS` overrides it. Min-spec is 4.
- No job is dropped unrun. `scope` returns only when `pending == 0`, and workers never die on a job panic.
- No cancellation in v1: after a panic, every job already spawned still runs. The first payload is re-raised.
- Help depth is bounded by `HELP_DEPTH_LIMIT` (16). The restricted-helping argument is being strengthened (#77).

## unsafe
Only `src/scope.rs` is designated (`#[allow(unsafe_code)]`). It holds the lifetime erasure and everything its soundness depends on (invariants I1 to I7 in the spec). `// SAFETY:` on every block, Miri tests, extra reviewer scrutiny (ADR-0005). **Do not implement the job wrapper from the spec text until #77 lands**: it fixes the wrapper shape (I1/I5) that Miri rejects as written. Keep other modules safe.

## Dependencies
None external, and no `rayon`. A switch needs a PR with the measured trigger from the spec. Optional workspace-internal features `reflect` and `profiling` are off by default.

## wasm
Builds on `wasm32-unknown-unknown` and runs inline (`threads == 1`). Only `platform.rs` may use `cfg(target_arch = "wasm32")`. `clippy.toml` bans `Instant::now`, `thread::spawn` and `available_parallelism` elsewhere. Tests run under `ThreadBudget::single()` as the wasm proxy.

## Benchmarks
`benches/pool.rs` (#30), thread count from `EXITIUM_THREADS` (`docs/benchmarks.md`). Required cases are the spec's Performance table. The scaling ratios against a serial loop are binding. Absolute times are provisional ceilings.
