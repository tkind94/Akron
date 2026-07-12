//! TKI-55: `conftest.py` (any depth, not just under `tests/`) must classify
//! as test infrastructure end-to-end through the compiled binary — not just
//! at the `is_test_path` unit level (`src/parse.rs`), since `is_test` also
//! feeds `explain`'s ambiguous-candidate ordering and entry-point in-degree
//! (`explain.rs`'s `entry_point_tag`/`indegree`, both filter `is_test`
//! symbols first).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run_explain(root: &Path, target: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_akron"))
        .arg("explain")
        .arg(root)
        .arg(target)
        .args(["--min-nodes", "1"])
        .output()
        .expect("run akron explain")
}

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("akron-tki55-test-")
        .tempdir()
        .expect("tempdir")
}

#[test]
fn root_level_conftest_ranks_after_its_non_test_namesake() {
    // Same qname (`helper`) defined twice: once in an ordinary module, once
    // in a root-level conftest.py that lives nowhere near a `tests/` dir.
    // `resolve_name`'s ambiguity ranking (`ambiguous`, explain.rs) sorts
    // non-test candidates first — this only distinguishes the two if
    // conftest.py's `helper` is actually flagged `is_test`.
    let dir = tempdir();
    fs::write(
        dir.path().join("conftest.py"),
        "def helper():\n    return 1\n",
    )
    .unwrap();
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("prod.py"), "def helper():\n    return 2\n").unwrap();

    let out = run_explain(dir.path(), "helper");
    assert_eq!(out.status.code(), Some(2), "exact qname collision must be ambiguous");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let pos_prod = stderr.find("pkg/prod.py").expect("prod.py listed");
    let pos_conftest = stderr.find("conftest.py").expect("conftest.py listed");
    assert!(
        pos_prod < pos_conftest,
        "the production helper must rank before the conftest.py one:\n{stderr}"
    );
}

#[test]
fn nested_conftest_outside_tests_dir_is_excluded_from_entry_point_ranking() {
    // `hub` lives IN a package-level conftest.py (not under `tests/`) and is
    // called three times from a sibling module — a high in-degree that would
    // ordinarily win the directory's "entry" tag (mirrors
    // `entry_point_tag_marks_the_directorys_most_called_symbol` in
    // tests/explain.rs). `entry_point_tag`/`indegree` filter `is_test`
    // symbols first, so if conftest.py were still misclassified as
    // production, `hub` would win the tag; it must not.
    let dir = tempdir();
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("conftest.py"), "def hub():\n    return 1\n").unwrap();
    fs::write(
        pkg.join("callers.py"),
        "from pkg.conftest import hub\n\n\
         def caller_one():\n    return hub()\n\n\
         def caller_two():\n    return hub()\n\n\
         def caller_three():\n    return hub()\n",
    )
    .unwrap();

    let out = run_explain(dir.path(), "hub");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let header = String::from_utf8_lossy(&out.stdout).lines().next().unwrap().to_string();
    assert!(
        !header.contains("entry"),
        "conftest.py's hub must not win the directory's entry-point tag:\n{header}"
    );
}

#[test]
fn a_module_that_merely_starts_with_conftest_is_unaffected() {
    // Only the exact filename `conftest.py` is pytest infrastructure; a
    // similarly-named production module must resolve and rank as normal.
    let dir = tempdir();
    fs::write(
        dir.path().join("conftest_helpers.py"),
        "def widget():\n    return 1\n",
    )
    .unwrap();

    let out = run_explain(dir.path(), "widget");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("conftest_helpers.py:1  widget"),
        "conftest_helpers.py is a production module, not test infrastructure:\n{stdout}"
    );
}

