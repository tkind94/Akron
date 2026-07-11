//! Normalization (DESIGN.md §2.1): strip comments/docstrings, alpha-rename
//! locals by binding order, abstract literals to type tokens. Channel A sees
//! external names only as `EXT`; Channel B keeps every identifier's subwords.

use crate::types::{Call, ModuleRef, NormTree};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;
use xxhash_rust::xxh3::xxh3_64;

const PUNCT: &[&str] = &["(", ")", "[", "]", "{", "}", ",", ":", ";", "."];
const STOPWORDS: &[&str] = &["self", "cls", "none", "true", "false", "return", "the"];

pub struct Normalized {
    pub tree: NormTree,
    pub vocab_tf: HashMap<String, u32>,
    /// Calls seen in `call` nodes, resolved against the file imports (see
    /// `callrel.rs`).
    pub calls: HashSet<Call>,
}

pub fn normalize(root: Node, func: Node, source: &[u8], imports: &ImportTable) -> Normalized {
    let locals = collect_locals(func, source);
    let mut b = Builder {
        source,
        locals,
        imports,
        tree: NormTree {
            labels: Vec::new(),
            children: Vec::new(),
            spans: Vec::new(),
        },
        vocab_tf: HashMap::new(),
        calls: HashSet::new(),
    };
    b.build(root);
    Normalized {
        tree: b.tree,
        vocab_tf: b.vocab_tf,
        calls: b.calls,
    }
}

// ── file import bindings: object-segment resolution for call relations ──

/// One imported name's binding. `module` is the module the name lives in;
/// `symbol` is `Some(orig)` when the name is a symbol imported *from* a module
/// (`from m import orig [as name]`) and `None` when the name is itself a module
/// (`import m [as name]`, `from . import m`).
struct Binding {
    module: ModuleRef,
    symbol: Option<String>,
}

/// A file's import bindings: local name → what it refers to. Built once per
/// file (`collect_imports`) and shared by every function normalized in it.
pub struct ImportTable {
    binds: HashMap<String, Binding>,
}

impl ImportTable {
    pub fn empty() -> ImportTable {
        ImportTable {
            binds: HashMap::new(),
        }
    }
    fn get(&self, name: &str) -> Option<&Binding> {
        self.binds.get(name)
    }
    /// `from m import orig [as name]` binding for `name`: `(orig, module)`.
    /// TKI-61 viewer: the read-only mirror of `Builder::resolve_bare`, so the
    /// explore page's identifier links resolve imports exactly the way the
    /// call records the engine already trusts do.
    pub fn symbol_import(&self, name: &str) -> Option<(&str, &ModuleRef)> {
        self.get(name)
            .and_then(|b| b.symbol.as_deref().map(|s| (s, &b.module)))
    }
    /// `import m [as name]` / `from . import name` binding: `name` is itself
    /// a module. The read-only mirror of `Builder::resolve_attr`'s object
    /// lookup (TKI-61 viewer, same reasoning as `symbol_import`).
    pub fn module_import(&self, name: &str) -> Option<&ModuleRef> {
        self.get(name)
            .and_then(|b| b.symbol.is_none().then_some(&b.module))
    }
}

/// Collect a file's module-level import bindings. `rel_path` is the file's
/// repo-relative path, used to resolve relative imports (`from .mod import X`)
/// to an absolute dotted module against the file's own package directory.
/// Bindings are file-global (nested imports collapse into the same table),
/// matching the flat-scope approximation used for locals.
pub fn collect_imports(root: Node, source: &[u8], rel_path: &str) -> ImportTable {
    // The importing file's package directory, as path components. `.` in a
    // relative import refers to this directory (true for both a regular module
    // `pkg/mod.py` — package `pkg` — and a package's own `pkg/__init__.py`).
    let mut dir: Vec<&str> = rel_path.split('/').collect();
    dir.pop(); // drop the filename
    let mut binds = HashMap::new();
    walk_imports(root, source, &dir, &mut binds);
    ImportTable { binds }
}

fn walk_imports(node: Node, source: &[u8], dir: &[&str], binds: &mut HashMap<String, Binding>) {
    match node.kind() {
        "import_statement" => import_bindings(node, source, binds),
        "import_from_statement" => from_bindings(node, source, dir, binds),
        _ => {
            for i in 0..node.child_count() {
                walk_imports(node.child(i).unwrap(), source, dir, binds);
            }
        }
    }
}

/// `import a`, `import a.b`, `import a as b`, `import a.b as c`.
fn import_bindings(node: Node, source: &[u8], binds: &mut HashMap<String, Binding>) {
    for i in 0..node.named_child_count() {
        let child = node.named_child(i).unwrap();
        match child.kind() {
            "dotted_name" => {
                // `import a.b.c` binds the top name `a`, referring to package `a`.
                let text = child.utf8_text(source).unwrap_or("");
                if let Some(top) = text.split('.').next().filter(|s| !s.is_empty()) {
                    binds.insert(
                        top.to_string(),
                        Binding {
                            module: ModuleRef::Absolute(top.to_string()),
                            symbol: None,
                        },
                    );
                }
            }
            "aliased_import" => {
                // `import a.b as c` binds `c` to module `a.b`.
                let module = field_text(child, "name", source);
                let alias = field_text(child, "alias", source);
                if let (Some(module), Some(alias)) = (module, alias) {
                    binds.insert(
                        alias,
                        Binding {
                            module: ModuleRef::Absolute(module),
                            symbol: None,
                        },
                    );
                }
            }
            _ => {}
        }
    }
}

/// `from m import x`, `from m import x as y`, `from .m import x`,
/// `from . import x`, `from m import *` (wildcard skipped).
fn from_bindings(node: Node, source: &[u8], dir: &[&str], binds: &mut HashMap<String, Binding>) {
    let Some(module_node) = node.child_by_field_name("module_name") else {
        return;
    };
    // Resolve the `from` module to (absolute dotted path, has_named_module).
    // `has_named_module` is false only for a bare relative `from . import x`,
    // where each imported name is itself a submodule of the package.
    let (module_abs, has_named_module) = match module_node.kind() {
        "dotted_name" => (module_node.utf8_text(source).unwrap_or("").to_string(), true),
        "relative_import" => match resolve_relative(module_node, source, dir) {
            Some(r) => r,
            None => return,
        },
        _ => return,
    };

    for i in 0..node.named_child_count() {
        let child = node.named_child(i).unwrap();
        if child.id() == module_node.id() {
            continue; // the module_name node itself
        }
        match child.kind() {
            "dotted_name" => {
                let name = child.utf8_text(source).unwrap_or("");
                add_from_binding(binds, &module_abs, has_named_module, name, name);
            }
            "aliased_import" => {
                let orig = field_text(child, "name", source);
                let alias = field_text(child, "alias", source);
                if let (Some(orig), Some(alias)) = (orig, alias) {
                    add_from_binding(binds, &module_abs, has_named_module, &alias, &orig);
                }
            }
            _ => {} // wildcard_import and anything else: leave to base-name fallback
        }
    }
}

fn add_from_binding(
    binds: &mut HashMap<String, Binding>,
    module_abs: &str,
    has_named_module: bool,
    local: &str,
    orig: &str,
) {
    let binding = if has_named_module {
        // `from pkg.mod import orig` — `orig` is a symbol inside `pkg.mod`.
        Binding {
            module: ModuleRef::Absolute(module_abs.to_string()),
            symbol: Some(orig.to_string()),
        }
    } else {
        // `from . import orig` — `orig` is a submodule of the package.
        let sub = join_dotted(module_abs, orig);
        Binding {
            module: ModuleRef::Absolute(sub),
            symbol: None,
        }
    };
    binds.insert(local.to_string(), binding);
}

/// Resolve a `relative_import` (`.mod`, `..pkg.mod`, `.`) against the importing
/// file's package `dir`, returning `(absolute dotted path, has_named_module)`.
/// `has_named_module` is false when there is no module tail (bare `from .`),
/// in which case the dotted path is the package itself.
fn resolve_relative(node: Node, source: &[u8], dir: &[&str]) -> Option<(String, bool)> {
    let mut dots = 0usize;
    let mut tail: Option<String> = None;
    for i in 0..node.named_child_count() {
        let c = node.named_child(i).unwrap();
        match c.kind() {
            "import_prefix" => dots = c.utf8_text(source).unwrap_or("").matches('.').count(),
            "dotted_name" => tail = c.utf8_text(source).map(str::to_string).ok(),
            _ => {}
        }
    }
    if dots == 0 {
        // `import_prefix` is repeat1('.'), so a relative import always has ≥1;
        // fall back defensively if the grammar surprises us.
        dots = 1;
    }
    // `.` (1 dot) = the package dir itself; each extra dot climbs one level.
    let keep = dir.len().checked_sub(dots - 1)?;
    let mut comps: Vec<&str> = dir[..keep].to_vec();
    let has_tail = tail.is_some();
    let tail_owned = tail.unwrap_or_default();
    if has_tail {
        comps.extend(tail_owned.split('.').filter(|s| !s.is_empty()));
    }
    Some((comps.join("."), has_tail))
}

fn join_dotted(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}.{name}")
    }
}

fn field_text(node: Node, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|n| n.utf8_text(source).ok())
        .map(str::to_string)
}

// ── locals: parameters + assignment/for/with-as/walrus/nested-def targets ──

/// `pub` since TKI-61: the explore viewer skips link resolution for
/// identifiers that are locals (Python's own shadowing — a parameter named
/// `helper` must not link to a corpus function `helper`), using the exact
/// binding collection Channel A's alpha-rename already trusts.
pub fn collect_locals(func: Node, source: &[u8]) -> HashMap<String, u32> {
    let mut locals = HashMap::new();
    if let Some(params) = func.child_by_field_name("parameters") {
        param_names(params, source, &mut locals);
    }
    if let Some(body) = func.child_by_field_name("body") {
        binding_names(body, source, &mut locals);
    }
    locals
}

fn bind(name: &str, locals: &mut HashMap<String, u32>) {
    let next = locals.len() as u32;
    locals.entry(name.to_string()).or_insert(next);
}

fn param_names(node: Node, source: &[u8], locals: &mut HashMap<String, u32>) {
    match node.kind() {
        "identifier" => bind(node.utf8_text(source).unwrap_or(""), locals),
        "type" => {} // never descend into annotations
        "default_parameter" | "typed_default_parameter" => {
            if let Some(n) = node.child_by_field_name("name") {
                param_names(n, source, locals);
            }
        }
        "typed_parameter" => {
            if let Some(n) = node.child(0) {
                param_names(n, source, locals);
            }
        }
        _ => {
            for i in 0..node.child_count() {
                param_names(node.child(i).unwrap(), source, locals);
            }
        }
    }
}

fn binding_names(node: Node, source: &[u8], locals: &mut HashMap<String, u32>) {
    match node.kind() {
        "assignment" | "augmented_assignment" => {
            if let Some(left) = node.child_by_field_name("left") {
                target_names(left, source, locals);
            }
        }
        "for_statement" | "for_in_clause" => {
            if let Some(left) = node.child_by_field_name("left") {
                target_names(left, source, locals);
            }
        }
        "named_expression" => {
            if let Some(name) = node.child_by_field_name("name") {
                target_names(name, source, locals);
            }
        }
        "as_pattern" => {
            if let Some(alias) = node.child_by_field_name("alias") {
                target_names(alias, source, locals);
            }
        }
        // Nested defs: bind the name; treat scope as flat (approximation).
        "function_definition" | "class_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                bind(n.utf8_text(source).unwrap_or(""), locals);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        binding_names(node.child(i).unwrap(), source, locals);
    }
}

fn target_names(node: Node, source: &[u8], locals: &mut HashMap<String, u32>) {
    match node.kind() {
        "identifier" | "as_pattern_target" => {
            if node.kind() == "identifier" {
                bind(node.utf8_text(source).unwrap_or(""), locals);
            } else {
                for i in 0..node.child_count() {
                    target_names(node.child(i).unwrap(), source, locals);
                }
            }
        }
        // `a.b = x` / `a[i] = x` bind nothing new.
        "attribute" | "subscript" => {}
        _ => {
            for i in 0..node.child_count() {
                target_names(node.child(i).unwrap(), source, locals);
            }
        }
    }
}

// ── normalized tree + vocabulary, one walk ──

struct Builder<'s> {
    source: &'s [u8],
    locals: HashMap<String, u32>,
    imports: &'s ImportTable,
    tree: NormTree,
    vocab_tf: HashMap<String, u32>,
    calls: HashSet<Call>,
}

impl<'s> Builder<'s> {
    /// Returns the index of the created node, or None if skipped.
    fn build(&mut self, node: Node) -> Option<u32> {
        if skip(node, self.source) {
            return None;
        }
        let kind = node.kind();

        if kind == "call" {
            self.record_call(node);
        }
        if kind == "identifier" {
            let text = node.utf8_text(self.source).unwrap_or("");
            self.add_vocab(text);
            let label = match self.locals.get(text) {
                Some(idx) => xxh3_64(format!("x{idx}").as_bytes()),
                None => xxh3_64(b"EXT"),
            };
            return Some(self.push(label, Vec::new(), span_of(node)));
        }
        if kind == "string" {
            return Some(self.build_string(node));
        }
        if kind == "integer" || kind == "float" {
            return Some(self.push(xxh3_64(b"NUM"), Vec::new(), span_of(node)));
        }
        if !node.is_named() {
            // Anonymous tokens: operators and keywords are structure;
            // punctuation is uniform noise.
            let text = node.utf8_text(self.source).unwrap_or("");
            if PUNCT.contains(&text) {
                return None;
            }
            return Some(self.push(xxh3_64(text.as_bytes()), Vec::new(), span_of(node)));
        }

        let mut kids = Vec::new();
        for i in 0..node.child_count() {
            if let Some(k) = self.build(node.child(i).unwrap()) {
                kids.push(k);
            }
        }
        Some(self.push(xxh3_64(kind.as_bytes()), kids, span_of(node)))
    }

    /// Plain strings are `STR` leaves; f-strings keep their interpolations.
    fn build_string(&mut self, node: Node) -> u32 {
        let mut interps = Vec::new();
        for i in 0..node.child_count() {
            let c = node.child(i).unwrap();
            if c.kind() == "interpolation" {
                if let Some(k) = self.build(c) {
                    interps.push(k);
                }
            }
        }
        if interps.is_empty() {
            self.push(xxh3_64(b"STR"), Vec::new(), span_of(node))
        } else {
            self.push(xxh3_64(b"FSTR"), interps, span_of(node))
        }
    }

    fn push(&mut self, label: u64, kids: Vec<u32>, span: (u32, u32)) -> u32 {
        self.tree.labels.push(label);
        self.tree.children.push(kids);
        self.tree.spans.push(span);
        (self.tree.labels.len() - 1) as u32
    }

    fn add_vocab(&mut self, ident: &str) {
        for token in subwords(ident) {
            *self.vocab_tf.entry(token).or_insert(0) += 1;
        }
    }

    /// Record a call, resolving its object segment against the file's imports
    /// so `callrel` can be import-precise. `foo(...)` resolves `foo` as a
    /// possibly from-imported name; `mod.foo(...)` resolves `mod` as a possibly
    /// imported module; every other shape (subscript, call-result, attribute
    /// chain, local/`self` object) keeps the plain base-name join (`module:
    /// None`) — the fallback that reproduces today's behavior exactly.
    fn record_call(&mut self, node: Node) {
        let Some(func) = node.child_by_field_name("function") else {
            return;
        };
        let call = match func.kind() {
            "identifier" => self.resolve_bare(func.utf8_text(self.source).unwrap_or("")),
            "attribute" => {
                let Some(method) = func
                    .child_by_field_name("attribute")
                    .and_then(|a| a.utf8_text(self.source).ok())
                else {
                    return;
                };
                match func.child_by_field_name("object") {
                    Some(obj) if obj.kind() == "identifier" => {
                        self.resolve_attr(obj.utf8_text(self.source).unwrap_or(""), method)
                    }
                    // Attribute chain / call-result / subscript object: the base
                    // isn't a resolvable name → base-name fallback.
                    _ => Call {
                        base: method.to_string(),
                        module: None,
                    },
                }
            }
            _ => return,
        };
        self.calls.insert(call);
    }

    /// `foo(...)`: a `from m import foo [as name]` binding resolves to `foo` in
    /// module `m`; anything else (local, parameter, a bare-module name, or an
    /// unimported top-level function) keeps the base-name fallback.
    fn resolve_bare(&self, name: &str) -> Call {
        match self.imports.get(name) {
            Some(b) if b.symbol.is_some() => Call {
                base: b.symbol.clone().unwrap(),
                module: Some(b.module.clone()),
            },
            _ => Call {
                base: name.to_string(),
                module: None,
            },
        }
    }

    /// `obj.method(...)`: an `import obj` / `from . import obj` binding resolves
    /// `method` to module `obj`; if `obj` is instead a from-imported symbol
    /// (a class/function, not a module) or a local/`self`, fall back to the
    /// base-name join on `method`.
    fn resolve_attr(&self, obj: &str, method: &str) -> Call {
        match self.imports.get(obj) {
            Some(b) if b.symbol.is_none() => Call {
                base: method.to_string(),
                module: Some(b.module.clone()),
            },
            _ => Call {
                base: method.to_string(),
                module: None,
            },
        }
    }
}

/// The node's byte range in the original source, as the `(start, end)` pair a
/// `NormTree` node carries (source files fit well within `u32`).
fn span_of(node: Node) -> (u32, u32) {
    (node.start_byte() as u32, node.end_byte() as u32)
}

fn skip(node: Node, source: &[u8]) -> bool {
    match node.kind() {
        "comment" => true,
        // Bare-string statements are docstrings (or no-ops): not code.
        "expression_statement" => {
            node.named_child_count() == 1
                && node
                    .named_child(0)
                    .is_some_and(|c| c.kind() == "string" && string_has_no_interpolation(c, source))
        }
        _ => false,
    }
}

fn string_has_no_interpolation(node: Node, _source: &[u8]) -> bool {
    (0..node.child_count()).all(|i| node.child(i).unwrap().kind() != "interpolation")
}

/// `fetch_with_proxyURL2` → ["fetch", "with", "proxy", "url"]
pub fn subwords(ident: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = ident.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            flush(&mut cur, &mut words);
        } else if c.is_uppercase()
            && i > 0
            && (chars[i - 1].is_lowercase() || (i + 1 < chars.len() && chars[i + 1].is_lowercase()))
        {
            flush(&mut cur, &mut words);
            cur.push(c);
        } else {
            cur.push(c);
        }
    }
    flush(&mut cur, &mut words);
    words
}

fn flush(cur: &mut String, words: &mut Vec<String>) {
    if cur.len() >= 2 && !cur.chars().all(|c| c.is_numeric()) {
        let w = cur.to_lowercase();
        if !STOPWORDS.contains(&w.as_str()) {
            words.push(w);
        }
    }
    cur.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint;
    use crate::parse;

    fn print_of(src: &str) -> (crate::types::NormTree, HashMap<String, u32>) {
        let tree = parse::parse(src.as_bytes());
        let funcs = parse::extract_functions(&tree, src.as_bytes(), "t.py");
        assert_eq!(funcs.len(), 1, "expected exactly one function in fixture");
        let n = normalize(
            funcs[0].root,
            funcs[0].func,
            src.as_bytes(),
            &ImportTable::empty(),
        );
        (n.tree, n.vocab_tf)
    }

    /// The single function's resolved calls, from a whole-file source.
    fn calls_of(src: &str, rel: &str) -> HashSet<Call> {
        let tree = parse::parse(src.as_bytes());
        let imports = collect_imports(tree.root_node(), src.as_bytes(), rel);
        let funcs = parse::extract_functions(&tree, src.as_bytes(), rel);
        normalize(funcs[0].root, funcs[0].func, src.as_bytes(), &imports).calls
    }

    fn has_call(calls: &HashSet<Call>, base: &str, module: Option<&str>) -> bool {
        calls.contains(&Call {
            base: base.to_string(),
            module: module.map(|m| ModuleRef::Absolute(m.to_string())),
        })
    }

    #[test]
    fn alpha_rename_makes_renamed_clones_identical() {
        let a = "def f(dog):\n    if dog is None:\n        return 0\n    return dog + 1\n";
        let b = "def f(cat):\n    if cat is None:\n        return 0\n    return cat + 1\n";
        let (ta, _) = print_of(a);
        let (tb, _) = print_of(b);
        assert_eq!(fingerprint::merkle_root(&ta), fingerprint::merkle_root(&tb));
    }

    #[test]
    fn external_names_do_not_enter_channel_a() {
        let a = "def f(x):\n    return requests.get(x)\n";
        let b = "def f(y):\n    return httpx.fetch(y)\n";
        let (ta, _) = print_of(a);
        let (tb, _) = print_of(b);
        assert_eq!(fingerprint::merkle_root(&ta), fingerprint::merkle_root(&tb));
    }

    #[test]
    fn structure_change_changes_channel_a() {
        let a = "def f(x):\n    return x + 1\n";
        let b = "def f(x):\n    if x:\n        return x + 1\n    return 0\n";
        let (ta, _) = print_of(a);
        let (tb, _) = print_of(b);
        assert_ne!(fingerprint::merkle_root(&ta), fingerprint::merkle_root(&tb));
    }

    #[test]
    fn vocab_keeps_externals_and_subwords() {
        let src = "def fetch_page(url):\n    resp = httpx.get(url, follow_redirects=True)\n    return resp\n";
        let (_, vocab) = print_of(src);
        for term in ["httpx", "get", "fetch", "page", "follow", "redirects"] {
            assert!(vocab.contains_key(term), "missing term {term}: {vocab:?}");
        }
    }

    #[test]
    fn calls_collects_bare_and_attribute_callees() {
        // No imports: both stay unresolved (module: None) — today's base-name join.
        let calls = calls_of("def f(x):\n    y = helper(x)\n    return mod.other(y)\n", "t.py");
        assert!(has_call(&calls, "helper", None), "calls: {calls:?}");
        assert!(has_call(&calls, "other", None), "calls: {calls:?}");
    }

    #[test]
    fn plain_import_resolves_attribute_object_to_its_module() {
        let src = "import psycopg\ndef f():\n    return psycopg.connect(get_dsn())\n";
        let calls = calls_of(src, "conftest.py");
        // `psycopg.connect` resolves the object `psycopg` to module `psycopg`;
        // the base recorded is the method `connect`.
        assert!(has_call(&calls, "connect", Some("psycopg")), "calls: {calls:?}");
    }

    #[test]
    fn aliased_import_binds_the_alias_to_the_module() {
        let src = "import typing as t\ndef f():\n    return t.cast(int, 1)\n";
        let calls = calls_of(src, "m.py");
        assert!(has_call(&calls, "cast", Some("typing")), "calls: {calls:?}");
    }

    #[test]
    fn from_import_resolves_bare_call_to_its_module_symbol() {
        let src = "from appdb.engine import connect\ndef f():\n    with connect(None) as c:\n        return c\n";
        let calls = calls_of(src, "resources.py");
        // Bare `connect(...)` resolves to symbol `connect` in module `appdb.engine`.
        assert!(has_call(&calls, "connect", Some("appdb.engine")), "calls: {calls:?}");
    }

    #[test]
    fn from_import_alias_records_the_original_name() {
        let src = "from appdb.engine import connect as db_connect\ndef f():\n    return db_connect()\n";
        let calls = calls_of(src, "m.py");
        // Aliased: the base recorded is the *original* name the module defines.
        assert!(has_call(&calls, "connect", Some("appdb.engine")), "calls: {calls:?}");
        assert!(!has_call(&calls, "db_connect", Some("appdb.engine")), "calls: {calls:?}");
    }

    #[test]
    fn relative_import_resolves_against_the_files_package() {
        // `httpx/_client.py`: `from ._transports.default import HTTPTransport`
        // resolves to the absolute `httpx._transports.default`.
        let src = "from ._transports.default import HTTPTransport\ndef f(self):\n    return HTTPTransport(verify=True)\n";
        let calls = calls_of(src, "httpx/_client.py");
        assert!(
            has_call(&calls, "HTTPTransport", Some("httpx._transports.default")),
            "calls: {calls:?}"
        );
    }

    #[test]
    fn unimported_bare_and_local_object_keep_base_name_fallback() {
        // `cls` is a local; `self.foo` is a method on a local object — both must
        // stay unresolved so they keep the whole-corpus base-name behavior.
        let src = "def f(self):\n    cls = self.runner\n    self.foo()\n    return cls(1)\n";
        let calls = calls_of(src, "m.py");
        assert!(has_call(&calls, "cls", None), "calls: {calls:?}");
        assert!(has_call(&calls, "foo", None), "calls: {calls:?}");
    }

    #[test]
    fn subword_splitting() {
        assert_eq!(
            subwords("fetch_with_proxyURL"),
            vec!["fetch", "with", "proxy", "url"]
        );
        assert_eq!(subwords("HTTPServer"), vec!["http", "server"]);
    }
}
