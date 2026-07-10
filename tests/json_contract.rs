//! TKI-27: machine-contract hygiene for `scan --json` (skills/akron/SKILL.md
//! Gaps). Three guarantees:
//!  (a) the JSON carries a top-level `"schema"` version, like `check`'s;
//!  (b) every `repeated[]`/`families[]`/`competing[]` entry's `ref` matches
//!      its own array position (`R#`/`F#`/`C#`), so a machine consumer never
//!      has to re-derive it;
//!  (c) `--json -` writes pure, parseable JSON to stdout.
//!
//! TKI-50: `scan` lost its whole human surface (digest, `--full`, `--html`,
//! `--top`) — `--json` (required in effect) is the only thing left, so these
//! tests no longer compare against a text rendering that doesn't exist.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn tmp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("akron-test-{}-{name}", std::process::id()))
}

fn run(root: &Path, extra: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_akron"))
        .arg("scan")
        .arg(root)
        .args(["--min-nodes", "25"])
        .args(extra)
        .output()
        .expect("run akron scan");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

#[test]
fn schema_field_is_present_and_versioned() {
    let root = fixtures();
    let out = tmp_path("schema.json");
    let (ok, _) = run(&root, &["--json", out.to_str().unwrap()]);
    assert!(ok, "akron scan failed");

    let bytes = std::fs::read(&out).expect("read json");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
    assert_eq!(v["schema"], "akron.scan/v1", "versioned schema");

    let _ = std::fs::remove_file(&out);
}

/// Every entry's `ref` must equal `{prefix}{position+1}` — its own index
/// within the array, not a separately-derived label that could drift from it
/// (TKI-27). Re-derived directly from the JSON now that there's no `--full`
/// text surface to cross-check against (TKI-50).
fn assert_positional_refs(arr: &serde_json::Value, prefix: &str) {
    let entries = arr.as_array().expect("section is an array");
    assert!(!entries.is_empty(), "fixtures must produce at least one {prefix} finding");
    for (n, entry) in entries.iter().enumerate() {
        assert_eq!(
            entry["ref"].as_str().expect("entry has a ref"),
            format!("{prefix}{}", n + 1),
            "ref must match this entry's position in its array"
        );
    }
}

#[test]
fn json_refs_are_positional_within_each_array() {
    let root = fixtures();

    let (ok, bare) = run(&root, &["--json", "-"]);
    assert!(ok, "akron scan --json - failed");
    let v: serde_json::Value = serde_json::from_str(&bare).expect("valid JSON");
    assert_positional_refs(&v["repeated"], "R");

    let (ok, only_families) = run(&root, &["--only", "families", "--json", "-"]);
    assert!(ok, "akron scan --only families --json - failed");
    let v: serde_json::Value = serde_json::from_str(&only_families).expect("valid JSON");
    assert_positional_refs(&v["families"], "F");

    let (ok, only_competing) = run(&root, &["--only", "competing", "--json", "-"]);
    assert!(ok, "akron scan --only competing --json - failed");
    let v: serde_json::Value = serde_json::from_str(&only_competing).expect("valid JSON");
    assert_positional_refs(&v["competing"], "C");
}

/// TKI-50: `--json` is required in effect — bare `akron scan <path>` has
/// nothing to compute, so it points at `--json` and the human view
/// (`akron explore`) and exits 2, rather than silently rendering anything.
#[test]
fn bare_scan_prints_a_pointer_and_exits_2() {
    let root = fixtures();
    let out = Command::new(env!("CARGO_BIN_EXE_akron"))
        .arg("scan")
        .arg(&root)
        .output()
        .expect("run akron scan");
    assert_eq!(out.status.code(), Some(2), "bare `akron scan` must exit 2");
    assert!(out.stdout.is_empty(), "bare `akron scan` must not print to stdout");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--json") && stderr.contains("akron explore"),
        "bare `akron scan` must point at --json and akron explore: {stderr:?}"
    );
}

#[test]
fn json_dash_writes_pure_json_to_stdout() {
    let root = fixtures();
    let (ok, stdout) = run(&root, &["--json", "-"]);
    assert!(ok, "akron scan --json - failed");

    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be pure, parseable JSON");
    assert_eq!(v["schema"], "akron.scan/v1");
    assert!(
        v["repeated"].as_array().is_some_and(|a| !a.is_empty()),
        "sanity: fixtures still produce repeated clusters"
    );
}

/// TKI-45 (pivot 2), still true post-TKI-50 now that `--full` is gone:
/// `families`/`competing` findings leave the default JSON — only `--only
/// families`/`--only competing` opts a run in. `repeated`/`deprecated` are
/// never gated. Funnels (the neutral counts) stay present regardless.
#[test]
fn families_and_competing_are_gated_behind_only() {
    let root = fixtures();

    let (ok, bare) = run(&root, &["--json", "-"]);
    assert!(ok, "akron scan --json - failed");
    let v: serde_json::Value = serde_json::from_str(&bare).expect("valid JSON");
    assert!(
        v["families"].as_array().is_some_and(Vec::is_empty),
        "families data absent by default: {v}"
    );
    assert!(
        v["competing"].as_array().is_some_and(Vec::is_empty),
        "competing data absent by default: {v}"
    );
    assert!(
        !v["repeated"].as_array().unwrap().is_empty(),
        "repeated is never gated"
    );
    assert!(
        v["family_funnel"].is_object(),
        "the neutral funnel counts stay present regardless"
    );

    let (ok, only_families) = run(&root, &["--only", "families", "--json", "-"]);
    assert!(ok, "akron scan --only families --json - failed");
    let v: serde_json::Value = serde_json::from_str(&only_families).expect("valid JSON");
    assert!(
        !v["families"].as_array().unwrap().is_empty(),
        "--only families opts families data in"
    );
    assert!(
        v["competing"].as_array().is_some_and(Vec::is_empty),
        "--only families does not also opt competing in"
    );

    let (ok, only_competing) = run(&root, &["--only", "competing", "--json", "-"]);
    assert!(ok, "akron scan --only competing --json - failed");
    let v: serde_json::Value = serde_json::from_str(&only_competing).expect("valid JSON");
    assert!(
        !v["competing"].as_array().unwrap().is_empty(),
        "--only competing opts competing data in"
    );
}
