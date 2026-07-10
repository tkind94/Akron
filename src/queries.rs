//! Phase-0 queries over the embedding (DESIGN.md §3): repeated shapes
//! (high A) and competing patterns (high B, low A). Phase-1 adds the
//! deprecated-candidate query (Channel C, below).

use crate::callrel::CallGraph;
use crate::cluster::{ShapeClusters, UnionFind, VocabIndex, REP_RELAX};
use crate::fingerprint::cosine;
use crate::history::{self, Activity, ClusterDates};
use crate::types::SymbolPrint;
use std::collections::{HashMap, HashSet};

pub struct RepeatedCluster {
    pub members: Vec<u32>,
    pub n_files: usize,
    pub total_nodes: u32,
    pub all_test: bool,
}

pub fn repeated(symbols: &[SymbolPrint], shapes: &mut ShapeClusters) -> Vec<RepeatedCluster> {
    let clusters = shapes.uf.clusters(symbols.len());
    let mut out: Vec<RepeatedCluster> = clusters
        .into_values()
        .map(|mut members| {
            members.sort_by_key(|&i| {
                (
                    symbols[i as usize].sym.file.clone(),
                    symbols[i as usize].sym.line,
                )
            });
            let files: HashSet<&str> = members
                .iter()
                .map(|&i| symbols[i as usize].sym.file.as_str())
                .collect();
            RepeatedCluster {
                n_files: files.len(),
                total_nodes: members
                    .iter()
                    .map(|&i| symbols[i as usize].node_count)
                    .sum(),
                all_test: members.iter().all(|&i| symbols[i as usize].is_test),
                members,
            }
        })
        .collect();
    // Cross-file, non-test, big first: that ordering is the reinvention signal.
    // Ties (equal on all three) fall back to the first member's file:line —
    // content-derived and so run-invariant, unlike the insertion order from
    // `clusters.into_values()` (a HashMap), which is not.
    out.sort_by(|a, b| {
        let ka = (b.n_files > 1, !b.all_test, b.total_nodes);
        let kb = (a.n_files > 1, !a.all_test, a.total_nodes);
        ka.cmp(&kb).then_with(|| {
            let (sa, sb) = (
                &symbols[a.members[0] as usize].sym,
                &symbols[b.members[0] as usize].sym,
            );
            (&sa.file, sa.line).cmp(&(&sb.file, sb.line))
        })
    });
    out
}

pub struct CompetingGroup {
    pub members: Vec<u32>,
    pub b_max: f32,
    pub a_at_best: f32,
    pub shared_terms: Vec<String>,
}

/// Where candidate pairs died — so an empty result is explainable, never a
/// silent zero (DESIGN.md: no silent caps).
pub struct CompetingFunnel {
    pub vocab_pairs: usize,
    pub cross_context: usize,
    pub low_shape: usize,
    /// Shape-divergent pairs dropped because they are call-related
    /// (caller/callee, not competitors — see `callrel.rs`).
    pub call_related: usize,
    /// Shape-divergent pairs surviving the shared-vocab quality gate: their
    /// overlap has real mass, not just a couple of generic or rare-but-thin
    /// terms (see `shared_vocab_quality` below).
    pub vocab_quality: usize,
    /// Qualifying pairs whose union was refused because the two components'
    /// representatives didn't themselves clear the (relaxed) B threshold —
    /// the anti-chaining guard below. Counts blocked *merges*, not dropped
    /// pairs: both symbols stay in the result, just not forced into one group.
    pub chained: usize,
}

pub struct CompetingResult {
    pub groups: Vec<CompetingGroup>,
    pub funnel: CompetingFunnel,
}

const MIN_VOCAB_TERMS: usize = 5;

/// A pair's shared vocabulary must have real mass, not just a couple of
/// terms riding a high idf. Below this many shared terms, B is being carried
/// by too little overlap to trust (scrapy C9's `referrer` spike, httpx C8's
/// `auth,tuple` — both 1-2 "real" terms once low-signal ones are excluded).
const MIN_SHARED_TERMS: usize = 3;

/// The shared terms must also account for a real fraction of at least one
/// side's vocabulary "mass" (Σ tf-idf weight), not just a sliver riding on
/// top of two otherwise-unrelated, large vocabularies. Tuned on the graded
/// external-corpus set (R&D archive validation/2026-07-03-external-corpora.md): measured
/// across all 19 required-survivor TPs, the tightest is httpx C3
/// (ParseResult/URL host-rendering, ~0.40); this sits with margin below
/// that. Note this does *not* separate every targeted FP — scrapy C9's
/// `referrer` spike measures ~0.53, *above* several required TPs (httpx C3
/// ~0.40, flask C8 ~0.47, httpx C9 ~0.52) — so no fraction threshold can
/// drop it without also dropping those; that kill is left to the
/// low-signal-term rule below on the corpora where it applies, and accepted
/// as a residual miss elsewhere rather than risking a real TP (see the
/// determination in the commit message / final report).
const MIN_SHARED_MASS_FRACTION: f32 = 0.30;

/// Shared terms that carry a pair by construction rather than by genuine
/// topical overlap: dunder/comparison boilerplate, varargs plumbing, bare
/// builtins, and typing-annotation noise — all common enough to say nothing
/// about what the code actually does. A pair whose *entire* shared-term set
/// is drawn from this list is excluded regardless of mass (flask C4's
/// `args,kwargs,any` sits at B=0.84 on varargs vocabulary alone; httpx C2's
/// `__eq__` family and C6's `__repr__` family are carried entirely by the
/// `other`/`class`/`name` idioms every dunder of that kind writes).
const LOW_SIGNAL_TERMS: &[&str] = &[
    "eq",
    "repr",
    "str",
    "hash",
    "other",
    "class",
    "name", // dunder-ish
    "args",
    "kwargs",
    "any", // varargs
    "dict",
    "list",
    "int",
    "key",
    "value",
    "bool", // bare builtins
    "isinstance",
    "typing", // typing-annotation noise
];

/// Whether a pair's shared vocabulary has enough real, non-generic mass to
/// count as topical overlap rather than a coincidental or boilerplate match.
fn shared_vocab_quality(vocab: &VocabIndex, a: u32, b: u32) -> bool {
    let shared = vocab.shared_term_weights(a, b);
    if shared.len() < MIN_SHARED_TERMS {
        return false;
    }
    if shared
        .iter()
        .all(|&(id, _)| LOW_SIGNAL_TERMS.contains(&vocab.term_name(id)))
    {
        return false;
    }
    let shared_mass: f32 = shared.iter().map(|&(_, w)| w).sum();
    let smaller_mass = vocab.weight_mass(a).min(vocab.weight_mass(b));
    smaller_mass > 0.0 && shared_mass / smaller_mass >= MIN_SHARED_MASS_FRACTION
}

pub fn competing(
    symbols: &[SymbolPrint],
    vocab: &VocabIndex,
    theta_b: f32,
    theta_a_low: f32,
    calls: &CallGraph,
) -> CompetingResult {
    let mut kept: Vec<(u32, u32, f32, f32)> = Vec::new();
    let pairs = vocab.similar_pairs(theta_b);
    let mut funnel = CompetingFunnel {
        vocab_pairs: pairs.len(),
        cross_context: 0,
        low_shape: 0,
        vocab_quality: 0,
        call_related: 0,
        chained: 0,
    };

    for (a, b, cb) in pairs {
        let (sa, sb) = (&symbols[a as usize], &symbols[b as usize]);
        // Tests legitimately share vocabulary with what they test; same-file
        // vocabulary overlap is usually cohesion, not competition.
        if sa.is_test || sb.is_test || sa.sym.file == sb.sym.file {
            continue;
        }
        if sa.vocab_tf.len() < MIN_VOCAB_TERMS || sb.vocab_tf.len() < MIN_VOCAB_TERMS {
            continue;
        }
        funnel.cross_context += 1;
        let ca = cosine(&sa.wl, &sb.wl);
        if ca > theta_a_low {
            continue;
        }
        funnel.low_shape += 1;
        // A high B can be carried by a couple of generic or rare-but-thin
        // terms rather than genuine topical overlap; require real mass.
        if !shared_vocab_quality(vocab, a, b) {
            continue;
        }
        funnel.vocab_quality += 1;
        // A wrapper shares vocabulary with what it wraps by construction;
        // that is a caller/callee relationship, not competition.
        if calls.related(a, b) {
            funnel.call_related += 1;
            continue;
        }
        kept.push((a, b, cb, ca));
    }

    // Best-first union with a representative check, mirroring cluster.rs's
    // anti-chaining guard on Channel A: plain single-link union-find lets a
    // long chain of pairwise-qualifying edges glue dozens of only vaguely
    // related symbols into one "competing" blob (scrapy's 26-member
    // from_crawler group; httpx's 7-member str/bytes blob) even though most
    // members aren't pairwise similar. Processing qualifying pairs
    // best-first (highest B first) and requiring each pair's *component
    // representatives* — not just the pair itself — to also clear a relaxed
    // B threshold before merging stops that chaining while still letting a
    // genuinely similar cluster (flask's three registration mechanisms)
    // assemble around its core.
    let mut sorted = kept.clone();
    sorted.sort_by(|x, y| y.2.total_cmp(&x.2).then((x.0, x.1).cmp(&(y.0, y.1))));

    let mut uf = UnionFind::new(symbols.len());
    let mut rep: Vec<u32> = (0..symbols.len() as u32).collect();
    let mut size: Vec<u32> = vec![1; symbols.len()];
    for &(a, b, ..) in &sorted {
        let (ra, rb) = (uf.find(a), uf.find(b));
        if ra == rb {
            continue;
        }
        let (pa, pb) = (rep[ra as usize], rep[rb as usize]);
        if vocab.cosine_between(pa, pb) < theta_b * REP_RELAX {
            funnel.chained += 1;
            continue;
        }
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

    let clusters = uf.clusters(symbols.len());
    let mut out: Vec<CompetingGroup> = clusters
        .into_values()
        .filter_map(|mut members| {
            members.sort_by_key(|&i| {
                (
                    symbols[i as usize].sym.file.clone(),
                    symbols[i as usize].sym.line,
                )
            });
            let in_group: HashSet<u32> = members.iter().copied().collect();
            // Tie-break on (a,b): `kept`'s insertion order tracks
            // `similar_pairs`'s output, itself ordered by a HashMap's
            // (per-process-random) iteration, so an unbroken tie in cb would
            // let max_by's "last wins" pick a different pair on every run.
            let best = kept
                .iter()
                .filter(|(a, b, ..)| in_group.contains(a) && in_group.contains(b))
                .max_by(|x, y| x.2.total_cmp(&y.2).then((x.0, x.1).cmp(&(y.0, y.1))))?;
            Some(CompetingGroup {
                members,
                b_max: best.2,
                a_at_best: best.3,
                shared_terms: vocab.shared_terms(best.0, best.1, 8),
            })
        })
        .collect();
    // Ties broken by the first member's file:line (content-derived, stable
    // across runs) rather than left to `clusters.into_values()`'s HashMap order.
    out.sort_by(|a, b| {
        b.b_max.total_cmp(&a.b_max).then_with(|| {
            let (sa, sb) = (
                &symbols[a.members[0] as usize].sym,
                &symbols[b.members[0] as usize].sym,
            );
            (&sa.file, sa.line).cmp(&(&sb.file, sb.line))
        })
    });
    CompetingResult {
        groups: out,
        funnel,
    }
}

// ── Deprecated candidates (DESIGN.md §3, Channel C) ──
//
// Two shape clusters are role-equivalent when their vocabulary centroids agree
// (Channel B ≥ theta_b); being separate shape clusters, they are already
// structurally distinct. When one such cluster is dead and its role-twin is
// growing, the growing one has displaced the dead one in practice —
// "deprecation is a measurement, not a status field" (DESIGN.md §1).
//
// This query runs at the *tight-cluster* altitude, not the family altitude
// (family.rs): a family already spans shapes and mixes activity (corpus-P's ingest
// family's core is growing while some drifted members are older), so a family's
// "role" and single adoption curve are ill-defined here. Re-expressing the
// dead-vs-growing test over families is a semantic change to the query, not a
// small edit, so it is deferred as a future refinement once family-level
// dating can measure a family's internal drift curve directly.

pub struct DeprecatedCandidate {
    pub dead: Vec<u32>,
    pub growing: Vec<u32>,
    pub vocab_cosine: f32,
    pub shared_terms: Vec<String>,
    pub dead_dates: ClusterDates,
    pub growing_dates: ClusterDates,
}

/// Where candidate pairs died, so an empty result is explainable rather than a
/// silent zero (mirrors `CompetingFunnel`).
#[derive(Default)]
pub struct DeprecatedFunnel {
    pub dated_clusters: usize,
    pub dead_clusters: usize,
    pub growing_clusters: usize,
    pub role_pairs: usize,
    pub vocab_matched: usize,
}

pub struct DeprecatedResult {
    pub candidates: Vec<DeprecatedCandidate>,
    pub funnel: DeprecatedFunnel,
}

/// A shape cluster reduced to what the deprecated query needs: its members, a
/// normalized vocabulary centroid, and its dated activity class.
struct DatedCluster {
    members: Vec<u32>,
    centroid: Vec<(u32, f32)>, // sorted by term id, L2-normalized
    dates: ClusterDates,
}

pub fn deprecated_candidates(
    symbols: &[SymbolPrint],
    repeated: &[RepeatedCluster],
    vocab: &VocabIndex,
    anchor: i64,
    theta_b: f32,
) -> DeprecatedResult {
    // Date and vocabulary-centroid every repeated cluster that has history.
    // All-test clusters are skipped: a dead vs growing *test* family is not a
    // deprecation signal (tests legitimately mirror what they cover).
    let dated: Vec<DatedCluster> = repeated
        .iter()
        .filter(|c| !c.all_test)
        .filter_map(|c| {
            let dates = history::cluster_dating(symbols, &c.members, anchor)?;
            Some(DatedCluster {
                centroid: centroid(vocab, &c.members),
                members: c.members.clone(),
                dates,
            })
        })
        .collect();

    let dead: Vec<&DatedCluster> = dated
        .iter()
        .filter(|c| c.dates.activity == Activity::Dead)
        .collect();
    let growing: Vec<&DatedCluster> = dated
        .iter()
        .filter(|c| c.dates.activity == Activity::Growing)
        .collect();

    let mut funnel = DeprecatedFunnel {
        dated_clusters: dated.len(),
        dead_clusters: dead.len(),
        growing_clusters: growing.len(),
        role_pairs: dead.len() * growing.len(),
        vocab_matched: 0,
    };

    let mut candidates = Vec::new();
    for d in &dead {
        for g in &growing {
            let c = centroid_cosine(&d.centroid, &g.centroid);
            if c >= theta_b {
                funnel.vocab_matched += 1;
                candidates.push(DeprecatedCandidate {
                    dead: d.members.clone(),
                    growing: g.members.clone(),
                    vocab_cosine: c,
                    shared_terms: centroid_shared_terms(&d.centroid, &g.centroid, &vocab.terms, 8),
                    dead_dates: clone_dates(&d.dates),
                    growing_dates: clone_dates(&g.dates),
                });
            }
        }
    }
    // Insertion order already tracks `dated` (deterministic post `repeated`'s
    // own fix), but tie-break explicitly on the pair's first members rather
    // than lean on that as an unstated invariant.
    candidates.sort_by(|a, b| {
        b.vocab_cosine
            .total_cmp(&a.vocab_cosine)
            .then((a.dead[0], a.growing[0]).cmp(&(b.dead[0], b.growing[0])))
    });
    DeprecatedResult { candidates, funnel }
}

fn clone_dates(d: &ClusterDates) -> ClusterDates {
    ClusterDates {
        first_seen: d.first_seen,
        last_touched: d.last_touched,
        activity: d.activity,
    }
}

/// Mean of the members' tf-idf vectors, L2-normalized: the cluster's Channel B
/// signature. Idiosyncratic per-member terms average down; the shared role
/// vocabulary reinforces.
fn centroid(vocab: &VocabIndex, members: &[u32]) -> Vec<(u32, f32)> {
    let mut acc: HashMap<u32, f32> = HashMap::new();
    for &m in members {
        for &(id, w) in &vocab.vecs[m as usize] {
            *acc.entry(id).or_default() += w;
        }
    }
    let n = members.len() as f32;
    let mut v: Vec<(u32, f32)> = acc.into_iter().map(|(id, w)| (id, w / n)).collect();
    // `acc`'s HashMap iteration order is randomized per process, but term ids
    // (cluster.rs `vocab_index`) are assigned by sorted term string, so
    // sorting by id here *is* sorting by a run-invariant key — the norm sum
    // below is reproducible across runs.
    v.sort_unstable_by_key(|&(id, _)| id);
    let norm: f32 = v.iter().map(|&(_, w)| w * w).sum::<f32>().sqrt();
    if norm > 0.0 {
        for e in v.iter_mut() {
            e.1 /= norm;
        }
    }
    v
}

fn centroid_cosine(a: &[(u32, f32)], b: &[(u32, f32)]) -> f32 {
    let (mut i, mut j, mut dot) = (0, 0, 0.0f32);
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                dot += a[i].1 * b[j].1;
                i += 1;
                j += 1;
            }
        }
    }
    dot // both inputs are already L2-normalized
}

fn centroid_shared_terms(
    a: &[(u32, f32)],
    b: &[(u32, f32)],
    terms: &[String],
    k: usize,
) -> Vec<String> {
    let (mut i, mut j) = (0, 0);
    let mut shared: Vec<(f32, u32)> = Vec::new();
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared.push((a[i].1.min(b[j].1), a[i].0));
                i += 1;
                j += 1;
            }
        }
    }
    // Tie-break on term id: deterministic given `a`/`b` are already in
    // canonical (id-sorted) order, spelled out rather than left implicit.
    shared.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
    shared
        .iter()
        .take(k)
        .map(|&(_, id)| terms[id as usize].clone())
        .collect()
}
