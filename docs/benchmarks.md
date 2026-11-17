# Benchmarks

Design rule 4: every hot-path change ships with a benchmark, and CI fails when
one gets more than 5% slower. This page is the convention every benchmark
follows so the gate can compare them run to run.

## Where benchmarks live

- One file per area: `crates/<crate>/benches/<area>.rs`.
- Each file is a `[[bench]]` target with `harness = false`, so criterion
  provides `main`:

  ```toml
  [dev-dependencies]
  criterion.workspace = true

  [[bench]]
  name = "num"
  harness = false
  ```

- `criterion` is a dev-dependency only. It never reaches game builds or the
  WASM target.
- Start the file with a `//!` doc that says what is measured and why it is hot.

## Naming

Benchmark IDs are `<crate>/<area>/<case>`, with the input size as criterion's
parameter:

```rust
let mut group = c.benchmark_group("exitium_core/num");
group.bench_function(BenchmarkId::new("add", len), |b| { /* ... */ });
// -> exitium_core/num/add/1000
```

The gate matches results by ID. Renaming or deleting a benchmark makes the gate
fail as "missing at HEAD" unless the PR carries the `bench:removal-ok` label,
so pick names you will keep.

## Sizes

Run each case at **1k, 100k and 1M** elements (`[1_000, 100_000, 1_000_000]`).
1k fits in L1 and shows per-item overhead. 100k spills to L2/L3. 1M is the
memory-bound case and the scale of a large scene. Drop a size only when it is
meaningless for the case, and say why in the file's `//!` doc.

Also:

- Set `group.throughput(Throughput::Elements(len))` so reports show items per
  second and sizes compare directly.
- Build inputs outside `b.iter`, deterministically (fixed seeds, no clock or
  OS randomness), so base and HEAD measure the same work.
- Wrap inputs and outputs in `std::hint::black_box` so the optimizer cannot
  delete the work.

## Thread count

Benchmarks of multithreaded code read the thread count from the
`EXITIUM_THREADS` environment variable, the same override the engine's
`ThreadBudget` uses (`docs/specs/tasks-job-system.md`). Unset means the
default budget. The min-spec profile runs the same benchmarks with
`EXITIUM_THREADS=4` (#66).

- Resolve the count and build the pool once, outside the timed region.
- Do **not** put the thread count in the benchmark ID. Base and HEAD run under
  the same environment, so the same ID compares the same configuration, and
  the full and min-spec runs keep matching names.
- Until `exitium_tasks` lands, read the variable with
  `std::env::var("EXITIUM_THREADS")` and fall back to
  `std::thread::available_parallelism()`.

## Running

```bash
# One file, while working on it:
cargo bench -p exitium_core --bench num

# The CI gate, locally: benchmarks <ref> as the baseline, then HEAD against it.
cargo xtask bench-ci origin/main

# Re-check the last results against a different threshold (percent):
cargo xtask bench-compare 3
```

`bench-ci` checks out `<ref>` in a temporary worktree under `target/`, runs
every `[[bench]]` target there, and saves the `base` baseline. Then it runs HEAD
against it. It fails when a benchmark's mean is more than 5% slower **and** the
slowdown is statistically significant (the lower bound of the 95% confidence
interval is above zero). Close the IDE and other heavy programs first, or noise
will hide real changes.

CI runs `bench-ci` against the PR's base commit on the dedicated
`exitium-bench` runner, one PR at a time.
