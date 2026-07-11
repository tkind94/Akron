//! Deterministic PCA over the `find` embeddings, for `akron explore`'s map
//! axes (TKI-47). Top-k principal components of the L2-normalized semantic
//! vectors — the only place the learned embedding feeds a visual surface,
//! and it stays inside DESIGN.md §1.2's decoration clause: PCA positions
//! points, it never detects, gates, or labels anything.
//!
//! Why not UMAP/t-SNE: both are stochastic (or "seeded" with
//! platform-varying float reductions), heavy dependencies, and their axes
//! mean nothing afterwards. PCA is linear, tiny, and each axis is a fixed
//! direction in embedding space a user can swap in and out of a 2-D view.
//!
//! Determinism, spelled out:
//! - covariance is accumulated in ascending row order (never a HashMap);
//! - power iteration starts from a fixed LCG-seeded vector per component;
//! - convergence is a fixed tolerance with a fixed iteration cap;
//! - ties and sign are pinned: each component's largest-|loading|
//!   coordinate (ties → lowest index) is forced positive, so a re-run — or
//!   a tiny input perturbation — cannot flip an axis.
//! Same machine + same input ⇒ identical bytes. Cross-machine bit-identity
//! is inherited from f64 arithmetic (no SIMD reduction here, so it holds in
//! practice), but the promise we rely on is per-machine, matching the
//! embedding cache's own scope.

/// Principal components of a row matrix, deflation order (largest variance
/// first). `components[c]` is a unit d-vector; `variances[c]` is the
/// centered data's variance along it; `scores[i][c]` is row i's centered
/// projection onto component c. `scores` is always n × k — components past
/// the data's rank are all-zero rather than absent, so a fixed-width
/// consumer (the map's 8 axis slots) never has to special-case rank.
/// `total_variance` is the centered data's total variance (covariance
/// trace / n) — `variances[c] / total_variance` is component c's explained
/// share, and the shares over all d components sum to 1.
pub struct Pca {
    pub components: Vec<Vec<f64>>,
    pub variances: Vec<f64>,
    pub total_variance: f64,
    pub scores: Vec<Vec<f32>>,
}

const MAX_ITERS: usize = 300;
const CONVERGED: f64 = 1e-12;
/// Below this, a deflated eigenvalue direction is numerically exhausted:
/// the data has no variance left and the component is recorded as zero.
const RANK_EPS: f64 = 1e-12;

/// Top-`k` PCA of `rows` (all rows the same width). Empty input yields an
/// empty `Pca` with no components.
pub fn pca(rows: &[Vec<f32>], k: usize) -> Pca {
    if rows.is_empty() || k == 0 {
        return Pca {
            components: Vec::new(),
            variances: Vec::new(),
            total_variance: 0.0,
            scores: vec![Vec::new(); rows.len()],
        };
    }
    let n = rows.len();
    let d = rows[0].len();

    // Column means, ascending row order (fixed float reduction).
    let mut mean = vec![0.0f64; d];
    for r in rows {
        for (m, &x) in mean.iter_mut().zip(r) {
            *m += x as f64;
        }
    }
    for m in &mut mean {
        *m /= n as f64;
    }

    // Covariance (unnormalized): C = Xcᵀ Xc, upper triangle then mirrored.
    // O(n·d²) once; every power iteration afterwards is O(d²).
    let mut cov = vec![0.0f64; d * d];
    let mut centered = vec![0.0f64; d];
    for r in rows {
        for (c, (&x, m)) in r.iter().zip(&mean).enumerate() {
            centered[c] = x as f64 - m;
        }
        for i in 0..d {
            let ci = centered[i];
            if ci == 0.0 {
                continue;
            }
            for j in i..d {
                cov[i * d + j] += ci * centered[j];
            }
        }
    }
    for i in 0..d {
        for j in (i + 1)..d {
            cov[j * d + i] = cov[i * d + j];
        }
    }
    // Total variance = trace of the covariance / n (the eigenvalue sum, by
    // invariance of the trace) — the denominator of every explained share.
    let total_variance = (0..d).map(|i| cov[i * d + i]).sum::<f64>() / n as f64;

    let mut components: Vec<Vec<f64>> = Vec::with_capacity(k);
    let mut variances: Vec<f64> = Vec::with_capacity(k);
    let mut exhausted = false;
    for c in 0..k.min(d) {
        if exhausted {
            components.push(vec![0.0; d]);
            variances.push(0.0);
            continue;
        }
        let mut v = seed_vector(d, c as u64);
        orthogonalize(&mut v, &components);
        if normalize(&mut v) < RANK_EPS {
            // The seed fell (numerically) inside the found subspace; the
            // remaining space is degenerate for our purposes.
            exhausted = true;
            components.push(vec![0.0; d]);
            variances.push(0.0);
            continue;
        }
        let mut dead = false;
        for _ in 0..MAX_ITERS {
            let mut w = mat_vec(&cov, &v, d);
            orthogonalize(&mut w, &components);
            if normalize(&mut w) < RANK_EPS {
                dead = true; // no variance left along any remaining direction
                break;
            }
            let cos = dot(&w, &v);
            v = w;
            if cos > 1.0 - CONVERGED {
                break;
            }
        }
        if dead {
            exhausted = true;
            components.push(vec![0.0; d]);
            variances.push(0.0);
            continue;
        }
        fix_sign(&mut v);
        let cv = mat_vec(&cov, &v, d);
        variances.push(dot(&cv, &v) / n as f64);
        components.push(v);
    }
    // k > d: pad with zero components so the output is always k wide.
    while components.len() < k {
        components.push(vec![0.0; d]);
        variances.push(0.0);
    }

    let scores = rows
        .iter()
        .map(|r| {
            components
                .iter()
                .map(|comp| {
                    let mut s = 0.0f64;
                    for ((&x, m), &w) in r.iter().zip(&mean).zip(comp) {
                        s += (x as f64 - m) * w;
                    }
                    s as f32
                })
                .collect()
        })
        .collect();

    Pca {
        components,
        variances,
        total_variance,
        scores,
    }
}

/// Fixed pseudo-random start vector: an LCG (Knuth's MMIX constants) seeded
/// by the component index — no OS RNG, no clock, identical every run.
fn seed_vector(d: usize, component: u64) -> Vec<f64> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ (component.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    (0..d)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) as f64 / (1u64 << 31) as f64) - 0.5
        })
        .collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// `m` (d×d, row-major) times `v`. TKI-59: rows are computed 4 at a time
/// with 4 independent accumulators — the single-chain `dot` is bound by
/// f64 add latency, and this is the power iteration's whole cost. Each
/// row keeps `dot`'s exact ascending-element order, so every output
/// element is bit-identical to `dot(&m[i*d..], v)`. (8 lanes measured
/// ~2x SLOWER than 4 here — register pressure; don't widen it back.)
fn mat_vec(m: &[f64], v: &[f64], d: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(d);
    let mut i = 0;
    while i + 4 <= d {
        let r0 = &m[i * d..i * d + d];
        let r1 = &m[(i + 1) * d..(i + 1) * d + d];
        let r2 = &m[(i + 2) * d..(i + 2) * d + d];
        let r3 = &m[(i + 3) * d..(i + 3) * d + d];
        let (mut s0, mut s1, mut s2, mut s3) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (t, &x) in v.iter().enumerate() {
            s0 += r0[t] * x;
            s1 += r1[t] * x;
            s2 += r2[t] * x;
            s3 += r3[t] * x;
        }
        out.extend_from_slice(&[s0, s1, s2, s3]);
        i += 4;
    }
    while i < d {
        out.push(dot(&m[i * d..(i + 1) * d], v));
        i += 1;
    }
    out
}

/// Subtract `v`'s projection onto each of `basis` (all unit vectors) — the
/// deflation that keeps each new component orthogonal to those found.
fn orthogonalize(v: &mut [f64], basis: &[Vec<f64>]) {
    for b in basis {
        let p = dot(v, b);
        if p != 0.0 {
            for (x, &y) in v.iter_mut().zip(b) {
                *x -= p * y;
            }
        }
    }
}

/// Scale to unit norm; returns the pre-scaling norm (0-safe: leaves `v`
/// untouched when it is numerically zero).
fn normalize(v: &mut [f64]) -> f64 {
    let n = dot(v, v).sqrt();
    if n >= RANK_EPS {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
    n
}

/// Sign convention: the largest-|loading| coordinate (ties → lowest index)
/// is forced positive, so component orientation is a function of the data,
/// not of the iteration path.
fn fix_sign(v: &mut [f64]) {
    let mut best = 0usize;
    for (i, x) in v.iter().enumerate() {
        if x.abs() > v[best].abs() {
            best = i;
        }
    }
    if v[best] < 0.0 {
        for x in v.iter_mut() {
            *x = -*x;
        }
    }
}
