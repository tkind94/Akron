//! `akron explore` (TKI-47). Two layers, split the way `find`'s tests are:
//!  (a) model-free — `explore::respond`/`find_response` are pure functions
//!      of an `ExploreState`, and a state builds from any embeddings, so
//!      the whole endpoint contract (shapes, errors, determinism) runs
//!      under plain `cargo test` with synthetic vectors;
//!  (b) one end-to-end server run behind `#[ignore]` with the real model:
//!      real embeddings, a real ephemeral-port tiny_http loop, raw HTTP.

use akron::branch::BranchInfo;
use akron::explore::{self, ExploreState};
use akron::run;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Deterministic synthetic embeddings: unit vectors from a fixed LCG — the
/// state doesn't care where vectors came from, only that there is one per
/// symbol.
fn synthetic_embeddings(n: usize, d: usize) -> Vec<Vec<f32>> {
    let mut state = 0xA5A5_5A5Au64;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
    };
    (0..n)
        .map(|_| {
            let mut v: Vec<f32> = (0..d).map(|_| next()).collect();
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            for x in &mut v {
                *x /= norm;
            }
            v
        })
        .collect()
}

fn fixture_state() -> ExploreState {
    fixture_state_with(None)
}

fn fixture_state_with(branch: Option<BranchInfo>) -> ExploreState {
    let cfg = explore::explore_cfg();
    let analysis = run::analyze(&fixtures_root(), &cfg);
    let n = analysis.scanned.symbols.len();
    assert!(n > 0, "fixtures must yield symbols at min_nodes=8");
    let docs = explore::scan_file_docs(&fixtures_root());
    explore::state_from(
        "fixtures",
        cfg,
        analysis,
        synthetic_embeddings(n, 24),
        docs,
        false,
        branch,
    )
}

/// A synthetic branch context marking `clone_original.py` as the one changed
/// file, with `base_roots` chosen by the test (empty = every symbol there is
/// branch-new).
fn synthetic_branch(base_roots: HashSet<u64>) -> BranchInfo {
    BranchInfo {
        branch: "feat".to_string(),
        base: "main".to_string(),
        changed_files: HashSet::from(["clone_original.py".to_string()]),
        base_roots,
    }
}

fn json(resp: &explore::Resp) -> serde_json::Value {
    assert_eq!(resp.content_type, "application/json");
    serde_json::from_slice(&resp.body).expect("valid json body")
}

// ── /api/symbols ──

#[test]
fn symbols_endpoint_has_one_row_per_symbol_with_the_contract_fields() {
    let state = fixture_state();
    let resp = explore::respond(&state, "/api/symbols");
    assert_eq!(resp.status, 200);
    let v = json(&resp);
    let rows = v.as_array().expect("an array of symbols");
    assert_eq!(rows.len(), state.analysis.scanned.symbols.len());
    let r = &rows[0];
    assert_eq!(r["id"], 0);
    for field in [
        "qname", "file", "line", "nodes", "indeg", "is_test", "branch_new", "dir", "pca", "x", "y",
    ] {
        assert!(!r[field].is_null() || field == "pca", "field {field} present");
    }
    // No branch context: the field is present but false on every row.
    assert!(rows.iter().all(|r| r["branch_new"] == false));
    // Map coordinates are the layout's normalized plane.
    let (x, y) = (r["x"].as_f64().unwrap(), r["y"].as_f64().unwrap());
    assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
    // first_seen/last_touched may be null (no git history at the fixture
    // root) but the keys must exist.
    assert!(r.as_object().unwrap().contains_key("first_seen"));
    assert!(r.as_object().unwrap().contains_key("last_touched"));
    assert_eq!(r["pca"].as_array().unwrap().len(), 8, "always 8 pca floats");
}

#[test]
fn symbols_endpoint_is_byte_identical_across_two_state_builds() {
    let a = explore::respond(&fixture_state(), "/api/symbols");
    let b = explore::respond(&fixture_state(), "/api/symbols");
    assert_eq!(
        a.body, b.body,
        "/api/symbols must be deterministic (relaxed positions included)"
    );
}

#[test]
fn symbols_rows_carry_layout_edge_ids() {
    let state = fixture_state();
    let n = state.analysis.scanned.symbols.len();
    let v = json(&explore::respond(&state, "/api/symbols"));
    for r in v.as_array().unwrap() {
        // full-geometry edges: always an array of valid ids, never self
        let id = r["id"].as_u64().unwrap();
        let nn = r["nn"].as_array().expect("nn is the full-geometry edge list");
        for e in nn {
            let e = e.as_u64().unwrap();
            assert!(e < n as u64, "edge id in range");
            assert_ne!(e, id, "no self edge");
        }
        // product-geometry edges: null for tests, ids of non-test symbols
        // otherwise — mirrors x/y being the product-only plane
        if r["is_test"].as_bool().unwrap() {
            assert!(r["nnp"].is_null(), "tests are off the product plane");
        } else {
            let nnp = r["nnp"].as_array().expect("nnp present for product symbols");
            for e in nnp {
                let e = e.as_u64().unwrap() as usize;
                assert!(
                    !state.analysis.scanned.symbols[e].is_test,
                    "product-plane edges stay on the product plane"
                );
            }
        }
    }
}

#[test]
fn relaxed_positions_do_not_stack() {
    // The overlap pass: no two product symbols may sit closer than the sum
    // of their (reference-scale) radii, minus a small edge-clamp tolerance.
    // Radii live in [2,7]px over an 832px reference extent, so the floor for
    // any pair is 4/832 — assert with a tolerance for pairs pushed against
    // the plane's edge.
    let state = fixture_state();
    let v = json(&explore::respond(&state, "/api/symbols"));
    let pts: Vec<(f64, f64)> = v
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| !r["x"].is_null())
        .map(|r| (r["x"].as_f64().unwrap(), r["y"].as_f64().unwrap()))
        .collect();
    let mut min_d = f64::INFINITY;
    for i in 0..pts.len() {
        for j in (i + 1)..pts.len() {
            let d = ((pts[i].0 - pts[j].0).powi(2) + (pts[i].1 - pts[j].1).powi(2)).sqrt();
            if d < min_d {
                min_d = d;
            }
        }
    }
    assert!(
        min_d > 1e-4,
        "no two points may coincide after relaxation: min pair distance {min_d}"
    );
}

#[test]
fn symbols_rows_carry_the_import_aware_indegree() {
    let state = fixture_state();
    let v = json(&explore::respond(&state, "/api/symbols"));
    for r in v.as_array().unwrap() {
        let i = r["id"].as_u64().unwrap() as usize;
        assert_eq!(
            r["indeg"].as_u64().unwrap(),
            state.indeg[i] as u64,
            "row indeg is explain's in-degree"
        );
    }
}

// ── /api/sublayout (TKI-54 drill-down) ──

#[test]
fn sublayout_file_drill_scopes_members_and_keys_by_class_context() {
    let state = fixture_state();
    let symbols = &state.analysis.scanned.symbols;
    let resp = explore::respond(&state, "/api/sublayout?path=role_guard_classes.py");
    assert_eq!(resp.status, 200);
    let v = json(&resp);
    assert_eq!(v["path"], "role_guard_classes.py");
    assert_eq!(v["kind"], "file");
    let ids: Vec<usize> = v["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as usize)
        .collect();
    let expected: Vec<usize> = (0..symbols.len())
        .filter(|&i| symbols[i].sym.file == "role_guard_classes.py")
        .collect();
    assert_eq!(ids, expected, "members are exactly the file's symbols");
    // single-file drill: color keys are the qname's class context
    let keys: Vec<&str> = v["dir"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
    for (slot, &i) in ids.iter().enumerate() {
        let q = &symbols[i].sym.qname;
        let expected_key = q.rsplit_once('.').map(|(c, _)| c).unwrap_or(".");
        assert_eq!(keys[slot], expected_key, "key for {q}");
    }
    assert!(
        keys.contains(&"AlphaSuite") && keys.contains(&"GammaSuite"),
        "the planted classes key the drill: {keys:?}"
    );
    // aligned planes: coords normalized, edges stay inside the member set
    let member: HashSet<usize> = ids.iter().copied().collect();
    let n = ids.len();
    for arr in ["x", "y", "xt", "yt", "nn", "dir"] {
        assert_eq!(v[arr].as_array().unwrap().len(), n, "{arr} aligned with ids");
    }
    for slot in 0..n {
        let (x, y) = (v["xt"][slot].as_f64().unwrap(), v["yt"][slot].as_f64().unwrap());
        assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
        for e in v["nn"][slot].as_array().unwrap() {
            assert!(member.contains(&(e.as_u64().unwrap() as usize)), "edges are global ids of members");
        }
    }
}

#[test]
fn sublayout_prefix_is_segment_bounded() {
    let state = fixture_state();
    // "clone" is a prefix of clone_original.py the STRING but not a path
    // segment — it must not match.
    assert_eq!(explore::respond(&state, "/api/sublayout?path=clone").status, 404);
    assert_eq!(explore::respond(&state, "/api/sublayout?path=nope/nope.py").status, 404);
    assert_eq!(explore::respond(&state, "/api/sublayout").status, 400);
    assert_eq!(explore::respond(&state, "/api/sublayout?path=").status, 400);
}

#[test]
fn sublayout_is_byte_identical_across_two_state_builds() {
    let url = "/api/sublayout?path=role_guard_classes.py";
    let a = explore::respond(&fixture_state(), url);
    let b = explore::respond(&fixture_state(), url);
    assert_eq!(a.body, b.body, "drill layout must be deterministic");
}

/// A tree with real directory structure for the dir-drill rules — the flat
/// fixtures can't exercise relative color keys or test-plane splitting.
fn drill_tree_state() -> (tempfile::TempDir, ExploreState) {
    let dir = tempfile::Builder::new()
        .prefix("akron-drill-test-")
        .tempdir()
        .expect("tempdir");
    let body = |name: &str| {
        format!(
            "def {name}(path):\n    rows = []\n    with open(path) as fh:\n        for line in fh:\n            line = line.strip()\n            if line:\n                rows.append(line.split(','))\n    return rows\n"
        )
    };
    let write = |rel: &str, names: &[&str]| {
        let p = dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let src: String = names.iter().map(|n| body(n)).collect::<Vec<_>>().join("\n\n");
        std::fs::write(p, src).unwrap();
    };
    write("pkg/net/client.py", &["send_request", "recv_response"]);
    write("pkg/net/server.py", &["accept_loop"]);
    write("pkg/util/text.py", &["split_lines", "join_lines"]);
    write("pkg/root.py", &["main_entry"]);
    write("tests/test_client.py", &["test_send"]);
    let cfg = explore::explore_cfg();
    let analysis = run::analyze(dir.path(), &cfg);
    let n = analysis.scanned.symbols.len();
    assert_eq!(n, 7, "the planted tree yields all 7 symbols");
    let docs = explore::scan_file_docs(dir.path());
    let state = explore::state_from(
        "drill",
        cfg,
        analysis,
        synthetic_embeddings(n, 24),
        docs,
        false,
        None,
    );
    (dir, state)
}

#[test]
fn sublayout_dir_drill_rederives_relative_color_keys_and_planes() {
    let (_dir, state) = drill_tree_state();
    let symbols = &state.analysis.scanned.symbols;
    let v = json(&explore::respond(&state, "/api/sublayout?path=pkg"));
    assert_eq!(v["kind"], "dir");
    let ids: Vec<usize> = v["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as usize)
        .collect();
    assert_eq!(ids.len(), 6, "everything under pkg/, nothing under tests/");
    // relative keys at depth 1: net, util, and `.` for pkg/root.py
    for (slot, &i) in ids.iter().enumerate() {
        let expected = match symbols[i].sym.file.as_str() {
            "pkg/root.py" => ".",
            f if f.starts_with("pkg/net/") => "net",
            _ => "util",
        };
        assert_eq!(v["dir"][slot], expected, "relative key for {}", symbols[i].sym.file);
    }
    // no test members here: every x/y and nnp is populated
    for slot in 0..ids.len() {
        assert!(v["x"][slot].is_f64() && v["y"][slot].is_f64());
        assert!(v["nnp"][slot].is_array());
    }
    // deeper drill: pkg/net is FLAT — no subdir structure to color, so the
    // keys fall back to the files (the ladder's next rung)
    let v = json(&explore::respond(&state, "/api/sublayout?path=pkg/net"));
    assert_eq!(v["ids"].as_array().unwrap().len(), 3);
    let keys: HashSet<&str> =
        v["dir"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
    assert_eq!(keys, HashSet::from(["client.py", "server.py"]));
    // labels: both geometries always present as arrays (may be empty)
    assert!(v["labels"]["prod"].is_array() && v["labels"]["full"].is_array());
}

#[test]
fn sublayout_test_members_stay_off_the_product_plane() {
    let (_dir, state) = drill_tree_state();
    let symbols = &state.analysis.scanned.symbols;
    let v = json(&explore::respond(&state, "/api/sublayout?path=tests"));
    let ids = v["ids"].as_array().unwrap();
    assert_eq!(ids.len(), 1);
    let i = ids[0].as_u64().unwrap() as usize;
    assert!(symbols[i].is_test);
    // a test symbol rides the full plane only — same rule as the global map
    assert!(v["x"][0].is_null() && v["nnp"][0].is_null());
    assert!(v["xt"][0].is_f64() && v["yt"][0].is_f64());
}

// ── /api/meta ──

#[test]
fn meta_endpoint_serves_variance_shares_and_labels() {
    let state = fixture_state();
    let resp = explore::respond(&state, "/api/meta");
    assert_eq!(resp.status, 200);
    let v = json(&resp);
    let shares = v["pca_var"].as_array().expect("pca_var present");
    assert_eq!(shares.len(), 8, "one share per shipped component");
    let vals: Vec<f64> = shares.iter().map(|s| s.as_f64().unwrap()).collect();
    for w in vals.windows(2) {
        assert!(w[0] >= w[1] - 1e-12, "shares ordered: {vals:?}");
    }
    let sum: f64 = vals.iter().sum();
    assert!(sum <= 1.0 + 1e-9, "shares sum ≤ 1: {sum}");
    assert!(vals.iter().all(|&s| s >= 0.0));
    // no branch context: the key is present and null (shape stability)
    assert!(v.as_object().unwrap().contains_key("branch"));
    assert!(v["branch"].is_null());
    // labels: both geometries present; every label is a known dir with
    // in-plane coordinates and a count that meets the gate
    for geom in ["prod", "full"] {
        let labels = v["labels"][geom].as_array().expect("labels array per geometry");
        for l in labels {
            assert!(l["dir"].is_string());
            let (x, y) = (l["x"].as_f64().unwrap(), l["y"].as_f64().unwrap());
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
            assert!(l["count"].as_u64().unwrap() >= 4);
            // r is the gating median — labeled dirs sit under the gate
            let r = l["r"].as_f64().unwrap();
            assert!((0.0..=0.12).contains(&r), "label r within the gate: {r}");
        }
    }
}

#[test]
fn meta_endpoint_is_byte_identical_across_two_state_builds() {
    let a = explore::respond(&fixture_state(), "/api/meta");
    let b = explore::respond(&fixture_state(), "/api/meta");
    assert_eq!(a.body, b.body, "/api/meta must be deterministic");
}

// ── module-docstring prevalence (TKI-56) ──

#[test]
fn meta_endpoint_carries_one_file_doc_row_per_scanned_file() {
    let state = fixture_state();
    let v = json(&explore::respond(&state, "/api/meta"));
    let rows = v["file_docs"].as_array().expect("file_docs array present");
    // Fixture ground truth (checked by reading the files): 15 planted
    // fixtures, 13 open with a bare `"""..."""` docstring; the two
    // `todict_*.py` fixtures start directly with `def to_dict...`.
    assert_eq!(rows.len(), 15, "one row per scanned fixture file");
    let with_doc = rows.iter().filter(|r| r["doc"] == true).count();
    assert_eq!(with_doc, 13, "13 of 15 fixtures open with a module docstring");
    for undocumented in ["todict_core.py", "todict_member.py"] {
        let row = rows.iter().find(|r| r["file"] == undocumented).expect("row present");
        assert_eq!(row["doc"], false, "{undocumented} has no module docstring");
    }
    let documented = rows.iter().find(|r| r["file"] == "clone_original.py").expect("row present");
    assert_eq!(documented["doc"], true);
    assert_eq!(documented["is_test"], false);
}

#[test]
fn has_module_docstring_true_for_plain_string_after_comments_and_blanks() {
    let src = b"# -*- coding: utf-8 -*-\n# a header comment\n\n\"\"\"real docstring\"\"\"\ndef f():\n    pass\n";
    assert!(explore::has_module_docstring(src));
}

#[test]
fn has_module_docstring_true_for_single_quoted_and_literal_braces() {
    let src = b"'''use {} braces literally, no interpolation'''\nx = 1\n";
    assert!(explore::has_module_docstring(src));
}

#[test]
fn has_module_docstring_false_when_comment_only_header_precedes_code() {
    // A comment-only file header is not a docstring: no string statement at all.
    let src = b"# just a header, no docstring\ndef f():\n    pass\n";
    assert!(!explore::has_module_docstring(src));
}

#[test]
fn has_module_docstring_false_for_future_import_before_a_string() {
    // `from __future__ import` is a real statement — it blocks the string
    // behind it from being the first statement, exactly like CPython's own
    // `__doc__` assignment rule.
    let src = b"from __future__ import annotations\n\"\"\"not a docstring\"\"\"\n";
    assert!(!explore::has_module_docstring(src));
}

#[test]
fn has_module_docstring_true_when_docstring_precedes_future_import() {
    let src = b"\"\"\"real docstring\"\"\"\nfrom __future__ import annotations\n";
    assert!(explore::has_module_docstring(src));
}

#[test]
fn has_module_docstring_false_for_f_string_first_statement() {
    // An f-string is never assigned to `__doc__`, even with no `{}` inside.
    let src = b"f\"\"\"looks like a docstring but is an f-string\"\"\"\n";
    assert!(!explore::has_module_docstring(src));
}

#[test]
fn has_module_docstring_false_for_f_string_with_interpolation() {
    let name = "x";
    let src = format!("f\"hello {{{name}}}\"\n");
    assert!(!explore::has_module_docstring(src.as_bytes()));
}

#[test]
fn has_module_docstring_false_for_empty_file() {
    assert!(!explore::has_module_docstring(b""));
}

#[test]
fn has_module_docstring_false_when_expression_is_not_a_bare_string() {
    // A string that's part of a larger expression (concatenation, a call
    // argument, an assignment) is not a bare string statement.
    assert!(!explore::has_module_docstring(b"x = \"not a docstring\"\n"));
    assert!(!explore::has_module_docstring(b"print(\"not a docstring\")\n"));
}

// ── branch highlighting (TKI-53) ──

#[test]
fn branch_new_marks_changed_file_symbols_whose_root_is_absent_from_base() {
    let state = fixture_state_with(Some(synthetic_branch(HashSet::new())));
    let v = json(&explore::respond(&state, "/api/symbols"));
    let mut marked = 0;
    for r in v.as_array().unwrap() {
        let expected = r["file"] == "clone_original.py"; // empty base set: all its symbols are new
        assert_eq!(r["branch_new"], expected, "row {}", r["id"]);
        if expected {
            marked += 1;
        }
    }
    assert!(marked > 0, "the planted fixture file yields symbols");
    // meta carries the branch block; `changed` counts the marked symbols
    let m = json(&explore::respond(&state, "/api/meta"));
    assert_eq!(m["branch"]["name"], "feat");
    assert_eq!(m["branch"]["base"], "main");
    assert_eq!(m["branch"]["changed"], marked);
}

#[test]
fn branch_new_spares_symbols_whose_root_exists_in_the_base_versions() {
    // The moved-but-unchanged rule: a changed-file symbol whose Merkle root
    // is in the base-version root set is NOT branch-new.
    let cfg = explore::explore_cfg();
    let probe = run::analyze(&fixtures_root(), &cfg);
    let orig = probe
        .scanned
        .symbols
        .iter()
        .find(|s| s.sym.file == "clone_original.py")
        .expect("planted fixture present");
    let spared_root = orig.merkle_root;
    let state = fixture_state_with(Some(synthetic_branch(HashSet::from([spared_root]))));
    let v = json(&explore::respond(&state, "/api/symbols"));
    for r in v.as_array().unwrap() {
        if r["file"] != "clone_original.py" {
            assert_eq!(r["branch_new"], false);
            continue;
        }
        let i = r["id"].as_u64().unwrap() as usize;
        let expected = state.analysis.scanned.symbols[i].merkle_root != spared_root;
        assert_eq!(r["branch_new"], expected, "root-anchored, not file-anchored");
    }
}

#[test]
fn explain_serves_nearest_existing_for_branch_new_symbols_channel_numbers_only() {
    let state = fixture_state_with(Some(synthetic_branch(HashSet::new())));
    let symbols = &state.analysis.scanned.symbols;
    let new_id = symbols
        .iter()
        .position(|s| s.sym.file == "clone_original.py")
        .expect("planted fixture present");
    let v = json(&explore::respond(&state, &format!("/api/explain?id={new_id}")));
    assert_eq!(v["branch_new"], true);
    let rows = v["nearest_existing"].as_array().expect("array for a branch-new symbol");
    assert!(!rows.is_empty() && rows.len() <= 8, "top-8: {}", rows.len());
    for r in rows {
        // exactly the ref fields + the two deterministic channel cosines —
        // THE LAW: the semantic score that ranked the list is NOT shipped
        let keys: Vec<&str> = r.as_object().unwrap().keys().map(String::as_str).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, ["a_cos", "b_cos", "file", "id", "line", "qname"]);
        let i = r["id"].as_u64().unwrap() as usize;
        assert!(!symbols[i].is_test, "test symbols excluded (find's default)");
        assert_ne!(
            symbols[i].sym.file, "clone_original.py",
            "branch-new symbols excluded from nearest-existing"
        );
    }
    // a non-branch-new symbol: flag false, field null (shape stable)
    let old_id = symbols
        .iter()
        .position(|s| s.sym.file != "clone_original.py")
        .expect("some unchanged symbol");
    let v = json(&explore::respond(&state, &format!("/api/explain?id={old_id}")));
    assert_eq!(v["branch_new"], false);
    assert!(v["nearest_existing"].is_null());
}

#[test]
fn branch_state_endpoints_are_byte_identical_across_two_builds() {
    let mk = || fixture_state_with(Some(synthetic_branch(HashSet::new())));
    for path in ["/api/symbols", "/api/meta"] {
        let a = explore::respond(&mk(), path);
        let b = explore::respond(&mk(), path);
        assert_eq!(a.body, b.body, "{path} must be deterministic with branch context");
    }
}

// ── /api/explain ──

#[test]
fn explain_endpoint_serves_the_card_fields() {
    let state = fixture_state();
    let resp = explore::respond(&state, "/api/explain?id=0");
    assert_eq!(resp.status, 200);
    let v = json(&resp);
    assert_eq!(v["id"], 0);
    let keys = v.as_object().unwrap();
    for field in [
        "qname", "file", "line", "nodes", "is_test", "branch_new", "nearest_existing", "entry",
        "dating", "clones", "twins", "callers", "callees", "family",
    ] {
        assert!(keys.contains_key(field), "card field {field} present");
    }
    assert!(v["clones"]["exact"].is_array());
    assert!(v["clones"]["near"].is_array());
    assert!(v["callers"].is_array());
}

#[test]
fn explain_lists_clones_with_ids_that_point_back_into_the_map() {
    // The planted clone pair (clone_original/clone_renamed) must reference
    // each other by id, so the panel's click-to-navigate works.
    let state = fixture_state();
    let symbols = &state.analysis.scanned.symbols;
    let orig = symbols
        .iter()
        .position(|s| s.sym.file == "clone_original.py")
        .expect("planted fixture present");
    let v = json(&explore::respond(&state, &format!("/api/explain?id={orig}")));
    let exact = v["clones"]["exact"].as_array().unwrap();
    assert!(
        !exact.is_empty(),
        "clone_original has an exact clone (clone_renamed)"
    );
    let id = exact[0]["id"].as_u64().unwrap() as usize;
    assert!(id < symbols.len(), "clone reference is a valid map id");
    assert_eq!(exact[0]["file"], symbols[id].sym.file.as_str());
}

#[test]
fn explain_rejects_bad_ids() {
    let state = fixture_state();
    assert_eq!(explore::respond(&state, "/api/explain").status, 400);
    assert_eq!(explore::respond(&state, "/api/explain?id=abc").status, 400);
    assert_eq!(explore::respond(&state, "/api/explain?id=999999").status, 404);
}

// ── /api/anchor ──

#[test]
fn anchor_endpoint_returns_full_width_cosine_arrays() {
    let state = fixture_state();
    let n = state.analysis.scanned.symbols.len();
    let resp = explore::respond(&state, "/api/anchor?id=0");
    assert_eq!(resp.status, 200);
    let v = json(&resp);
    let a = v["a_cos"].as_array().unwrap();
    let b = v["b_cos"].as_array().unwrap();
    assert_eq!(a.len(), n, "one Channel-A cosine per symbol");
    assert_eq!(b.len(), n, "one Channel-B cosine per symbol");
    let self_a = a[0].as_f64().unwrap();
    assert!(
        (self_a - 1.0).abs() < 1e-5,
        "the anchor's Channel-A cosine to itself is 1: {self_a}"
    );
}

#[test]
fn anchor_rejects_bad_ids() {
    let state = fixture_state();
    assert_eq!(explore::respond(&state, "/api/anchor").status, 400);
    assert_eq!(explore::respond(&state, "/api/anchor?id=999999").status, 404);
}

// ── / and unknown paths ──

#[test]
fn page_is_served_with_the_boot_payload_substituted() {
    let state = fixture_state();
    let resp = explore::respond(&state, "/");
    assert_eq!(resp.status, 200);
    assert!(resp.content_type.starts_with("text/html"));
    let page = String::from_utf8(resp.body).unwrap();
    assert!(page.contains("<canvas"), "the map canvas is in the page");
    assert!(
        !page.contains("__AKRON_BOOT__"),
        "the boot token must be substituted at state build"
    );
    assert!(page.contains("\"repo\":\"fixtures\""), "boot carries the repo name");
    assert!(
        !page.contains("http://") && !page.contains("https://"),
        "self-contained page: no external URLs"
    );
}

#[test]
fn unknown_paths_are_404() {
    let state = fixture_state();
    assert_eq!(explore::respond(&state, "/api/nope").status, 404);
    assert_eq!(explore::respond(&state, "/etc/passwd").status, 404);
    // ...but the browser's automatic favicon request is answered, so the
    // devtools console stays clean.
    assert_eq!(explore::respond(&state, "/favicon.ico").status, 200);
}

// ── /api/find's ranking layer (model-free: the query vector is an input) ──

#[test]
fn find_response_ranks_the_matching_vector_first() {
    let state = fixture_state();
    // A query vector equal to symbol 2's embedding must rank symbol 2 first
    // with cosine ~1 (all synthetic vectors are unit-norm).
    let query = state.embeddings[2].clone();
    let resp = explore::find_response(&state, &query, false, 5);
    assert_eq!(resp.status, 200);
    let v = json(&resp);
    let hits = v["hits"].as_array().unwrap();
    assert!(hits.len() <= 5, "top truncates");
    assert_eq!(hits[0]["id"], 2);
    assert_eq!(hits[0]["rank"], 1);
    assert!((hits[0]["score"].as_f64().unwrap() - 1.0).abs() < 1e-5);
    // scores descend
    let scores: Vec<f64> = hits.iter().map(|h| h["score"].as_f64().unwrap()).collect();
    for w in scores.windows(2) {
        assert!(w[0] >= w[1], "hits must be ranked by descending score");
    }
}

// ── end-to-end, real model + real HTTP loop: run manually with
// `cargo test --test explore -- --ignored --nocapture` ──

#[cfg(feature = "semantic")]
fn http_get(port: u16, path: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).expect("read response");
    let status: u16 = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status line");
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

#[cfg(feature = "semantic")]
#[test]
#[ignore = "needs the real bge-small-q model (network on first pull); run manually"]
fn explore_server_end_to_end() {
    let root = fixtures_root().canonicalize().unwrap();
    let (state, mut embedder) = explore::build_state(&root, false, None).expect("build state");
    let n = state.analysis.scanned.symbols.len();
    let server = explore::bind(0).expect("bind ephemeral port");
    let port = explore::bound_port(&server).expect("bound port");
    std::thread::spawn(move || explore::run_loop(&server, &state, &mut embedder));

    let (status, body) = http_get(port, "/api/symbols");
    assert_eq!(status, 200);
    let symbols: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(symbols.as_array().unwrap().len(), n);
    assert_eq!(symbols[0]["pca"].as_array().unwrap().len(), 8);

    let (status, body) = http_get(port, "/api/explain?id=0");
    assert_eq!(status, 200);
    let card: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(card["id"], 0);
    assert!(card["callers"].is_array());

    let (status, body) = http_get(port, "/api/anchor?id=0");
    assert_eq!(status, 200);
    let anchor: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(anchor["a_cos"].as_array().unwrap().len(), n);
    assert_eq!(anchor["b_cos"].as_array().unwrap().len(), n);

    let (status, body) = http_get(port, "/api/find?q=convert%20a%20record%20to%20a%20dict&top=5");
    assert_eq!(status, 200);
    let found: serde_json::Value = serde_json::from_str(&body).unwrap();
    let hits = found["hits"].as_array().unwrap();
    assert!(!hits.is_empty(), "a real query over the fixtures finds hits");
    assert!(hits.len() <= 5);

    let (status, page) = http_get(port, "/");
    assert_eq!(status, 200);
    assert!(page.contains("<canvas"));

    // The isolation invariant find's e2e also asserts: no writes under the
    // scanned repo.
    assert!(
        !root.join(".akron").exists(),
        "explore must never create .akron/ in the scanned repo"
    );
}
