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

# The CI gate, locally: benchmarks <ref> against HEAD (about 6 minutes today).
cargo xtask bench-ci origin/main

# Re-check the last bench-ci results against a different threshold (percent):
cargo xtask bench-compare 3
```

`bench-ci` checks out `<ref>` in a temporary worktree under `target/` and builds
both sides, each in its own cargo target dir. Then it runs every `[[bench]]`
target in 10 short rounds per side, alternating base and HEAD (`base head head
base base head ...`). Each round is a separate process with a 0.3 s warm-up,
0.5 s of measurement and 10 samples per benchmark, saved as the criterion
baseline `base-N` or `head-N` under `target/criterion/`. Uncommitted changes
count as HEAD.

## The gate rule

1. **First pass.** For each benchmark, take the median of its 10 base rounds
   and the median of its 10 HEAD rounds. Flag it when the HEAD median is more
   than 5% slower.
2. **Confirm pass.** Run only the flagged benchmarks again, in 20 fresh rounds
   per side (baselines `confirm-base-N`, `confirm-head-N`). The gate fails only
   for a benchmark that this pass flags too.

The output lists every benchmark with its change in each pass and its round
times, so you can see why it passed or failed. `noise` marks a benchmark the
first pass flagged and the confirm pass cleared. `bench-compare` re-applies the
rule to saved rounds without measuring again. At a lower threshold than 5% it
has no confirm rounds for newly flagged benchmarks, so the first pass decides
alone.

### Why this rule (#82)

The runners share the owner's PC with other CI jobs and agents. Two kinds of
noise show up, and the earlier one-run-per-side gate could not handle either:

- **Bursts.** A benchmark runs 1.3-3x slower for a few seconds while something
  else compiles, on either side. One HEAD run hit by a burst read as +224%.
- **Run-to-run spread.** One process run of the `num` add and mul benchmarks
  can be 10% faster or slower than the next, even for the same binary, probably
  from memory layout. A rule on the fastest round failed on one lucky base round.

Interleaving spreads slow drift evenly over both sides. The median of many
short rounds ignores anything that hits fewer than half the rounds of a side,
in either direction. The confirm pass covers what is left: a rare false flag
has to happen twice in a row, while a real slowdown is in every round of both
passes.

### Measured noise

Identical code on both sides, change of the median HEAD round against the
median base round, first pass:

| Setup | Runs | Failed | Worst change |
|---|---|---|---|
| Old gate (one run per side, criterion's confidence interval), runner | 12 PR runs | 8 | -65% to +224% |
| Fastest of 3 rounds per side, runner | 1 | 1 | +6.7% |
| Fastest of 3 rounds per side, dev PC | 1 | 1 | +5.1% |
| Median of 10 rounds + confirm pass, runner environment | 5 | 0 | -9.2% to +7.0% |
| Median of 10 rounds, dev PC (Windows) | 3 | 0 | -5.6% to +2.5% |

The old-gate row is the 12 PR bench jobs of 2026-11-17 before this change.
Every benchmark they flagged was in code the PR did not touch. The
runner-environment row is 2 CI jobs on this PR plus 3 runs inside the
`exitium-linux-2` container (same image and host, one-off scratch clone).
Two of those 5 runs flagged one benchmark in the first pass (`mul/100000`
+7.0%, `add/100000` +5.1%), and the confirm pass cleared both (-5.2%, +0.8%).

The first-pass noise floor is about **±9% for the `num` add and mul
benchmarks**, **±6% for `num` format** and **±3% for the profiler
benchmarks**. A 5% regression on add or mul can sit inside that floor, so the
gate can miss one there. It catches slowdowns of about 10% and more, and it no
longer fails on noise.

An injected slowdown (an extra pass over 10% of the elements in `num/add`
only) failed the gate on the dev PC: `add/1000` +13.6% (confirm +10.9%) and
`add/1000000` +7.3% (confirm +5.4%). Every `mul`, `format` and profiler
benchmark passed. `add/100000` read +3.6%: the extra elements were still in
cache, so the injected cost there was below 5%.

### How much to trust local results

Local `bench-ci` uses the same rule and, on the dev PC, showed less noise
than the runner (table above), so a local pass or fail is about as
trustworthy as CI's. On a busy machine expect `noise` lines and a confirm
pass. A first-pass change under about 10% on the `num` add and mul benchmarks
can be noise in either direction: only the confirmed result counts. For a
benchmark that sits near 5%, rerun `bench-ci` or raise the rounds with
`EXITIUM_BENCH_ROUNDS=20` (the confirm pass doubles it).

CI runs `bench-ci` against the PR's base commit on the dedicated
`exitium-bench` runner, one PR at a time. The job takes about 5 minutes, plus
about a minute when the confirm pass runs. Each new benchmark adds about 20 s.
