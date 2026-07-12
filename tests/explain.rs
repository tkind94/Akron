//! `akron explain` (EXP-C): resolution end-to-end through the compiled
//! binary, mirroring `tests/check.rs`'s / `tests/families_flag.rs`'s
//! CARGO_BIN_EXE convention so this also exercises `main.rs`'s flag wiring
//! and exit codes, not just the library function.
//!
//! `--min-nodes 1` is used throughout: several of the planted fixtures this
//! module reuses (`todict_core.py`/`todict_member.py`, in particular) are
//! deliberately tiny and would otherwise be skipped by the default
//! `min_nodes` gate before resolution ever runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn run(target: &str) -> Output {
    run_in(&fixtures_root(), target)
}

/// Same as `run`, against an arbitrary root — used by the TKI-42 tests below,
/// which plant their own throwaway corpora in a temp dir rather than growing
/// the shared `tests/fixtures/` tree.
fn run_in(root: &Path, target: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_akron"))
        .arg("explain")
        .arg(root)
        .arg(target)
        .args(["--min-nodes", "1"])
        .output()
        .expect("run akron explain")
}

/// Same as `run_in`, but omits the target entirely (TKI-51 bare-root case).
fn run_bare_in(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_akron"))
        .arg("explain")
        .arg(root)
        .args(["--min-nodes", "1"])
        .output()
        .expect("run akron explain (bare root)")
}

#[test]
fn exact_qname_resolves_to_its_card() {
    let out = run("parse_records");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("clone_original.py:4  parse_records"),
        "card must open with the resolved symbol's own location:\n{stdout}"
    );
    // The renamed clone (identical Merkle root) is the exact clone; the
    // planted near-miss is the near clone — both already-computed facts.
    assert!(
        stdout.contains("exact: extract_rows"),
        "clones section must name the exact clone:\n{stdout}"
    );
    assert!(
        stdout.contains("near: parse_records_v2"),
        "clones section must name the near-miss:\n{stdout}"
    );
}

#[test]
fn file_line_containment_resolves_to_the_enclosing_symbol() {
    // Line 10 sits inside `parse_records`'s body (def at line 4, body through
    // line 17) — containment, not an exact `def`-line match.
    let out = run("clone_original.py:10");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("clone_original.py:4  parse_records"),
        "a line inside the body must resolve to the enclosing symbol's own \
         def-line card, not the queried line:\n{stdout}"
    );
}

#[test]
fn ambiguous_name_lists_candidates_and_exits_2() {
    // `to_dict` is planted twice (todict_core.py, todict_member.py) — an
    // exact-qname collision across two files, the deliberate ambiguity case.
    let out = run("to_dict");
    assert_eq!(
        out.status.code(),
        Some(2),
        "ambiguous resolution must exit 2: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ambiguous"), "must say why: {stderr}");
    assert!(
        stderr.contains("todict_core.py") && stderr.contains("todict_member.py"),
        "must list every candidate: {stderr}"
    );
}

// ── TKI-74: facts (corpus-grounded prevalence) ──

#[test]
fn facts_line_states_shape_and_family_prevalence_against_fixtures() {
    // parse_records' repeated-shape cluster is 3 members (itself, the
    // renamed exact clone, the near-miss) across 3 files; its pattern family
    // additionally pulls in the drifted variant, 4 members across 4 files —
    // the two altitudes DESIGN.md §3.1 distinguishes, both already-computed.
    let out = run("parse_records");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "facts       one of 3 symbols with this shape, across 3 files; the only symbol named \"parse_records\"; one of 4 members in family F1, across 4 files"
        ),
        "facts line must state shape and family prevalence, in that order:\n{stdout}"
    );
}

#[test]
fn facts_line_uses_singular_file_when_span_is_one() {
    // AlphaSuite.run's clone cluster and family live entirely inside
    // role_guard_classes.py — the span must read "1 file", never "1 files".
    let out = run("role_guard_classes.py:31");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "facts       one of 2 symbols with this shape, in the same file; one of 3 named \"run\"; one of 3 members in family F3, in the same file"
        ),
        "single-file spans must read \"in the same file\", never \"across 1 files\":\n{stdout}"
    );
}

#[test]
fn facts_line_states_name_prevalence_even_when_shape_is_unique() {
    // to_dict is planted twice (todict_core.py, todict_member.py) with
    // deliberately different bodies (no shared shape or family), so this
    // isolates the name-prevalence fact from the shape-prevalence fact.
    let out = run("todict_core.py:1");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "facts       one of 2 named \"to_dict\"; the only symbol with this shape"
        ),
        "facts line must report the name collision even without a shape/family match:\n{stdout}"
    );
    let facts_line = stdout.lines().find(|l| l.starts_with("facts")).unwrap();
    assert!(
        !facts_line.contains("family"),
        "no family section for a symbol with no family membership:\n{facts_line}"
    );
}

#[test]
fn constructor_name_prevalence_keys_off_the_class_not_dunder_init() {
    // Every class defines an `__init__`, so counting occurrences of the
    // literal name "__init__" would be a near-universal, useless number.
    // The fact must key off the class name instead (mirrors `call_targets`'s
    // own constructor join) — two classes both named `Widget` (different
    // files) share that count; `Gadget`, defined once, is corpus-unique.
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("a.py"),
        "class Widget:\n    def __init__(self, x):\n        self.x = x\n        return x\n",
    )
    .unwrap();
    fs::write(
        pkg.join("b.py"),
        "class Widget:\n    def __init__(self, y, z):\n        self.y = y\n        self.z = z\n        total = y + z\n        return total\n",
    )
    .unwrap();
    fs::write(
        pkg.join("c.py"),
        "class Gadget:\n    def __init__(self, q):\n        self.q = q\n        return q\n",
    )
    .unwrap();

    let out = run_in(dir.path(), "pkg/a.py:2");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("one of 2 named \"Widget\""),
        "must key the count off the class name, not \"__init__\":\n{stdout}"
    );

    let out = run_in(dir.path(), "pkg/c.py:2");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("the only symbol named \"Gadget\""),
        "a singly-defined class's constructor must not inherit every other class's __init__ count:\n{stdout}"
    );
}

#[test]
fn facts_line_reports_corpus_unique_shape_and_name_for_a_lone_symbol() {
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("a.py"),
        "def lonely_fn(a, b, c):\n    total = a + b\n    total += c\n    return total\n",
    )
    .unwrap();

    let out = run_in(dir.path(), "lonely_fn");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let facts_line = stdout.lines().find(|l| l.starts_with("facts")).unwrap();
    assert_eq!(
        facts_line,
        "facts       nothing else in the corpus shares this shape or name",
        "a wholly unmatched symbol must report both prevalence facts as 1, with no family part:\n{stdout}"
    );
}

// ── TKI-42: entry-point tag (salvaged from R&D archive spike/orient) ──

#[test]
fn entry_point_tag_marks_the_directorys_most_called_symbol() {
    // akron's own walker skips dot-prefixed directories (`src/parse.rs`'s
    // `SKIP_DIRS`/hidden-dir convention) — including the root itself — so
    // a plain `TempDir::new()` (named `.tmpXXXX`) would scan to nothing.
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("a.py"), "def hub():\n    return 1\n").unwrap();
    fs::write(
        pkg.join("b.py"),
        "from pkg.a import hub\n\n\
         def caller_one():\n    return hub()\n\n\
         def caller_two():\n    return hub()\n\n\
         def caller_three():\n    return hub()\n",
    )
    .unwrap();

    let out = run_in(dir.path(), "hub");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let header = String::from_utf8_lossy(&out.stdout).lines().next().unwrap().to_string();
    assert!(
        header.contains("entry 3\u{d7}"),
        "hub is called by 3 distinct symbols in its directory, the max: {header}"
    );

    // `caller_one` has no callers of its own, so it isn't its directory's
    // champion — no tag, matching the module's section-omission discipline.
    let out = run_in(dir.path(), "caller_one");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let header = String::from_utf8_lossy(&out.stdout).lines().next().unwrap().to_string();
    assert!(!header.contains("entry"), "not the directory's champion: {header}");
}

// ── TKI-42: generic-verb caller/callee noise ──

#[test]
fn generic_verb_calls_are_excluded_unless_same_file_or_corpus_unique() {
    // non-dot-prefixed temp dir — see the sibling test above for why.
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let widgets = dir.path().join("widgets");
    fs::create_dir_all(&widgets).unwrap();
    // `get` is defined twice (Foo.get, Bar.get) — a generic-verb collision:
    // both are named identically, and `call_local`'s `obj.get()` call can't
    // statically resolve which one it means (`obj` is a local, not an
    // import).
    fs::write(
        widgets.join("one.py"),
        "class Foo:\n    def get(self):\n        return 1\n\n\
         def call_local():\n    obj = Foo()\n    return obj.get()\n",
    )
    .unwrap();
    fs::write(widgets.join("two.py"), "class Bar:\n    def get(self):\n        return 2\n").unwrap();
    // `frobnicate` is corpus-unique, called cross-file through an equally
    // unresolvable object (a bare parameter) — must still count: the fix
    // targets name ambiguity, not import-resolution distance.
    fs::write(
        widgets.join("three.py"),
        "class Baz:\n    def frobnicate(self):\n        return 3\n",
    )
    .unwrap();
    fs::write(widgets.join("four.py"), "def call_it(b):\n    return b.frobnicate()\n").unwrap();

    // Foo.get: `call_local` lives in the SAME file — trusted despite the
    // corpus-wide name collision.
    let out = run_in(dir.path(), "Foo.get");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("callers     1 \u{2014} call_local"),
        "same-file caller of a same-named-elsewhere method must still count:\n{stdout}"
    );

    // Bar.get: the only textual match for `.get(` anywhere is `call_local`'s
    // obj.get() in a DIFFERENT file — a cross-file, ambiguous name, and must
    // be excluded rather than misattributed.
    let out = run_in(dir.path(), "Bar.get");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("callers     0"),
        "cross-file generic-verb match must not be counted as a caller:\n{stdout}"
    );

    // Baz.frobnicate: cross-file and unresolvable too, but corpus-unique —
    // must still count.
    let out = run_in(dir.path(), "Baz.frobnicate");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("callers     1 \u{2014} call_it"),
        "cross-file caller of a corpus-unique name must still count:\n{stdout}"
    );
}

#[test]
fn builtin_names_never_gain_cross_file_callers_even_when_corpus_unique() {
    // TKI-54 (found by the explore guide panel): `join` is corpus-unique as
    // a DEFINITION, but `", ".join(…)` already means str.join — httpx's
    // URL.join inherited 27 fake callers this way. Cross-file unresolved
    // calls to a Python-builtin name must be excluded; same-file calls stay
    // trusted (own-scope resolution).
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("url.py"),
        "class URL:\n    def join(self, other):\n        return self.raw + other\n\n\
         def build(u):\n    return u.join(\"/x\")\n",
    )
    .unwrap();
    fs::write(
        pkg.join("render.py"),
        "def render_lines(lines):\n    return \", \".join(lines)\n",
    )
    .unwrap();

    let out = run_in(dir.path(), "URL.join");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("callers     1 \u{2014} build"),
        "same-file caller counts; render_lines's str.join must not:\n{stdout}"
    );
}

#[test]
fn super_init_chains_do_not_cross_wire_sibling_constructors() {
    // TKI-54 (found by the explore guide panel): every `super().__init__()`
    // in httpx's _exceptions.py was attributed to every same-file
    // `__init__` — 13 fake callers each. Dunder bases never bind by name;
    // the real constructor edge (`raise BadInput(…)`) flows through the
    // class-name path and must still count.
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("errors.py"),
        "class BadInput(Exception):\n    def __init__(self, message):\n        super().__init__(message)\n        self.message = message\n\n\
         class BadOutput(Exception):\n    def __init__(self, message):\n        super().__init__(message)\n        self.message = message\n",
    )
    .unwrap();
    fs::write(
        pkg.join("check.py"),
        "def validate(x):\n    if not x:\n        raise BadInput(\"empty\")\n    return x\n",
    )
    .unwrap();

    // BadOutput.__init__ is never constructed: BadInput's super().__init__
    // chain must not count as its caller.
    let out = run_in(dir.path(), "BadOutput.__init__");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("callers     0"),
        "a sibling's super().__init__ is not a caller:\n{stdout}"
    );

    // BadInput.__init__ IS constructed (raise BadInput(…)) — the class-name
    // constructor edge still counts.
    let out = run_in(dir.path(), "BadInput.__init__");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("callers     1 \u{2014} validate"),
        "class-name constructor call must still count:\n{stdout}"
    );
}

// ── TKI-51: widened resolution ──

#[test]
fn unique_case_insensitive_substring_resolves_to_its_card() {
    // "ROWS" matches no qname exactly and no qname's dotted suffix (nothing
    // in the fixture tree has a `.` before it anywhere in this word), but is
    // a case-insensitive substring of exactly one qname: `extract_rows`.
    let out = run("ROWS");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("clone_renamed.py:4  extract_rows"),
        "a unique case-insensitive substring must resolve to its own card:\n{stdout}"
    );
}

#[test]
fn exact_case_sensitive_match_wins_over_case_insensitive_ambiguity() {
    // `get` and `Get` are two distinct top-level functions (case-sensitive
    // qnames). Querying the lowercase form must resolve directly to `get`
    // (tier 1, exact) rather than falling into the substring tier, which
    // would case-fold both into a 2-way ambiguity.
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("a.py"), "def get():\n    return 1\n").unwrap();
    fs::write(pkg.join("b.py"), "def Get():\n    return 2\n").unwrap();

    let out = run_in(dir.path(), "get");
    assert!(out.status.success(), "exit code: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("pkg/a.py:1  get"),
        "exact case-sensitive match must win over the case-insensitive pair:\n{stdout}"
    );
}

#[test]
fn ambiguous_substring_ranks_non_test_first_then_by_node_count_desc() {
    // Three top-level functions sharing the substring `nozzle` (none is an
    // exact or dotted-suffix match for the bare query, so this is resolved
    // entirely by the new substring tier): two ordinary source files of
    // different sizes, and one under `tests/` that is the biggest of all
    // three by node count. Expected order: non-test candidates first
    // (larger body first), the test candidate last regardless of its size.
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    let tests_dir = dir.path().join("tests");
    fs::create_dir_all(&pkg).unwrap();
    fs::create_dir_all(&tests_dir).unwrap();
    fs::write(pkg.join("a.py"), "def nozzle_alpha():\n    return 1\n").unwrap();
    fs::write(
        pkg.join("b.py"),
        "def nozzle_beta():\n    x = 1\n    y = 2\n    z = 3\n    return x + y + z\n",
    )
    .unwrap();
    fs::write(
        tests_dir.join("test_c.py"),
        "def nozzle_gamma():\n    a = 1\n    b = 2\n    c = 3\n    d = 4\n    e = 5\n    return a + b + c + d + e\n",
    )
    .unwrap();

    let out = run_in(dir.path(), "nozzle");
    assert_eq!(
        out.status.code(),
        Some(2),
        "ambiguous substring must exit 2: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("3 candidates"), "{stderr}");
    let pos_beta = stderr.find("pkg/b.py:1").expect("nozzle_beta listed");
    let pos_alpha = stderr.find("pkg/a.py:1").expect("nozzle_alpha listed");
    let pos_gamma = stderr.find("tests/test_c.py:1").expect("nozzle_gamma listed");
    assert!(
        pos_beta < pos_alpha && pos_alpha < pos_gamma,
        "non-test candidates must rank first (larger body first), test candidate last regardless of its size:\n{stderr}"
    );
}

#[test]
fn ambiguous_list_caps_at_12_with_narrow_the_pattern_suffix() {
    // 14 top-level functions sharing the substring `widget`, none an exact
    // or suffix match — 2 more than the 12-row cap.
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    for i in 0..14 {
        fs::write(pkg.join(format!("w{i}.py")), format!("def widget_{i}():\n    return {i}\n")).unwrap();
    }

    let out = run_in(dir.path(), "widget");
    assert_eq!(out.status.code(), Some(2), "ambiguous substring must exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("14 candidates"), "{stderr}");
    assert!(
        stderr.contains("+2 more \u{2014} narrow the pattern"),
        "must state how many were cut and invite narrowing:\n{stderr}"
    );
    let row_count = stderr.matches("  widget_").count();
    assert_eq!(row_count, 12, "must print exactly 12 rows:\n{stderr}");
}

#[test]
fn bare_root_prints_usage_and_examples_then_exits_2() {
    let dir = tempfile::Builder::new()
        .prefix("akron-explain-test-")
        .tempdir()
        .expect("tempdir");
    let pkg = dir.path().join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("a.py"), "def small_fn():\n    return 1\n").unwrap();
    fs::write(
        pkg.join("b.py"),
        "def medium_fn():\n    a = 1\n    b = 2\n    c = 3\n    d = 4\n    return a + b + c + d\n",
    )
    .unwrap();
    fs::write(
        pkg.join("c.py"),
        "def large_fn():\n    a = 1\n    b = 2\n    c = 3\n    d = 4\n    e = 5\n    f = 6\n    \
         g = 7\n    h = 8\n    return a + b + c + d + e + f + g + h\n",
    )
    .unwrap();
    let tests_dir = dir.path().join("tests");
    fs::create_dir_all(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("test_d.py"),
        "def huge_test_fn():\n    a = 1\n    b = 2\n    c = 3\n    d = 4\n    e = 5\n    f = 6\n    \
         g = 7\n    h = 8\n    i = 9\n    j = 10\n    k = 11\n    return a\n",
    )
    .unwrap();

    let out1 = run_bare_in(dir.path());
    assert_eq!(
        out1.status.code(),
        Some(2),
        "target-omitted invocation must exit 2: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out1.stdout),
        String::from_utf8_lossy(&out1.stderr)
    );
    let stderr1 = String::from_utf8_lossy(&out1.stderr).to_string();
    let lines: Vec<&str> = stderr1.lines().collect();
    assert_eq!(
        lines.len(),
        4,
        "one usage line plus exactly three examples:\n{stderr1}"
    );
    assert_eq!(lines[0], "usage: akron explain <root> <target>");
    // Largest non-test symbols first, as a suffix-form target; the third
    // (smallest of the three shown) as a file:line target. The biggest
    // symbol of all (`huge_test_fn`) is a test symbol and must never appear.
    assert!(lines[1].contains("large_fn"), "{stderr1}");
    assert!(lines[2].contains("medium_fn"), "{stderr1}");
    assert!(lines[3].contains("pkg/a.py:1"), "{stderr1}");
    assert!(
        !stderr1.contains("huge_test_fn"),
        "test symbols must never appear in bare-root examples:\n{stderr1}"
    );

    // Determinism: the same command run twice produces identical output.
    let out2 = run_bare_in(dir.path());
    assert_eq!(out1.stdout, out2.stdout);
    assert_eq!(out1.stderr, out2.stderr);
}
