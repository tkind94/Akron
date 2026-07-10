# Akron

A reviewer opens a PR and realizes it's the *third* implementation of the same
retry logic in this codebase — and can't say where the other two live.

A new engineer finds two connection helpers sitting side by side. "Which one
do I copy?"

An AI coding agent reinvents a helper that already exists, because nothing
told it what's already there.

**Akron is a local codebase exploration tool.** Three verbs today, each
graded before shipping against a real alternative (grep, manual reading):

| Verb | Question | Status |
|---|---|---|
| [`find`](#find--semantic-search-what-am-i-looking-for) | what am I looking for | shipped |
| [`explain`](#explain--one-card-per-symbol-what-is-this-thing) | what is this thing | shipped |
| [`explore`](#explore--the-live-map-what-neighborhoods-does-this-codebase-have) | what neighborhoods does this codebase have | shipped |

`scan` — the shape/vocabulary engine behind all three — is no longer a human
verb; see [For tooling](#for-tooling-scan---json) below.

Read [Honest scope](#honest-scope) before adopting this on a repo unlike the
ones it was graded on — it names where each verb is strong and where a good
grep or plain reading is still the better default.

---

## `find` — semantic search (what am I looking for)

**Shipped.** Embeds a natural-language question and every
symbol in the repo, ranks by cosine similarity, prints `file:line qname` for
the closest matches — a complement to grep, not a replacement for it (see
Honest scope). Ranking only: a pinned local embedding model may reorder
`find`'s results; it never changes `scan`/family/competing output (DESIGN.md
§1.2). First use pulls the model (embeddinggemma-300m-q, 331 MB, every
runtime file sha256-pinned; fetched from Hugging Face under the model's own
terms — Gemma Terms of Use — Akron redistributes nothing) into
`~/.cache/akron/models`. The embedding index is cached outside the repo and
re-embeds only what changed: a cold index takes ~1–2 minutes on a
1k-symbol repo, once; warm queries answer in ~2 s. Test symbols are dropped
from the ranking unless `--tests` is passed.

Real, unedited output against a public clone of
[httpx](https://github.com/encode/httpx):

```
$ akron find . "how do I stream a large response without loading it into memory" --top 5
 1  0.50  Client.stream  httpx/_client.py:828
 2  0.49  AsyncClient.stream  httpx/_client.py:1543
 3  0.47  stream  httpx/_api.py:124
 4  0.46  ResponseStream.__init__  httpx/_transports/default.py:122
 5  0.45  AsyncResponseStream.__init__  httpx/_transports/default.py:266
```

## `explain` — one card per symbol (what is this thing)

**Shipped.** One screen for a single symbol: near-clones, role twins (shared
vocabulary, different shape), callers/callees, an entry-point tag, and family
membership — everything the engine already knows about it, read off the same
analysis `scan` computes. Graded 8 wins / 3 ties / 1 loss against ~5 minutes
of `rg` + reading, across 12 symbols picked cold on three real corpora
(R&D archive validation/explain-eval.md); the remaining loss traces to one documented
limitation — dict/variable-dispatched calls (`handlers[name](...)`) are
invisible to the caller/callee pass.

`<target>` (now optional — omit it for real example targets from the scanned
repo) takes an exact qname, `file:line`, a unique dotted-suffix, or (TKI-51) a
unique case-insensitive substring, in that preference order, printing a
ranked did-you-mean list instead of guessing when two or more symbols match.

Real, unedited output against a public clone of
[httpx](https://github.com/encode/httpx):

```
$ akron explain /tmp/akron-corpora/httpx httpx/_client.py:1123
httpx/_client.py:1123  Client.post  (172 nodes)
clones      exact: put httpx/_client.py:1160, patch httpx/_client.py:1197 · near: post httpx/_client.py:1838 (0.93), put httpx/_client.py:1875 (0.93), +23 more
role twins  (vocab ≥0.55, different shape): none
callers     0; callees 2 — request, request  [import-aware]
```

`callers 0` is deliberate: `post` is a generic base name called on many
client objects, so the caller pass refuses to guess rather than list 28
maybes — precision over recall (R&D archive validation/explain-eval.md grades this exact
choice).

---

## `explore` — the live map (what neighborhoods does this codebase have)

**Shipped.** `akron explore <path>` scans the repo, embeds every symbol
(same pinned model and cache as `find`), and serves an interactive map on
localhost:

```
$ akron explore .
explore — 1134 symbols — http://127.0.0.1:4816
```

Four views, each named for the question it answers:

- **map** — what neighborhoods does this codebase have? A
  neighborhood-preserving 2D layout of the full embedding space
  (8-nearest-neighbor graph, deterministic force layout — not a PCA
  projection, which measurably collapses small modules;
  R&D archive spike/embed2/RESULTS.md). Islands are modules; color is directory; point
  size tracks symbol size. Test symbols are hidden by default — on measured
  repos they smear every projection — the toggle (or `--tests`) shows them.
- **anchor** — what else is like the selected symbol, and in which way?
  x = structure similarity, y = vocabulary similarity to it, straight from
  the deterministic channels.
- **time** — what's active, what's untouched? Real date axis from git
  history (file-level dating today).
- **axes** — free dimension pickers (PCA components, similarities, age)
  for power use.

Click any point for its `explain` card — clones with cosines, twins,
callers/callees, every reference clickable. The search box is `find` over
the live index; hits highlight on the map, Enter jumps to the top hit.

On a feature branch, explore marks what the branch changed: symbols whose
content is absent from the merge-base against the default branch
(`--base <ref>` overrides; uncommitted and untracked work counts) get a
`⎇` chip — `new-decoder · 4 changed vs origin/master` — that toggles
branch focus: branch-new points wear a double ring, everything else
recedes. Each branch-new symbol's card opens with **nearest existing** —
the closest already-existing implementations, ranked by the embedding
space, annotated with the deterministic structure/vocabulary cosines.

Everything on screen is derived per run; the server holds it in memory and
writes nothing into the repo. Layout and PCA are deterministic — the same
repo state renders the same map.

## For tooling: `scan --json`

`scan` is the engine behind `find`/`explain`/`explore` — three independent
lenses over every symbol (repeated shapes, shared-vocabulary/divergent-shape
pairs, and, with git history, dead-vs-growing pairs; full derivation in
[DESIGN.md](DESIGN.md)) — but it has no human view of its own (TKI-50): bare
`akron scan <path>` just points at `--json` and at `explore` and exits 2.
`--json <file|->` is the whole surface; `--only <section>` populates
`families`/`competing` (empty otherwise — they read as an assertion when
surfaced unprompted; `repeated`/`deprecated` are always populated).
skills/akron/SKILL.md documents the full field list for a script or agent
reading this.

Real, unedited excerpt (keys only, trimmed to one small cluster) against the
same httpx clone:

```json
{
  "schema": "akron.scan/v1",
  "repeated": [{
    "ref": "R6",
    "members": [
      {"file": "httpx/_models.py", "line": 496, "qname": "Request.__repr__", "nodes": 41, "is_test": false},
      {"file": "httpx/_urls.py", "line": 626, "qname": "QueryParams.__repr__", "nodes": 34, "is_test": false}
    ],
    "n_files": 2,
    "total_nodes": 75,
    "all_test": false,
    "dating": null
  }],
  "families": [],
  "competing": []
}
```

## How it works

Akron stores no content. Docs, ADRs, golden examples, and rule libraries all
rot the moment the team moves on, because nothing breaks when reality
diverges from them. Akron instead recomputes everything — fingerprints,
clusters, adoption history — from scratch on every run, as a pure function of
the repo. `scan` and `explain` are **deterministic and model-free**: no LLM,
no embeddings-as-oracle, bit-for-bit reproducible given a repo state. The
sole exception is `find`'s *ranking* — a pinned local embedding model may
reorder its results, and only its results (DESIGN.md §1.2). Akron writes
nothing to the scanned repo; nothing it computes is stored anywhere.

## Honest scope

(Citations marked "R&D archive" refer to measurement records kept outside
this repo: they grade akron against private codebases, so their contents —
per-symbol tables and question sets — stay out of the distributable tree.)

Strong on domain-vocabulary, adapter-shaped codebases — the target: internal
work repos where identifiers speak the team's own vocabulary:

- `scan`: emits shape/vocabulary similarity as JSON, judgment-free — the
  numbers describe what the channels measured, not whether a finding is
  useful. The experimental family view (`--only families`) graded 0.976–0.989
  member precision on the two domain-shaped corpora it was calibrated against
  (R&D archive validation/family-membership.md).
- `explain`: 8 wins / 3 ties / 1 loss against manual `rg` + reading, across
  12 symbols picked cold (R&D archive validation/explain-eval.md).
- `find`: on a private domain-vocabulary codebase, P@5 0.68 vs a best-faith
  grep's 0.36 — semantic search recovers matches grep's lexical join
  structurally cannot (R&D archive spike/embed2/RESULTS.md, round 2, graded by reading
  the code behind every hit).

Weaker on library/protocol-shaped code, where identifiers already speak
plain English and a good grep is hard to beat:

- `find` on [httpx](https://github.com/encode/httpx) wins by a thin margin
  (P@5 0.50 vs grep's 0.40 with the shipped model; the previous model lost
  here) — when identifiers already speak the query's language, lexical
  match stays competitive; grep remains a good complement
  (R&D archive spike/embed2/RESULTS.md).
- The `scan` family view scores 0.702 member-level precision on httpx —
  below the 0.80 keep bar — which is why it ships **experimental**, populated
  only behind `--only families` (R&D archive validation/family-membership.md).

Other current limits, honest not aspirational:

- **Python only, today.** The parser is tree-sitter-Python; other languages
  are unimplemented, not degraded.
- **File-level dating.** A symbol's first-seen/last-touched dates are
  inherited from its file's git history, not its own hunks — every symbol in
  a file is dated identically until symbol-precise (blame-based) dating
  lands.

## Install

There is no `cargo install` or Homebrew tap yet. Build from source:

```
git clone <this repo> && cd akron
cargo build --release
./target/release/akron explore .
```

To build portable release binaries (macOS arm64/x86_64, Linux x86_64 musl)
with the version and commit baked in, run `./scripts/release.sh` — see the
script header for one-time toolchain setup; artifacts and `.sha256`
checksums land in `dist/`.

One caveat: only the native arm64 macOS artifact includes `find` — the
cross-compiled x86_64 macOS and musl artifacts are built without the
`semantic` feature (the ONNX runtime ships no prebuilt slices for those
cross targets) and `akron find` exits 2 there with a one-line message.
Building from source on the target machine includes `find` everywhere
fastembed supports.

## Status

Akron is pre-1.0. The CLI surface and JSON schema are still settling —
expect breaking changes between versions until a 1.0 tag says otherwise.

## Documents

- [DESIGN.md](DESIGN.md) — the derived-canon architecture, the
  three-channel code embedding, and the narrow embedding-ranking exception
  for `find`, in full.
- [JOURNEYS.md](JOURNEYS.md) — the personas and journeys this CLI is built
  to serve.
- [docs/POSITIONING.md](docs/POSITIONING.md) — why anyone comes looking, and
  who they find today.
- [PLAN.md](PLAN.md) — the implementation plan and phasing.
- [research/](research/) — pre-pivot research briefs (tool landscape,
  pattern representation, mining/anti-unification literature).
