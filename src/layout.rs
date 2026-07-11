//! The Map view's 2-D layout (TKI-47): a deterministic force-directed
//! embedding of the kNN graph over the full-dimension semantic vectors.
//!
//! Why not PCA for the default view: the TKI-46 bake-off measured 2-D PCA
//! collapsing small symbol groups on every model (kNN@10 same-group
//! agreement 0.78 → 0.57 after projection) while full-dimension structure
//! stays real (0.72–0.96). So neighborhoods are computed in full dimension
//! (k nearest by cosine) and only *drawn* in 2-D: edges attract, everything
//! repels, islands emerge where the embedding has real neighborhoods.
//!
//! Determinism (same contract as `pca.rs`): kNN per row is an independent
//! top-k with ties broken on index (rayon changes nothing — each row's
//! result is a pure function of the inputs); the force pass is sequential
//! in ascending node order with all "randomness" from a fixed LCG; fixed
//! iteration count, no clock, no OS RNG. Same machine + same embeddings ⇒
//! byte-identical coordinates.

use rayon::prelude::*;

pub const KNN_K: usize = 8;
const ITERS: usize = 400;
/// Sampled-repulsion pairs per node per iteration. An unbiased estimate of
/// the full O(n²) repulsion sum (scaled by n/SAMPLES), which keeps the pass
/// O(n·(k+S)) — the classic negative-sampling trade.
const SAMPLES: usize = 16;
/// Force model (LinLog-leaning, the combination that renders tight
/// neighborhoods with visible gaps rather than one hairball):
/// - linear springs along kNN edges (attraction ∝ distance),
/// - 1/d repulsion between (sampled) pairs, weight `REPULSION`,
/// - gravity ∝ distance-to-centroid, weight `GRAVITY`, which bounds
///   disconnected components instead of letting them fly apart.
/// Equilibria: within a connected clump of m nodes, spacing settles near
/// √(m·REPULSION/k); the whole cloud's radius settles near
/// √(n·REPULSION/GRAVITY).
const REPULSION: f64 = 1.5e-5;
const GRAVITY: f64 = 0.05;
const TEMP0: f64 = 0.06;

/// `k` nearest neighbors of every row by cosine (inputs are L2-normalized,
/// so plain dot), self excluded, ties broken on lower index.
///
/// TKI-59: the dots are computed 8 candidate rows at a time (`dot8`) and
/// the top-k comes from an exact select-then-sort instead of a full sort.
/// Both are bit-identical to the naive form: each dot keeps its own
/// accumulator in the same ascending-element order, and the (score desc,
/// index asc) comparator is a strict total order, so partition + sort of
/// the top `k` yields exactly the prefix the full sort would.
pub fn knn(embeddings: &[Vec<f32>], k: usize) -> Vec<Vec<u32>> {
    let n = embeddings.len();
    let keep = k.min(n.saturating_sub(1));
    (0..n)
        .into_par_iter()
        .map(|i| {
            if keep == 0 {
                return Vec::new();
            }
            let a = &embeddings[i];
            let mut scored: Vec<(f32, u32)> = Vec::with_capacity(n - 1);
            let mut j = 0;
            while j + 8 <= n {
                let s = dot8(
                    a,
                    [
                        &embeddings[j],
                        &embeddings[j + 1],
                        &embeddings[j + 2],
                        &embeddings[j + 3],
                        &embeddings[j + 4],
                        &embeddings[j + 5],
                        &embeddings[j + 6],
                        &embeddings[j + 7],
                    ],
                );
                for (t, &sv) in s.iter().enumerate() {
                    if j + t != i {
                        scored.push((sv, (j + t) as u32));
                    }
                }
                j += 8;
            }
            while j < n {
                if j != i {
                    scored.push((dot(a, &embeddings[j]), j as u32));
                }
                j += 1;
            }
            let cmp = |a: &(f32, u32), b: &(f32, u32)| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1));
            if scored.len() > keep {
                scored.select_nth_unstable_by(keep - 1, cmp);
                scored.truncate(keep);
            }
            scored.sort_unstable_by(cmp);
            scored.into_iter().map(|(_, j)| j).collect()
        })
        .collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Dots of `a` against 8 rows at once — 8 independent accumulator chains
/// hide the scalar f32 add latency the single-chain `dot` is bound by.
/// Each lane's accumulation order is exactly `dot`'s (ascending elements,
/// one accumulator), so every result is bit-identical to `dot(a, b[l])`.
fn dot8(a: &[f32], b: [&[f32]; 8]) -> [f32; 8] {
    let d = a.len();
    let (b0, b1, b2, b3) = (&b[0][..d], &b[1][..d], &b[2][..d], &b[3][..d]);
    let (b4, b5, b6, b7) = (&b[4][..d], &b[5][..d], &b[6][..d], &b[7][..d]);
    let mut s = [0.0f32; 8];
    for t in 0..d {
        let x = a[t];
        s[0] += x * b0[t];
        s[1] += x * b1[t];
        s[2] += x * b2[t];
        s[3] += x * b3[t];
        s[4] += x * b4[t];
        s[5] += x * b5[t];
        s[6] += x * b6[t];
        s[7] += x * b7[t];
    }
    s
}

/// The edge set the layout springs act on: *mutual* kNN pairs (each lists
/// the other) weighted by cosine² — one-sided edges are mostly bridges from
/// a symbol to a merely-least-distant stranger, and keeping them drags real
/// neighborhoods into one hairball. A node with no mutual neighbor keeps
/// its single top-1 edge so nothing floats unanchored. Each node's list is
/// sorted ascending by neighbor id. Public because `explore` ships these
/// ids per symbol — the page draws a selection's edges to show why it sits
/// where it sits.
pub fn adjacency(embeddings: &[Vec<f32>], neighbors: &[Vec<u32>]) -> Vec<Vec<(u32, f64)>> {
    let n = neighbors.len();
    let w_of = |i: usize, j: usize| {
        let c = dot(&embeddings[i], &embeddings[j]).max(0.0) as f64;
        c * c
    };
    let mut adj: Vec<Vec<(u32, f64)>> = vec![Vec::new(); n];
    for (i, ns) in neighbors.iter().enumerate() {
        for &j in ns {
            let j = j as usize;
            if j > i && neighbors[j].contains(&(i as u32)) {
                let w = w_of(i, j);
                adj[i].push((j as u32, w));
                adj[j].push((i as u32, w));
            }
        }
    }
    for (i, ns) in neighbors.iter().enumerate() {
        if adj[i].is_empty() && !ns.is_empty() {
            let j = ns[0] as usize; // top-1 fallback anchor
            let w = w_of(i, j);
            adj[i].push((j as u32, w));
            adj[j].push((i as u32, w));
        }
    }
    for a in &mut adj {
        a.sort_unstable_by(|x, y| x.0.cmp(&y.0));
        a.dedup_by_key(|e| e.0);
    }
    adj
}

/// Force-directed 2-D positions for the kNN graph. `init` seeds the global
/// arrangement (the caller passes PCA-1/2 — a good deterministic start);
/// degenerate or missing init falls back to an LCG scatter. Output is
/// normalized to [0,1]².
pub fn layout(
    embeddings: &[Vec<f32>],
    neighbors: &[Vec<u32>],
    init: &[(f32, f32)],
) -> Vec<(f32, f32)> {
    let n = neighbors.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![(0.5, 0.5)];
    }

    let adj = adjacency(embeddings, neighbors);

    // Init: normalize the seed plane to [0,1]²; if it is degenerate on
    // either coordinate, scatter that coordinate with the LCG instead.
    let mut state = 0x51_7C_C1B7_2722_0A95u64;
    let mut rand01 = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as f64 / (1u64 << 31) as f64
    };
    let mut pos: Vec<(f64, f64)> = Vec::with_capacity(n);
    let (xs, ys): (Vec<f64>, Vec<f64>) = init
        .iter()
        .map(|&(x, y)| (x as f64, y as f64))
        .chain(std::iter::repeat((0.0, 0.0)))
        .take(n)
        .unzip();
    let xr = min_max(&xs);
    let yr = min_max(&ys);
    for i in 0..n {
        let x = match xr {
            Some((lo, hi)) if hi > lo => (xs[i] - lo) / (hi - lo),
            _ => rand01(),
        };
        let y = match yr {
            Some((lo, hi)) if hi > lo => (ys[i] - lo) / (hi - lo),
            _ => rand01(),
        };
        pos.push((x, y));
    }

    // Force pass: sequential updates in ascending index order (Gauss–Seidel
    // style: later nodes see earlier nodes' fresh positions — deterministic
    // because the order is fixed).
    let scale = n as f64 / SAMPLES as f64;
    for t in 0..ITERS {
        let temp = TEMP0 * (1.0 - t as f64 / ITERS as f64) + 1e-4;
        // centroid, ascending order (gravity's anchor this iteration)
        let (mut cx, mut cy) = (0.0f64, 0.0f64);
        for &(x, y) in &pos {
            cx += x;
            cy += y;
        }
        cx /= n as f64;
        cy /= n as f64;
        for i in 0..n {
            let (xi, yi) = pos[i];
            // weighted linear springs along mutual-kNN edges
            let (mut fx, mut fy) = (0.0f64, 0.0f64);
            for &(j, w) in &adj[i] {
                fx += (pos[j as usize].0 - xi) * w;
                fy += (pos[j as usize].1 - yi) * w;
            }
            // sampled 1/d repulsion, scaled to estimate the all-pairs sum
            for _ in 0..SAMPLES {
                let j = (rand01() * n as f64) as usize % n;
                if j == i {
                    continue;
                }
                let (dx, dy) = (xi - pos[j].0, yi - pos[j].1);
                let d2 = (dx * dx + dy * dy).max(1e-6);
                let f = REPULSION * scale / d2; // magnitude R·scale/d
                fx += dx * f;
                fy += dy * f;
            }
            // gravity toward the centroid: bounds disconnected islands
            fx += (cx - xi) * GRAVITY;
            fy += (cy - yi) * GRAVITY;
            let disp = (fx * fx + fy * fy).sqrt();
            if disp > 1e-12 {
                let step = disp.min(temp) / disp;
                pos[i].0 = xi + fx * step;
                pos[i].1 = yi + fy * step;
            }
        }
    }

    // Normalize to [0,1]².
    let fxs: Vec<f64> = pos.iter().map(|p| p.0).collect();
    let fys: Vec<f64> = pos.iter().map(|p| p.1).collect();
    let (xlo, xhi) = min_max(&fxs).unwrap();
    let (ylo, yhi) = min_max(&fys).unwrap();
    pos.iter()
        .map(|&(x, y)| {
            (
                if xhi > xlo { ((x - xlo) / (xhi - xlo)) as f32 } else { 0.5 },
                if yhi > ylo { ((y - ylo) / (yhi - ylo)) as f32 } else { 0.5 },
            )
        })
        .collect()
}

/// Overlap-relaxation sweeps (fixed count — the pass is O(sweeps·n) via a
/// uniform grid, and stacks of ~20 exact clones spread into a disc well
/// inside this budget; it also exits early once a sweep moves nothing).
const RELAX_SWEEPS: usize = 48;

/// Final overlap pass: points that stack at rest are pushed apart until
/// pairwise distance ≥ the sum of their radii, where trivially resolvable.
/// `pos` and `radii` share the normalized [0,1] plane (the caller converts
/// pixel radii at its reference scale). Deterministic: sweeps in ascending
/// index order over a uniform grid (Gauss–Seidel — later pairs see fresh
/// positions), coincident pairs split along an index-hashed angle, fixed
/// sweep cap. Every push is clamped to [0,1]² inside the sweep, so stacks
/// against the plane's edge spread along it instead of resolving out of
/// bounds and snapping back broken at the end.
pub fn relax(pos: &mut [(f32, f32)], radii: &[f32]) {
    let n = pos.len();
    if n < 2 {
        return;
    }
    let mut p: Vec<(f64, f64)> = pos.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
    let r: Vec<f64> = radii.iter().map(|&x| x as f64).collect();
    let rmax = r.iter().cloned().fold(0.0f64, f64::max);
    if rmax <= 0.0 {
        return;
    }
    let cell = 2.0 * rmax; // any overlapping pair is within one cell ring
    let key = |x: f64, y: f64| ((x / cell).floor() as i64, (y / cell).floor() as i64);
    for _ in 0..RELAX_SWEEPS {
        let mut grid: std::collections::HashMap<(i64, i64), Vec<u32>> =
            std::collections::HashMap::new();
        for (i, &(x, y)) in p.iter().enumerate() {
            grid.entry(key(x, y)).or_default().push(i as u32);
        }
        let mut moved = false;
        for i in 0..n {
            let (gx, gy) = key(p[i].0, p[i].1);
            let mut cands: Vec<u32> = Vec::new();
            for dx in -1..=1i64 {
                for dy in -1..=1i64 {
                    if let Some(v) = grid.get(&(gx + dx, gy + dy)) {
                        cands.extend(v.iter().copied().filter(|&j| (j as usize) > i));
                    }
                }
            }
            cands.sort_unstable(); // grid cell order must not matter
            for j in cands {
                let j = j as usize;
                let (dx, dy) = (p[j].0 - p[i].0, p[j].1 - p[i].1);
                let d2 = dx * dx + dy * dy;
                let target = r[i] + r[j];
                if d2 >= target * target {
                    continue;
                }
                let d = d2.sqrt();
                let (ux, uy) = if d > 1e-9 {
                    (dx / d, dy / d)
                } else {
                    // coincident (exact clones share PCA seeds and forces):
                    // split along a fixed per-pair angle
                    let h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        ^ (j as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                    let a = (h >> 11) as f64 / (1u64 << 53) as f64 * std::f64::consts::TAU;
                    (a.cos(), a.sin())
                };
                let push = (target - d) / 2.0;
                p[i].0 = (p[i].0 - ux * push).clamp(0.0, 1.0);
                p[i].1 = (p[i].1 - uy * push).clamp(0.0, 1.0);
                p[j].0 = (p[j].0 + ux * push).clamp(0.0, 1.0);
                p[j].1 = (p[j].1 + uy * push).clamp(0.0, 1.0);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    for (out, &(x, y)) in pos.iter_mut().zip(&p) {
        *out = (x as f32, y as f32);
    }
}

/// A directory needs at least this many points before a centroid means
/// anything worth printing.
pub const LABEL_MIN_POINTS: usize = 4;
/// Dispersion gate: median distance to the directory's medoid, as a
/// fraction of the layout extent (the plane is normalized [0,1]). Above
/// this the directory is smeared across the map and gets NO label — a
/// centroid label there would point at nothing.
pub const LABEL_MAX_MEDIAN: f32 = 0.12;

/// A map label: the directory name (exactly the legend chip's text) at its
/// island's centroid, the point count for draw priority, and `r` — the
/// gating median-to-medoid distance, which doubles as the island's core
/// radius so the page can place the text clear of the points.
pub struct DirLabel {
    pub dir: String,
    pub x: f32,
    pub y: f32,
    pub count: usize,
    pub r: f32,
}

/// Labels for directories whose points actually cohere on the layout
/// plane. Gate: ≥ `LABEL_MIN_POINTS` points AND median distance to the
/// dir's medoid (the member minimizing summed distance, ties → lowest
/// index; upper median on even counts) ≤ `LABEL_MAX_MEDIAN`. Position: the
/// members' centroid. Output sorted by count desc, then dir asc — the draw
/// order that keeps big islands labeled when labels collide. Deterministic:
/// BTreeMap grouping, fixed tie-breaks, no float reordering.
pub fn dir_labels(points: &[(f32, f32)], dirs: &[String]) -> Vec<DirLabel> {
    let dist = |a: (f32, f32), b: (f32, f32)| {
        let (dx, dy) = ((a.0 - b.0) as f64, (a.1 - b.1) as f64);
        (dx * dx + dy * dy).sqrt()
    };
    let mut groups: std::collections::BTreeMap<&str, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, d) in dirs.iter().enumerate() {
        groups.entry(d).or_default().push(i);
    }
    let mut out: Vec<DirLabel> = Vec::new();
    for (dir, ids) in groups {
        if ids.len() < LABEL_MIN_POINTS {
            continue;
        }
        let medoid = ids
            .iter()
            .map(|&a| ids.iter().map(|&b| dist(points[a], points[b])).sum::<f64>())
            .enumerate()
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(slot, _)| ids[slot])
            .expect("non-empty group");
        let mut dists: Vec<f64> = ids.iter().map(|&b| dist(points[medoid], points[b])).collect();
        dists.sort_by(f64::total_cmp);
        let median = dists[dists.len() / 2];
        if median > LABEL_MAX_MEDIAN as f64 {
            continue;
        }
        let m = ids.len() as f64;
        let (cx, cy) = ids.iter().fold((0.0f64, 0.0f64), |acc, &b| {
            (acc.0 + points[b].0 as f64 / m, acc.1 + points[b].1 as f64 / m)
        });
        out.push(DirLabel {
            dir: dir.to_string(),
            x: cx as f32,
            y: cy as f32,
            count: ids.len(),
            r: median as f32,
        });
    }
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.dir.cmp(&b.dir)));
    out
}

fn min_max(v: &[f64]) -> Option<(f64, f64)> {
    if v.is_empty() {
        return None;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &x in v {
        if x < lo {
            lo = x;
        }
        if x > hi {
            hi = x;
        }
    }
    Some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two planted groups of mutually-similar unit vectors, far apart.
    fn two_cluster_embeddings() -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        for g in 0..2 {
            for i in 0..10 {
                // base direction per group, small deterministic wiggle
                let mut v = vec![0.0f32; 8];
                v[g * 4] = 1.0;
                v[g * 4 + 1] = 0.05 * (i as f32);
                let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                out.push(v.into_iter().map(|x| x / n).collect());
            }
        }
        out
    }

    #[test]
    fn knn_finds_the_planted_group() {
        let e = two_cluster_embeddings();
        let g = knn(&e, 4);
        assert_eq!(g.len(), 20);
        for (i, ns) in g.iter().enumerate() {
            assert_eq!(ns.len(), 4);
            for &j in ns {
                assert_eq!(
                    (j as usize) / 10,
                    i / 10,
                    "node {i}'s neighbors stay in its own group"
                );
            }
        }
    }

    #[test]
    fn knn_excludes_self_and_caps_at_n_minus_1() {
        let e = two_cluster_embeddings()[..3].to_vec();
        let g = knn(&e, 8);
        for (i, ns) in g.iter().enumerate() {
            assert_eq!(ns.len(), 2, "k caps at n-1");
            assert!(!ns.contains(&(i as u32)), "self excluded");
        }
    }

    #[test]
    fn layout_is_bit_identical_across_runs() {
        let e = two_cluster_embeddings();
        let g = knn(&e, 4);
        let init: Vec<(f32, f32)> = (0..20).map(|i| (i as f32, (i * 7 % 5) as f32)).collect();
        let a = layout(&e, &g, &init);
        let b = layout(&e, &g, &init);
        for (p, q) in a.iter().zip(&b) {
            assert_eq!(p.0.to_bits(), q.0.to_bits());
            assert_eq!(p.1.to_bits(), q.1.to_bits());
        }
    }

    #[test]
    fn layout_separates_the_two_planted_groups() {
        let e = two_cluster_embeddings();
        let g = knn(&e, 4);
        let init: Vec<(f32, f32)> = (0..20).map(|i| (i as f32 * 0.01, 0.0)).collect();
        let p = layout(&e, &g, &init);
        // mean within-group distance must be well below the between-group
        // distance of the group centroids
        let centroid = |r: std::ops::Range<usize>| {
            let m = r.len() as f32;
            r.map(|i| p[i]).fold((0.0, 0.0), |acc, q| (acc.0 + q.0 / m, acc.1 + q.1 / m))
        };
        let (c0, c1) = (centroid(0..10), centroid(10..20));
        let between = ((c0.0 - c1.0).powi(2) + (c0.1 - c1.1).powi(2)).sqrt();
        let mut within = 0.0f32;
        for i in 0..10 {
            within += ((p[i].0 - c0.0).powi(2) + (p[i].1 - c0.1).powi(2)).sqrt() / 10.0;
            within += ((p[i + 10].0 - c1.0).powi(2) + (p[i + 10].1 - c1.1).powi(2)).sqrt() / 10.0;
        }
        assert!(
            between > within,
            "groups must separate: between {between}, mean within {within}"
        );
    }

    #[test]
    fn layout_output_is_normalized_and_full_width() {
        let e = two_cluster_embeddings();
        let g = knn(&e, 4);
        let p = layout(&e, &g, &[]);
        assert_eq!(p.len(), 20);
        for &(x, y) in &p {
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
        }
    }

    #[test]
    fn empty_and_singleton_do_not_panic() {
        assert!(layout(&[], &[], &[]).is_empty());
        assert_eq!(layout(&[vec![1.0]], &[vec![]], &[]), vec![(0.5, 0.5)]);
    }

    // ── relax ──

    #[test]
    fn relax_separates_a_stack_to_sum_of_radii() {
        // 12 points at the exact same spot, mid-plane: room to resolve.
        let mut pos = vec![(0.5f32, 0.5f32); 12];
        let radii = vec![0.008f32; 12];
        relax(&mut pos, &radii);
        for i in 0..12 {
            for j in (i + 1)..12 {
                let d = ((pos[i].0 - pos[j].0).powi(2) + (pos[i].1 - pos[j].1).powi(2)).sqrt();
                assert!(
                    d >= 0.016 - 1e-4,
                    "pair ({i},{j}) still overlaps after relax: d={d}"
                );
            }
        }
    }

    #[test]
    fn relax_is_deterministic_and_stays_in_bounds() {
        let mk = || {
            let mut pos: Vec<(f32, f32)> = (0..40)
                .map(|i| (((i * 13 % 7) as f32) / 7.0, ((i * 5 % 11) as f32) / 11.0))
                .collect();
            let radii: Vec<f32> = (0..40).map(|i| 0.004 + 0.004 * ((i % 3) as f32)).collect();
            relax(&mut pos, &radii);
            pos
        };
        let (a, b) = (mk(), mk());
        for (p, q) in a.iter().zip(&b) {
            assert_eq!(p.0.to_bits(), q.0.to_bits());
            assert_eq!(p.1.to_bits(), q.1.to_bits());
        }
        for &(x, y) in &a {
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
        }
    }

    #[test]
    fn relax_resolves_edge_and_corner_stacks_in_bounds() {
        // Stacks clamped against the plane's boundary must spread along it
        // (and inward), never resolve out of [0,1]² — the in-sweep clamp is
        // what keeps an edge stack from separating out of bounds and
        // snapping back broken at the end.
        for &(sx, sy) in &[(1.0f32, 0.5f32), (1.0, 1.0), (0.0, 0.0)] {
            let mut pos = vec![(sx, sy); 8];
            let radii = vec![0.008f32; 8];
            relax(&mut pos, &radii);
            for &(x, y) in &pos {
                assert!(
                    (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
                    "stack at ({sx},{sy}) resolved out of bounds: ({x},{y})"
                );
            }
            for i in 0..8 {
                for j in (i + 1)..8 {
                    let d =
                        ((pos[i].0 - pos[j].0).powi(2) + (pos[i].1 - pos[j].1).powi(2)).sqrt();
                    assert!(
                        d >= 0.016 - 1e-4,
                        "stack at ({sx},{sy}): pair ({i},{j}) still overlaps: d={d}"
                    );
                }
            }
        }
    }

    #[test]
    fn relax_leaves_separated_points_alone() {
        let mut pos = vec![(0.1f32, 0.1), (0.9, 0.9)];
        let before = pos.clone();
        relax(&mut pos, &[0.01, 0.01]);
        assert_eq!(pos, before, "no overlap ⇒ no movement");
    }

    // ── dir_labels ──

    #[test]
    fn cohesive_dir_is_labeled_at_its_centroid() {
        let points: Vec<(f32, f32)> = (0..6).map(|i| (0.30 + 0.01 * i as f32, 0.70)).collect();
        let dirs: Vec<String> = vec!["src/core".to_string(); 6];
        let labels = dir_labels(&points, &dirs);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].dir, "src/core");
        assert_eq!(labels[0].count, 6);
        assert!((labels[0].x - 0.325).abs() < 1e-4);
        assert!((labels[0].y - 0.70).abs() < 1e-4);
    }

    #[test]
    fn dispersed_dir_gets_no_label() {
        // 8 points smeared over the whole plane: median medoid distance far
        // above the gate — a centroid label would lie.
        let points: Vec<(f32, f32)> = (0..8)
            .map(|i| (((i * 37 % 8) as f32) / 7.0, ((i * 53 % 8) as f32) / 7.0))
            .collect();
        let dirs: Vec<String> = vec!["utils".to_string(); 8];
        assert!(dir_labels(&points, &dirs).is_empty());
    }

    #[test]
    fn small_dirs_get_no_label_and_order_is_count_desc() {
        let mut points = Vec::new();
        let mut dirs = Vec::new();
        // "big": 8 tight points; "mid": 5 tight points; "tiny": 3 points.
        for i in 0..8 {
            points.push((0.2 + 0.005 * i as f32, 0.2));
            dirs.push("big".to_string());
        }
        for i in 0..5 {
            points.push((0.8 + 0.005 * i as f32, 0.8));
            dirs.push("mid".to_string());
        }
        for i in 0..3 {
            points.push((0.5 + 0.005 * i as f32, 0.5));
            dirs.push("tiny".to_string());
        }
        let labels = dir_labels(&points, &dirs);
        let names: Vec<&str> = labels.iter().map(|l| l.dir.as_str()).collect();
        assert_eq!(names, ["big", "mid"], "tiny (<{LABEL_MIN_POINTS}) unlabeled; count-desc order");
    }
}
