//! End-to-end falsification harness on planted fixtures: a renamed clone
//! pair must land in one repeated cluster, and the sync/async proxy-fetcher
//! pair must surface as competing (same vocabulary, different shape).

use akron::types::{Config, SymbolPrint, SymbolRef};
use akron::{callrel, cluster, family, fingerprint, queries, scan};
use std::collections::HashSet;
use std::path::Path;

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

#[test]
fn planted_patterns_are_found() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cfg = cfg();
    let out = scan::scan_repo(&root, &cfg);
    let symbols = &out.symbols;

    let idx = |q: &str| {
        symbols
            .iter()
            .position(|s| s.sym.qname == q)
            .unwrap_or_else(|| {
                panic!(
                    "symbol {q} not extracted (have: {:?})",
                    symbols.iter().map(|s| &s.sym.qname).collect::<Vec<_>>()
                )
            }) as u32
    };
    let clone_a = idx("parse_records");
    let clone_b = idx("extract_rows");
    let comp_a = idx("fetch_page_with_proxy");
    let comp_b = idx("ProxyFetcher.fetch_page");

    // Renamed clone: normalization must make the fingerprints identical.
    assert_eq!(
        symbols[clone_a as usize].merkle_root, symbols[clone_b as usize].merkle_root,
        "identifier renaming and literal abstraction should yield identical Merkle roots"
    );

    let mut shapes = cluster::shape_clusters(symbols, cfg.theta_clone);

    // TKI-18: the repeated-shape funnel (cluster.rs's actual clustering path)
    // must be real pipeline measurements, not a reconstruction — and must
    // account for the planted near-miss (parse_records_v2) joining the
    // exact-clone pair via LSH + the cosine/representative-check stage.
    let rf = shapes.funnel;
    assert_eq!(rf.symbols_considered, symbols.len());
    assert!(
        rf.candidate_pairs > 0,
        "planted near-duplicates should surface LSH candidate pairs"
    );
    assert!(
        rf.survived_guards > 0 && rf.survived_guards <= rf.candidate_pairs,
        "the funnel must narrow monotonically: guards {} vs pairs {}",
        rf.survived_guards,
        rf.candidate_pairs
    );
    assert!(
        rf.survived_cosine >= 1 && rf.survived_cosine <= rf.survived_guards,
        "the near-miss variant should survive the cosine threshold and merge"
    );
    assert!(
        rf.clusters_formed >= 1,
        "the record-parser lineage should form at least one cluster"
    );

    let repeated = queries::repeated(symbols, &mut shapes);
    assert_eq!(
        rf.clusters_formed,
        repeated.len(),
        "the funnel's final stage must match the query's actual cluster count"
    );
    assert!(
        repeated
            .iter()
            .any(|c| c.members.contains(&clone_a) && c.members.contains(&clone_b)),
        "renamed clones should share a repeated cluster"
    );

    let vocab = cluster::vocab_index(symbols);
    let calls = callrel::build(symbols);
    let competing = queries::competing(symbols, &vocab, cfg.theta_b, cfg.theta_a_low, &calls);

    let found = competing
        .groups
        .iter()
        .any(|g| g.members.contains(&comp_a) && g.members.contains(&comp_b));
    if !found {
        let b_vec = |i: u32| -> Vec<(u64, f32)> {
            vocab.vecs[i as usize]
                .iter()
                .map(|&(id, w)| (id as u64, w))
                .collect()
        };
        let b = fingerprint::cosine(&b_vec(comp_a), &b_vec(comp_b));
        let a = fingerprint::cosine(&symbols[comp_a as usize].wl, &symbols[comp_b as usize].wl);
        panic!(
            "planted competing pair not found: B={b:.3} (θ_b={}), A={a:.3} (θ_a_low={}), \
             funnel: {} vocab pairs → {} cross-context → {} shape-divergent",
            cfg.theta_b,
            cfg.theta_a_low,
            competing.funnel.vocab_pairs,
            competing.funnel.cross_context,
            competing.funnel.low_shape
        );
    }

    // The clone pair shares vocabulary AND shape: it must NOT be reported
    // as competing (that would mean the A-filter is broken).
    assert!(
        competing
            .groups
            .iter()
            .all(|g| !(g.members.contains(&clone_a) && g.members.contains(&clone_b))),
        "identical clones must not be reported as competing"
    );

    // Wrapper/wrapped pair: cross-file, shares vocabulary, diverges in shape
    // — it would clear the high-B/low-A bar, but fetch_atlas_tile calls
    // fetch_tile directly, so it's a caller/callee relationship, not
    // competition (the wrapper-fixture pair, graded on a private corpus).
    let wrap_caller = idx("fetch_atlas_tile");
    let wrap_callee = idx("fetch_tile");
    assert!(
        competing
            .groups
            .iter()
            .all(|g| !(g.members.contains(&wrap_caller) && g.members.contains(&wrap_callee))),
        "caller/callee pair must not be reported as competing"
    );
    assert!(
        competing.funnel.call_related > 0,
        "wrapper pair should be counted as call-related in the funnel, funnel.call_related = {}",
        competing.funnel.call_related
    );

    // Constructor-wrapper pair: build_thumbnail constructs Thumbnail(...)
    // directly — a constructor call, syntactically indistinguishable from
    // calling a bare function named Thumbnail. It shares vocabulary with
    // Thumbnail.__init__ by construction and diverges in shape (a cache
    // lookup/branch __init__ doesn't have), so it must be suppressed the
    // same way a plain wrapper/wrapped pair is (TKI-21 constructor-call fix).
    let ctor_caller = idx("build_thumbnail");
    let ctor_callee = idx("Thumbnail.__init__");
    assert!(
        competing
            .groups
            .iter()
            .all(|g| !(g.members.contains(&ctor_caller) && g.members.contains(&ctor_callee))),
        "constructor caller/callee pair must not be reported as competing"
    );
    assert!(
        calls.related(ctor_caller, ctor_callee),
        "constructing a class should be call-related to its __init__"
    );
}

/// The graded family: an exact clone pair (parse_records / extract_rows) + a
/// near-miss (parse_records_v2, ~0.75) + a drifted variant (gather_records,
/// ~0.44), across four files, must assemble into ONE family — clones + near-
/// miss as the core, the drifted variant as drift — while the unrelated proxy
/// fetchers stay out. This is the whole point of the second altitude: at
/// theta_clone the drifted variant fragments off; the family altitude reunites
/// it without dragging in unrelated code.
#[test]
fn graded_family_assembles_with_drift_gradient() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cfg = cfg();
    let out = scan::scan_repo(&root, &cfg);
    let symbols = &out.symbols;
    let idx = |q: &str| {
        symbols
            .iter()
            .position(|s| s.sym.qname == q)
            .unwrap_or_else(|| panic!("symbol {q} not extracted")) as u32
    };
    let clone_a = idx("parse_records");
    let clone_b = idx("extract_rows");
    let nearmiss = idx("parse_records_v2");
    let drifted = idx("gather_records");
    let proxy_a = idx("fetch_page_with_proxy");
    let proxy_b = idx("ProxyFetcher.fetch_page");

    let mut shapes = cluster::shape_clusters(symbols, cfg.theta_clone);

    // The drifted variant must fall *below* theta_clone (so the tight altitude
    // leaves it out) — otherwise the family altitude proves nothing.
    let a_drift = fingerprint::cosine(&symbols[drifted as usize].wl, &symbols[clone_a as usize].wl);
    assert!(
        a_drift < cfg.theta_clone && a_drift > cfg.theta_family,
        "drifted variant should sit between theta_family and theta_clone (was {a_drift:.2})"
    );

    let vocab = cluster::vocab_index(symbols);
    let res = family::assemble(
        symbols,
        &mut shapes,
        &vocab.vecs,
        cfg.theta_family,
        cfg.theta_b_family,
    );
    let fam = res
        .families
        .iter()
        .find(|f| {
            let ids: HashSet<u32> = f.members.iter().map(|m| m.sym).collect();
            ids.contains(&clone_a) && ids.contains(&clone_b)
        })
        .expect("the clone pair should anchor an assembled family");

    let member = |s: u32| fam.members.iter().find(|m| m.sym == s).unwrap();
    let ids: HashSet<u32> = fam.members.iter().map(|m| m.sym).collect();

    // All four lineage members present; exact clones + near-miss are the core,
    // the drifted variant is drift.
    assert!(
        ids.contains(&nearmiss) && ids.contains(&drifted),
        "near-miss and drifted variant should both join the family"
    );
    assert!(
        member(clone_a).is_core && member(clone_b).is_core && member(nearmiss).is_core,
        "exact clones and the near-miss form the family core"
    );
    assert!(
        !member(drifted).is_core,
        "the ~0.44 variant is a drifted member, not core"
    );

    // Drift gradient: the drifted variant is farther from the core than the
    // near-miss, and drift members sort after core members.
    assert!(
        member(drifted).cos_to_core < member(nearmiss).cos_to_core,
        "drifted variant must be farther from core than the near-miss"
    );
    assert_eq!(
        fam.members.last().unwrap().sym,
        drifted,
        "the most-drifted member sorts last in the family readout"
    );
    let last_core = fam.members.iter().position(|m| !m.is_core).unwrap();
    assert!(
        fam.members[..last_core].iter().all(|m| m.is_core),
        "core members precede drift members in the readout"
    );

    // Unrelated proxy fetchers must NOT be pulled into the record-parser family.
    assert!(
        !ids.contains(&proxy_a) && !ids.contains(&proxy_b),
        "unrelated proxy fetchers must stay out of the record-parser family"
    );
}

/// Vocabulary-coherence gate (TKI-22): `mix_channels` shares the parser family's
/// generic loop/accumulate SHAPE — its Channel-A cosine to the core sits in the
/// same drift band as the genuine `gather_records` variant, so Channel A alone
/// would pull it in — but its vocabulary is disjoint audio-DSP domain. The gate
/// must reject it (funnel `guard_rejected` ≥ 1) while the vocabulary-sharing
/// `gather_records` still familizes. This is the scrapy blob in miniature: same
/// shape, disjoint vocabulary must NOT familize; the graded lineage must.
#[test]
fn generic_shape_with_disjoint_vocab_is_gated_out() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cfg = cfg();
    let out = scan::scan_repo(&root, &cfg);
    let symbols = &out.symbols;
    let idx = |q: &str| {
        symbols
            .iter()
            .position(|s| s.sym.qname == q)
            .unwrap_or_else(|| panic!("symbol {q} not extracted")) as u32
    };
    let clone_a = idx("parse_records");
    let drifted = idx("gather_records");
    let interloper = idx("mix_channels");

    // The interloper is a Channel-A merge candidate: A-drift into the family band
    // (≥ theta_family so it seeds an edge) yet below theta_clone (stays a singleton
    // unit, so only the family-altitude B gate — not tight clustering — can hold
    // it out). If this band assertion fails the fixture no longer tests the gate.
    let a = fingerprint::cosine(
        &symbols[interloper as usize].wl,
        &symbols[clone_a as usize].wl,
    );
    assert!(
        a > cfg.theta_family && a < cfg.theta_clone,
        "interloper must A-drift into the merge-candidate band (was {a:.2})"
    );

    let mut shapes = cluster::shape_clusters(symbols, cfg.theta_clone);
    let vocab = cluster::vocab_index(symbols);
    let res = family::assemble(
        symbols,
        &mut shapes,
        &vocab.vecs,
        cfg.theta_family,
        cfg.theta_b_family,
    );

    // The gate fired, and it is visible in the funnel.
    assert!(
        res.funnel.guard_rejected >= 1,
        "the vocabulary gate should report ≥1 rejected merge (was {})",
        res.funnel.guard_rejected
    );

    // No assembled family contains the disjoint-vocab interloper …
    for fam in &res.families {
        assert!(
            fam.members.iter().all(|m| m.sym != interloper),
            "generic-shape, disjoint-vocab `mix_channels` must not familize"
        );
    }
    // … while the vocabulary-sharing graded variant still does.
    assert!(
        res.families.iter().any(|f| {
            let ids: HashSet<u32> = f.members.iter().map(|m| m.sym).collect();
            ids.contains(&clone_a) && ids.contains(&drifted)
        }),
        "the vocabulary-sharing gather_records lineage must still familize"
    );
}

/// Blob guard: no assembled family may contain BOTH the parse_records lineage
/// and the proxy-fetcher lineage. These share almost no structure (A ~0.05),
/// so a family spanning both would mean the altitude cut or a chaining bug let
/// unrelated code fuse — the phase-0 blob, one altitude up.
#[test]
fn no_family_blends_parse_and_proxy_lineages() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cfg = cfg();
    let out = scan::scan_repo(&root, &cfg);
    let symbols = &out.symbols;

    let parse_lineage = [
        "parse_records",
        "extract_rows",
        "parse_records_v2",
        "gather_records",
    ];
    let proxy_lineage = [
        "fetch_page_with_proxy",
        "ProxyFetcher.fetch_page",
        "fetch_tile",
        "fetch_atlas_tile",
    ];

    let mut shapes = cluster::shape_clusters(symbols, cfg.theta_clone);
    let vocab = cluster::vocab_index(symbols);
    let res = family::assemble(
        symbols,
        &mut shapes,
        &vocab.vecs,
        cfg.theta_family,
        cfg.theta_b_family,
    );

    for fam in &res.families {
        let names: HashSet<&str> = fam
            .members
            .iter()
            .map(|m| symbols[m.sym as usize].sym.qname.as_str())
            .collect();
        let has_parse = parse_lineage.iter().any(|q| names.contains(q));
        let has_proxy = proxy_lineage.iter().any(|q| names.contains(q));
        assert!(
            !(has_parse && has_proxy),
            "family blob mixes parse and proxy lineages: {names:?}"
        );
    }
}

/// TKI-33: the F4 bug in miniature (`tests/fixtures/role_guard_classes.py`).
/// `AlphaSuite`/`BetaSuite` write byte-identical `__init__`/`run` methods (an
/// exact-clone core per role); `GammaSuite` drifts both just enough to join
/// each role's family without tight-clustering directly. Both roles read
/// alike at theta_family and share enough field vocabulary to clear the
/// Channel-B gate on their own — so only the dunder role guard can keep
/// `__init__` and `run` apart, exactly as it must on corpus-R's F4 grading case.
#[test]
fn dunder_role_guard_keeps_init_and_run_families_apart() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cfg = cfg();
    let out = scan::scan_repo(&root, &cfg);
    let symbols = &out.symbols;
    let idx = |q: &str| {
        symbols
            .iter()
            .position(|s| s.sym.qname == q)
            .unwrap_or_else(|| panic!("symbol {q} not extracted")) as u32
    };

    let inits: HashSet<u32> = ["AlphaSuite.__init__", "BetaSuite.__init__", "GammaSuite.__init__"]
        .into_iter()
        .map(idx)
        .collect();
    let runs: HashSet<u32> = ["AlphaSuite.run", "BetaSuite.run", "GammaSuite.run"]
        .into_iter()
        .map(idx)
        .collect();

    let mut shapes = cluster::shape_clusters(symbols, cfg.theta_clone);
    let vocab = cluster::vocab_index(symbols);
    let res = family::assemble(
        symbols,
        &mut shapes,
        &vocab.vecs,
        cfg.theta_family,
        cfg.theta_b_family,
    );

    // The guard actually fired — this is not a coincidence of the thresholds.
    assert!(
        res.funnel.role_rejected >= 1,
        "the dunder role guard should report \u{2265}1 rejected merge (was {})",
        res.funnel.role_rejected
    );

    let init_family = res
        .families
        .iter()
        .find(|f| f.members.iter().any(|m| inits.contains(&m.sym)))
        .expect("the three __init__ methods should assemble into a family");
    let run_family = res
        .families
        .iter()
        .find(|f| f.members.iter().any(|m| runs.contains(&m.sym)))
        .expect("the three run methods should assemble into a family");

    let init_ids: HashSet<u32> = init_family.members.iter().map(|m| m.sym).collect();
    let run_ids: HashSet<u32> = run_family.members.iter().map(|m| m.sym).collect();
    assert_eq!(
        init_ids, inits,
        "the init family must contain exactly the three __init__ methods, no run members"
    );
    assert_eq!(
        run_ids, runs,
        "the run family must contain exactly the three run methods, no init members"
    );

    // No assembled family may mix a dunder member with a non-dunder member —
    // the general form of the guard, not just this one pair.
    for fam in &res.families {
        let has_init = fam.members.iter().any(|m| inits.contains(&m.sym));
        let has_run = fam.members.iter().any(|m| runs.contains(&m.sym));
        assert!(
            !(has_init && has_run),
            "a family mixes __init__ and run members: the role guard failed"
        );
    }
}

/// A hand-built symbol with a unique Channel-A label (disjoint from every
/// other synth symbol, so pairwise cosine is exactly 0) but an identical
/// MinHash signature (so it lands in the same LSH bucket as every other synth
/// symbol, guaranteeing real candidate pairs). Distinct file per symbol keeps
/// the nesting guard out of the way; equal node_count keeps the size-ratio
/// guard satisfied.
fn synth_symbol(idx: u32, merkle_root: u64) -> SymbolPrint {
    SymbolPrint {
        sym: SymbolRef {
            file: format!("synth_{idx}.py"),
            qname: format!("func_{idx}"),
            line: 1,
        },
        span: (0, 0),
        node_count: 50,
        merkle_root,
        wl: vec![(idx as u64, 1.0)],
        minhash: vec![7u64; fingerprint::MINHASH_FNS],
        vocab_tf: Default::default(),
        calls: Default::default(),
        is_test: false,
        dating: None,
    }
}

/// TKI-18: the repeated-shape funnel must explain a fully empty result rather
/// than reporting a silent zero. Four synth symbols share an LSH bucket
/// (identical MinHash) — so real candidate pairs are generated and survive
/// the size/nesting guards — but their Channel-A vectors are pairwise
/// disjoint (cosine 0 < theta_clone) and their Merkle roots are distinct (no
/// exact clones), so nothing merges and zero clusters form. The funnel must
/// still show the real upstream counts, not just "0 clusters".
#[test]
fn repeated_funnel_explains_a_fully_empty_result() {
    let cfg = cfg();
    let symbols: Vec<SymbolPrint> = (0..4).map(|i| synth_symbol(i, (i + 1) as u64)).collect();

    let mut shapes = cluster::shape_clusters(&symbols, cfg.theta_clone);
    let rf = shapes.funnel;

    assert_eq!(rf.symbols_considered, 4);
    assert_eq!(rf.oversized_buckets, 0);
    assert_eq!(
        rf.candidate_pairs, 6,
        "4 symbols sharing one LSH bucket should yield C(4,2)=6 unique candidate pairs"
    );
    assert_eq!(
        rf.survived_guards, 6,
        "equal-size, different-file pairs should all survive the size/nesting guards"
    );
    assert_eq!(
        rf.survived_cosine, 0,
        "pairwise-disjoint Channel-A vectors must not clear the cosine threshold"
    );
    assert_eq!(rf.clusters_formed, 0, "no merges means no clusters form");

    let repeated = queries::repeated(&symbols, &mut shapes);
    assert!(
        repeated.is_empty(),
        "no clusters should be reported for pairwise-disjoint synth symbols"
    );
}
