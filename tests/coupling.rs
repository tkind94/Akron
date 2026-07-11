//! `coupling::mine` against synthetic git repos built in a tempdir (see
//! `tests/branch.rs` / `tests/history.rs` for the same shelling-out-to-`git`
//! pattern). Covers: the worked-example confidence/degree math, the
//! changeset-size cap, lockfile exclusion, the insufficient-history gate,
//! determinism, and ticket grouping.

use akron::coupling::{self, CouplingConfig, CouplingSignal};
use std::fs;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .expect("git runs");
    assert!(st.success(), "git {args:?} failed");
}

/// Deterministic, strictly increasing commit date: an hour per tick.
fn date(tick: i64) -> String {
    format!("@{} +0000", 1_700_000_000 + tick * 3600)
}

fn commit_at(root: &Path, msg: &str, tick: i64) {
    let d = date(tick);
    let st = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(["commit", "-q", "-m", msg])
        .env("GIT_AUTHOR_DATE", &d)
        .env("GIT_COMMITTER_DATE", &d)
        .status()
        .expect("git runs");
    assert!(st.success(), "git commit failed");
}

fn init(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
}

/// Append a line to `file` (creating it if needed) so every touch is a real
/// content change `git` will actually commit.
fn touch(root: &Path, file: &str, tick: i64) {
    let p = root.join(file);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut content = fs::read_to_string(&p).unwrap_or_default();
    content.push_str(&format!("# rev {tick}\n"));
    fs::write(&p, content).unwrap();
}

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("akron-coupling-test-").tempdir().expect("tempdir")
}

fn file<'a>(report: &'a coupling::CouplingReport, path: &str) -> &'a coupling::FileHistory {
    report.files.iter().find(|f| f.path == path).unwrap_or_else(|| panic!("{path} not in report"))
}

fn pair<'a>(
    report: &'a coupling::CouplingReport,
    a: &str,
    b: &str,
) -> Option<&'a coupling::CoupledPair> {
    report.pairs.iter().find(|p| (p.a == a && p.b == b) || (p.a == b && p.b == a))
}

// ── worked example: A=100 revs, B=80, shared=50 → degree ~56% ──

#[test]
fn worked_example_confidence_and_degree() {
    let dir = tempdir();
    let root = dir.path();
    init(root);
    touch(root, "README.md", 0);
    git(root, &["add", "-A"]);
    commit_at(root, "init", 0);

    let mut tick = 1;
    // 50 transactions touching both A and B (shared_revs).
    for _ in 0..50 {
        touch(root, "a.py", tick);
        touch(root, "b.py", tick);
        git(root, &["add", "-A"]);
        commit_at(root, "touch both", tick);
        tick += 1;
    }
    // 50 more touching only A: revs(A) = 100.
    for _ in 0..50 {
        touch(root, "a.py", tick);
        git(root, &["add", "-A"]);
        commit_at(root, "touch a", tick);
        tick += 1;
    }
    // 30 more touching only B: revs(B) = 80.
    for _ in 0..30 {
        touch(root, "b.py", tick);
        git(root, &["add", "-A"]);
        commit_at(root, "touch b", tick);
        tick += 1;
    }

    let report = coupling::mine(root, &CouplingConfig::default()).expect("mine");
    let a = file(&report, "a.py");
    let b = file(&report, "b.py");
    assert_eq!(a.revs, 100);
    assert_eq!(b.revs, 80);

    let p = pair(&report, "a.py", "b.py").expect("a.py/b.py pair reported");
    assert_eq!(p.shared_revs, 50);
    assert!((p.confidence_ab - 0.5).abs() < 1e-9, "confidence(a->b) = 50/100");
    assert!((p.confidence_ba - 0.625).abs() < 1e-9, "confidence(b->a) = 50/80");
    // degree = 50 / ((100+80)/2) * 100 = 55.555...%, rounds to 56%.
    assert!((p.degree - 500.0 / 9.0).abs() < 1e-9);
    assert_eq!(p.degree.round(), 56.0);
}

// ── changeset-size cap ──

#[test]
fn oversized_changeset_is_excluded_entirely() {
    let dir = tempdir();
    let root = dir.path();
    init(root);
    touch(root, "README.md", 0);
    git(root, &["add", "-A"]);
    commit_at(root, "init", 0);

    let mut tick = 1;
    // 12 clean shared transactions: enough to clear min_revs (10) and the
    // pair gate (shared_revs >= 5, degree >= 30%).
    for _ in 0..12 {
        touch(root, "a.py", tick);
        touch(root, "b.py", tick);
        git(root, &["add", "-A"]);
        commit_at(root, "touch both", tick);
        tick += 1;
    }

    // One oversized transaction: a.py, b.py, plus 33 new files -> 35 total,
    // over the default cap of 30. Must be dropped whole: a.py/b.py revs
    // stay at 12, not 13, and the 33 new files never earn a rev either.
    touch(root, "a.py", tick);
    touch(root, "b.py", tick);
    for i in 0..33 {
        touch(root, &format!("extra_{i}.py"), tick);
    }
    git(root, &["add", "-A"]);
    commit_at(root, "huge changeset", tick);

    let report = coupling::mine(root, &CouplingConfig::default()).expect("mine");
    let a = file(&report, "a.py");
    let b = file(&report, "b.py");
    assert_eq!(a.revs, 12, "the oversized transaction must not count");
    assert_eq!(b.revs, 12);
    let extra = file(&report, "extra_0.py");
    assert_eq!(extra.revs, 0, "its only touch was in the dropped transaction");
    assert_eq!(extra.signal, CouplingSignal::InsufficientHistory);

    let p = pair(&report, "a.py", "b.py").expect("pair still reported from the 12 clean transactions");
    assert_eq!(p.shared_revs, 12);
}

// ── lockfile exclusion ──

#[test]
fn lockfiles_are_excluded_from_the_report() {
    let dir = tempdir();
    let root = dir.path();
    init(root);
    touch(root, "README.md", 0);
    git(root, &["add", "-A"]);
    commit_at(root, "init", 0);

    let mut tick = 1;
    for _ in 0..12 {
        touch(root, "a.py", tick);
        touch(root, "b.py", tick);
        touch(root, "Cargo.lock", tick);
        git(root, &["add", "-A"]);
        commit_at(root, "touch a, b, and the lockfile", tick);
        tick += 1;
    }

    let report = coupling::mine(root, &CouplingConfig::default()).expect("mine");
    assert!(
        !report.files.iter().any(|f| f.path == "Cargo.lock"),
        "Cargo.lock must not appear in the report at all"
    );
    let a = file(&report, "a.py");
    let b = file(&report, "b.py");
    // The lockfile riding along must not inflate or shrink real revs.
    assert_eq!(a.revs, 12);
    assert_eq!(b.revs, 12);
    let p = pair(&report, "a.py", "b.py").expect("a.py/b.py pair reported");
    assert_eq!(p.shared_revs, 12);
}

// ── insufficient-history gate ──

#[test]
fn file_below_min_revs_reports_insufficient_history_never_a_number() {
    let dir = tempdir();
    let root = dir.path();
    init(root);
    touch(root, "README.md", 0);
    git(root, &["add", "-A"]);
    commit_at(root, "init", 0);

    let mut tick = 1;
    // Only 3 touches: below the default min_revs of 10.
    for _ in 0..3 {
        touch(root, "rare.py", tick);
        git(root, &["add", "-A"]);
        commit_at(root, "touch rare", tick);
        tick += 1;
    }

    let report = coupling::mine(root, &CouplingConfig::default()).expect("mine");
    let rare = file(&report, "rare.py");
    assert_eq!(rare.revs, 3);
    assert_eq!(rare.signal, CouplingSignal::InsufficientHistory);
    // Type-level guarantee: the only way to read a churn number back out is
    // through the `Ready` arm, which this file never reaches.
    match rare.signal {
        CouplingSignal::Ready { .. } => panic!("must not carry a churn number below min_revs"),
        CouplingSignal::InsufficientHistory => {}
    }
}

// ── determinism ──

#[test]
fn two_runs_on_the_same_repo_are_byte_identical() {
    let dir = tempdir();
    let root = dir.path();
    init(root);
    touch(root, "README.md", 0);
    git(root, &["add", "-A"]);
    commit_at(root, "init", 0);

    let mut tick = 1;
    for _ in 0..20 {
        touch(root, "a.py", tick);
        touch(root, "b.py", tick);
        touch(root, "c.py", tick);
        git(root, &["add", "-A"]);
        commit_at(root, "touch a, b, c", tick);
        tick += 1;
    }

    let cfg = CouplingConfig::default();
    let r1 = coupling::mine(root, &cfg).expect("mine 1");
    let r2 = coupling::mine(root, &cfg).expect("mine 2");
    let j1 = serde_json::to_vec(&r1).unwrap();
    let j2 = serde_json::to_vec(&r2).unwrap();
    assert_eq!(j1, j2, "identical repo state must yield byte-identical output");
}

// ── ticket grouping ──

#[test]
fn ticket_grouping_merges_same_ticket_commits_into_one_transaction() {
    let dir = tempdir();
    let root = dir.path();
    init(root);
    touch(root, "README.md", 0);
    git(root, &["add", "-A"]);
    commit_at(root, "init", 0);

    // 6 ticket groups, each two commits by the same author: one touches
    // x.py only, the other touches y.py only. Grouped, each pair of commits
    // becomes one transaction touching {x.py, y.py} -> shared_revs = 6.
    // Ungrouped, no single transaction ever touches both -> shared_revs = 0.
    let mut tick = 1;
    for n in 0..6 {
        touch(root, "x.py", tick);
        git(root, &["add", "-A"]);
        commit_at(root, &format!("TKI-{n}: touch x"), tick);
        tick += 1;
        touch(root, "y.py", tick);
        git(root, &["add", "-A"]);
        commit_at(root, &format!("TKI-{n}: touch y"), tick);
        tick += 1;
    }

    let mut grouped_cfg = CouplingConfig::default();
    grouped_cfg.group_by_ticket = true;
    let grouped = coupling::mine(root, &grouped_cfg).expect("mine grouped");
    let p = pair(&grouped, "x.py", "y.py").expect("grouped transactions co-touch x.py and y.py");
    assert_eq!(p.shared_revs, 6);

    let ungrouped = coupling::mine(root, &CouplingConfig::default()).expect("mine ungrouped");
    assert!(
        pair(&ungrouped, "x.py", "y.py").is_none(),
        "without grouping, no single commit ever touches both files"
    );
}
