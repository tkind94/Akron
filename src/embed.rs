//! Embedding backend for `akron find` and `akron explore` (TKI-41 port,
//! model swapped by TKI-49). The one sanctioned exception to DESIGN.md
//! §1.2's no-learned-model rule — entirely behind the `semantic` Cargo
//! feature (on by default). `find.rs` and `explore.rs` are the only
//! consumers; nothing else in the engine depends on this module.
//!
//! Model: EmbeddingGemma-300m, int8 (`onnx-community/embeddinggemma-300m-ONNX`)
//! — the strongest model of both bake-off rounds
//! (R&D archive spike/embed2/RESULTS.md "Round 2"): best graded find P@5 (pooled 0.59;
//! 0.68 on the private domain corpus, 0.50 on httpx — beats a best-faith
//! grep on both repo shapes). NB: that 0.59 pools a private corpus absent
//! from `scripts/eval-find`; the public httpx+scrapy questions score 0.514
//! (reproduced 2026-07-12, TKI-75) — compare candidates against 0.514, not
//! 0.59. Best-or-tied map quality, at 331 MB
//! download / ~2.2 GB peak RSS. Pulled from Hugging Face on first use
//! under the model's own terms (Gemma Terms of Use) — Akron redistributes
//! nothing.

use crate::find::MODEL_KEY;
use anyhow::{bail, Context, Result};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MODEL_VARIANT: EmbeddingModel = EmbeddingModel::EmbeddingGemma300MQ;
/// hf-hub's directory for this model inside `models_dir()`. Pin
/// verification walks THIS subtree only — the cache root can hold other
/// (orphaned or future) models whose same-named files would otherwise be
/// hashed against the wrong pins.
const MODEL_REPO_DIR: &str = "models--onnx-community--embeddinggemma-300m-ONNX";

/// Task prompts the model's authors require — fastembed does not prepend
/// them itself. Dropping these silently costs retrieval quality
/// (R&D archive spike/embed2/RESULTS.md, round 2: the exact pair measured).
pub const QUERY_PREFIX: &str = "task: search result | query: ";
pub const DOC_PREFIX: &str = "title: none | text: ";

const APPROX_DOWNLOAD_MB: u64 = 331;

/// Every file fastembed reads at runtime: pulled once (2026-07-06 from
/// `onnx-community/embeddinggemma-300m-ONNX`, snapshot 5090578), hashed,
/// and hardcoded here. All six are pinned — the weights carry an
/// external-data companion (`model_quantized.onnx_data`), and
/// `tokenizer_config.json` drives truncation/padding and can change
/// embeddings without touching the model file. Verified on every `load()`;
/// a mismatch means the on-disk file is no longer what shipped, and we
/// refuse to embed with it rather than silently serve vectors from a
/// tampered or corrupted model.
const PINNED_FILES: [(&str, &str); 6] = [
    (
        "model_quantized.onnx",
        "172efde319fe1542dc41f31be6154910b05b78f7a861c265c4600eec906bd6d8",
    ),
    (
        "model_quantized.onnx_data",
        "705626e28e4c23c82ade34566b4197d97f534c12275fa406dfb71e9937d388c0",
    ),
    (
        "tokenizer.json",
        "4dda02faaf32bc91031dc8c88457ac272b00c1016cc679757d1c441b248b9c47",
    ),
    (
        "config.json",
        "6e1f06404b7163e0325ed2ea3e6781cde50f4a50b31780a95ad0d30e8404d77b",
    ),
    (
        "special_tokens_map.json",
        "2f7b0adf4fb469770bb1490e3e35df87b1dc578246c5e7e6fc76ecf33213a397",
    ),
    (
        "tokenizer_config.json",
        "3ca953eea6c3c9fcda9cf3df22949ff18b216f7c74bd6459230f3f1013953f3a",
    ),
];

/// `$XDG_CACHE_HOME/akron/models` (default `~/.cache/akron/models`).
pub fn models_dir() -> PathBuf {
    xdg_cache_home().join("akron").join("models")
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

/// L2-normalize in place, so cosine similarity reduces to a plain dot
/// product at rank time (mirrors the spike's `l2norm`).
fn l2_normalize(v: &mut [f32]) {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}

pub struct Embedder(TextEmbedding);

impl Embedder {
    /// Load the pinned model, pulling it into `models_dir()` on first use.
    /// Prints a terse stderr notice — what's downloading, how big, where
    /// from, where it lands, that it's one-time, and a pointer to the
    /// model's own terms — only when actually pulling (silent on a warm
    /// cache), used by both `find` and `explore` since both go through this
    /// one loader. Network failure or a missing model offline surfaces as
    /// `Err` — the caller turns that into exit 2.
    pub fn load() -> Result<Self> {
        let dir = models_dir();
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let repo_dir = dir.join(MODEL_REPO_DIR);
        // Verify whatever is already on disk BEFORE handing it to ort/the
        // tokenizer — a tampered cache must never reach a parser. Files not
        // present yet get pulled by `try_new` below and verified after.
        let mut all_pre_verified = true;
        for (name, sha) in PINNED_FILES {
            let matches = find_files(&repo_dir, name);
            if matches.is_empty() {
                all_pre_verified = false;
            } else {
                verify_matches(&dir, &matches, name, sha)?;
            }
        }
        if !all_pre_verified {
            eprintln!(
                "akron: first run — pulling {MODEL_KEY} (~{APPROX_DOWNLOAD_MB} MB) from Hugging Face -> {}",
                dir.display()
            );
            eprintln!(
                "akron: one-time; cached there for future runs, pulled under the model's own Gemma Terms of Use (https://ai.google.dev/gemma/terms) — akron redistributes nothing"
            );
        }
        // hf-hub draws a per-file progress bar (indicatif, stderr) when this
        // is true — only worth paying for when we know a pull is happening;
        // a warm cache never calls hf-hub's download path at all, so this
        // flag is a no-op there either way.
        let opts = TextInitOptions::new(MODEL_VARIANT)
            .with_cache_dir(dir.clone())
            .with_show_download_progress(!all_pre_verified);
        let model = TextEmbedding::try_new(opts).with_context(|| {
            format!(
                "loading embedding model {MODEL_KEY} (network unreachable, or not cached and offline)"
            )
        })?;
        if !all_pre_verified {
            for (name, sha) in PINNED_FILES {
                let matches = find_files(&repo_dir, name);
                if matches.is_empty() {
                    bail!(
                        "model file {name} missing after load (cache dir {})",
                        dir.display()
                    );
                }
                verify_matches(&dir, &matches, name, sha)?;
            }
            eprintln!("akron: model cached — subsequent runs use the local copy");
        }
        Ok(Embedder(model))
    }

    /// Embed document texts (the "qualified" qname+path+source text
    /// `find::doc_text` builds), L2-normalized. Chunked EXTERNALLY with
    /// per-call batch `None`: the int8 dynamically-quantized model refuses
    /// fastembed-internal batching (quantization ranges would differ per
    /// batch), and a fixed external chunk bounds memory instead — the same
    /// scheme the bake-off harness measured with (R&D archive spike/embed2).
    pub fn embed_docs(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for part in texts.chunks(32) {
            let prefixed: Vec<String> =
                part.iter().map(|t| format!("{DOC_PREFIX}{t}")).collect();
            let mut vecs = self.0.embed(prefixed, None).context("embedding failed")?;
            for v in &mut vecs {
                l2_normalize(v);
            }
            out.append(&mut vecs);
        }
        Ok(out)
    }

    /// Embed one query, with the model's retrieval-instruction prefix.
    pub fn embed_query(&mut self, text: &str) -> Result<Vec<f32>> {
        let prefixed = format!("{QUERY_PREFIX}{text}");
        let mut out = self
            .0
            .embed(vec![prefixed], None)
            .context("embedding query failed")?;
        let mut v = out.pop().context("empty embedding output")?;
        l2_normalize(&mut v);
        Ok(v)
    }
}

/// Find every entry named `name` under `dir` — fastembed/hf-hub nest the
/// actual files under a `models--org--repo/snapshots/<rev>/` path whose
/// `<rev>` component isn't under our control (and can move if the upstream
/// repo's `main` ref advances), so we locate them by filename instead of a
/// hardcoded relative path. ALL matches are returned and ALL get verified:
/// if two snapshots ever coexist, fastembed loaded one of them, and pinning
/// only the first walkdir hit would leave the loaded one unchecked.
fn find_files(dir: &Path, name: &str) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str() == Some(name))
        .map(|e| e.into_path())
        .collect()
}

fn verify_matches(dir: &Path, matches: &[PathBuf], name: &str, expected_sha256: &str) -> Result<()> {
    for path in matches {
        let bytes =
            std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let got = format!("{:x}", hasher.finalize());
        if got != expected_sha256 {
            bail!(
                "model file integrity check failed: {name} sha256 mismatch (expected {expected_sha256}, got {got}).\n\
                 Delete {} (or the whole {}) and re-run to re-pull the pinned model.",
                path.display(),
                dir.display()
            );
        }
    }
    Ok(())
}
