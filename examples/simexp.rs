//! Threshold-tuning experiment: pairwise Channel A cosine for the planted
//! fixtures under different WL iteration-weight schemes. Not part of the
//! product; run with `cargo run --example simexp`.

use akron::fingerprint::cosine;
use akron::types::NormTree;
use akron::{normalize, parse};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use xxhash_rust::xxh3::xxh3_64;

fn wl_weighted(tree: &NormTree, iters: usize, weight: impl Fn(usize) -> f32) -> Vec<(u64, f32)> {
    let n = tree.labels.len();
    // Bidirectional neighborhood: parent + children, so leaves differentiate.
    let mut parent = vec![u32::MAX; n];
    for (i, kids) in tree.children.iter().enumerate() {
        for &k in kids {
            parent[k as usize] = i as u32;
        }
    }
    let mut cur = tree.labels.clone();
    let mut hist: HashMap<u64, f32> = HashMap::new();
    let mut buf = Vec::new();
    for iter in 1..=iters {
        let mut next = vec![0u64; n];
        for i in 0..n {
            buf.clear();
            buf.extend_from_slice(&cur[i].to_le_bytes());
            let p = parent[i];
            buf.extend_from_slice(&(if p == u32::MAX { 0 } else { cur[p as usize] }).to_le_bytes());
            for &k in &tree.children[i] {
                buf.extend_from_slice(&cur[k as usize].to_le_bytes());
            }
            next[i] = xxh3_64(&buf);
        }
        cur = next;
        for &l in &cur {
            *hist.entry(l).or_default() += weight(iter);
        }
    }
    let mut out: Vec<(u64, f32)> = hist.into_iter().collect();
    out.sort_unstable_by_key(|&(l, _)| l);
    out
}

fn main() {
    // Optional: simexp <repo> <qname-substring> probes a real repo instead.
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 {
        probe(Path::new(&args[1]), &args[2]);
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let want = [
        "parse_records",
        "parse_records_v2",
        "extract_rows",
        "fetch_page_with_proxy",
        "ProxyFetcher.fetch_page",
    ];
    let mut trees: Vec<(String, NormTree)> = Vec::new();
    for f in parse::python_files(&root) {
        let src = fs::read(&f).unwrap();
        let tree = parse::parse(&src);
        for occ in parse::extract_functions(&tree, &src, &f.display().to_string()) {
            if want.contains(&occ.sym.qname.as_str()) {
                trees.push((
                    occ.sym.qname.clone(),
                    normalize::normalize(occ.root, occ.func, &src, &normalize::ImportTable::empty()).tree,
                ));
            }
        }
    }

    run_schemes(&trees);
}

fn probe(root: &Path, substr: &str) {
    let mut trees: Vec<(String, NormTree)> = Vec::new();
    for f in parse::python_files(root) {
        let src = fs::read(&f).unwrap();
        let tree = parse::parse(&src);
        for occ in parse::extract_functions(&tree, &src, &f.display().to_string()) {
            if occ.sym.qname.contains(substr) {
                trees.push((
                    occ.sym.qname.clone(),
                    normalize::normalize(occ.root, occ.func, &src, &normalize::ImportTable::empty()).tree,
                ));
            }
        }
    }
    trees.truncate(8);
    println!("probing {} symbols matching '{substr}'", trees.len());
    run_schemes(&trees);
}

fn run_schemes(trees: &[(String, NormTree)]) {
    let schemes: Vec<(&str, Box<dyn Fn(usize) -> f32>)> = vec![
        ("linear i", Box::new(|i| i as f32)),
        ("quadratic i^2", Box::new(|i| (i * i) as f32)),
        ("exp 3^i", Box::new(|i| 3f32.powi(i as i32))),
        ("iter3 only", Box::new(|i| if i == 3 { 1.0 } else { 0.0 })),
    ];

    for (name, w) in &schemes {
        println!("\n=== scheme: {name} ===");
        let hists: Vec<(String, Vec<(u64, f32)>)> = trees
            .iter()
            .map(|(q, t)| (q.clone(), wl_weighted(t, 3, w)))
            .collect();
        for i in 0..hists.len() {
            for j in i + 1..hists.len() {
                println!(
                    "  {:<22} vs {:<22} A={:.3}",
                    hists[i].0,
                    hists[j].0,
                    cosine(&hists[i].1, &hists[j].1)
                );
            }
        }
    }
}
