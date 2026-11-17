//! `Num` arithmetic benchmarks.
//!
//! Every `Num` operation sends its `f64` result through the clamping
//! `Num::from_f64`, so these measure that overhead on the two hottest
//! operations in an incremental game: adding income (`add`) and applying
//! multipliers (`mul`). Each case is an element-wise pass over slices, the
//! shape of a per-entity resource update. Formatting benchmarks follow in #17.
//!
//! Conventions are in `docs/benchmarks.md`.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use exitium_core::Num;

const SIZES: [usize; 3] = [1_000, 100_000, 1_000_000];

/// Deterministic inputs between 0.5 and 1.5, so products stay near 1 and
/// never saturate: the benchmark measures the common path, not the clamp.
fn inputs(len: usize, seed: u64) -> Vec<Num> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            // 64-bit LCG (Knuth's MMIX constants); the top 53 bits become a
            // fraction in [0, 1).
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            Num::from_f64(0.5 + (state >> 11) as f64 / (1u64 << 53) as f64)
        })
        .collect()
}

/// Generic over `op` so the operation is inlined into the loop, as game code
/// calling `+` or `*` would be, rather than called through a pointer.
fn bench_binary_op(c: &mut Criterion, name: &str, op: impl Fn(Num, Num) -> Num) {
    let mut group = c.benchmark_group("exitium_core/num");
    for len in SIZES {
        let lhs = inputs(len, 1);
        let rhs = inputs(len, 2);
        let mut out = vec![Num::ZERO; len];
        group.throughput(Throughput::Elements(len as u64));
        group.bench_function(BenchmarkId::new(name, len), |b| {
            b.iter(|| {
                for ((o, &l), &r) in out.iter_mut().zip(black_box(&lhs)).zip(black_box(&rhs)) {
                    *o = op(l, r);
                }
                black_box(&mut out);
            });
        });
    }
    group.finish();
}

fn add(c: &mut Criterion) {
    bench_binary_op(c, "add", |a, b| a + b);
}

fn mul(c: &mut Criterion) {
    bench_binary_op(c, "mul", |a, b| a * b);
}

criterion_group!(benches, add, mul);
criterion_main!(benches);
