//! Private-name hygiene gate (TKI-24). The repo must never carry identifiers
//! from the private codebases akron is graded against. The token list itself
//! is the secret, so it lives OUTSIDE the repo — this test reads it from a
//! sibling archive and is a silent no-op on machines where that file does not
//! exist (CI, contributors).

use std::fs;
use std::path::{Path, PathBuf};

const DENYLIST: &str = "../Akron-private/name-denylist.txt";
const SKIP_DIRS: [&str; 2] = ["target", "node_modules"];

/// Tracked-tree walk: skips dot-dirs (`.git`, harness worktrees), build
/// output, and symlinks (never followed — a link out of the repo must not
/// pull the outside world into the scan).
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_str().unwrap_or("");
        let Ok(ftype) = entry.file_type() else { continue };
        if ftype.is_symlink() || name.starts_with('.') {
            continue;
        }
        if ftype.is_dir() {
            if !SKIP_DIRS.contains(&name) {
                walk(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

#[test]
fn tree_carries_no_denylisted_names() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let denylist_path = root.join(DENYLIST);
    let Ok(raw) = fs::read_to_string(&denylist_path) else {
        return; // no denylist on this machine — nothing to enforce
    };
    let tokens: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_lowercase)
        .collect();

    let mut files = Vec::new();
    walk(root, &mut files);
    let mut hits = Vec::new();
    for file in &files {
        let Ok(bytes) = fs::read(file) else { continue };
        let text = String::from_utf8_lossy(&bytes).to_lowercase();
        for tok in &tokens {
            if text.contains(tok) {
                // Name the file but never the token: test output can land in
                // logs and commit messages.
                hits.push(format!(
                    "{} (token #{})",
                    file.strip_prefix(root).unwrap_or(file).display(),
                    tokens.iter().position(|t| t == tok).unwrap() + 1
                ));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "denylisted private names found (see {} for tokens):\n{}",
        DENYLIST,
        hits.join("\n")
    );
}
