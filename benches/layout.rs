//! Layout/compute benchmarks (TKI-59): kNN graph, force layout, overlap
//! relax, sublayout drill, PCA, indegree. Placeholder until the perf
//! harness lands.

use criterion::{criterion_group, criterion_main, Criterion};

fn placeholder(c: &mut Criterion) {
    c.bench_function("placeholder", |b| b.iter(|| std::hint::black_box(0)));
}

criterion_group!(benches, placeholder);
criterion_main!(benches);
