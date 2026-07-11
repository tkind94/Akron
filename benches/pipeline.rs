//! Core-pipeline benchmarks (TKI-58): parse → normalize → fingerprint →
//! vocabulary → family/callrel. Placeholder until the perf harness lands.

use criterion::{criterion_group, criterion_main, Criterion};

fn placeholder(c: &mut Criterion) {
    c.bench_function("placeholder", |b| b.iter(|| std::hint::black_box(0)));
}

criterion_group!(benches, placeholder);
criterion_main!(benches);
