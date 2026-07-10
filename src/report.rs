use crate::cluster::RepeatedFunnel;
use crate::family::{Family, FamilyResult};
use crate::history::{self, History};
use crate::queries::{CompetingResult, DeprecatedResult, RepeatedCluster};
use crate::types::{Config, SymbolPrint};
use serde::Serialize;
use serde_json::json;
use std::path::Path;

pub struct Stats {
    pub files: usize,
    pub symbols: usize,
    pub skipped_small: usize,
    /// Where repeated-shape clustering narrowed (cluster.rs's actual
    /// clustering path) — folds in what used to be a standalone
    /// `oversized_buckets` field.
    pub repeated_funnel: RepeatedFunnel,
}

/// Opts one gated section into the JSON (`--only`). `repeated`/`deprecated`
/// are unconditionally in every report (see `json_report`'s unconditional
/// fields below) so they have no `--only` value — only `families`/
/// `competing` are gated, and only these two are valid here.
#[derive(clap::ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Families,
    Competing,
}

fn family_member_ids(fam: &Family) -> Vec<u32> {
    fam.members.iter().map(|m| m.sym).collect()
}

/// The single source for a finding's ref tag: a 0-based array index becomes
/// `R<n>`/`F<n>`/`C<n>` (1-based) — the position of the entry within its own
/// JSON array, embedded as `"ref"` so a consumer never has to re-derive it
/// positionally (TKI-27).
fn ref_tag(prefix: &str, i: usize) -> String {
    format!("{prefix}{}", i + 1)
}

#[derive(Serialize)]
struct MemberOut<'a> {
    file: &'a str,
    line: usize,
    qname: &'a str,
    nodes: u32,
    is_test: bool,
    first_seen: Option<String>,
    last_touched: Option<String>,
}

fn members_out<'a>(symbols: &'a [SymbolPrint], ids: &[u32]) -> Vec<MemberOut<'a>> {
    ids.iter()
        .map(|&i| {
            let s = &symbols[i as usize];
            MemberOut {
                file: &s.sym.file,
                line: s.sym.line,
                qname: &s.sym.qname,
                nodes: s.node_count,
                is_test: s.is_test,
                first_seen: s.dating.map(|d| history::fmt_date(d.first_seen)),
                last_touched: s.dating.map(|d| history::fmt_date(d.last_touched)),
            }
        })
        .collect()
}

/// Per-cluster date span + activity class, or `null` without git history.
fn dating_out(
    symbols: &[SymbolPrint],
    ids: &[u32],
    history: Option<&History>,
) -> serde_json::Value {
    let Some(h) = history else {
        return serde_json::Value::Null;
    };
    match history::cluster_dating(symbols, ids, h.anchor) {
        Some(cd) => json!({
            "first_seen": history::fmt_date(cd.first_seen),
            "last_touched": history::fmt_date(cd.last_touched),
            "activity": cd.activity.label(),
        }),
        None => serde_json::Value::Null,
    }
}

/// A family member with its drift coordinate (cosine to the family core).
fn family_members_out(symbols: &[SymbolPrint], fam: &Family) -> Vec<serde_json::Value> {
    fam.members
        .iter()
        .map(|m| {
            let s = &symbols[m.sym as usize];
            json!({
                "file": s.sym.file,
                "line": s.sym.line,
                "qname": s.sym.qname,
                "nodes": s.node_count,
                "is_test": s.is_test,
                "is_core": m.is_core,
                "cos_to_core": m.cos_to_core,
                "first_seen": s.dating.map(|d| history::fmt_date(d.first_seen)),
                "last_touched": s.dating.map(|d| history::fmt_date(d.last_touched)),
            })
        })
        .collect()
}

/// Whether `families`/`competing` data is populated in the JSON: only the
/// section named by `--only` is included. `repeated`/`deprecated` are always
/// included — unprompted, `families`/`competing` read as an assertion about
/// how the code relates to itself (a shared vocabulary read as "competing",
/// test scaffolding glued into a "family"), so they stay opt-in (TKI-45).
fn section_included(only: Option<Section>, s: Section) -> bool {
    only == Some(s)
}

#[allow(clippy::too_many_arguments)]
pub fn json_report(
    repo: &Path,
    stats: &Stats,
    symbols: &[SymbolPrint],
    repeated: &[RepeatedCluster],
    families: &FamilyResult,
    competing: &CompetingResult,
    deprecated: &DeprecatedResult,
    history: Option<&History>,
    cfg: &Config,
    only: Option<Section>,
) -> serde_json::Value {
    let include_families = section_included(only, Section::Families);
    let include_competing = section_included(only, Section::Competing);
    json!({
        "schema": "akron.scan/v1",
        "repo": repo.display().to_string(),
        "config": {
            "min_nodes": cfg.min_nodes,
            "wl_iters": cfg.wl_iters,
            "theta_clone": cfg.theta_clone,
            "theta_b": cfg.theta_b,
            "theta_a_low": cfg.theta_a_low,
            "theta_family": cfg.theta_family,
            "theta_b_family": cfg.theta_b_family,
        },
        "stats": {
            "files": stats.files,
            "symbols": stats.symbols,
            "skipped_small": stats.skipped_small,
        },
        "history": history.map(|h| json!({ "anchor": history::fmt_date(h.anchor) })),
        "repeated_funnel": {
            "symbols_considered": stats.repeated_funnel.symbols_considered,
            "oversized_buckets": stats.repeated_funnel.oversized_buckets,
            "candidate_pairs": stats.repeated_funnel.candidate_pairs,
            "survived_guards": stats.repeated_funnel.survived_guards,
            "survived_cosine": stats.repeated_funnel.survived_cosine,
            "clusters_formed": stats.repeated_funnel.clusters_formed,
        },
        "repeated": repeated.iter().enumerate().map(|(n, c)| json!({
            "ref": ref_tag("R", n),
            "members": members_out(symbols, &c.members),
            "n_files": c.n_files,
            "total_nodes": c.total_nodes,
            "all_test": c.all_test,
            "dating": dating_out(symbols, &c.members, history),
        })).collect::<Vec<_>>(),
        "family_funnel": {
            "core_clusters": families.funnel.core_clusters,
            "singletons": families.funnel.singletons,
            "merges": families.funnel.merges,
            "guard_rejected": families.funnel.guard_rejected,
            "role_rejected": families.funnel.role_rejected,
            "families": families.funnel.families,
        },
        "families": if include_families {
            families.families.iter().enumerate().map(|(n, fam)| json!({
                "ref": ref_tag("F", n),
                "core": symbols[fam.core_medoid as usize].sym.qname,
                "sub_clusters": fam.sub_clusters,
                "core_size": fam.core_size,
                "drift_size": fam.members.len() - fam.core_size,
                "n_files": fam.n_files,
                "cos_to_core_min": fam.cos_to_core_min,
                "cos_to_core_max": fam.cos_to_core_max,
                "all_test": fam.all_test,
                "dating": dating_out(symbols, &family_member_ids(fam), history),
                "members": family_members_out(symbols, fam),
            })).collect::<Vec<_>>()
        } else {
            Vec::new()
        },
        "competing_funnel": {
            "vocab_pairs": competing.funnel.vocab_pairs,
            "cross_context": competing.funnel.cross_context,
            "low_shape": competing.funnel.low_shape,
            "vocab_quality": competing.funnel.vocab_quality,
            "call_related": competing.funnel.call_related,
            "chained": competing.funnel.chained,
        },
        "competing": if include_competing {
            competing.groups.iter().enumerate().map(|(n, g)| json!({
                "ref": ref_tag("C", n),
                "members": members_out(symbols, &g.members),
                "b_max": g.b_max,
                "a_at_best": g.a_at_best,
                "shared_terms": g.shared_terms,
            })).collect::<Vec<_>>()
        } else {
            Vec::new()
        },
        "deprecated_funnel": {
            "dated_clusters": deprecated.funnel.dated_clusters,
            "dead_clusters": deprecated.funnel.dead_clusters,
            "growing_clusters": deprecated.funnel.growing_clusters,
            "role_pairs": deprecated.funnel.role_pairs,
            "vocab_matched": deprecated.funnel.vocab_matched,
        },
        "deprecated": deprecated.candidates.iter().map(|c| json!({
            "vocab_cosine": c.vocab_cosine,
            "shared_terms": c.shared_terms,
            "dead": {
                "members": members_out(symbols, &c.dead),
                "first_seen": history::fmt_date(c.dead_dates.first_seen),
                "last_touched": history::fmt_date(c.dead_dates.last_touched),
                "activity": c.dead_dates.activity.label(),
            },
            "growing": {
                "members": members_out(symbols, &c.growing),
                "first_seen": history::fmt_date(c.growing_dates.first_seen),
                "last_touched": history::fmt_date(c.growing_dates.last_touched),
                "activity": c.growing_dates.activity.label(),
            },
        })).collect::<Vec<_>>(),
    })
}
