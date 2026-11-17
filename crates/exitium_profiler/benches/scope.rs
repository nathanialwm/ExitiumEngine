//! `profile_scope!` overhead benchmarks.
//!
//! Engine systems open a scope per system per frame, and some open one per
//! chunk or per item, so the macro sits on every hot path. Both cases run the
//! same per-item loop; `scoped` opens a scope on every iteration, `baseline`
//! does not.
//!
//! - With the `profiling` feature off (the default, and what the CI gate runs)
//!   the two must match: this is the zero-cost claim.
//! - With it on and no subscriber installed
//!   (`cargo bench -p exitium_profiler --features profiling`), `scoped` shows
//!   the cost of a disabled `tracing` callsite.
//!
//! Conventions are in `docs/benchmarks.md`.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use exitium_profiler::profile_scope;

const SIZES: [usize; 3] = [1_000, 100_000, 1_000_000];

/// Deterministic inputs from a 64-bit LCG (Knuth's MMIX constants).
fn inputs(len: usize) -> Vec<u64> {
    let mut state = 1u64;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        })
        .collect()
}

/// A little per-item work, so the loop is not a pure scope benchmark that the
/// optimizer could reduce to nothing in the baseline.
#[inline(always)]
fn step(acc: u64, x: u64) -> u64 {
    acc.rotate_left(5) ^ black_box(x)
}

fn bench_scope(c: &mut Criterion) {
    let mut group = c.benchmark_group("exitium_profiler/scope");
    for len in SIZES {
        let data = inputs(len);
        group.throughput(Throughput::Elements(len as u64));
        group.bench_function(BenchmarkId::new("baseline", len), |b| {
            b.iter(|| {
                let mut acc = 0u64;
                for &x in black_box(&data) {
                    acc = step(acc, x);
                }
                black_box(acc)
            });
        });
        group.bench_function(BenchmarkId::new("scoped", len), |b| {
            b.iter(|| {
                let mut acc = 0u64;
                for &x in black_box(&data) {
                    profile_scope!("exitium_profiler::bench");
                    acc = step(acc, x);
                }
                black_box(acc)
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_scope);
criterion_main!(benches);
