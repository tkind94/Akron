//! Call-relation suppression for the competing-patterns query (DESIGN.md §3,
//! queries.rs `competing`): a wrapper shares vocabulary with the function it
//! wraps by construction (`fetch_atlas_tile` in wrapper_caller.py calling
//! `fetch_tile` in wrapper_callee.py — tests/fixtures), so caller/callee pairs clear the
//! high-B/low-A bar without being competitors. This module gives the
//! competing query a way to recognize and drop them.
//!
//! Matching is name-based over the callee identifiers collected during
//! normalization (`normalize.rs`), sharpened by **per-file import
//! resolution**. Two symbols are call-related if one calls the other, or if a
//! directed two-hop delegation chain connects them: X calls Z and Z calls Y
//! (in either starting direction). This is deliberately a *chain*, not "X and
//! Y both call Z" — two independent implementations that happen to share a
//! lower-level helper are competing siblings, not a wrapper pair, and must not
//! be conflated with one.
//!
//! **Import-precise resolution.** Each recorded call carries the module its
//! object segment resolves to, decided against the file's `import` bindings
//! (`normalize::collect_imports`):
//! - A call whose base resolves to a module **not present in the scanned
//!   corpus** (judged by repo-relative file layout — `psycopg.connect(...)`
//!   under `import psycopg`, no `psycopg` module in the tree) contributes **no
//!   edge**: it can no longer collide with a corpus symbol that merely shares
//!   the base name `connect`.
//! - A call whose base resolves to a **corpus module** (`from appdb.engine
//!   import connect` → `connect(...)`; `from ._transports import HTTPTransport`
//!   → `HTTPTransport(...)`) matches that module's symbols **specifically** —
//!   `connect` in `appdb/engine.py`, `HTTPTransport.__init__` in the transport
//!   module — not every same-base-name symbol anywhere.
//! - An **unresolvable** base (a local, a parameter, `self`/attribute chains,
//!   or a name that isn't an import binding) keeps the base-name join over the
//!   whole corpus — today's behavior. Resolution only ever makes edges *more
//!   precise*, never more numerous.
//!
//! **Constructor calls.** `ClassName(...)` is syntactically a call to a name
//! that never appears as any function's own base name (`__init__` is qualified
//! as `ClassName.__init__`), so it would fall through the base-name join
//! untouched — yet a builder that *constructs* an object shares that object's
//! vocabulary by construction (httpx's `Response.iter_bytes` constructing
//! `ByteChunker`; corpus-R's builders constructing the records they return) exactly
//! like a wrapper calling its callee. So a called name is also matched against
//! class-name segments of qnames: a call to `ByteChunker` relates the caller
//! to `ByteChunker.__init__` specifically — not blanket to every
//! `ByteChunker.*` method, which would over-suppress two genuinely competing
//! methods on the same class. When the constructed name is import-resolved the
//! match is scoped to that class's own module (and an aliased `import X as Y;
//! Y(...)` resolves to `X`'s `__init__`, which the raw base name `Y` would
//! miss). Reaching a *sibling* method still requires the existing two-hop chain
//! (caller → `__init__` → sibling), i.e. only when `__init__` itself calls it.
//!
//! Known limits, accepted for zero setup cost:
//! - **Variable-held callables**: a parameter or local holding a callable and
//!   invoked as `f(...)` / `cls(...)` looks identical to a call of a top-level
//!   `f`; the recorded callee is the *variable*, not what it holds (flask's
//!   `cls = self.test_cli_runner_class; cls(self, **kwargs)`), so the
//!   construction is invisible. Resolving it needs dataflow/type inference.
//! - **Same-name methods on different classes / sync-async twins**:
//!   `HTTPTransport.__init__` and `AsyncHTTPTransport.__init__` share the base
//!   name `__init__`, and httpx's sync/async transports have byte-identical
//!   signatures. A sync builder constructs only the sync transport, so the
//!   surviving competing edge is the *cross* pair (sync builder ↔ async
//!   `__init__`): a genuine coincidence with no call between them that import
//!   resolution cannot suppress — it needs type / twin-class awareness.
//! - Suppressed pairs are counted, never silently dropped
//!   (`CompetingFunnel::call_related` in queries.rs).

use crate::types::{ModuleRef, SymbolPrint};
use std::collections::{HashMap, HashSet};

/// qname's final `.`-segment, e.g. `"ProxyFetcher.fetch_page"` -> `"fetch_page"`.
fn base_name(qname: &str) -> &str {
    qname.rsplit('.').next().unwrap_or(qname)
}

/// qname's class segment when qname is that class's `__init__`, e.g.
/// `"ByteChunker.__init__"` -> `Some("ByteChunker")`. `None` for free
/// functions and for a class's non-constructor methods.
fn class_of_init(qname: &str) -> Option<&str> {
    let (class, method) = qname.rsplit_once('.')?;
    (method == "__init__").then_some(class)
}

/// Directed call adjacency over the corpus: `calls_out[i]` are the symbols
/// whose base name appears among symbol `i`'s collected callee names.
pub struct CallGraph {
    calls_out: Vec<Vec<u32>>,
}

/// A file's importable module path components: path split on `/`, `.py`
/// stripped, and a trailing `__init__` dropped (a package is its directory),
/// e.g. `packages/db/src/appdb/engine.py` → `[packages, db, src, appdb, engine]`.
fn module_components(file: &str) -> Vec<&str> {
    let stem = file.strip_suffix(".py").unwrap_or(file);
    let mut comps: Vec<&str> = stem.split('/').collect();
    if comps.last() == Some(&"__init__") {
        comps.pop();
    }
    comps
}

/// Index every dotted suffix of a file's module path to that file, so an
/// import `appdb.engine` (or a relative import resolved to its full path)
/// matches `.../appdb/engine.py` by layout, while a third-party `psycopg`
/// matches nothing and so resolves as external.
fn index_module_suffixes<'a>(file: &'a str, module_files: &mut HashMap<String, Vec<&'a str>>) {
    let comps = module_components(file);
    for start in 0..comps.len() {
        module_files
            .entry(comps[start..].join("."))
            .or_default()
            .push(file);
    }
}

fn push_targets(out: &mut Vec<u32>, from: usize, targets: Option<&Vec<u32>>) {
    if let Some(targets) = targets {
        out.extend(targets.iter().copied().filter(|&j| j as usize != from));
    }
}

pub fn build(symbols: &[SymbolPrint]) -> CallGraph {
    // Base-name indices — the fallback for unresolvable calls — plus the same
    // keyed additionally by defining file, for import-precise matches.
    let mut by_name: HashMap<&str, Vec<u32>> = HashMap::new();
    let mut class_init: HashMap<&str, Vec<u32>> = HashMap::new();
    let mut by_name_file: HashMap<(&str, &str), Vec<u32>> = HashMap::new();
    let mut class_init_file: HashMap<(&str, &str), Vec<u32>> = HashMap::new();
    // Dotted module suffix → files that provide it (the repo-relative layout).
    let mut module_files: HashMap<String, Vec<&str>> = HashMap::new();
    let mut seen_files: HashSet<&str> = HashSet::new();

    for (i, s) in symbols.iter().enumerate() {
        let file = s.sym.file.as_str();
        let bn = base_name(&s.sym.qname);
        by_name.entry(bn).or_default().push(i as u32);
        by_name_file.entry((file, bn)).or_default().push(i as u32);
        if let Some(class) = class_of_init(&s.sym.qname) {
            class_init.entry(class).or_default().push(i as u32);
            class_init_file
                .entry((file, class))
                .or_default()
                .push(i as u32);
        }
        if seen_files.insert(file) {
            index_module_suffixes(file, &mut module_files);
        }
    }

    let mut calls_out: Vec<Vec<u32>> = vec![Vec::new(); symbols.len()];
    for (i, s) in symbols.iter().enumerate() {
        let out = &mut calls_out[i];
        for c in &s.calls {
            let base = c.base.as_str();
            match &c.module {
                // Unresolvable: base-name join over the whole corpus. The
                // `class_init` half handles constructor calls (see module doc).
                None => {
                    push_targets(out, i, by_name.get(base));
                    push_targets(out, i, class_init.get(base));
                }
                // Resolved: a corpus module matches its own file's symbols
                // specifically; an external module (absent from the map)
                // contributes nothing, killing coincidental base-name edges.
                Some(ModuleRef::Absolute(m)) => {
                    if let Some(files) = module_files.get(m.as_str()) {
                        for f in files {
                            push_targets(out, i, by_name_file.get(&(*f, base)));
                            push_targets(out, i, class_init_file.get(&(*f, base)));
                        }
                    }
                }
            }
        }
    }
    CallGraph { calls_out }
}

impl CallGraph {
    /// Whether `x` and `y` are call-related: one calls the other directly,
    /// or a directed two-hop delegation chain connects them, starting from
    /// either side (`x` calls `z` calls `y`, or `y` calls `z` calls `x`).
    pub fn related(&self, x: u32, y: u32) -> bool {
        self.chains_to(x, y) || self.chains_to(y, x)
    }

    /// Directed reachability from `from` to `to` within two call hops.
    fn chains_to(&self, from: u32, to: u32) -> bool {
        if self.calls_out[from as usize].contains(&to) {
            return true;
        }
        self.calls_out[from as usize]
            .iter()
            .any(|&z| self.calls_out[z as usize].contains(&to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Call, SymbolRef};
    use std::collections::{HashMap as Map, HashSet};

    /// A base-name (unresolvable) call — today's whole-corpus join.
    fn call(base: &str) -> Call {
        Call {
            base: base.into(),
            module: None,
        }
    }

    /// An import-resolved call: `base` in module `module`.
    fn mcall(base: &str, module: &str) -> Call {
        Call {
            base: base.into(),
            module: Some(ModuleRef::Absolute(module.into())),
        }
    }

    fn sym_at(file: &str, qname: &str, calls: HashSet<Call>) -> SymbolPrint {
        SymbolPrint {
            sym: SymbolRef {
                file: file.into(),
                qname: qname.into(),
                line: 1,
            },
            span: (0, 0),
            node_count: 0,
            merkle_root: 0,
            wl: Vec::new(),
            minhash: Vec::new(),
            vocab_tf: Map::new(),
            calls,
            is_test: false,
            dating: None,
        }
    }

    fn sym(qname: &str, calls: &[&str]) -> SymbolPrint {
        sym_at("f.py", qname, calls.iter().map(|s| call(s)).collect())
    }

    #[test]
    fn direct_call_either_direction_is_related() {
        let symbols = vec![sym("fetch_wrap", &["fetch"]), sym("fetch", &[])];
        let g = build(&symbols);
        assert!(g.related(0, 1));
        assert!(g.related(1, 0));
    }

    #[test]
    fn unrelated_symbols_stay_unrelated() {
        let symbols = vec![sym("conn", &[]), sym("connection", &[])];
        let g = build(&symbols);
        assert!(!g.related(0, 1));
    }

    #[test]
    fn transitive_delegation_chain_is_related() {
        // a calls b, b calls c => a and c related through b (a chain), not directly.
        let symbols = vec![sym("a", &["b"]), sym("b", &["c"]), sym("c", &[])];
        let g = build(&symbols);
        assert!(g.related(0, 2));
        assert!(
            !g.calls_out[0].contains(&2),
            "should be transitive, not direct"
        );
    }

    #[test]
    fn constructor_call_relates_caller_to_that_classs_init() {
        // `build_widget()` calls `Widget(...)`: a constructor call, which is
        // syntactically indistinguishable from calling a bare function named
        // `Widget` — the callee name recorded is just "Widget".
        let symbols = vec![
            sym("build_widget", &["Widget"]),
            sym("Widget.__init__", &[]),
        ];
        let g = build(&symbols);
        assert!(
            g.related(0, 1),
            "constructing a class should relate the caller to its __init__"
        );
    }

    #[test]
    fn constructor_call_does_not_blanket_relate_to_sibling_methods() {
        // `build_widget()` constructs `Widget(...)` but `Widget.__init__`
        // itself never calls `Widget.resize` — so the caller must NOT be
        // related to `resize` just because they're both on the same class
        // (that would over-suppress two genuinely competing methods).
        let symbols = vec![
            sym("build_widget", &["Widget"]),
            sym("Widget.__init__", &[]),
            sym("Widget.resize", &[]),
        ];
        let g = build(&symbols);
        assert!(
            !g.related(0, 2),
            "constructor call must not blanket-relate the caller to every method of the class"
        );
    }

    #[test]
    fn constructor_call_reaches_sibling_only_via_init_chain() {
        // Same as above, but `Widget.__init__` itself calls `Widget.resize`
        // (e.g. to set an initial size) — now the existing two-hop chain
        // mechanism legitimately reaches it: build_widget -> __init__ -> resize.
        let symbols = vec![
            sym("build_widget", &["Widget"]),
            sym("Widget.__init__", &["resize"]),
            sym("Widget.resize", &[]),
        ];
        let g = build(&symbols);
        assert!(
            g.related(0, 2),
            "should reach the sibling transitively, because __init__ itself calls it"
        );
    }

    #[test]
    fn siblings_sharing_a_callee_are_not_related() {
        // Two independent implementations both call a shared helper `h`,
        // but neither calls the other, and `h` calls neither back: this is
        // NOT a delegation chain, so must not be suppressed.
        let symbols = vec![sym("impl_a", &["h"]), sym("impl_b", &["h"]), sym("h", &[])];
        let g = build(&symbols);
        assert!(
            !g.related(0, 1),
            "co-callers of a shared helper are competing siblings, not a wrapper pair"
        );
    }

    #[test]
    fn external_module_call_makes_no_edge_to_a_same_base_name_corpus_symbol() {
        // The corpus-P anchor scenario: `conn` (conftest) calls
        // `psycopg.connect(...)` — a third-party module absent from the corpus.
        // The corpus *does* have its own `appdb.engine.connect`, but `conn`
        // never calls it, so the external call must not fabricate that edge.
        let symbols = vec![
            sym_at(
                "conftest.py",
                "conn",
                [mcall("connect", "psycopg")].into_iter().collect(),
            ),
            sym_at("packages/db/src/appdb/engine.py", "connect", HashSet::new()),
        ];
        let g = build(&symbols);
        assert!(
            !g.related(0, 1),
            "an external `psycopg.connect` must not collide with corpus `engine.connect`"
        );
    }

    #[test]
    fn corpus_module_call_matches_that_module_specifically() {
        // `connection` does `from appdb.engine import connect; connect(...)`.
        // It must relate to `connect` in engine.py — and *only* there, not to a
        // coincidental same-named `connect` in an unrelated module.
        let symbols = vec![
            sym_at(
                "orchestration/resources.py",
                "PostgresResource.connection",
                [mcall("connect", "appdb.engine")].into_iter().collect(),
            ),
            sym_at("packages/db/src/appdb/engine.py", "connect", HashSet::new()),
            sym_at("some/other/pkg.py", "connect", HashSet::new()),
        ];
        let g = build(&symbols);
        assert!(g.related(0, 1), "resolved call should reach engine.connect");
        assert!(
            !g.related(0, 2),
            "a same-named `connect` in an unrelated module must not match"
        );
    }

    #[test]
    fn psycopg_shadowing_regains_the_third_competing_member() {
        // The full AC in miniature: three DB-connection symbols. `connection`
        // genuinely delegates to `engine.connect` (suppressed); `conn` only
        // *looks* like it does via the base name `connect` on a third-party
        // call (must NOT be suppressed). So `conn` stays competing against both
        // the other two, rather than being falsely tied to `connect`.
        let symbols = vec![
            sym_at(
                "conftest.py",
                "conn",
                [mcall("connect", "psycopg")].into_iter().collect(),
            ),
            sym_at("packages/db/src/appdb/engine.py", "connect", HashSet::new()),
            sym_at(
                "orchestration/resources.py",
                "PostgresResource.connection",
                [mcall("connect", "appdb.engine")].into_iter().collect(),
            ),
        ];
        let g = build(&symbols);
        assert!(g.related(1, 2), "connection genuinely calls engine.connect");
        assert!(!g.related(0, 1), "conn only shadows connect via third-party psycopg");
        assert!(!g.related(0, 2), "conn does not call connection either");
    }

    #[test]
    fn import_precise_constructor_reaches_the_aliased_class_init() {
        // `from ._transports import HTTPTransport as HT; HT(...)`. Normalization
        // records the *original* class name with the resolved module, so the
        // construction relates to `HTTPTransport.__init__` in that module —
        // something the raw base name `HT` (no class `HT` exists) would miss.
        let symbols = vec![
            sym_at(
                "httpx/_client.py",
                "Client._init_transport",
                [mcall("HTTPTransport", "httpx._transports.default")]
                    .into_iter()
                    .collect(),
            ),
            sym_at(
                "httpx/_transports/default.py",
                "HTTPTransport.__init__",
                HashSet::new(),
            ),
            // A different class named HTTPTransport in an unrelated module must
            // NOT be reached — the resolution is module-scoped.
            sym_at("other/pkg.py", "HTTPTransport.__init__", HashSet::new()),
        ];
        let g = build(&symbols);
        assert!(g.related(0, 1), "aliased constructor should reach its class __init__");
        assert!(
            !g.related(0, 2),
            "constructor resolution must be scoped to the imported module"
        );
    }
}
