//! Family assembly (DESIGN.md §3 "drifting"): a second, coarser clustering
//! altitude above the tight repeated-shape clusters of `cluster.rs`.
//!
//! Real pattern families are internally graded: corpus-P (a private pipeline
//! corpus used for grading) has 21 per-source ingest jobs that are one house
//! pattern, but pairwise Channel-A cosine ranges 0.36–0.76, so at θ_clone they
//! surface as *fragments* — three tight clusters where a human sees one
//! family. This module reassembles the fragments.
//!
//! **Unit of assembly is a sub-cluster, not a symbol.** Each tight cluster from
//! `shape_clusters` is a *core* unit; each symbol in no tight cluster is a
//! *singleton* unit (a candidate drifted variant). Units merge at
//! `theta_family` (< θ_clone) so a drifted variant that missed the tight bar
//! still attaches to the family it belongs to.
//!
//! **Linkage: UPGMA average-linkage, cut at `theta_family`.** Two units' (and
//! two families') similarity is the *mean* Channel-A cosine over all their
//! member pairs, maintained exactly across merges by the Lance–Williams
//! size-weighted update. Measured on corpus-P, average-linkage separates
//! the graded ingest family (within-family mean 0.38–0.49) from its nearest
//! non-family neighbour (its landing-step helpers, ~0.30) with a clean gap — a
//! gap that medoid-to-medoid linkage collapses (ingest medoids 0.39–0.41
//! overlap the landing medoids 0.43–0.44). That is why the linkage is average,
//! not medoid.
//!
//! **Blob guards (the phase-0 lesson, re-applied one altitude up).** `cluster.rs`
//! prevents A~B~C single-link chaining with a representative check; the drift
//! gradient makes chaining *more* tempting here. Average-linkage supplies the
//! same protection structurally: a single close member-pair can never bridge
//! two otherwise-distant clusters, because the mean averages in every distant
//! pair too (co-merging an unrelated core through a shared bridging singleton
//! drops the recomputed mean below the cut). On top of that:
//!
//! 1. **Altitude cut above the noise floor.** `theta_family` (~0.35) sits above
//!    where unrelated Python sits in Channel A (row-builders 0.13, windows 0.16,
//!    fetchers ≤0.18 to the ingest core) — the primary separator — while below
//!    θ_clone (0.60) so graded families reunite.
//! 2. **Core anchor.** A family must contain ≥1 core unit (a real tight
//!    cluster) and span ≥2 units. A cloud of drifted singletons never forms a
//!    family; a lone tight cluster with nothing assembled is not a family —
//!    that is just the tight-cluster altitude the repeated query already
//!    reports. Coreless↔coreless pairs are never selected to merge.
//! 3. **Vocabulary-coherence gate (TKI-22).** Average-linkage *on Channel A* alone
//!    cannot stop a large *generic-shape* core from acting as an A-attractor once
//!    the symbol population is big: on scrapy (3,997 symbols) a 22-member cluster
//!    of `pytest.raises`/`warns` boilerplate — near-identical in Channel A because
//!    it normalizes class names to `EXT` — pulled a 51-member drift ring down to
//!    A-cos 0.31, mixing TestFormRequest, TestBaseSettings, `without_none_values`,
//!    SitemapSpider methods across 29 files. A bare A-floor cannot separate this
//!    from a real graded family: corpus-P's coherent 26-member ingest family
//!    drifts just as low (A-cos 0.305, its two landing helpers included).
//!    What differs is coherence in Channel B. The *centroid* cosine to the core
//!    does NOT separate them — it rewards a partial match to a broad generic-test
//!    centroid (gating on it left the blob almost intact). Channel-B **average-
//!    linkage** does: mean pairwise tf-idf cosine to *all* core members. A real
//!    family's drift relates to the whole core, so it stays up (corpus-P drift ≥
//!    0.161 to all 13 ingest core jobs, the graded fixture 0.56, httpx's verb-family
//!    constructor relatives 0.18–0.38); a generic-shape interloper relates only to
//!    its same-subject core members, so it drops (scrapy blob drift mostly ≤ 0.12,
//!    Limits.__init__ into httpx's verb family 0.00). So a merge is gated on a
//!    second, orthogonal linkage — the candidate's Channel-B average-linkage to the
//!    family's *stable core* (never the drifting node, which would re-broaden into
//!    an attractor) must clear `theta_b_family`. This is the phase-0 representative
//!    check (`cluster.rs` REP_RELAX) re-expressed one altitude up and one channel
//!    over — A drives *candidacy* (the merge order), B *gates* it — and it reuses
//!    the same sum-vector factorization as Channel A (`⟨Σ,Σ⟩/(cntᵢ·cntⱼ)` is one
//!    sparse dot, merges additively), so the gate is O(1) per candidate and the
//!    TKI-20 sparsity holds. Measured drift minima that fixed `theta_b_family=0.16`
//!    (B average-linkage to core):  KEEP corpus-P ingest 0.161 · fixture 0.56 ·
//!    httpx verbs 0.18  |  DROP scrapy blobs ≤0.12 · httpx Limits.__init__ 0.00.
//!    Rejected alternatives, each measured: scale-aware θ_family (the ingest family
//!    & blobs share A-cos ~0.31 — no A-cut separates them); core B-coherence gate
//!    (ingest core 0.23 ≈ scrapy blob core 0.21 — overlap); B-centroid cosine
//!    (broad generic centroids pass partial matches); A medoid rep-check (DESIGN
//!    §3.1: collapses the ingest/landing gap). The ingest family assembles through
//!    0.18, then fragments at a 0.20 core↔core cliff, so 0.16 sits with margin on
//!    both sides.
//! 4. **Dunder role guard (TKI-33).** Neither linkage stops within-class role
//!    mixing: on corpus-R (a private review-app corpus used for grading), three
//!    `__init__` methods (cos_a 0.33–0.43)
//!    glued onto a suite-runner `run` family — both roles read as "rows of
//!    `self.x = …` plus calls" at θ_family, and same-class methods share field
//!    vocabulary, so the Channel-B gate (built against cross-subsystem blobs,
//!    not within-class roles) passed them too. A symbol's role — its qname's
//!    final `.`-segment — is either a dunder (`__x__`: double-underscore
//!    prefix/suffix, length > 4) or not; a unit (sub-cluster or singleton) is a
//!    dunder unit iff any member is. A merge is rejected outright when it would
//!    join a dunder unit with a non-dunder unit, or two units tagged with
//!    *different* dunders (`__init__` vs `__repr__`) — no cosine involved, this
//!    is a deterministic name check ahead of both linkages. Same-dunder units
//!    (init with init) still merge: that is genuine repetition, not role
//!    mixing. Rejections are counted separately from the vocabulary gate
//!    (`FamilyFunnel::role_rejected`), since they fire for a different reason.

use crate::cluster::ShapeClusters;
use crate::fingerprint::cosine;
use crate::types::SymbolPrint;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

/// Assemble families at this Channel-A average-linkage cosine (default;
/// overridable via CLI). Chosen from the corpus-P gap: the graded ingest
/// family holds together at ≥0.38, the nearest non-family neighbour sits at
/// ~0.30, so 0.35 splits them and stays well clear of the ~0.2 unrelated floor.
pub const THETA_FAMILY: f32 = 0.35;

/// Minimum Channel-B **average-linkage** (mean pairwise tf-idf cosine) between a
/// merge candidate and a family's core members for them to merge (the vocabulary-
/// coherence gate, blob-guard #3). Chosen from the measured A/B separation
/// (TKI-22): a real graded family's drift relates to the *whole* core, so its
/// B average-linkage stays up — corpus-P's ingest family ≥ 0.161 to all 13 core
/// jobs (its two landing helpers at 0.23/0.29), the fixture parse-records lineage
/// 0.56, httpx's verb-family constructor relatives 0.18–0.38 — whereas a generic-
/// shape interloper relates to only a few same-subject core members, so it sits
/// low: scrapy's blob drift is mostly ≤ 0.12 (Limits.__init__ into httpx's verb
/// family 0.00, a telnet port-range test 0.07). 0.16 splits them: it keeps the
/// ingest family (assembles at 26 members through 0.18, then fragments at a 0.20
/// core↔core cliff) and every genuine family, while shedding the vocabulary-
/// disjoint drift and blocking generic core↔core chaining that A-linkage alone
/// allowed (scrapy's 73- and 70-member cross-subsystem blobs break up; the
/// feed-export family drops 45→29 members / 16→10 files). 0.16 sits with margin
/// below the ingest family's fragmentation cliff and above the generic-vocab
/// noise the blobs chain on.
pub const THETA_B_FAMILY: f32 = 0.16;

/// One member of a family, tagged with its distance from the family core.
pub struct FamilyMember {
    pub sym: u32,
    /// Channel-A cosine to the core representative (1.0 for the core medoid).
    pub cos_to_core: f32,
    /// Channel-B (tf-idf vocabulary) cosine to the core representative (1.0 for
    /// the core medoid). The scatter's y-axis in the HTML report (TKI-31);
    /// carried here so the report needs no second vocab pass. Additive: the
    /// per-symbol b-vectors already flow into `assemble` for the coherence gate.
    pub b_cos_to_core: f32,
    /// Whether this symbol belongs to the family's core sub-cluster.
    pub is_core: bool,
}

pub struct Family {
    /// The core representative: medoid of the largest core sub-cluster.
    pub core_medoid: u32,
    /// All members, drift-ordered: core sub-cluster first (nearest-to-core
    /// first), then drifted members (nearest-to-core first).
    pub members: Vec<FamilyMember>,
    /// How many sub-clusters (core clusters + singletons) were assembled (≥2).
    pub sub_clusters: usize,
    /// Size of the core sub-cluster.
    pub core_size: usize,
    pub n_files: usize,
    /// Similarity spread: min / max Channel-A cosine-to-core over the
    /// non-medoid members (the drift gradient's extremes).
    pub cos_to_core_min: f32,
    pub cos_to_core_max: f32,
    pub all_test: bool,
}

/// Where the assembly narrowed, so an empty families section is explainable
/// rather than a silent zero (mirrors the query funnels in `queries.rs`).
#[derive(Default)]
pub struct FamilyFunnel {
    pub core_clusters: usize,
    pub singletons: usize,
    pub merges: usize,
    /// Core-adjacent unit pairs that cleared the Channel-A cut but were blocked
    /// by the Channel-B vocabulary-coherence gate (blob-guard #3, TKI-22).
    pub guard_rejected: usize,
    /// Core-adjacent unit pairs that cleared the Channel-A cut but were blocked
    /// by the dunder role guard (blob-guard #4, TKI-33): joining a dunder unit
    /// with a non-dunder unit, or two units tagged with different dunders.
    pub role_rejected: usize,
    pub families: usize,
}

pub struct FamilyResult {
    pub families: Vec<Family>,
    pub funnel: FamilyFunnel,
}

/// A sub-cluster feeding the family altitude.
struct Unit {
    members: Vec<u32>,
    /// Members of a tight cluster (≥2); 0 for a singleton. `core_size > 0`
    /// marks a *core* unit.
    core_size: usize,
}

/// The member of `members` most central by Channel-A cosine (max total cosine
/// to the rest); ties break to the lowest symbol id for determinism. Public so
/// other readouts (report JSON, explore) can reuse the same medoid the
/// family readout uses.
pub fn medoid(symbols: &[SymbolPrint], members: &[u32]) -> u32 {
    if members.len() == 1 {
        return members[0];
    }
    let mut best = members[0];
    let mut best_score = f32::NEG_INFINITY;
    for &m in members {
        let score: f32 = members
            .iter()
            .filter(|&&o| o != m)
            .map(|&o| cosine(&symbols[m as usize].wl, &symbols[o as usize].wl))
            .sum();
        if score > best_score || (score == best_score && m < best) {
            best_score = score;
            best = m;
        }
    }
    best
}

/// Mean pairwise Channel-A cosine within a sub-cluster — how tight it is.
fn tightness(symbols: &[SymbolPrint], members: &[u32]) -> f32 {
    if members.len() < 2 {
        return 1.0;
    }
    let (mut sum, mut count) = (0.0f32, 0u32);
    for (i, &a) in members.iter().enumerate() {
        for &b in &members[i + 1..] {
            sum += cosine(&symbols[a as usize].wl, &symbols[b as usize].wl);
            count += 1;
        }
    }
    sum / count as f32
}

/// Sum of the members' L2-normalized WL vectors, as one sorted sparse vector.
/// Its dot with another unit's sum-vector is the un-normalized average linkage:
/// `⟨Σ x̂, Σ ŷ⟩ = Σ_{x,y} cos(x,y)`, so dividing by `|A|·|B|` gives the exact
/// mean cross-pair cosine (average linkage) in a single dot.
///
/// Accumulation order is fixed (members are pre-sorted; a *stable* label sort
/// keeps per-label additions in member order), so the float sum is bit-for-bit
/// reproducible — no HashMap iteration-order nondeterminism.
fn sum_normalized(symbols: &[SymbolPrint], members: &[u32]) -> Vec<(u64, f32)> {
    if members.len() == 1 {
        return normalized(&symbols[members[0] as usize].wl);
    }
    let mut all: Vec<(u64, f32)> = Vec::new();
    for &m in members {
        all.extend(normalized(&symbols[m as usize].wl));
    }
    all.sort_by_key(|&(l, _)| l); // stable: preserves member order within a label
    let mut out: Vec<(u64, f32)> = Vec::new();
    for (l, w) in all {
        match out.last_mut() {
            Some(last) if last.0 == l => last.1 += w,
            _ => out.push((l, w)),
        }
    }
    out
}

/// L2-normalize a sorted sparse WL vector (already sorted by label).
fn normalized(wl: &[(u64, f32)]) -> Vec<(u64, f32)> {
    let norm: f32 = wl.iter().map(|&(_, w)| w * w).sum::<f32>().sqrt();
    if norm == 0.0 {
        return wl.to_vec();
    }
    wl.iter().map(|&(l, w)| (l, w / norm)).collect()
}

/// Merge-add two sorted sparse vectors (the Lance–Williams UPGMA update in
/// vector form: `Σ_{A∪B} = Σ_A + Σ_B`). Deterministic given sorted inputs.
fn sparse_add(a: &[(u64, f32)], b: &[(u64, f32)]) -> Vec<(u64, f32)> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push((a[i].0, a[i].1 + b[j].1));
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

/// All unit pairs whose average linkage ≥ `theta`. WL cosine vectors are dense
/// with near-uniform post-normalization weights, so inverted-index / prefix
/// pruning does not pay (every vector would index most of its features); the
/// exact all-pairs dot is instead run in parallel across `i`. Each pair is one
/// sparse dot (the sum-vector factorization), and the row split is disjoint, so
/// this is deterministic regardless of thread scheduling — the agglomeration
/// selects by an explicit (similarity, index) order, not by edge arrival order.
fn theta_edges(
    unit_sum: &[Vec<(u64, f32)>],
    sizes: &[f32],
    theta: f32,
) -> Vec<(usize, usize, f32)> {
    let u0 = unit_sum.len();
    (0..u0)
        .into_par_iter()
        .flat_map_iter(|i| {
            let mut local = Vec::new();
            for j in (i + 1)..u0 {
                let s = dot(&unit_sum[i], &unit_sum[j]) / (sizes[i] * sizes[j]);
                if s >= theta {
                    local.push((i, j, s));
                }
            }
            local
        })
        .collect()
}

/// Dot product of two sorted sparse vectors.
fn dot(a: &[(u64, f32)], b: &[(u64, f32)]) -> f32 {
    let (mut i, mut j, mut acc) = (0, 0, 0.0f32);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                acc += a[i].1 * b[j].1;
                i += 1;
                j += 1;
            }
        }
    }
    acc
}

/// A unit's Channel-B vocabulary sum-vector: the sum of its members' (already
/// L2-normalized) tf-idf vectors, as one sorted sparse u64-keyed vector — the
/// Channel-B parallel of `sum_normalized`. Cosine between two units' sum-vectors
/// is their vocabulary-centroid agreement; it merges additively (`sparse_add`),
/// so the coherence gate reuses the TKI-20 sum-vector factorization exactly.
/// Members' B-vectors arrive already normalized from `vocab_index`, so this is a
/// plain deterministic merge-sum (stable sort preserves per-label member order).
fn b_sum(b_vecs: &[Vec<(u32, f32)>], members: &[u32]) -> Vec<(u64, f32)> {
    let mut all: Vec<(u64, f32)> = Vec::new();
    for &m in members {
        all.extend(b_vecs[m as usize].iter().map(|&(id, w)| (id as u64, w)));
    }
    all.sort_by_key(|&(l, _)| l); // stable
    let mut out: Vec<(u64, f32)> = Vec::new();
    for (l, w) in all {
        match out.last_mut() {
            Some(last) if last.0 == l => last.1 += w,
            _ => out.push((l, w)),
        }
    }
    out
}

/// Vocabulary-coherence between two nodes: Channel-B **average-linkage** — the
/// mean pairwise tf-idf cosine over the cross-product of their representative
/// members. A node that owns a core is represented by its CORE members (the
/// stable `core_b` sum and `core_cnt` count); a coreless singleton by its own one
/// member (`full_b`, count 1). For a singleton joining a core this is exactly the
/// singleton's mean B-cosine to the core members — the quantity the guard
/// thresholds. Average- not centroid-linkage on purpose: centroid-cosine rewards
/// a partial match to a broad generic-test centroid (which left scrapy's blob
/// intact), whereas average-linkage demands the candidate relate to the *whole*
/// core — which a real graded family's drift does (corpus-P ingest ≥ 0.161 to
/// all core jobs) and a generic-shape interloper does not (scrapy blob mostly
/// ≤ 0.12, relating to only its same-subject core members). Member B-vectors are
/// already L2-normalized, so `⟨Σ,Σ⟩/(cntᵢ·cntⱼ)` is the exact mean cross cosine.
#[allow(clippy::too_many_arguments)]
fn coherence(
    full_b: &[Vec<(u64, f32)>],
    core_b: &[Vec<(u64, f32)>],
    core_cnt: &[usize],
    has_core: &[bool],
    i: usize,
    j: usize,
) -> f32 {
    let (vi, ci) = if has_core[i] {
        (&core_b[i], core_cnt[i])
    } else {
        (&full_b[i], 1usize)
    };
    let (vj, cj) = if has_core[j] {
        (&core_b[j], core_cnt[j])
    } else {
        (&full_b[j], 1usize)
    };
    if ci == 0 || cj == 0 {
        return 0.0;
    }
    dot(vi, vj) / (ci as f32 * cj as f32)
}

/// qname's final `.`-segment, e.g. `"Loader.__init__"` -> `"__init__"`
/// (mirrors `callrel::base_name` / `explain::base_name`; each module keeps its
/// own copy rather than a shared export).
fn base_name(qname: &str) -> &str {
    qname.rsplit('.').next().unwrap_or(qname)
}

/// Whether a base name is a dunder: starts and ends with a double underscore,
/// and is longer than 4 characters (`__` + \u{2265}1 char + `__`), e.g.
/// `__init__`, `__repr__` (TKI-33).
fn is_dunder(name: &str) -> bool {
    name.len() > 4 && name.starts_with("__") && name.ends_with("__")
}

/// A unit's dunder role guard tag (TKI-33, blob-guard #4): `None` if none of
/// its members' base names are a dunder; otherwise the dunder name found
/// (pre-merge units are not expected to mix two different dunders, or a
/// dunder with a non-dunder member, in one unit — that is exactly what the
/// guard below keeps a *merge* from creating).
fn unit_role<'a>(symbols: &'a [SymbolPrint], members: &[u32]) -> Option<&'a str> {
    members
        .iter()
        .map(|&s| base_name(&symbols[s as usize].sym.qname))
        .find(|n| is_dunder(n))
}

/// Whether merging the nodes at `i` and `j` would violate the dunder role
/// guard (TKI-33): joining a dunder unit with a non-dunder unit, or two units
/// tagged with different dunders. `None` (no dunder members) merges with
/// anything; `Some(x)` only merges with `Some(x)` — same-dunder repetition
/// (init with init) is exactly what the family altitude is for.
fn role_blocks(role: &[Option<&str>], i: usize, j: usize) -> bool {
    match (role[i], role[j]) {
        (None, None) => false,
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

/// Assemble families over the tight clusters and singletons of `shapes` by
/// UPGMA average-linkage cut at `theta_family` (Channel A), with every merge
/// gated on Channel-B vocabulary agreement at `theta_b_family` (blob-guard #3).
/// `b_vecs` is the per-symbol tf-idf vocabulary vectors (`VocabIndex::vecs`).
pub fn assemble(
    symbols: &[SymbolPrint],
    shapes: &mut ShapeClusters,
    b_vecs: &[Vec<(u32, f32)>],
    theta_family: f32,
    theta_b_family: f32,
) -> FamilyResult {
    let n = symbols.len();

    // ── build units: tight clusters (cores) + lone symbols (singletons) ──
    // Deterministic order (DESIGN.md Principle 1): cores sorted by their lowest
    // member id, then singletons in symbol-id order. The tight-cluster set
    // arrives as an unordered map, so this sort is what makes assembly
    // bit-for-bit reproducible.
    let tight = shapes.uf.clusters(n);
    let mut in_cluster = vec![false; n];
    let mut cores: Vec<Vec<u32>> = tight
        .into_values()
        .map(|mut m| {
            m.sort_unstable();
            for &s in &m {
                in_cluster[s as usize] = true;
            }
            m
        })
        .collect();
    cores.sort_by_key(|m| m[0]);

    let mut units: Vec<Unit> = cores
        .into_iter()
        .map(|m| Unit {
            core_size: m.len(),
            members: m,
        })
        .collect();
    let core_clusters = units.len();
    for i in 0..n as u32 {
        if !in_cluster[i as usize] {
            units.push(Unit {
                members: vec![i],
                core_size: 0,
            });
        }
    }
    let singletons = units.len() - core_clusters;

    // ── sparse average-linkage agglomeration over units ──
    // Each unit's sum-vector: the sum of its members' L2-normalized WL vectors.
    // Average-linkage factorizes exactly through it —
    //   mean_{x∈A,y∈B} cos(x,y) = ⟨Σ_x x̂, Σ_y ŷ⟩ / (|A|·|B|)  —
    // so a unit pair costs one sparse dot regardless of member counts, and
    // sum-vectors merge additively (Σ_{A∪B} = Σ_A+Σ_B) — the Lance–Williams
    // UPGMA update in vector form. Instead of the O(u0²) dense fill, an exact
    // prefix-filter join seeds only the ≥theta edges (the rest can never drive
    // a merge). Because a merged edge is a size-weighted mean —
    // sim[A∪B][k] = (|A|·sim[A][k] + |B|·sim[B][k]) / (|A|+|B|) ≤ max(…) — an
    // above-theta edge can only appear where one component already had one, so
    // after each merge we recompute edges to the *union of the two endpoints'
    // neighbours* and keep those still ≥theta. This is the same partition the
    // dense matrix produces (a singleton that reaches no unit at ≥theta stays a
    // singleton, exactly as the old max-link prune dropped it), computed sparsely.
    let unit_sum: Vec<Vec<(u64, f32)>> = units
        .iter()
        .map(|u| sum_normalized(symbols, &u.members))
        .collect();
    let sizes: Vec<f32> = units.iter().map(|u| u.members.len() as f32).collect();
    let u0 = units.len();

    // Dunder role guard (blob-guard #4, TKI-33): each unit's role tag, fixed at
    // construction. A merge is only permitted when both sides agree (see
    // `role_blocks`), so a surviving node's tag never changes across merges —
    // no per-merge update is needed, unlike `core_b`/`core_cnt` below.
    let role: Vec<Option<&str>> = units.iter().map(|u| unit_role(symbols, &u.members)).collect();

    // Channel-B vocabulary sum-vectors per node (blob-guard #3). The gate measures
    // a merge candidate's B average-linkage to the family's *core* members — the
    // STABLE anchor, not the whole (drifting) node: if drift shifted the reference,
    // a large generic-shape core would broaden into a "generic test" attractor that
    // each weakly-related member then clears (measured: gating on the drifting node
    // reference left scrapy's 73-member deprecation blob almost intact). So each
    // node carries `core_b`/`core_cnt` (Σ B̂ and count over its CORE members only —
    // grows only when two cores merge, never from drift) and `full_b` (the single
    // coreless singleton's own vector; coreless↔coreless never merges, so it never
    // grows). A node's representative is its core once it owns one, else its lone
    // member. Both sum-vectors merge additively, so the TKI-20 sparsity holds.
    let full_b: Vec<Vec<(u64, f32)>> = units.iter().map(|u| b_sum(b_vecs, &u.members)).collect();
    let mut core_b: Vec<Vec<(u64, f32)>> = units
        .iter()
        .map(|u| {
            if u.core_size > 0 {
                b_sum(b_vecs, &u.members)
            } else {
                Vec::new()
            }
        })
        .collect();
    let mut core_cnt: Vec<usize> = units.iter().map(|u| u.core_size).collect();

    let edges = theta_edges(&unit_sum, &sizes, theta_family);

    // Live agglomeration state, per node (a node id survives as the lower index
    // when two merge, mirroring the dense loop's `merge j into i`).
    let mut s_vec = unit_sum;
    let mut size: Vec<usize> = units.iter().map(|u| u.members.len()).collect();
    let mut has_core: Vec<bool> = units.iter().map(|u| u.core_size > 0).collect();
    let mut active = vec![true; u0];
    let mut unit_ids: Vec<Vec<usize>> = (0..u0).map(|i| vec![i]).collect();
    let mut nbr: Vec<HashMap<usize, f32>> = vec![HashMap::new(); u0];
    // Seed only A-adjacent pairs that also clear the dunder role guard and the
    // Channel-B coherence gate; a role-mismatched or B-disjoint pair can never
    // drive a family merge (blob-guards #4 and #3). Blocked core-adjacent pairs
    // are counted for the funnel, each under its own reason.
    let mut guard_rejected = 0usize;
    let mut role_rejected = 0usize;
    for &(i, j, s) in &edges {
        if role_blocks(&role, i, j) {
            if has_core[i] || has_core[j] {
                role_rejected += 1;
            }
            continue;
        }
        if coherence(&full_b, &core_b, &core_cnt, &has_core, i, j) < theta_b_family {
            if has_core[i] || has_core[j] {
                guard_rejected += 1;
            }
            continue;
        }
        nbr[i].insert(j, s);
        nbr[j].insert(i, s);
    }

    // Repeatedly merge the closest edge that involves ≥1 core (coreless↔coreless
    // is never selected — the core-anchor guard), max similarity with lowest
    // (i,j) tie-break, until no edge clears the cut. Average-linkage's mean keeps
    // a bridging singleton from dragging two distant cores together.
    let mut merges = 0usize;
    loop {
        let mut best = (theta_family, usize::MAX, usize::MAX);
        for i in 0..u0 {
            if !active[i] {
                continue;
            }
            for (&j, &s) in &nbr[i] {
                if i >= j || !active[j] || !(has_core[i] || has_core[j]) {
                    continue;
                }
                // ≥ with lower-index tie-break: deterministic selection.
                if s >= best.0 && (s > best.0 || (i, j) < (best.1, best.2)) {
                    best = (s, i, j);
                }
            }
        }
        let (_, i, j) = best;
        if i == usize::MAX {
            break;
        }
        // Merge j into i: combine sum-vectors, then recompute i's edges to the
        // union of both endpoints' neighbours (the only nodes that can still be
        // ≥theta), dropping any that fell below the cut.
        s_vec[i] = sparse_add(&s_vec[i], &s_vec[j]);
        // The core anchor grows only by absorbing another core's members — drift
        // singletons (empty `core_b`, `core_cnt` 0) leave the stable anchor intact.
        core_b[i] = sparse_add(&core_b[i], &core_b[j]);
        core_cnt[i] += core_cnt[j];
        size[i] += size[j];
        has_core[i] = has_core[i] || has_core[j];
        let taken = std::mem::take(&mut unit_ids[j]);
        unit_ids[i].extend(taken);
        active[j] = false;

        let i_nbrs: Vec<usize> = nbr[i].keys().copied().collect();
        let j_nbrs: Vec<usize> = nbr[j].keys().copied().collect();
        for &k in &i_nbrs {
            nbr[k].remove(&i);
        }
        for &k in &j_nbrs {
            nbr[k].remove(&j);
        }
        nbr[i].clear();
        nbr[j].clear();
        let mut cand = i_nbrs;
        cand.extend(j_nbrs);
        cand.sort_unstable();
        cand.dedup();
        for k in cand {
            if k == i || k == j || !active[k] {
                continue;
            }
            if role_blocks(&role, i, k) {
                if has_core[i] || has_core[k] {
                    role_rejected += 1;
                }
                continue;
            }
            let s = dot(&s_vec[i], &s_vec[k]) / (size[i] as f32 * size[k] as f32);
            // Both linkages must clear their cut: A drives candidacy, B gates it.
            if s >= theta_family
                && coherence(&full_b, &core_b, &core_cnt, &has_core, i, k) >= theta_b_family
            {
                nbr[i].insert(k, s);
                nbr[k].insert(i, s);
            }
        }
        merges += 1;
    }

    // ── read out families: core-anchored, ≥2 units assembled ──
    let mut families: Vec<Family> = (0..u0)
        .filter(|&i| active[i] && has_core[i] && unit_ids[i].len() >= 2)
        .map(|i| build_family(symbols, &units, &unit_ids[i], b_vecs))
        .collect();
    // Largest, most cross-file first — the strongest reinvention signal,
    // matching the repeated query's ordering.
    families.sort_by_key(|f| std::cmp::Reverse((f.members.len(), f.n_files)));

    let funnel = FamilyFunnel {
        core_clusters,
        singletons,
        merges,
        guard_rejected,
        role_rejected,
        families: families.len(),
    };
    FamilyResult { families, funnel }
}

/// Channel-B cosine between two symbols' L2-normalized tf-idf vectors (both
/// sorted by term id, as `vocab_index` emits them): one merge-join dot. The
/// vectors arrive normalized, so the dot IS the cosine. Deterministic — the
/// accumulation walks term ids in order, no map iteration.
fn b_cos(a: &[(u32, f32)], b: &[(u32, f32)]) -> f32 {
    let (mut i, mut j, mut acc) = (0usize, 0usize, 0.0f32);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                acc += a[i].1 * b[j].1;
                i += 1;
                j += 1;
            }
        }
    }
    acc
}

fn build_family(
    symbols: &[SymbolPrint],
    units: &[Unit],
    unit_ids: &[usize],
    b_vecs: &[Vec<(u32, f32)>],
) -> Family {
    // Family core = largest core sub-cluster (ties → tightest, then lowest
    // medoid id): the tightest, best-attested shape in the family.
    let core_unit = unit_ids
        .iter()
        .filter(|&&i| units[i].core_size > 0)
        .copied()
        .max_by(|&a, &b| {
            let (ua, ub) = (&units[a], &units[b]);
            ua.core_size
                .cmp(&ub.core_size)
                .then_with(|| {
                    tightness(symbols, &ua.members).total_cmp(&tightness(symbols, &ub.members))
                })
                .then_with(|| medoid(symbols, &ub.members).cmp(&medoid(symbols, &ua.members)))
        })
        .expect("family has ≥1 core unit by construction");
    let core_medoid = medoid(symbols, &units[core_unit].members);
    let core_set: HashSet<u32> = units[core_unit].members.iter().copied().collect();

    let mut members: Vec<FamilyMember> = Vec::new();
    for &ui in unit_ids {
        for &s in &units[ui].members {
            members.push(FamilyMember {
                cos_to_core: cosine(&symbols[s as usize].wl, &symbols[core_medoid as usize].wl),
                b_cos_to_core: b_cos(&b_vecs[s as usize], &b_vecs[core_medoid as usize]),
                is_core: core_set.contains(&s),
                sym: s,
            });
        }
    }

    // Drift order: core members first, then drifted; within each, nearest to
    // the core first (descending cosine = ascending distance). Ties → symbol id.
    members.sort_by(|a, b| {
        (b.is_core, b.cos_to_core, a.sym)
            .partial_cmp(&(a.is_core, a.cos_to_core, b.sym))
            .unwrap()
    });

    let files: HashSet<&str> = members
        .iter()
        .map(|m| symbols[m.sym as usize].sym.file.as_str())
        .collect();
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for m in &members {
        if m.sym != core_medoid {
            lo = lo.min(m.cos_to_core);
            hi = hi.max(m.cos_to_core);
        }
    }

    Family {
        core_medoid,
        core_size: core_set.len(),
        sub_clusters: unit_ids.len(),
        n_files: files.len(),
        cos_to_core_min: lo,
        cos_to_core_max: hi,
        all_test: members.iter().all(|m| symbols[m.sym as usize].is_test),
        members,
    }
}
