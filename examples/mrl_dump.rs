//! TKI-64 Part 4 (MRL truncation evidence) helper: dumps the full 768-d,
//! L2-normalized embedding vectors for every ranked symbol in a repo, plus
//! the 768-d query vectors for that corpus's questions in
//! `scripts/eval-find/questions.json`, to a JSON file. Not part of the
//! product — measurement-only, run with:
//!
//!   cargo run --release --example mrl_dump -- <repo-root> <corpus-name> <out.json>
//!
//! `<corpus-name>` filters `questions.json` by its `corpus` field (e.g.
//! `httpx-full`). Downstream truncation-to-k / re-normalization / P@5 /
//! kNN@8-overlap math is done by `scripts/eval-find/mrl_analyze.py` — pure
//! numeric work with no model dependency, kept out of Rust for iteration
//! speed.
//!
//! Reuses the corpus's existing `akron find` embedding cache (same
//! `scan_cfg`/`doc_texts`/`doc_key` pipeline as `find::search`) so this never
//! re-embeds documents that a prior `akron find` run already cached — only
//! the question queries need a fresh embed.
//!
//! Everything below needs the `semantic` feature (the embedder, and
//! `find::embed_missing`), gated the same way `find::search` gates itself —
//! `cargo test --no-default-features` compile-checks examples too, so `run`
//! and its imports live behind `#[cfg(feature = "semantic")]` and `main`
//! falls back to a one-line error under the no-`semantic` build.

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "semantic")]
    {
        imp::run()
    }
    #[cfg(not(feature = "semantic"))]
    {
        anyhow::bail!("mrl_dump requires the `semantic` feature (rebuild without --no-default-features)")
    }
}

#[cfg(feature = "semantic")]
mod imp {
    use akron::embed::Embedder;
    use akron::find::{self, EmbCache};
    use akron::scan;
    use akron::types::Config;
    use serde_json::json;
    use std::path::PathBuf;

    // Mirrors find.rs's private `scan_cfg()` verbatim (find's index-friendly
    // config, not the shipping default) — duplicated here rather than made
    // `pub` since this is the one caller outside find.rs that needs it, and
    // it's a measurement script, not a seam another module should grow to
    // depend on.
    fn scan_cfg() -> Config {
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

    pub fn run() -> anyhow::Result<()> {
        let args: Vec<String> = std::env::args().collect();
        if args.len() != 4 {
            anyhow::bail!("usage: mrl_dump <repo-root> <corpus-name> <out.json>");
        }
        let root = PathBuf::from(&args[1]);
        let corpus_name = &args[2];
        let out_path = PathBuf::from(&args[3]);

        let cfg = scan_cfg();
        let scanned = scan::scan_repo(&root, &cfg);
        let symbols = &scanned.symbols;
        let ids = find::ranked_symbol_ids(symbols, false);
        let texts = find::doc_texts(&root, symbols, &ids);
        let keys: Vec<String> = texts.iter().map(|t| find::doc_key(t)).collect();

        let index_file = find::index_path(&root);
        let mut cache = EmbCache::load(&index_file);
        eprintln!(
            "mrl_dump: {} ranked symbols, cache at {} ({} entries loaded)",
            ids.len(),
            index_file.display(),
            cache.map.len()
        );

        let mut embedder = Embedder::load()?;
        if find::embed_missing(&mut embedder, &mut cache, &texts, &keys)? {
            cache.save(&index_file)?;
            eprintln!("mrl_dump: embedded missing symbols, cache saved");
        }

        let symbols_json: Vec<_> = ids
            .iter()
            .zip(&keys)
            .map(|(&i, k)| {
                let s = &symbols[i];
                json!({
                    "qname": s.sym.qname,
                    "file": s.sym.file,
                    "line": s.sym.line,
                    "vec": cache.map[k],
                })
            })
            .collect();

        // Pull this corpus's questions straight from questions.json so the
        // query text used here always matches what `run.sh` graded.
        let questions_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/eval-find/questions.json");
        let questions_raw = std::fs::read_to_string(&questions_path)?;
        let questions: serde_json::Value = serde_json::from_str(&questions_raw)?;
        let mut queries_json = Vec::new();
        for q in questions["questions"].as_array().unwrap() {
            if q["corpus"].as_str() != Some(corpus_name.as_str()) {
                continue;
            }
            let id = q["id"].as_str().unwrap().to_string();
            let query = q["query"].as_str().unwrap().to_string();
            let vec = embedder.embed_query(&query)?;
            let expected = q["expected"].clone();
            queries_json.push(json!({
                "id": id,
                "query": query,
                "vec": vec,
                "expected": expected,
            }));
        }
        eprintln!("mrl_dump: {} matching questions embedded", queries_json.len());

        let out = json!({
            "corpus": corpus_name,
            "root": root.display().to_string(),
            "symbols": symbols_json,
            "queries": queries_json,
        });
        std::fs::write(&out_path, serde_json::to_vec(&out)?)?;
        eprintln!("mrl_dump: wrote {}", out_path.display());
        Ok(())
    }
}
