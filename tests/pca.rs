//! `pca` module (TKI-47): variance ordering, determinism, sign stability —
//! the three properties `akron explore`'s axes lean on. All model-free.

use akron::pca::pca;

/// Deterministic pseudo-random test matrix (fixed LCG — the tests must not
/// depend on OS randomness any more than the module does).
fn lcg_rows(n: usize, d: usize, seed: u64) -> Vec<Vec<f32>> {
    let mut state = seed;
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
    };
    (0..n).map(|_| (0..d).map(|_| next()).collect()).collect()
}

#[test]
fn variances_are_non_increasing() {
    let rows = lcg_rows(60, 16, 7);
    let p = pca(&rows, 8);
    assert_eq!(p.variances.len(), 8);
    for w in p.variances.windows(2) {
        assert!(
            w[0] >= w[1] - 1e-9,
            "variance must be deflation-ordered: {} then {}",
            w[0],
            w[1]
        );
    }
    assert!(p.variances[0] > 0.0, "random data has variance");
}

#[test]
fn identical_input_gives_bit_identical_output() {
    let rows = lcg_rows(40, 12, 42);
    let a = pca(&rows, 8);
    let b = pca(&rows, 8);
    for (ca, cb) in a.components.iter().zip(&b.components) {
        for (x, y) in ca.iter().zip(cb) {
            assert_eq!(x.to_bits(), y.to_bits(), "components must be bit-identical");
        }
    }
    for (sa, sb) in a.scores.iter().zip(&b.scores) {
        for (x, y) in sa.iter().zip(sb) {
            assert_eq!(x.to_bits(), y.to_bits(), "scores must be bit-identical");
        }
    }
    assert_eq!(a.variances, b.variances);
}

#[test]
fn sign_convention_largest_loading_is_positive() {
    let rows = lcg_rows(50, 10, 3);
    let p = pca(&rows, 8);
    for (c, comp) in p.components.iter().enumerate() {
        if p.variances[c] == 0.0 {
            continue; // zero components carry no sign
        }
        let mut best = 0usize;
        for (i, x) in comp.iter().enumerate() {
            if x.abs() > comp[best].abs() {
                best = i;
            }
        }
        assert!(
            comp[best] > 0.0,
            "component {c}: largest-|loading| coordinate must be positive"
        );
    }
}

#[test]
fn sign_is_stable_under_small_perturbation() {
    let rows = lcg_rows(50, 10, 9);
    let p = pca(&rows, 2);
    // Perturb every entry by ~1e-4 (deterministically) and re-run: the
    // leading components must not flip orientation.
    let noise = lcg_rows(50, 10, 10);
    let perturbed: Vec<Vec<f32>> = rows
        .iter()
        .zip(&noise)
        .map(|(r, nr)| r.iter().zip(nr).map(|(x, e)| x + e * 2e-4).collect())
        .collect();
    let q = pca(&perturbed, 2);
    for c in 0..2 {
        let cos: f64 = p.components[c]
            .iter()
            .zip(&q.components[c])
            .map(|(x, y)| x * y)
            .sum();
        assert!(
            cos > 0.99,
            "component {c} flipped or wandered under a tiny perturbation: cos {cos}"
        );
    }
}

#[test]
fn recovers_a_planted_dominant_direction() {
    // Points spread widely along u = (3,4)/5 in a 6-dim space, with small
    // spread on an orthogonal axis: PC-1 must align with u, and variance
    // must concentrate there.
    let u = [0.6f32, 0.8, 0.0, 0.0, 0.0, 0.0];
    let w = [0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0];
    let rows: Vec<Vec<f32>> = (0..40)
        .map(|i| {
            let t = (i as f32 - 20.0) / 2.0; // big spread on u
            let s = ((i * 7 % 11) as f32 - 5.0) / 50.0; // small spread on w
            u.iter().zip(&w).map(|(&a, &b)| t * a + s * b).collect()
        })
        .collect();
    let p = pca(&rows, 3);
    let align: f64 = p.components[0]
        .iter()
        .zip(u.iter())
        .map(|(x, &y)| x * y as f64)
        .sum();
    assert!(
        align.abs() > 0.999,
        "PC-1 must align with the planted direction: {align}"
    );
    assert!(
        p.variances[0] > 20.0 * p.variances[1],
        "variance must concentrate on PC-1: {:?}",
        p.variances
    );
}

#[test]
fn rank_deficient_data_pads_with_zero_components() {
    // Rank-1 data (all points on one line) asked for 4 components: PC-2..4
    // must be all-zero with zero variance, and every score row stays 4 wide.
    let rows: Vec<Vec<f32>> = (0..10)
        .map(|i| vec![i as f32, 2.0 * i as f32, -i as f32])
        .collect();
    let p = pca(&rows, 4);
    assert_eq!(p.variances.len(), 4);
    assert!(p.variances[0] > 0.0);
    for c in 1..4 {
        assert!(
            p.variances[c].abs() < 1e-9,
            "rank-1 data has no variance past PC-1: {:?}",
            p.variances
        );
    }
    for s in &p.scores {
        assert_eq!(s.len(), 4, "scores stay k wide regardless of rank");
    }
}

#[test]
fn identical_points_yield_all_zero_scores() {
    let rows: Vec<Vec<f32>> = (0..5).map(|_| vec![1.0, -2.0, 3.0]).collect();
    let p = pca(&rows, 3);
    for s in &p.scores {
        for &x in s {
            assert!(x.abs() < 1e-9, "no variance ⇒ no displacement");
        }
    }
}

#[test]
fn empty_input_is_empty_not_a_panic() {
    let p = pca(&[], 8);
    assert!(p.components.is_empty());
    assert!(p.scores.is_empty());
    assert_eq!(p.total_variance, 0.0);
}

#[test]
fn variance_shares_sum_to_at_most_one_and_are_ordered() {
    // Wide data (d=16), k=8: the shipped shares are a strict subset of the
    // spectrum — they must be ordered and sum below 1.
    let rows = lcg_rows(60, 16, 7);
    let p = pca(&rows, 8);
    assert!(p.total_variance > 0.0);
    let shares: Vec<f64> = p.variances.iter().map(|v| v / p.total_variance).collect();
    for w in shares.windows(2) {
        assert!(w[0] >= w[1] - 1e-12, "shares must be ordered: {shares:?}");
    }
    let sum: f64 = shares.iter().sum();
    assert!(sum <= 1.0 + 1e-9, "shares of total cannot exceed 1: {sum}");
    assert!(sum > 0.0);
}

#[test]
fn full_rank_shares_partition_the_total_variance() {
    // k = d: every direction is recovered, so the eigenvalue sum must equal
    // the covariance trace (within power-iteration tolerance).
    let rows = lcg_rows(50, 8, 21);
    let p = pca(&rows, 8);
    let sum: f64 = p.variances.iter().sum();
    assert!(
        (sum - p.total_variance).abs() < 1e-6 * p.total_variance,
        "eigenvalue sum {sum} vs trace {}",
        p.total_variance
    );
}
