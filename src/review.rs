//! `akron review` (TKI-63): a deterministic evidence surface for reviewing
//! a diff range. For each changed symbol: where it sits (module/dir), the
//! nearest existing implementations (embedding-ranked, annotated with
//! deterministic structure/vocabulary cosines only), the file's co-change
//! partners outside the diff ("also review these"), and pattern prevalence.
//! Facts only — no verdicts, no scores beside code.
//!
//! **Unit of review**: the branch-new/changed symbols vs a base ref —
//! exactly `branch.rs`'s changed-symbol rule (content-anchored: a symbol
//! counts iff its file is in the branch's changed set AND its Merkle root
//! is absent from the base version's root set), the same rule
//! `explore::state_from` marks branch-new points with. `branch_new_ids`
//! below is that one-line rule, re-derived rather than imported (branch.rs
//! computes `changed_files`/`base_roots`, not the resulting symbol ids —
//! `explore.rs` folds the rule into its own private `state_from`, so
//! there's nothing importable; a one-line predicate is this codebase's own
//! precedent for such small duplication, see `explain.rs`'s `base_name` doc
//! comment).
//!
//! **Pure core / impure shell.** [`assemble`] takes an already-computed
//! [`Analysis`], [`coupling::CouplingReport`], file-doc list, and an
//! *optional* pre-computed embedding map, and does everything else
//! (ranking, co-change lookups, prevalence) as pure data transforms — no
//! model, no git, no filesystem. That's what makes it testable with
//! synthetic embeddings the way `explore::state_from` is (see
//! `tests/explore.rs`'s `synthetic_embeddings`), and what makes the
//! `--no-default-features` degrade a one-line decision at the call site
//! (`embeddings: None`) rather than a second code path through the ranking
//! logic. [`review`] is the impure shell: git IO (branch resolution,
//! operational-error checks), the scan, `coupling::mine`'s git walk, and —
//! only behind the `semantic` feature — the embedding model.
//!
//! **Exit discipline** (enforced by `main.rs`, not here): "not a git
//! repository" and "no such base ref" are operational errors (`Err`, exit
//! 2); sitting on the base branch, a detached HEAD without `--base`, or any
//! other reason `branch::detect` degrades silently all mean "nothing to
//! review" — `Ok` with `changed_symbols: 0`, exit 0. `branch::detect`
//! collapses several of those into one `None`, so `review` runs its own
//! tiny pre-checks (`require_git_repo`, `require_base_resolves`) first to
//! pull the two real error cases out before falling back to "no changes".
//!
//! **Nearest existing** (TKI-53's card rule, called rather than
//! duplicated): candidates are existing, non-test, non-changed symbols;
//! each ranked hit carries Channel-A (WL) and Channel-B (tf-idf) cosine to
//! the changed symbol — never the embedding score itself (DESIGN.md §1.2:
//! a model may rank, the deterministic channels are the only source of the
//! numbers shown).
//!
//! **Also review** (`coupling::mine`): top-3 confidence-ranked co-change
//! partners of the symbol's file, excluding any file already in the diff.
//! Directional confidence — P(partner changes | this file changes) —
//! matches the human line's "38 of 42 commits" phrasing. A file whose own
//! `CouplingSignal` is `InsufficientHistory` (or that `coupling::mine`
//! never saw at all — an uncommitted new file) reports no partners rather
//! than a number built on too little history.
//!
//! **Prevalence**: the module-docstring fact (`explore::has_module_docstring`
//! via `explore::scan_file_docs`) and the symbol count, both scoped to the
//! changed symbol's own directory (one level, not depth-adjusted — matches
//! `explain.rs`'s `dir_of`, re-derived here for the same "small duplication"
//! reason as `branch_new_ids`).

use crate::run::Analysis;
use crate::types::SymbolPrint;
use crate::{branch, coupling, explore, fingerprint, find, run};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::process::Command;

pub const SCHEMA: &str = "akron.review/v1";

/// One embedding-ranked existing symbol, annotated with the deterministic
/// channels only.
#[derive(Clone)]
pub struct NearestHit {
    pub qname: String,
    pub file: String,
    pub line: usize,
    pub a_cos: f32,
    pub b_cos: f32,
}

/// One co-change partner outside the diff.
pub struct CoChangePartner {
    pub file: String,
    pub shared_revs: u32,
    /// This symbol's own file's total counted revisions — the denominator
    /// behind `confidence` and the "N of M commits" text line.
    pub file_revs: u32,
    /// P(partner changes | this file changes) = shared_revs / file_revs.
    pub confidence: f64,
}

pub struct Prevalence {
    pub dir: String,
    pub file_has_docstring: bool,
    pub dir_docstring_files: usize,
    pub dir_total_files: usize,
    pub dir_symbol_count: usize,
}

pub struct SymbolReview {
    pub qname: String,
    pub file: String,
    pub line: usize,
    pub dir: String,
    pub is_test: bool,
    /// Empty when `semantic_available` is false, or when the repo has no
    /// eligible candidates.
    pub nearest_existing: Vec<NearestHit>,
    pub also_review: Vec<CoChangePartner>,
    pub prevalence: Prevalence,
}

pub struct ReviewReport {
    pub branch: String,
    pub base: String,
    pub changed_files: usize,
    pub changed_symbols: usize,
    /// False under a `--no-default-features` build: `nearest_existing` is
    /// empty on every symbol, honestly, rather than silently omitted.
    pub semantic_available: bool,
    /// Union of every symbol's `also_review` file names, deduped and sorted.
    pub also_review: Vec<String>,
    /// Sorted by (file, line).
    pub symbols: Vec<SymbolReview>,
}

// ── pure core ──

/// `file`'s containing directory, one level (mirrors `explain.rs`'s private
/// `dir_of`): `.` for a root-level file.
fn dir_of(file: &str) -> &str {
    file.rsplit_once('/').map(|(d, _)| d).unwrap_or(".")
}

/// The branch-new/changed rule (see module doc): a symbol counts iff its
/// file is in the branch's changed set AND its Merkle root is absent from
/// the base version's root set.
fn branch_new_ids(info: &branch::BranchInfo, symbols: &[SymbolPrint]) -> HashSet<usize> {
    (0..symbols.len())
        .filter(|&i| {
            info.changed_files.contains(&symbols[i].sym.file)
                && !info.base_roots.contains(&symbols[i].merkle_root)
        })
        .collect()
}

/// Top-3 embedding-ranked existing (non-test, non-changed) symbols per
/// changed symbol, annotated with Channel-A/B cosines. Pure and model-free:
/// ranking itself is a plain dot product over already-computed vectors
/// (`find::rank`); ids missing from `embeddings` (a candidate the caller
/// chose not to embed) are silently excluded rather than erroring, so a
/// partial map still degrades sanely.
fn nearest_existing_for(
    analysis: &Analysis,
    changed: &HashSet<usize>,
    embeddings: &HashMap<usize, Vec<f32>>,
) -> HashMap<usize, Vec<NearestHit>> {
    let symbols = &analysis.scanned.symbols;
    let candidates: Vec<(usize, &[f32])> = (0..symbols.len())
        .filter(|i| !symbols[*i].is_test && !changed.contains(i))
        .filter_map(|i| embeddings.get(&i).map(|v| (i, v.as_slice())))
        .collect();

    let mut out = HashMap::with_capacity(changed.len());
    for &id in changed {
        let Some(query) = embeddings.get(&id) else { continue };
        let ranked = find::rank(query, &candidates, 3);
        let hits = ranked
            .iter()
            .map(|&(i, _)| NearestHit {
                qname: symbols[i].sym.qname.clone(),
                file: symbols[i].sym.file.clone(),
                line: symbols[i].sym.line,
                a_cos: fingerprint::cosine(&symbols[i].wl, &symbols[id].wl),
                b_cos: analysis.vocab.cosine_between(i as u32, id as u32),
            })
            .collect();
        out.insert(id, hits);
    }
    out
}

/// Top-3 confidence-ranked co-change partners of `file`, excluding anything
/// already in `diff_files`. Empty when `coupling::mine` never saw `file`
/// (e.g. an uncommitted new file) or reports `InsufficientHistory` for it —
/// both mean "not enough history to say anything reliable", never a
/// fabricated number.
fn co_change_partners(
    report: &coupling::CouplingReport,
    file: &str,
    diff_files: &HashSet<String>,
) -> Vec<CoChangePartner> {
    let Some(fh) = report.files.iter().find(|f| f.path == file) else {
        return Vec::new();
    };
    if fh.signal == coupling::CouplingSignal::InsufficientHistory {
        return Vec::new();
    }
    let mut out: Vec<CoChangePartner> = report
        .pairs
        .iter()
        .filter_map(|p| {
            let (partner, confidence) = if p.a == file {
                (&p.b, p.confidence_ab)
            } else if p.b == file {
                (&p.a, p.confidence_ba)
            } else {
                return None;
            };
            if diff_files.contains(partner) {
                return None;
            }
            Some(CoChangePartner {
                file: partner.clone(),
                shared_revs: p.shared_revs,
                file_revs: fh.revs,
                confidence,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| b.shared_revs.cmp(&a.shared_revs))
            .then_with(|| a.file.cmp(&b.file))
    });
    out.truncate(3);
    out
}

/// Module-docstring + symbol-count prevalence, scoped to `dir` (see module
/// doc). `file_docs` is `explore::scan_file_docs`'s output; `symbols` is the
/// same scan `review` ran everything else over.
fn prevalence_for(file_docs: &[explore::FileDoc], symbols: &[SymbolPrint], file: &str, dir: &str) -> Prevalence {
    let file_has_docstring = file_docs.iter().find(|d| d.file == file).is_some_and(|d| d.has_docstring);
    let dir_total_files = file_docs.iter().filter(|d| dir_of(&d.file) == dir).count();
    let dir_docstring_files =
        file_docs.iter().filter(|d| dir_of(&d.file) == dir && d.has_docstring).count();
    let dir_symbol_count = symbols.iter().filter(|s| dir_of(&s.sym.file) == dir).count();
    Prevalence {
        dir: dir.to_string(),
        file_has_docstring,
        dir_docstring_files,
        dir_total_files,
        dir_symbol_count,
    }
}

/// Assemble the report from already-computed inputs — no IO, no model.
/// `embeddings`: `None` degrades `nearest_existing` to empty on every
/// symbol (the `--no-default-features` / feature-disabled path);
/// `Some(map)` need only carry vectors for the ids `review` chose to embed
/// (changed symbols + non-test/non-changed candidates) — see
/// `nearest_existing_for`.
pub fn assemble(
    info: &branch::BranchInfo,
    analysis: &Analysis,
    embeddings: Option<&HashMap<usize, Vec<f32>>>,
    coupling_report: &coupling::CouplingReport,
    file_docs: &[explore::FileDoc],
) -> ReviewReport {
    let symbols = &analysis.scanned.symbols;
    let changed = branch_new_ids(info, symbols);
    let mut changed_ids: Vec<usize> = changed.iter().copied().collect();
    changed_ids.sort_by_key(|&i| (symbols[i].sym.file.clone(), symbols[i].sym.line));

    let mut nearest = embeddings
        .map(|emb| nearest_existing_for(analysis, &changed, emb))
        .unwrap_or_default();

    let mut also_review_union: BTreeSet<String> = BTreeSet::new();
    let mut out_symbols = Vec::with_capacity(changed_ids.len());
    for &id in &changed_ids {
        let s = &symbols[id];
        let dir = dir_of(&s.sym.file).to_string();
        let also = co_change_partners(coupling_report, &s.sym.file, &info.changed_files);
        for p in &also {
            also_review_union.insert(p.file.clone());
        }
        let prevalence = prevalence_for(file_docs, symbols, &s.sym.file, &dir);
        out_symbols.push(SymbolReview {
            qname: s.sym.qname.clone(),
            file: s.sym.file.clone(),
            line: s.sym.line,
            dir,
            is_test: s.is_test,
            nearest_existing: nearest.remove(&id).unwrap_or_default(),
            also_review: also,
            prevalence,
        });
    }

    ReviewReport {
        branch: info.branch.clone(),
        base: info.base.clone(),
        changed_files: info.changed_files.len(),
        changed_symbols: changed_ids.len(),
        semantic_available: embeddings.is_some(),
        also_review: also_review_union.into_iter().collect(),
        symbols: out_symbols,
    }
}

// ── impure shell: git IO, the scan, and (semantic builds) the model ──

/// One git command's success/stdout-trimmed line, `None` on any failure.
/// Small, deliberate duplication of `branch.rs`'s own `git`/`git_line`
/// helpers — see module doc.
fn git_ok(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn require_git_repo(root: &Path) -> Result<()> {
    match git_ok(root, &["rev-parse", "--is-inside-work-tree"]) {
        Some(s) if s == "true" => Ok(()),
        _ => anyhow::bail!("not a git repository: {}", root.display()),
    }
}

fn require_base_resolves(root: &Path, base: &str) -> Result<()> {
    match git_ok(root, &["rev-parse", "--verify", "--quiet", base]) {
        Some(_) => Ok(()),
        None => anyhow::bail!("no such base ref: {base:?}"),
    }
}

/// Best-effort branch name for the "no changes" header — `review` never
/// resolves the full branch context in that path (there's nothing else to
/// compute), so this is the one git call it still makes.
fn current_branch(root: &Path) -> String {
    git_ok(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "HEAD".to_string())
}

/// Real embeddings for `review`'s changed symbols + their ranking
/// candidates, through `find`'s own content-keyed cache (same doc texts,
/// same cache file `explore`/`find` warm) — a small diff never re-embeds
/// symbols whose text hasn't changed, and candidates already cached from a
/// prior `find`/`explore` run on this repo cost nothing here either.
#[cfg(feature = "semantic")]
fn compute_embeddings(
    root: &Path,
    info: &branch::BranchInfo,
    analysis: &Analysis,
) -> Result<HashMap<usize, Vec<f32>>> {
    let symbols = &analysis.scanned.symbols;
    let changed = branch_new_ids(info, symbols);
    let candidate_ids = (0..symbols.len()).filter(|i| !symbols[*i].is_test && !changed.contains(i));
    let mut need_ids: Vec<usize> = changed.iter().copied().chain(candidate_ids).collect();
    need_ids.sort_unstable();
    need_ids.dedup();
    if need_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let texts = find::doc_texts(root, symbols, &need_ids);
    let keys: Vec<String> = texts.iter().map(|t| find::doc_key(t)).collect();
    let index_file = find::index_path(root);
    let mut cache = find::EmbCache::load(&index_file);
    let mut embedder = crate::embed::Embedder::load()?;
    if find::embed_missing(&mut embedder, &mut cache, &texts, &keys)? {
        cache.save(&index_file)?;
    }
    Ok(need_ids.iter().zip(&keys).map(|(&i, k)| (i, cache.map[k].clone())).collect())
}

/// `akron review <path> [--base <ref>]`: resolve branch context, scan,
/// mine change coupling, and (semantic builds) rank nearest-existing —
/// then hand everything to [`assemble`]. `Err` only for the two genuine
/// operational errors (not a repo, bad `--base`); every other reason the
/// branch context is absent degrades to a zero-symbol report (see module
/// doc's exit discipline).
pub fn review(root: &Path, base: Option<&str>) -> Result<ReviewReport> {
    require_git_repo(root)?;
    if let Some(b) = base {
        require_base_resolves(root, b)?;
    }
    let semantic_available = cfg!(feature = "semantic");

    let Some(info) = branch::detect(root, base) else {
        return Ok(ReviewReport {
            branch: current_branch(root),
            base: base.map(str::to_string).unwrap_or_else(|| "(repo default)".to_string()),
            changed_files: 0,
            changed_symbols: 0,
            semantic_available,
            also_review: Vec::new(),
            symbols: Vec::new(),
        });
    };

    let cfg = explore::explore_cfg();
    let analysis = run::analyze(root, &cfg);
    let coupling_report =
        coupling::mine(root, &coupling::CouplingConfig::default()).context("mining change coupling")?;
    let file_docs = explore::scan_file_docs(root);

    #[cfg(feature = "semantic")]
    let embeddings = Some(compute_embeddings(root, &info, &analysis)?);
    #[cfg(not(feature = "semantic"))]
    let embeddings: Option<HashMap<usize, Vec<f32>>> = None;

    Ok(assemble(&info, &analysis, embeddings.as_ref(), &coupling_report, &file_docs))
}

// ── rendering ──

/// Tool-voice text: a header line, the diff-wide "also review" union, then
/// one block per changed symbol — counts and names only, no judgment words.
pub fn render_text(report: &ReviewReport) {
    if report.changed_symbols == 0 {
        println!("review vs {} \u{2014} no changed symbols", report.base);
        return;
    }
    println!(
        "review vs {} \u{2014} {} files, {} symbols changed",
        report.base, report.changed_files, report.changed_symbols
    );
    if !report.also_review.is_empty() {
        println!("also review: {}", report.also_review.join(", "));
    }
    if !report.semantic_available {
        println!("nearest existing unavailable \u{2014} built without the semantic feature");
    }
    for s in &report.symbols {
        println!();
        println!(
            "{}:{}  {}  ({}, {})",
            s.file,
            s.line,
            s.qname,
            if s.is_test { "test" } else { "production" },
            s.dir
        );
        if report.semantic_available {
            if s.nearest_existing.is_empty() {
                println!("  nearest existing  none");
            } else {
                let parts: Vec<String> = s
                    .nearest_existing
                    .iter()
                    .map(|h| {
                        format!(
                            "{} {}:{} (structure {:.2}, vocab {:.2})",
                            h.qname, h.file, h.line, h.a_cos, h.b_cos
                        )
                    })
                    .collect();
                println!("  nearest existing  {}", parts.join(", "));
            }
        }
        if s.also_review.is_empty() {
            println!("  also review       none");
        } else {
            let parts: Vec<String> = s
                .also_review
                .iter()
                .map(|p| format!("changes with {} in {} of {} commits", p.file, p.shared_revs, p.file_revs))
                .collect();
            println!("  also review       {}", parts.join(", "));
        }
        let p = &s.prevalence;
        println!(
            "  prevalence        module docstring: {}; {} of {} files documented in {}; {} symbols in {}",
            if p.file_has_docstring { "yes" } else { "no" },
            p.dir_docstring_files,
            p.dir_total_files,
            p.dir,
            p.dir_symbol_count,
            p.dir
        );
    }
}

/// Versioned JSON, stable field names, sorted arrays. Built through
/// `json!()` (a `serde_json::Value::Object` is a `BTreeMap` in this crate's
/// build — no `preserve_order` feature — so keys serialize alphabetically
/// besides), matching `find::render_json`'s own pattern: byte-identical
/// across two runs on identical repo state (see `tests/review.rs`).
pub fn render_json(report: &ReviewReport) -> Value {
    json!({
        "schema": SCHEMA,
        "branch": report.branch,
        "base": report.base,
        "changed_files": report.changed_files,
        "changed_symbols": report.changed_symbols,
        "semantic_available": report.semantic_available,
        "also_review": report.also_review,
        "symbols": report.symbols.iter().map(|s| json!({
            "qname": s.qname,
            "file": s.file,
            "line": s.line,
            "dir": s.dir,
            "is_test": s.is_test,
            "nearest_existing": s.nearest_existing.iter().map(|h| json!({
                "qname": h.qname,
                "file": h.file,
                "line": h.line,
                "a_cos": h.a_cos,
                "b_cos": h.b_cos,
            })).collect::<Vec<_>>(),
            "also_review": s.also_review.iter().map(|p| json!({
                "file": p.file,
                "shared_revs": p.shared_revs,
                "file_revs": p.file_revs,
                "confidence": p.confidence,
            })).collect::<Vec<_>>(),
            "prevalence": {
                "dir": s.prevalence.dir,
                "file_has_docstring": s.prevalence.file_has_docstring,
                "dir_docstring_files": s.prevalence.dir_docstring_files,
                "dir_total_files": s.prevalence.dir_total_files,
                "dir_symbol_count": s.prevalence.dir_symbol_count,
            },
        })).collect::<Vec<_>>(),
    })
}
