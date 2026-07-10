use crate::types::SymbolRef;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser, Tree};
use walkdir::WalkDir;

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".venv",
    "venv",
    "env",
    "node_modules",
    "__pycache__",
    ".tox",
    ".mypy_cache",
    ".ruff_cache",
    ".pytest_cache",
    "build",
    "dist",
    "site-packages",
    ".eggs",
];

pub fn python_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !(e.file_type().is_dir()
                && (SKIP_DIRS.contains(&name.as_ref()) || name.starts_with('.')))
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
        .map(|e| e.into_path())
        .collect()
}

pub fn parse(source: &[u8]) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("tree-sitter-python grammar/version mismatch");
    parser.parse(source, None).expect("parser returned no tree")
}

/// A function/method occurrence: the subtree to fingerprint (includes
/// decorators when present) plus the `function_definition` node itself.
pub struct FnOccurrence<'t> {
    pub root: Node<'t>,
    pub func: Node<'t>,
    pub sym: SymbolRef,
}

pub fn is_test_path(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    lower.split('/').any(|part| {
        part == "tests" || part == "test" || part.starts_with("test_") || part.ends_with("_test.py")
    })
}

pub fn extract_functions<'t>(
    tree: &'t Tree,
    source: &[u8],
    rel_path: &str,
) -> Vec<FnOccurrence<'t>> {
    let mut out = Vec::new();
    let mut scope = Vec::new();
    walk(tree.root_node(), source, rel_path, &mut scope, &mut out);
    out
}

fn walk<'t>(
    node: Node<'t>,
    source: &[u8],
    rel_path: &str,
    scope: &mut Vec<String>,
    out: &mut Vec<FnOccurrence<'t>>,
) {
    match node.kind() {
        "function_definition" => {
            let name = field_text(node, "name", source).unwrap_or_else(|| "<anon>".into());
            let mut qual: Vec<&str> = scope.iter().map(String::as_str).collect();
            qual.push(&name);
            // Include decorators in the fingerprinted subtree: they are both
            // structure and high-value vocabulary (@retry, @app.route).
            let root = match node.parent() {
                Some(p) if p.kind() == "decorated_definition" => p,
                _ => node,
            };
            out.push(FnOccurrence {
                root,
                func: node,
                sym: SymbolRef {
                    file: rel_path.to_string(),
                    qname: qual.join("."),
                    line: node.start_position().row + 1,
                },
            });
            scope.push(name);
            descend(node, source, rel_path, scope, out);
            scope.pop();
        }
        "class_definition" => {
            let name = field_text(node, "name", source).unwrap_or_else(|| "<anon>".into());
            scope.push(name);
            descend(node, source, rel_path, scope, out);
            scope.pop();
        }
        _ => descend(node, source, rel_path, scope, out),
    }
}

fn descend<'t>(
    node: Node<'t>,
    source: &[u8],
    rel_path: &str,
    scope: &mut Vec<String>,
    out: &mut Vec<FnOccurrence<'t>>,
) {
    for i in 0..node.child_count() {
        walk(node.child(i).unwrap(), source, rel_path, scope, out);
    }
}

fn field_text(node: Node, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|n| n.utf8_text(source).ok())
        .map(str::to_string)
}
