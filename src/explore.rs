//! `akron explore <path>` (TKI-47): a live, local map of the repo. One scan
//! + one embedding pass at launch, then a tiny HTTP server answers from
//! memory: every symbol as a point, in four named views — Map (a
//! deterministic kNN force layout over the full-dimension embeddings, see
//! `layout.rs`), Anchor (Channel-A/B cosines to a selected symbol), Time
//! (dates × map geometry), and free Axes (PCA-1..8 and the rest) for power
//! use. The page shows what the channels measured; it names nothing and
//! judges nothing (R&D archive spike/orient/RESULTS.md is the graveyard of the
//! auto-labeled alternative — the human drives, the tool shows).
//!
//! Index reuse: the embedding index is `find`'s own — same qualified doc
//! texts (`find::doc_texts`), same content-hash keys, same cache file
//! (`find::index_path`), so an `explore` launch warms `find` and vice
//! versa. The scan runs at `find`'s indexing altitude (`min_nodes: 8` — a
//! map, like a search index, wants broad coverage) with the shipping
//! defaults for everything else; the analysis (clones, twins, callers) is
//! computed over that same symbol universe so every point on the map has a
//! card and every card reference is a point on the map. Test symbols are
//! scanned and embedded too — the map can show them (hollow), unlike
//! `find`'s default ranking.
//!
//! Layering: everything below `serve` is model-free and feature-free —
//! `respond`/`find_response` are pure functions of `ExploreState`, so the
//! endpoint contract is testable without the model (tests/explore.rs). Only
//! `build_state`/`serve`/`run_loop` (embedding + tiny_http) sit behind the
//! `semantic` feature; without it the CLI exits 2, matching `find`.
//!
//! Never writes inside the scanned repo: the only write anywhere is the
//! embedding cache under `$XDG_CACHE_HOME/akron/find/` (find's own file).
//!
//! Branch highlighting (TKI-53): when the scanned root is a git repo on a
//! branch, `branch::detect` resolves the base at launch and `state_from`
//! marks branch-new symbols (see `branch.rs` for the rule). The feature
//! degrades to absent silently — `branch_new` stays `false` everywhere and
//! `/api/meta`'s `branch` is `null`, so the payload shape never changes.
//!
//! Drill-down (TKI-54): `/api/sublayout?path=<dir-or-file>` re-runs the
//! whole layout pipeline over just the symbols under that path, so a
//! subset's internal structure gets the full plane instead of the corner
//! the global layout squeezed it into. See `sublayout_json` for the rules
//! (global-coordinate seeding, drill-relative color keys). Computed per
//! request — a pure function of the immutable state, so it is exactly as
//! deterministic as the launch-time layout.
//!
//! Code viewer + navigation loop + compare (TKI-61): every symbol's full
//! source is captured once at boot (`scan_sources` — the only filesystem
//! read; requests never touch disk) with server-side highlight spans from
//! the tree-sitter parse and identifier links resolved with the exact
//! caller discipline `explain.rs` uses (same-file always, cross-file only
//! when corpus-unique and non-ambient — an ambiguous name stays plain
//! text). `/api/source?id=` serves one symbol's viewer payload;
//! `/api/compare?id=[&b=]` serves a side-by-side pair — right-hand side
//! defaults to the top embedding-ranked neighbor (the card's own ranking
//! discipline; the semantic score is never shipped) — with `align::align`'s
//! deterministic shared-region pairs. Offsets are UTF-16 code units so the
//! page slices its JS strings directly.
//!
//! Convention prevalence (TKI-56): the start card's one factual line —
//! `module docstring: 14 of 19 files`. `file_docs` classifies every scanned
//! `.py` file once at boot (`has_module_docstring`, model-free, feature-free)
//! and ships the raw per-file list in `/api/meta`; the page scopes the count
//! to the current drill (root = whole repo, a dir drill = that dir's files,
//! a file drill has no prevalence to report) the same way it already scopes
//! "largest dirs"/"most callers" — client-side filtering over an immutable
//! list, never a second server round trip.

use crate::align;
use crate::branch::BranchInfo;
use crate::explain;
use crate::fingerprint;
use crate::history;
use crate::layout;
use crate::normalize;
use crate::parse;
use crate::pca;
use crate::run::Analysis;
use crate::types::{Config, ModuleRef, NormTree, SymbolPrint};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Node;

pub const DEFAULT_PORT: u16 = 4816;

/// The single embedded page (vanilla JS + canvas; no CDN, no framework).
const PAGE: &str = include_str!("assets/explore.html");

/// Answered at `/favicon.ico` so the browser's automatic request doesn't
/// log a 404 in the console: one map point.
const FAVICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5" fill="#dba25e"/></svg>"##;

/// One HTTP answer, transport-agnostic so the router is pure.
pub struct Resp {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Everything a request reads. Built once at launch, immutable for the
/// server's lifetime (the query embedder is the one mutable piece and lives
/// with the loop, not here).
pub struct ExploreState {
    pub analysis: Analysis,
    pub cfg: Config,
    /// Import-aware in-degree (explain's), computed once for all cards.
    pub indeg: Vec<u32>,
    /// Per-symbol L2-normalized embedding, index-aligned with symbols.
    pub embeddings: Vec<Vec<f32>>,
    /// Per-symbol top-8 PCA scores, index-aligned with symbols.
    pub pca_scores: Vec<Vec<f32>>,
    /// Per-symbol branch-new flag (TKI-53), index-aligned with symbols.
    /// All-false when no branch context resolved at launch.
    pub branch_new: Vec<bool>,
    /// Reference-scale point radii (see `REF_EXTENT_PX`), kept for the
    /// drill relaxation so drilled points keep their global sizes.
    world_radii: Vec<f32>,
    /// Global full-geometry coordinates — the drill seed: a subset unfolds
    /// from the arrangement the reader just saw (and stays deterministic).
    full_xy: Vec<(f32, f32)>,
    /// Global product-geometry coordinates (`None` for test symbols).
    prod_xy: Vec<Option<(f32, f32)>>,
    /// Per-symbol viewer payload (TKI-61): source text, highlight/link
    /// tokens (UTF-16 offsets), and the normalized tree the compare view
    /// aligns with. Index-aligned with symbols.
    sources: Vec<SymbolSource>,
    /// Pre-rendered `/api/symbols` body (the endpoint the determinism
    /// contract is stated over — built once, byte-stable).
    symbols_json: Vec<u8>,
    /// Pre-rendered `/api/meta` body: PCA explained-variance shares + the
    /// dispersion-gated directory labels. Same byte-stability as symbols.
    meta_json: Vec<u8>,
    /// Pre-rendered page with the boot payload substituted.
    page: Vec<u8>,
}

/// The scan config explore shares with `find`'s index: `min_nodes: 8` for
/// coverage (R&D archive spike/find RESULTS.md — "a search index wants broad coverage");
/// everything else at the shipping defaults so clones/twins/families on the
/// card match what `scan`'s own thresholds would derive over this symbol set.
pub fn explore_cfg() -> Config {
    Config {
        min_nodes: 8,
        wl_iters: 3,
        theta_clone: 0.60,
        theta_b: 0.55,
        theta_a_low: 0.30,
        theta_family: crate::family::THETA_FAMILY,
        theta_b_family: crate::family::THETA_B_FAMILY,
        top: 20,
    }
}

/// `file`'s directory prefix truncated to `depth` segments (the map's color
/// key), `.` for root files.
fn dir_key(file: &str, depth: usize) -> String {
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop(); // the file name itself
    if parts.is_empty() {
        return ".".to_string();
    }
    let d = depth.min(parts.len());
    parts[..d].join("/")
}

/// Shallowest directory depth (1..=4) at which `files` (the product-code
/// file paths — callers filter tests) show at least 3 distinct color keys.
/// Top-level alone carries no information on single-package repos (httpx:
/// everything is `httpx/`), and single-root repos need to descend further
/// (`backend/src/<here>`). Drills reuse this over drill-relative paths.
fn color_depth(files: &[&str]) -> usize {
    for depth in 1..=4 {
        let distinct: std::collections::HashSet<String> =
            files.iter().map(|f| dir_key(f, depth)).collect();
        if distinct.len() >= 3 {
            return depth;
        }
    }
    4
}

/// The page's point-radius mapping, mirrored server-side so the overlap
/// relaxation works in the same units the canvas draws: `sqrt(nodes)`
/// min-max mapped into [2,7] px (see `prep()` in explore.html), divided by
/// the reference stage extent (a 900 px stage minus 2×34 px padding). On
/// other window sizes the guarantee scales with the zoom factor — the
/// relaxation is exact at reference scale, proportional elsewhere.
const REF_EXTENT_PX: f64 = 832.0;

fn world_radii(symbols: &[crate::types::SymbolPrint]) -> Vec<f32> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in symbols {
        let r = (s.node_count as f64).sqrt();
        if r < lo {
            lo = r;
        }
        if r > hi {
            hi = r;
        }
    }
    symbols
        .iter()
        .map(|s| {
            let px = if hi > lo {
                2.0 + 5.0 * ((s.node_count as f64).sqrt() - lo) / (hi - lo)
            } else {
                3.5
            };
            (px / REF_EXTENT_PX) as f32
        })
        .collect()
}

/// One file's module-docstring fact (TKI-56), plus enough context
/// (`is_test`) for the panel to respect the tests toggle. Passive data —
/// no methods, no hidden state.
pub struct FileDoc {
    pub file: String,
    pub is_test: bool,
    pub has_docstring: bool,
}

/// Every `.py` file under `root`, module-docstring classified — the same
/// file list and rel-path convention `scan.rs::process_file` uses, so
/// `file` lines up with `SymbolPrint.sym.file`. Model-free (no `semantic`
/// gate): this is boot's own filesystem work, not the model's, so it runs
/// and tests identically in both build configs. Sorted by file so the
/// output (and the JSON built from it) doesn't depend on walk order.
pub fn scan_file_docs(root: &Path) -> Vec<FileDoc> {
    let mut docs: Vec<FileDoc> = parse::python_files(root)
        .into_iter()
        .filter_map(|path| {
            let rel = path.strip_prefix(root).unwrap_or(&path).display().to_string();
            let source = std::fs::read(&path).ok()?;
            Some(FileDoc {
                is_test: parse::is_test_path(&rel),
                has_docstring: has_module_docstring(&source),
                file: rel,
            })
        })
        .collect();
    docs.sort_by(|a, b| a.file.cmp(&b.file));
    docs
}

/// A module docstring, matching Python's own `__doc__` rule: the file's
/// first top-level statement — comments and blank lines don't count, but
/// any other statement (e.g. `from __future__ import annotations`) does —
/// is a bare, non-interpolated, non-f string literal.
pub fn has_module_docstring(source: &[u8]) -> bool {
    let tree = parse::parse(source);
    let root = tree.root_node();
    let mut cursor = root.walk();
    let Some(first) = root.children(&mut cursor).find(|c| c.kind() != "comment") else {
        return false;
    };
    if first.kind() != "expression_statement" || first.named_child_count() != 1 {
        return false;
    }
    match first.named_child(0) {
        Some(s) if s.kind() == "string" => is_plain_string(s, source),
        _ => false,
    }
}

/// A `string` node counts as a docstring only if it carries no `{}`
/// interpolation and no `f`/`F` prefix — an f-string is never assigned to
/// `__doc__` even with zero braces, since the prefix alone routes it
/// through `JoinedStr` rather than a plain string constant.
fn is_plain_string(node: Node, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        match c.kind() {
            "interpolation" => return false,
            "string_start" => {
                let prefix: String = c
                    .utf8_text(source)
                    .unwrap_or("")
                    .chars()
                    .take_while(|ch| *ch != '"' && *ch != '\'')
                    .collect();
                if prefix.to_lowercase().contains('f') {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

// ── the code viewer (TKI-61): boot-time source capture ──

/// One symbol's viewer source as captured at boot: text plus byte-offset
/// highlight spans and unresolved identifier sites (they resolve in
/// `state_from`, where the whole symbol table exists). Passive data.
pub struct RawSource {
    text: String,
    /// (start, end, class), byte offsets relative to `text`;
    /// class ∈ kw/str/com/num/def/call.
    syntax: Vec<(u32, u32, &'static str)>,
    idents: Vec<IdentSite>,
    /// The symbol's normalized tree (spans stay absolute file offsets) —
    /// `align::align`'s input for the compare view.
    norm: NormTree,
}

/// An identifier occurrence that may become a link: byte span (relative to
/// the symbol's text) plus the same `(base, module)` join key a call at
/// this site would record (`normalize.rs`) — so link resolution can apply
/// exactly the caller-edge discipline.
struct IdentSite {
    start: u32,
    end: u32,
    base: String,
    module: Option<ModuleRef>,
}

/// A resolved viewer token: UTF-16 offsets into the symbol's text, a
/// highlight class (may be empty for link-only identifiers), and the
/// symbol id the token navigates to, if any.
type Tok = (u32, u32, &'static str, Option<u32>);

/// One symbol's servable viewer payload.
pub struct SymbolSource {
    text: String,
    toks: Vec<Tok>,
    norm: NormTree,
}

fn empty_raw() -> RawSource {
    RawSource {
        text: String::new(),
        syntax: Vec::new(),
        idents: Vec::new(),
        norm: NormTree { labels: Vec::new(), children: Vec::new(), spans: Vec::new() },
    }
}

/// Capture every symbol's source once (the viewer's only filesystem read —
/// requests answer from memory). Each file is read and parsed once;
/// symbols map back to their occurrence by exact byte span (the same
/// `occ.root.byte_range()` the scan stored). Model-free and feature-free,
/// like `scan_file_docs`. A file that became unreadable since the scan
/// leaves its symbols with an empty payload rather than failing the boot.
pub fn scan_sources(root: &Path, symbols: &[SymbolPrint]) -> Vec<RawSource> {
    let mut by_file: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, s) in symbols.iter().enumerate() {
        by_file.entry(s.sym.file.as_str()).or_default().push(i);
    }
    let mut out: Vec<RawSource> = (0..symbols.len()).map(|_| empty_raw()).collect();
    for (file, ids) in by_file {
        let Ok(source) = std::fs::read(root.join(file)) else { continue };
        let tree = parse::parse(&source);
        let imports = normalize::collect_imports(tree.root_node(), &source, file);
        let occs = parse::extract_functions(&tree, &source, file);
        let by_span: HashMap<(usize, usize), &parse::FnOccurrence> = occs
            .iter()
            .map(|o| ((o.root.byte_range().start, o.root.byte_range().end), o))
            .collect();
        for &i in &ids {
            let span = symbols[i].span;
            let Some(occ) = by_span.get(&span) else { continue };
            let slice = &source[span.0..span.1];
            let Ok(text) = std::str::from_utf8(slice) else {
                // Invalid UTF-8: byte offsets and UTF-16 offsets diverge —
                // serve the code as plain text rather than misaligned spans.
                out[i].text = String::from_utf8_lossy(slice).into_owned();
                continue;
            };
            let locals = normalize::collect_locals(occ.func, &source);
            let mut syntax = Vec::new();
            let mut idents = Vec::new();
            walk_syntax(
                occ.root, &source, span.0 as u32, &locals, &imports, false,
                &mut syntax, &mut idents,
            );
            out[i] = RawSource {
                text: text.to_string(),
                syntax,
                idents,
                norm: normalize::normalize(occ.root, occ.func, &source, &imports).tree,
            };
        }
    }
    out
}

fn tok_span(node: Node, base: u32) -> (u32, u32) {
    (node.start_byte() as u32 - base, node.end_byte() as u32 - base)
}

/// Hand-rolled highlight + link-site walk over one symbol's tree-sitter
/// subtree. Classes are deliberately few (kw/str/com/num/def/call) — the
/// page needs legibility, not a theme engine. Link sites skip: locals
/// (Python shadowing — `collect_locals` is the same binding set Channel A's
/// alpha-rename trusts), definition names, keyword-argument names, module
/// names, and anything inside import statements (module wiring, not
/// references the index can answer for).
#[allow(clippy::too_many_arguments)]
fn walk_syntax(
    node: Node,
    source: &[u8],
    base: u32,
    locals: &HashMap<String, u32>,
    imports: &normalize::ImportTable,
    in_import: bool,
    syntax: &mut Vec<(u32, u32, &'static str)>,
    idents: &mut Vec<IdentSite>,
) {
    match node.kind() {
        "comment" => {
            let (s, e) = tok_span(node, base);
            syntax.push((s, e, "com"));
        }
        "string" => {
            // Quotes and content highlight as string — coalesced into one
            // token per contiguous run; interpolations are code.
            let mut run: Option<(u32, u32)> = None;
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "interpolation" {
                    if let Some((s, e)) = run.take() {
                        syntax.push((s, e, "str"));
                    }
                    for j in 0..c.child_count() {
                        walk_syntax(
                            c.child(j).unwrap(), source, base, locals, imports,
                            in_import, syntax, idents,
                        );
                    }
                } else {
                    let (s, e) = tok_span(c, base);
                    run = Some(match run {
                        Some((rs, _)) => (rs, e),
                        None => (s, e),
                    });
                }
            }
            if let Some((s, e)) = run {
                syntax.push((s, e, "str"));
            }
        }
        "integer" | "float" => {
            let (s, e) = tok_span(node, base);
            syntax.push((s, e, "num"));
        }
        "true" | "false" | "none" => {
            let (s, e) = tok_span(node, base);
            syntax.push((s, e, "kw"));
        }
        "identifier" => {
            ident_token(node, source, base, locals, imports, in_import, syntax, idents);
        }
        _ => {
            if !node.is_named() {
                // Anonymous alphabetic tokens are Python's keywords (def,
                // return, if, async, …); operators/punctuation stay plain.
                let text = node.utf8_text(source).unwrap_or("");
                if !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphabetic()) {
                    let (s, e) = tok_span(node, base);
                    syntax.push((s, e, "kw"));
                }
                return;
            }
            let in_import = in_import
                || matches!(node.kind(), "import_statement" | "import_from_statement");
            for i in 0..node.child_count() {
                walk_syntax(
                    node.child(i).unwrap(), source, base, locals, imports,
                    in_import, syntax, idents,
                );
            }
        }
    }
}

/// One identifier: classify (def-name / call / plain) and record a link
/// site when the occurrence could reference a corpus symbol.
#[allow(clippy::too_many_arguments)]
fn ident_token(
    node: Node,
    source: &[u8],
    base: u32,
    locals: &HashMap<String, u32>,
    imports: &normalize::ImportTable,
    in_import: bool,
    syntax: &mut Vec<(u32, u32, &'static str)>,
    idents: &mut Vec<IdentSite>,
) {
    let text = node.utf8_text(source).unwrap_or("");
    let (s, e) = tok_span(node, base);
    let field_is = |p: Node, f: &str| {
        p.child_by_field_name(f).is_some_and(|n| n.id() == node.id())
    };
    if let Some(p) = node.parent() {
        if matches!(p.kind(), "function_definition" | "class_definition") && field_is(p, "name") {
            syntax.push((s, e, "def"));
            return;
        }
        if p.kind() == "keyword_argument" && field_is(p, "name") {
            return; // a call-site parameter name, never a reference
        }
        if p.kind() == "attribute" && field_is(p, "attribute") {
            let is_call = p.parent().is_some_and(|gp| {
                gp.kind() == "call"
                    && gp.child_by_field_name("function").is_some_and(|f| f.id() == p.id())
            });
            if is_call {
                syntax.push((s, e, "call"));
            }
            if !in_import {
                // `obj.attr`: resolve the object exactly like
                // `normalize::resolve_attr` — a module binding pins the
                // module; anything else keeps the base-name fallback.
                let module = match p.child_by_field_name("object") {
                    Some(o) if o.kind() == "identifier" => imports
                        .module_import(o.utf8_text(source).unwrap_or(""))
                        .cloned(),
                    _ => None,
                };
                idents.push(IdentSite { start: s, end: e, base: text.to_string(), module });
            }
            return;
        }
        if p.kind() == "call" && field_is(p, "function") {
            syntax.push((s, e, "call"));
            bare_site(s, e, text, locals, imports, in_import, idents);
            return;
        }
    }
    // A bare reference (callback argument, decorator, class access object).
    bare_site(s, e, text, locals, imports, in_import, idents);
}

/// A bare name's link site, mirroring `normalize::resolve_bare`: locals
/// shadow (no site), a from-imported symbol pins `(orig, module)`, a
/// module binding is a module (no symbol to link), anything else keeps the
/// corpus base-name fallback.
fn bare_site(
    s: u32,
    e: u32,
    text: &str,
    locals: &HashMap<String, u32>,
    imports: &normalize::ImportTable,
    in_import: bool,
    idents: &mut Vec<IdentSite>,
) {
    if in_import || text.is_empty() || locals.contains_key(text) {
        return;
    }
    if let Some((orig, module)) = imports.symbol_import(text) {
        idents.push(IdentSite {
            start: s,
            end: e,
            base: orig.to_string(),
            module: Some(module.clone()),
        });
        return;
    }
    if imports.module_import(text).is_some() {
        return; // the name IS a module, not a symbol
    }
    idents.push(IdentSite { start: s, end: e, base: text.to_string(), module: None });
}

// ── link resolution (pure; runs in `state_from` over the full table) ──

/// The lookup tables `resolve_link` joins against — the same maps
/// `explain::indegree` builds, kept over ALL symbols (the viewer shows test
/// code too; `base_name_counts` already counts everything).
struct LinkIndex<'a> {
    name_counts: HashMap<&'a str, u32>,
    by_name: HashMap<&'a str, Vec<u32>>,
    class_init: HashMap<&'a str, Vec<u32>>,
    by_name_file: HashMap<(&'a str, &'a str), Vec<u32>>,
    class_init_file: HashMap<(&'a str, &'a str), Vec<u32>>,
    module_files: HashMap<String, Vec<&'a str>>,
}

fn link_index(symbols: &[SymbolPrint]) -> LinkIndex<'_> {
    let mut idx = LinkIndex {
        name_counts: explain::base_name_counts(symbols),
        by_name: HashMap::new(),
        class_init: HashMap::new(),
        by_name_file: HashMap::new(),
        class_init_file: HashMap::new(),
        module_files: HashMap::new(),
    };
    let mut seen_files: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (t, s) in symbols.iter().enumerate() {
        let file = s.sym.file.as_str();
        if seen_files.insert(file) {
            let comps = explain::module_components(file);
            for start in 0..comps.len() {
                idx.module_files.entry(comps[start..].join(".")).or_default().push(file);
            }
        }
        let bn = explain::base_name(&s.sym.qname);
        if !explain::is_dunder(bn) {
            idx.by_name.entry(bn).or_default().push(t as u32);
            idx.by_name_file.entry((file, bn)).or_default().push(t as u32);
        }
        if let Some(class) = explain::class_of_init(&s.sym.qname) {
            idx.class_init.entry(class).or_default().push(t as u32);
            idx.class_init_file.entry((file, class)).or_default().push(t as u32);
        }
    }
    idx
}

/// Resolve one identifier site inside symbol `i` to a UNIQUE target, or
/// nothing — the viewer's law is explain's caller law plus uniqueness: a
/// link must never invent a binding, so any ambiguity renders as plain
/// text. Same-file candidates resolve first (Python's own scope order);
/// cross-file unresolved names pass only the `is_real_edge` gates
/// (corpus-unique AND not an ambient Python name); import-pinned names
/// resolve within the pinned module's files.
fn resolve_link(idx: &LinkIndex, symbols: &[SymbolPrint], i: usize, site: &IdentSite) -> Option<u32> {
    let base = site.base.as_str();
    if explain::is_dunder(base) {
        return None;
    }
    let gather = |a: Option<&Vec<u32>>, b: Option<&Vec<u32>>| -> Vec<u32> {
        let mut v: Vec<u32> = a
            .into_iter()
            .flatten()
            .chain(b.into_iter().flatten())
            .copied()
            .filter(|&t| t as usize != i)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    match &site.module {
        Some(ModuleRef::Absolute(m)) => {
            let files = idx.module_files.get(m.as_str())?;
            let mut v = Vec::new();
            for f in files {
                v.extend(gather(
                    idx.by_name_file.get(&(*f, base)),
                    idx.class_init_file.get(&(*f, base)),
                ));
            }
            v.sort_unstable();
            v.dedup();
            (v.len() == 1).then(|| v[0])
        }
        None => {
            let file = symbols[i].sym.file.as_str();
            let same = gather(
                idx.by_name_file.get(&(file, base)),
                idx.class_init_file.get(&(file, base)),
            );
            if same.len() == 1 {
                return Some(same[0]);
            }
            if !same.is_empty() {
                return None; // same-file ambiguity: plain text
            }
            if idx.name_counts.get(base).copied().unwrap_or(0) >= 2
                || explain::is_ambient_name(base)
            {
                return None; // the exact is_real_edge cross-file gates
            }
            let all = gather(idx.by_name.get(base), idx.class_init.get(base));
            (all.len() == 1).then(|| all[0])
        }
    }
}

/// Byte offset → UTF-16 code-unit offset for every position in `text`
/// (JS strings index in UTF-16; the page slices the shipped text directly).
fn utf16_map(text: &str) -> Vec<u32> {
    let mut map = vec![0u32; text.len() + 1];
    let mut u = 0u32;
    for (bi, ch) in text.char_indices() {
        for b in bi..bi + ch.len_utf8() {
            map[b] = u;
        }
        u += ch.len_utf16() as u32;
    }
    map[text.len()] = u;
    map
}

/// Resolve captured sources against the full symbol table: link sites become
/// link toks (unresolved ones vanish — plain text), spans convert to UTF-16.
fn resolve_sources(raw: Vec<RawSource>, symbols: &[SymbolPrint]) -> Vec<SymbolSource> {
    let idx = link_index(symbols);
    raw.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let mut toks: Vec<Tok> = r.syntax.iter().map(|&(s, e, c)| (s, e, c, None)).collect();
            for site in &r.idents {
                let Some(t) = resolve_link(&idx, symbols, i, site) else { continue };
                match toks.iter_mut().find(|k| k.0 == site.start && k.1 == site.end) {
                    Some(k) => k.3 = Some(t),
                    None => toks.push((site.start, site.end, "", Some(t))),
                }
            }
            toks.sort_unstable_by_key(|k| (k.0, k.1));
            let map = utf16_map(&r.text);
            for k in &mut toks {
                k.0 = map[k.0 as usize];
                k.1 = map[k.1 as usize];
            }
            SymbolSource { text: r.text, toks, norm: r.norm }
        })
        .collect()
}

/// Assemble the server state from an analysis + aligned embeddings. Pure
/// (PCA + JSON pre-rendering); the model never enters here, so tests build
/// states from synthetic vectors.
#[allow(clippy::too_many_arguments)]
pub fn state_from(
    repo_name: &str,
    cfg: Config,
    analysis: Analysis,
    embeddings: Vec<Vec<f32>>,
    file_docs: Vec<FileDoc>,
    raw_sources: Vec<RawSource>,
    tests_default: bool,
    branch: Option<BranchInfo>,
) -> ExploreState {
    let symbols = &analysis.scanned.symbols;
    assert_eq!(
        embeddings.len(),
        symbols.len(),
        "one embedding per symbol is the state contract"
    );
    assert_eq!(
        raw_sources.len(),
        symbols.len(),
        "one captured source per symbol is the state contract"
    );
    let sources = resolve_sources(raw_sources, symbols);
    let indeg = explain::indegree(symbols);
    let p = pca::pca(&embeddings, 8);
    let pca_scores: Vec<Vec<f32>> = if p.scores.is_empty() {
        vec![vec![0.0; 8]; symbols.len()]
    } else {
        p.scores
    };

    // The Map view's geometry: kNN in full dimension (the structure 2-D PCA
    // provably collapses — see layout.rs), drawn by the deterministic force
    // pass, seeded with PCA-1/2 for a stable global arrangement. Two
    // geometries, because test symbols are the majority on real repos and
    // smear the product code's neighborhoods when they share one plane:
    // `x`/`y` lay out ONLY non-test symbols (the default view; null for
    // tests), `xt`/`yt` lay out everything (what the tests toggle shows).
    let init: Vec<(f32, f32)> = pca_scores.iter().map(|s| (s[0], s[1])).collect();
    let radii = world_radii(symbols);
    let neighbors = layout::knn(&embeddings, layout::KNN_K);
    let mut full_xy = layout::layout(&embeddings, &neighbors, &init);
    layout::relax(&mut full_xy, &radii);
    // Edge ids per symbol: the exact adjacency the layout's springs used.
    let full_nn_ids: Vec<Vec<u32>> = layout::adjacency(&embeddings, &neighbors)
        .into_iter()
        .map(|a| a.into_iter().map(|(j, _)| j).collect())
        .collect();

    let prod_ids: Vec<usize> = (0..symbols.len()).filter(|&i| !symbols[i].is_test).collect();
    let prod_emb: Vec<Vec<f32>> = prod_ids.iter().map(|&i| embeddings[i].clone()).collect();
    let prod_init: Vec<(f32, f32)> = prod_ids.iter().map(|&i| init[i]).collect();
    let prod_radii: Vec<f32> = prod_ids.iter().map(|&i| radii[i]).collect();
    let prod_nn = layout::knn(&prod_emb, layout::KNN_K);
    let mut prod_xy = layout::layout(&prod_emb, &prod_nn, &prod_init);
    layout::relax(&mut prod_xy, &prod_radii);
    let mut xy: Vec<Option<(f32, f32)>> = vec![None; symbols.len()];
    let mut prod_nn_ids: Vec<Option<Vec<u32>>> = vec![None; symbols.len()];
    for (slot, adj) in layout::adjacency(&prod_emb, &prod_nn).into_iter().enumerate() {
        xy[prod_ids[slot]] = Some(prod_xy[slot]);
        prod_nn_ids[prod_ids[slot]] =
            Some(adj.into_iter().map(|(j, _)| prod_ids[j as usize] as u32).collect());
    }

    // Branch-new (TKI-53), content-anchored: a symbol is branch-new iff its
    // file changed vs the merge-base AND its Merkle root is absent from the
    // base versions of the changed files. Moved-but-unchanged code keeps its
    // root, so it never marks. All-false when no branch context resolved.
    let branch_new: Vec<bool> = symbols
        .iter()
        .map(|s| {
            branch.as_ref().is_some_and(|b| {
                b.changed_files.contains(&s.sym.file) && !b.base_roots.contains(&s.merkle_root)
            })
        })
        .collect();

    let prod_files: Vec<&str> = symbols
        .iter()
        .filter(|s| !s.is_test)
        .map(|s| s.sym.file.as_str())
        .collect();
    let depth = color_depth(&prod_files);
    let dirs: Vec<String> = symbols.iter().map(|s| dir_key(&s.sym.file, depth)).collect();
    let rows: Vec<Value> = symbols
        .iter()
        .enumerate()
        .map(|(i, s)| {
            json!({
                "id": i,
                "qname": s.sym.qname,
                "file": s.sym.file,
                "line": s.sym.line,
                "nodes": s.node_count,
                // import-aware in-degree (explain's number) — the page's
                // guidance panel ranks "most callers" from it, per scope
                "indeg": indeg[i],
                "is_test": s.is_test,
                "branch_new": branch_new[i],
                "first_seen": s.dating.map(|d| d.first_seen),
                "last_touched": s.dating.map(|d| d.last_touched),
                "dir": dirs[i],
                "x": xy[i].map(|p| p.0),
                "y": xy[i].map(|p| p.1),
                "xt": full_xy[i].0,
                "yt": full_xy[i].1,
                "pca": pca_scores[i],
                // layout edges: product-geometry ids (null for tests) and
                // full-geometry ids — the page picks by the tests toggle
                "nnp": prod_nn_ids[i],
                "nn": full_nn_ids[i],
            })
        })
        .collect();
    let symbols_json = serde_json::to_vec(&Value::Array(rows)).expect("symbols json");

    // /api/meta: PCA explained-variance shares (of TOTAL variance, so the 8
    // shipped shares sum to < 1) + directory labels per geometry.
    let var_shares: Vec<f64> = p
        .variances
        .iter()
        .map(|v| if p.total_variance > 0.0 { v / p.total_variance } else { 0.0 })
        .collect();
    let prod_dirs: Vec<String> = prod_ids.iter().map(|&i| dirs[i].clone()).collect();
    // `branch.changed` counts branch-new SYMBOLS — the same convention as the
    // dir chips (symbol counts), and exactly the points the chip highlights.
    let meta = json!({
        "pca_var": var_shares,
        "labels": {
            "prod": label_json(&layout::dir_labels(&prod_xy, &prod_dirs)),
            "full": label_json(&layout::dir_labels(&full_xy, &dirs)),
        },
        "branch": branch.as_ref().map(|b| json!({
            "name": b.branch,
            "base": b.base,
            "changed": branch_new.iter().filter(|&&x| x).count(),
        })),
        // TKI-56: the raw per-file fact list — the page scopes it to the
        // current drill (client-side), the same way it scopes the other
        // guide-panel facts over `/api/symbols`.
        "file_docs": file_docs.iter().map(|f| json!({
            "file": f.file,
            "is_test": f.is_test,
            "doc": f.has_docstring,
        })).collect::<Vec<_>>(),
    });
    let meta_json = meta.to_string().into_bytes();

    let boot = json!({
        "repo": repo_name,
        "count": symbols.len(),
        "tests": tests_default,
        "k": layout::KNN_K,
    });
    let page = PAGE
        .replace("__AKRON_BOOT__", &boot.to_string())
        .into_bytes();

    ExploreState {
        analysis,
        cfg,
        indeg,
        embeddings,
        pca_scores,
        branch_new,
        world_radii: radii,
        full_xy,
        prod_xy: xy,
        sources,
        symbols_json,
        meta_json,
        page,
    }
}

fn label_json(labels: &[layout::DirLabel]) -> Vec<Value> {
    labels
        .iter()
        .map(|l| json!({ "dir": l.dir, "x": l.x, "y": l.y, "count": l.count, "r": l.r }))
        .collect()
}

// ── the router (pure; /api/find is separate because it needs the model) ──

/// Answer every endpoint except `/api/find` (whose query must be embedded
/// first — see `find_response`). `url` is the raw request target, e.g.
/// `/api/explain?id=3`.
pub fn respond(state: &ExploreState, url: &str) -> Resp {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    match path {
        "/" => Resp {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: state.page.clone(),
        },
        "/favicon.ico" => Resp {
            status: 200,
            content_type: "image/svg+xml",
            body: FAVICON.as_bytes().to_vec(),
        },
        "/api/symbols" => json_ok(state.symbols_json.clone()),
        "/api/meta" => json_ok(state.meta_json.clone()),
        "/api/sublayout" => match param(query, "path").filter(|p| !p.is_empty()) {
            None => error(400, "missing path parameter"),
            Some(p) => match sublayout_json(state, &p) {
                Some(v) => json_ok(v.to_string().into_bytes()),
                None => error(404, "no symbols under that path"),
            },
        },
        "/api/explain" => match parse_id(state, query) {
            Ok(id) => json_ok(explain_json(state, id).to_string().into_bytes()),
            Err(r) => r,
        },
        "/api/source" => match parse_id(state, query) {
            Ok(id) => json_ok(source_json(state, id).to_string().into_bytes()),
            Err(r) => r,
        },
        "/api/compare" => match parse_id(state, query) {
            Ok(id) => match compare_json(state, id, query) {
                Ok(v) => json_ok(v.to_string().into_bytes()),
                Err(r) => r,
            },
            Err(r) => r,
        },
        "/api/anchor" => match parse_id(state, query) {
            Ok(id) => json_ok(anchor_json(state, id).to_string().into_bytes()),
            Err(r) => r,
        },
        _ => error(404, "no such endpoint"),
    }
}

/// `/api/find`: rank the whole in-memory index against an already-embedded
/// query vector — `find::rank` over `find::ranked_symbol_ids`, exactly the
/// CLI's levers (test symbols dropped unless `include_tests`).
pub fn find_response(
    state: &ExploreState,
    query_vec: &[f32],
    include_tests: bool,
    top: usize,
) -> Resp {
    let symbols = &state.analysis.scanned.symbols;
    let ids = crate::find::ranked_symbol_ids(symbols, include_tests);
    let candidates: Vec<(usize, &[f32])> = ids
        .iter()
        .map(|&i| (i, state.embeddings[i].as_slice()))
        .collect();
    let ranked = crate::find::rank(query_vec, &candidates, top);
    let hits: Vec<Value> = ranked
        .iter()
        .enumerate()
        .map(|(n, &(i, score))| {
            json!({
                "rank": n + 1,
                "id": i,
                "score": score,
                "qname": symbols[i].sym.qname,
                "file": symbols[i].sym.file,
                "line": symbols[i].sym.line,
            })
        })
        .collect();
    json_ok(json!({ "hits": hits }).to_string().into_bytes())
}

fn json_ok(body: Vec<u8>) -> Resp {
    Resp {
        status: 200,
        content_type: "application/json",
        body,
    }
}

fn error(status: u16, msg: &str) -> Resp {
    Resp {
        status,
        content_type: "application/json",
        body: json!({ "error": msg }).to_string().into_bytes(),
    }
}

/// `id=<n>` from a query string, bounds-checked against the symbol table:
/// missing/malformed → 400, out of range → 404.
fn parse_id(state: &ExploreState, query: &str) -> Result<usize, Resp> {
    let raw = param(query, "id").ok_or_else(|| error(400, "missing id parameter"))?;
    let id: usize = raw
        .parse()
        .map_err(|_| error(400, "id must be an integer"))?;
    if id >= state.analysis.scanned.symbols.len() {
        return Err(error(404, "no symbol with that id"));
    }
    Ok(id)
}

/// First `name=value` from a query string, percent- and plus-decoded.
pub fn param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| url_decode(v))
    })
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => match (hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: Option<&u8>) -> Option<u8> {
    (*b? as char).to_digit(16).map(|d| d as u8)
}

// ── endpoint bodies ──

fn sym_ref(state: &ExploreState, i: usize) -> Value {
    let s = &state.analysis.scanned.symbols[i].sym;
    json!({ "id": i, "qname": s.qname, "file": s.file, "line": s.line })
}

/// The explain card as JSON — the same `explain::card` data the CLI card
/// prints, symbol references resolved to ids so the panel can navigate.
fn explain_json(state: &ExploreState, id: usize) -> Value {
    let symbols = &state.analysis.scanned.symbols;
    let c = explain::card(&state.analysis, &state.indeg, id);
    let s = &symbols[id];
    json!({
        "id": id,
        "qname": s.sym.qname,
        "file": s.sym.file,
        "line": s.sym.line,
        "nodes": s.node_count,
        "is_test": s.is_test,
        "branch_new": state.branch_new[id],
        "nearest_existing": nearest_existing(state, id),
        "entry": c.entry,
        "dating": c.dating.as_ref().map(|d| json!({
            "first_seen": d.first_seen,
            "last_touched": d.last_touched,
            "first": history::fmt_date(d.first_seen),
            "last": history::fmt_date(d.last_touched),
            "activity": d.activity.label(),
        })),
        "clones": {
            "exact": c.exact_clones.iter().map(|&i| sym_ref(state, i)).collect::<Vec<_>>(),
            "near": c.near_clones.iter().map(|&(i, cos)| {
                let mut v = sym_ref(state, i);
                v["cos"] = json!(cos);
                v
            }).collect::<Vec<_>>(),
        },
        "twins": c.twins.as_ref().map(|t| json!({
            "members": t.members.iter().map(|&i| sym_ref(state, i)).collect::<Vec<_>>(),
            "b_max": t.b_max,
            "theta_b": state.cfg.theta_b,
            "shared_terms": t.shared_terms,
        })),
        "callers": c.callers.iter().map(|&i| sym_ref(state, i)).collect::<Vec<_>>(),
        "callees": c.callees.iter().map(|&i| sym_ref(state, i)).collect::<Vec<_>>(),
        "family": c.family.as_ref().map(|f| json!({
            "index": f.index + 1,
            "role": if f.is_core { "core" } else { "drift" },
            "cos": f.cos_to_core,
        })),
    })
}

/// For a branch-new symbol: the top-8 existing (non-branch-new, non-test —
/// find's default exclusion) symbols by semantic cosine to it. THE LAW
/// (DESIGN.md §1.2): the model may RANK this list, but the numbers shipped
/// are the deterministic channels' only — each row carries the Channel-A
/// (WL structure) and Channel-B (tf-idf vocabulary) cosines to the branch
/// symbol, and the semantic score is dropped here. `null` for symbols that
/// are not branch-new.
fn nearest_existing(state: &ExploreState, id: usize) -> Value {
    if !state.branch_new[id] {
        return Value::Null;
    }
    let symbols = &state.analysis.scanned.symbols;
    let candidates: Vec<(usize, &[f32])> = (0..symbols.len())
        .filter(|&i| !symbols[i].is_test && !state.branch_new[i])
        .map(|i| (i, state.embeddings[i].as_slice()))
        .collect();
    let ranked = crate::find::rank(&state.embeddings[id], &candidates, 8);
    let rows: Vec<Value> = ranked
        .iter()
        .map(|&(i, _)| {
            let mut v = sym_ref(state, i);
            v["a_cos"] = json!(fingerprint::cosine(&symbols[i].wl, &symbols[id].wl));
            v["b_cos"] = json!(state.analysis.vocab.cosine_between(i as u32, id as u32));
            v
        })
        .collect();
    Value::Array(rows)
}

/// `/api/source` (TKI-61): one symbol's viewer payload — full source text,
/// highlight/link tokens as `[start, end, class, link-id|null]` with
/// UTF-16 offsets. Everything was resolved at boot; this is a read.
fn source_json(state: &ExploreState, id: usize) -> Value {
    let s = &state.analysis.scanned.symbols[id];
    let src = &state.sources[id];
    json!({
        "id": id,
        "qname": s.sym.qname,
        "file": s.sym.file,
        "line": s.sym.line,
        "is_test": s.is_test,
        "text": src.text,
        "toks": src.toks.iter()
            .map(|t| json!([t.0, t.1, t.2, t.3]))
            .collect::<Vec<_>>(),
    })
}

/// The compare view's default right-hand side: the top embedding-ranked
/// symbol other than the selection — the same ranking discipline the
/// card's `nearest_existing` uses (test symbols excluded unless the
/// selection is itself a test symbol). THE LAW (DESIGN.md §1.2): the model
/// only picks WHICH symbol; no semantic number ships anywhere.
fn nearest_neighbor(state: &ExploreState, id: usize) -> Option<usize> {
    let symbols = &state.analysis.scanned.symbols;
    let candidates: Vec<(usize, &[f32])> = (0..symbols.len())
        .filter(|&j| j != id && (symbols[id].is_test || !symbols[j].is_test))
        .map(|j| (j, state.embeddings[j].as_slice()))
        .collect();
    crate::find::rank(&state.embeddings[id], &candidates, 1)
        .first()
        .map(|&(j, _)| j)
}

/// `/api/compare?id=<a>[&b=<b>]` (TKI-61): both sides' viewer payloads plus
/// `align::align`'s deterministic shared-region pairs (tier 1 = identical
/// after whitespace collapse, tier 2 = structurally aligned near-miss).
/// Region offsets are UTF-16 into each side's text. No similarity number
/// appears here — the card already carries the deterministic A/B cosines.
fn compare_json(state: &ExploreState, id: usize, query: &str) -> Result<Value, Resp> {
    let symbols = &state.analysis.scanned.symbols;
    let bid = match param(query, "b") {
        Some(raw) => {
            let b: usize = raw.parse().map_err(|_| error(400, "b must be an integer"))?;
            if b >= symbols.len() {
                return Err(error(404, "no symbol with that id"));
            }
            b
        }
        None => nearest_neighbor(state, id)
            .ok_or_else(|| error(404, "no comparison candidate"))?,
    };
    let (sa, sb) = (&state.sources[id], &state.sources[bid]);
    let regions = align::align(
        &sa.norm, &sa.text, symbols[id].span.0 as u32,
        &sb.norm, &sb.text, symbols[bid].span.0 as u32,
    );
    let (ma, mb) = (utf16_map(&sa.text), utf16_map(&sb.text));
    Ok(json!({
        "a": source_json(state, id),
        "b": source_json(state, bid),
        "regions": regions.iter().map(|r| json!({
            "a": [ma[r.a.0 as usize], ma[r.a.1 as usize]],
            "b": [mb[r.b.0 as usize], mb[r.b.1 as usize]],
            "tier": r.tier,
        })).collect::<Vec<_>>(),
    }))
}

/// Drill-down (TKI-54): the layout pipeline re-run over the symbols under
/// `path` — a directory prefix (segment-bounded: `a/b` matches `a/b/…`,
/// never `a/bc.py`) or a single file. `None` when nothing lives there.
///
/// Same pipeline as the global map, three drill-specific rules:
/// - **Seeding**: the force pass starts from the members' GLOBAL
///   coordinates, so the drill unfolds the arrangement the reader just saw
///   instead of reshuffling it — and inherits the launch layout's
///   determinism.
/// - **Color keys** re-derive inside the drill: subdirectories at the
///   shallowest depth showing ≥3 distinct keys (`.` = files at the drill
///   root); a single-file drill keys by the qname's class context instead
///   (methods group by class, `.` = module-level).
/// - **Two geometries**, like the global map: `x`/`y` lay out only non-test
///   members (null for tests), `xt`/`yt` everything. Labels are
///   dispersion-gated on the drill plane per geometry.
///
/// All arrays are index-aligned with `ids`; edge lists carry GLOBAL symbol
/// ids so cards, search hits and the drill plane stay one id space.
fn sublayout_json(state: &ExploreState, path: &str) -> Option<Value> {
    let symbols = &state.analysis.scanned.symbols;
    let prefix = format!("{path}/");
    let ids: Vec<usize> = (0..symbols.len())
        .filter(|&i| {
            let f = &symbols[i].sym.file;
            f == path || f.starts_with(&prefix)
        })
        .collect();
    if ids.is_empty() {
        return None;
    }
    let is_file = ids.iter().all(|&i| symbols[i].sym.file == path);

    // Drill color keys: class context inside one file, relative dirs otherwise.
    let keys: Vec<String> = if is_file {
        ids.iter()
            .map(|&i| match symbols[i].sym.qname.rsplit_once('.') {
                Some((ctx, _)) => ctx.to_string(),
                None => ".".to_string(),
            })
            .collect()
    } else {
        let rel = |i: usize| symbols[i].sym.file.strip_prefix(&prefix).unwrap_or(path);
        let prod_rel: Vec<&str> = ids
            .iter()
            .filter(|&&i| !symbols[i].is_test)
            .map(|&i| rel(i))
            .collect();
        let depth = color_depth(&prod_rel);
        let by_dir: Vec<String> = ids.iter().map(|&i| dir_key(rel(i), depth)).collect();
        // A flat directory has no subdir structure to color — fall back to
        // first-segment keys, which name the FILES (the ladder's next rung).
        let distinct: std::collections::HashSet<&String> = by_dir.iter().collect();
        if distinct.len() >= 2 {
            by_dir
        } else {
            ids.iter()
                .map(|&i| rel(i).split('/').next().expect("split yields ≥1").to_string())
                .collect()
        }
    };

    // Full plane: every member, seeded from its global full-geometry spot.
    let emb: Vec<Vec<f32>> = ids.iter().map(|&i| state.embeddings[i].clone()).collect();
    let radii: Vec<f32> = ids.iter().map(|&i| state.world_radii[i]).collect();
    let init: Vec<(f32, f32)> = ids.iter().map(|&i| state.full_xy[i]).collect();
    let nn = layout::knn(&emb, layout::KNN_K);
    let mut full = layout::layout(&emb, &nn, &init);
    layout::relax(&mut full, &radii);
    let nn_ids: Vec<Vec<u32>> = layout::adjacency(&emb, &nn)
        .into_iter()
        .map(|a| a.into_iter().map(|(j, _)| ids[j as usize] as u32).collect())
        .collect();

    // Product plane: non-test members only (mirrors the global map).
    let prod_slots: Vec<usize> = (0..ids.len()).filter(|&s| !symbols[ids[s]].is_test).collect();
    let p_emb: Vec<Vec<f32>> = prod_slots.iter().map(|&s| emb[s].clone()).collect();
    let p_radii: Vec<f32> = prod_slots.iter().map(|&s| radii[s]).collect();
    let p_init: Vec<(f32, f32)> = prod_slots
        .iter()
        .map(|&s| state.prod_xy[ids[s]].expect("non-test symbols have product coords"))
        .collect();
    let p_nn = layout::knn(&p_emb, layout::KNN_K);
    let mut prod = layout::layout(&p_emb, &p_nn, &p_init);
    layout::relax(&mut prod, &p_radii);
    let mut xy: Vec<Option<(f32, f32)>> = vec![None; ids.len()];
    let mut nnp_ids: Vec<Option<Vec<u32>>> = vec![None; ids.len()];
    for (slot, adj) in layout::adjacency(&p_emb, &p_nn).into_iter().enumerate() {
        xy[prod_slots[slot]] = Some(prod[slot]);
        nnp_ids[prod_slots[slot]] = Some(
            adj.into_iter()
                .map(|(j, _)| ids[prod_slots[j as usize]] as u32)
                .collect(),
        );
    }

    let prod_keys: Vec<String> = prod_slots.iter().map(|&s| keys[s].clone()).collect();
    Some(json!({
        "path": path,
        // "file" ⇒ color keys are class contexts, not drillable paths
        "kind": if is_file { "file" } else { "dir" },
        "ids": ids,
        "dir": keys,
        "x": xy.iter().map(|p| p.map(|q| q.0)).collect::<Vec<_>>(),
        "y": xy.iter().map(|p| p.map(|q| q.1)).collect::<Vec<_>>(),
        "xt": full.iter().map(|p| p.0).collect::<Vec<_>>(),
        "yt": full.iter().map(|p| p.1).collect::<Vec<_>>(),
        "nn": nn_ids,
        "nnp": nnp_ids,
        "labels": {
            "prod": label_json(&layout::dir_labels(&prod, &prod_keys)),
            "full": label_json(&layout::dir_labels(&full, &keys)),
        },
    }))
}

/// The two deterministic engine dimensions, relative to an anchor symbol:
/// Channel-A (WL histogram) and Channel-B (tf-idf) cosine of every symbol
/// to `id`. Index-aligned with `/api/symbols`.
fn anchor_json(state: &ExploreState, id: usize) -> Value {
    let symbols = &state.analysis.scanned.symbols;
    let a_cos: Vec<f32> = symbols
        .iter()
        .map(|s| fingerprint::cosine(&s.wl, &symbols[id].wl))
        .collect();
    let b_cos: Vec<f32> = (0..symbols.len())
        .map(|i| state.analysis.vocab.cosine_between(i as u32, id as u32))
        .collect();
    json!({ "id": id, "a_cos": a_cos, "b_cos": b_cos })
}

// ── launch (model + tiny_http; `semantic` builds only) ──

/// Scan, embed (find's cache), and assemble the server state. The one
/// model-dependent constructor; returns the embedder so `/api/find` can
/// embed queries for the server's lifetime.
#[cfg(feature = "semantic")]
pub fn build_state(
    root: &Path,
    tests_default: bool,
    base: Option<&str>,
) -> Result<(ExploreState, crate::embed::Embedder)> {
    use crate::find;

    let cfg = explore_cfg();
    let analysis = crate::run::analyze(root, &cfg);
    let symbols = &analysis.scanned.symbols;

    // Embed EVERY symbol (tests included — the map can show them), through
    // find's cache: same doc texts, same keys, same file.
    let ids: Vec<usize> = (0..symbols.len()).collect();
    let texts = find::doc_texts(root, symbols, &ids);
    let keys: Vec<String> = texts.iter().map(|t| find::doc_key(t)).collect();
    let index_file = find::index_path(root);
    let mut cache = find::EmbCache::load(&index_file);
    let mut embedder = crate::embed::Embedder::load()?;
    if find::embed_missing(&mut embedder, &mut cache, &texts, &keys)? {
        cache.save(&index_file)?;
    }
    let embeddings: Vec<Vec<f32>> = keys.iter().map(|k| cache.map[k].clone()).collect();

    let repo_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());
    // Branch context (TKI-53): resolved once here; None degrades silently.
    let branch = crate::branch::detect(root, base);
    // TKI-56: module-docstring prevalence, per file — boot's own filesystem
    // work, same as the scan above.
    let file_docs = scan_file_docs(root);
    // TKI-61: viewer sources — the last boot-time filesystem pass.
    let raw_sources = scan_sources(root, &analysis.scanned.symbols);
    Ok((
        state_from(
            &repo_name, cfg, analysis, embeddings, file_docs, raw_sources, tests_default, branch,
        ),
        embedder,
    ))
}

/// Serve until killed. Prints exactly one stdout line:
/// `explore — <N> symbols — http://127.0.0.1:<port>`. Port 0 asks the OS
/// for a free port (the printed line shows the real one).
#[cfg(feature = "semantic")]
pub fn serve(root: &Path, port: u16, tests_default: bool, base: Option<&str>) -> Result<()> {
    let (state, mut embedder) = build_state(root, tests_default, base)?;
    let server = bind(port)?;
    let bound = bound_port(&server).unwrap_or(port);
    println!(
        "explore \u{2014} {} symbols \u{2014} http://127.0.0.1:{bound}",
        state.analysis.scanned.symbols.len()
    );
    run_loop(&server, &state, &mut embedder);
    Ok(())
}

#[cfg(feature = "semantic")]
pub fn bind(port: u16) -> Result<tiny_http::Server> {
    tiny_http::Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow::anyhow!("binding 127.0.0.1:{port}: {e}"))
}

#[cfg(feature = "semantic")]
pub fn bound_port(server: &tiny_http::Server) -> Option<u16> {
    server.server_addr().to_ip().map(|a| a.port())
}

/// One request at a time: every answer is an in-memory read (or one query
/// embedding), so sequential handling stays in the milliseconds and the
/// state needs no locks.
#[cfg(feature = "semantic")]
pub fn run_loop(
    server: &tiny_http::Server,
    state: &ExploreState,
    embedder: &mut crate::embed::Embedder,
) {
    for request in server.incoming_requests() {
        let url = request.url().to_string();
        let resp = if *request.method() != tiny_http::Method::Get {
            error(405, "GET only")
        } else {
            route(state, embedder, &url)
        };
        let out = tiny_http::Response::from_data(resp.body)
            .with_status_code(resp.status)
            .with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], resp.content_type.as_bytes())
                    .expect("static header"),
            );
        let _ = request.respond(out);
    }
}

#[cfg(feature = "semantic")]
fn route(state: &ExploreState, embedder: &mut crate::embed::Embedder, url: &str) -> Resp {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if path != "/api/find" {
        return respond(state, url);
    }
    let Some(q) = param(query, "q").filter(|q| !q.is_empty()) else {
        return error(400, "missing q parameter");
    };
    let include_tests = param(query, "tests").as_deref() == Some("1");
    let top = param(query, "top")
        .and_then(|t| t.parse().ok())
        .unwrap_or(20);
    match embedder.embed_query(&q) {
        Ok(v) => find_response(state, &v, include_tests, top),
        Err(e) => error(500, &format!("embedding the query failed: {e}")),
    }
}

#[cfg(not(feature = "semantic"))]
pub fn serve(_root: &Path, _port: u16, _tests_default: bool, _base: Option<&str>) -> Result<()> {
    anyhow::bail!("akron was built without the semantic feature")
}
