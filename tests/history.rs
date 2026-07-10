//! Channel C end-to-end harness. Builds a synthetic git repo (shelling out to
//! `git`, with commit timestamps pinned via GIT_*_DATE) containing an old,
//! never-touched helper family and a recently-added role-equivalent family.
//! Asserts symbol dating and that the deprecated-candidate query flags the
//! pair: dead shape ← growing role-twin.

use akron::types::Config;
use akron::{cluster, history, queries, scan};
use std::path::Path;
use std::process::Command;

// Two structurally-identical (renamed-clone) fetchers: a for-loop + try/except
// retry over `requests`. Same shape → one cluster; committed once, long ago.
const OLD_FAMILY: &str = r#"
import requests


def download_report(url, proxy, retries):
    proxies = {"http": proxy, "https": proxy}
    response = None
    for attempt in range(retries):
        try:
            response = requests.get(url, proxies=proxies, timeout=30)
            response.raise_for_status()
            return response.text
        except requests.RequestException:
            response = None
    raise ConnectionError("report proxy fetch failed after retries")


def download_summary(link, proxy, attempts):
    proxies = {"http": proxy, "https": proxy}
    response = None
    for attempt in range(attempts):
        try:
            response = requests.get(link, proxies=proxies, timeout=30)
            response.raise_for_status()
            return response.text
        except requests.RequestException:
            response = None
    raise ConnectionError("summary proxy fetch failed after retries")
"#;

// Same behavior (proxy fetch with retry → response text), different shape: a
// while-loop + status check over `httpx`. Renamed-clone pair → one cluster;
// committed recently, so this family is "growing".
const NEW_FAMILY: &str = r#"
import httpx


def pull_feed(url, proxy, retries):
    proxies = {"http": proxy, "https": proxy}
    response = None
    attempt = 0
    while attempt < retries:
        response = httpx.get(url, proxies=proxies, timeout=30)
        if response.status_code == 200:
            return response.text
        attempt = attempt + 1
    raise ConnectionError("feed proxy fetch failed after retries")


def pull_events(link, proxy, retries):
    proxies = {"http": proxy, "https": proxy}
    response = None
    attempt = 0
    while attempt < retries:
        response = httpx.get(link, proxies=proxies, timeout=30)
        if response.status_code == 200:
            return response.text
        attempt = attempt + 1
    raise ConnectionError("events proxy fetch failed after retries")
"#;

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .expect("git not on PATH")
        .success();
    assert!(ok, "git {args:?} failed");
}

/// Commit everything staged, pinning author + committer time so the walk is
/// deterministic regardless of when the test runs.
fn commit(root: &Path, msg: &str, date: &str) {
    let ok = Command::new("git")
        .current_dir(root)
        .args(["commit", "-q", "-m", msg])
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .status()
        .expect("git not on PATH")
        .success();
    assert!(ok, "git commit failed");
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

fn idx(symbols: &[akron::types::SymbolPrint], qname: &str) -> u32 {
    symbols
        .iter()
        .position(|s| s.sym.qname == qname)
        .unwrap_or_else(|| {
            panic!(
                "symbol {qname} not extracted (have: {:?})",
                symbols
                    .iter()
                    .map(|s| (s.sym.file.as_str(), s.sym.qname.as_str()))
                    .collect::<Vec<_>>()
            )
        }) as u32
}

#[test]
fn deprecated_candidate_flags_dead_family_against_growing_twin() {
    // Non-hidden prefix: the scanner skips dot-directories, and tempfile's
    // default prefix is ".tmp" (hidden), which would hide the whole tree.
    let tmp = tempfile::Builder::new()
        .prefix("akron-hist-")
        .tempdir()
        .expect("tempdir");
    let root = tmp.path();

    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@akron.dev"]);
    git(root, &["config", "user.name", "akron test"]);
    git(root, &["config", "commit.gpgsign", "false"]);

    // Old family: a single ancient commit, never touched again.
    std::fs::write(root.join("old_fetch.py"), OLD_FAMILY).unwrap();
    git(root, &["add", "-A"]);
    commit(root, "old fetch family", "2023-01-01T12:00:00 +0000");

    // Growing family: added ~17 months later — the newest commit (the anchor).
    std::fs::write(root.join("new_fetch.py"), NEW_FAMILY).unwrap();
    git(root, &["add", "-A"]);
    commit(root, "new fetch family", "2024-06-01T12:00:00 +0000");

    let cfg = cfg();
    let out = scan::scan_repo(root, &cfg);
    let symbols = &out.symbols;
    let hist = out.history.as_ref().expect("temp repo should have history");
    assert_eq!(
        history::fmt_date(hist.anchor),
        "2024-06-01",
        "anchor is the newest commit"
    );

    // ── symbol dating (file-level v0) ──
    let dates = |q: &str| {
        symbols[idx(symbols, q) as usize]
            .dating
            .as_ref()
            .unwrap_or_else(|| panic!("{q} should be dated"))
    };
    let old = dates("download_report");
    assert_eq!(history::fmt_date(old.first_seen), "2023-01-01");
    assert_eq!(history::fmt_date(old.last_touched), "2023-01-01");
    let new = dates("pull_feed");
    assert_eq!(history::fmt_date(new.first_seen), "2024-06-01");
    assert_eq!(history::fmt_date(new.last_touched), "2024-06-01");

    // ── the query ──
    let mut shapes = cluster::shape_clusters(symbols, cfg.theta_clone);
    let repeated = queries::repeated(symbols, &mut shapes);
    let vocab = cluster::vocab_index(symbols);
    let dep = queries::deprecated_candidates(symbols, &repeated, &vocab, hist.anchor, cfg.theta_b);

    let dead_member = idx(symbols, "download_report");
    let growing_member = idx(symbols, "pull_feed");
    let flagged = dep
        .candidates
        .iter()
        .any(|c| c.dead.contains(&dead_member) && c.growing.contains(&growing_member));
    assert!(
        flagged,
        "expected the old dead family to be flagged deprecated against the growing twin; \
         funnel: {} dated → {} dead × {} growing = {} pairs → {} vocab-matched",
        dep.funnel.dated_clusters,
        dep.funnel.dead_clusters,
        dep.funnel.growing_clusters,
        dep.funnel.role_pairs,
        dep.funnel.vocab_matched,
    );
}
