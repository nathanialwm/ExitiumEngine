//! `Num` arithmetic and formatting benchmarks.
//!
//! Every `Num` operation sends its `f64` result through the clamping
//! `Num::from_f64`, so these measure that overhead on the two hottest
//! operations in an incremental game: adding income (`add`) and applying
//! multipliers (`mul`). Each case is an element-wise pass over slices, the
//! shape of a per-entity resource update.
//!
//! The `format_*` cases write each value into one reused `String` with
//! `Num::format_into`, the way a UI refreshes its counter labels every frame.
//! They run at 1k values only: a frame shows a few thousand numbers at most,
//! and formatting one costs far more than reading it, so larger slices would
//! measure the same per-value cost while criterion's sampling stretched each
//! run to minutes. Plain notation is left out because it writes every digit,
//! so its cost grows with the value, and counters do not use it for large
//! values.
//!
//! Conventions are in `docs/benchmarks.md`.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use exitium_core::{Notation, Num, NumFormat};

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

/// Deterministic inputs whose exponents are spread evenly from 10^0 to
/// 10^300, the range an incremental game's counters cross. One in a hundred
/// is below 1000 and takes the plain path.
fn spread_inputs(len: usize, seed: u64) -> Vec<Num> {
    inputs(len, seed)
        .into_iter()
        .map(|x| Num::from_f64(10f64.powf((x.as_f64() - 0.5) * 300.0)))
        .collect()
}

fn format(c: &mut Criterion) {
    const LEN: usize = 1_000;
    let values = spread_inputs(LEN, 3);
    let mut text = String::new();
    let mut group = c.benchmark_group("exitium_core/num");
    group.throughput(Throughput::Elements(LEN as u64));
    for (name, notation) in [
        ("format_scientific", Notation::Scientific),
        ("format_engineering", Notation::Engineering),
        ("format_suffix", Notation::Suffix),
        ("format_letter", Notation::Letter),
    ] {
        // Two digits after the point for large values and none below 1000,
        // as a counter shows them.
        let format = NumFormat {
            notation,
            precision: 2,
            small_precision: 0,
        };
        group.bench_function(BenchmarkId::new(name, LEN), |b| {
            b.iter(|| {
                for value in black_box(&values) {
                    text.clear();
                    value.format_into(&format, &mut text).unwrap();
                    black_box(text.as_str());
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, add, mul, format);
criterion_main!(benches);
