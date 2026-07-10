---
name: akron
description: Consult Akron's derived shape/vocabulary landscape before writing new code, when searching for where a behavior is handled (`akron find`), or before modifying an unfamiliar symbol (`akron explain`). Use whenever writing a new helper/function in a repo, when searching for where a behavior is handled, or before modifying an unfamiliar symbol.
---

# Akron: consult the pattern landscape instead of reinventing it

Akron recomputes a repo's shape/vocabulary landscape (repeated shapes,
shared-vocabulary/divergent-shape pairs, drift) from code + git history on
every run — nothing here is prose that can rot, and nothing here is stored.
This skill is the CLI contract for three moments (JOURNEYS.md J1-J4, J9-J10).
Depth: README.md, JOURNEYS.md, DESIGN.md.

All flags/fields below are verified against the `akron` binary (`--help` on
each subcommand is the source of truth if this drifts). The `schema` field,
each finding's `ref`, and `--json -` were verified 2026-07-05 by running
`akron scan` directly against tests/fixtures and an external corpus
(TKI-27); the `--only`-gated JSON shape was verified 2026-07-06 (TKI-45).
`scan` lost its whole human surface (digest, `--full`, `--html`, `--top`) in
TKI-50 — `--json` is now required in effect and is the only thing `scan`
does; the human view of this data is `akron explore`.

## 1. Before searching for where something is handled

Run `akron find <root> "<question>"` (first use pulls a 331 MB pinned model;
needs a build with the `semantic` feature — if it exits 2, fall back to
`rg`). Embeds the question and every symbol, ranks by cosine, prints
`file:line qname` for the closest matches; `--tests` includes test symbols,
`--json` for scripting. Ranking only: a pinned local embedding model may
reorder results; it never changes `scan` output. Strongest on
domain-vocabulary repos (P@5 0.68 vs a best-faith grep's 0.36); on
library-shaped code the margin is thin (0.50 vs 0.40) — run both when
unsure.

## 2. Before modifying an unfamiliar symbol

Run `akron explain <root> [target]` (`target`: an exact qname, `file:line`, a
unique dotted-suffix, or a unique case-insensitive substring — omit it for
real example targets from the repo; two-or-more matches print a ranked
did-you-mean list instead of guessing). One card, read off the same analysis
`scan` already computes: near-clones, role twins (shared vocabulary, different
shape), callers/callees (import-aware), family membership — no new
detection. Graded 8 wins / 3 ties / 1 loss against ~5 minutes of `rg` +
reading; every "is this the one to use" case was a clear win. Ambiguous
callers (generic base names on unresolvable objects) are excluded rather
than guessed — `callers 0` means none resolved reliably, not none exist.
Caveat: dict/variable-dispatched calls (`handlers[name](...)`) are invisible
to the caller/callee pass; cross-check with `rg` when that matters.

## 3. Before writing a new helper or function

Run `akron scan <root> --json <tmp-file>` (JSON goes to the file you name),
or `akron scan <root> --json -` to get pure JSON on stdout instead (safe to
pipe straight into a JSON parser — `scan` has no other output mode; bare
`akron scan <root>` with no `--json` just prints a pointer to `--json` and to
`akron explore`, and exits 2). **`repeated`/`deprecated` are always in the
JSON; `families`/`competing` are opt-in** — pass `--only families` or
`--only competing` if you need them (no other `--only` value exists, since
there's nothing left to opt in).

Read these JSON arrays (all under the top-level object, versioned
`"schema": "akron.scan/v1"`):
- `repeated[]` — near-identical shapes across the repo. Always populated.
  Each entry: `ref` (`R#`, matches its own position in this array),
  `members[]` (`file`, `line`, `qname`, `nodes`, `is_test`), `n_files`,
  `dating.activity` (`growing`/`flat`/`dead`).
- `families[]` — a repeated cluster plus its drifted variants: `ref` (`F#`),
  `core` qname, `core_size`, `drift_size`, `members[].cos_to_core`.
  Populated only with `--only families`.
- `competing[]` — same vocabulary, different shape (two implementations of one
  job): `ref` (`C#`), `members[]`, `shared_terms[]`, `b_max`/`a_at_best`
  cosines. Populated only with `--only competing`.
- `deprecated[]` — a dead cluster paired with a growing role-twin (a
  git-history measurement, not a judgment about which to use). Always
  populated when the repo has git history.

**Before writing a new function, check whether a `repeated`/`families`/
`competing` entry already covers this job** (match on `shared_terms` /
qname / file area). A hit doesn't tell you which shape is "right" — Akron
doesn't judge that — but it tells you a similar shape already exists, so
extend it or say so in the PR/commit message rather than adding a silent
Nth copy.

Each entry's `ref` is stable within a working tree — it is that entry's own
1-based position in its array (`R1`, `R2`, ... `F1`, ... `C1`, ...), so it
never has to be re-derived (TKI-27).

## Worked example

```
$ akron scan . --json -
```
```json
{
  "schema": "akron.scan/v1",
  "repeated": [{
    "ref": "R1",
    "members": [
      {"file": "fb.py", "line": 1, "qname": "fetch_b", "nodes": 41, "is_test": false},
      {"file": "fc.py", "line": 1, "qname": "fetch_c", "nodes": 41, "is_test": false}
    ],
    "n_files": 2,
    "dating": {"activity": "flat"}
  }],
  "families": [],
  "competing": []
}
```
About to write `fetch_d` that does what `fetch_b`/`fetch_c` already do? Extend
one of those two instead of writing a third near-identical copy, or note the
duplication in the PR so a human can decide whether to consolidate.
