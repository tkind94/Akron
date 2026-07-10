//! `branch::detect` (TKI-53) against scratch git repos: absence cases
//! (silent degrade), the changed-file union (committed + working tree +
//! untracked), base-root fingerprinting, and determinism.

use akron::branch;
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

const ALPHA: &str = "def alpha(x):\n    total = 0\n    for i in range(x):\n        total += i * i\n    return total\n";
const BETA: &str = "def beta(items):\n    out = []\n    for it in items:\n        if it:\n            out.append(str(it))\n    return out\n";
const GAMMA: &str = "def gamma(n):\n    acc = 1\n    while n > 1:\n        acc *= n\n        n -= 1\n    return acc\n";
const DELTA: &str = "def delta(s):\n    parts = s.split(',')\n    return [p.strip() for p in parts if p.strip()]\n";

/// The same pipeline `branch::detect` fingerprints base blobs with, via the
/// public API — so tests assert against bit-identical roots.
fn roots(source: &[u8], rel: &str) -> Vec<u64> {
    let tree = akron::parse::parse(source);
    let imports = akron::normalize::collect_imports(tree.root_node(), source, rel);
    akron::parse::extract_functions(&tree, source, rel)
        .iter()
        .map(|o| {
            akron::fingerprint::merkle_root(
                &akron::normalize::normalize(o.root, o.func, source, &imports).tree,
            )
        })
        .collect()
}

/// Base repo on `main`: mod.py (alpha) + util.py (beta), one commit.
fn base_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    fs::write(dir.join("mod.py"), ALPHA).unwrap();
    fs::write(dir.join("util.py"), BETA).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);
}

/// On top of `base_repo`: branch `feat` with a committed edit (gamma appended
/// to mod.py) + a committed new file (fresh.py), then an uncommitted
/// working-tree edit (delta appended to util.py) and an untracked file.
fn feature_branch(dir: &Path) {
    git(dir, &["checkout", "-qb", "feat"]);
    fs::write(dir.join("mod.py"), format!("{ALPHA}\n{GAMMA}")).unwrap();
    fs::write(dir.join("fresh.py"), GAMMA).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "feat work"]);
    fs::write(dir.join("util.py"), format!("{BETA}\n{DELTA}")).unwrap(); // uncommitted
    fs::write(dir.join("loose.py"), DELTA).unwrap(); // untracked
}

#[test]
fn not_a_git_repo_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    assert!(branch::detect(dir.path(), None).is_none());
}

#[test]
fn sitting_on_the_base_branch_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    base_repo(dir.path());
    // HEAD == merge-base(HEAD, main): nothing is "the branch's".
    assert!(branch::detect(dir.path(), None).is_none());
    // ...even with working-tree noise; the rule is HEAD == merge-base.
    fs::write(dir.path().join("mod.py"), format!("{ALPHA}\n{GAMMA}")).unwrap();
    assert!(branch::detect(dir.path(), None).is_none());
}

#[test]
fn detached_head_is_absent_without_base_but_resolves_with_it() {
    let dir = tempfile::tempdir().unwrap();
    base_repo(dir.path());
    feature_branch(dir.path());
    git(dir.path(), &["checkout", "-q", "--detach"]);
    assert!(branch::detect(dir.path(), None).is_none());
    let b = branch::detect(dir.path(), Some("main")).expect("explicit --base resolves");
    assert_eq!(b.base, "main");
    assert!(b.changed_files.contains("mod.py"));
}

#[test]
fn feature_branch_collects_committed_worktree_and_untracked_changes() {
    let dir = tempfile::tempdir().unwrap();
    base_repo(dir.path());
    feature_branch(dir.path());
    let b = branch::detect(dir.path(), None).expect("branch context resolves");
    assert_eq!(b.branch, "feat");
    assert_eq!(b.base, "main", "default base: local main");
    let mut changed: Vec<&str> = b.changed_files.iter().map(String::as_str).collect();
    changed.sort_unstable();
    assert_eq!(
        changed,
        ["fresh.py", "loose.py", "mod.py", "util.py"],
        "committed + working-tree + untracked, .py only"
    );
    // Base roots: the merge-base versions of mod.py/util.py (alpha, beta).
    // fresh.py/loose.py have no base version and contribute nothing.
    let alpha_root = roots(ALPHA.as_bytes(), "mod.py")[0];
    let beta_root = roots(BETA.as_bytes(), "util.py")[0];
    let gamma_root = roots(format!("{ALPHA}\n{GAMMA}").as_bytes(), "mod.py")[1];
    assert!(b.base_roots.contains(&alpha_root), "unchanged alpha is a base shape");
    assert!(b.base_roots.contains(&beta_root), "unchanged beta is a base shape");
    assert!(!b.base_roots.contains(&gamma_root), "gamma is the branch's own");
    assert_eq!(b.base_roots.len(), 2);
}

#[test]
fn moved_function_keeps_its_base_root() {
    // Move alpha from mod.py to moved.py on the branch: both files are
    // changed, and alpha's root must still be in the base set (so explore
    // will not mark the moved-but-unchanged code).
    let dir = tempfile::tempdir().unwrap();
    base_repo(dir.path());
    git(dir.path(), &["checkout", "-qb", "feat"]);
    fs::remove_file(dir.path().join("mod.py")).unwrap();
    fs::write(dir.path().join("moved.py"), ALPHA).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "move alpha"]);
    let b = branch::detect(dir.path(), None).expect("branch context resolves");
    assert!(b.changed_files.contains("mod.py"), "deletions stay in the changed set");
    assert!(b.changed_files.contains("moved.py"));
    let alpha_root = roots(ALPHA.as_bytes(), "mod.py")[0];
    assert!(
        b.base_roots.contains(&alpha_root),
        "the deleted file's base version still contributes its roots"
    );
}

#[test]
fn explicit_base_overrides_the_default() {
    let dir = tempfile::tempdir().unwrap();
    base_repo(dir.path());
    // A second base-like branch that already has gamma.
    git(dir.path(), &["checkout", "-qb", "develop"]);
    fs::write(dir.path().join("mod.py"), format!("{ALPHA}\n{GAMMA}")).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "develop has gamma"]);
    git(dir.path(), &["checkout", "-qb", "feat"]);
    fs::write(dir.path().join("fresh.py"), DELTA).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "feat"]);
    let vs_develop = branch::detect(dir.path(), Some("develop")).expect("resolves");
    assert_eq!(vs_develop.base, "develop");
    assert_eq!(
        vs_develop.changed_files,
        std::collections::HashSet::from(["fresh.py".to_string()]),
        "vs develop only fresh.py changed"
    );
    let vs_main = branch::detect(dir.path(), Some("main")).expect("resolves");
    assert!(vs_main.changed_files.contains("mod.py"), "vs main, mod.py changed too");
}

#[test]
fn default_base_falls_back_to_master() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "master"]);
    fs::write(dir.path().join("mod.py"), ALPHA).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "base"]);
    git(dir.path(), &["checkout", "-qb", "feat"]);
    fs::write(dir.path().join("fresh.py"), GAMMA).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "feat"]);
    let b = branch::detect(dir.path(), None).expect("resolves");
    assert_eq!(b.base, "master");
}

#[test]
fn nonexistent_base_ref_degrades_silently() {
    let dir = tempfile::tempdir().unwrap();
    base_repo(dir.path());
    feature_branch(dir.path());
    assert!(branch::detect(dir.path(), Some("no-such-ref")).is_none());
}

#[test]
fn detection_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    base_repo(dir.path());
    feature_branch(dir.path());
    let a = branch::detect(dir.path(), None).expect("resolves");
    let b = branch::detect(dir.path(), None).expect("resolves");
    assert_eq!(a.branch, b.branch);
    assert_eq!(a.base, b.base);
    assert_eq!(a.changed_files, b.changed_files);
    assert_eq!(a.base_roots, b.base_roots);
}
