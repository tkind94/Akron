//! `akron explain <root> <target>` (EXP-C): one terse, factual card about a
//! single symbol — what it is, whether something already looks like it
//! (clones, role twins), who calls it, and its family membership. This is a
//! *view*: every fact is read off
//! the already-computed `Analysis` (run.rs) and a direct-call-edge pass over
//! the already-collected `SymbolPrint::calls` — no new detection, no new
//! threshold, no prose.
//!
//! **Why callers/callees aren't read off `callrel::CallGraph` directly.**
//! `CallGraph` exists to *suppress* competing-pattern pairs and exposes only
//! `related(x, y) -> bool` — a symmetric two-hop-chain predicate built for
//! that purpose. It cannot tell a caller from a callee, and a "callers of X"
//! listing wants literal direct edges, not that broader relation (which
//! `analysis.competing` has already applied — any two symbols sharing a
//! `role twins` group are, by construction, NOT call-related). So this module
//! re-derives direct edges from the same public fields `callrel.rs` itself
//! reads (`SymbolPrint::calls`, `Call::{base,module}`), mirroring its
//! base-name/import-module join. This is a small, deliberate duplication —
//! this codebase's own precedent for such one-line helpers (see
//! `family::base_name`'s doc comment: "each module keeps its own copy rather
//! than a shared export").
//!
//! **Section omission.** `clones` and `family` are structural-membership
//! sections: omitted entirely when the symbol isn't part of any such
//! structure (nothing to report). `role twins` and `callers`/`callees` are
//! queries that always run and always have a defined answer (including a
//! negative one), so they always print — `none` is itself a fact worth
//! stating (JOURNEYS.md principle 4: an empty result explains itself).
//!
//! **Entry-point tag (TKI-42, salvaged from the EXP-B orientation-map spike,
//! `R&D archive spike/orient/RESULTS.md`).** That spike's outcome was KILL, but its
//! salvage note singled out one sub-result as worth keeping: import-aware
//! call-graph in-degree — "the only output a stranger consistently wanted" —
//! computed per directory rather than per embedding cluster (the spike's own
//! model-free substitute; explain has no embeddings and none are added here).
//! Ported verbatim as `indegree`/`entry_point_tag` below: a header tag,
//! present only when this symbol is the most-called member of its own
//! directory, omitted otherwise (same omission discipline as `clones`/
//! `family` — nothing to report isn't printed as a negative).
//!
//! **Generic-verb caller/callee noise (TKI-42).** `R&D archive validation/explain-eval.md`
//! traced two graded weaknesses (cases #5, #7, #8) to the same mechanism:
//! `call_targets` below, like `callrel.rs`, falls back to a corpus-wide
//! base-name join when a call's import module can't be resolved (`client.post(...)`,
//! `os.environ.get(...)` — the object is a local/attribute chain, not a
//! statically resolvable import). That fallback is deliberately kept in
//! `callrel.rs` for its own recall needs (suppressing wrapper pairs), but for
//! explain's caller/callee *listing* an unresolved base name that also
//! matches two-or-more other corpus symbols (`get`, `post`, `update`, …)
//! is indistinguishable noise, not a fact about this specific symbol — so
//! `is_real_edge` gates that one fallback case out of the presentation layer
//! only (`callrel.rs` is untouched; its own base-name fallback keeps its
//! current, separately-justified recall behavior).

use crate::fingerprint;
use crate::history;
use crate::run::Analysis;
use crate::types::{Call, Config, ModuleRef, SymbolPrint};
use anyhow::{bail, Context, Result};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Resolve `target` against `analysis` and print its explain card to stdout.
pub fn explain(root: &Path, analysis: &Analysis, cfg: &Config, target: &str) -> Result<()> {
    let idx = resolve(root, &analysis.scanned.symbols, target)?;
    render_card(analysis, cfg, idx)
}

/// Bare `akron explain <root>` (TKI-51, target omitted): the owner's own
/// complaint was never getting the target syntax right, so the fix is to
/// hand back working examples instead of a syntax lecture. One usage line
/// plus three real invocations built from `symbols`' own largest non-test
/// members (the ones most worth reading first) — two as a bare suffix-form
/// target, one as `file:line` — so the very first thing printed is already
/// copy-pasteable. `root_arg` is the path exactly as the caller typed it
/// (not the canonicalized root), so the example matches what they just ran.
pub fn print_usage(root_arg: &Path, symbols: &[SymbolPrint]) {
    eprintln!("usage: akron explain <root> <target>");
    let mut biggest: Vec<usize> = (0..symbols.len()).filter(|&i| !symbols[i].is_test).collect();
    biggest.sort_by_key(|&i| {
        let s = &symbols[i];
        (Reverse(s.node_count), s.sym.file.clone(), s.sym.line)
    });
    let root = root_arg.display();
    for &i in biggest.iter().take(2) {
        eprintln!(
            "  akron explain {root} {}",
            base_name(&symbols[i].sym.qname)
        );
    }
    if let Some(&i) = biggest.get(2) {
        let s = &symbols[i].sym;
        eprintln!("  akron explain {root} {}:{}", s.file, s.line);
    }
}

// ── resolution: exact qname, file:line, dotted-suffix, or substring (TKI-51) ──

fn resolve(root: &Path, symbols: &[SymbolPrint], target: &str) -> Result<usize> {
    if let Some((file, line)) = target.rsplit_once(':') {
        if let Ok(line) = line.parse::<usize>() {
            if !file.is_empty() {
                return resolve_file_line(root, symbols, file, line);
            }
        }
    }
    resolve_name(symbols, target)
}

/// Exact qname match takes priority; only when nothing matches exactly does
/// a dotted-suffix match apply (so `post` finds `Client.post`, but a repo
/// with two literal `to_dict` free functions reports *those* two, not every
/// `.to_dict` besides); only when neither is unique does a case-insensitive
/// substring of the qname apply (TKI-51) — the most forgiving tier, tried
/// last so `stream` still lands on the one symbol literally named `stream`
/// before it starts matching every `ResponseStream`/`AsyncByteStream` that
/// merely contains the letters. Each tier stops at its first hit if that hit
/// is unique; a tier with two or more candidates is reported as ambiguous
/// immediately rather than falling through to a laxer tier's false
/// uniqueness.
fn resolve_name(symbols: &[SymbolPrint], query: &str) -> Result<usize> {
    let exact: Vec<usize> = symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.sym.qname == query)
        .map(|(i, _)| i)
        .collect();
    match exact.len() {
        1 => return Ok(exact[0]),
        n if n > 1 => return Err(ambiguous(symbols, exact, query)),
        _ => {}
    }

    let suffix = format!(".{query}");
    let suffixed: Vec<usize> = symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.sym.qname.ends_with(&suffix))
        .map(|(i, _)| i)
        .collect();
    match suffixed.len() {
        1 => return Ok(suffixed[0]),
        n if n > 1 => return Err(ambiguous(symbols, suffixed, query)),
        _ => {}
    }

    // Case-folded on both sides so `Stream` and `stream` reach the same
    // candidates; an exact case-sensitive match above already returns before
    // this tier runs, so case-insensitivity here can never steal a precise
    // match (it only ever widens what an otherwise-failed lookup finds).
    let needle = query.to_lowercase();
    let substr: Vec<usize> = symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.sym.qname.to_lowercase().contains(&needle))
        .map(|(i, _)| i)
        .collect();
    match substr.len() {
        0 => bail!("no symbol matches {query:?}"),
        1 => Ok(substr[0]),
        _ => Err(ambiguous(symbols, substr, query)),
    }
}

/// Resolve `file:line` by containment: any line inside the symbol's
/// fingerprinted byte span (decorators included, matching
/// `parse::extract_functions`'s `root`), not just its `def` line — so a
/// stack-trace or `rg` hit anywhere in the body resolves. `file` may be an
/// exact scanned path or a path suffix (`_client.py` matching
/// `httpx/_client.py`). Nested containment within one file resolves to the
/// innermost (smallest-span) symbol; containment spanning distinct files
/// (an ambiguous path suffix) is reported as ambiguous, not guessed at.
fn resolve_file_line(
    root: &Path,
    symbols: &[SymbolPrint],
    file_query: &str,
    line: usize,
) -> Result<usize> {
    let exact_file: Vec<usize> = symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.sym.file == file_query)
        .map(|(i, _)| i)
        .collect();
    let file_matches: Vec<usize> = if !exact_file.is_empty() {
        exact_file
    } else {
        let suffix = format!("/{file_query}");
        symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| s.sym.file.ends_with(&suffix))
            .map(|(i, _)| i)
            .collect()
    };
    if file_matches.is_empty() {
        bail!("no scanned file matches {file_query:?}");
    }

    let mut by_file: HashMap<&str, Vec<usize>> = HashMap::new();
    for &i in &file_matches {
        by_file.entry(symbols[i].sym.file.as_str()).or_default().push(i);
    }

    let mut containing: Vec<(usize, usize)> = Vec::new(); // (symbol idx, span size in lines)
    for idxs in by_file.values() {
        let file = &symbols[idxs[0]].sym.file;
        let source = fs::read(root.join(file))
            .with_context(|| format!("cannot read {file} to resolve {file_query}:{line}"))?;
        let line_of = |byte: usize| {
            1 + source[..byte.min(source.len())]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
        };
        for &i in idxs {
            let (start, end) = (line_of(symbols[i].span.0), line_of(symbols[i].span.1));
            if start <= line && line <= end {
                containing.push((i, end - start));
            }
        }
    }

    if containing.is_empty() {
        bail!("no symbol in {file_query:?} contains line {line}");
    }
    let distinct_files: HashSet<&str> = containing
        .iter()
        .map(|&(i, _)| symbols[i].sym.file.as_str())
        .collect();
    if distinct_files.len() > 1 {
        let ids = containing.iter().map(|&(i, _)| i).collect();
        return Err(ambiguous(symbols, ids, &format!("{file_query}:{line}")));
    }
    containing.sort_by_key(|&(i, size)| (size, i));
    Ok(containing[0].0)
}

/// Ambiguity is helpful, never a dead end: every candidate this tier matched
/// is ranked non-test symbols first, then by descending `node_count` (the
/// more likely targets first), capped at 12 rows so a corpus-wide substring
/// hit doesn't dump hundreds of lines. Every row prints `qname  file:line` —
/// the file:line half is always copy-pasteable as the next `explain` target,
/// even when the qname alone is still ambiguous.
fn ambiguous(symbols: &[SymbolPrint], mut ids: Vec<usize>, query: &str) -> anyhow::Error {
    const CAP: usize = 12;
    ids.sort_by_key(|&i| {
        let s = &symbols[i];
        (s.is_test, Reverse(s.node_count), s.sym.file.clone(), s.sym.line)
    });
    let total = ids.len();
    let mut msg = format!("{query:?} is ambiguous \u{2014} {total} candidates:\n");
    for &i in ids.iter().take(CAP) {
        let s = &symbols[i].sym;
        msg.push_str(&format!("  {}  {}:{}\n", s.qname, s.file, s.line));
    }
    if total > CAP {
        msg.push_str(&format!("  +{} more \u{2014} narrow the pattern\n", total - CAP));
    }
    anyhow::anyhow!(msg.trim_end().to_string())
}

// ── direct call edges (mirrors callrel.rs's join; see module doc) ──

/// qname's final `.`-segment (mirrors `callrel::base_name` /
/// `family::base_name` — each module keeps its own copy rather than a shared
/// export, per this codebase's established precedent). `pub(crate)` since
/// TKI-61: the explore viewer's identifier links must apply the *same*
/// resolution discipline as this card's callers, so the gates are shared
/// rather than re-derived (a second copy of `PY_AMBIENT_NAMES` would drift).
pub(crate) fn base_name(qname: &str) -> &str {
    qname.rsplit('.').next().unwrap_or(qname)
}

/// qname's class segment when qname is that class's `__init__` (mirrors
/// `callrel::class_of_init`).
pub(crate) fn class_of_init(qname: &str) -> Option<&str> {
    let (class, method) = qname.rsplit_once('.')?;
    (method == "__init__").then_some(class)
}

/// A file's dotted module path components (mirrors `callrel::module_components`).
pub(crate) fn module_components(file: &str) -> Vec<&str> {
    let stem = file.strip_suffix(".py").unwrap_or(file);
    let mut comps: Vec<&str> = stem.split('/').collect();
    if comps.last() == Some(&"__init__") {
        comps.pop();
    }
    comps
}

/// Whether `file` provides the dotted module path `m` (mirrors
/// `callrel::index_module_suffixes`'s lookup, recomputed directly here since
/// that index isn't part of `CallGraph`'s public surface).
pub(crate) fn module_matches(file: &str, m: &str) -> bool {
    let comps = module_components(file);
    (0..comps.len()).any(|start| comps[start..].join(".") == m)
}

/// Whether a recorded call `c` (from some symbol's `SymbolPrint::calls`)
/// targets `callee` — the same base-name (+ import-module) join
/// `callrel::build` performs, re-derived per-symbol here.
fn call_targets(callee: &SymbolPrint, c: &Call) -> bool {
    let base = c.base.as_str();
    let name_matches = (base_name(&callee.sym.qname) == base && !is_dunder(base))
        || class_of_init(&callee.sym.qname) == Some(base);
    if !name_matches {
        return false;
    }
    match &c.module {
        None => true,
        Some(ModuleRef::Absolute(m)) => module_matches(&callee.sym.file, m),
    }
}

/// How many corpus symbols a bare name `n` could mean — a free function/method
/// named `n`, or a class `n` via its `__init__`. The same worst-case set
/// `call_targets`'s `None`-module fallback matches against.
pub(crate) fn base_name_counts(symbols: &[SymbolPrint]) -> HashMap<&str, u32> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for s in symbols {
        *counts.entry(base_name(&s.sym.qname)).or_insert(0) += 1;
        if let Some(class) = class_of_init(&s.sym.qname) {
            *counts.entry(class).or_insert(0) += 1;
        }
    }
    counts
}

/// Names that already mean something on Python's built-in types, builtins,
/// or the ambient stdlib objects present in effectively every Python file
/// (`", ".join(…)`, `d.items()`, `open(…)`, `logger.error(…)`,
/// `pattern.match(…)`): an unresolved cross-file call to one of these is
/// never evidence of an edge to a same-named corpus symbol, even when that
/// symbol is the corpus's only definition — corpus-uniqueness can't see
/// the standard library. Found by TKI-54's guide panel: httpx ranked
/// `URL.join` its most-called symbol (27 callers; the code has two real
/// `url.join(…)` sites, the rest were `str.join`), and scrapy ranked
/// `CurlParser.error` fourth (32 callers; all were `logger.error`).
/// Same-file calls stay ungated, like `is_real_edge`.
const PY_AMBIENT_NAMES: &[&str] = &[
    // str / bytes
    "join", "split", "rsplit", "splitlines", "strip", "lstrip", "rstrip", "startswith",
    "endswith", "find", "rfind", "index", "rindex", "replace", "encode", "decode", "format",
    "format_map", "lower", "upper", "title", "capitalize", "casefold", "center", "ljust",
    "rjust", "zfill", "count", "partition", "rpartition", "translate", "expandtabs",
    // dict / list / set
    "keys", "values", "items", "get", "setdefault", "update", "pop", "popitem", "clear",
    "copy", "fromkeys", "append", "extend", "insert", "remove", "sort", "reverse", "add",
    "discard", "union", "intersection", "difference",
    // file-like
    "read", "write", "close", "readline", "readlines", "writelines", "flush", "seek", "tell",
    // logging.Logger
    "debug", "info", "warning", "warn", "error", "exception", "critical", "log",
    // re.Pattern
    "match", "fullmatch", "search", "sub", "subn", "findall", "finditer",
    // builtin functions
    "open", "print", "len", "iter", "next", "filter", "map", "sorted", "reversed",
    "enumerate", "zip", "range", "isinstance", "issubclass", "getattr", "setattr", "hasattr",
    "repr", "hash", "type", "super", "min", "max", "sum", "abs", "round", "any", "all",
    "compile",
];

pub(crate) fn is_ambient_name(name: &str) -> bool {
    PY_AMBIENT_NAMES.contains(&name)
}

/// Dunder call bases (`super().__init__()`, `x.__enter__()`) never bind a
/// call edge BY NAME: `super().__init__()` targets the parent class, and
/// attributing it to every same-file `__init__` handed each exception
/// class in httpx's `_exceptions.py` thirteen fake callers (TKI-54, found
/// by the guide panel — the real signal, `raise InvalidURL(…)`, flows
/// through the class-name constructor path and still counts).
pub(crate) fn is_dunder(name: &str) -> bool {
    name.starts_with("__") && name.ends_with("__")
}

/// `call_targets`, gated against the generic-verb collision documented in
/// `R&D archive validation/explain-eval.md` (module doc, "Generic-verb caller/callee
/// noise"): a call with no resolvable import module is trustworthy evidence
/// of a real edge to a symbol in *another* file only when its base name is
/// corpus-unique (nothing else it could mean) AND not an ambient Python name
/// (`PY_AMBIENT_NAMES` — something else it always could mean); otherwise
/// that cross-file unresolved match is excluded here rather than counted.
/// A same-file candidate is never gated — an unqualified call resolves in
/// its own file's scope first (Python's own name-resolution order), which
/// is exactly why `resolve_bare`/`resolve_attr` (`normalize.rs`) leave
/// same-file callees as `module: None` to begin with: nothing to statically
/// import when the callee already lives alongside the caller. This is what
/// keeps a symbol's own intra-file callers (a real, common case) from being
/// swept up with the cross-file coincidences (`client.post(...)`,
/// `os.environ.get(...)`) the noise report is about.
fn is_real_edge(caller_file: &str, callee: &SymbolPrint, c: &Call, name_counts: &HashMap<&str, u32>) -> bool {
    if c.module.is_none()
        && caller_file != callee.sym.file
        && (name_counts.get(c.base.as_str()).copied().unwrap_or(0) >= 2
            || is_ambient_name(c.base.as_str()))
    {
        return false;
    }
    call_targets(callee, c)
}

// ── entry-point tag (salvaged from R&D archive spike/orient; see module doc) ──

/// `file`'s containing directory — the deterministic, embedding-free
/// substitute R&D archive spike/orient's own salvage note names for its k-means
/// "concept" grouping (`R&D archive spike/orient/RESULTS.md`: "anchor concepts on
/// directories ... rather than on embedding clusters").
fn dir_of(file: &str) -> &str {
    file.rsplit_once('/').map(|(d, _)| d).unwrap_or(".")
}

/// Import-aware call-graph in-degree per symbol, corpus-wide. Ported from
/// R&D archive spike/orient/src/main.rs's `indegree` (same base-name/import-module join
/// as `call_targets` above, restated as a whole-corpus hashmap pass since
/// every symbol's count is needed here, not just one target's), with two
/// gates the card's own caller pass already applies — the tag and the
/// `callers` line must not contradict each other on the same screen:
/// - test symbols don't count, as callers or as targets (the salvage note's
///   own prescription: "filter is_test symbols first");
/// - an unresolved cross-file call counts only when its base name is
///   corpus-unique AND not an ambient Python name (the `is_real_edge`
///   gates) — otherwise a symbol named `get` inherits every `x.get(...)`
///   in the corpus, and a corpus-unique `join` every `", ".join(...)`,
///   as fake in-degree.
///
/// Public (TKI-47) so `explore` can compute it once per server lifetime
/// instead of once per card request; `card` takes the result as a parameter
/// for the same reason.
pub fn indegree(symbols: &[SymbolPrint]) -> Vec<u32> {
    let name_counts = base_name_counts(symbols);
    let mut by_name: HashMap<&str, Vec<u32>> = HashMap::new();
    let mut class_init: HashMap<&str, Vec<u32>> = HashMap::new();
    let mut by_name_file: HashMap<(&str, &str), Vec<u32>> = HashMap::new();
    let mut class_init_file: HashMap<(&str, &str), Vec<u32>> = HashMap::new();
    let mut module_files: HashMap<String, Vec<&str>> = HashMap::new();
    let mut seen_files: HashSet<&str> = HashSet::new();

    for (i, s) in symbols.iter().enumerate() {
        let file = s.sym.file.as_str();
        if seen_files.insert(file) {
            let comps = module_components(file);
            for start in 0..comps.len() {
                module_files.entry(comps[start..].join(".")).or_default().push(file);
            }
        }
        if s.is_test {
            continue;
        }
        let bn = base_name(&s.sym.qname);
        // dunder names never bind by name (see `is_dunder`); constructors
        // still receive edges through the class-name maps below
        if !is_dunder(bn) {
            by_name.entry(bn).or_default().push(i as u32);
            by_name_file.entry((file, bn)).or_default().push(i as u32);
        }
        if let Some(class) = class_of_init(&s.sym.qname) {
            class_init.entry(class).or_default().push(i as u32);
            class_init_file.entry((file, class)).or_default().push(i as u32);
        }
    }

    let mut indeg = vec![0u32; symbols.len()];
    for (i, s) in symbols.iter().enumerate() {
        if s.is_test {
            continue;
        }
        let file = s.sym.file.as_str();
        // dedup targets per caller so one caller counts at most once per callee
        let mut targets: HashSet<u32> = HashSet::new();
        for c in &s.calls {
            let base = c.base.as_str();
            match &c.module {
                None => {
                    // Same-file candidates always count (own-scope
                    // resolution, same reasoning as `is_real_edge`)…
                    if let Some(t) = by_name_file.get(&(file, base)) {
                        targets.extend(t.iter().copied());
                    }
                    if let Some(t) = class_init_file.get(&(file, base)) {
                        targets.extend(t.iter().copied());
                    }
                    // …cross-file unresolved only when corpus-unique AND
                    // not an ambient Python name (mirrors `is_real_edge`).
                    if name_counts.get(base).copied().unwrap_or(0) < 2 && !is_ambient_name(base) {
                        if let Some(t) = by_name.get(base) {
                            targets.extend(t.iter().copied());
                        }
                        if let Some(t) = class_init.get(base) {
                            targets.extend(t.iter().copied());
                        }
                    }
                }
                Some(ModuleRef::Absolute(m)) => {
                    if let Some(files) = module_files.get(m.as_str()) {
                        for f in files {
                            if let Some(t) = by_name_file.get(&(*f, base)) {
                                targets.extend(t.iter().copied());
                            }
                            if let Some(t) = class_init_file.get(&(*f, base)) {
                                targets.extend(t.iter().copied());
                            }
                        }
                    }
                }
            }
        }
        targets.remove(&(i as u32));
        for t in targets {
            indeg[t as usize] += 1;
        }
    }
    indeg
}

/// Directed direct-call adjacency over the whole corpus, `is_real_edge`-
/// gated (TKI-72): `out[i]` are the symbols `i` calls directly, exactly the
/// edges `card`'s callers/callees pass lists — the map's "calls" channel
/// and the panel on the same screen must give one answer to "who calls
/// this" (the same contract `indegree`'s doc states for the entry tag).
/// Unlike `indegree`, test symbols keep their edges: the card lists them
/// and the map draws them. Restated as an indexed pass because the per-
/// target O(n·calls) loop in `card` is fine for one card but not for a
/// whole-corpus channel. Each list is sorted ascending and deduped, so the
/// output is a pure, byte-stable function of the scan.
pub fn call_edges(symbols: &[SymbolPrint]) -> Vec<Vec<u32>> {
    let name_counts = base_name_counts(symbols);
    let mut by_name: HashMap<&str, Vec<u32>> = HashMap::new();
    let mut class_init: HashMap<&str, Vec<u32>> = HashMap::new();
    let mut by_name_file: HashMap<(&str, &str), Vec<u32>> = HashMap::new();
    let mut class_init_file: HashMap<(&str, &str), Vec<u32>> = HashMap::new();
    let mut module_files: HashMap<String, Vec<&str>> = HashMap::new();
    let mut seen_files: HashSet<&str> = HashSet::new();

    for (i, s) in symbols.iter().enumerate() {
        let file = s.sym.file.as_str();
        if seen_files.insert(file) {
            let comps = module_components(file);
            for start in 0..comps.len() {
                module_files.entry(comps[start..].join(".")).or_default().push(file);
            }
        }
        let bn = base_name(&s.sym.qname);
        // dunder names never bind by name (`is_dunder`, mirrored from
        // `call_targets`); constructors still bind via the class-name maps
        if !is_dunder(bn) {
            by_name.entry(bn).or_default().push(i as u32);
            by_name_file.entry((file, bn)).or_default().push(i as u32);
        }
        if let Some(class) = class_of_init(&s.sym.qname) {
            class_init.entry(class).or_default().push(i as u32);
            class_init_file.entry((file, class)).or_default().push(i as u32);
        }
    }

    let mut out: Vec<Vec<u32>> = vec![Vec::new(); symbols.len()];
    for (i, s) in symbols.iter().enumerate() {
        let file = s.sym.file.as_str();
        let mut targets: HashSet<u32> = HashSet::new();
        for c in &s.calls {
            let base = c.base.as_str();
            match &c.module {
                None => {
                    // Same-file candidates always count; cross-file
                    // unresolved only when corpus-unique AND not ambient —
                    // the `is_real_edge` gates, in index form.
                    if let Some(t) = by_name_file.get(&(file, base)) {
                        targets.extend(t.iter().copied());
                    }
                    if let Some(t) = class_init_file.get(&(file, base)) {
                        targets.extend(t.iter().copied());
                    }
                    if name_counts.get(base).copied().unwrap_or(0) < 2 && !is_ambient_name(base) {
                        if let Some(t) = by_name.get(base) {
                            targets.extend(t.iter().copied());
                        }
                        if let Some(t) = class_init.get(base) {
                            targets.extend(t.iter().copied());
                        }
                    }
                }
                Some(ModuleRef::Absolute(m)) => {
                    if let Some(files) = module_files.get(m.as_str()) {
                        for f in files {
                            if let Some(t) = by_name_file.get(&(*f, base)) {
                                targets.extend(t.iter().copied());
                            }
                            if let Some(t) = class_init_file.get(&(*f, base)) {
                                targets.extend(t.iter().copied());
                            }
                        }
                    }
                }
            }
        }
        targets.remove(&(i as u32));
        let mut v: Vec<u32> = targets.into_iter().collect();
        v.sort_unstable();
        out[i] = v;
    }
    out
}

/// Whether `target` is its directory's "entry" — the highest import-aware
/// in-degree member among the directory's non-test symbols (R&D archive spike/orient's
/// salvaged signal; tie-break ported verbatim: higher `node_count`, then
/// lower index). `None` when `target` is a test symbol, isn't that
/// directory's champion, or nothing in the directory is reliably called.
/// `indeg` is `indegree(symbols)`, passed in rather than recomputed.
fn entry_point_tag(symbols: &[SymbolPrint], indeg: &[u32], target: usize) -> Option<u32> {
    if symbols[target].is_test {
        return None;
    }
    let dir = dir_of(&symbols[target].sym.file);
    let champion = (0..symbols.len())
        .filter(|&i| !symbols[i].is_test && dir_of(&symbols[i].sym.file) == dir)
        .max_by(|&a, &b| {
            indeg[a]
                .cmp(&indeg[b])
                .then(symbols[a].node_count.cmp(&symbols[b].node_count))
                .then(b.cmp(&a)) // lower index wins
        })?;
    (champion == target && indeg[target] > 0).then_some(indeg[target])
}

// ── the card's data (computed once; rendered by text and JSON alike) ──

/// The competing-patterns group `target` belongs to, seen from `target`.
pub struct CardTwins {
    /// The other members, sorted by (file, line).
    pub members: Vec<usize>,
    pub b_max: f32,
    pub shared_terms: Vec<String>,
}

pub struct CardFamily {
    /// 0-based family index (rendered as `F{index+1}`).
    pub index: usize,
    pub is_core: bool,
    pub cos_to_core: f32,
    /// Total family membership (this symbol included) — prevalence of the
    /// broader pattern, not just this symbol's own core/drift status.
    pub members: u32,
    pub n_files: usize,
}

/// Corpus-grounded prevalence (TKI-74): how common this symbol's shape and
/// name are, so the reader can weigh everything else on the card against a
/// base rate instead of reading each finding in isolation. Both counts
/// include `target` itself (a lone symbol is "1 member" of its own shape,
/// not zero) — every number here is hand-verifiable: `shape_members`/
/// `shape_files` against the same repeated cluster `clones` is read from,
/// `name_count` by grepping `def {name_key}` across the corpus.
pub struct CardFacts {
    /// Size of `target`'s repeated-shape cluster (`analysis.repeated`); 1
    /// when the shape appears nowhere else in the corpus.
    pub shape_members: u32,
    /// Distinct files that cluster spans; 1 alongside `shape_members == 1`.
    pub shape_files: u32,
    /// The name actually counted: the class name for an `__init__` (mirrors
    /// `call_targets`'s constructor join — "`__init__`" itself is near-
    /// universal and not a meaningful prevalence signal), else the base name.
    pub name_key: String,
    /// How many corpus symbols (any file) are defined under `name_key`;
    /// includes `target`. 1 means the name is corpus-unique.
    pub name_count: u32,
}

/// Everything the explain card states about one symbol — the same facts
/// whether rendered as the CLI's text card or `explore`'s JSON side panel
/// (TKI-47). Pure data; ordering inside each list is already the rendered
/// ordering.
pub struct Card {
    pub target: usize,
    pub dating: Option<history::ClusterDates>,
    /// `Some(n)` when this symbol is its directory's entry (in-degree n).
    pub entry: Option<u32>,
    pub facts: CardFacts,
    /// Same tight cluster, identical Merkle root; sorted by (file, line).
    pub exact_clones: Vec<usize>,
    /// Same tight cluster, different root, with Channel-A cosine; sorted by
    /// cosine descending, then (file, line).
    pub near_clones: Vec<(usize, f32)>,
    pub twins: Option<CardTwins>,
    /// Direct call edges (`is_real_edge`-gated); sorted by (file, line).
    pub callers: Vec<usize>,
    pub callees: Vec<usize>,
    pub family: Option<CardFamily>,
}

/// Read `target`'s card off the analysis. `indeg` is `indegree(symbols)` —
/// a parameter so a long-lived caller (`explore`'s server) computes it once.
pub fn card(analysis: &Analysis, indeg: &[u32], target: usize) -> Card {
    let symbols = &analysis.scanned.symbols;

    let dating = analysis
        .scanned
        .history
        .as_ref()
        .and_then(|h| history::cluster_dating(symbols, &[target as u32], h.anchor));
    let entry = entry_point_tag(symbols, indeg, target);

    // `base_name_counts` is needed both for the corpus-name prevalence fact
    // below and for the callers/callees noise gate further down — computed
    // once and shared, per this module's own precedent (`indeg` above).
    let name_counts = base_name_counts(symbols);

    // Clones: this symbol's tight repeated-shape cluster, split exact
    // (identical Merkle root) vs near (same cluster, different root).
    let mut exact: Vec<usize> = Vec::new();
    let mut near: Vec<(usize, f32)> = Vec::new();
    let mut shape_members: u32 = 1;
    let mut shape_files: u32 = 1;
    if let Some(cluster) = analysis
        .repeated
        .iter()
        .find(|c| c.members.contains(&(target as u32)))
    {
        shape_members = cluster.members.len() as u32;
        shape_files = cluster
            .members
            .iter()
            .map(|&m| symbols[m as usize].sym.file.as_str())
            .collect::<HashSet<_>>()
            .len() as u32;
        let target_merkle = symbols[target].merkle_root;
        for &m in &cluster.members {
            let m = m as usize;
            if m == target {
                continue;
            }
            if symbols[m].merkle_root == target_merkle {
                exact.push(m);
            } else {
                near.push((m, fingerprint::cosine(&symbols[target].wl, &symbols[m].wl)));
            }
        }
        exact.sort_by_key(|&i| (symbols[i].sym.file.clone(), symbols[i].sym.line));
        near.sort_by(|a, b| {
            b.1.total_cmp(&a.1).then_with(|| {
                (symbols[a.0].sym.file.clone(), symbols[a.0].sym.line)
                    .cmp(&(symbols[b.0].sym.file.clone(), symbols[b.0].sym.line))
            })
        });
    }

    // Role twins: the competing-patterns group this symbol belongs to.
    // Already excludes caller/callee pairs — `queries::competing` suppresses
    // those via `callrel::CallGraph` before a group ever forms.
    let twins = analysis
        .competing
        .groups
        .iter()
        .find(|g| g.members.contains(&(target as u32)))
        .map(|g| {
            let mut others: Vec<usize> = g
                .members
                .iter()
                .copied()
                .filter(|&m| m as usize != target)
                .map(|m| m as usize)
                .collect();
            others.sort_by_key(|&i| (symbols[i].sym.file.clone(), symbols[i].sym.line));
            CardTwins {
                members: others,
                b_max: g.b_max,
                shared_terms: g.shared_terms.clone(),
            }
        });

    // Callers / callees: direct call edges only (see module doc).
    // `is_real_edge` additionally excludes unresolved-module calls whose
    // base name isn't corpus-unique — the generic-verb noise fix.
    let s_file = symbols[target].sym.file.as_str();
    let (mut callers, mut callees) = (Vec::new(), Vec::new());
    for (i, other) in symbols.iter().enumerate() {
        if i == target {
            continue;
        }
        if symbols[target]
            .calls
            .iter()
            .any(|c| is_real_edge(s_file, other, c, &name_counts))
        {
            callees.push(i);
        }
        if other
            .calls
            .iter()
            .any(|c| is_real_edge(&other.sym.file, &symbols[target], c, &name_counts))
        {
            callers.push(i);
        }
    }
    callers.sort_by_key(|&i| (symbols[i].sym.file.clone(), symbols[i].sym.line));
    callees.sort_by_key(|&i| (symbols[i].sym.file.clone(), symbols[i].sym.line));

    let family = analysis
        .families
        .families
        .iter()
        .enumerate()
        .find_map(|(fi, f)| {
            f.members
                .iter()
                .find(|m| m.sym == target as u32)
                .map(|m| CardFamily {
                    index: fi,
                    is_core: m.is_core,
                    cos_to_core: m.cos_to_core,
                    members: f.members.len() as u32,
                    n_files: f.n_files,
                })
        });

    // Name prevalence: the class name for an `__init__` (mirrors
    // `call_targets`'s own constructor join), else the base name.
    let target_qname = &symbols[target].sym.qname;
    let name_key = class_of_init(target_qname).unwrap_or_else(|| base_name(target_qname));
    let name_count = name_counts.get(name_key).copied().unwrap_or(1);
    let facts = CardFacts {
        shape_members,
        shape_files,
        name_key: name_key.to_string(),
        name_count,
    };

    Card {
        target,
        dating,
        entry,
        facts,
        exact_clones: exact,
        near_clones: near,
        twins,
        callers,
        callees,
        family,
    }
}

// ── rendering ──

fn fmt_named_list(symbols: &[SymbolPrint], ids: &[usize], show: usize) -> String {
    let items: Vec<String> = ids
        .iter()
        .take(show)
        .map(|&i| {
            let s = &symbols[i].sym;
            format!("{} {}:{}", base_name(&s.qname), s.file, s.line)
        })
        .collect();
    let mut out = items.join(", ");
    if ids.len() > show {
        out.push_str(&format!(", +{} more", ids.len() - show));
    }
    out
}

fn fmt_near_list(symbols: &[SymbolPrint], items: &[(usize, f32)], show: usize) -> String {
    let parts: Vec<String> = items
        .iter()
        .take(show)
        .map(|&(i, c)| {
            let s = &symbols[i].sym;
            format!("{} {}:{} ({c:.2})", base_name(&s.qname), s.file, s.line)
        })
        .collect();
    let mut out = parts.join(", ");
    if items.len() > show {
        out.push_str(&format!(", +{} more", items.len() - show));
    }
    out
}

fn fmt_bare_list(symbols: &[SymbolPrint], ids: &[usize], show: usize) -> String {
    let items: Vec<&str> = ids
        .iter()
        .take(show)
        .map(|&i| base_name(&symbols[i].sym.qname))
        .collect();
    let mut out = items.join(", ");
    if ids.len() > show {
        out.push_str(" \u{2026}");
    }
    out
}

/// A caller/callee count: bare `0` when empty (no dangling separator), else
/// `N — name, name …`.
fn fmt_count(symbols: &[SymbolPrint], ids: &[usize]) -> String {
    if ids.is_empty() {
        "0".to_string()
    } else {
        format!("{} \u{2014} {}", ids.len(), fmt_bare_list(symbols, ids, 2))
    }
}

fn render_card(analysis: &Analysis, cfg: &Config, target: usize) -> Result<()> {
    let symbols = &analysis.scanned.symbols;
    let s = &symbols[target].sym;
    let indeg = indegree(symbols);
    let data = card(analysis, &indeg, target);

    // ── header ──
    let dating = data
        .dating
        .as_ref()
        .map(|cd| {
            format!(
                " \u{b7} {} \u{2192} {} \u{b7} {}",
                history::fmt_date(cd.first_seen),
                history::fmt_date(cd.last_touched),
                cd.activity.label()
            )
        })
        .unwrap_or_default();
    let entry_tag = data
        .entry
        .map(|n| format!(" \u{b7} entry {n}\u{d7}"))
        .unwrap_or_default();
    println!(
        "{}:{}  {}  ({} nodes{}{})",
        s.file, s.line, s.qname, symbols[target].node_count, entry_tag, dating
    );

    // ── facts: corpus-grounded prevalence — always printed, like role
    // twins/callers/callees below (a base rate of 1 is itself a fact) ──
    let files_word = |n: u32| if n == 1 { "file" } else { "files" };
    let mut fact_parts = vec![
        if data.facts.shape_members > 1 {
            format!(
                "shape {} members across {} {}",
                data.facts.shape_members,
                data.facts.shape_files,
                files_word(data.facts.shape_files)
            )
        } else {
            "shape corpus-unique".to_string()
        },
        if data.facts.name_count > 1 {
            format!(
                "name {:?} shared by {} corpus symbols",
                data.facts.name_key, data.facts.name_count
            )
        } else {
            format!("name {:?} corpus-unique", data.facts.name_key)
        },
    ];
    if let Some(f) = &data.family {
        fact_parts.push(format!(
            "family F{} {} members across {} {}",
            f.index + 1,
            f.members,
            f.n_files,
            files_word(f.n_files as u32)
        ));
    }
    println!("facts       {}", fact_parts.join(" \u{b7} "));

    // ── clones: omitted entirely when the symbol has none ──
    let mut parts = Vec::new();
    if !data.exact_clones.is_empty() {
        parts.push(format!(
            "exact: {}",
            fmt_named_list(symbols, &data.exact_clones, 2)
        ));
    }
    if !data.near_clones.is_empty() {
        parts.push(format!(
            "near: {}",
            fmt_near_list(symbols, &data.near_clones, 2)
        ));
    }
    if !parts.is_empty() {
        println!("clones      {}", parts.join(" \u{b7} "));
    }

    // ── role twins: always printed — `none` is itself a fact ──
    match &data.twins {
        Some(t) => println!(
            "role twins  (vocab \u{2265}{:.2}, different shape): {} \u{b7} B={:.2} shared: {}",
            cfg.theta_b,
            fmt_named_list(symbols, &t.members, 2),
            t.b_max,
            t.shared_terms.join(",")
        ),
        None => println!(
            "role twins  (vocab \u{2265}{:.2}, different shape): none",
            cfg.theta_b
        ),
    }

    // ── callers / callees ──
    println!(
        "callers     {}; callees {}  [import-aware]",
        fmt_count(symbols, &data.callers),
        fmt_count(symbols, &data.callees),
    );

    // ── family: membership only, omitted entirely when absent — mirrors
    // TKI-35's demotion of family findings from default surfaces ──
    if let Some(f) = &data.family {
        println!(
            "family      F{} member ({}, cos {:.2}) [experimental]",
            f.index + 1,
            if f.is_core { "core" } else { "drift" },
            f.cos_to_core
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SymbolRef;

    /// A base-name (unresolvable) call (mirrors `callrel::tests::call` —
    /// each module keeps its own copy, per this codebase's precedent).
    fn call(base: &str) -> Call {
        Call { base: base.into(), module: None }
    }

    fn mcall(base: &str, module: &str) -> Call {
        Call { base: base.into(), module: Some(ModuleRef::Absolute(module.into())) }
    }

    fn sym_at(file: &str, qname: &str, calls: HashSet<Call>) -> SymbolPrint {
        SymbolPrint {
            sym: SymbolRef { file: file.into(), qname: qname.into(), line: 1 },
            span: (0, 0),
            node_count: 0,
            merkle_root: 0,
            wl: Vec::new(),
            minhash: Vec::new(),
            vocab_tf: HashMap::new(),
            calls,
            is_test: false,
            dating: None,
        }
    }

    fn sym(file: &str, qname: &str, calls: &[&str]) -> SymbolPrint {
        sym_at(file, qname, calls.iter().map(|s| call(s)).collect())
    }

    // ── call_edges (TKI-72): the map channel's adjacency must reproduce
    // the card's own `is_real_edge` semantics, indexed ──

    #[test]
    fn call_edges_are_directed() {
        let symbols = vec![
            sym("a.py", "fetch_wrap", &["fetch"]),
            sym("a.py", "fetch", &[]),
        ];
        let out = call_edges(&symbols);
        assert_eq!(out[0], vec![1], "caller gets the out-edge");
        assert!(out[1].is_empty(), "callee gets none — edges are directed");
    }

    #[test]
    fn cross_file_ambient_name_makes_no_edge() {
        // `logger.error(...)` in another file must not become an edge to a
        // corpus symbol named `error` (the CurlParser.error regression).
        let symbols = vec![
            sym("a.py", "parse", &["error"]),
            sym("b.py", "Handler.error", &[]),
        ];
        let out = call_edges(&symbols);
        assert!(out[0].is_empty(), "ambient base names never bind cross-file");
    }

    #[test]
    fn cross_file_non_unique_name_makes_no_edge() {
        let symbols = vec![
            sym("a.py", "caller", &["run"]),
            sym("b.py", "run", &[]),
            sym("c.py", "Job.run", &[]),
        ];
        let out = call_edges(&symbols);
        assert!(out[0].is_empty(), "two candidate `run`s: unresolved cross-file is noise");
    }

    #[test]
    fn same_file_call_stays_ungated() {
        // Same base name is non-unique corpus-wide, but an unqualified call
        // resolves in its own file first — the same-file edge must survive.
        let symbols = vec![
            sym("a.py", "caller", &["run"]),
            sym("a.py", "run", &[]),
            sym("b.py", "run", &[]),
        ];
        let out = call_edges(&symbols);
        assert_eq!(out[0], vec![1], "own-file candidate binds; the cross-file one doesn't");
    }

    #[test]
    fn import_resolved_call_reaches_that_module_only() {
        let symbols = vec![
            sym_at("app.py", "connection", [mcall("connect", "appdb.engine")].into_iter().collect()),
            sym_at("appdb/engine.py", "connect", HashSet::new()),
            sym_at("other/pkg.py", "connect", HashSet::new()),
        ];
        let out = call_edges(&symbols);
        assert_eq!(out[0], vec![1], "module-resolved call binds its module's symbol only");
    }

    #[test]
    fn dunder_call_never_binds_by_name() {
        let symbols = vec![
            sym("a.py", "Child.__init__", &["__init__"]), // super().__init__()
            sym("a.py", "Base.__init__", &[]),
        ];
        let out = call_edges(&symbols);
        assert!(out[0].is_empty(), "super().__init__() must not bind same-file __init__s");
    }

    #[test]
    fn constructor_call_binds_the_class_init() {
        let symbols = vec![
            sym("a.py", "build_widget", &["Widget"]),
            sym("a.py", "Widget.__init__", &[]),
        ];
        let out = call_edges(&symbols);
        assert_eq!(out[0], vec![1], "ClassName(...) reaches ClassName.__init__");
    }

    #[test]
    fn recursion_yields_no_self_edge_and_lists_sort() {
        let symbols = vec![
            sym("a.py", "walk", &["walk", "beta_step", "alpha_step"]),
            sym("a.py", "beta_step", &[]),
            sym("a.py", "alpha_step", &[]),
        ];
        let out = call_edges(&symbols);
        assert_eq!(out[0], vec![1, 2], "no self-edge; ids sorted ascending");
    }

    #[test]
    fn call_edges_match_the_cards_is_real_edge_pass() {
        // The channel and the card must be the same relation: brute-force
        // `is_real_edge` over every ordered pair equals the indexed pass.
        let symbols = vec![
            sym("a.py", "caller", &["run", "error", "helper"]),
            sym("a.py", "helper", &["run"]),
            sym("b.py", "run", &[]),
            sym("c.py", "Job.run", &[]),
            sym("c.py", "Handler.error", &[]),
            sym_at("app.py", "connection", [mcall("connect", "appdb.engine")].into_iter().collect()),
            sym_at("appdb/engine.py", "connect", HashSet::new()),
        ];
        let out = call_edges(&symbols);
        let name_counts = base_name_counts(&symbols);
        for (i, s) in symbols.iter().enumerate() {
            let brute: Vec<u32> = (0..symbols.len())
                .filter(|&j| j != i)
                .filter(|&j| {
                    s.calls.iter().any(|c| {
                        is_real_edge(&s.sym.file, &symbols[j], c, &name_counts)
                    })
                })
                .map(|j| j as u32)
                .collect();
            assert_eq!(out[i], brute, "indexed pass diverges from the card at {i}");
        }
    }
}
