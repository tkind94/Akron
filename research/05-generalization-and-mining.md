# Generalization & Mining: turning N similar files into one enforceable canon

**Research brief for Akron** — the pipeline *cluster similar code → generalize cluster into template-with-holes → human ratifies → template compiled to checks.*

---

## Executive summary

**Is `mine → generalize → ratify → compile` feasible with known techniques? Yes — every stage has a shipped industrial precedent, and the weak link is not the algorithms but the *precision expectations* you set for the auto-generated candidate.**

- **Generalization (cluster → template-with-holes)** is a solved formal problem: **anti-unification / least-general-generalization (LGG)**, originally Plotkin (1970). It literally computes "the most specific template that all N examples are instances of," introducing a *hole* (metavariable) exactly where the examples disagree. Facebook's **Getafix**, Microsoft's **REFAZER**, and **Revisar** all ship AU-over-AST in production or near-production. The owner's intuition that the representation is "sort of like the intermediate AST representation" is exactly right — AU operates on ASTs and yields an AST-with-holes.

- **The core risk is the generality knob, not the mechanism.** A single AU of N drifted files over-generalizes (holes swallow the meaningful structure → the checker matches everything) *or* under-generalizes (template = one exemplar → matches only the training cluster). The industrial answer is **hierarchical clustering + anti-unification** (Getafix): build a *dendrogram* of patterns from general (root) to specific (leaves), then let ranking/human choose the altitude. This is the single most important technique for Akron and directly supports the "drift gradient matters" intuition.

- **Clustering (finding the 20 parser files)** is mature at repo scale: token-based **SourcererCC** (250 MLOC on a workstation), tree-based **DECKARD** (AST vectors + LSH), text/AST-hybrid **NiCad**, and **MinHash+LSH** for O(n) near-duplicate grouping. A 1–5k-file Python repo is *small* by these standards — expect seconds-to-minutes.

- **Compilation (template → checker)** has direct precedent: **ast-grep** and **Semgrep** are pattern-as-code matchers whose surface syntax is essentially "code with holes" — an AU template maps to them almost mechanically, and both now support LLM-assisted rule drafting from examples. Amazon's **RhoSynth** (OOPSLA 2022) synthesizes static-analysis rules from example diffs and ships them in CodeGuru at **>75% precision**. The self-test guarantee (rule must match all ratified members + exemplar) is cheap and should be a hard gate.

- **Grounded precision expectations:** Getafix's *top-1* suggestion exactly matched the human fix in **~25%** of cases (top-k much higher); HAGGIS idiom mining hit **~67% precision at 31% coverage** on StackOverflow (96.6% precision at 21% coverage on Android). Read these as: **a mediocre auto-candidate is the realistic and acceptable v1 output** — which matches the owner's stated intuition. The human-ratify step is not optional polish; it is load-bearing.

**Recommended v1 stack (Python-first):** tree-sitter/native-AST → structural feature-bag + **MinHash/LSH** blocking → **pq-gram / tree-edit refinement** into clean clusters → **hierarchical anti-unification** over each cluster to produce a *pattern dendrogram* → rank candidate altitude by coverage×specificity, seed the hole-fill from the **newest** cluster member (drift gradient) → emit the ratified template as an **ast-grep** rule → **self-test** the rule against all cluster members before it can be merged.

---

## Task 1 — Anti-unification / least-general-generalization (the formal core)

**What it is.** Anti-unification (AU) constructs the *least general generalization* (LGG) of two or more terms: the most specific term `t` such that every input is a substitution instance of `t`. Where inputs agree, structure is preserved; where they differ, a fresh **hole** (metavariable) is introduced. Dual to unification (which finds the most-general common *instance*), AU finds the most-specific common *generalization*. First-order syntactic AU has a unique LGG computable by Plotkin's (1970) algorithm.
- Overview & Plotkin: <https://en.wikipedia.org/wiki/Anti-unification>

**Worked example (the whole idea in one line).** AU of `if (dog == null) return;` and `if (cat == null) return;` = `if (h0 == null) return;` — the differing leaf becomes hole `h0`. That template-with-hole is exactly Akron's "candidate canonical pattern with variation points."

**Higher-order & equational AU.** For higher-order terms there is in general no unique LGG, so restricted classes (higher-order *patterns*) are used to keep it unique and tractable. Relevant if Akron ever generalizes over *function-shaped* holes (e.g. "some callback here") rather than value holes.
- Higher-Order Pattern Anti-Unification in Linear Time: <https://www.ncbi.nlm.nih.gov/pmc/articles/PMC6109779/>
- Higher-Order Equational Pattern Anti-Unification (preprint): <https://arxiv.org/pdf/1801.07438>

**Systems that USED AU over trees — study these:**

- **Getafix (Facebook, OOPSLA 2019)** — the closest existing thing to Akron's generalizer. Mines fix patterns from thousands of past AST edit pairs using **hierarchical clustering + anti-unification**, producing a *dendrogram* of edit patterns ranging from very general (root) to very specific (leaves). Holes (`h0`, `h1`, …) are introduced by AU at differing positions; crucially it **also retains a portion of the *unchanged* surrounding code** as context, so a pattern carries its own applicability constraint. This "keep the surrounding context" idea is the **"focus"** mechanism (see Task 6) and is the antidote to over-generalization.
  - Paper: <https://arxiv.org/abs/1902.06111> · DOI: <https://dl.acm.org/doi/10.1145/3360585>
  - Engineering blog (readable, has numbers): <https://engineering.fb.com/2018/11/06/developer-tools/getafix-how-facebook-tools-learn-to-fix-bugs-automatically/>
  - Mirror PDF: <https://software-lab.org/publications/Getafix_arXiv_1902.06111.pdf>

- **REFAZER (Microsoft, ICSE 2017; built on PROSE/FlashMeta)** — learns *syntactic program transformations* from example edits via programming-by-example over a DSL, generalizing example edits "at the right level of abstraction" (variables/subexpressions become holes). Fixed **87%** of students across 4 tasks (720 students); also learns repetitive-edit refactorings in large C# projects. Directly relevant to the "apply the ratified edit elsewhere" direction.
  - Paper: <https://arxiv.org/abs/1608.09000> · MS Research: <https://www.microsoft.com/en-us/research/publication/learning-syntactic-program-transformations-examples/> · Code: <https://github.com/gustavoasoares/refazer>

- **Revisar (SBES 2021 / arXiv 2018) — "Learning Quick Fixes from Code Repositories"** — uses a **variant of anti-unification** to find the LGG of concrete AST edits (extracted via **GumTree** tree-diff), then a greedy algorithm clusters edits describable by one pattern. On 9 GitHub projects it found **89 edit patterns appearing in ≥3 projects**, 64% not covered by existing tools. This is essentially the "mine repeated structure → emit a template" loop Akron wants.
  - Paper: <https://ar5iv.labs.arxiv.org/html/1803.03806> · DOI: <https://dl.acm.org/doi/10.1145/3474624.3474650>

- **babble (POPL 2023) — "Learning Better Abstractions with E-Graphs and Anti-Unification"** — modern, fast AU-based *library learning*: compresses a corpus by extracting shared structure into reusable abstractions. Key advance: runs AU over **e-classes** (equality-saturated e-graphs) instead of raw AST nodes, making it **robust to syntactic variation** and orders of magnitude faster. This is the state of the art for "syntactic drift shouldn't defeat the generalizer," which is *exactly* Akron's brownfield-drift problem.
  - Paper: <https://arxiv.org/abs/2212.04596>
  - Survey/review context (AU + library learning): <https://inst.eecs.berkeley.edu/~cs294-260/sp24/projects/erawn/>

**Open-source AU over tree-sitter:** there is no single dominant off-the-shelf "AU over tree-sitter" library; the practical route is (a) reuse **GumTree** for AST diffing/mapping (as Revisar does) and implement Plotkin-style first-order AU over the mapped node correspondence, or (b) adopt an e-graph library (e.g. `egg`) and port babble's e-graph-AU idea. For Python specifically, native `ast` or tree-sitter-python gives the tree; AU is a ~few-hundred-line recursive tree walk once node correspondence is fixed.

---

## Task 2 — Code idiom / pattern mining

**HAGGIS — "Mining Idioms from Source Code" (Allamanis & Sutton, FSE 2014).** The canonical academic idiom miner. Represents code as ASTs and learns a **nonparametric Bayesian probabilistic tree-substitution grammar (pTSG)**; frequently-recurring tree fragments (with metavariables, e.g. a for-loop body) are the mined idioms. Found semantically meaningful idioms (resource management, exception handling, object creation) that recur across projects.
- Paper: <https://arxiv.org/abs/1404.0417> · Author page: <https://miltos.allamanis.com/publications/2014mining/> · ML4Code: <https://ml4code.github.io/publications/allamanis2014mining/>
- **Precision/coverage numbers (grounding for Akron's risk section):** on StackOverflow, HAGGIS idioms achieved **~67% precision at 31% coverage**; restricted to Android libraries, **96.6% precision at 21% coverage**. Takeaway: high-precision mining is possible but *coverage is low* — you mine the confident core and leave a long tail. Perfect fit for "emit a candidate, human ratifies."

**Mining Idioms in the Wild — "Jezero" (Sivaraman et al., ICSE-SEIP 2022).** Argues purely-syntactic miners (like HAGGIS) miss *semantic* idioms and pure-semantic ones need test suites; introduces **canonicalized dataflow trees** to mine semantic idioms from Facebook's Hack repo, surfacing suggestions during **code review** (human-in-the-loop, not auto-apply) — the same delivery model Akron plans.
- Paper: <https://arxiv.org/abs/2107.06402> · DOI: <https://dl.acm.org/doi/abs/10.1145/3510457.3513046>

**API-usage pattern mining (representation reference).**
- **MAPO** (ECOOP 2009): clusters snippets, then **frequent-subsequence mining** of API call sequences → sequential usage patterns. <https://taoxie.cs.illinois.edu/publications/ecoop09-mapo.pdf>
- **GrouMiner** (FSE 2009): **frequent-subgraph mining** over *groums* (graph-based object-usage patterns) → code skeletons; suffers subgraph-isomorphism cost. <https://www.researchgate.net/publication/221560251_Graph-based_mining_of_multiple_object_usage_patterns>
- **BigGroum** (SANER 2018): scales GrouMiner via frequent-itemset mining + semantic analysis. <https://plv.colorado.edu/bec/papers/biggroum-saner18.pdf>
- **API usage templates via structural generalization** (JSS 2024): recent work using **structural generalization (an AU flavour)** to produce API usage *templates with holes* from examples — closest recent analogue to Akron's template output. (Publisher blocks scraping; cite by DOI/landing.) <https://www.sciencedirect.com/science/article/pii/S0164121224000177>

**LLM-era pattern mining (2024–2026).** Emerging but *noisier and less reproducible* than symbolic AU: e.g. "An Exploration of Pattern Mining with ChatGPT" (EuroPLoP 2024) uses an LLM to surface patterns from known uses. Practical read for Akron: **use an LLM to *name/summarize/label* a symbolically-mined template and to draft its rationale, not to be the generalizer of record** — the deterministic AU template is what gets compiled and enforced; the LLM makes it human-legible.
- <https://arxiv.org/pdf/2412.16814>

**Representation summary:** mined patterns take one of three shapes — (1) **AST fragments with metavariables** (HAGGIS, AU systems) — best fit for Akron; (2) **API call sequences** (MAPO); (3) **object-usage graphs** (GrouMiner). Akron should standardize on (1): AST-with-holes is directly compilable to ast-grep/Semgrep.

---

## Task 3 — Structural clustering at repo scale (finding the "20 parser files")

You need to *discover* the cluster before you can generalize it. Two families, best combined:

**Token/text-based clone detection (fast, scales hugely):**
- **SourcererCC** (ICSE 2016): token-based, inverted-index with token-ordering filter heuristics; detects Type-1/2/3 (near-miss) clones; scaled to **25K projects / 250 MLOC on one workstation** — the only then-current Type-3 detector to reach 100 MLOC. A 1–5k-file repo is trivial for it. <https://arxiv.org/abs/1512.06448>
- **NiCad** (ICPC 2011): hybrid text+AST — parses functions, **flexible pretty-printing** + normalization to line up near-miss clones, then line-based comparison; strong precision/recall at function/block granularity, and clusters clone classes. Good exemplar for *function-level* Python clustering. <https://ieeexplore.ieee.org/document/5970189/>

**Tree/AST-structure-based (captures structure token methods miss):**
- **DECKARD** (ICSE 2007): computes **characteristic vectors of AST subtrees**, then clusters with **LSH** — the classic "AST → vector → LSH" recipe. Tree-based and scalable; directly usable for structural clustering. <https://www.researchgate.net/publication/4251304_DECKARD_scalable_and_accurate_tree-based_detection_of_code_clones>
- **pq-grams** (Augsten et al., TODS 2010): an **O(n)-space approximation of (fanout-weighted) tree edit distance** — a lower bound on TED with a metric normalization (triangle inequality holds). Ideal *refinement* metric to clean up LSH candidate clusters without paying full O(n²·…) tree-edit cost. <https://dbresearch.uni-salzburg.at/publications/2010tods-pq-gram-distance-ordered-labeled-trees/paper.pdf>
- **Full tree edit distance** (Zhang-Shasha and successors) is accurate but expensive; use only on small candidate sets, not repo-wide.

**LSH / MinHash for the blocking step:** MinHash estimates Jaccard similarity of feature sets (AST n-grams / token shingles) with compact signatures; **LSH** turns that into **sub-linear O(n) near-duplicate grouping**, sidestepping O(n²) all-pairs. A recent ast-grep-mcp writeup reports MinHash+LSH replacing an O(n²) matcher to scale from ~1k to 100k+ functions. This is the right *first pass* for Akron: cheaply bucket candidate clusters, then refine each bucket with pq-gram/tree-edit.
- <https://www.aledlie.com/reports/2025-12-02-minhash-lsh-implementation-code-clone-detection/>

**Embedding-based vs structure-based (and hybrid).** Learned code embeddings (code2vec/ASTNN-style, or modern code-LLM embeddings) cluster on *semantic* similarity and tolerate surface drift, but are noisier and harder to explain/audit; pure structural metrics are precise and explainable but brittle to reordering. **Recommended hybrid:** embeddings (or MinHash) for cheap recall/blocking → structural (pq-gram/tree-edit) for precise, explainable cluster boundaries. Explainability matters because a human ratifies each cluster.

**Performance on 1–5k-file repos:** all of the above are comfortable. Expect: MinHash/LSH blocking in **seconds**; pq-gram refinement of buckets in **seconds–low minutes**; the AU generalization step (Task 1) dominates only if clusters are large, and even then is per-cluster and parallelizable. The stated "checker runs in 1–5s" budget is about *enforcement* (a compiled ast-grep rule), which is easily met; *mining* is an offline/PR-time batch job and can take longer.

---

## Task 4 — The drift gradient (which variant is the intended canon?)

The owner's intuition — *oldest→newest ordering within a cluster matters, and the newest file may be the intended canon* — has direct prior art in **clone genealogy** research.

**Clone genealogies (Kim et al., ESEC/FSE 2005).** Coined "clone genealogy": tracks how a *family* of clones evolves across versions. A **Clone Genealogy Extractor** takes (1) multiple chronological program versions, (2) a clone detector, (3) a location tracker, and reconstructs how each clone changed. Key empirical findings relevant to Akron: clones are either **short-lived** (disappear via natural evolution) or **long-lived and consistently co-changed** — i.e., a real, maintained pattern shows up as a genealogy with *consistent* edits, which is a strong signal that a cluster is genuinely a "canon" worth ratifying rather than accidental similarity.
- <https://web.cs.ucla.edu/~miryung/Publications/esecfse05-clonegenealogy.pdf>

**Tracking clones as code evolves (Duala-Ekoko & Robillard).** CloneTracker / clone-region descriptors track clones across edits and can notify on divergence — the mechanism for computing the *within-cluster drift gradient* from git history. <https://www.cs.mcgill.ca/~eduala/papers/duala-ekoko-TrackingCodeClones.pdf>

**Near-miss genealogies at scale: gCad (ICSM 2013).** Extracts and classifies exact *and* near-miss clone genealogies efficiently — practically what Akron needs to order drifted members chronologically. <https://ieeexplore.ieee.org/abstract/document/6676939/>

**How Akron should use history (concrete):**
1. For each cluster member, get `git log`/blame recency → order oldest→newest.
2. **Pick the canonical exemplar = the newest member** (owner's intuition) **unless** the genealogy shows the newest is an outlier (edited once, never co-changed) — then prefer the member with the most *consistent* recent co-change history (the "living standard").
3. **Seed the AU hole-fills / concrete-side of the template from the exemplar**, so the ratified template reads like the code the team actually writes today, while the hole *positions* come from AU across all members.
4. Surface the drift gradient to the human ("18 files match the 2019 shape, 2 files moved to a newer shape in 2024") — this framing is itself a strong ratification aid and matches how Jezero/Getafix present suggestions in review.

---

## Task 5 — Template → checker compilation

Given a ratified template-with-holes, compile it to a fast deterministic validator.

**Target formats (pattern-as-code matchers):**
- **ast-grep**: tree-sitter-based search/lint/rewrite; **pattern syntax is literally code with metavariables** (`$VAR`, `$$$ARGS`), plus composite/relational rules for context. An AU template maps almost 1:1: concrete nodes → literal syntax, holes → metavariables, retained context → `inside`/`has` relational rules. Fast (Rust), Python-supported, and the natural compile target. AI-rule-drafting flow decomposes a query into atomic sub-rules, tests against example snippets, and iterates — a model for Akron's own template→rule step. <https://ast-grep.github.io/blog/ast-grep-agent.html> · Rule reference: <https://ast-grep.github.io/reference/rule.html>
- **Semgrep**: pattern syntax also mirrors source with `...` ellipses and `$METAVAR`; `pattern-either` unions multiple shapes (useful when a cluster has 2–3 legit variants). Manual-authoring-first, but the same template→pattern mapping applies. <https://semgrep.dev/docs/writing-rules/pattern-syntax>

**Rule synthesis from examples (the automated compile precedent):**
- **RhoSynth / "Synthesizing code quality rules from examples"** (Amazon, OOPSLA 2022) — the strongest evidence this stage works. Formulates rule synthesis as inferring **first-order-logic formulas over graph representations of code**, using **ILP-based graph alignment** to find the elements of interest; bootstraps positive/negative examples from **developer code changes**; supports **incremental refinement** with more examples. Synthesized **30+ Java rules deployed in Amazon CodeGuru Reviewer at >75% precision** on live reviews. This is the "diffs → deployed checker" loop, proven in production. <https://dl.acm.org/doi/10.1145/3563350> · <https://www.amazon.science/publications/synthesizing-code-quality-rules-from-examples> · arXiv preprint "Example-based Synthesis of Static Analysis Rules": <https://arxiv.org/pdf/2204.08643>
- **REFAZER/PROSE (FlashMeta)** — general programming-by-example framework (deductive synthesis over a DSL) behind FlashFill/FlashExtract; the engine to reach for if Akron wants to *learn* the rule DSL program rather than template-map it. <https://arxiv.org/abs/1608.09000>

**The inverse guarantee (self-test) — make this a hard gate.** After emitting a rule, *verify it before it can merge*:
1. **Recall check:** the compiled rule must match **all** ratified cluster members **and** the chosen exemplar (else the template over-specialized). RhoSynth's example-based loop is exactly this discipline.
2. **Precision guard:** run the rule across the whole repo; if it matches far *more* than the cluster (e.g. matches unrelated files), it over-generalized — flag for the human to tighten holes / add context. This is cheap (ast-grep is fast) and directly counters the over-generalization failure mode.
3. Store cluster members as a **regression corpus** so future rule edits can't silently break recall.

---

## Task 6 — Failure modes & quality bars

**The central tension: over-generalization vs over-specialization.**
- *Over-generalization*: too many holes → template collapses to something trivial (`$A = $B`) that matches everything → useless checker, false-positive storm.
- *Over-specialization*: too few holes → template = one exemplar → matches only the training cluster, no generalization.
- **Mitigation (from Getafix):** don't pick one altitude — build a **general→specific hierarchy** and select. Getafix keeps **unchanged surrounding context** in patterns as an applicability constraint (the **"focus"** idea), which stops general patterns from over-firing.

**How the shipped systems ranked candidates (adopt this):**
- **Getafix ranks by a multi-score scheme combining specificity and prevalence** — more specific patterns (matching fewer locations, carrying more context) rank higher to avoid over-application, balanced against how frequently the pattern occurred in history. Ablations showed **both hierarchical learning and multi-score ranking were worthwhile for 5 of 6 bug categories**. Akron should rank candidate template altitudes by **coverage × specificity** (matches all/most members, but not the whole repo).
- **REFAZER** ranks DSL programs learned by PBE; **RhoSynth** refines rules against added examples and uses developer feedback as the precision signal.

**Grounded precision expectations (the numbers to set roadmap goals against):**
- **Getafix:** top-1 suggested patch **exactly matched the human fix in ~25%** of cases (top-k substantially higher); on Instagram it auto-patched **~53%** of null-method-call bugs, and **~90%** of attempted patches passed automatic validation. Read: *top-1 auto-quality is modest; the value is a good ranked shortlist a human confirms.*
- **HAGGIS:** **~67% precision @ 31% coverage** (StackOverflow); **96.6% @ 21%** (Android). Read: *high precision is reachable only at low coverage — mine the confident core, accept a long tail.*
- **RhoSynth:** **>75% precision** on deployed synthesized rules. Read: *the compile step, with self-test + human ratify, can clear a production bar.*
- **Revisar:** **89 cross-project patterns** mined, 64% novel vs existing tools — evidence the mining finds real, reusable structure.

**Human-in-the-loop flows in shipped systems (the delivery model to copy):** Getafix and Jezero both **surface ranked suggestions in code review** rather than auto-applying; RhoSynth deploys rules into a **review** system and refines from feedback; REFAZER proposes a transformation the developer applies. None trust the fully-automatic candidate — all treat it as a **draft a human ratifies**, which validates Akron's core UX ("mediocre auto-candidate is fine as a starting point → engineer edits → ratifies via PR").

**Akron-specific quality bars to enforce:**
1. **Self-test recall = 100%** of ratified cluster members (hard gate, Task 5).
2. **Precision guard:** repo-wide match count ≤ (cluster size + small ε) at ratification time, or explicit human sign-off on the wider match set.
3. **Minimum cluster size / support** (e.g. ≥3 members, à la Revisar's ≥3-project bar) before proposing a candidate — kills noise.
4. **Hole budget:** flag templates whose hole-to-node ratio exceeds a threshold (over-generalization smell).
5. **Explainable clusters** (structural, not just embedding) so the human can judge the proposal.

---

## Recommended pipeline for a Python-first v1

**Stage 0 — Parse.** tree-sitter-python (or native `ast`) → AST per file/function. Normalize identifiers/literals into a canonical form for the similarity step (NiCad-style), but keep the raw AST for generalization.

**Stage 1 — Cluster (find the 20 parser files).**
- *Blocking:* feature-bag of AST n-grams / structural shingles → **MinHash + LSH** → candidate buckets in O(n). Trivial for 1–5k files.
- *Refinement:* within each bucket, compute **pq-gram distance** (O(n)-space TED approximation) and cut clusters at a distance threshold; optionally full tree-edit on small buckets for boundary cases. Keep clusters **structural and explainable**.
- *Support filter:* drop clusters with < 3 members.

**Stage 2 — Order by drift gradient.** For each cluster, pull `git` recency/blame → order oldest→newest; compute a simple genealogy (consistent co-change vs one-off). Pick the **canonical exemplar = newest, well-maintained member**.

**Stage 3 — Generalize (cluster → template-with-holes).**
- Establish node correspondence with **GumTree**-style mapping (as Revisar does).
- Run **first-order anti-unification** hierarchically to build a **general→specific dendrogram** of templates (Getafix's method). Introduce holes at disagreement points; **retain surrounding unchanged context** as applicability constraints (Getafix "focus").
- For drift-robustness on badly-diverged clusters, consider **e-graph AU (babble-style)** so syntactic variants don't defeat generalization.
- Seed the concrete/hole-fill side of the presented candidate from the **exemplar** so it reads like current house style.

**Stage 4 — Rank & present the candidate.** Choose the dendrogram altitude by **coverage × specificity** (Getafix multi-score): the most specific template that still covers all/most members without matching the whole repo. Optionally use an **LLM to name and narrate** the template (not to generate it). Present with the drift-gradient story ("18 files old shape, 2 files newer shape").

**Stage 5 — Human ratifies via PR.** Engineer edits holes/context, accepts/renames variation points. This is load-bearing, not optional (all shipped systems rely on it).

**Stage 6 — Compile to checker.** Map ratified template → **ast-grep rule** (holes→metavariables, context→relational `inside`/`has` rules). Fast (Rust), Python-supported, 1–5s enforcement budget easily met. (Semgrep as an alternative/parallel target; RhoSynth-style FOL-over-graph synthesis if template-mapping proves too rigid.)

**Stage 7 — Self-test gate (mandatory).** Rule must match **all** ratified members + exemplar (recall=100%); repo-wide match count must stay near cluster size (precision guard) or get explicit sign-off; persist members as a **regression corpus**. Only then may the rule merge and start enforcing.

---

## Risks (precision expectations, grounded in the literature)

- **Auto-candidate quality will be "mediocre," and that's the documented normal.** Getafix top-1 ≈ 25% exact-match; HAGGIS ≈ 67% precision at only 31% coverage. Set roadmap KPIs around **ranked-shortlist usefulness and human-ratify throughput**, not top-1 auto-correctness. This aligns with the owner's own intuition — treat it as validated, not a warning.
- **The generality knob is the hard part, not AU itself.** Without the general→specific hierarchy + specificity-aware ranking + self-test precision guard, candidates will oscillate between "matches everything" and "matches only the cluster." Getafix's hierarchy+multi-score is the proven counter; ablation says it mattered for 5/6 categories.
- **Drift can defeat naïve AU.** Heavily-diverged clusters produce hole-riddled (useless) templates under plain first-order AU. Mitigate with normalization before clustering and **e-graph/robust AU** (babble) for the worst clusters; also let the human split a too-diverse cluster.
- **Coverage is inherently low at high precision.** Expect to confidently canonize a *core* of clusters and leave a long tail unmined. Communicate coverage honestly; don't imply full-repo canonization.
- **Compilation fidelity gaps.** A template may be expressible as AST-with-holes but awkward as an ast-grep/Semgrep pattern (data-flow, ordering-insensitive, cross-file constraints). RhoSynth's FOL-over-graph route (>75% precision, deployed) is the fallback when pattern-matching syntax can't express the constraint — but it's heavier to build.
- **LLM temptation.** Using an LLM as the *generalizer of record* reintroduces nondeterminism and unverifiable output into a pipeline whose whole value proposition is *deterministic enforcement*. Keep the LLM to labeling/explanation; keep AU + self-test as the trusted spine.

---

## Source index (verified)

**Anti-unification / LGG:** <https://en.wikipedia.org/wiki/Anti-unification> · <https://www.ncbi.nlm.nih.gov/pmc/articles/PMC6109779/> · <https://arxiv.org/pdf/1801.07438>
**Getafix:** <https://arxiv.org/abs/1902.06111> · <https://dl.acm.org/doi/10.1145/3360585> · <https://engineering.fb.com/2018/11/06/developer-tools/getafix-how-facebook-tools-learn-to-fix-bugs-automatically/> · <https://software-lab.org/publications/Getafix_arXiv_1902.06111.pdf>
**REFAZER / PROSE:** <https://arxiv.org/abs/1608.09000> · <https://www.microsoft.com/en-us/research/publication/learning-syntactic-program-transformations-examples/> · <https://github.com/gustavoasoares/refazer>
**Revisar:** <https://ar5iv.labs.arxiv.org/html/1803.03806> · <https://dl.acm.org/doi/10.1145/3474624.3474650>
**babble:** <https://arxiv.org/abs/2212.04596> · <https://inst.eecs.berkeley.edu/~cs294-260/sp24/projects/erawn/>
**Idiom mining:** <https://arxiv.org/abs/1404.0417> · <https://miltos.allamanis.com/publications/2014mining/> · <https://arxiv.org/abs/2107.06402> · <https://dl.acm.org/doi/abs/10.1145/3510457.3513046>
**API-usage mining:** <https://taoxie.cs.illinois.edu/publications/ecoop09-mapo.pdf> · <https://plv.colorado.edu/bec/papers/biggroum-saner18.pdf> · <https://www.sciencedirect.com/science/article/pii/S0164121224000177>
**LLM-era mining:** <https://arxiv.org/pdf/2412.16814>
**Clustering at scale:** <https://arxiv.org/abs/1512.06448> · <https://ieeexplore.ieee.org/document/5970189/> · <https://www.researchgate.net/publication/4251304_DECKARD_scalable_and_accurate_tree-based_detection_of_code_clones> · <https://dbresearch.uni-salzburg.at/publications/2010tods-pq-gram-distance-ordered-labeled-trees/paper.pdf> · <https://www.aledlie.com/reports/2025-12-02-minhash-lsh-implementation-code-clone-detection/>
**Drift gradient / genealogy:** <https://web.cs.ucla.edu/~miryung/Publications/esecfse05-clonegenealogy.pdf> · <https://www.cs.mcgill.ca/~eduala/papers/duala-ekoko-TrackingCodeClones.pdf> · <https://ieeexplore.ieee.org/abstract/document/6676939/>
**Template → checker:** <https://ast-grep.github.io/blog/ast-grep-agent.html> · <https://ast-grep.github.io/reference/rule.html> · <https://semgrep.dev/docs/writing-rules/pattern-syntax> · <https://dl.acm.org/doi/10.1145/3563350> · <https://www.amazon.science/publications/synthesizing-code-quality-rules-from-examples> · <https://arxiv.org/pdf/2204.08643>
