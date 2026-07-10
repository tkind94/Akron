//! `akron find <path> <query>` (TKI-41 / EXP-A): symbol-level semantic code
//! search, ported from the measured spike (`R&D archive spike/find`, see RESULTS.md
//! there). Read-only over the engine's scan output — never loads verdicts,
//! never touches `.akron/`, no gating concept anywhere.
//! `<path>` follows the same convention as `scan`/`explain`.
//!
//! Ranking levers ported exactly as measured (RESULTS.md "Conditions
//! attached to KEEP", in order of measured value):
//!   1. `is_test` symbols are DROPPED from the ranking by default. The spike
//!      measured full exclusion (a P@5 table computed with test symbols
//!      removed entirely), not a soft downrank factor — there is no
//!      "downranking factor" to port, only a drop. `--tests` opts back in,
//!      for the usage-phrased-question exception RESULTS.md also measured
//!      ("how do I stream a response" is legitimately answered by a test).
//!   2. Embedding text is the "qualified" variant RESULTS.md measured:
//!      `qname + "  (" + file + ")\n" + source` — free precision on every
//!      model, both corpora, in the bake-off.
//!   3. Default (only) model is embeddinggemma-300m-q (`embed.rs`; swapped
//!      by TKI-49), cosine-ranked (vectors pre-normalized, so plain dot
//!      product).
//!
//! NOT ported: indexing module-level spans. RESULTS.md names this as the
//! fix for one loss class (P-q8, module-level orchestration wiring) but the spike never
//! implemented or measured it — it's a proposed follow-up, not a measured
//! lever, so this cutover ships function/method-granularity only, exactly
//! what the spike's own binary produced.
//!
//! Embedding index cache key: a hash of the FULL embedded document text
//! (the qualified string above), not the unit's Merkle root. A Merkle root
//! doesn't change on rename, but the qualified text (and thus the correct
//! embedding) does — keying by Merkle root would serve a stale vector under
//! the old qname after a rename. This mirrors the spike's own
//! content-hash-of-doc-text cache (`EmbCache`/`fnv1a` in
//! `R&D archive spike/find/src/main.rs`), with `xxhash-rust`'s xxh3 standing in for the
//! spike's FNV-1a since xxh3 is already this crate's hash of choice
//! (`fingerprint::merkle_root`, `cluster.rs`'s LSH buckets) — same scheme,
//! no new hash implementation.

#[cfg(feature = "semantic")]
use crate::scan;
#[cfg(feature = "semantic")]
use crate::types::Config;
use crate::types::SymbolPrint;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Identifies the embedding model this build ranks with — part of the index
/// cache's file name so a future model change can never mix old-model doc
/// vectors with new-model query vectors (they'd score garbage silently).
/// Lives here (not `embed.rs`) so the cache path stays derivable without
/// the `semantic` feature.
pub const MODEL_KEY: &str = "embeddinggemma-300m-q";
/// EmbeddingGemma-300m's embedding width. Cached vectors of any other
/// length are rejected at load (re-embedded), and a model that returns a
/// different width fails loudly rather than ranking on truncated dots.
pub const EMBED_DIM: usize = 768;

/// One ranked hit.
pub struct Hit {
    pub rank: usize,
    pub score: f32,
    pub qname: String,
    pub file: String,
    pub line: usize,
}

pub struct FindReport {
    pub query: String,
    pub top: usize,
    pub hits: Vec<Hit>,
}

/// Tool-voice text output: one row per hit, rank/score(2dp)/qname/file:line,
/// no headers, no prose.
pub fn render_text(report: &FindReport) {
    if report.hits.is_empty() {
        println!("no hits");
        return;
    }
    for h in &report.hits {
        println!(
            "{:>2}  {:.2}  {}  {}:{}",
            h.rank, h.score, h.qname, h.file, h.line
        );
    }
}

pub fn render_json(report: &FindReport) -> serde_json::Value {
    json!({
        "schema": "akron.find/v1",
        "query": report.query,
        "top": report.top,
        "hits": report.hits.iter().map(|h| json!({
            "rank": h.rank,
            "score": h.score,
            "qname": h.qname,
            "file": h.file,
            "line": h.line,
        })).collect::<Vec<_>>(),
    })
}

// ── repo-key + embedding-index cache path (pure; no model needed) ──

/// Hex digest of the canonical repo root path — the cache-dir identity
/// (`$XDG_CACHE_HOME/akron/find/<repo-key>/index.bin`), so the same repo
/// scanned from the same location always lands on the same cache regardless
/// of what else is running.
pub fn repo_key(root: &Path) -> String {
    format!(
        "{:016x}",
        xxhash_rust::xxh3::xxh3_64(root.display().to_string().as_bytes())
    )
}

fn xdg_cache_home() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
        })
}

/// `$XDG_CACHE_HOME/akron/find/<repo-key>/<MODEL_KEY>.bin` — never under
/// the scanned repo's own `.akron/`. The model key in the file name
/// versions the cache per model.
pub fn index_path(root: &Path) -> PathBuf {
    xdg_cache_home()
        .join("akron")
        .join("find")
        .join(repo_key(root))
        .join(format!("{MODEL_KEY}.bin"))
}

/// The embedding cache: entries keyed by `doc_key` (a hash of the full
/// embedded document text — see module doc for why this, not Merkle root).
/// A pure accelerator: on each run, only keys absent from the map are
/// embedded; everything else is served from disk.
#[derive(Default)]
pub struct EmbCache {
    pub map: HashMap<String, Vec<f32>>,
}

/// Cache key for one document's embedding: a hash of its full text.
pub fn doc_key(doc_text: &str) -> String {
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(doc_text.as_bytes()))
}

impl EmbCache {
    /// Unreadable/corrupted files load as empty (the cache self-heals by
    /// re-embedding); entries whose vector isn't `EMBED_DIM` wide are
    /// dropped the same way — `dot` would silently truncate to the shorter
    /// vector and rank on garbage otherwise.
    pub fn load(path: &Path) -> Self {
        let mut map: HashMap<String, Vec<f32>> = fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        map.retain(|_, v| v.len() == EMBED_DIM);
        EmbCache { map }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let bytes = serde_json::to_vec(&self.map).context("serializing embedding cache")?;
        // Write to a temp file in the same directory, then rename over the
        // real path — rename is atomic, a plain `fs::write` is not (a
        // process killed mid-write, or two concurrent `find` runs on the
        // same repo, would otherwise leave a truncated/interleaved cache
        // that `load` would then have to fail on rather than self-heal).
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        fs::write(&tmp, &bytes).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, path)
            .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))
    }
}

// ── ranking (pure; unit-testable without the model) ──

/// The "qualified" embedding text RESULTS.md measured: `qname  (file)\n
/// source` — exact spike format (`R&D archive spike/find/src/main.rs`'s `qualified`
/// field).
pub fn doc_text(qname: &str, file: &str, source: &str) -> String {
    format!("{qname}  ({file})\n{source}")
}

/// Indices of the symbols kept for ranking: `is_test` symbols dropped
/// unless `include_tests` (RESULTS.md's measured lever — full exclusion).
pub fn ranked_symbol_ids(symbols: &[SymbolPrint], include_tests: bool) -> Vec<usize> {
    symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| include_tests || !s.is_test)
        .map(|(i, _)| i)
        .collect()
}

/// The qualified doc text for each of `ids`, reading each symbol's source
/// span from disk (each file read once, in `ids` order). Extracted verbatim
/// from `search` so `explore` builds the identical index — same texts,
/// therefore same cache keys, same cache file.
pub fn doc_texts(root: &Path, symbols: &[SymbolPrint], ids: &[usize]) -> Vec<String> {
    let mut file_bytes: HashMap<&str, Vec<u8>> = HashMap::new();
    let mut texts = Vec::with_capacity(ids.len());
    for &i in ids {
        let s = &symbols[i];
        let bytes = file_bytes
            .entry(s.sym.file.as_str())
            .or_insert_with(|| fs::read(root.join(&s.sym.file)).unwrap_or_default());
        let (a, b) = s.span;
        let src = String::from_utf8_lossy(bytes.get(a..b).unwrap_or(&[])).into_owned();
        texts.push(doc_text(&s.sym.qname, &s.sym.file, &src));
    }
    texts
}

/// Embed every `texts[i]` whose `keys[i]` is absent from `cache`, inserting
/// the fresh vectors. Returns whether anything was embedded — the caller
/// decides whether the cache is worth persisting. Extracted verbatim from
/// `search` (shared with `explore`'s startup indexing).
#[cfg(feature = "semantic")]
pub fn embed_missing(
    embedder: &mut crate::embed::Embedder,
    cache: &mut EmbCache,
    texts: &[String],
    keys: &[String],
) -> Result<bool> {
    let miss_idx: Vec<usize> = keys
        .iter()
        .enumerate()
        .filter(|(_, k)| !cache.map.contains_key(*k))
        .map(|(i, _)| i)
        .collect();
    if miss_idx.is_empty() {
        return Ok(false);
    }
    let miss_texts: Vec<String> = miss_idx.iter().map(|&i| texts[i].clone()).collect();
    let fresh = embedder.embed_docs(&miss_texts)?;
    if let Some(bad) = fresh.iter().find(|v| v.len() != EMBED_DIM) {
        anyhow::bail!(
            "model returned {}-dim vectors, expected {EMBED_DIM} — model/config drift",
            bad.len()
        );
    }
    for (mi, v) in miss_idx.iter().zip(fresh) {
        cache.map.insert(keys[*mi].clone(), v);
    }
    Ok(true)
}

/// Cosine-rank `candidates` (symbol index, pre-normalized vector) against
/// `query_vec` (also pre-normalized, so this is a plain dot product), then
/// take the top `top`. Ties break on symbol index for determinism (never on
/// hash-map iteration order).
pub fn rank(query_vec: &[f32], candidates: &[(usize, &[f32])], top: usize) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = candidates
        .iter()
        .map(|&(i, v)| (i, dot(query_vec, v)))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scored.truncate(top);
    scored
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// ── the scan config find uses (deliberately not the shipping default) ──

#[cfg(feature = "semantic")]
fn scan_cfg() -> Config {
    // min_nodes deliberately low: RESULTS.md — "a search index wants broad
    // coverage; the shipping default of 30 drops many real answer
    // functions". Ported verbatim from the spike's own `cfg()`. The other
    // fields feed Channel A/B computation `scan_repo` always performs;
    // find doesn't read any of it, so they're just the spike's values.
    Config {
        min_nodes: 8,
        wl_iters: 1,
        theta_clone: 0.6,
        theta_b: 0.55,
        theta_a_low: 0.30,
        theta_family: 0.5,
        theta_b_family: 0.5,
        top: 20,
    }
}

/// Scan `root`, embed (cache-accelerated), rank against `query`, return the
/// top `top` hits. Read-only: no verdict loading, no `.akron/` writes ever.
#[cfg(feature = "semantic")]
pub fn search(root: &Path, query: &str, top: usize, include_tests: bool) -> Result<FindReport> {
    use crate::embed::Embedder;

    let cfg = scan_cfg();
    let scanned = scan::scan_repo(root, &cfg);
    let symbols = &scanned.symbols;
    let ids = ranked_symbol_ids(symbols, include_tests);

    // Doc text per included symbol: find reads each symbol's own source
    // slice (already-public `SymbolPrint::span`) directly — no dependency
    // on scan.rs beyond what it already exposes.
    let texts = doc_texts(root, symbols, &ids);
    let keys: Vec<String> = texts.iter().map(|t| doc_key(t)).collect();

    let index_file = index_path(root);
    let mut cache = EmbCache::load(&index_file);

    // The query always needs a fresh embedding (it's new text every call),
    // and any cache miss needs the model too — so the model loads whenever
    // `find` runs at all. The model's warm load is sub-second
    // (RESULTS.md's own logistics table), so this is cheap.
    let mut embedder = Embedder::load()?;
    if embed_missing(&mut embedder, &mut cache, &texts, &keys)? {
        cache.save(&index_file)?;
    }
    let query_vec = embedder.embed_query(query)?;

    let candidates: Vec<(usize, &[f32])> = ids
        .iter()
        .zip(&keys)
        .map(|(&i, k)| (i, cache.map[k].as_slice()))
        .collect();
    let ranked = rank(&query_vec, &candidates, top);

    let hits = ranked
        .into_iter()
        .enumerate()
        .map(|(n, (i, score))| Hit {
            rank: n + 1,
            score,
            qname: symbols[i].sym.qname.clone(),
            file: symbols[i].sym.file.clone(),
            line: symbols[i].sym.line,
        })
        .collect();

    Ok(FindReport {
        query: query.to_string(),
        top,
        hits,
    })
}

#[cfg(not(feature = "semantic"))]
pub fn search(
    _root: &Path,
    _query: &str,
    _top: usize,
    _include_tests: bool,
) -> Result<FindReport> {
    anyhow::bail!("akron was built without the semantic feature")
}
