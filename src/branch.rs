//! Branch-vs-base detection for `akron explore` (TKI-53): which symbols did
//! this branch introduce, content-anchored so a move or an untouched neighbor
//! never lights up.
//!
//! Resolution happens once at launch: current branch name, base ref (`--base`
//! if given, else the repo's default branch — `origin/HEAD` if set, else
//! `main`, else `master`), then `git merge-base HEAD <base>`. Changed files
//! are the union of committed (`merge-base..HEAD`), working-tree
//! (`diff <merge-base>`), and untracked (`ls-files --others`) paths — the
//! human being answered lives in their working tree. For each changed file,
//! the BASE version's bytes (`git show <merge-base>:<file>`) run the exact
//! scan fingerprint pipeline (parse → extract → normalize → Merkle), and the
//! union of those roots is the "already existed" shape set.
//!
//! The rule `explore::state_from` applies: a current symbol is branch-new iff
//! its file is in the changed set AND its Merkle root is absent from the
//! base-version root set. Moved-but-unchanged code between changed files
//! keeps its root, so it never marks.
//!
//! Degradation is SILENT (the time-view precedent, not `check`'s exit-2
//! pattern): not a git repo, detached HEAD without `--base`, sitting on the
//! base branch (HEAD == merge-base), or any git error at all → `None`, and
//! the feature is simply absent. Git runs as a subprocess (the deleted
//! `check.rs`'s ratified choice — an in-process tree diff buys nothing here);
//! every command is a pure function of repo state, so the result is
//! deterministic for a fixed HEAD + working tree + base.

use crate::{fingerprint, normalize, parse};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// Everything `explore` needs to mark branch-new symbols. Passive data:
/// `detect` does the git IO, `explore::state_from` applies the rule — so the
/// endpoint contract stays testable with synthetic values.
pub struct BranchInfo {
    /// Current branch short name (or the short HEAD sha when detached under
    /// an explicit `--base`).
    pub branch: String,
    /// The ref the merge-base was computed against: the `--base` value, or
    /// the resolved default branch.
    pub base: String,
    /// Changed `.py` files (relative to the scan root, `SymbolPrint::file`
    /// form): committed since the merge-base, working-tree-modified, or
    /// untracked.
    pub changed_files: HashSet<String>,
    /// Merkle roots of every function in the merge-base version of each
    /// changed file — the shapes that already existed on the base.
    pub base_roots: HashSet<u64>,
}

/// Run one git command at `root`; `None` on any failure (silent degrade).
fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// First line of a git command's stdout, trimmed; `None` if empty.
fn git_line(root: &Path, args: &[&str]) -> Option<String> {
    let out = git(root, args)?;
    let line = String::from_utf8_lossy(&out).lines().next()?.trim().to_string();
    (!line.is_empty()).then_some(line)
}

/// NUL-separated path list (`-z` keeps non-ASCII paths verbatim — git would
/// otherwise quote them and they'd never match a `SymbolPrint::file`).
fn nul_paths(raw: &[u8]) -> impl Iterator<Item = String> + '_ {
    raw.split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
}

/// The repo's default branch: `origin/HEAD` when set, else a local `main`,
/// else a local `master`.
fn default_base(root: &Path) -> Option<String> {
    if let Some(r) = git_line(root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]) {
        if r != "origin/HEAD" {
            return Some(r);
        }
    }
    for cand in ["main", "master"] {
        if git(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{cand}")]).is_some()
        {
            return Some(cand.to_string());
        }
    }
    None
}

/// Merkle roots of every function in `source` as if it lived at `rel` — the
/// fingerprint half of `scan::process_file`, bit-identical by construction
/// (same parse, same imports collection, same normalize, same hash). No
/// `min_nodes` filter: the base set wants every shape that existed, so a
/// small base function can never falsely mark its unchanged descendant.
fn roots_of_source(source: &[u8], rel: &str) -> Vec<u64> {
    let tree = parse::parse(source);
    let imports = normalize::collect_imports(tree.root_node(), source, rel);
    parse::extract_functions(&tree, source, rel)
        .iter()
        .map(|occ| fingerprint::merkle_root(&normalize::normalize(occ.root, occ.func, source, &imports).tree))
        .collect()
}

/// Resolve the branch context at `root`, or `None` when the feature is
/// absent: not a git repo, detached HEAD without `--base`, HEAD == merge-base
/// (sitting on the base branch), or any git error.
pub fn detect(root: &Path, base_flag: Option<&str>) -> Option<BranchInfo> {
    if git_line(root, &["rev-parse", "--is-inside-work-tree"])?.as_str() != "true" {
        return None;
    }
    let head = git_line(root, &["rev-parse", "HEAD"])?;
    let mut branch = git_line(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == "HEAD" {
        // Detached HEAD: only meaningful under an explicit --base.
        base_flag?;
        branch = git_line(root, &["rev-parse", "--short", "HEAD"])?;
    }
    let base = match base_flag {
        Some(b) => b.to_string(),
        None => default_base(root)?,
    };
    let merge_base = git_line(root, &["merge-base", "HEAD", &base])?;
    if merge_base == head {
        return None; // sitting on the base branch: nothing is "the branch's"
    }

    // Changed files: committed on the branch, changed in the working tree
    // (the human lives there), and untracked (a brand-new file is the most
    // branch-new thing there is). `--relative` bases paths on `root` so they
    // match `SymbolPrint::file`; `--no-renames` keeps a rename as its
    // delete+add pair — the deleted source's base version must contribute
    // its roots (that is what keeps moved code unmarked), and the result
    // must not depend on the caller's `diff.renames` config (the same
    // config-independence rule as `history.rs`'s disabled rename tracking).
    let mut changed_files: HashSet<String> = HashSet::new();
    let committed = git(
        root,
        &["diff", "--name-only", "-z", "--relative", "--no-renames", &format!("{merge_base}..HEAD")],
    )?;
    let worktree = git(root, &["diff", "--name-only", "-z", "--relative", "--no-renames", &merge_base])?;
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    for raw in [&committed, &worktree, &untracked] {
        changed_files.extend(nul_paths(raw).filter(|p| p.ends_with(".py")));
    }

    let mut base_roots: HashSet<u64> = HashSet::new();
    for file in &changed_files {
        // `./` anchors the path at the cwd (`root`), matching `--relative`.
        // A failing `show` means the file is absent at the merge-base (new
        // file): it simply contributes no roots.
        if let Some(bytes) = git(root, &["show", &format!("{merge_base}:./{file}")]) {
            base_roots.extend(roots_of_source(&bytes, file));
        }
    }

    Some(BranchInfo {
        branch,
        base,
        changed_files,
        base_roots,
    })
}
