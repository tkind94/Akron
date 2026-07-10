//! LSH candidate pairing + union-find clustering for Channel A, and the
//! tf-idf vocabulary index for Channel B.

use crate::fingerprint::{cosine, MINHASH_FNS};
use crate::types::SymbolPrint;
use std::collections::{HashMap, HashSet};
use xxhash_rust::xxh3::xxh3_64;

// 32 bands × 2 rows: candidate recall stays high (~95%) down to Jaccard
// ~0.3, so mid-similarity family members surface as candidates; precision
// is restored downstream by the WL-cosine threshold, not by the LSH.
const LSH_BANDS: usize = 32;
const LSH_ROWS: usize = MINHASH_FNS / LSH_BANDS;
/// Buckets larger than this are generic shapes (e.g. tiny wrappers); pairing
/// them is quadratic noise. Logged, not silent (DESIGN.md: no silent caps).
const MAX_BUCKET: usize = 200;

pub struct UnionFind {
    parent: Vec<u32>,
}

impl UnionFind {
    pub fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n as u32).collect(),
        }
    }
    pub fn find(&mut self, i: u32) -> u32 {
        if self.parent[i as usize] != i {
            let root = self.find(self.parent[i as usize]);
            self.parent[i as usize] = root;
        }
        self.parent[i as usize]
    }
    pub fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra as usize] = rb;
        }
    }
    pub fn clusters(&mut self, n: usize) -> HashMap<u32, Vec<u32>> {
        let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
        for i in 0..n as u32 {
            out.entry(self.find(i)).or_default().push(i);
        }
        out.retain(|_, v| v.len() >= 2);
        out
    }
}

pub struct ShapeClusters {
    pub uf: UnionFind,
    pub funnel: RepeatedFunnel,
}

/// Where repeated-shape clustering narrowed, measured on the actual
/// clustering path (this module), so an empty repeated-shapes result is
/// explainable rather than a silent count — mirrors `CompetingFunnel` /
/// `DeprecatedFunnel` (queries.rs) and `FamilyFunnel` (family.rs). Every
/// field is a real count taken during `shape_clusters`, never reconstructed
/// from stats or config after the fact.
#[derive(Clone, Copy, Default)]
pub struct RepeatedFunnel {
    pub symbols_considered: usize,
    /// Generic-shape LSH buckets (>200 members) skipped entirely for
    /// pairing — quadratic noise, logged rather than silently dropped
    /// (folds in what used to be a standalone stats field).
    pub oversized_buckets: usize,
    /// Unique candidate pairs surfaced by LSH banding (deduped across bands
    /// via `seen`), drawn only from buckets small enough to pair.
    pub candidate_pairs: usize,
    /// Candidate pairs surviving the size-ratio and nesting guards.
    pub survived_guards: usize,
    /// Guard-surviving pairs whose cosine cleared `theta_clone` *and* whose
    /// merge cleared the representative anti-chaining check below — i.e.
    /// pairs that actually produced a union.
    pub survived_cosine: usize,
    /// Final multi-member clusters (exact-Merkle unions and LSH merges
    /// alike): `shapes.uf.clusters(n)`'s count.
    pub clusters_formed: usize,
}

/// Near-clones have near-equal sizes; pairs further apart than this ratio
/// are never merged (also blocks nested parent/child at the size level).
const MIN_SIZE_RATIO: f32 = 0.5;

/// The representative check runs at this fraction of theta: strict enough to
/// stop cross-family chaining into blobs, loose enough that a pattern family
/// with an internal drift gradient can still assemble around its core.
/// Shared with the competing-patterns union-find (queries.rs), which mirrors
/// this anti-chaining check on Channel B.
pub const REP_RELAX: f32 = 0.8;

fn nested(a: &SymbolPrint, b: &SymbolPrint) -> bool {
    a.sym.file == b.sym.file
        && (a.span.0 <= b.span.0 && b.span.1 <= a.span.1
            || b.span.0 <= a.span.0 && a.span.1 <= b.span.1)
}

/// Channel A clustering: exact Merkle identity, then LSH candidates merged
/// best-first with a representative check — a pair joins two clusters only if
/// the cluster *representatives* also clear the threshold. Plain single-link
/// union-find chains A~B~C into giant blobs; this is the anti-chaining guard.
pub fn shape_clusters(symbols: &[SymbolPrint], theta_clone: f32) -> ShapeClusters {
    let n = symbols.len();
    let mut uf = UnionFind::new(n);

    // Exact structural clones: same Merkle root (identical, no guard needed).
    let mut by_root: HashMap<u64, u32> = HashMap::new();
    for (i, s) in symbols.iter().enumerate() {
        match by_root.get(&s.merkle_root) {
            Some(&first) => uf.union(first, i as u32),
            None => {
                by_root.insert(s.merkle_root, i as u32);
            }
        }
    }

    // Near-miss candidates: LSH banding over MinHash signatures.
    let mut buckets: HashMap<u64, Vec<u32>> = HashMap::new();
    for (i, s) in symbols.iter().enumerate() {
        for band in 0..LSH_BANDS {
            let mut key = Vec::with_capacity(LSH_ROWS * 8 + 1);
            key.push(band as u8);
            for r in 0..LSH_ROWS {
                key.extend_from_slice(&s.minhash[band * LSH_ROWS + r].to_le_bytes());
            }
            buckets.entry(xxh3_64(&key)).or_default().push(i as u32);
        }
    }

    let mut oversized = 0usize;
    let mut candidate_pairs = 0usize;
    let mut survived_guards = 0usize;
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut cands: Vec<(f32, u32, u32)> = Vec::new();
    for members in buckets.values() {
        if members.len() < 2 {
            continue;
        }
        if members.len() > MAX_BUCKET {
            oversized += 1;
            continue;
        }
        for (ai, &a) in members.iter().enumerate() {
            for &b in &members[ai + 1..] {
                let pair = (a.min(b), a.max(b));
                if !seen.insert(pair) {
                    continue;
                }
                candidate_pairs += 1;
                let (sa, sb) = (&symbols[a as usize], &symbols[b as usize]);
                let ratio = sa.node_count.min(sb.node_count) as f32
                    / sa.node_count.max(sb.node_count) as f32;
                if ratio < MIN_SIZE_RATIO || nested(sa, sb) {
                    continue;
                }
                survived_guards += 1;
                let c = cosine(&sa.wl, &sb.wl);
                if c >= theta_clone {
                    cands.push((c, pair.0, pair.1));
                }
            }
        }
    }

    // Best-first merging with representative check. Candidates are collected
    // by iterating LSH buckets (a HashMap), so insertion order varies between
    // runs; a stable sort over just the cosine would let that order leak into
    // ties and change which merges happen first (not just display order —
    // the anti-chaining check below is order-sensitive). Break ties on the
    // pair itself so the merge sequence, and hence cluster membership, is
    // reproducible run to run.
    cands.sort_by(|x, y| y.0.total_cmp(&x.0).then((x.1, x.2).cmp(&(y.1, y.2))));
    let mut rep: Vec<u32> = (0..n as u32).collect();
    let mut size: Vec<u32> = vec![1; n];
    // Fold exact-clone unions into rep/size state.
    for i in 0..n as u32 {
        let r = uf.find(i);
        if r != i {
            size[r as usize] += 1;
            size[i as usize] = 0;
        }
    }
    let mut survived_cosine = 0usize;
    for (_, a, b) in cands {
        let (ra, rb) = (uf.find(a), uf.find(b));
        if ra == rb {
            continue;
        }
        let (pa, pb) = (rep[ra as usize], rep[rb as usize]);
        if cosine(&symbols[pa as usize].wl, &symbols[pb as usize].wl) < theta_clone * REP_RELAX {
            continue;
        }
        survived_cosine += 1;
        uf.union(ra, rb);
        let merged = uf.find(ra);
        let (bigger, total) = if size[ra as usize] >= size[rb as usize] {
            (pa, size[ra as usize] + size[rb as usize])
        } else {
            (pb, size[ra as usize] + size[rb as usize])
        };
        rep[merged as usize] = bigger;
        size[merged as usize] = total;
    }

    // Final multi-member cluster count (exact-Merkle unions + LSH merges
    // alike): the funnel's last stage, measured on the same union-find
    // `queries::repeated` will read — `clusters()` only path-compresses, it
    // never changes the equivalence classes, so counting here is safe to
    // call again downstream.
    let clusters_formed = uf.clusters(n).len();

    ShapeClusters {
        uf,
        funnel: RepeatedFunnel {
            symbols_considered: n,
            oversized_buckets: oversized,
            candidate_pairs,
            survived_guards,
            survived_cosine,
            clusters_formed,
        },
    }
}

// ── Channel B: tf-idf vocabulary vectors ──

pub struct VocabIndex {
    /// Per symbol: sorted (term_id, weight), L2-normalized.
    pub vecs: Vec<Vec<(u32, f32)>>,
    pub terms: Vec<String>,
    postings: HashMap<u32, Vec<u32>>, // term_id -> symbol ids, mid-df terms only
}

/// Terms in more than this fraction of symbols don't discriminate enough to
/// generate candidate pairs (they still contribute to the exact cosine).
const PAIRING_DF_FRACTION: f64 = 0.10;

pub fn vocab_index(symbols: &[SymbolPrint]) -> VocabIndex {
    let n = symbols.len();

    // Term ids are assigned by sorted term string, not first-seen order.
    // First-seen order falls out of iterating each symbol's `vocab_tf` (a
    // HashMap), whose iteration order is randomized per process — so any id
    // numbering derived from it would make every id-ordered float
    // accumulation downstream (cosine dot products, norms, centroids) sum
    // its terms in a different permutation on every run, a 1-ULP wobble even
    // though the underlying multiset of weights is identical. Sorting the
    // term set once up front makes id assignment, and therefore every
    // downstream summation order, a pure function of the corpus's contents.
    let mut term_set: HashSet<&str> = HashSet::new();
    for s in symbols {
        term_set.extend(s.vocab_tf.keys().map(String::as_str));
    }
    let mut terms: Vec<String> = term_set.iter().map(|&t| t.to_string()).collect();
    terms.sort_unstable();
    let term_ids: HashMap<&str, u32> = terms
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), i as u32))
        .collect();

    let mut df: Vec<u32> = vec![0; terms.len()];
    for s in symbols {
        for t in s.vocab_tf.keys() {
            df[term_ids[t.as_str()] as usize] += 1;
        }
    }

    let mut vecs = Vec::with_capacity(n);
    for s in symbols {
        let mut v: Vec<(u32, f32)> = s
            .vocab_tf
            .iter()
            .map(|(t, &tf)| {
                let id = term_ids[t.as_str()];
                let idf = (1.0 + n as f32 / df[id as usize] as f32).ln();
                (id, tf as f32 * idf)
            })
            .collect();
        v.sort_unstable_by_key(|&(id, _)| id); // canonical: id order == term-string order
        let norm: f32 = v.iter().map(|&(_, w)| w * w).sum::<f32>().sqrt();
        if norm > 0.0 {
            for e in v.iter_mut() {
                e.1 /= norm;
            }
        }
        vecs.push(v);
    }

    let df_cap = ((n as f64 * PAIRING_DF_FRACTION).ceil() as u32).max(4);
    let mut postings: HashMap<u32, Vec<u32>> = HashMap::new();
    for (i, s) in symbols.iter().enumerate() {
        for t in s.vocab_tf.keys() {
            let id = term_ids[t.as_str()];
            let d = df[id as usize];
            if d >= 2 && d <= df_cap {
                postings.entry(id).or_default().push(i as u32);
            }
        }
    }

    VocabIndex {
        vecs,
        terms,
        postings,
    }
}

impl VocabIndex {
    /// Candidate pairs sharing at least one discriminating term, with their
    /// exact vocabulary cosine ≥ theta_b.
    pub fn similar_pairs(&self, theta_b: f32) -> Vec<(u32, u32, f32)> {
        let mut seen: HashSet<(u32, u32)> = HashSet::new();
        let mut out = Vec::new();
        for members in self.postings.values() {
            for (ai, &a) in members.iter().enumerate() {
                for &b in &members[ai + 1..] {
                    let pair = (a.min(b), a.max(b));
                    if !seen.insert(pair) {
                        continue;
                    }
                    let c = cosine(&self.to_u64(pair.0), &self.to_u64(pair.1));
                    if c >= theta_b {
                        out.push((pair.0, pair.1, c));
                    }
                }
            }
        }
        out
    }

    fn to_u64(&self, i: u32) -> Vec<(u64, f32)> {
        self.vecs[i as usize]
            .iter()
            .map(|&(id, w)| (id as u64, w))
            .collect()
    }

    /// Top shared terms between two symbols, by combined weight.
    pub fn shared_terms(&self, a: u32, b: u32, k: usize) -> Vec<String> {
        let (va, vb) = (&self.vecs[a as usize], &self.vecs[b as usize]);
        let mut shared: Vec<(f32, u32)> = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < va.len() && j < vb.len() {
            match va[i].0.cmp(&vb[j].0) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    shared.push((va[i].1.min(vb[j].1), va[i].0));
                    i += 1;
                    j += 1;
                }
            }
        }
        // Tie-break on term id: `shared` is built by a merge-join over two
        // vectors that are already in canonical (id-sorted) order, so this
        // is deterministic — spelled out explicitly rather than relying on
        // sort_by's stability to carry the invariant silently.
        shared.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
        shared
            .iter()
            .take(k)
            .map(|&(_, id)| self.terms[id as usize].clone())
            .collect()
    }

    /// Channel B cosine between two symbols by id (thin wrapper over the
    /// same sorted-sparse-vector cosine `similar_pairs` uses internally).
    pub fn cosine_between(&self, a: u32, b: u32) -> f32 {
        cosine(&self.to_u64(a), &self.to_u64(b))
    }

    /// Every shared term between two symbols (id, combined weight = the
    /// smaller of the two per-symbol weights), unsorted and untruncated —
    /// the full set the shared-vocab quality gate (queries.rs) needs, unlike
    /// `shared_terms`'s display-oriented top-k.
    pub fn shared_term_weights(&self, a: u32, b: u32) -> Vec<(u32, f32)> {
        let (va, vb) = (&self.vecs[a as usize], &self.vecs[b as usize]);
        let mut shared = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < va.len() && j < vb.len() {
            match va[i].0.cmp(&vb[j].0) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    shared.push((va[i].0, va[i].1.min(vb[j].1)));
                    i += 1;
                    j += 1;
                }
            }
        }
        shared
    }

    /// A symbol's total (L1) tf-idf weight mass — the denominator for the
    /// shared-vocab quality gate's mass-fraction check.
    pub fn weight_mass(&self, i: u32) -> f32 {
        self.vecs[i as usize].iter().map(|&(_, w)| w).sum()
    }

    pub fn term_name(&self, id: u32) -> &str {
        &self.terms[id as usize]
    }
}
