//! Layout/compute benchmarks (TKI-59): kNN graph build, deterministic force
//! layout, overlap relax, and PCA — the four hot paths behind `explore`'s
//! boot and its `/api/sublayout` drill (which re-runs kNN → force → relax
//! over a path-prefix subset). Inputs are synthetic and seeded from a fixed
//! LCG (Knuth's MMIX constants, same style as `layout.rs`/`pca.rs`
//! themselves) — no corpus, no clock, no OS RNG, so `cargo bench` never
//! touches `/tmp/akron-corpora` and every run starts from the same numbers.

use akron::{layout, pca};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

/// Fixed LCG (Knuth MMIX constants) seeded by an arbitrary but constant
/// value — identical sequence on every run, on every machine.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2545_F491_4F6C_DD1D)
    }
    /// Next value in [0, 1).
    fn next01(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as f64 / (1u64 << 31) as f64
    }
}

/// `n` random unit d-vectors — the shape real embeddings take (L2-normalized,
/// so `layout::knn`'s plain dot is cosine similarity).
fn synth_embeddings(n: usize, d: usize, seed: u64) -> Vec<Vec<f32>> {
    let mut rng = Lcg::new(seed);
    (0..n)
        .map(|_| {
            let mut v: Vec<f32> = (0..d).map(|_| (rng.next01() as f32) * 2.0 - 1.0).collect();
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut v {
                    *x /= norm;
                }
            }
            v
        })
        .collect()
}

/// Embedding dimension of the shipping model (embeddinggemma-300m) — the
/// benches match production width so the per-pair dot product cost is
/// representative, not just the row count.
const DIM: usize = 768;

fn bench_knn(c: &mut Criterion) {
    let mut group = c.benchmark_group("knn_build");
    for &n in &[200usize, 1000usize] {
        let embeddings = synth_embeddings(n, DIM, 0xA5A5_1234);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| layout::knn(std::hint::black_box(&embeddings), layout::KNN_K));
        });
    }
    group.finish();
}

fn bench_force_layout(c: &mut Criterion) {
    let mut group = c.benchmark_group("force_layout");
    for &n in &[200usize, 1000usize] {
        let embeddings = synth_embeddings(n, DIM, 0xB6B6_5678);
        let neighbors = layout::knn(&embeddings, layout::KNN_K);
        let mut rng = Lcg::new(0xC7C7_9ABC);
        let init: Vec<(f32, f32)> = (0..n)
            .map(|_| (rng.next01() as f32, rng.next01() as f32))
            .collect();
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                layout::layout(
                    std::hint::black_box(&embeddings),
                    std::hint::black_box(&neighbors),
                    std::hint::black_box(&init),
                )
            });
        });
    }
    group.finish();
}

/// Synthetic points with deliberate overlap (stacks of identical/near-
/// identical positions), the case `relax` exists to resolve — mirrors real
/// point clouds where clone groups and near-duplicates land on top of each
/// other after the force pass.
fn synth_overlapping_points(n: usize, seed: u64) -> (Vec<(f32, f32)>, Vec<f32>) {
    let mut rng = Lcg::new(seed);
    let stacks = (n / 20).max(1); // ~20 points per stack, some genuine overlap
    let pos: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let stack = (i % stacks) as f32;
            let jitter_x = (rng.next01() as f32 - 0.5) * 0.002;
            let jitter_y = (rng.next01() as f32 - 0.5) * 0.002;
            let base = stack / stacks as f32;
            (base + jitter_x, base + jitter_y)
        })
        .collect();
    let radii = vec![0.004f32; n];
    (pos, radii)
}

fn bench_relax(c: &mut Criterion) {
    let mut group = c.benchmark_group("relax");
    for &n in &[200usize, 1000usize] {
        let (pos, radii) = synth_overlapping_points(n, 0xD8D8_DEF0);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched(
                || pos.clone(),
                |mut p| layout::relax(std::hint::black_box(&mut p), std::hint::black_box(&radii)),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_pca(c: &mut Criterion) {
    let mut group = c.benchmark_group("pca");
    for &n in &[200usize, 1000usize] {
        let rows = synth_embeddings(n, DIM, 0xE9E9_1357);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| pca::pca(std::hint::black_box(&rows), 8));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_knn, bench_force_layout, bench_relax, bench_pca);
criterion_main!(benches);
