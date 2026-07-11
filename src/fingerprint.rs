//! Channel A fingerprints (DESIGN.md §2.2): Merkle subtree hashing for exact
//! structural identity, Weisfeiler-Leman label histograms for near-miss
//! similarity, MinHash signatures for sub-linear candidate pairing.

use crate::types::NormTree;
use std::collections::{HashMap, HashSet};
use xxhash_rust::xxh3::{xxh3_64, xxh3_64_with_seed};

pub const MINHASH_FNS: usize = 64;

pub fn merkle_root(tree: &NormTree) -> u64 {
    fn h(tree: &NormTree, i: u32) -> u64 {
        let mut acc: Vec<u8> = tree.labels[i as usize].to_le_bytes().to_vec();
        for &k in &tree.children[i as usize] {
            acc.extend_from_slice(&h(tree, k).to_le_bytes());
        }
        xxh3_64(&acc)
    }
    h(tree, (tree.labels.len() - 1) as u32) // post-order: root is last
}

/// Every node's subtree Merkle hash, index-aligned to `tree` (position `i` holds
/// the hash of node `i`'s subtree). Computed bottom-up in one pass: post-order
/// guarantees every child index is `< i`, so a child's hash is already resolved
/// when its parent is reached. Byte-identical to `merkle_root`'s recursion — the
/// same accumulator (label bytes, then each child's subtree hash, little-endian)
/// and the same `xxh3_64` — so `subtree_hashes(t).last() == Some(&merkle_root(t))`.
pub fn subtree_hashes(tree: &NormTree) -> Vec<u64> {
    let n = tree.labels.len();
    let mut hashes = vec![0u64; n];
    for i in 0..n {
        let mut acc: Vec<u8> = tree.labels[i].to_le_bytes().to_vec();
        for &k in &tree.children[i] {
            acc.extend_from_slice(&hashes[k as usize].to_le_bytes());
        }
        hashes[i] = xxh3_64(&acc);
    }
    hashes
}

/// The set of all subtree Merkle hashes in `tree` — the structural vocabulary a
/// member shares with an exemplar (see `regions.rs`). Reuses `subtree_hashes`,
/// so it stays byte-compatible with `merkle_root` (the root hash is always a
/// member of the returned set).
pub fn subtree_hash_set(tree: &NormTree) -> HashSet<u64> {
    subtree_hashes(tree).into_iter().collect()
}

/// Multiset of WL labels across iterations 1..=iters, sorted by label.
///
/// The relabeling neighborhood is bidirectional (parent + children), as in
/// standard WL on the undirected tree. Children-only relabeling leaves leaf
/// labels frozen forever — and since ~half of all normalized nodes are leaves
/// with heavily shared labels (EXT, x0, STR), that collision mass made every
/// pair of Python functions look ~0.5-similar. Iteration 0 (bare node kinds)
/// is excluded for the same reason; deeper iterations weigh more because
/// they encode more specific structure.
pub fn wl_histogram(tree: &NormTree, iters: usize) -> Vec<(u64, f32)> {
    let n = tree.labels.len();
    let mut parent = vec![u32::MAX; n];
    for (i, kids) in tree.children.iter().enumerate() {
        for &k in kids {
            parent[k as usize] = i as u32;
        }
    }
    let mut cur = tree.labels.clone();
    let mut hist: HashMap<u64, f32> = HashMap::new();
    let mut buf = Vec::new();
    for iter in 1..=iters {
        let mut next = vec![0u64; n];
        for i in 0..n {
            buf.clear();
            buf.extend_from_slice(&cur[i].to_le_bytes());
            let p = parent[i];
            buf.extend_from_slice(&(if p == u32::MAX { 0 } else { cur[p as usize] }).to_le_bytes());
            for &k in &tree.children[i] {
                buf.extend_from_slice(&cur[k as usize].to_le_bytes());
            }
            next[i] = xxh3_64(&buf);
        }
        cur = next;
        for &l in &cur {
            *hist.entry(l).or_default() += iter as f32;
        }
    }
    let mut out: Vec<(u64, f32)> = hist.into_iter().collect();
    out.sort_unstable_by_key(|&(l, _)| l);
    out
}

pub fn minhash(labels: impl Iterator<Item = u64> + Clone) -> Vec<u64> {
    (0..MINHASH_FNS as u64)
        .map(|seed| {
            labels
                .clone()
                .map(|l| xxh3_64_with_seed(&l.to_le_bytes(), seed))
                .min()
                .unwrap_or(u64::MAX)
        })
        .collect()
}

/// Sum of squared weights of a sorted sparse vector — the un-square-rooted
/// half of `cosine`'s norm that depends on only one side. Hoisting this out
/// of a candidate-pair loop (compute once per vector instead of once per
/// pair) is bit-identical to the norm `cosine` computes inline: same
/// summation order, just memoized (`cluster.rs`'s shape-clustering and
/// vocabulary candidate loops re-touch the same vectors across many pairs).
pub fn norm_sq<K: Copy>(v: &[(K, f32)]) -> f32 {
    v.iter().map(|&(_, w)| w * w).sum()
}

/// Cosine given precomputed (squared) norms — the merge-join dot product
/// only. `cosine(a, b) == cosine_with_norms(a, b, norm_sq(a), norm_sq(b))`
/// bit-for-bit; used where the same vector's norm is reused across many
/// pairs.
pub fn cosine_with_norms<K: Ord + Copy>(a: &[(K, f32)], b: &[(K, f32)], na: f32, nb: f32) -> f32 {
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
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Cosine over two sorted sparse vectors, keyed by any `Ord` key (WL labels
/// are `u64`, vocabulary term ids are `u32` — genericizing over the key type
/// avoids a throwaway copy-and-widen at every call site that only had `u32`
/// keys, e.g. `cluster.rs`'s vocabulary cosine).
pub fn cosine<K: Ord + Copy>(a: &[(K, f32)], b: &[(K, f32)]) -> f32 {
    cosine_with_norms(a, b, norm_sq(a), norm_sq(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(labels: &[u64]) -> NormTree {
        // Post-order chain: node 0 is the leaf, the last node is the root.
        let n = labels.len();
        NormTree {
            labels: labels.to_vec(),
            children: (0..n)
                .map(|i| if i > 0 { vec![(i - 1) as u32] } else { vec![] })
                .collect(),
            spans: (0..n as u32).map(|i| (i, i + 1)).collect(),
        }
    }

    #[test]
    fn identical_trees_identical_fingerprints() {
        let a = chain(&[1, 2, 3]);
        let b = chain(&[1, 2, 3]);
        assert_eq!(merkle_root(&a), merkle_root(&b));
        assert_eq!(wl_histogram(&a, 3), wl_histogram(&b, 3));
        assert_eq!(
            minhash(wl_histogram(&a, 3).iter().map(|&(l, _)| l)),
            minhash(wl_histogram(&b, 3).iter().map(|&(l, _)| l))
        );
    }

    #[test]
    fn subtree_hashes_root_equals_merkle_root() {
        let a = chain(&[1, 2, 3, 4]);
        let hs = subtree_hashes(&a);
        // Byte-compatibility: the last (root) per-node hash IS the merkle root.
        assert_eq!(hs.last().copied(), Some(merkle_root(&a)));
        let set = subtree_hash_set(&a);
        assert!(set.contains(&merkle_root(&a)));
        assert_eq!(set.len(), 4); // four distinct nested subtrees
    }

    #[test]
    fn different_trees_differ() {
        let a = chain(&[1, 2, 3]);
        let b = chain(&[1, 2, 4]);
        assert_ne!(merkle_root(&a), merkle_root(&b));
        assert!(cosine(&wl_histogram(&a, 3), &wl_histogram(&b, 3)) < 1.0);
    }

    #[test]
    fn cosine_bounds() {
        let a = chain(&[1, 2, 3, 4, 5]);
        let b = chain(&[1, 2, 3, 4, 6]);
        let c = cosine(&wl_histogram(&a, 2), &wl_histogram(&b, 2));
        assert!(
            c > 0.0 && c < 1.0,
            "partial overlap should be in (0,1): {c}"
        );
        assert!((cosine(&wl_histogram(&a, 2), &wl_histogram(&a, 2)) - 1.0).abs() < 1e-6);
    }
}
