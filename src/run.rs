//! The full analysis pipeline, factored out of `main` so `akron scan`,
//! `explain`, and `explore` all compute from *one* deterministic engine.
//! Finding refs (`R1`, `F1`, …) are positions in this pipeline's output, so
//! every consumer must run the identical sequence the report prints —
//! otherwise a ref could resolve to a different finding than the one the
//! user saw. This module is that guarantee.
//!
//! Pure analysis only: no IO lives here, keeping the functional core free of
//! side effects.

use crate::cluster::{ShapeClusters, VocabIndex};
use crate::family::FamilyResult;
use crate::queries::{CompetingResult, DeprecatedResult, RepeatedCluster};
use crate::report::Stats;
use crate::scan::ScanOutput;
use crate::types::Config;
use crate::{callrel, cluster, family, queries, scan};
use std::path::Path;
use std::time::{Duration, Instant};

/// Per-phase wall time, for `scan --timings`.
pub struct Timings {
    pub shape: Duration,
    pub repeated: Duration,
    pub vocab: Duration,
    pub family: Duration,
    pub calls: Duration,
    pub competing: Duration,
    pub deprecated: Duration,
}

/// Everything the report/explain/explore layers read, from one scan.
pub struct Analysis {
    pub scanned: ScanOutput,
    pub shapes: ShapeClusters,
    pub repeated: Vec<RepeatedCluster>,
    pub vocab: VocabIndex,
    pub families: FamilyResult,
    pub competing: CompetingResult,
    pub deprecated: DeprecatedResult,
    pub stats: Stats,
    pub timings: Timings,
}

pub fn analyze(root: &Path, cfg: &Config) -> Analysis {
    let scanned = scan::scan_repo(root, cfg);

    // shape_clusters and vocab_index each read only `scanned.symbols`
    // immutably and write no shared state (Channel A and Channel B are
    // independent projections, DESIGN.md §2), so running them concurrently
    // cannot change either's output — the same guarantee family.rs's
    // `theta_edges` already relies on for its own rayon fan-out.
    let ((mut shapes, shape), (vocab, t_vocab)) = rayon::join(
        || {
            let t = Instant::now();
            let r = cluster::shape_clusters(&scanned.symbols, cfg.theta_clone);
            (r, t.elapsed())
        },
        || {
            let t = Instant::now();
            let r = cluster::vocab_index(&scanned.symbols);
            (r, t.elapsed())
        },
    );

    let t = Instant::now();
    let repeated = queries::repeated(&scanned.symbols, &mut shapes);
    let t_repeated = t.elapsed();

    // family::assemble and callrel::build are also mutually independent
    // (callrel::build reads only `scanned.symbols`), but measured: pairing
    // them via rayon::join bought nothing (callrel::build is ~3ms — the
    // wrapper's own internal `theta_edges` rayon fan-out is already the
    // heavy work) and cost real time — family assembly's own phase slowed
    // ~30-40% under this box's load, apparently from an outer rayon::join
    // contending with the inner one for worker threads. Not worth it; left
    // sequential.
    let t = Instant::now();
    let families = family::assemble(
        &scanned.symbols,
        &mut shapes,
        &vocab.vecs,
        cfg.theta_family,
        cfg.theta_b_family,
    );
    let t_family = t.elapsed();

    let t = Instant::now();
    let calls = callrel::build(&scanned.symbols);
    let t_calls = t.elapsed();

    let t = Instant::now();
    let competing = queries::competing(
        &scanned.symbols,
        &vocab,
        cfg.theta_b,
        cfg.theta_a_low,
        &calls,
    );
    let t_competing = t.elapsed();

    let t = Instant::now();
    let deprecated = match scanned.history.as_ref() {
        Some(h) => queries::deprecated_candidates(
            &scanned.symbols,
            &repeated,
            &vocab,
            h.anchor,
            cfg.theta_b,
        ),
        None => DeprecatedResult {
            candidates: Vec::new(),
            funnel: Default::default(),
        },
    };
    let t_deprecated = t.elapsed();

    let stats = Stats {
        files: scanned.files,
        symbols: scanned.symbols.len(),
        skipped_small: scanned.skipped_small,
        repeated_funnel: shapes.funnel,
    };

    Analysis {
        scanned,
        shapes,
        repeated,
        vocab,
        families,
        competing,
        deprecated,
        stats,
        timings: Timings {
            shape,
            repeated: t_repeated,
            vocab: t_vocab,
            family: t_family,
            calls: t_calls,
            competing: t_competing,
            deprecated: t_deprecated,
        },
    }
}
