#!/usr/bin/env python3
"""TKI-64 Part 4: MRL (Matryoshka) truncation evidence.

Reads the 768-d vector dumps produced by `cargo run --release --example
mrl_dump -- <root> <corpus-name> <out.json>` and, for each requested
dimension, truncates every vector to the first k dims, re-normalizes
(L2), and reports:

  (a) find P@5 on the eval questions at that dimension (query vectors are
      truncated/re-normalized the same way as doc vectors — this mirrors
      what a real truncated-embedding find would do, purely offline,
      no product code touched).
  (b) kNN@8 neighbor-set overlap vs the 768-d kNN, across every ranked
      symbol in the corpus — the map-relevant metric (explore's kNN
      layout, not wired here; see TKI-64 report).

Pure numeric, no model dependency — stdlib only (no numpy requirement).

Usage:
    python3 mrl_analyze.py <dump1.json> [<dump2.json> ...] [--dims 768,256,128] [--knn-k 8]
"""
import argparse
import json
import math
import sys


def truncate_renorm(vec, k):
    v = vec[:k]
    n = math.sqrt(sum(x * x for x in v))
    if n > 0:
        v = [x / n for x in v]
    return v


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def p_at_k(query_vec, symbols, expected, top=5):
    scored = []
    for i, s in enumerate(symbols):
        scored.append((dot(query_vec, s["vec"]), i))
    scored.sort(key=lambda t: (-t[0], t[1]))
    hits = 0
    for _, i in scored[:top]:
        qname = symbols[i]["qname"]
        if any(e in qname for e in expected):
            hits += 1
    return hits, top


def knn_set(vecs, i, k):
    """Indices of the k nearest neighbors of vecs[i] (excluding itself),
    by cosine (plain dot product — vecs are already L2-normalized)."""
    scored = []
    for j, v in enumerate(vecs):
        if j == i:
            continue
        scored.append((dot(vecs[i], v), j))
    scored.sort(key=lambda t: (-t[0], t[1]))
    return {j for _, j in scored[:k]}


def analyze(dump_path, dims, knn_k, knn_sample):
    with open(dump_path) as f:
        data = json.load(f)
    corpus = data["corpus"]
    symbols = data["symbols"]
    queries = data["queries"]
    n = len(symbols)
    print(f"=== {corpus}  ({n} symbols, {len(queries)} questions) ===")

    base_dim = len(symbols[0]["vec"])

    # Precompute 768-d kNN for a sample of symbols (all of them if the
    # corpus is small; O(n^2) dot products otherwise gets expensive —
    # cap at knn_sample, deterministic first-N so results are reproducible).
    sample_idx = list(range(min(n, knn_sample)))
    full_vecs = [s["vec"] for s in symbols]
    knn_768 = {i: knn_set(full_vecs, i, knn_k) for i in sample_idx}

    results = {}
    for k in dims:
        trunc_symbols = [
            {"qname": s["qname"], "vec": truncate_renorm(s["vec"], k)} for s in symbols
        ]
        trunc_vecs = [s["vec"] for s in trunc_symbols]

        # --- P@5 at this dimension ---
        total_hits, total_possible = 0, 0
        per_q = []
        for q in queries:
            qvec = truncate_renorm(q["vec"], k)
            hits, top = p_at_k(qvec, trunc_symbols, q["expected"], top=5)
            total_hits += hits
            total_possible += top
            per_q.append((q["id"], hits, top))

        # --- kNN@k overlap vs 768-d ---
        overlaps = []
        for i in sample_idx:
            knn_k_dim = knn_set(trunc_vecs, i, knn_k)
            overlap = len(knn_k_dim & knn_768[i]) / knn_k
            overlaps.append(overlap)
        mean_overlap = sum(overlaps) / len(overlaps) if overlaps else float("nan")

        p5 = total_hits / total_possible if total_possible else float("nan")
        print(
            f"  dim={k:>4}  P@5={total_hits}/{total_possible}={p5:.3f}  "
            f"kNN@{knn_k} overlap vs 768d = {mean_overlap:.3f}  "
            f"(n_sampled={len(sample_idx)})"
        )
        results[k] = {"p5": p5, "hits": total_hits, "possible": total_possible, "knn_overlap": mean_overlap}
    return corpus, results


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dumps", nargs="+")
    ap.add_argument("--dims", default="768,256,128")
    ap.add_argument("--knn-k", type=int, default=8)
    ap.add_argument("--knn-sample", type=int, default=100000)
    args = ap.parse_args()
    dims = [int(x) for x in args.dims.split(",")]

    all_results = {}
    for dump in args.dumps:
        corpus, results = analyze(dump, dims, args.knn_k, args.knn_sample)
        all_results[corpus] = results

    print("\n=== summary (all corpora) ===")
    for k in dims:
        h = sum(r[k]["hits"] for r in all_results.values())
        p = sum(r[k]["possible"] for r in all_results.values())
        ov = sum(r[k]["knn_overlap"] for r in all_results.values()) / len(all_results)
        print(f"  dim={k:>4}  overall P@5={h}/{p}={h/p:.3f}  mean kNN@{args.knn_k} overlap={ov:.3f}")


if __name__ == "__main__":
    main()
