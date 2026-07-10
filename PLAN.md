# Akron — Implementation Plan

**Proposal · 2026-07-03 · status: ratified by owner 2026-07-03 — Phase 0 in progress**

---

## 1. Stack recommendation: Rust

The workload is parse-everything, hash-everything, cluster-everything on every
run — CPU-bound and embarrassingly parallel — and the tool must ship as a
single portable binary that runs in a pre-commit hook. That points one way:

| Requirement | Rust answer |
|---|---|
| Heavy parse/hash/cluster workload | native speed; `rayon` data-parallelism across files |
| tree-sitter | first-class native bindings; the entire grammar ecosystem ships as crates; ast-grep proves the model |
| Later rule compilation | `ast-grep` is itself a Rust library — consumable as a crate, not shelled out to |
| Git history channel | `gix` (gitoxide): pure-Rust, fast one-pass log/diff walking, no libgit2 C dependency |
| Portability | static single binary, clean cross-compilation (no cgo, no interpreter) |
| Design philosophy | sum types, exhaustive matching, `Result` everywhere — the high-trust style is the language default |

**Alternative considered — Go:** faster to write, but tree-sitter goes through
cgo (which breaks the easy-cross-compilation argument), there is no ast-grep
library story, and error handling degrades the sum-typed verdict core.
Rejected. Python remains the *target* language of analysis, not the
implementation language.

Core crates: `tree-sitter` + `tree-sitter-python`, `gix`, `rayon`, `serde`/
`serde_json`, a fast non-crypto hash (`xxhash`/`blake3`), clap for the CLI.
No database in v0 — JSONL out, recompute is the storage model.

## 2. Phases

### Phase 0 — the falsification spike (the only phase that matters yet)

> **Status 2026-07-03: built; validated on corpus-P (a private pipeline repo); corpus-S (work
> machine) pending.** `akron scan` runs the full pipeline in ~0.15 s on a
> 95-file repo. Verified hits: a `_*_window` helper reinvented 7× across
> 3 files (repeated query) and three coexisting Postgres-connection
> patterns (competing query, B=0.78/A=0.26). Planted-fixture integration
> test covers a renamed clone pair and a sync-vs-async competing pair.
> Two implementation lessons are baked into the code and worth keeping:
> WL relabeling must be bidirectional (children-only freezes leaf labels
> and made everything ~0.5-similar), and single-link union-find needs a
> representative check (at 0.8·θ) or near-miss clusters chain into blobs.
> Calibrated defaults on the honest WL scale: θ_clone 0.60, θ_b 0.55,
> θ_a_low 0.30. **Closed (TKI-9):** the mid-similarity pattern-family gap
> (e.g. corpus-P's per-source ingest variants, pairwise A 0.36–0.76) now assembles
> at a second altitude — UPGMA average-linkage over tight clusters + drifted
> singletons, cut at θ_family 0.35, with a drift-gradient view (`family.rs`,
> DESIGN §3.1). On corpus-P 24/26 ingest jobs land in one family,
> with the `fetch_*` and `_*_row` house patterns held out as their own
> families.

Build the minimum pipeline that can be proven wrong:
parse → normalize → Channel A (Merkle + WL + MinHash/LSH) → Channel B
(TF-IDF vocab) → cluster → `akron scan --out clusters.jsonl` + a readable
report of the top clusters per query (repeated / competing).

- **Success criterion:** on a real repo with duplication the owner can see by
  eye, the high-B/low-A query surfaces the known competing-pattern cases
  (e.g. the two crawlers / two proxy methods) in a report scannable in
  minutes, at noise low enough to keep reading.
- **Kill criterion:** if the embedding cannot find by machine what the owner
  can find by eye — after reasonable threshold tuning — the design is wrong;
  stop and rethink before any further investment.
- Deliberately out of scope: git history, verdicts, gates, UI, any language
  beyond Python.

### Phase 1 — the time channel

One-pass `gix` history walk → file/hunk-level first-seen & last-touched →
adoption curves per cluster → the drifting / deprecated / dead queries.
Proves "deprecation is a measurement" on a real repo's history.

### Phase 2 — verdicts

`.akron/verdicts/` store; content-addressed anchoring, re-binding, expiry,
canon-health reporting (DESIGN.md §4). `akron ratify` as a minimal CLI flow
over scan output. This is where the derived canon becomes governable.

### Phase 3 — surfaces

`akron check` on a staged diff ("this matches a deprecated shape",
"role-similar helper exists at X") within a 1–5 s budget, plus an MCP/ask
surface so agents query the landscape before writing. Advisory before
blocking; a wrong block is the fastest route to `--no-verify` culture.

### Phase 4 — deferred until earned

Derived rule compilation (anti-unification → ast-grep, research/05), cluster
naming/rationale decoration via LLM, UI, additional target languages
(tree-sitter makes parsing cheap; normalization + callee resolution is the
real per-language cost).

## 3. Decisions (ratified by owner, 2026-07-03)

1. **Language: Rust.**
2. **Spike corpora: two private repos (corpus-P, corpus-S).** corpus-P is local and
   is the development-loop corpus; corpus-S lives on the work machine and is
   tested by carrying the (portable, self-contained) binary there — the repo
   is not synced here.
3. **Granularity: functions/methods only in v1.**
4. **research/ kept as reference.**

## 4. Risks

| Risk | Mitigation |
|---|---|
| Channel B too noisy (vocabulary collides across unrelated code) | TF-IDF damping; require minimum vocab mass; spike measures this directly |
| Thresholds don't transfer across repos | calibrate on 2–3 repos in phase 0; expose as config with sane defaults |
| History walk slow on big repos | one-pass log at file/hunk level (no blame); incremental caching later — cache is a pure derivation, safe to delete |
| Competing-pattern recall on disjoint-stack variants | accepted limit (DESIGN.md §5); candidates not proofs |
| Rust dev speed | phase 0 scope is deliberately small (~1–2 kLOC) |
