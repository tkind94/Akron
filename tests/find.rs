//! `akron find` (TKI-41 / EXP-A port of `R&D archive spike/find`). Coverage split the
//! way the acceptance gate asks:
//!  (a) pure logic — ranking math, is_test dropping, cache round-trip,
//!      repo-key derivation, doc-key content-addressing — none of this
//!      needs the network or the model, so it always runs under `cargo
//!      test`;
//!  (b) one end-to-end run behind `#[ignore]`, exercised manually with the
//!      real model present: a real query against a tiny fixture repo, and
//!      the isolation invariant that a `find` run creates nothing under the
//!      scanned repo's `.akron/`.

use akron::find;
use akron::types::{SymbolPrint, SymbolRef};
use std::collections::{HashMap, HashSet};
use std::path::Path;

fn mk_symbol(qname: &str, file: &str, is_test: bool) -> SymbolPrint {
    SymbolPrint {
        sym: SymbolRef {
            file: file.to_string(),
            qname: qname.to_string(),
            line: 1,
        },
        span: (0, 0),
        node_count: 0,
        merkle_root: 0,
        wl: Vec::new(),
        minhash: Vec::new(),
        vocab_tf: HashMap::new(),
        calls: HashSet::new(),
        is_test,
        dating: None,
    }
}

// ── ranking math ──

#[test]
fn rank_orders_by_cosine_descending_and_truncates_to_top() {
    // Pre-normalized vectors (as `embed.rs` always stores them), so cosine
    // is a plain dot product. query=(1,0): id0 is an exact match (1.0),
    // id2 is a 45-degree match (~0.707), id1 is orthogonal (0.0).
    let query = [1.0_f32, 0.0];
    let v0 = [1.0_f32, 0.0];
    let v1 = [0.0_f32, 1.0];
    let v2 = [std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2];
    let candidates: Vec<(usize, &[f32])> = vec![(1, &v1), (0, &v0), (2, &v2)];

    let top2 = find::rank(&query, &candidates, 2);
    assert_eq!(top2.len(), 2, "truncates to the requested top-N");
    assert_eq!(top2[0].0, 0, "the exact match ranks first");
    assert!((top2[0].1 - 1.0).abs() < 1e-6);
    assert_eq!(top2[1].0, 2, "the 45-degree match ranks second, ahead of orthogonal");
    assert!((top2[1].1 - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
}

#[test]
fn rank_breaks_exact_ties_on_symbol_index_not_hashmap_order() {
    let query = [1.0_f32, 0.0];
    let same = [1.0_f32, 0.0];
    // Deliberately inserted out of index order — a HashMap-iteration-order
    // bug would make this test flaky across runs; a fixed tie-break must not.
    let candidates: Vec<(usize, &[f32])> = vec![(5, &same), (2, &same), (9, &same)];
    let ranked = find::rank(&query, &candidates, 3);
    let order: Vec<usize> = ranked.iter().map(|&(i, _)| i).collect();
    assert_eq!(order, vec![2, 5, 9], "equal scores break on ascending symbol index");
}

// ── is_test dropping (RESULTS.md's measured lever: full exclusion) ──

#[test]
fn ranked_symbol_ids_drops_test_symbols_by_default() {
    let symbols = vec![
        mk_symbol("app_db.engine.connect", "engine.py", false),
        mk_symbol("test_engine.test_connect", "test_engine.py", true),
        mk_symbol("app_db.engine.close", "engine.py", false),
    ];
    let ids = find::ranked_symbol_ids(&symbols, false);
    assert_eq!(ids, vec![0, 2], "is_test symbols are dropped, not merely downranked");
}

#[test]
fn ranked_symbol_ids_includes_tests_when_asked() {
    let symbols = vec![
        mk_symbol("app_db.engine.connect", "engine.py", false),
        mk_symbol("test_engine.test_connect", "test_engine.py", true),
    ];
    let ids = find::ranked_symbol_ids(&symbols, true);
    assert_eq!(ids, vec![0, 1], "--tests opts test symbols back into the ranking");
}

// ── cache keying: hash of the FULL doc text, not the unit's Merkle root ──

#[test]
fn doc_key_is_stable_for_identical_text() {
    let text = find::doc_text("app_db.engine.connect", "engine.py", "def connect():\n    ...");
    assert_eq!(find::doc_key(&text), find::doc_key(&text));
}

#[test]
fn doc_key_changes_on_rename_even_with_identical_body() {
    // Same Merkle root (identical source), different qname — the VP's
    // correction: keying by Merkle root would serve a stale vector under
    // the old name after a rename; keying by the full qualified text
    // (qname+path+source) re-embeds instead.
    let body = "def connect():\n    return psycopg.connect(dsn)";
    let before = find::doc_text("app_db.engine.connect", "engine.py", body);
    let after = find::doc_text("app_db.engine.open_connection", "engine.py", body);
    assert_ne!(
        find::doc_key(&before),
        find::doc_key(&after),
        "a rename must produce a different cache key so the renamed symbol re-embeds"
    );
}

#[test]
fn doc_key_is_a_lowercase_hex_string() {
    let key = find::doc_key("anything");
    assert_eq!(key.len(), 16);
    assert!(key.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

// ── embedding cache: serialization round-trip (no model needed) ──

fn full_width(seed: f32) -> Vec<f32> {
    (0..find::EMBED_DIM).map(|i| seed + i as f32).collect()
}

#[test]
fn emb_cache_round_trips_through_disk() {
    let dir = std::env::temp_dir().join(format!("akron-find-cache-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("index.bin");

    let mut cache = find::EmbCache::default();
    cache.map.insert("aaaa".to_string(), full_width(0.1));
    cache.map.insert("bbbb".to_string(), full_width(-1.0));
    cache.save(&path).expect("save");

    let loaded = find::EmbCache::load(&path);
    assert_eq!(loaded.map.len(), 2);
    assert_eq!(loaded.map["aaaa"], full_width(0.1));
    assert_eq!(loaded.map["bbbb"], full_width(-1.0));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emb_cache_load_drops_wrong_width_vectors() {
    let dir =
        std::env::temp_dir().join(format!("akron-find-cache-dim-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("index.bin");

    let mut cache = find::EmbCache::default();
    cache.map.insert("good".to_string(), full_width(0.5));
    cache.map.insert("short".to_string(), vec![0.1, 0.2, 0.3]);
    cache.map.insert("long".to_string(), vec![0.0; find::EMBED_DIM + 1]);
    cache.save(&path).expect("save");

    // A wrong-width vector must never reach ranking: `dot` zips and would
    // silently truncate to the shorter vector, scoring garbage. Dropping it
    // at load means the symbol just re-embeds.
    let loaded = find::EmbCache::load(&path);
    assert_eq!(loaded.map.len(), 1);
    assert!(loaded.map.contains_key("good"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emb_cache_load_of_missing_file_is_empty_not_an_error() {
    let path = Path::new("/nonexistent/akron-find-cache-that-does-not-exist/index.bin");
    let cache = find::EmbCache::load(path);
    assert!(cache.map.is_empty());
}

#[test]
fn emb_cache_load_of_corrupted_file_is_empty_not_an_error() {
    let dir =
        std::env::temp_dir().join(format!("akron-find-cache-corrupt-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("index.bin");
    std::fs::write(&path, b"{ this is not json").unwrap();

    let cache = find::EmbCache::load(&path);
    assert!(cache.map.is_empty(), "corrupted cache self-heals as empty");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── repo-key derivation (pure; drives the cache path) ──

#[test]
fn repo_key_is_deterministic_for_the_same_path() {
    let root = Path::new("/Users/someone/dev/some-repo");
    assert_eq!(find::repo_key(root), find::repo_key(root));
}

#[test]
fn repo_key_differs_across_paths() {
    let a = find::repo_key(Path::new("/Users/someone/dev/repo-a"));
    let b = find::repo_key(Path::new("/Users/someone/dev/repo-b"));
    assert_ne!(a, b);
}

#[test]
fn repo_key_is_lowercase_hex() {
    let key = find::repo_key(Path::new("/tmp/whatever"));
    assert_eq!(key.len(), 16);
    assert!(key.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn index_path_never_lives_under_the_scanned_repos_own_tree() {
    let root = Path::new("/Users/someone/dev/some-repo");
    let path = find::index_path(root);
    assert!(
        !path.starts_with(root),
        "the embedding cache must live under $XDG_CACHE_HOME, never inside the scanned repo: {}",
        path.display()
    );
    // Model-keyed file name: a future model change gets a fresh cache file
    // instead of silently mixing vectors from two models.
    assert!(path.ends_with(format!("{}.bin", find::MODEL_KEY)));
}

// ── JSON contract (schema field + hits array shape; no model needed) ──

#[test]
fn render_json_carries_the_versioned_schema_and_hit_fields() {
    let report = find::FindReport {
        query: "how do we open a connection to the database".to_string(),
        top: 10,
        hits: vec![find::Hit {
            rank: 1,
            score: 0.8234,
            qname: "app_db.engine.connect".to_string(),
            file: "app_db/engine.py".to_string(),
            line: 42,
        }],
    };
    let v = find::render_json(&report);
    assert_eq!(v["schema"], "akron.find/v1");
    assert_eq!(v["query"], report.query);
    assert_eq!(v["hits"][0]["rank"], 1);
    assert_eq!(v["hits"][0]["qname"], "app_db.engine.connect");
    assert_eq!(v["hits"][0]["file"], "app_db/engine.py");
    assert_eq!(v["hits"][0]["line"], 42);
    let score = v["hits"][0]["score"].as_f64().unwrap();
    assert!((score - 0.8234).abs() < 1e-3);
}

// ── without the `semantic` feature: exact message, via the library call.
// Only compiles under `cargo test --no-default-features` — a default
// `cargo test` build never exercises this arm (semantic is on by default).
#[cfg(not(feature = "semantic"))]
#[test]
fn search_without_semantic_feature_reports_the_one_line() {
    match find::search(Path::new("."), "anything", 10, false) {
        Ok(_) => panic!("expected an error when built without the semantic feature"),
        Err(e) => assert_eq!(format!("{e}"), "akron was built without the semantic feature"),
    }
}

// ── end-to-end, real model: run manually once with the model present
// (`cargo test --test find -- --ignored --nocapture find_end_to_end`) ──

#[test]
#[ignore = "needs the real embeddinggemma-300m-q model (network on first pull); run manually"]
fn find_end_to_end_hits_the_planted_symbol_and_touches_no_dot_akron() {
    let dir = std::env::temp_dir().join(format!("akron-find-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Isolated from the user's real `~/.cache` — this run must not read or
    // write the operator's actual model/index cache. A stable (not
    // per-pid) path so repeated manual runs reuse the pulled model instead
    // of re-downloading 331 MB every time.
    let cache_home = std::env::temp_dir().join("akron-find-e2e-cache");
    std::fs::create_dir_all(&cache_home).unwrap();
    std::fs::write(
        dir.join("engine.py"),
        r#"
import psycopg

def open_connection(dsn):
    """Open a new connection to the Postgres database."""
    conn = psycopg.connect(dsn)
    conn.autocommit = True
    return conn

def close_connection(conn):
    conn.close()
"#,
    )
    .unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_akron"))
        .arg("find")
        .arg(&dir)
        .arg("how do we open a connection to the database")
        .args(["--top", "5"])
        .env("XDG_CACHE_HOME", &cache_home)
        .output()
        .expect("run akron find");

    assert!(out.status.success(), "exit code: {:?}, stderr: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("open_connection"),
        "expected the planted connection function in the results:\n{stdout}"
    );

    assert!(
        !dir.join(".akron").exists(),
        "find must never create .akron/ in the scanned repo"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
