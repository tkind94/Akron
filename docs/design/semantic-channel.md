# Design: The Semantic Channel (opt-in recall expander)

**TKI-30 · 2026-07-04 · Status: Proposed (design only, no implementation)**

The CEO's ask, verbatim: *"I would also like you to consider how an embedding
model would work. I don't expect us to bundle the whole model into our bin, but
I would expect us to be able to fairly easily pull and run one."*

## 0. The tension, stated once

This proposal collides head-on with **DESIGN.md §1 Principle 2**: *"No LLM, no
learned embedding model, no sampling anywhere in detection."* An embedding model
is exactly the thing that principle names and forbids. So this note does not
smuggle an embedding into the core; it proposes a **peripheral, opt-in recall
expander** that obeys the spirit of Principle 2's own escape clause — *"LLMs may
decorate, never decide"* — extended to embeddings:

> A learned model may **propose additional candidate pairs**. It may never
> decide what is shown, re-rank a deterministic finding, gate a commit, or
> anchor a verdict.

Everything below is built to hold that line. Whether the line is acceptable at
all is **Open Question 1** for the CEO — it needs an explicit amendment to
Principle 2, not a quiet reinterpretation. The rest of the market wedge survives
intact: default builds stay model-free and byte-identical (§3); even switched
on, the tool stays **token-free and $0-per-run** (a local encoder is not an LLM
API call), preserving the POSITIONING.md claim; determinism weakens only to
*pinned, platform-scoped determinism*, and only for findings that are visibly
labeled and never enter the durable canon.

---

## 1. What it buys — concrete rescue targets

Channel B (vocabulary, TF-IDF over external names + identifier subwords) is
blind to **naming divergence**: two symbols with the same behavior whose surface
vocabulary diverges enough that their cosine falls below `θ_b = 0.55` never
become a candidate pair at all. They are invisible *before* any filter runs, so
no filter or threshold work can recover them (proven in TKI-14). A code
embedding scores meaning, not tokens, and is the only signal that reaches below
the θ_b cliff.

### 1.1 The one proven, named target

**corpus-P `conn` ↔ `connect` (↔ `connection`)** — the anchor case (corpus-P is a private pipeline corpus used for grading).

| Symbol | file:line (per `src/callrel.rs` tests) | Role |
|---|---|---|
| `conn` | `conftest.py` | opens a DB connection (calls `psycopg.connect`) |
| `connect` | `packages/db/src/appdb/engine.py` | opens a DB connection |
| `PostgresResource.connection` | `orchestration/resources.py` | opens a DB connection (delegates to `engine.connect`) |

`conn` ↔ `connect` sits at **B = 0.522 < θ_b = 0.55**. The names are
subword-disjoint (`conn` is one token; `connect`/`connection` differ) and the
external stacks diverge (`psycopg` vs `appdb.engine`), so TF-IDF cosine cannot
clear the bar. TKI-14 proved the filter stack handles the *other* half of this
scenario correctly — it keeps `conn` competing rather than falsely suppressing
it as a `connect` caller — but it cannot manufacture a candidate pair that the
B-threshold discarded upstream. **What semantic must score:** cosine over code
embeddings of `conn` and `connect` above whatever `θ_sem` we calibrate (§5);
all three are textbook DB-connection setup and should embed close.

### 1.2 The inferred rescue population (the shoulder below the cliff)

The validation docs grade *surfaced* findings, so genuine sub-θ_b misses are, by
construction, mostly absent from them — the honest statement is that **exactly
one** naming-divergence miss is named at file:line today. But the docs do expose
the *shoulder* of the distribution: graded TPs that barely cleared θ_b. Each has
a slightly-more-divergent cousin sitting just below the cliff, and `queries.rs`
records how thin their overlap already is (shared-mass fraction in comments):

| TP that barely cleared | B | shared-mass frac | Signal |
|---|---|---|---|
| httpx C2/C7 ParseResult/URL host-render | 0.69–0.78 | **~0.40** (tightest surviving TP) | the θ_b floor is already grazing real TPs |
| flask C8 `after_this_request`/`after_app_request`/`after_request` | 0.68 | ~0.47 | three synonym verbs for one hook — pure naming divergence, barely caught |
| httpx C8 reason-phrase (`format_response_headers` vs `Response.reason_phrase`) | 0.68 | ~0.52 | thin overlap, one rare shared term from clearing |
| httpx C10 `encode_content` vs `MultipartStream.get_headers` | 0.66 | — | body-encoding twins, low vocab overlap |

These prove the mechanism (same-behavior pairs already crowd the θ_b boundary from
above); the symmetric population *below* the boundary is what semantic extends
into. We do not get to name them from file:line until we build the labeled
sub-θ_b set in §5 — and that honesty is the point: **no silent recall claim.**
The rescue target list is "1 proven + a measurable population," not a number we
made up.

### 1.3 Honest the other way — FPs a semantic signal would *worsen*

Code embeddings capture **topic, not job-polarity** (DESIGN.md §7, research/02).
That is precisely the failure mode of our worst existing competing FPs, and
semantic would *amplify* them:

- **Opposite-job, same-topic (COINCIDENTAL_VOCAB).** corpus-R C1: an HTTP-read
  endpoint vs a DB-write reflector — same domain nouns, opposite
  operations. An embedding rates them *more* similar, not less.
- **Producer/consumer & cooperating halves.** httpx C9 (`elapsed` set vs read),
  scrapy C3 (protocol calls `stream.close`). Topically identical, functionally
  complementary — semantic scores them high.
- **Conforming-implementation blobs (PROTOCOL_IMPL).** scrapy C8 — 19–26
  `from_crawler` impls of one framework hook. These are *maximally* similar
  semantically; semantic would re-inflate the blob the rep-check just shrank,
  unless the role-conformance guard runs ahead of it.

Balancing the ledger: semantic would *dodge* the TF-IDF-spike and dunder FPs
(rare-term `referrer`/`auth,tuple` spikes, `__eq__`/`__repr__` families) because
it does not ride individual rare tokens. Net: semantic trades one FP class for
another. This is why it is an **additive, separately-labeled, low-ranked**
source (§4) and why its precision must be measured on *its own additions*, never
folded into the headline (§5).

---

## 2. How it runs — 2026 survey and recommendation

### 2.1 Candidate models (CPU-runnable, local)

| Model | Params | License | Code-specialized | ONNX / weights | Notes |
|---|---|---|---|---|---|
| **jina-embeddings-v2-base-code** | 161M | **Apache-2.0** | **yes**, 30 langs incl. Python+Rust | safetensors (F16 ~161MB, F32 ~322MB); int8 ONNX quants ~80MB; **in fastembed-rs registry**; **candle JinaBert support** | 768-dim, 8k ctx via ALiBi. The pragmatic default. |
| jina-code-embeddings-0.5B / 1.5B (2025) | 0.5B / 1.5B | check per-repo | yes (Qwen2.5-Coder backbone) | safetensors; **no ONNX**; decoder, last-token pooling | SOTA (MTEB-Code 78.9% @1.5B, beats much larger models) but 3–9× the weight and a heavier decoder forward on CPU. Overkill for a candidate expander. |
| CodeRankEmbed | 137M | code-specialized | yes | compact bi-encoder | Smallest code-specialized baseline; viable fallback. |
| CodeSage (small/base) | ~130M+ | Salesforce | yes | ONNX-friendly | Reasonable alternative; less turnkey in Rust than jina-v2. |
| nomic-embed-text-v2 | ~137M | Apache-2.0 | general text | ONNX, fast CPU (~580 chunks/s) | Fastest on CPU but **not** code-specialized — wrong tool for code-to-code. |

### 2.2 Rust integration paths

| Path | What it is | For us |
|---|---|---|
| **candle** (HuggingFace, pure Rust) | Loads safetensors + `tokenizers` crate; ships a **JinaBert** implementation. | **No C++ native dependency** → a single statically-linked, cross-compiled binary. Directly compatible with TKI-13's "prebuilt binaries / `cargo install` / Homebrew everywhere" story. Cost: we wire the model fetch, forward pass, mean-pool + L2 ourselves (bounded, well-trodden). |
| **ort** (ONNX Runtime bindings) | Mature, ~3–5× faster than Python; `compile-static`, `minimal-build`, `load-dynamic`; `ORT_STRATEGY = download\|compile\|system`. | Fast and battle-tested, **but ONNX Runtime is a C++ native library**. Cross-compiling it to aarch64/Windows is the exact tax TKI-13 exists to avoid (known-fiddly per upstream issues); the default `download` strategy needs a prebuilt runtime per target. `load-dynamic` keeps it out of the binary but pushes a shared-lib dependency onto the user. |
| **fastembed-rs** | High-level wrapper over `ort`. Ships **`jina-embeddings-v2-base-code` in its registry**, downloads from HF hub, caches (`FASTEMBED_CACHE_DIR`/`HF_HOME`), bundles `tokenizers`, supports user-supplied models (`try_new_from_user_defined`/`try_new_from_path`). | The fastest possible spike — an afternoon to first embedding — but inherits ort's C++ dependency and forks our clean-binary story. |

### 2.3 Pull-on-first-use

Download source: **HF hub by default, with our own release-asset mirror as the
pinned fallback** (Open Question 3 — mirror vs trust-HF). Mechanism:

1. Model + tokenizer resolved by a **sha256-pinned** manifest baked into the
   binary (model id, revision, per-file sha256).
2. Fetched once to a user cache dir (`dirs::cache_dir()/akron/models/<sha>/`) —
   **never** into `.akron/` (that path is verdict canon; models are not canon).
3. `--semantic` with the model absent **and** offline → a **helpful, loud
   error** naming the model, the expected cache path, and the download command;
   never a silent skip, never a silent fallback to the deterministic path
   pretending nothing was asked for.

### 2.4 Recommendation

**Model: `jina-embeddings-v2-base-code`.** Apache-2.0, genuinely
code-specialized across Python and Rust, 161M is CPU-cheap (tens of ms/symbol,
one-threaded), 768-dim, and it is the one model both candidate Rust stacks
already know.

**Integration: candle, behind a `semantic` Cargo feature (default off).** It is
the only path that keeps Akron a single static cross-compiled binary, which is
load-bearing for TKI-13 and for the "no server, no runtime to install"
positioning. One-line reasoning: *candle is pure Rust, so the semantic build
still cross-compiles to every TKI-13 target without dragging the ONNX Runtime
C++ toolchain behind it.*

**But spike with fastembed-rs first.** It has the exact model in-registry, so
the §5 kill-criterion experiment ("does semantic actually rescue conn/connect
and a handful of sub-θ_b TPs without drowning them in topic-collision FPs?") can
be answered in an afternoon *before* we spend engineering on the candle port.
Falsification culture: run the cheapest experiment that could kill the idea
first; only build the shippable path if it survives.

---

## 3. Architecture preservation

**Embeddings are derived state, never canon.** A symbol's embedding is a pure
function of `(model_sha, normalized-symbol-hash, inference-config)`. It is
recomputed per run, or content-hash-cached under the user cache dir keyed by
`(model_sha, normalized-symbol-hash)` — the normalized hash already exists (the
Merkle/WL identity). It is never written to `.akron/`. Principle 1 (the repo is
the only source of truth) holds: the cache is a pure accelerator; deleting it
changes nothing but wall time.

**Determinism becomes pinned, platform-scoped determinism.** With the model sha
pinned, code fixed, and inference config fixed (single-threaded, fp32 or a fixed
fp16, no dynamic quantization), output is reproducible **on a given target**.
Cross-platform bit-identity is **not** guaranteed — float reduction order,
SIMD width, and int8 VNNI-vs-scalar paths differ across CPUs (confirmed: neither
ORT/MLAS nor candle promise cross-machine bit-identity). This single fact drives
the three hard rules below.

**Model id stamped where — and where NOT.**

- **JSON:** yes — stamp `model_id`, `model_sha`, and `inference_config`, so a
  semantic-sourced finding is explainable and a diff across machines/model-bumps
  is attributable rather than mysterious.
- **Report/HTML:** yes — as a **visible label** on every semantic-sourced
  finding (e.g. a `semantic` badge), so a reader always knows which findings
  came from the deterministic core and which from the opt-in model.
- **Verdict anchors: NO.** Verdicts must **never** anchor on a semantic
  similarity. Justification, directly from DESIGN.md §0: a verdict authored on
  machine A under model X must re-bind on machine B, under model X+1, and in the
  **default (semantic-off) build** where the model is absent entirely. Anchoring
  identity on a platform-nondeterministic float from a pinned model would make
  the verdict fail to bind across exactly those boundaries — reintroducing the
  staleness the derived-canon pivot was built to kill. Semantic only ever
  proposes the candidate *pair*; the instant a human ratifies it, the verdict
  anchors on Merkle + WL (deterministic, model-free) and the semantic channel is
  irrelevant to its entire future life. The pinned thresholds already recorded
  in a verdict (`theta_b`, etc.) do **not** gain a `theta_sem` field.

**Flag default OFF ⇒ byte-identical to today.** The entire semantic path lives
behind `#[cfg(feature = "semantic")]`. A default `cargo build` links none of it;
the determinism test (`tests/determinism.rs`) stays green and output is
byte-for-byte today's. **When on**, semantic-sourced findings are visibly
labeled at every surface (text/digest/JSON/HTML) and segregated below the
deterministic findings — never interleaved, never re-ranking them.

---

## 4. Where it plugs in

### 4.1 Accept: the competing-recall expander (a new `semantic` funnel stage)

Semantic enters `queries.rs::competing` as an **additional candidate source**,
not a replacement for the B-threshold. A pair `(a, b)` is a semantic candidate
when `sem_cosine(a, b) ≥ θ_sem` **and** its Channel-B cosine fell below θ_b (so
it is genuinely a naming-divergence miss, not a duplicate of a B-found pair). It
then must pass the **same downstream filters** every competing candidate passes,
because those encode job-distinctness, not vocabulary:

- cross-context: different file, neither is a test;
- `low_shape`: Channel-A cosine ≤ `θ_a_low` (still structurally distinct — a
  clone belongs to the *repeated* query, not here);
- `call_related` suppression (`callrel.rs`): still not a caller/callee pair;
- a **new semantic quality/representative guard** replacing `vocab_quality`
  (which a low-B pair cannot pass by definition) — e.g. a `θ_sem` margin and a
  best-first rep-check mirroring the B-side anti-chaining guard, so semantic
  candidates cannot chain into topic blobs.

Output contract: semantic candidates are added to the funnel with a `semantic`
stage counter (empty result stays explainable — no silent recall, DESIGN.md),
surface **labeled**, and rank **below** all B-sourced findings. They **add**;
they never remove or re-order a deterministic finding. The competing funnel
becomes `… low_shape → vocab_quality → call_related → chained` **+** a parallel
`semantic → (same filters) → chained_sem` branch feeding the same union.

### 4.2 Evaluate and reject

- **Family / repeated-cluster naming — reject for detection (accept only as
  decoration, out of scope here).** Naming a cluster is Principle-4 decoration
  that never gates; if we ever want it, an LLM label is cheaper than an
  embedding and it is explicitly *not* a detection use. Not in this design.
- **The `akron check` gate — reject hard.** `check.rs`'s founding rule is *"a
  wrong block is the fastest path to `--no-verify` culture, so precision beats
  recall — only an exact (Merkle-level) match to a deprecated verdict ever fails
  `--strict`."* A semantic similarity is fuzzy, platform-nondeterministic, and
  model-version-dependent — it can answer differently on the same commit across
  two machines. Putting it anywhere near the gate is the precise thing that
  earns a `--no-verify` and it directly blunts the determinism wedge
  (POSITIONING §4: *"a gate that answers differently on rerun gets
  `--no-verify`'d; ours cannot"*). **Semantic never touches `check`**, not even
  advisory-mode: the gate path stays model-free so its byte-reproducibility is
  unconditional.

---

## 5. Evaluation plan + kill criterion

**Corpora:** the graded set — corpus-R, httpx, flask, scrapy (both
validation docs) plus corpus-P (the conn/connect home).

**Step 1 — build the sub-θ_b ground truth.** Scan each corpus with `θ_b` lowered
(e.g. to 0.35) to *enumerate* the pairs that live below the cliff, then
hand-grade the new pairs into TP/FP by reading the source (same rubric as the
validation docs). This is the labeled population semantic is supposed to rescue,
and it is the artifact §1.2 promised — it converts "an inferred population" into
named file:line targets. Commit it as a third validation doc.

**Step 2 — measure semantic on that set, two numbers only:**

1. **Rescue rate:** of the graded sub-θ_b **TPs** (including conn/connect), what
   fraction does the semantic expander surface in its top-K additions per repo?
2. **Added-FP rate:** of the semantic **additions** at top-K, what fraction are
   FP (graded independently — this is precision of the *added* candidates, never
   pooled into the headline competing precision)?

**Pre-committed kill criterion.** Semantic ships only if **both** hold on the
pooled graded set:

- it **recovers the conn/connect target** (the one proven case — if it can't
  rescue the case that motivated it, the feature has no reason to exist); **and**
- its **added-candidate precision at top-10-per-repo ≥ 0.50** — strictly better
  than the existing competing query's ~0.44, since a recall source that is
  *noisier* than the query it feeds makes the tool's weakest axis worse.

If added precision **< 0.40**, or it misses conn/connect, **kill it.** No amount
of "but it found other things" overrides: a below-baseline expander on our
weakest query is negative value. What would kill the whole idea outright: the
topic-not-job-polarity FPs of §1.3 dominate the additions (opposite-job pairs
outrank naming-divergence TPs), confirming embeddings can't tell same-behavior from
"same topic" on our corpora.

---

## 6. Cost / effort estimate

- **Implementation:** ~600–900 LOC, all behind `#[cfg(feature = "semantic")]`:
  `embed.rs` (candle JinaBert load + forward + mean-pool + L2), a sha256-pinned
  model fetcher + cache, the `semantic` funnel branch + guard in `queries.rs`,
  the flag + JSON/report/HTML labels, and the `(model_sha, symbol-hash)→vector`
  cache.
- **New dependencies (feature-gated only):** `candle-core`, `candle-nn`,
  `candle-transformers` (JinaBert), `tokenizers`, `safetensors`, and a minimal
  `hf-hub` or a `reqwest` + `sha2` downloader. **All pure Rust, no C++.**
- **Binary size when OFF: ~0.** A feature flag, not `dlopen`, is the right tool
  here — investigated: `dlopen`/`load-dynamic` is the mechanism you reach for
  when the runtime is a *C shared library* (i.e. if we'd chosen ort/ONNX
  Runtime, you'd keep the C++ lib optional via `load-dynamic`). candle is a Rust
  crate with no shared lib, so a Cargo feature simply excludes it from the
  default build entirely — nothing linked, output byte-identical, determinism
  test unaffected. Distribution: either `cargo install akron --features
  semantic` or a separate published `akron-semantic` artifact (Open Question 2).
- **Binary size when ON:** candle + transformers + tokenizers add a few MB of
  Rust code. **Model weights are never in the binary** — pulled to the user
  cache on first `--semantic` use — exactly the CEO's "don't bundle the whole
  model, but pull and run one."
- **Maintenance surface:** the sha256 model pin + mirror availability (a HF
  repo can vanish → our release mirror is the hedge); candle version churn; a
  second (opt-in) build target in CI for TKI-13; and the sub-θ_b ground-truth
  set as a regression guard so a model bump can't silently rot rescue quality.

---

## Open questions needing a CEO decision

1. **Principle-2 amendment.** DESIGN.md §1.2 forbids "no learned embedding model
   anywhere in detection." A recall expander proposes detection candidates. Do
   we ratify the reframing — *model may expand candidates additively, off by
   default, never decide/gate/anchor* — as an explicit amendment, or hold the
   hard line and shelve this? Everything else depends on this answer.
2. **Distribution shape:** `cargo install --features semantic` only, or a
   separate prebuilt `akron-semantic` binary alongside the default one (TKI-13)?
3. **Model source of truth:** trust HF hub for the pull, or stand up our own
   sha-pinned release mirror (supply-chain + offline control vs hosting cost)?
4. **Is the weakest-axis risk worth it?** Semantic adds recall to the *competing*
   query, already our lowest-precision surface (~44%). We're proposing to feed
   the weak query more candidates. The §5 kill criterion is the guardrail, but
   the strategic call — spend engineering to chase naming-divergence recall at
   all, vs. bank the deterministic wedge — is yours.
