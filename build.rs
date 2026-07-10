//! Embeds the short git SHA (plus a `-dirty` suffix if the tree has
//! uncommitted changes) into `AKRON_GIT_SHA` for `main.rs` to print via
//! `akron --version`. Falls back to "unknown" when git isn't available
//! (e.g. a crates.io/tarball build with no `.git` directory).
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let version = match sha {
        Some(sha) => {
            let dirty = Command::new("git")
                .args(["status", "--porcelain", "-uno"])
                .output()
                .map(|o| !o.stdout.is_empty())
                .unwrap_or(false);
            if dirty {
                format!("{sha}-dirty")
            } else {
                sha
            }
        }
        None => "unknown".to_string(),
    };

    println!("cargo:rustc-env=AKRON_GIT_SHA={version}");
}
