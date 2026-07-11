//! Similar-code alignment (TKI-61): deterministic region matching between
//! two functions for the side-by-side view. Merkle-equal subtrees are exact
//! anchors (tier 1); statement-level then token-level DP alignment fills
//! the near-miss stretches between them (tier 2); everything else is
//! unmatched. No model involvement — the highlight is pure structure.
//!
//! Algorithm (GumTree-style top-down, kept monotonic left-to-right):
//! 1. If the two subtree roots hash-equal, the whole pair is one region —
//!    stop descending (there is nothing more precise to say about an
//!    identical subtree).
//! 2. Otherwise align the two child sequences with a size-weighted LCS: the
//!    DP maximizes total matched subtree size over exact-hash child pairs,
//!    ignoring children below `MIN_ANCHOR_NODES` (too small to be a
//!    meaningful anchor — left for statement-level handling instead, since
//!    matching e.g. two unrelated `x = 0` children would be noise).
//! 3. Matched children become regions directly (they're hash-equal, so
//!    `emit` just applies the raw-text tier rule). The gaps between/around
//!    matches are either a single equal-label pair (recurse — cheap, avoids
//!    falling back to statement mode for a lone `if`/`block`) or handed to
//!    statement-level alignment.
//! 4. Statement-level alignment linearizes each gap into "simple statements"
//!    (leaves of the statement grammar: `return`, `expression_statement`,
//!    etc. — compound statements are represented by their nested simple
//!    statements, never by themselves, so an edited line inside a big `if`
//!    doesn't force the whole `if` to be treated as one blob). Statements
//!    are compared by a token-level LCS similarity and aligned with another
//!    Needleman-Wunsch pass (gap score 0, so it never penalizes leaving a
//!    statement unmatched — only rewards matching one).
//!
//! Honesty gates (TKI-66): statement-level matching used to pair lines on
//! token mass alone, which let boilerplate (`self`, a generic return-value
//! name, call syntax) glue statements of unrelated functions together. Two
//! gates fix that:
//! - A statement pair is only accepted when the two raw texts share at
//!   least one DISTINCTIVE identifier — not a Python keyword, not in
//!   `STOPLIST_IDENTS`, not single-character (see the const's doc).
//!   Sharing only boilerplate can never create a match, even for
//!   hash-equal or byte-identical statements (`return result` alone is
//!   not evidence of shared structure). Non-hash-equal pairs must
//!   additionally share MORE than `DIST_JACCARD` of their combined
//!   distinctive vocabulary — normalization collapses attribute chains and
//!   calls to near-identical label sequences, so token-LCS similarity alone
//!   still pairs e.g. `self.stats.inc_value(...)` with
//!   `assert self.crawler.stats` (sim 0.75, one shared name out of five).
//! - Pair level: when the whole alignment produced ZERO structural anchors
//!   (hash-equal subtrees of `MIN_ANCHOR_NODES`+ nodes) AND the two
//!   functions' WL cosine is below `WL_FLOOR`, `align` returns no regions
//!   at all — the honest empty state. Statement-level near-misses are only
//!   trustworthy in the neighborhood of real structural agreement.
//!
//! Tier rule: the tier is decided by the raw TEXT, not the hash. A
//! hash-equal pair *looks* identical structurally, but normalization
//! abstracts literals and alpha-renames locals, so the raw source can
//! still differ (a renamed clone, a changed string literal) — demoted to
//! tier 2. Conversely, a DP-matched statement can be byte-identical while
//! hashing differently (alpha indices depend on the surrounding binding
//! order) — promoted to tier 1. `emit` re-slices both sides' source,
//! collapses whitespace, and compares: tier 1 means "identical code",
//! exactly and only.
//!
//! Determinism: every choice here is either a total order (sort by span) or
//! an explicit tie-break (documented at each DP), so `align` is a pure
//! function of its inputs — no `HashMap` iteration order ever leaks in,
//! because every collection here is a `Vec` indexed by position.

use crate::fingerprint;
use crate::types::NormTree;
use xxhash_rust::xxh3::xxh3_64;

/// A child pair is only accepted as an anchor when its subtree has at least
/// this many nodes; smaller hash-equal children (e.g. a shared `pass` or a
/// trivial `x = 0`) are left for statement-level alignment, which can still
/// match them there without them soaking up DP scoring as if they were
/// meaningful structural anchors.
const MIN_ANCHOR_NODES: u32 = 8;

/// Minimum token-LCS similarity for two statements to be reported as a
/// tier-2 region (unless they're already hash-equal). Raised from 0.5 in
/// TKI-66: at 0.5, half the token mass may disagree, and on normalized
/// labels (locals alpha-renamed, calls collapsed to `EXT`) call-syntax
/// boilerplate alone reaches that — the reproduced defect paired
/// `response = self.adapt_response(response)` with
/// `ret = iterate_spider_output(self.parse_row(response, row))` at
/// sim ≈ 0.55. 0.66 (two thirds of the tokens agree) clears every such
/// spurious pair in the scrapy-full sweep while the genuine sibling
/// alignments (HTTP11/H2 `__init__`s, `follow_all` kwargs blocks,
/// GZip/Deflate decoders) keep their regions — tuning table in TKI-66.
const SIM_THRESHOLD: f32 = 0.66;

/// Pair-level honesty floor: with zero structural anchors, statement-level
/// regions are only reported when the two functions' WL cosine (Channel A,
/// the caller passes it in) is at least this. 0.5 sits just under
/// θ_clone = 0.60: anything the clone channel would even consider "near"
/// stays alignable, while structurally unrelated pairs (the b= override,
/// the embedding neighbor of a symbol with no real sibling) get the honest
/// empty state instead of token-mass noise.
const WL_FLOOR: f32 = 0.5;

/// Distinctive-vocabulary Jaccard floor for NON-hash-equal statement pairs:
/// `shared / union` must exceed 1/4 (checked in integer math, `4·shared >
/// union`). Tuned on the scrapy-full sweep: the spurious cross-purpose
/// pairs share exactly one name out of 4–5 (≤ 0.25), while every genuine
/// sibling line observed sits at 2/7 (HTTP11/H2 pool init) or above.
/// Hash-equal pairs are exempt — identical structure is already strong
/// evidence; they only need one shared distinctive name.
const DIST_JACCARD: (usize, usize) = (1, 4);

/// Identifiers too ubiquitous to count as evidence that two statements are
/// about the same thing: receivers/variadics and generic value names. The
/// list is deliberately short and corpus-agnostic — domain nouns (e.g.
/// `response` in an HTTP library) stay distinctive; single-character names
/// are handled by a length rule in `is_distinctive`, not listed here.
const STOPLIST_IDENTS: &[&str] = &[
    "self", "cls", "args", "kwargs", // receivers and variadics
    "result", "ret", "res", "retval", "value", "val", "data", "out", "obj", "tmp",
    "item", // generic value names
];

/// Python keywords (incl. `True`/`False`/`None`) — structure, not vocabulary.
const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
    "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
    "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
    "try", "while", "with", "yield",
];

/// Below this many leaf tokens a statement is too short for its similarity
/// score to be meaningful (e.g. two single-token `pass` statements would
/// otherwise "match" at sim = 1.0 regardless of context).
const MIN_STMT_TOKENS: usize = 3;

/// Node kinds that count as one "simple statement" for linearization —
/// statements tile a gap; compound statements (`if`, `for`, `block`, ...)
/// are never collected themselves, only descended into.
const STATEMENT_KINDS: &[&str] = &[
    "expression_statement",
    "return_statement",
    "raise_statement",
    "assert_statement",
    "delete_statement",
    "global_statement",
    "nonlocal_statement",
    "import_statement",
    "import_from_statement",
    "pass_statement",
    "break_statement",
    "continue_statement",
];

/// One matched region pair. Byte spans are relative to each side's own
/// source slice (i.e. absolute NormTree span minus that side's base).
pub struct Region {
    pub a: (u32, u32),
    pub b: (u32, u32),
    /// 1 = exact: the two slices are identical after whitespace collapsing;
    /// 2 = near-miss: structurally aligned but the raw text differs
    /// (renamed locals, changed literals, edited statements).
    pub tier: u8,
}

/// Deterministic region alignment between two functions.
/// `a`/`b` are the two symbols' normalized trees (spans are ABSOLUTE file
/// byte offsets); `a_src`/`b_src` are each symbol's own source slice;
/// `a_base`/`b_base` are the file byte offsets where each slice starts
/// (so a node with span (s,e) maps to a_src[(s-a_base)..(e-a_base)]).
/// `wl_cos` is the pair's Channel-A (WL histogram) cosine — the caller
/// already has both histograms; it feeds only the pair-level honesty gate
/// (§ module doc) and never changes which statements match.
pub fn align(
    a: &NormTree, a_src: &str, a_base: u32,
    b: &NormTree, b_src: &str, b_base: u32,
    wl_cos: f32,
) -> Vec<Region> {
    if a.labels.is_empty() || b.labels.is_empty() {
        return Vec::new();
    }
    let mut ctx = Ctx {
        a,
        b,
        hash_a: fingerprint::subtree_hashes(a),
        hash_b: fingerprint::subtree_hashes(b),
        // Only `size_a` is needed: a hash-equal pair is structurally
        // isomorphic, so its two subtrees have the same node count.
        size_a: subtree_sizes(a),
        a_src,
        b_src,
        a_base,
        b_base,
        stmt_labels: STATEMENT_KINDS.iter().map(|k| xxh3_64(k.as_bytes())).collect(),
        regions: Vec::new(),
        anchored: false,
    };
    let root_a = (a.labels.len() - 1) as u32;
    let root_b = (b.labels.len() - 1) as u32;
    ctx.align_pair(root_a, root_b);
    if !ctx.anchored && wl_cos < WL_FLOOR {
        return Vec::new(); // the honest empty state (§ module doc)
    }
    ctx.regions.sort_by_key(|r| r.a.0);
    debug_assert!(non_overlapping(&ctx.regions), "align produced overlapping regions");
    ctx.regions
}

/// Subtree node counts, index-aligned to `tree`. Post-order guarantees every
/// child index is `< i`, so a bottom-up single pass suffices (same shape as
/// `fingerprint::subtree_hashes`).
fn subtree_sizes(tree: &NormTree) -> Vec<u32> {
    let mut sizes = vec![0u32; tree.labels.len()];
    for i in 0..tree.labels.len() {
        sizes[i] = 1 + tree.children[i].iter().map(|&c| sizes[c as usize]).sum::<u32>();
    }
    sizes
}

fn non_overlapping(regions: &[Region]) -> bool {
    for i in 0..regions.len() {
        for j in (i + 1)..regions.len() {
            let overlaps = |(s1, e1): (u32, u32), (s2, e2): (u32, u32)| s1 < e2 && s2 < e1;
            if overlaps(regions[i].a, regions[j].a) || overlaps(regions[i].b, regions[j].b) {
                return false;
            }
        }
    }
    true
}

/// Bundles the two trees/sources/hash+size tables threaded through every
/// recursive step, so `align_pair`/`align_gap`/`emit` don't each take ten
/// parameters.
struct Ctx<'a> {
    a: &'a NormTree,
    b: &'a NormTree,
    hash_a: Vec<u64>,
    hash_b: Vec<u64>,
    size_a: Vec<u32>,
    a_src: &'a str,
    b_src: &'a str,
    a_base: u32,
    b_base: u32,
    stmt_labels: Vec<u64>,
    regions: Vec<Region>,
    /// Whether any hash-equal pair of `MIN_ANCHOR_NODES`+ nodes was emitted
    /// (a recursion-rule-1 subtree, a matched child, or a Merkle-equal
    /// statement) — the structural evidence the pair-level honesty gate
    /// keys on.
    anchored: bool,
}

impl<'a> Ctx<'a> {
    /// Anchor + recursion step for one node pair. Hash-equal stops
    /// descending (rule 1); otherwise the child sequences are DP-aligned and
    /// the gaps between matches are handled recursively or statement-wise.
    fn align_pair(&mut self, na: u32, nb: u32) {
        if self.hash_a[na as usize] == self.hash_b[nb as usize] {
            if self.size_a[na as usize] >= MIN_ANCHOR_NODES {
                self.anchored = true;
            }
            self.emit(na, nb);
            return;
        }
        let children_a = self.a.children[na as usize].clone();
        let children_b = self.b.children[nb as usize].clone();
        let pairs = match_children(&children_a, &children_b, &self.hash_a, &self.hash_b, &self.size_a);

        let (mut cursor_a, mut cursor_b) = (0usize, 0usize);
        for (ia, ib) in pairs {
            self.align_gap(&children_a[cursor_a..ia], &children_b[cursor_b..ib]);
            self.anchored = true; // match_children pairs are MIN_ANCHOR_NODES+ by construction
            self.emit(children_a[ia], children_b[ib]);
            cursor_a = ia + 1;
            cursor_b = ib + 1;
        }
        self.align_gap(&children_a[cursor_a..], &children_b[cursor_b..]);
    }

    /// A gap of unmatched children between two anchors (or before the
    /// first/after the last). A lone equal-label pair recurses directly —
    /// no need to fall back to statement mode for a single `if`/`block`
    /// that just didn't hash-match its sibling. Everything else goes
    /// through statement-level near-miss alignment.
    fn align_gap(&mut self, gap_a: &[u32], gap_b: &[u32]) {
        if gap_a.is_empty() && gap_b.is_empty() {
            return;
        }
        if gap_a.len() == 1 && gap_b.len() == 1 && self.a.labels[gap_a[0] as usize] == self.b.labels[gap_b[0] as usize] {
            self.align_pair(gap_a[0], gap_b[0]);
            return;
        }
        let stmts_a = collect_statements(gap_a, self.a, &self.stmt_labels);
        let stmts_b = collect_statements(gap_b, self.b, &self.stmt_labels);
        if stmts_a.is_empty() || stmts_b.is_empty() {
            return;
        }
        for (i, j) in self.align_statements(&stmts_a, &stmts_b) {
            let (na, nb) = (stmts_a[i], stmts_b[j]);
            // A Merkle-equal statement of anchor size is structural evidence
            // too — the statement DP is where block-level children land when
            // the enclosing blocks differ, so anchors must be creditable here.
            if self.hash_a[na as usize] == self.hash_b[nb as usize]
                && self.size_a[na as usize] >= MIN_ANCHOR_NODES
            {
                self.anchored = true;
            }
            self.emit(na, nb);
        }
    }

    /// A statement's distinctive identifiers: the raw text of its identifier
    /// leaves, minus Python keywords, `STOPLIST_IDENTS`, and single-character
    /// names. Leaf spans are absolute file offsets; slices come off this
    /// side's own source. Sorted+deduped so the sharing check is a merge.
    fn distinctive_idents(&self, n: u32, side_a: bool) -> Vec<&'a str> {
        let (tree, src, base) = if side_a {
            (self.a, self.a_src, self.a_base)
        } else {
            (self.b, self.b_src, self.b_base)
        };
        let mut out: Vec<&str> = Vec::new();
        let mut stack = vec![n];
        while let Some(k) = stack.pop() {
            let kids = &tree.children[k as usize];
            if !kids.is_empty() {
                stack.extend_from_slice(kids);
                continue;
            }
            let (s, e) = tree.spans[k as usize];
            let Some(text) = src.get(s.saturating_sub(base) as usize..e.saturating_sub(base) as usize)
            else {
                continue;
            };
            if is_distinctive(text) {
                out.push(text);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Needleman-Wunsch alignment of two statement sequences (gap score 0,
    /// so a statement is never penalized for staying unmatched — only
    /// rewarded for matching). Returns the accepted pairs — an optimal
    /// diagonal step is only reported as a match when it clears the
    /// acceptance bar — as indices into `stmts_a`/`stmts_b`.
    ///
    /// Acceptance (TKI-66, § module doc): every statement pair — hash-equal
    /// or not — must share at least one distinctive identifier; sharing only
    /// keywords/boilerplate never creates a match. Non-hash-equal pairs
    /// additionally need `sim >= SIM_THRESHOLD`, both sides at least
    /// `MIN_STMT_TOKENS` tokens long, and distinctive-vocabulary Jaccard
    /// above `DIST_JACCARD`.
    ///
    /// Tie-break: identical to `match_children` — the diagonal wins ties
    /// against either skip direction (more information for the same score),
    /// and advancing A wins ties between the two skip directions.
    fn align_statements(&self, stmts_a: &[u32], stmts_b: &[u32]) -> Vec<(usize, usize)> {
        let (m, n) = (stmts_a.len(), stmts_b.len());
        let tokens_a: Vec<Vec<u64>> = stmts_a.iter().map(|&s| leaf_labels(s, self.a)).collect();
        let tokens_b: Vec<Vec<u64>> = stmts_b.iter().map(|&s| leaf_labels(s, self.b)).collect();
        let dist_a: Vec<Vec<&str>> = stmts_a.iter().map(|&s| self.distinctive_idents(s, true)).collect();
        let dist_b: Vec<Vec<&str>> = stmts_b.iter().map(|&s| self.distinctive_idents(s, false)).collect();

        let hash_eq = |i: usize, j: usize| {
            self.hash_a[stmts_a[i] as usize] == self.hash_b[stmts_b[j] as usize]
        };
        let sim = |i: usize, j: usize| {
            if hash_eq(i, j) { 1.0 } else { token_sim(&tokens_a[i], &tokens_b[j]) }
        };
        let accept = |i: usize, j: usize, s: f32| {
            let (shared, union) = dist_overlap(&dist_a[i], &dist_b[j]);
            if shared == 0 {
                return false;
            }
            if hash_eq(i, j) {
                return true;
            }
            s >= SIM_THRESHOLD
                && tokens_a[i].len() >= MIN_STMT_TOKENS
                && tokens_b[j].len() >= MIN_STMT_TOKENS
                && shared * DIST_JACCARD.1 > union * DIST_JACCARD.0
        };

        let mut dp = vec![vec![0f32; n + 1]; m + 1];
        for i in 1..=m {
            for j in 1..=n {
                let diag = dp[i - 1][j - 1] + sim(i - 1, j - 1);
                dp[i][j] = diag.max(dp[i - 1][j]).max(dp[i][j - 1]);
            }
        }
        let mut pairs = Vec::new();
        let (mut i, mut j) = (m, n);
        while i > 0 && j > 0 {
            let s = sim(i - 1, j - 1);
            if dp[i][j] == dp[i - 1][j - 1] + s {
                if accept(i - 1, j - 1, s) {
                    pairs.push((i - 1, j - 1));
                }
                i -= 1;
                j -= 1;
                continue;
            }
            if dp[i][j] == dp[i - 1][j] {
                i -= 1;
            } else {
                j -= 1;
            }
        }
        pairs.reverse();
        pairs
    }

    /// Emit a region for a matched node pair, applying the raw-text tier
    /// rule (§ module doc): tier is decided by the TEXT alone. Hash-equal
    /// anchors usually pass (identical structure, identical bytes), but the
    /// rule cuts both ways — a renamed/re-literaled anchor demotes to tier
    /// 2, and a DP-matched statement whose raw text is byte-identical
    /// promotes to tier 1 even though its Merkle hash differs (alpha-rename
    /// indices depend on the surrounding function's binding order, so the
    /// same `raise X(str(exc)) from exc` line hashes differently in two
    /// different bodies — the reader's question is "is this the same
    /// code?", and the text answers it).
    fn emit(&mut self, na: u32, nb: u32) {
        let (sa, ea) = self.a.spans[na as usize];
        let (sb, eb) = self.b.spans[nb as usize];
        debug_assert!(sa >= self.a_base && ea >= self.a_base, "span before symbol base");
        debug_assert!(sb >= self.b_base && eb >= self.b_base, "span before symbol base");
        let a_span = (sa.saturating_sub(self.a_base), ea.saturating_sub(self.a_base));
        let b_span = (sb.saturating_sub(self.b_base), eb.saturating_sub(self.b_base));
        let ta = collapse_ws(&self.a_src[a_span.0 as usize..a_span.1 as usize]);
        let tb = collapse_ws(&self.b_src[b_span.0 as usize..b_span.1 as usize]);
        let tier = if ta == tb { 1 } else { 2 };
        self.regions.push(Region { a: a_span, b: b_span, tier });
    }
}

/// Size-weighted LCS over two child-node sequences: maximize total subtree
/// size of matched exact-hash pairs, restricted to children whose subtree
/// has at least `MIN_ANCHOR_NODES` nodes. Returns matched pairs as indices
/// into `children_a`/`children_b`, ascending in both.
///
/// Tie-break: when a cell's optimum is reachable both by matching (the
/// diagonal) and by skipping a child, the diagonal wins — it surfaces
/// strictly more information (an anchor) for the same total score. Between
/// the two skip directions, advancing A is preferred (arbitrary but fixed,
/// so results never depend on which side happens to be "a" vs "b" only
/// through the DP's iteration, not through any incidental ordering).
fn match_children(children_a: &[u32], children_b: &[u32], hash_a: &[u64], hash_b: &[u64], size_a: &[u32]) -> Vec<(usize, usize)> {
    let (m, n) = (children_a.len(), children_b.len());
    let is_anchor = |i: usize, j: usize| -> Option<u32> {
        let (na, nb) = (children_a[i], children_b[j]);
        if hash_a[na as usize] == hash_b[nb as usize] && size_a[na as usize] >= MIN_ANCHOR_NODES {
            Some(size_a[na as usize])
        } else {
            None
        }
    };
    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            let mut best = dp[i - 1][j].max(dp[i][j - 1]);
            if let Some(w) = is_anchor(i - 1, j - 1) {
                best = best.max(dp[i - 1][j - 1] + w);
            }
            dp[i][j] = best;
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 && j > 0 {
        if let Some(w) = is_anchor(i - 1, j - 1) {
            if dp[i][j] == dp[i - 1][j - 1] + w {
                pairs.push((i - 1, j - 1));
                i -= 1;
                j -= 1;
                continue;
            }
        }
        if dp[i][j] == dp[i - 1][j] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    pairs.reverse();
    pairs
}

/// DFS-collect a gap's "simple statements" (see `STATEMENT_KINDS`): a node
/// whose label matches a target kind is collected and NOT descended into
/// (statements tile — no nesting); anything else is descended into looking
/// for statements inside it. Sorted by span start — `tree.children` is
/// already left-to-right, but the sort makes the order an explicit
/// guarantee rather than an incidental one.
fn collect_statements(nodes: &[u32], tree: &NormTree, targets: &[u64]) -> Vec<u32> {
    fn go(n: u32, tree: &NormTree, targets: &[u64], out: &mut Vec<u32>) {
        if targets.contains(&tree.labels[n as usize]) {
            out.push(n);
            return;
        }
        for &c in &tree.children[n as usize] {
            go(c, tree, targets, out);
        }
    }
    let mut out = Vec::new();
    for &n in nodes {
        go(n, tree, targets, &mut out);
    }
    out.sort_by_key(|&n| tree.spans[n as usize].0);
    out
}

/// A statement's token stream: the labels of its leaf nodes, in span order.
fn leaf_labels(n: u32, tree: &NormTree) -> Vec<u64> {
    fn go(n: u32, tree: &NormTree, out: &mut Vec<(u32, u64)>) {
        let kids = &tree.children[n as usize];
        if kids.is_empty() {
            out.push((tree.spans[n as usize].0, tree.labels[n as usize]));
        } else {
            for &c in kids {
                go(c, tree, out);
            }
        }
    }
    let mut pairs = Vec::new();
    go(n, tree, &mut pairs);
    pairs.sort_by_key(|&(s, _)| s);
    pairs.into_iter().map(|(_, l)| l).collect()
}

/// LCS length over two label sequences — a Needleman-Wunsch alignment with
/// match = +1 on equal labels and mismatch/gap = 0 reduces exactly to this.
fn lcs_length(a: &[u64], b: &[u64]) -> usize {
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }
    dp[m][n]
}

/// Token-level similarity of two statements' label sequences: twice the LCS
/// length over the summed lengths (so identical sequences score 1.0).
fn token_sim(a: &[u64], b: &[u64]) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    2.0 * lcs_length(a, b) as f32 / (a.len() + b.len()) as f32
}

/// Identifier-shaped raw text that carries vocabulary evidence: at least
/// two characters (single letters are loop/temp names), `[A-Za-z_]` head
/// with `[A-Za-z0-9_]` tail (so strings, numbers, and operators never
/// qualify), and neither a Python keyword nor a `STOPLIST_IDENTS` name.
fn is_distinctive(text: &str) -> bool {
    let mut chars = text.chars();
    let head_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    if !head_ok || text.len() < 2 || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    !PY_KEYWORDS.contains(&text) && !STOPLIST_IDENTS.contains(&text)
}

/// `(shared, union)` sizes of two sorted+deduped identifier lists — the
/// distinctive-vocabulary overlap `accept` gates on (a merge, no allocation).
fn dist_overlap(a: &[&str], b: &[&str]) -> (usize, usize) {
    let (mut i, mut j, mut shared) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    (shared, a.len() + b.len() - shared)
}

/// Collapse every run of ASCII whitespace to a single space and trim — the
/// raw-text tier rule compares "the same code modulo formatting", not byte
/// identity.
fn collapse_ws(s: &str) -> String {
    let mut out = String::new();
    let mut ws_pending = false;
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            ws_pending = !out.is_empty();
        } else {
            if ws_pending {
                out.push(' ');
            }
            ws_pending = false;
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::{self, ImportTable};
    use crate::parse;

    /// Build a NormTree for the single function in `src`, plus the byte
    /// range of its own source slice (decorators included, matching
    /// `scan.rs`'s `occ.root.byte_range()`).
    fn build(src: &str) -> (NormTree, u32, u32) {
        let tree = parse::parse(src.as_bytes());
        let funcs = parse::extract_functions(&tree, src.as_bytes(), "t.py");
        assert_eq!(funcs.len(), 1, "expected exactly one function in fixture");
        let norm = normalize::normalize(funcs[0].root, funcs[0].func, src.as_bytes(), &ImportTable::empty());
        let range = funcs[0].root.byte_range();
        (norm.tree, range.start as u32, range.end as u32)
    }

    fn align_srcs<'x>(a: &'x str, b: &'x str) -> Vec<Region> {
        let (ta, sa, ea) = build(a);
        let (tb, sb, eb) = build(b);
        // The same Channel-A cosine the production caller (explore.rs)
        // passes: WL histograms at the shipping wl_iters = 3.
        let wl_cos = fingerprint::cosine(
            &fingerprint::wl_histogram(&ta, 3),
            &fingerprint::wl_histogram(&tb, 3),
        );
        align(&ta, &a[sa as usize..ea as usize], sa, &tb, &b[sb as usize..eb as usize], sb, wl_cos)
    }

    /// The symbol's own byte width (its `function_definition` node span may
    /// exclude a trailing newline, so this is not necessarily `src.len()`).
    fn whole_width(src: &str) -> u32 {
        let (_, s, e) = build(src);
        e - s
    }

    #[test]
    fn identical_same_name_is_tier1_whole_region() {
        let a = "def f(x):\n    if x is None:\n        return 0\n    return x + 1\n";
        let b = "def f(x):\n    if x is None:\n        return 0\n    return x + 1\n";
        let regions = align_srcs(a, b);
        assert_eq!(regions.len(), 1, "regions: {:?}", regions.iter().map(|r| (r.a, r.b, r.tier)).collect::<Vec<_>>());
        assert_eq!(regions[0].tier, 1);
        assert_eq!(regions[0].a, (0, whole_width(a)));
        assert_eq!(regions[0].b, (0, whole_width(b)));
    }

    #[test]
    fn identical_different_name_is_tier2_whole_region() {
        // Same body, different def name: the name identifier normalizes to
        // EXT so the roots still hash-equal, but the raw text now differs.
        let a = "def f(x):\n    if x is None:\n        return 0\n    return x + 1\n";
        let b = "def g(x):\n    if x is None:\n        return 0\n    return x + 1\n";
        let regions = align_srcs(a, b);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].tier, 2);
        assert_eq!(regions[0].a, (0, whole_width(a)));
        assert_eq!(regions[0].b, (0, whole_width(b)));
    }

    #[test]
    fn renamed_local_clone_is_tier2_whole_region() {
        // normalize.rs's alpha-rename fixture: same shape, different local name.
        let a = "def f(dog):\n    if dog is None:\n        return 0\n    return dog + 1\n";
        let b = "def f(cat):\n    if cat is None:\n        return 0\n    return cat + 1\n";
        let regions = align_srcs(a, b);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].tier, 2);
        assert_eq!(regions[0].a, (0, whole_width(a)));
        assert_eq!(regions[0].b, (0, whole_width(b)));
    }

    #[test]
    fn disjoint_functions_produce_no_regions() {
        let a = "def f(x):\n    return f'{x}!'\n";
        let b = "def g(d):\n    total = 0\n    for k, v in d.items():\n        total += v\n    return total\n";
        let regions = align_srcs(a, b);
        assert!(regions.is_empty(), "expected no regions, got {:?}", regions.iter().map(|r| (r.a, r.b, r.tier)).collect::<Vec<_>>());
    }

    #[test]
    fn near_miss_edited_statement_keeps_anchor_and_flags_the_edit() {
        // Note: a *literal-only* edit (e.g. `split(',')` vs `split('\t')`)
        // would NOT exercise this path — normalize.rs abstracts every string
        // to a content-blind `STR` token (see `Builder::build_string`), so
        // the two functions would hash-equal at the root and rule 1 (§
        // module doc) would report one whole-function tier-2 region instead
        // of splitting into an anchor + a near-miss statement. Editing the
        // *shape* of one line (adding a `.strip()` call) instead makes only
        // that line's subtree hash differ, while the surrounding `rows = []`
        // and `return rows` statements still hash-equal — the scenario this
        // test is actually meant to cover.
        let a = "def parse(path):\n    rows = []\n    with open(path) as fh:\n        for line in fh:\n            rows.append(line.split(','))\n    return rows\n";
        let b = "def parse(path):\n    rows = []\n    with open(path) as fh:\n        for line in fh:\n            rows.append(line.strip().split(','))\n    return rows\n";
        let regions = align_srcs(a, b);
        assert!(regions.iter().any(|r| r.tier == 1), "expected at least one tier-1 anchor: {:?}", regions.iter().map(|r| (r.a, r.b, r.tier)).collect::<Vec<_>>());
        let edit = regions.iter().find(|r| r.tier == 2).unwrap_or_else(|| {
            panic!("expected the edited statement as a tier-2 region: {:?}", regions.iter().map(|r| (r.a, r.b, r.tier)).collect::<Vec<_>>())
        });
        let a_src = &a[edit.a.0 as usize..edit.a.1 as usize];
        let b_src = &b[edit.b.0 as usize..edit.b.1 as usize];
        assert!(a_src.contains("split"), "a slice: {a_src:?}");
        assert!(b_src.contains("split"), "b slice: {b_src:?}");
    }

    #[test]
    fn identical_statement_in_different_binding_context_promotes_to_tier1() {
        // The same `raise` line hashes DIFFERENTLY in the two bodies (alpha
        // indices shift with the surrounding bindings), so it can only pair
        // via the statement DP — but its raw text is byte-identical, and the
        // tier rule answers from the text (found on httpx's Decoder pair:
        // GZip vs Deflate `decode` share this exact raise).
        //
        // TKI-66 adjustment: the pair carries a shared `header = ...` anchor
        // (hash-equal, MIN_ANCHOR_NODES+) so the pair-level honesty gate
        // stays off — matching the real Decoder finding, where the raise
        // sits inside structurally sibling functions. A lone shared line in
        // an anchorless, WL-dissimilar pair is now the honest empty state
        // (see unrelated_pair_with_one_shared_line_gets_the_honest_empty_state).
        let a = "def f(x):\n    header = parse_header(x, 'strict')\n    try:\n        return g(x)\n    except Error as exc:\n        raise Wrapped(str(exc)) from exc\n";
        let b = "def f(x):\n    header = parse_header(x, 'strict')\n    y = x + 1\n    z = y * 2\n    try:\n        return g(z)\n    except Error as exc:\n        raise Wrapped(str(exc)) from exc\n";
        let regions = align_srcs(a, b);
        let raise_region = regions
            .iter()
            .find(|r| a[r.a.0 as usize..r.a.1 as usize].contains("raise Wrapped"))
            .expect("the shared raise statement pairs");
        assert_eq!(raise_region.tier, 1, "byte-identical text is tier 1 regardless of hash");
    }

    #[test]
    fn trivial_bodies_do_not_panic() {
        let a = "def f():\n    pass\n";
        let b = "def g():\n    pass\n";
        let regions = align_srcs(a, b);
        // Both are tiny single-node bodies (below MIN_ANCHOR_NODES doesn't
        // apply at the root itself — the whole function still hash-equals).
        assert!(regions.len() <= 1);
    }

    #[test]
    fn empty_normtree_yields_no_regions() {
        let empty = NormTree { labels: vec![], children: vec![], spans: vec![] };
        let (t, s, e) = build("def f():\n    pass\n");
        let src = "def f():\n    pass\n";
        assert!(align(&empty, "", 0, &t, &src[s as usize..e as usize], s, 0.0).is_empty());
        assert!(align(&t, &src[s as usize..e as usize], s, &empty, "", 0, 0.0).is_empty());
    }

    // ── TKI-66 honesty gates ──

    #[test]
    fn boilerplate_only_statement_overlap_yields_no_regions() {
        // The two `value = self.<call>(value)` lines are hash-equal after
        // normalization (locals alpha-rename, calls collapse to EXT) and
        // used to pair — but they share only stoplisted names (`self`,
        // `value`); the call names differ. The distinctive-token gate must
        // reject them, and nothing else here can match.
        let a = "def fa(self, value):\n    value = self.adapt(value)\n    items = [v.strip() for v in value]\n    return items\n";
        let b = "def fb(self, value):\n    while self.pending:\n        self.pending.pop()\n    value = self.transform(value)\n    return value\n";
        let regions = align_srcs(a, b);
        assert!(
            regions.is_empty(),
            "boilerplate-only sharing must not create regions: {:?}",
            regions.iter().map(|r| (r.a, r.b, r.tier)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unrelated_pair_with_one_shared_line_gets_the_honest_empty_state() {
        // A byte-identical `validate_schema(payload)` line in two otherwise
        // structurally unrelated functions: the statement WOULD pass the
        // distinctive gate (shared `validate_schema`/`payload`), but with
        // zero structural anchors and WL cosine far below the floor the
        // pair-level gate returns no regions — one shared line in an
        // unrelated body is not shared structure.
        let a = "def read_config(path):\n    payload = json.loads(path.read_text())\n    validate_schema(payload)\n    return payload\n";
        let b = "def sync_remote(client, bucket, payload):\n    for obj in client.list(bucket):\n        if obj.stale:\n            client.upload(obj)\n    validate_schema(payload)\n    while client.pending():\n        client.wait()\n";
        let regions = align_srcs(a, b);
        assert!(
            regions.is_empty(),
            "zero anchors + WL below floor must yield the empty state: {:?}",
            regions.iter().map(|r| (r.a, r.b, r.tier)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn distinctive_ident_shapes() {
        assert!(is_distinctive("adapt_response"));
        assert!(is_distinctive("payload"));
        assert!(is_distinctive("str")); // builtins are still vocabulary
        assert!(!is_distinctive("self"));
        assert!(!is_distinctive("result"));
        assert!(!is_distinctive("return"));
        assert!(!is_distinctive("x")); // single-character rule
        assert!(!is_distinctive("','")); // string slice, not an identifier
        assert!(!is_distinctive("42"));
        assert!(!is_distinctive(""));
    }

    #[test]
    fn deterministic_across_runs() {
        let a = "def parse(path):\n    rows = []\n    with open(path) as fh:\n        for line in fh:\n            rows.append(line.split(','))\n    return rows\n";
        let b = "def parse(path):\n    rows = []\n    with open(path) as fh:\n        for line in fh:\n            rows.append(line.strip().split(','))\n    return rows\n";
        let r1 = align_srcs(a, b);
        let r2 = align_srcs(a, b);
        assert_eq!(r1.len(), r2.len());
        for (x, y) in r1.iter().zip(r2.iter()) {
            assert_eq!((x.a, x.b, x.tier), (y.a, y.b, y.tier));
        }
    }

    #[test]
    fn regions_sorted_non_overlapping_and_in_bounds() {
        let a = "def parse(path):\n    rows = []\n    with open(path) as fh:\n        for line in fh:\n            rows.append(line.split(','))\n    return rows\n";
        let b = "def parse(path):\n    rows = []\n    with open(path) as fh:\n        for line in fh:\n            rows.append(line.strip().split(','))\n    return rows\n";
        let regions = align_srcs(a, b);
        assert!(regions.windows(2).all(|w| w[0].a.0 <= w[1].a.0));
        assert!(non_overlapping(&regions));
        for r in &regions {
            assert!(r.a.1 as usize <= a.len());
            assert!(r.b.1 as usize <= b.len());
        }
    }
}
