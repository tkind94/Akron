use akron::report::Section;
use akron::run;
use akron::types::Config;
use akron::{explain, find, report, review};
use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "akron",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("AKRON_GIT_SHA"), ")"),
    about = "Local codebase exploration tool"
)]
enum Cli {
    /// Scan a repo: similarity data as JSON, for tooling. No human view —
    /// see `explore` for that.
    Scan(ScanArgs),
    /// One terse card on a single symbol: clones, role twins, callers/callees,
    /// and family — everything the engine already knows about it.
    Explain(ExplainArgs),
    /// Semantic code search over a repo (TKI-41 / EXP-A): a natural-language
    /// question, ranked hits. Read-only, no writes anywhere in the repo.
    /// Requires a build with the `semantic` feature; exits 2 otherwise.
    Find(FindArgs),
    /// Live map of the repo (TKI-47): every symbol as a point on a local
    /// page, positioned by engine dimensions (PCA over find's embeddings,
    /// per-anchor channel cosines, dates). Read-only, serves until killed.
    /// Requires a build with the `semantic` feature; exits 2 otherwise.
    Explore(ExploreArgs),
    /// A deterministic evidence surface for reviewing a diff range (TKI-63):
    /// for each branch-new/changed symbol vs `--base`, where it sits,
    /// nearest-existing siblings (deterministic channels only), co-change
    /// partners outside the diff, and pattern prevalence. Facts only — no
    /// verdicts. `nearest_existing` degrades to empty (honestly) without the
    /// `semantic` feature; everything else works either way.
    Review(ReviewArgs),
}

/// Engine knobs shared by every subcommand.
#[derive(clap::Args)]
struct EngineArgs {
    /// Ignore functions smaller than this many normalized AST nodes
    #[arg(long, default_value_t = 30)]
    min_nodes: u32,
    /// Weisfeiler-Leman iterations
    #[arg(long, default_value_t = 3)]
    wl_iters: usize,
    /// Channel A cosine at/above which two shapes cluster as repeats
    #[arg(long, default_value_t = 0.60)]
    theta_clone: f32,
    /// Channel B cosine at/above which vocabularies count as shared
    #[arg(long, default_value_t = 0.55)]
    theta_b: f32,
    /// Channel A cosine at/below which shared-vocab code counts as competing
    #[arg(long, default_value_t = 0.30)]
    theta_a_low: f32,
    /// Channel A average-linkage at/above which clusters assemble into a family
    #[arg(long, default_value_t = akron::family::THETA_FAMILY)]
    theta_family: f32,
    /// Channel B centroid cosine two units must share to merge into a family
    /// (the vocabulary-coherence gate against generic-shape blob chaining)
    #[arg(long, default_value_t = akron::family::THETA_B_FAMILY)]
    theta_b_family: f32,
}

impl EngineArgs {
    fn to_config(&self) -> Config {
        Config {
            min_nodes: self.min_nodes,
            wl_iters: self.wl_iters,
            theta_clone: self.theta_clone,
            theta_b: self.theta_b,
            theta_a_low: self.theta_a_low,
            theta_family: self.theta_family,
            theta_b_family: self.theta_b_family,
            // No CLI surface reads this by count anymore (TKI-50 removed
            // `--top` along with every human renderer that used it);
            // `explore`'s own `explore_cfg()` sets the same field
            // independently, which is the only reason `Config` still has it.
            top: 20,
        }
    }
}

/// `akron scan <path> --json <file|->` — the versioned JSON contract, for
/// tooling (skills/akron/SKILL.md is the agent-facing consumer). No human
/// rendering lives here any more (TKI-50, CEO: "Yep, decommission scan") —
/// the human view of this data is `akron explore`.
#[derive(clap::Args)]
struct ScanArgs {
    /// Repo root to scan
    path: PathBuf,
    #[command(flatten)]
    engine: EngineArgs,
    /// Populate one optional section of the JSON: `families`/`competing` are
    /// empty unless named here (`repeated`/`deprecated` are always
    /// populated) — see skills/akron/SKILL.md for the full field list.
    #[arg(long, value_enum)]
    only: Option<Section>,
    /// Write the JSON contract: a file path, or `-` for stdout. Required in
    /// effect — omit it and `scan` has nothing to do, so it prints a pointer
    /// to `--json` and to `akron explore` for the human view, then exits 2.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Print wall time per pipeline phase to stderr (off by default)
    #[arg(long)]
    timings: bool,
}

/// `akron explain <root> [target]` — resolve `target` (an exact qname,
/// `file:line`, a unique dotted-suffix, or a unique case-insensitive
/// substring, TKI-51) against the same engine `scan` uses, and print its
/// one-screen card. `target` omitted prints usage + real example targets
/// from this repo instead (exit 2).
#[derive(clap::Args)]
struct ExplainArgs {
    /// Repo root to scan
    path: PathBuf,
    /// The symbol to explain: an exact qname, `file:line` (any line inside
    /// the symbol's body), a unique suffix (e.g. a bare method name), or a
    /// unique case-insensitive substring. Omit to print usage + real example
    /// targets from this repo.
    target: Option<String>,
    #[command(flatten)]
    engine: EngineArgs,
}

/// `akron find <path> <query>` — same `<path>` convention as `scan`/`explain`.
#[derive(clap::Args)]
struct FindArgs {
    /// Repo root to scan
    path: PathBuf,
    /// Natural-language question to search the repo for
    query: String,
    /// Max hits printed
    #[arg(long, default_value_t = 10)]
    top: usize,
    /// Include test symbols in the ranking (dropped by default — see
    /// R&D archive spike/find/RESULTS.md's is_test finding)
    #[arg(long)]
    tests: bool,
    /// Emit hits as versioned JSON instead of the text rows. Bare `--json` (or
    /// `--json -`) writes to stdout; any other value is a file path.
    #[arg(long, num_args = 0..=1, default_missing_value = "-")]
    json: Option<PathBuf>,
}

/// `akron explore <path>` — same `<path>` convention as the other verbs.
#[derive(clap::Args)]
struct ExploreArgs {
    /// Repo root to scan
    path: PathBuf,
    /// Port to serve on (0 lets the OS pick a free one)
    #[arg(long, default_value_t = akron::explore::DEFAULT_PORT)]
    port: u16,
    /// Start with test symbols shown on the map (they are always scanned;
    /// this only sets the page's initial toggle)
    #[arg(long)]
    tests: bool,
    /// Base ref for branch highlighting (default: the repo's default branch
    /// — origin/HEAD if set, else main, else master). Branch-new symbols are
    /// those introduced since `merge-base HEAD <base>`.
    #[arg(long)]
    base: Option<String>,
}

/// `akron review <path> [--base <ref>]` — same `<path>` convention as the
/// other verbs; `--base` follows `explore`'s convention (default: the
/// repo's default branch).
#[derive(clap::Args)]
struct ReviewArgs {
    /// Repo root to review
    path: PathBuf,
    /// Base ref to diff against (default: the repo's default branch —
    /// origin/HEAD if set, else main, else master)
    #[arg(long)]
    base: Option<String>,
    /// Emit the report as versioned JSON instead of the text rendering.
    /// Bare `--json` (or `--json -`) writes to stdout; any other value is a
    /// file path.
    #[arg(long, num_args = 0..=1, default_missing_value = "-")]
    json: Option<PathBuf>,
}

fn main() -> Result<()> {
    // Rust sets SIGPIPE to SIG_IGN at startup, so a write to a closed pipe
    // (e.g. `akron explain ... | head`) surfaces as an `io::Error` instead
    // of killing the process — but every subcommand here writes through
    // `println!`, which panics on that error rather than handling it.
    // Restoring the default disposition makes the OS terminate the process
    // directly on that write, the same way any Unix pipeline tool behaves.
    reset_sigpipe();
    match Cli::parse() {
        Cli::Scan(args) => run_scan(args),
        // No exit-1 gate concept applies to a read-only explain card: every
        // resolution failure (not found, ambiguous, unreadable file) is an
        // operational error, uniformly exit 2.
        Cli::Explain(args) => match run_explain(args) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("error: {e:#}");
                std::process::exit(2);
            }
        },
        // Same discipline as explain: any failure (feature disabled,
        // model/network trouble, a bad path) is an operational error, exit 2
        // uniformly. No "error: " prefix here: the no-feature case must print
        // exactly its one line, and every other find failure is already a
        // single clear sentence.
        Cli::Find(args) => match run_find(args) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("{e:#}");
                std::process::exit(2);
            }
        },
        // Same discipline as find: feature disabled, model/network trouble,
        // a bad path, a busy port — all operational errors, exit 2.
        Cli::Explore(args) => match run_explore(args) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("{e:#}");
                std::process::exit(2);
            }
        },
        // Same discipline: not-a-repo / bad --base are the only operational
        // errors (exit 2); "no changed symbols" is a normal Ok result (exit
        // 0) — see review.rs's module doc "Exit discipline".
        Cli::Review(args) => match run_review(args) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("{e:#}");
                std::process::exit(2);
            }
        },
    }
}

#[cfg(unix)]
fn reset_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

/// `akron explore <path>` — scan once, serve the map until killed.
fn run_explore(args: ExploreArgs) -> Result<()> {
    let root = args
        .path
        .canonicalize()
        .with_context(|| format!("no such path: {}", args.path.display()))?;
    akron::explore::serve(&root, args.port, args.tests, args.base.as_deref())
}

fn run_scan(args: ScanArgs) -> Result<()> {
    let cfg = args.engine.to_config();
    let root = args
        .path
        .canonicalize()
        .with_context(|| format!("no such path: {}", args.path.display()))?;

    // `scan` is a machine contract now (TKI-50) — the human view moved to
    // `explore`. No `--json` means nothing to compute, so this short-circuits
    // before running the (expensive) analysis below.
    let Some(json_out) = args.json else {
        eprintln!(
            "scan produces data for tooling; use --json (or - for stdout). \
             For the human view: akron explore <path>"
        );
        std::process::exit(2);
    };

    let analysis = run::analyze(&root, &cfg);
    let symbols = &analysis.scanned.symbols;
    let history = analysis.scanned.history.as_ref();

    let t = Instant::now();
    let v = report::json_report(
        &root,
        &analysis.stats,
        symbols,
        &analysis.repeated,
        &analysis.families,
        &analysis.competing,
        &analysis.deprecated,
        history,
        &cfg,
        args.only,
    );
    let t_json = t.elapsed();

    if json_out == Path::new("-") {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        fs::write(&json_out, serde_json::to_vec_pretty(&v)?)
            .with_context(|| format!("writing {}", json_out.display()))?;
        eprintln!("wrote {}", json_out.display());
    }

    if args.timings {
        let tm = &analysis.timings;
        let rows = [
            ("parse+fingerprint", analysis.scanned.t_parse_fp),
            ("shape clustering", tm.shape),
            ("repeated query", tm.repeated),
            ("vocab index", tm.vocab),
            ("family assembly", tm.family),
            ("call graph", tm.calls),
            ("competing query", tm.competing),
            ("history walk", analysis.scanned.t_history),
            ("deprecated query", tm.deprecated),
            ("json assembly", t_json),
        ];
        eprintln!("\n── timings ──");
        for (name, d) in rows {
            eprintln!("  {name:<18} {:>8.3} s", d.as_secs_f64());
        }
    }
    Ok(())
}

fn run_explain(args: ExplainArgs) -> Result<()> {
    let cfg = args.engine.to_config();
    let root = args
        .path
        .canonicalize()
        .with_context(|| format!("no such path: {}", args.path.display()))?;

    // Same engine `scan` uses, so the card's refs (F#/R#) agree with what a
    // scan of this tree would show.
    let analysis = run::analyze(&root, &cfg);

    // Bare `akron explain <root>` (TKI-51): no target to resolve, so there's
    // nothing for the generic error path below to report — print usage and
    // exit 2 directly rather than threading a fake error through `Result`.
    let Some(target) = args.target.as_deref() else {
        explain::print_usage(&args.path, &analysis.scanned.symbols);
        std::process::exit(2);
    };
    explain::explain(&root, &analysis, &cfg, target)
}

/// `akron find <path> <query>` — isolated from every other code path above.
fn run_find(args: FindArgs) -> Result<()> {
    let root = args
        .path
        .canonicalize()
        .with_context(|| format!("no such path: {}", args.path.display()))?;

    let report = find::search(&root, &args.query, args.top, args.tests)?;

    match args.json.as_deref() {
        Some(path) if path == Path::new("-") => {
            println!("{}", serde_json::to_string_pretty(&find::render_json(&report))?);
        }
        Some(path) => {
            let v = find::render_json(&report);
            fs::write(path, serde_json::to_vec_pretty(&v)?)
                .with_context(|| format!("writing {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
        None => find::render_text(&report),
    }
    Ok(())
}

/// `akron review <path> [--base <ref>]` — same JSON/stdout-or-file
/// convention as `find`'s `--json`.
fn run_review(args: ReviewArgs) -> Result<()> {
    let root = args
        .path
        .canonicalize()
        .with_context(|| format!("no such path: {}", args.path.display()))?;

    let report = review::review(&root, args.base.as_deref())?;

    match args.json.as_deref() {
        Some(path) if path == Path::new("-") => {
            println!("{}", serde_json::to_string_pretty(&review::render_json(&report))?);
        }
        Some(path) => {
            let v = review::render_json(&report);
            fs::write(path, serde_json::to_vec_pretty(&v)?)
                .with_context(|| format!("writing {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
        None => review::render_text(&report),
    }
    Ok(())
}
