//! TKI-23: `@overload` stubs (bare or dotted, e.g. `@t.overload`) must be
//! dropped during extraction — their body is `...` and carries no shape or
//! vocabulary signal. Only the real implementation should survive as the
//! extracted symbol, and decorators that merely contain the substring
//! "overload" (e.g. `@my_overload_helper`) must not be mistaken for it.

use akron::scan;
use akron::types::Config;
use std::path::Path;

fn cfg() -> Config {
    Config {
        min_nodes: 10,
        wl_iters: 3,
        theta_clone: 0.60,
        theta_b: 0.55,
        theta_a_low: 0.30,
        theta_family: 0.35,
        theta_b_family: 0.16,
        top: 20,
    }
}

#[test]
fn overload_stubs_dropped_implementation_and_decoy_survive() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let out = scan::scan_repo(&root, &cfg());

    let from_fixture: Vec<&str> = out
        .symbols
        .iter()
        .filter(|s| s.sym.file == "overloaded.py")
        .map(|s| s.sym.qname.as_str())
        .collect();

    assert_eq!(
        from_fixture.len(),
        2,
        "expected exactly the implementation + decoy, got: {from_fixture:?}"
    );
    assert!(
        from_fixture.contains(&"normalize_input"),
        "the implementation (not the @overload stubs) must be extracted: {from_fixture:?}"
    );
    assert!(
        from_fixture.contains(&"process_batch"),
        "the @my_overload_helper decoy must still be extracted: {from_fixture:?}"
    );
}
