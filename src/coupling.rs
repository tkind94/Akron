//! Change-coupling mining (TKI-62): file-level co-change from git history.
//! Confidence = shared_revs / revs(entity); degree = shared / avg-revs
//! (code-maat form). Noise filters: changeset-size cap, path excludes,
//! min-revision floors. Files below the history gate report "insufficient
//! history" rather than a false-precise number. Deterministic: sorted
//! output, recomputed at HEAD.
//!
//! Transactions default to one-commit-one-transaction; ticket grouping
//! (`CouplingConfig::group_by_ticket`) merges commits that share a ticket
//! token (`ABC-123` or `#123`) and author into one logical changeset.
//! First-parent walk only (matches `history.rs`); merge commits never form
//! a transaction of their own, so a file introduced solely by a merge is
//! `revs == 0` (reports `InsufficientHistory`, never a fabricated number).
//!
//! Scope: both per-file stats and pairs are reported only for paths present
//! in the HEAD tree — "current file size" (the churn denominator) is
//! undefined for a deleted path, so deleted paths are dropped rather than
//! carried as a half-populated row.

use crate::history::Secs;
use anyhow::{Context, Result};
use gix::bstr::ByteSlice;
use gix::ObjectId;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

const DAY: Secs = 86_400;

#[derive(Clone, Copy, Debug)]
pub struct CouplingConfig {
    /// Skip transactions touching more than this many (post-filter) files.
    /// Literature range 8-30 (code-maat / Nagappan-Ball); 30 is the
    /// permissive end, so a real-but-wide refactor still counts.
    pub max_changeset_size: usize,
    /// A file below this transaction count reports `CouplingSignal::InsufficientHistory`.
    pub min_revs: u32,
    /// A pair is only reported when `shared_revs >= min_shared_revs`.
    pub min_shared_revs: u32,
    /// A pair is only reported when `degree >= min_degree_pct` (0-100).
    pub min_degree_pct: f64,
    /// Merge commits sharing a ticket token (`ABC-123` / `#123`) and author
    /// into one logical changeset, instead of one-commit-one-transaction.
    pub group_by_ticket: bool,
    /// Half-life, in days, for the recency weight behind `recent_confidence`.
    pub half_life_days: f64,
}

impl Default for CouplingConfig {
    fn default() -> Self {
        CouplingConfig {
            max_changeset_size: 30,
            min_revs: 10,
            min_shared_revs: 5,
            min_degree_pct: 30.0,
            group_by_ticket: false,
            half_life_days: 182.5, // ~6 months
        }
    }
}

/// Honesty gate (as a type rather than a convention): a file with too
/// little history never produces a churn number, it produces this variant
/// instead.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum CouplingSignal {
    Ready {
        /// Total lines changed across counted transactions, divided by the
        /// current line count (Nagappan/Ball size-normalization).
        relative_churn: f64,
    },
    InsufficientHistory,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FileHistory {
    pub path: String,
    /// Number of counted transactions (post noise-filter, non-merge,
    /// under the changeset cap) touching this path.
    pub revs: u32,
    /// Local unix seconds of the last counted touch; `0` when `revs == 0`.
    pub last_touched: Secs,
    pub signal: CouplingSignal,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CoupledPair {
    /// Lexicographically the smaller of the two paths.
    pub a: String,
    pub b: String,
    pub shared_revs: u32,
    /// P(b changes | a changes) = shared_revs / revs(a). Directional.
    pub confidence_ab: f64,
    /// P(a changes | b changes) = shared_revs / revs(b). Directional.
    pub confidence_ba: f64,
    /// code-maat form: shared / avg(revs(a), revs(b)) * 100. Symmetric,
    /// for display.
    pub degree: f64,
    /// Recency-weighted analog of `degree` (half-life `half_life_days`):
    /// weighted_shared / avg(weighted_revs(a), weighted_revs(b)) * 100.
    /// Secondary signal; `degree` above is primary for v1.
    pub recent_confidence: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CouplingReport {
    /// Sorted by `path` ascending.
    pub files: Vec<FileHistory>,
    /// Sorted by `max(confidence_ab, confidence_ba)` descending, then by
    /// `(a, b)` ascending.
    pub pairs: Vec<CoupledPair>,
}

/// Mine change coupling from `repo`'s first-parent history, evaluated at
/// HEAD. Deterministic: identical repo state always yields byte-identical
/// output (see `tests/coupling.rs`).
pub fn mine(repo: &Path, cfg: &CouplingConfig) -> Result<CouplingReport> {
    let repo = gix::open(repo).context("coupling::mine: not a git repository")?;
    let head = repo.head_commit().context("coupling::mine: no HEAD commit")?;

    // First-parent chain, newest first — same walk shape as history.rs.
    let mut chain = vec![head];
    loop {
        let parent = chain.last().unwrap().parent_ids().next();
        match parent {
            Some(id) => chain.push(repo.find_commit(id.detach())?),
            None => break,
        }
    }
    let anchor = commit_secs(&chain[0]).context("coupling::mine: HEAD has no commit time")?;

    let (current_paths, current_size) = current_tree_state(&repo, &chain[0])?;

    // ── phase 1: per-commit touched-file lists (cheap; no blob diffing yet) ──
    let raw = build_raw_transactions(&repo, &chain, &current_paths)?;

    // ── phase 2: group (optional), cap-filter, then accumulate ──
    let transactions = if cfg.group_by_ticket {
        group_by_ticket(raw)
    } else {
        raw.into_iter().map(Transaction::from_raw).collect()
    };

    let paths_sorted: Vec<&String> = {
        let mut v: Vec<&String> = current_paths.iter().collect();
        v.sort();
        v
    };
    let path_id: HashMap<&str, u32> =
        paths_sorted.iter().enumerate().map(|(i, p)| (p.as_str(), i as u32)).collect();

    let mut revs = vec![0u32; paths_sorted.len()];
    let mut last_touched = vec![0 as Secs; paths_sorted.len()];
    let mut weighted_revs = vec![0f64; paths_sorted.len()];
    let mut churn = vec![0u32; paths_sorted.len()];
    let mut pairs: HashMap<(u32, u32), PairAgg> = HashMap::new();

    let half_life_secs = (cfg.half_life_days * DAY as f64).max(1.0);

    for txn in &transactions {
        let mut touched: Vec<u32> =
            txn.files.keys().filter_map(|p| path_id.get(p.as_str()).copied()).collect();
        if touched.len() > cfg.max_changeset_size {
            continue;
        }
        touched.sort_unstable();
        touched.dedup();
        let weight = 0.5f64.powf((anchor - txn.secs) as f64 / half_life_secs);

        for &id in &touched {
            revs[id as usize] += 1;
            last_touched[id as usize] = last_touched[id as usize].max(txn.secs);
            weighted_revs[id as usize] += weight;
        }
        for i in 0..touched.len() {
            for j in (i + 1)..touched.len() {
                let key = (touched[i], touched[j]);
                let agg = pairs.entry(key).or_default();
                agg.shared += 1;
                agg.weighted_shared += weight;
            }
        }

        // Churn: every individual blob transition counts, even when several
        // land in one grouped transaction (churn is additive, not
        // transaction-gated).
        for (path, edits) in &txn.files {
            let Some(&id) = path_id.get(path.as_str()) else { continue };
            for edit in edits {
                let lines = diff_line_count(&repo, edit.prev_id, edit.id);
                churn[id as usize] += lines;
            }
        }
    }

    let files: Vec<FileHistory> = paths_sorted
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let signal = if revs[i] < cfg.min_revs {
                CouplingSignal::InsufficientHistory
            } else {
                let size = current_size.get(path.as_str()).copied().unwrap_or(0);
                let relative_churn = if size == 0 { 0.0 } else { churn[i] as f64 / size as f64 };
                CouplingSignal::Ready { relative_churn }
            };
            FileHistory { path: (*path).clone(), revs: revs[i], last_touched: last_touched[i], signal }
        })
        .collect();

    let mut out_pairs: Vec<CoupledPair> = pairs
        .into_iter()
        .filter_map(|((ia, ib), agg)| {
            let ra = revs[ia as usize];
            let rb = revs[ib as usize];
            if ra == 0 || rb == 0 {
                return None;
            }
            let degree = agg.shared as f64 / ((ra as f64 + rb as f64) / 2.0) * 100.0;
            if agg.shared < cfg.min_shared_revs || degree < cfg.min_degree_pct {
                return None;
            }
            let wra = weighted_revs[ia as usize];
            let wrb = weighted_revs[ib as usize];
            let recent_confidence =
                if wra + wrb == 0.0 { 0.0 } else { agg.weighted_shared / ((wra + wrb) / 2.0) * 100.0 };
            Some(CoupledPair {
                a: paths_sorted[ia as usize].clone(),
                b: paths_sorted[ib as usize].clone(),
                shared_revs: agg.shared,
                confidence_ab: agg.shared as f64 / ra as f64,
                confidence_ba: agg.shared as f64 / rb as f64,
                degree,
                recent_confidence,
            })
        })
        .collect();

    out_pairs.sort_by(|x, y| {
        let cx = x.confidence_ab.max(x.confidence_ba);
        let cy = y.confidence_ab.max(y.confidence_ba);
        cy.partial_cmp(&cx)
            .unwrap()
            .then_with(|| (x.a.as_str(), x.b.as_str()).cmp(&(y.a.as_str(), y.b.as_str())))
    });

    Ok(CouplingReport { files, pairs: out_pairs })
}

#[derive(Default)]
struct PairAgg {
    shared: u32,
    weighted_shared: f64,
}

fn commit_secs(c: &gix::Commit) -> Option<Secs> {
    let t = c.time().ok()?;
    Some(t.seconds + t.offset as i64)
}

/// Paths present at HEAD (blob entries only), and their current line count
/// — the churn denominator. Deleted paths never make it into either map, so
/// they never enter the report (see module doc: scope is current-tree files).
fn current_tree_state(
    repo: &gix::Repository,
    head: &gix::Commit,
) -> Result<(HashSet<String>, HashMap<String, u32>)> {
    let entries = head.tree()?.traverse().breadthfirst.files()?;
    let mut paths = HashSet::with_capacity(entries.len());
    let mut sizes = HashMap::with_capacity(entries.len());
    for e in entries {
        if !e.mode.is_blob() {
            continue;
        }
        let Ok(path) = e.filepath.to_str() else { continue };
        if is_noise_path(path) {
            continue;
        }
        let path = path.to_string();
        if let Ok(blob) = repo.find_blob(e.oid) {
            if !looks_binary(&blob.data) {
                sizes.insert(path.clone(), count_lines(&blob.data));
            }
        }
        paths.insert(path);
    }
    Ok((paths, sizes))
}

/// One blob transition for a single path within a single commit step.
struct Edit {
    prev_id: Option<ObjectId>,
    id: Option<ObjectId>,
}

struct RawTransaction {
    secs: Secs,
    author: String,
    ticket: Option<String>,
    /// path -> the (usually one) edits it received in this commit.
    files: HashMap<String, Vec<Edit>>,
}

/// One non-merge commit is one raw transaction; noise-filtered paths and
/// paths no longer at HEAD are dropped before this returns ("noise filters
/// apply before counting").
fn build_raw_transactions(
    repo: &gix::Repository,
    chain: &[gix::Commit],
    current_paths: &HashSet<String>,
) -> Result<Vec<RawTransaction>> {
    let mut out = Vec::new();
    for i in 0..chain.len() {
        let commit = &chain[i];
        if commit.parent_ids().count() > 1 {
            continue; // merge commit: no changeset of its own (first-parent walk).
        }
        let Some(secs) = commit_secs(commit) else { continue };
        let new_tree = commit.tree()?;
        let old_tree = match chain.get(i + 1) {
            Some(parent) => parent.tree()?,
            None => repo.empty_tree(),
        };
        let author = commit.author().map(|a| a.email.to_str_lossy().into_owned()).unwrap_or_default();
        let message = commit.message_raw_sloppy().to_str_lossy();
        let ticket = first_ticket_token(&message);

        let mut files: HashMap<String, Vec<Edit>> = HashMap::new();
        let mut platform = old_tree.changes()?;
        platform.options(|o| {
            o.track_path().track_rewrites(None);
        });
        platform.for_each_to_obtain_tree(&new_tree, |change| {
            let path = change.location().to_str().unwrap_or_default();
            if current_paths.contains(path) && !is_noise_path(path) {
                let edit = match change {
                    gix::object::tree::diff::Change::Addition { id, .. } => {
                        Edit { prev_id: None, id: Some(id.detach()) }
                    }
                    gix::object::tree::diff::Change::Deletion { id, .. } => {
                        Edit { prev_id: Some(id.detach()), id: None }
                    }
                    gix::object::tree::diff::Change::Modification { previous_id, id, .. } => {
                        Edit { prev_id: Some(previous_id.detach()), id: Some(id.detach()) }
                    }
                    gix::object::tree::diff::Change::Rewrite { .. } => {
                        // rewrite tracking is disabled above; unreachable.
                        return Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue(()));
                    }
                };
                files.entry(path.to_string()).or_default().push(edit);
            }
            Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue(()))
        })?;

        if !files.is_empty() {
            out.push(RawTransaction { secs, author, ticket, files });
        }
    }
    Ok(out)
}

struct Transaction {
    secs: Secs,
    files: HashMap<String, Vec<Edit>>,
}

impl Transaction {
    fn from_raw(r: RawTransaction) -> Self {
        Transaction { secs: r.secs, files: r.files }
    }
}

/// Merge raw transactions sharing an `(author, ticket)` key into one logical
/// changeset: the touched-file set is deduped for revs/pairs, `secs` is the
/// newest member's, and every underlying edit is kept (churn is additive
/// across the group, not deduped).
fn group_by_ticket(raw: Vec<RawTransaction>) -> Vec<Transaction> {
    let mut grouped: HashMap<(String, String), Transaction> = HashMap::new();
    let mut standalone = Vec::new();
    for r in raw {
        match r.ticket.clone() {
            Some(ticket) => {
                let key = (r.author.clone(), ticket);
                let entry =
                    grouped.entry(key).or_insert_with(|| Transaction { secs: r.secs, files: HashMap::new() });
                entry.secs = entry.secs.max(r.secs);
                for (path, edits) in r.files {
                    entry.files.entry(path).or_default().extend(edits);
                }
            }
            None => standalone.push(Transaction::from_raw(r)),
        }
    }
    let mut out: Vec<Transaction> = grouped.into_values().collect();
    out.extend(standalone);
    out
}

/// First `[A-Z]+-\d+` or `#\d+` token in a commit message, hand-scanned (no
/// regex dependency). Word-boundary checked so `TKI-62` isn't picked out of
/// a longer alnum run.
fn first_ticket_token(msg: &str) -> Option<String> {
    let b = msg.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n {
        if b[i] == b'#' {
            let start = i;
            let mut j = i + 1;
            while j < n && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > start + 1 {
                return Some(msg[start..j].to_string());
            }
            i += 1;
            continue;
        }
        if b[i].is_ascii_uppercase() {
            let start = i;
            let mut j = i;
            while j < n && b[j].is_ascii_uppercase() {
                j += 1;
            }
            if j < n && b[j] == b'-' {
                let mut k = j + 1;
                while k < n && b[k].is_ascii_digit() {
                    k += 1;
                }
                let before_ok = start == 0 || !b[start - 1].is_ascii_alphanumeric();
                let after_ok = k == n || !b[k].is_ascii_alphanumeric();
                if k > j + 1 && before_ok && after_ok {
                    return Some(msg[start..k].to_string());
                }
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    None
}

/// Lockfiles, and vendored/generated directories: these paths are stripped
/// before any counting.
fn is_noise_path(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    const LOCKFILES: [&str; 3] = ["Cargo.lock", "package-lock.json", "poetry.lock"];
    if LOCKFILES.contains(&file_name) || file_name.ends_with(".lock") {
        return true;
    }
    if file_name.ends_with("_pb2.py") || file_name.contains(".min.") {
        return true;
    }
    const EXCLUDED_DIRS: [&str; 4] = ["vendor", "node_modules", "dist", "target"];
    path.split('/').any(|seg| EXCLUDED_DIRS.contains(&seg))
}

/// git's own heuristic: a NUL byte in the first 8000 bytes marks a blob
/// binary. Binary files are excluded from line-based churn.
fn looks_binary(data: &[u8]) -> bool {
    data.iter().take(8000).any(|&b| b == 0)
}

fn count_lines(data: &[u8]) -> u32 {
    if data.is_empty() {
        return 0;
    }
    let newlines = data.iter().filter(|&&b| b == b'\n').count() as u32;
    if data.last() == Some(&b'\n') {
        newlines
    } else {
        newlines + 1
    }
}

/// Lines added + removed between two optional blobs (`None` = the empty
/// blob, for pure additions/deletions). Binary content contributes 0.
fn diff_line_count(repo: &gix::Repository, prev_id: Option<ObjectId>, id: Option<ObjectId>) -> u32 {
    let load = |oid: Option<ObjectId>| -> Vec<u8> {
        match oid {
            Some(oid) => repo.find_blob(oid).map(|b| b.data.clone()).unwrap_or_default(),
            None => Vec::new(),
        }
    };
    let before = load(prev_id);
    let after = load(id);
    if looks_binary(&before) || looks_binary(&after) {
        return 0;
    }
    let before_text = String::from_utf8_lossy(&before);
    let after_text = String::from_utf8_lossy(&after);
    let input = gix::diff::blob::InternedInput::new(before_text.as_ref(), after_text.as_ref());
    let diff = gix::diff::blob::Diff::compute(gix::diff::blob::Algorithm::Histogram, &input);
    diff.count_additions() + diff.count_removals()
}
