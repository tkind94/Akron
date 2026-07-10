//! The pipeline entry point: repo path → per-symbol embeddings.
//! One engine, N call sites (CLI, tests; later hooks/MCP).

use crate::history::{self, History};
use crate::types::{Config, SymbolPrint};
use crate::{fingerprint, normalize, parse};
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tree_sitter::Node;

pub struct ScanOutput {
    pub symbols: Vec<SymbolPrint>,
    pub files: usize,
    pub skipped_small: usize,
    /// Channel C: `None` when `root` has no git history (scan still works).
    pub history: Option<History>,
    /// Wall time of parse + fingerprint (the file fan-out), for `--timings`.
    pub t_parse_fp: Duration,
    /// Wall time of the Channel-C history walk, for `--timings`.
    pub t_history: Duration,
}

pub fn scan_repo(root: &Path, cfg: &Config) -> ScanOutput {
    let t_pf = Instant::now();
    let files = parse::python_files(root);
    let per_file: Vec<(Vec<SymbolPrint>, usize)> = files
        .par_iter()
        .map(|f| process_file(f, root, cfg))
        .collect();

    let mut symbols = Vec::new();
    let mut skipped_small = 0usize;
    for (prints, skipped) in per_file {
        symbols.extend(prints);
        skipped_small += skipped;
    }
    let t_parse_fp = t_pf.elapsed();

    // Channel C: one history walk (IO at the edge), then attach file-level
    // dates to each symbol. Absent history leaves every `dating` at `None`.
    let t_h = Instant::now();
    let history = history::walk(root);
    if let Some(h) = &history {
        for s in &mut symbols {
            s.dating = h.dates_for(&s.sym.file);
        }
    }
    let t_history = t_h.elapsed();

    ScanOutput {
        symbols,
        files: files.len(),
        skipped_small,
        history,
        t_parse_fp,
        t_history,
    }
}

fn process_file(path: &Path, root: &Path, cfg: &Config) -> (Vec<SymbolPrint>, usize) {
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string();
    let Ok(source) = fs::read(path) else {
        eprintln!("warn: unreadable, skipping {rel}");
        return (Vec::new(), 0);
    };
    let tree = parse::parse(&source);
    let is_test = parse::is_test_path(&rel);
    // File-level import bindings: resolve call object-segments precisely so the
    // call-relation graph doesn't confuse third-party names with corpus symbols
    // that merely share a base name (see `callrel.rs`).
    let imports = normalize::collect_imports(tree.root_node(), &source, &rel);

    let mut prints = Vec::new();
    let mut skipped = 0usize;
    for occ in parse::extract_functions(&tree, &source, &rel) {
        // TKI-23: `@overload` stubs (bare or dotted, e.g. `@t.overload`) carry
        // no shape or vocabulary signal — their body is `...`. Drop them so
        // the real implementation is the single extracted symbol, instead of
        // fragmenting clusters with duplicate stub qnames.
        if is_overload_stub(occ.root, &source) {
            continue;
        }
        let norm = normalize::normalize(occ.root, occ.func, &source, &imports);
        let node_count = norm.tree.labels.len() as u32;
        if node_count < cfg.min_nodes {
            skipped += 1;
            continue;
        }
        let wl = fingerprint::wl_histogram(&norm.tree, cfg.wl_iters);
        let minhash = fingerprint::minhash(wl.iter().map(|&(l, _)| l));
        let range = occ.root.byte_range();
        prints.push(SymbolPrint {
            sym: occ.sym,
            span: (range.start, range.end),
            node_count,
            merkle_root: fingerprint::merkle_root(&norm.tree),
            wl,
            minhash,
            vocab_tf: norm.vocab_tf,
            calls: norm.calls,
            is_test,
            dating: None, // filled by the history walk in `scan_repo`
        });
    }
    (prints, skipped)
}

/// True when `root` is a `decorated_definition` carrying an `@overload`
/// decorator — bare (`@overload`) or a dotted path ending in `overload`
/// (`@t.overload`, `@typing.overload`). Calls and other decorator shapes
/// never match, so `@overload_something` or `@my_overload_helper` (which
/// merely contain the substring) are left alone.
fn is_overload_stub(root: Node, source: &[u8]) -> bool {
    root.kind() == "decorated_definition"
        && (0..root.child_count())
            .filter_map(|i| root.child(i))
            .filter(|c| c.kind() == "decorator")
            .any(|d| decorator_final_segment(d, source) == Some("overload"))
}

/// The final identifier segment of a decorator's expression: the name itself
/// for a bare identifier, or the last attribute for a dotted path. `None`
/// for anything else (calls, subscripts, ...), so those never match.
fn decorator_final_segment<'a>(decorator: Node, source: &'a [u8]) -> Option<&'a str> {
    let expr = decorator.named_child(0)?;
    match expr.kind() {
        "identifier" => expr.utf8_text(source).ok(),
        "attribute" => expr
            .child_by_field_name("attribute")
            .and_then(|a| a.utf8_text(source).ok()),
        _ => None,
    }
}
