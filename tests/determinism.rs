//! TKi-19: running the full pipeline twice, in-process, on the same input
//! must produce byte-identical JSON. The vocab tf-idf build used to sum
//! float weights in HashMap iteration order (a fresh random seed per
//! process/HashMap instance), giving a 1-ULP wobble in competing b_max
//! across runs; several sorts over f32 keys also had unbroken ties whose
//! resolution leaned on HashMap insertion order. Both are fixed in
//! cluster.rs/queries.rs — this test is the falsification harness for that.

use akron::report::{self, Stats};
use akron::types::Config;
use akron::{callrel, cluster, family, queries, scan};
use std::path::Path;

fn cfg() -> Config {
    Config {
        min_nodes: 25,
        wl_iters: 3,
        theta_clone: 0.60,
        theta_b: 0.55,
        theta_a_low: 0.30,
        theta_family: 0.35,
        theta_b_family: family::THETA_B_FAMILY,
        top: 20,
    }
}

fn run_once(root: &Path, cfg: &Config, only: Option<report::Section>) -> serde_json::Value {
    let scanned = scan::scan_repo(root, cfg);
    let symbols = &scanned.symbols;

    let mut shapes = cluster::shape_clusters(symbols, cfg.theta_clone);
    let repeated = queries::repeated(symbols, &mut shapes);
    let vocab = cluster::vocab_index(symbols);
    let families = family::assemble(
        symbols,
        &mut shapes,
        &vocab.vecs,
        cfg.theta_family,
        cfg.theta_b_family,
    );
    let calls = callrel::build(symbols);
    let competing = queries::competing(symbols, &vocab, cfg.theta_b, cfg.theta_a_low, &calls);
    let history = scanned.history.as_ref();
    let deprecated = match history {
        Some(h) => {
            queries::deprecated_candidates(symbols, &repeated, &vocab, h.anchor, cfg.theta_b)
        }
        None => queries::DeprecatedResult {
            candidates: Vec::new(),
            funnel: Default::default(),
        },
    };

    let stats = Stats {
        files: scanned.files,
        symbols: symbols.len(),
        skipped_small: scanned.skipped_small,
        repeated_funnel: shapes.funnel,
    };

    report::json_report(
        root,
        &stats,
        symbols,
        &repeated,
        &families,
        &competing,
        &deprecated,
        history,
        cfg,
        only,
    )
}

#[test]
fn two_in_process_runs_are_byte_identical() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cfg = cfg();

    // `--full` is gone (TKI-50): `--only` is the sole way to populate
    // `families`/`competing`, so exercise determinism at every scope a real
    // `scan --json` run can produce — including `competing`, whose `b_max`
    // float-tie wobble is exactly what this test was written to falsify.
    for only in [
        None,
        Some(report::Section::Families),
        Some(report::Section::Competing),
    ] {
        let a = run_once(&root, &cfg, only);
        let b = run_once(&root, &cfg, only);

        let ba = serde_json::to_vec_pretty(&a).unwrap();
        let bb = serde_json::to_vec_pretty(&b).unwrap();
        assert_eq!(
            ba, bb,
            "two in-process scans of the same repo must produce byte-identical JSON (only={only:?})"
        );
    }
}
