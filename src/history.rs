//! Channel C (DESIGN.md §2.4): the git time channel. One first-parent history
//! walk dates every current path; symbols inherit their file's dates. Per
//! cluster, the member-date distribution yields an activity class — the input
//! to the drifting / deprecated / dead queries.
//!
//! v0 dating is **file-level** (see DESIGN.md §2.4 "Approximations"): all
//! symbols in a file share that file's first-seen / last-touched. Honest and
//! deterministic; symbol-precise dating is a later refinement.

use crate::types::{SymbolDates, SymbolPrint};
use gix::bstr::ByteSlice;
use std::collections::HashMap;
use std::path::Path;

/// Local unix seconds: committer time plus its timezone offset, so a formatted
/// date matches what `git log` shows in the committer's own timezone.
pub type Secs = i64;

const DAY: Secs = 86_400;

/// A cluster is **dead** when nothing in it was touched within this many days
/// of the repo's newest commit, and **growing** when a member was first seen
/// within the growth window. Anchored to the repo head — not wall-clock now —
/// so the classification is a pure function of repo state (DESIGN.md §1): the
/// same commit graph always classifies the same way, whenever it is scanned.
const DEAD_AFTER_DAYS: Secs = 180;
const GROWING_WITHIN_DAYS: Secs = 90;

struct FileDates {
    first_seen: Secs,
    last_touched: Secs,
}

pub struct History {
    file_dates: HashMap<String, FileDates>,
    /// Newest commit time in the walked history: the "now" that all recency is
    /// measured against.
    pub anchor: Secs,
}

impl History {
    pub fn dates_for(&self, file: &str) -> Option<SymbolDates> {
        self.file_dates.get(file).map(|f| SymbolDates {
            first_seen: f.first_seen,
            last_touched: f.last_touched,
        })
    }
}

/// Walk the repo's first-parent history once and date every path it touches.
/// Returns `None` when `root` is not a git repository, so the scan degrades to
/// no dating and callers surface an explicit "no git history" note rather than
/// a silent absence.
///
/// Approximation (documented in DESIGN.md §2.4): rename tracking is disabled,
/// so a path's `first_seen` is the earliest commit touching *that current
/// path* — history before a rename is attributed to the old path and not
/// followed (matching `git log` without `--follow`).
pub fn walk(root: &Path) -> Option<History> {
    let repo = gix::open(root).ok()?;
    let head = repo.head_commit().ok()?;

    // First-parent chain, newest first. `parent_ids().next()` is the first
    // parent; merge second-parents are not followed (first-parent walk).
    let mut chain = vec![head];
    loop {
        let parent = chain.last().unwrap().parent_ids().next();
        match parent {
            Some(id) => chain.push(repo.find_commit(id.detach()).ok()?),
            None => break,
        }
    }

    let anchor = commit_secs(&chain[0])?;
    let mut file_dates: HashMap<String, FileDates> = HashMap::new();
    for i in 0..chain.len() {
        let secs = commit_secs(&chain[i])?;
        let new_tree = chain[i].tree().ok()?;
        let old_tree = match chain.get(i + 1) {
            Some(parent) => parent.tree().ok()?,
            None => repo.empty_tree(), // root commit: diff against nothing
        };
        let mut platform = old_tree.changes().ok()?;
        // Track paths; disable rename detection so the dates are a pure
        // function of the trees, independent of the caller's git config.
        platform.options(|o| {
            o.track_path().track_rewrites(None);
        });
        platform
            .for_each_to_obtain_tree(&new_tree, |change| {
                if let Ok(path) = change.location().to_str() {
                    record(&mut file_dates, path, secs);
                }
                Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue(()))
            })
            .ok()?;
    }
    Some(History { file_dates, anchor })
}

fn commit_secs(c: &gix::Commit) -> Option<Secs> {
    let t = c.time().ok()?;
    Some(t.seconds + t.offset as i64)
}

fn record(map: &mut HashMap<String, FileDates>, path: &str, secs: Secs) {
    map.entry(path.to_string())
        .and_modify(|d| {
            d.first_seen = d.first_seen.min(secs);
            d.last_touched = d.last_touched.max(secs);
        })
        .or_insert(FileDates {
            first_seen: secs,
            last_touched: secs,
        });
}

// ── per-cluster aggregation: adoption curve → activity class ──

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Activity {
    Growing,
    Flat,
    Dead,
}

impl Activity {
    pub fn label(self) -> &'static str {
        match self {
            Activity::Growing => "growing",
            Activity::Flat => "flat",
            Activity::Dead => "dead",
        }
    }
}

pub struct ClusterDates {
    /// Span start: oldest member `first_seen`.
    pub first_seen: Secs,
    /// Span end: newest member `last_touched`.
    pub last_touched: Secs,
    pub activity: Activity,
}

/// Classify a cluster from its date extremes, relative to the repo `anchor`.
///
/// Evaluated in this order:
/// 1. **Dead** — the newest touch is older than `DEAD_AFTER_DAYS`: no member
///    has moved recently, so staleness dominates.
/// 2. **Growing** — otherwise, if the newest member was born within
///    `GROWING_WITHIN_DAYS`: the cluster gained a fresh member recently.
/// 3. **Flat** — touched recently, but with no recent new members.
pub fn classify(newest_touch: Secs, newest_birth: Secs, anchor: Secs) -> Activity {
    if anchor - newest_touch > DEAD_AFTER_DAYS * DAY {
        Activity::Dead
    } else if anchor - newest_birth <= GROWING_WITHIN_DAYS * DAY {
        Activity::Growing
    } else {
        Activity::Flat
    }
}

/// Aggregate a cluster's member dates into a span + activity class. `None` when
/// no member carries dating (path outside git history).
pub fn cluster_dating(
    symbols: &[SymbolPrint],
    members: &[u32],
    anchor: Secs,
) -> Option<ClusterDates> {
    let mut first = Secs::MAX;
    let mut last = Secs::MIN;
    let mut newest_birth = Secs::MIN;
    let mut any = false;
    for &m in members {
        if let Some(d) = &symbols[m as usize].dating {
            any = true;
            first = first.min(d.first_seen);
            last = last.max(d.last_touched);
            newest_birth = newest_birth.max(d.first_seen);
        }
    }
    if !any {
        return None;
    }
    Some(ClusterDates {
        first_seen: first,
        last_touched: last,
        activity: classify(last, newest_birth, anchor),
    })
}

/// Format local unix seconds as `YYYY-MM-DD` (Hinnant's civil-from-days).
pub fn fmt_date(secs: Secs) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(DAY));
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANCHOR: Secs = 1_700_000_000; // arbitrary fixed instant

    #[test]
    fn stale_touch_is_dead() {
        let touch = ANCHOR - 200 * DAY;
        assert_eq!(classify(touch, touch, ANCHOR), Activity::Dead);
    }

    #[test]
    fn recent_new_member_is_growing() {
        let t = ANCHOR - 10 * DAY;
        assert_eq!(classify(t, t, ANCHOR), Activity::Growing);
    }

    #[test]
    fn maintained_without_new_members_is_flat() {
        // Touched 100d ago (not dead) but newest birth 100d ago (> growth 90d).
        let touch = ANCHOR - 100 * DAY;
        let birth = ANCHOR - 100 * DAY;
        assert_eq!(classify(touch, birth, ANCHOR), Activity::Flat);
    }

    #[test]
    fn dead_boundary_is_inclusive_of_180() {
        // Exactly 180d is not yet dead (rule uses strict >).
        let touch = ANCHOR - 180 * DAY;
        assert_ne!(classify(touch, touch, ANCHOR), Activity::Dead);
        // One day past the window is dead.
        let touch = ANCHOR - 181 * DAY;
        assert_eq!(classify(touch, touch, ANCHOR), Activity::Dead);
    }

    #[test]
    fn growth_boundary_is_inclusive_of_90() {
        // Touched recently, newest birth exactly 90d ago → still growing.
        let touch = ANCHOR - 5 * DAY;
        let birth = ANCHOR - 90 * DAY;
        assert_eq!(classify(touch, birth, ANCHOR), Activity::Growing);
        // 91d ago → flat.
        let birth = ANCHOR - 91 * DAY;
        assert_eq!(classify(touch, birth, ANCHOR), Activity::Flat);
    }

    #[test]
    fn civil_date_epoch_and_known_dates() {
        assert_eq!(fmt_date(0), "1970-01-01");
        assert_eq!(fmt_date(1_700_000_000), "2023-11-14");
    }
}
