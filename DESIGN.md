# Akron — Design: The Derived Canon

**v1.0 · 2026-07-03 · supersedes the archived PRD/PROBLEM-SPACE (git history)**

---

## 0. The pivot, stated once

The previous direction (mine → ratify → **store** pattern objects with
rationale + exemplar + rule + index → enforce) had an anti-decay contract:
exemplar refs must resolve in CI, rules must match their exemplars. That
catches **mechanical rot** — a dangling ref, a rule that stopped matching.

It cannot catch **semantic rot**: the canon says B is canonical, the exemplar
still resolves, the rule still matches, CI is green — and the team has quietly
moved on to C. The stored artifact is *valid* while being *wrong*. That is the
ADR graveyard rebuilt one level up. Validation can only check internal
consistency, never whether the world moved.

Conclusion: the fix is not better validation of stored artifacts. It is
**storing less**. Everything the tool believes about the code is recomputed
from the code; the only durable state is human judgment.

## 1. Principles

1. **The repo is the only source of truth.** Every representation Akron holds
   is a pure function of `(working tree, git history)`, recomputed each run.
   Nothing recomputed can be stale.
2. **Deterministic, model-free core.** Channels A/B/C — shape, vocabulary,
   time — remain deterministic and model-free: no LLM, no learned embedding
   model, no sampling anywhere in detection. Given a repo state, their output
   is bit-for-bit reproducible and explainable.
   **Exception (2026-07-06, revised 2026-07-06 pivot 2):** a pinned local
   embedding model (`embeddinggemma-300m-q`, sha256-pinned, pulled on first
   use) powers the search and exploration surfaces — `akron find` and
   `akron explore`. There are no gates or verdicts
   left to protect, so the old "never gates, never anchors" law is replaced
   by a narrower one: **model output ranks and lays out; the deterministic
   channels remain the only source of the similarity NUMBERS shown beside
   code.** A model may decide *where something sits on screen or how high it
   ranks*; it never produces a cosine, a Merkle hash, or a date span — those
   are always Channel A/B/C's.
3. **Store judgments, not content.** The durable artifact is a verdict — a
   decision plus a reason — never an example, a copy, or a hand-written rule.
   Judgments age well ("we chose B over A for connection pooling" stays true
   as B's code evolves); content rots.
4. **Labels decorate, never gate.** Cluster names and rationale prose are
   human/LLM-supplied and may go stale; nothing in detection or enforcement
   reads them, so stale labels degrade UX, not correctness.
5. **Deprecation is a measurement, not a status field.** Git history is the
   one document that is always current, because it is written mechanically as
   a side effect of work. "Deprecated" = a cluster whose new-member rate died
   while a role-equivalent cluster grows. Nobody has to declare it.

## 2. The embedding

The deterministic representation at the heart of the tool. Unit of analysis:
**symbol** = function/method (v1); classes/modules as composites later.

It is deliberately **not one vector**. "Repeated", "competing", "drifting",
and "deprecated" are different questions; each needs a different projection of
the code. Three channels suffice.

### 2.1 Normalization (the make-or-break decision)

Parse with tree-sitter, then:

| Element | Treatment | Why |
|---|---|---|
| Comments / docstrings | stripped | not code |
| Local identifiers (params, locals) | alpha-renamed by binding order (`x0, x1…`) | naming noise defeats structural matching |
| Literals | abstracted to type tokens (`STR`, `NUM`, `BOOL`) | values are instance detail, not shape |
| **External names** — imports, resolved callees, attribute accesses, decorators, base classes, raised/caught exception types | **kept verbatim** | the code's vocabulary; strongest role signal |
| Identifier subwords (`fetch_with_proxy` → `fetch, with, proxy`) | kept as a bag (Channel B only) | domain vocabulary survives renaming conventions |

Normalize *local* names away; keep *external* names. Locals are noise;
externals are what the code talks to.

### 2.2 Channel A — shape (structure)

Catches: repeated / near-duplicate / reinvented structure (clone Types 1–3).

- **Merkle subtree hashes.** `h(node) = hash(kind ‖ h(child₁) ‖ … ‖ h(childₙ))`
  over the normalized AST; store hashes for subtrees ≥ k nodes. Exact
  structural clones at *every granularity* become a hash join — free, and the
  hash set doubles as the content-address for verdict anchoring (§4).
- **Weisfeiler–Leman label histograms** for near-misses. Iterate
  `label₀(n) = kind(n)`; `labelᵢ₊₁(n) = hash(labelᵢ(n) ‖ [labelᵢ(children) in order])`
  for i = 0…3; the multiset of all labels across iterations is a sparse vector.
  Deterministic, linear-time, and — unlike tree-only metrics (pq-grams) —
  generalizes unchanged to CFGs and call graphs later.
- **Index:** MinHash signatures over the label/hash sets → LSH bands for
  sub-linear candidate pairing; cosine over WL histograms to refine clusters.
- **Channel A is vocabulary-free** (refinement, 2026-07-03): external names
  contribute to A only as a generic `EXT` leaf token; locals contribute as
  their alpha index; literals as type tokens. If external names entered A,
  the channel would become partially vocabulary-sensitive and the
  high-B/low-A competing-pattern query (§3) would degenerate — A and B must
  stay orthogonal. External names verbatim are Channel B's job (§2.3).

### 2.3 Channel B — vocabulary (role)

Catches: shared-vocabulary/divergent-shape pairs — the two-crawlers /
two-proxy-methods case.

- Bag of kept external names + identifier subwords, **TF-IDF weighted over the
  repo** (so `requests.get` discriminates and `self` doesn't), as a sparse
  vector with its own MinHash/LSH index.

### 2.4 Channel C — time (history)

Catches: drifting, deprecated, dead.

- Per symbol: first-seen and last-touched dates from git history.
- Per cluster: the member-date distribution = an **adoption curve**. Growing,
  flat, or dead is a computation, not an opinion.

**Approximations (v0, as implemented in `src/history.rs`, TKI-7).** Prefer
simple-and-honest over clever-and-fragile:

1. **File-level dating, not symbol-level.** One first-parent walk from `HEAD`
   (`gix`) diffs each commit's tree against its first parent; every current
   path's `first_seen` / `last_touched` are the oldest / newest commit that
   touched it. Every symbol inherits its file's two dates — so symbols in the
   same file are dated identically. Symbol-precise (hunk/blame) dating is a
   later refinement; it needs line-lineage tracking that file-level dating
   deliberately avoids.
2. **Committer time, in the committer's timezone**, reduced to `YYYY-MM-DD`.
   Chosen over author time because it orders with the first-parent walk and is
   stable under `git log --first-parent`.
3. **No rename following.** Rename detection is disabled for determinism, so a
   path's `first_seen` is the earliest commit touching *that current path* —
   pre-rename history is attributed to the old path (matches `git log` without
   `--follow`).
4. **Deterministic, not wall-clock.** "Recent" is measured against the repo's
   newest commit (the **anchor**), never `now`, so the same commit graph always
   classifies the same way (Principle 1). Activity thresholds are constants:
   **dead** = newest touch older than 180 d before the anchor; **growing** =
   newest member born within 90 d; **flat** = otherwise. Evaluated in that
   order (deadness dominates, then growth).
5. **Graceful degradation.** When the scan root is not a git repo, every symbol
   is undated and the report/JSON say so explicitly ("no git history") — never
   a silent absence.

### 2.5 The embedding record

Per symbol, emitted as data for further processing (JSONL):

```json
{
  "symbol": "src/crawlers/feed.py#FeedCrawler.fetch",
  "merkle": ["9f3a…", "c41b…"],
  "wl": {"a91f…": 3, "0b7c…": 1},
  "vocab": {"httpx": 2.1, "retry": 1.4, "proxy": 3.2},
  "first_seen": "2023-11-02", "last_touched": "2026-05-19"
}
```

## 3. Queries: the pattern taxonomy falls out

| Question | Signal |
|---|---|
| Repeated / reinvented helper | high A across modules |
| **Competing patterns** | **high B, low A** — same vocabulary and job, different shape |
| Drifting | one A-cluster whose internal variance grows along the C axis, or that splits over time |
| Deprecated | two B-overlapping clusters; one adoption curve dead, the other growing |
| Dead code candidate | cluster with no C activity and no inbound references |

The high-B/low-A cell is the detector for the motivating pain: two crawlers
that both import `httpx`/`bs4` and speak fetch/parse/retry vocabulary
(B-similar) while sharing no structure (A-dissimilar) are mechanically
distinguishable both from benign duplication (high A) and from unrelated code
(low both). No single-vector embedding can express this query.

### 3.1 Two altitudes: tight clusters and pattern families

Real pattern families are internally **graded**. On corpus-P (a private pipeline corpus used for grading) the 24
per-source ingest jobs are one house pattern, yet pairwise Channel-A cosine
runs 0.36–0.76: at `θ_clone` they surface as *three* tight clusters, not one
family. So the repeated query runs at two altitudes (`src/family.rs`):

- **Tight clusters** (`θ_clone`, `cluster.rs`): near-duplicate shape — the
  clone/reinvention altitude, unchanged.
- **Pattern families** (`θ_family` < θ_clone, `family.rs`): tight clusters and
  their drifted variants (singletons that missed the tight bar) reassembled by
  **UPGMA average-linkage** cut at `θ_family`, so a family is one core shape
  plus a **drift gradient** of variants ordered by A-distance from the core.
  Average- not medoid-linkage: on corpus-P it separates the graded family
  (mean 0.38–0.49) from its nearest non-family neighbour (~0.30) with a clean
  gap medoid-linkage collapses; and its size-weighted mean is intrinsically
  blob-resistant — a single close pair cannot bridge two distant clusters
  (the phase-0 chaining guard, one altitude up). Guards: cut above the
  unrelated-code floor (θ_family ≈ 0.35 > θ_a_low 0.30), and a **core anchor**
  (a family must contain ≥1 real tight cluster and span ≥2 sub-clusters — a
  cloud of singletons is not a family). This is the "Drifting" row's
  shape-side reading; the C-axis (variance growing over time) refines it once
  symbol-precise dating lands.

## 4. Verdicts: the only durable state

**Superseded 2026-07-06 (pivot 2, TKI-45): the verdict store, `akron
ratify`/`akron check`, and everything below in this section were removed.
Akron keeps no durable state at all now — every surface is recomputed from
`(working tree, git history)` on every run. Left in place for history.**

```yaml
# .akron/verdicts/canonical-parse_records-53f77ce2.yml   (as implemented, TKI-10)
verdict: canonical            # canonical | deprecated | ignore
reason: |
  Pooled-connection crawler is canonical; the per-request variant leaks
  sockets under load (incident 2026-03).
author: reviewer
date: 2026-07-03
anchor:                       # content-address of the judged variant's core
  merkle:                     # every core member's Merkle root (dedup, sorted)
    - "53f77ce212aba38f"
    - "f69727e7c4c4e56a"
  wl_sig: ["0104ea6a…", …]    # the core medoid's 64-slot MinHash signature
over:                         # optional: the losing variant, same form
  merkle: ["1d2e…"]
thresholds:                   # the cuts this verdict was ratified under —
  theta_clone: 0.60           # part of the content-address (warn on drift)
  theta_family: 0.35
  theta_b_family: 0.16
```

Semantics:

- **Re-binding.** Each run, a verdict attaches to whichever current clusters
  match its anchors (exact via Merkle intersection, fuzzy via WL signature).
  Anchored to *content*, not paths — it survives file moves and refactors the
  same way git survives renames.
- **Expiry.** If no current cluster matches, the verdict is reported as
  expired. It cannot silently lie.
- **Self-governance.** Channel C watches the canon with the same instrument
  that watches the code: if a `canonical` verdict's cluster stops growing
  while a role-equivalent (high-B) cluster rises, Akron reports *"your canon
  is dying in practice"*. Semantic rot becomes a flagged measurement.
- **Derived facets.** What the old design stored, the new design computes at
  read time: the *exemplar* is the cluster's current medoid (or newest
  well-maintained member — clone-genealogy logic, research/05 §4); a *rule*,
  when wanted, is compiled on demand from the ratified cluster
  (anti-unification → ast-grep, research/05 §5). Neither can dangle, because
  neither is frozen.

Everything a verdict needs from a human is one decision and one sentence.
That is the entire maintenance burden of the system.

**As implemented (TKI-10, `src/verdict.rs`).** Where §4 above left choices
open, the verdict store resolves them thus:

- **Anchor = the family *core*, not the whole family.** The anchor holds each
  core member's Merkle root plus the core medoid's MinHash signature; the drift
  ring is derived scope, recomputed each scan, deliberately excluded from
  identity. An `R`-ref (a tight repeated cluster) anchors to that cluster
  directly — a tight cluster *is* a core. `akron ratify <path> <F#|R#>` extracts
  the anchor; competing/deprecated findings are not ratifiable (a verdict names
  *one* shape, and those queries pair two).
- **Re-binding is two-tier and deterministic.** *Exact* = any current core (or
  tight cluster) whose Merkle set intersects the anchor's — because the anchor
  keeps every core member's root, an alpha-rename or a single-file edit still
  hits. *Fuzzy* = the best current core medoid whose MinHash similarity to the
  anchor's signature clears `FUZZY_MINHASH_MIN` (0.40; calibrated on the
  fixtures — near-miss 0.56, drift 0.42, unrelated 0.03). Verdicts bind in
  file-name sort order; candidates in report order; ties on explicit index.
- **Expiry never deletes.** An unmatched verdict is listed in a dedicated
  report section (text/digest/JSON/HTML) and counted in a `loaded → bound
  (exact/fuzzy) → expired` funnel — never garbage-collected (superseding the
  "garbage-collected" phrasing above): a human wrote it, so a human retires it.
- **Thresholds are part of the content-address.** The three cuts are recorded
  in the file; a re-bind under different current cuts still binds but is flagged
  (`⚠ thresholds differ from ratification`) at every surface.
- **Conflict = self-governance made concrete.** A `deprecated` verdict bound to
  a *growing* cluster, or a `canonical` verdict bound to a *dead* one, is a
  conflict: it outranks every other finding in the digest and is drawn loud
  (bold red / a callout box). The role-equivalent cross-reference (a canonical
  cluster dying *while its high-B twin grows*) is available today through the
  paired deprecated-candidate finding; folding it directly into the verdict
  conflict is left for when the deprecated query runs at the family altitude.
- **Schema on disk** is one hand-rendered YAML file per verdict (block-scalar
  `reason`, inline-flow signature) so it stays git-reviewable and human-editable;
  it is read back through `serde_yaml` and parsed into a trusted `Verdict` at
  the boundary — a malformed or empty-reason file is a loud error naming the
  file, never a silent skip.

## 5. Honest limits

1. **No proofs.** Type-4 semantic equivalence is undecidable; deterministic
   channels yield candidates, not certainties. Two implementations of one
   role with disjoint structure *and* disjoint stacks overlap only weakly
   even in Channel B. Accepted: same-repo competing patterns near-always
   share domain vocabulary and partial API surface — the 80% case is the
   catchable case, and clusters + candidates are the product, not proofs.
2. **Clusters are anonymous.** The embedding says "these 14 things share a
   shape; 12 stopped being written in 2023, 2 are new" — never "this is a web
   crawler" or which variant is *right*. Naming and choosing are human (or
   LLM-decorated, human-confirmed) acts: that is what verdicts are.
3. **Determinism ≠ no tuning.** LSH thresholds and cluster cutoffs are knobs.
   Output is reproducible; agreement with human judgment of "same pattern"
   needs calibration against real repos — hence the falsification spike in
   PLAN.md.

## 6. Non-goals

- No stored exemplars, golden files, or hand-maintained rule libraries as the
  primary mechanism (rules can be *derived*; see §4).
- Not a general code-Q&A/context engine; not an ADR manager; not security
  scanning; not an AI reviewer.
- No UI, no hosted service, no pre-edit (~100 ms) veto until the engine has
  survived the spike.

## 7. Relationship to the research briefs

research/01–05 remain the evidence base: clustering machinery and precision
expectations (05), representation trade-offs and the embeddings-capture-topic-
not-structure finding (02), erosion evidence (03). What changed is
architectural: the pattern object's four stored facets collapse into
*derived* artifacts plus a verdict; the WL-histogram channel and the
high-B/low-A competing-pattern query are new; ratification shrinks from
authoring workflow to one decision + one sentence.
