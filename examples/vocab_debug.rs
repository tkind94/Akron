//! Scratch debugging tool (not part of the product surface): print shared-
//! vocab stats for two qnames in a scanned repo, to tune the shared-vocab
//! quality gate thresholds in queries.rs against the graded external corpora.
//! Usage: cargo run --release --example vocab_debug -- <repo> <qname_a> <qname_b>

use akron::types::Config;
use akron::{cluster, scan};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let qa = &args[2];
    let qb = &args[3];

    let cfg = Config {
        min_nodes: 30,
        wl_iters: 3,
        theta_clone: 0.60,
        theta_b: 0.55,
        theta_a_low: 0.30,
        theta_family: 0.35,
        theta_b_family: akron::family::THETA_B_FAMILY,
        top: 20,
    };
    let scanned = scan::scan_repo(&root, &cfg);
    let symbols = &scanned.symbols;
    let vocab = cluster::vocab_index(symbols);

    let idx = |q: &str| {
        symbols
            .iter()
            .position(|s| s.sym.qname == q || s.sym.qname.ends_with(&format!(".{q}")))
            .unwrap_or_else(|| panic!("not found: {q}"))
    };
    let a = idx(qa) as u32;
    let b = idx(qb) as u32;

    let shared = vocab.shared_term_weights(a, b);
    let mass_a = vocab.weight_mass(a);
    let mass_b = vocab.weight_mass(b);
    let shared_mass: f32 = shared.iter().map(|&(_, w)| w).sum();
    let smaller = mass_a.min(mass_b);

    println!("a={qa} mass={mass_a:.4}  b={qb} mass={mass_b:.4}");
    println!(
        "shared terms ({}): {:?}",
        shared.len(),
        shared
            .iter()
            .map(|&(id, w)| (vocab.term_name(id).to_string(), w))
            .collect::<Vec<_>>()
    );
    println!(
        "shared_mass={shared_mass:.4}  smaller_mass={smaller:.4}  fraction={:.4}",
        shared_mass / smaller
    );
    println!("cosine_between = {:.4}", vocab.cosine_between(a, b));
}
