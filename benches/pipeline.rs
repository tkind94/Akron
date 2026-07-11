//! Core-pipeline benchmarks (TKI-58): parse -> normalize -> fingerprint ->
//! vocabulary -> shape/family clustering -> callrel, over the committed
//! fixtures at tests/fixtures. Fixed, deterministic input available in CI —
//! unlike the /tmp corpora, which are used only for wall-time measurement
//! (scripts/perf-pipeline.sh) and never touched inside `cargo bench`.

use akron::types::{Config, NormTree};
use akron::{callrel, cluster, family, fingerprint, normalize, parse, scan};
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::path::{Path, PathBuf};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn cfg() -> Config {
    Config {
        min_nodes: 25,
        wl_iters: 3,
        theta_clone: 0.60,
        theta_b: 0.55,
        theta_a_low: 0.30,
        theta_family: 0.35,
        theta_b_family: 0.16,
        top: 20,
    }
}

/// Every normalized function tree in the fixture set: the shared input for
/// the fingerprint bench, built once (untimed).
fn normalized_trees(files: &[PathBuf], root: &Path) -> Vec<NormTree> {
    let mut out = Vec::new();
    for f in files {
        let source = std::fs::read(f).unwrap();
        let tree = parse::parse(&source);
        let rel = f.strip_prefix(root).unwrap().display().to_string();
        let imports = normalize::collect_imports(tree.root_node(), &source, &rel);
        for occ in parse::extract_functions(&tree, &source, &rel) {
            out.push(normalize::normalize(occ.root, occ.func, &source, &imports).tree);
        }
    }
    out
}

/// parse -> collect_imports -> extract_functions -> normalize, the file
/// fan-out stage of `scan::scan_repo` (minus fingerprinting).
fn bench_parse_normalize(c: &mut Criterion) {
    let root = fixtures_root();
    let files = parse::python_files(&root);
    c.bench_function("parse_normalize", |b| {
        b.iter(|| {
            for f in &files {
                let source = std::fs::read(f).unwrap();
                let tree = parse::parse(&source);
                let rel = f.strip_prefix(&root).unwrap().display().to_string();
                let imports = normalize::collect_imports(tree.root_node(), &source, &rel);
                for occ in parse::extract_functions(&tree, &source, &rel) {
                    std::hint::black_box(normalize::normalize(
                        occ.root, occ.func, &source, &imports,
                    ));
                }
            }
        })
    });
}

/// WL histogram + Merkle root + MinHash over every normalized tree.
fn bench_fingerprint(c: &mut Criterion) {
    let root = fixtures_root();
    let files = parse::python_files(&root);
    let trees = normalized_trees(&files, &root);
    c.bench_function("fingerprint", |b| {
        b.iter(|| {
            for t in &trees {
                let wl = fingerprint::wl_histogram(t, 3);
                std::hint::black_box(fingerprint::merkle_root(t));
                std::hint::black_box(fingerprint::minhash(wl.iter().map(|&(l, _)| l)));
            }
        })
    });
}

/// tf-idf vocabulary index build (Channel B) over the fixture symbol set.
fn bench_vocab_build(c: &mut Criterion) {
    let root = fixtures_root();
    let cfg = cfg();
    let symbols = scan::scan_repo(&root, &cfg).symbols;
    c.bench_function("vocab_build", |b| {
        b.iter(|| std::hint::black_box(cluster::vocab_index(&symbols)))
    });
}

/// LSH candidate pairing + union-find clustering (Channel A, cluster.rs).
fn bench_shape_clustering(c: &mut Criterion) {
    let root = fixtures_root();
    let cfg = cfg();
    let symbols = scan::scan_repo(&root, &cfg).symbols;
    c.bench_function("shape_clustering", |b| {
        b.iter(|| std::hint::black_box(cluster::shape_clusters(&symbols, cfg.theta_clone)))
    });
}

/// Family assembly (family.rs): average-linkage agglomeration of shape
/// clusters, gated on the vocabulary-coherence and dunder-role guards.
fn bench_family_assemble(c: &mut Criterion) {
    let root = fixtures_root();
    let cfg = cfg();
    let symbols = scan::scan_repo(&root, &cfg).symbols;
    let vocab = cluster::vocab_index(&symbols);
    c.bench_function("family_assemble", |b| {
        b.iter_batched(
            || cluster::shape_clusters(&symbols, cfg.theta_clone),
            |mut shapes| {
                std::hint::black_box(family::assemble(
                    &symbols,
                    &mut shapes,
                    &vocab.vecs,
                    cfg.theta_family,
                    cfg.theta_b_family,
                ))
            },
            BatchSize::SmallInput,
        )
    });
}

/// Call-relation graph build (callrel.rs): the input to the competing
/// query's wrapper/callee suppression.
fn bench_callrel(c: &mut Criterion) {
    let root = fixtures_root();
    let cfg = cfg();
    let symbols = scan::scan_repo(&root, &cfg).symbols;
    c.bench_function("callrel_build", |b| {
        b.iter(|| std::hint::black_box(callrel::build(&symbols)))
    });
}

criterion_group!(
    benches,
    bench_parse_normalize,
    bench_fingerprint,
    bench_vocab_build,
    bench_shape_clustering,
    bench_family_assemble,
    bench_callrel,
);
criterion_main!(benches);
