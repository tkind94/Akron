use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize)]
pub struct SymbolRef {
    pub file: String,
    pub qname: String,
    pub line: usize, // 1-based
}

/// Channel C dating (DESIGN.md §2.4), in local unix seconds. Absent when the
/// symbol's file has no git history at the scanned root.
#[derive(Clone, Copy)]
pub struct SymbolDates {
    pub first_seen: i64,
    pub last_touched: i64,
}

/// The per-symbol deterministic embedding (DESIGN.md §2.5, phase-0 subset:
/// Channels A and B; Channel C arrives in phase 1).
pub struct SymbolPrint {
    pub sym: SymbolRef,
    /// Byte range of the source subtree; used to skip nested parent/child pairs.
    pub span: (usize, usize),
    pub node_count: u32,
    pub merkle_root: u64,
    /// Channel A: Weisfeiler-Leman label histogram, sorted by label.
    pub wl: Vec<(u64, f32)>,
    pub minhash: Vec<u64>,
    /// Channel B: raw term frequencies; weighted into `Corpus::vocab_vecs`.
    pub vocab_tf: HashMap<String, u32>,
    /// Calls made in this symbol's body, resolved against the file's import
    /// bindings (see `callrel.rs`).
    pub calls: HashSet<Call>,
    pub is_test: bool,
    /// Channel C: file-level first-seen / last-touched dates; `None` when the
    /// scanned path has no git history.
    pub dating: Option<SymbolDates>,
}

/// Where a call's callee resolves, decided against the importing file's
/// bindings during normalization (`normalize::collect_imports`). `callrel`
/// turns this into edges against the corpus file layout.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ModuleRef {
    /// An absolute dotted module path the callee belongs to: `import psycopg`
    /// → `"psycopg"`; `from appdb.engine import connect` → `"appdb.engine"`.
    /// Relative imports are resolved to their absolute path form at collection
    /// time, so only this variant is needed here.
    Absolute(String),
}

/// One callee reference recorded from a `call` node (see `callrel.rs`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Call {
    /// The name to match in the target: the callee's final segment, or — for a
    /// `from mod import orig as alias` binding — the original name `orig` the
    /// target module actually defines.
    pub base: String,
    /// The module the callee resolves to, or `None` to keep the base-name join
    /// over the whole corpus (the fallback for locals, parameters, `self`/
    /// attribute chains, and any name that isn't an import binding).
    pub module: Option<ModuleRef>,
}

/// Normalized AST as a flat tree in post-order: children precede parents,
/// so the root is the LAST node.
pub struct NormTree {
    pub labels: Vec<u64>,
    pub children: Vec<Vec<u32>>,
    /// Byte range `(start, end)` in the original source of the tree-sitter node
    /// each node was derived from, in the same post-order as `labels`. Skipped
    /// nodes (comments, docstrings, punctuation) are absent, exactly as in
    /// `labels`/`children`. Passive data: the projection `regions.rs` uses to
    /// map a subtree-hash match back onto the member's source bytes.
    pub spans: Vec<(u32, u32)>,
}

pub struct Config {
    pub min_nodes: u32,
    pub wl_iters: usize,
    pub theta_clone: f32,
    pub theta_b: f32,
    pub theta_a_low: f32,
    /// Channel A average-linkage at/above which tight clusters + drifted
    /// variants assemble into a family (a coarser altitude than `theta_clone`).
    pub theta_family: f32,
    /// Channel B centroid cosine two units must share to merge into a family —
    /// the vocabulary-coherence gate that stops generic-shape blob chaining.
    pub theta_b_family: f32,
    pub top: usize,
}
