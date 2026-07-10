# Representing Code Patterns as Searchable, Understandable, Enforceable Objects

*Research report — pattern/architecture governance tool. Prepared July 2026.*

---

## Executive Summary

**The core question:** how do we represent a codebase's "patterns" (e.g. *"all DB access goes through `repositories/`"*, *"errors use `Result` types, never `-1`/`None`"*, *"money formatting uses `money.format`"*) so that a single artifact serves three consumers at once — **SEARCH** (an agent asks "what pattern applies to what I'm about to write?"), **ENFORCEMENT** (mechanical checking of a diff), and **UNDERSTANDING** (a human rebuilds the design rationale)?

**The central finding: no single representation serves all three.** The three consumers pull in orthogonal directions:

| Consumer | Wants | Best-served by |
|---|---|---|
| SEARCH | fuzzy recall, "closest exemplar to my intent", NL→pattern | embeddings + hybrid (BM25) retrieval over prose + exemplars |
| ENFORCEMENT | precise, deterministic, low-false-positive matching of a diff | structural matchers (ast-grep/Semgrep) + graph/import queries |
| UNDERSTANDING | the *why*, the tradeoffs considered, the theory of the design | prose rationale in a decision-record format (ADR/MADR) |

A structural rule is precise but says nothing about *why*; an embedding captures *topic* but not *structure*; prose is human-readable but mechanically inert and rots silently. Each representation is strong exactly where the others are weak.

**Therefore the recommended representation is a *composite "pattern object"* — a single versioned record with four bound facets plus provenance:**

1. **Rationale (prose)** — the theory/why, in a lightweight decision-record schema (MADR-style: context, decision, drivers, consequences, alternatives). Serves UNDERSTANDING; also the highest-signal text for retrieval.
2. **Exemplar (a pointer to live code)** — a canonical "golden" instance in the actual repo (`file#Lsymbol@commit`), not a copy. Serves SEARCH (few-shot / nearest-exemplar) and UNDERSTANDING (concrete instance), and is the anchor for decay detection.
3. **Rule (machine-checkable)** — one or more of: a structural query (ast-grep/Semgrep/GritQL/tree-sitter), an import/layer contract (import-linter/Tach/ArchUnit), or a graph query (CodeQL/Joern/Glean/Cypher). Serves ENFORCEMENT. Chosen per-pattern because expressiveness differs (single-file vs. cross-file vs. dataflow).
4. **Index (embedding)** — vector(s) of the rationale + exemplar (and optionally the rule's intent), stored for nearest-neighbour SEARCH, using a code-specialized embedding model with hybrid lexical fallback.

Plus **provenance/decay controls**: the exemplar pointer and the rule are both *derived from and validated against live code in CI*, so drift makes them fail loudly rather than rot silently. This is the key move that keeps the object honest — the enforcement rule doubles as a docs-as-tests conformance check for the pattern itself.

The rest of this report surveys each approach family, tabulates what it can and cannot express, then specifies the composite pattern-object schema and open questions.

**One structural insight repeated throughout:** the *hard* architectural rules — "use the existing helper instead of reinventing it", "all DB access flows through the repository layer", "this module must not depend on that one, even transitively" — are **cross-file, cross-procedure, or semantic invariants** that single-file structural matchers *cannot* express. Those need graph/dataflow/import-contract representations (or Type-4 clone detection for the "you reinvented it" case). Any serious tool needs both the cheap single-file matcher *and* the expensive cross-file query, and must know which class each pattern falls into.

---

## Part A — Structural Pattern Languages

Tools that match **AST shapes within (mostly) a single file**. Fast, deterministic, good for local conventions ("use `Result`, not `-1`"; "don't call `console.log`"). Weak on anything that spans files or requires understanding data flow.

### A.1 ast-grep

Rust CLI for structural search, lint, and rewrite, built on tree-sitter. Patterns are written *as code* with metavariables (`$A`, `$$$ARGS`); YAML rules compose three categories ([rule-config](https://ast-grep.github.io/guide/rule-config.html), [yaml reference](https://ast-grep.github.io/reference/yaml.html)):

- **Atomic rules:** `pattern` (AST-shape match), `kind` (node type), `regex` (text).
- **Relational rules:** `inside`, `has`, `follows`, `precedes` — "is this node inside a function whose name is X?".
- **Composite rules:** `all` (AND), `any` (OR), `not`, `matches` (reference a named utility rule).

The docs describe rules as "like CSS selectors that compose to filter AST nodes." Vercel's Turbo uses ast-grep to lint Rust ([GitHub](https://github.com/ast-grep/ast-grep), [lint-rule guide](https://ast-grep.github.io/guide/project/lint-rule.html)).

**Limit:** the ast-grep team explicitly warns that "whether a node matches or not may depend on the order of rule being applied, especially when using `has`/`inside`" — relational matching is intra-tree. No cross-file reasoning, no dataflow, no type resolution.

### A.2 Semgrep

The most capable *convention-enforcement* matcher. Single-file mode is free; the interesting architectural power is behind Pro. Rule syntax uses `pattern`, `patterns` (AND), `pattern-either`, `pattern-inside`, `pattern-not`, ellipsis `...`, and metavariables ([rule-syntax](https://semgrep.dev/docs/writing-rules/rule-syntax), [pattern-examples](https://semgrep.dev/docs/writing-rules/pattern-examples)). Explicitly marketed for "banning deprecated function calls, enforcing naming conventions, blocking direct imports from internal packages that should only be accessed through a facade."

Two advanced modes matter for architecture:

- **Taint mode** ([overview](https://semgrep.dev/docs/writing-rules/data-flow/taint-mode/overview)): source→sink dataflow. Tracks taint across functions but **will not cross file boundaries** unless run with `--pro` (inter-file, inter-procedural — only a subset of languages) ([Pro engine](https://semgrep.dev/docs/semgrep-code/semgrep-pro-engine-intro)).
- **Join mode** ([overview](https://semgrep.dev/docs/writing-rules/experiments/join-mode/overview)): runs several rules and returns results only if conditions across their findings hold — a way to fake cross-file rules. **Serious limits:** it operates on *metavariable text strings, not code constructs*, so unrelated same-valued metavariables cause false positives; it reports only the last matching finding; it's experimental, unmaintained, and login-gated. Semgrep's own guidance: prefer Pro cross-file analysis over join mode where feasible.

**Takeaway:** Semgrep is the strongest single-file convention enforcer and *can* reach cross-file/dataflow, but the good cross-file story is a paid, language-limited engine, and the free cross-file story (join) is fragile.

### A.3 comby

Language-agnostic structural search-and-replace using lightweight "holes" `:[name]` that respect balanced delimiters, strings, and comments ([comby.dev](https://comby.dev/), [GitHub](https://github.com/comby-tools/comby)). Excellent for mechanical refactors across ~any language without a full grammar. **Limit:** it's a *syntactic* matcher (balanced-delimiter aware, but not a real typed AST) — no semantic/relational/cross-file reasoning. Best as a rewrite engine, weak as an invariant checker.

### A.4 GritQL

Declarative query language for search + rewrite, now the engine behind Biome's plugin system ([docs.grit.io](https://docs.grit.io/language/overview), [biomejs/gritql](https://github.com/biomejs/gritql), [Biome reference](https://biomejs.dev/reference/gritql/)). Backtick code-snippet patterns + `$metavariable`s; rewrites via `=>`; conditions via `where`, the `<:` match operator, `contains`, `bubble`, and `or` for deep/alternative matching ([patterns](https://docs.grit.io/language/patterns)). More composable than a bare pattern, comparable in spirit to ast-grep. **Limit:** still fundamentally a structural/AST matcher — the same single-file ceiling.

### A.5 CodeQL

The most powerful of the family because it is a *semantic* query language over a relational database of the code, with a real **global dataflow** library ([about data flow](https://codeql.github.com/docs/writing-codeql-queries/about-data-flow-analysis/), [Java dataflow](https://codeql.github.com/docs/codeql-language-guides/analyzing-data-flow-in-java/)). You implement `DataFlow::ConfigSig`, define sources/sinks/barriers, and apply `DataFlow::Global`. This *can* express many architectural invariants ("no data from an HTTP request reaches a raw SQL sink"; "every DB call is reachable only through the repository package"). **Limits:** (1) global dataflow is expensive and imprecise — "not generally feasible to compute all global data flow across the entire program", so you must constrain to specific source/sink pairs; (2) steep learning curve (QL is its own logic language); (3) requires a compiled database, so it's a CI-time tool, not an inline-hint tool.

### A.6 tree-sitter queries

The substrate under ast-grep/Grit/stack-graphs. S-expression patterns over the parse tree, `@captures`, and `#eq?`/`#match?`/`#any-of?` predicates ([syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html), [predicates](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/3-predicates-and-directives.html)). Portable across 100+ grammars and cheap enough to run on every keystroke. **Limit:** purely syntactic, single-tree; predicates are regex/string equality only — no types, no cross-file, no dataflow.

### Structural-matcher capability matrix

| Capability | ast-grep | Semgrep (free) | Semgrep Pro | comby | GritQL | CodeQL | tree-sitter |
|---|---|---|---|---|---|---|---|
| Single-file AST match | ✅ | ✅ | ✅ | ~ (syntactic) | ✅ | ✅ | ✅ |
| Rewrite/autofix | ✅ | ✅ | ✅ | ✅ | ✅ | ~ | ❌ |
| Intra-file dataflow/taint | ❌ | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ |
| **Cross-file / cross-procedure** | ❌ | ⚠️ join only | ✅ | ❌ | ❌ | ✅ | ❌ |
| Type-aware | ❌ | ~ | ✅ | ❌ | ~ | ✅ | ❌ |
| Runs inline (per-keystroke cheap) | ✅ | ~ | ❌ | ✅ | ✅ | ❌ | ✅ |
| Learning curve | low | low-med | med | low | med | high | med |

**What single-file structural matches CANNOT express** (the recurring blind spot):
- **Layering/dependency invariants** ("`domain/` must not import `infra/`, even transitively").
- **"Route everything through the facade/repository"** — requires knowing all call sites and the module graph.
- **"Use the existing helper instead of reinventing it"** — requires semantic clone detection (Part D), not shape matching.
- **Global uniqueness/exhaustiveness** ("every handler is registered", "no two configs define the same key").
- Anything requiring **type resolution** or **whole-program dataflow** beyond a paid/heavy engine.

---

## Part B — Architecture-as-Code & Conformance Checking

This family targets exactly the cross-file invariants Part A can't. Its academic lineage is the **reflexion model**; its practical embodiments are import/layer contract tools.

### B.1 The academic lineage

- **Software Reflexion Models** (Murphy, Notkin & Sullivan, FSE 1995) — the foundational idea ([UBC page](https://www.cs.ubc.ca/~murphy/papers/rm/fse95.html), [ACM DOI](https://dl.acm.org/doi/10.1145/222124.222136)). You (a) declare a high-level model (boxes = modules, arrows = allowed dependencies), (b) map source entities onto the boxes, (c) algorithmically compare declared vs. actual, yielding three verdicts: **convergences** (expected dependency present), **divergences** (dependency present but not allowed), **absences** (expected dependency missing). This triad is *the* conceptual core of every conformance checker since — and directly maps onto our ENFORCEMENT facet: a pattern's rule is a declared edge, a diff either converges or diverges.
- **Architecture Conformance Checking (ACC)** is the dominant evaluation-based approach in the erosion literature. The **"Understanding Software Architecture Erosion: A Systematic Mapping Study"** ([arXiv 2112.10934](https://arxiv.org/abs/2112.10934)) catalogs consistency-, evolution-, and defect-based approaches; **"Towards Automated Identification of Violation Symptoms of Architecture Erosion"** ([arXiv 2306.08616](https://arxiv.org/abs/2306.08616)) is a recent (2023) NLP-on-code-review take. Vendor framing of the same idea: Qt's "Architecture as Code" ([qt.io](https://www.qt.io/software-insights/architecture-as-code-a-developer-friendly-approach-to-architecture-verification)).
- **Dependency Structure Matrix (DSM)** — represent modules as rows/columns of a square matrix, dependencies in cells; cycles and layering violations become visually obvious. Commercialized by **Lattix** with architecture rules enforced in CI ([Lattix DSM guide](https://docs.lattix.com/lattix/userGuide/Working_with_the_Dependency_Structure_Matrix_DSM.html), [dsmweb.org](https://dsmweb.org/)). DSM is the *matrix dual* of the reflexion graph.

### B.2 ArchUnit (JVM)

A test-library: architecture rules are ordinary unit tests. `layeredArchitecture()` names layers and asserts access rules ("Controllers may not be accessed by any layer; Services only by Controllers; Persistence only by Services"); also detects cyclic dependencies, slices, naming conventions, and package access ([user guide](https://www.archunit.org/userguide/html/000_Index.html), [examples](https://github.com/TNG/ArchUnit-Examples)). **Strength:** rules live with the code, run in the normal test suite, fail the build on violation. **Limit:** JVM-only; operates on bytecode/imports, so it's about *dependency structure*, not intra-method conventions.

### B.3 import-linter (Python)

CLI that checks the import graph against declarative **contracts** ([contract types v2.1](https://import-linter.readthedocs.io/en/v2.1/contract_types.html)):

- **Layers** — higher layers may import lower, not vice versa; supports `containers` (apply the same layering across sibling packages) and an **`exhaustive`** flag (every layer must be declared, catching new undeclared modules — an anti-drift feature).
- **Forbidden** — set A must not import set B (descendants + indirect imports included); `allow_indirect_imports` and `ignore_imports` for controlled exceptions (the "protected/allowlist" semantics).
- **Independence** — a set of modules must not depend on each other in any direction, directly or indirectly.

All contracts support `ignore_imports` allowlists and `unmatched_ignore_imports_alerting` (warns when an allowlist entry is stale — again anti-drift). **Strength:** exactly the cross-file dependency invariants; declarative and readable. **Limit:** import graph only — cannot see *how* a dependency is used, only that it exists.

### B.4 Tach (Python, Rust-implemented)

Modular-monolith boundary enforcer: mark source roots and modules, declare allowed dependencies and public interfaces, `tach check` fails CI on violating imports; visualizes the dependency graph; adoptable incrementally with no runtime impact ([GitHub](https://github.com/tach-org/tach), [config docs](https://docs.gauge.sh/usage/configuration/)). Overlaps import-linter but adds explicit **interface/public-API** declarations (a module may be imported, but only its declared surface). **Limit:** same import-graph ceiling.

### Conformance-tool matrix

| Tool | Scope | Expresses | Runs as | Anti-drift feature |
|---|---|---|---|---|
| Reflexion model (concept) | any | declared vs. actual dependency (converge/diverge/absence) | analysis | absence detection |
| DSM / Lattix | any (multi-lang) | layering, cycles, coupling metrics | CI + GUI | impact analysis |
| ArchUnit | JVM | layers, slices, cycles, naming, access | unit tests | tests fail build |
| import-linter | Python | layers, forbidden, independence (transitive) | CLI/CI | `exhaustive`, stale-ignore alerts |
| Tach | Python | module boundaries + public interface | CLI/CI/pre-commit | interface violations |

**What conformance tools express that structural matchers can't:** transitive dependency invariants, layering, module independence, "route through the facade." **What they still can't express:** *value/shape* conventions inside a function ("use `Result` not `-1`", "call `money.format`") — that's back to Part A. **The two families are complementary, not competing.**

---

## Part C — Code-as-Graph

Represent the whole codebase as a queryable graph; express architectural invariants as graph queries (Datalog/Cypher/QL). This is the most *expressive* substrate and the natural backend for both ENFORCEMENT (run a query over a diff's neighborhood) and SEARCH (traverse to find the exemplar / the existing helper).

### C.1 Code Property Graph (CPG) / Joern

A CPG merges AST + control-flow graph + program-dependence (dataflow) graph into one typed, queryable graph ([Joern docs](https://docs.joern.io/), [CPG concept](https://docs.joern.io/code-property-graph/), [CPG spec](https://cpg.joern.io/)). Joern queries with a Scala/Gremlin-style traversal DSL. Because dataflow is *in* the graph, CPG can express "data from source X reaches sink Y through path Z" — the semantic invariants CodeQL targets, but in an interactive graph. **Strength:** single structure covers syntax + control + data. **Limit:** heavyweight to build/keep current; query language is expert-tier; primarily used for security/vuln analysis.

### C.2 Glean (Meta)

Open-source system storing **typed, schema-defined facts** about code in a queryable DB; query language **Angle** is Datalog-inspired and declarative ([glean.software](https://glean.software/), [Meta engineering post, Dec 2024](https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/), [intro](https://glean.software/docs/introduction/)). Facts cover definitions, references, types, calls, inheritance, imports. Indexers exist for C++, Hack, Python, Haskell, Flow, plus **SCIP/LSIF import** for Go, Java, Rust, TypeScript. Supports **incremental indexing** (crucial for staying in sync). **Strength:** language-neutral fact layer, scalable, incremental — a strong backbone for a governance tool's "code state" store. **Limit:** you must write indexers/derivations; Angle is a learning investment.

### C.3 Stack graphs / SCIP (GitHub / Sourcegraph)

- **Stack graphs** ([arXiv 2211.01224](https://arxiv.org/abs/2211.01224)): encode name-binding as a graph where *paths = valid name bindings*; resolving a reference is path-finding. Powers GitHub precise code navigation, built on tree-sitter, incremental and cross-repo.
- **SCIP** ([GitHub](https://github.com/sourcegraph/scip), [announcement](https://sourcegraph.com/blog/announcing-scip)): a protobuf-based, language-agnostic code-intelligence index (successor to LSIF) powering go-to-def / find-refs / cross-repo navigation.

**Relevance:** these give precise *symbol resolution* — the substrate for "find every caller of this helper" (needed for both enforcement and the "you reinvented the helper" check). **Limit:** they model *references/definitions*, not arbitrary architectural predicates; you build invariants on top.

### C.4 Neo4j / Cypher codebase graphs

General graph DB approach: ingest code entities + relations, query with Cypher for multi-hop traversal — dependency chains, blast-radius/impact analysis, cycle detection ([Neo4j codebase KG](https://neo4j.com/blog/developer/codebase-knowledge-graph/)). Design-pattern *detection* research also uses CPG-in-Neo4j + Gremlin matching. **Strength:** mature graph tooling, flexible schema, good for ad-hoc architectural questions. **Limit:** you own the ingestion pipeline and its freshness.

### Graph-approach matrix

| Approach | Query lang | Best at | Cross-file | Dataflow | Incremental | Effort |
|---|---|---|---|---|---|---|
| Joern CPG | Scala/Gremlin DSL | semantic invariants, vuln paths | ✅ | ✅ | ~ | high |
| Glean + Angle | Angle (Datalog) | scalable fact queries, code intel | ✅ | ~ (via facts) | ✅ | high |
| Stack graphs | path-finding | name resolution, nav | ✅ | ❌ | ✅ | med |
| SCIP | (index format) | go-to-def/refs, cross-repo | ✅ | ❌ | ✅ | med |
| Neo4j/Cypher | Cypher | ad-hoc traversal, impact | ✅ | ~ | ~ | med-high |

**Strengths vs. structural matchers:** graphs express *arbitrary relational invariants* (reachability, layering, uniqueness, "is there any path from A to B") that matchers fundamentally can't, and they answer SEARCH-style traversal questions ("what's the nearest existing helper that does this?"). **Weaknesses:** build/freshness cost, expert query languages, and — importantly — they still don't capture the *value-shape* conventions that a one-line ast-grep pattern nails cheaply. **The graph is the heavy artillery; keep the light matchers for local conventions.**

---

## Part D — Vector / Embedding & Semantic Search

This family serves **SEARCH** — the "what pattern applies to what I'm about to write?" and "find the exemplar closest to my intent" questions — and the "you reinvented an existing helper" detection. It is uniquely *bad* at enforcement.

### D.1 Code embedding models (2024–2026)

- **voyage-code-3** (Voyage AI, Dec 2024; updated Sept 2025) — SOTA code retrieval; reported to outperform OpenAI-v3-large and CodeSage-large by **13.8%** and **16.8%** avg across 32 code-retrieval datasets; supports Matryoshka dimensions + int8/binary quantization to cut storage/search cost ([Voyage blog](https://blog.voyageai.com/2024/12/04/voyage-code-3/), [MongoDB writeup](https://www.mongodb.com/company/blog/voyage-code-3-more-accurate-code-retrieval-lower-dimensional-quantized-embeddings)).
- Lineage/context: CodeBERT → CodeSage → CodeXEmbed ([arXiv 2411.12644](https://arxiv.org/abs/2411.12644)) and generalist code-embedding work ([arXiv 2505.12697](https://arxiv.org/abs/2505.12697)); OpenAI `text-embedding-3-*` and Jina code embeddings are common general baselines. Benchmark noise is a real hazard — Voyage notes the original CoSQA had **51%** mislabeled query/code pairs.

### D.2 Retrieval practice: hybrid, not pure-vector

- **Anthropic Contextual Retrieval** ([anthropic.com/engineering](https://www.anthropic.com/engineering/contextual-retrieval)) is the reference recipe: prepend an LLM-generated context blurb to each chunk *before* embedding *and* before BM25 indexing, then rank-fuse and rerank. Measured retrieval-failure reductions: **Contextual Embeddings −35%** (5.7%→3.7%); **+ Contextual BM25 −49%** (→2.9%); **+ reranking −67%** (→1.9%). The lesson for us: pattern retrieval should combine embeddings **and** lexical (BM25) matching **and** reranking, and each chunk (pattern) should carry contextual prose — which the pattern object's rationale facet naturally provides.
- **Sourcegraph** publicly **moved away from pure embeddings** toward a hybrid of keyword search + code-graph + agentic retrieval (Cody→Amp), and deprecated its embeddings context provider in favor of Sourcegraph Search ([semantic code search explainer](https://sourcegraph.com/blog/semantic-code-search-what-it-is-and-how-it-works)). Strong industry signal that **embeddings alone underperform hybrid** for code.

### D.3 Clone detection — the "you reinvented the helper" problem

The canonical taxonomy is Type-1 (exact), Type-2 (renamed), Type-3 (near-miss, added/removed statements), **Type-4 (semantic — syntactically different, same behavior)**. Type-4 is the one that catches "you rewrote a helper that already exists" and is the hardest ([systematic review, arXiv 2306.16171](https://arxiv.org/abs/2306.16171)).
- **SEED** builds a semantic graph from an intermediate representation and beats prior baselines by ~25% F1 on Type-4 ([arXiv 2109.12079](https://arxiv.org/abs/2109.12079)); survey of DL semantic-clone models (ASTNN, GMN, CodeBERT) at [arXiv 2412.14739](https://arxiv.org/abs/2412.14739). Key point: **graph/IR-based** methods beat pure token embeddings for Type-4 — echoing the "embeddings miss structure" finding below.

### D.4 Known failure modes of embeddings for code

Well-documented and central to the design decision:
- Embeddings **capture topic/surface syntax, not semantic/structural equivalence** — "embedding-based metrics capture syntax-level structure but fail to penetrate to code semantic equivalence" ([arXiv 2405.01580](https://arxiv.org/abs/2405.01580)).
- They **miss inter-procedural dataflow** — cannot represent "function A's return flows into function B's buffer" (motivation for CPG+LLM hybrids).
- They **conflate similar-looking, different-behavior code** and vice-versa (two correct implementations of the same thing look far apart if written differently).

**Implication:** embeddings are excellent for *recall/ranking of candidate patterns and exemplars* (SEARCH), and useless as a *precise gate* (ENFORCEMENT). Use them to surface "here are the 3 patterns and exemplars most likely relevant to your diff", then hand off to a structural/graph rule for the actual yes/no check.

### Embedding-approach matrix

| Use | Fit | Why |
|---|---|---|
| "Which pattern applies to what I'm writing?" | ✅ strong | NL/code→nearest pattern by meaning |
| "Find the exemplar closest to my intent" | ✅ strong | nearest-neighbour over exemplars |
| "Did I reinvent an existing helper?" (Type-4) | ~ (graph/IR clone detection > pure embedding) | embeddings miss structure |
| Precise diff enforcement (block/allow) | ❌ | non-deterministic, topic-not-structure, no clear threshold |
| Rebuild the design rationale | ❌ | embeddings aren't human-readable |

---

## Part E — Hybrid & Exemplar-Based Representations (prior art for the composite)

The most relevant prior art: how people already bundle *rationale + example + rule* for humans and agents.

### E.1 Decision-record formats (rationale/UNDERSTANDING)

- **ADR / MADR** ([adr.github.io/madr](https://adr.github.io/madr/), [adr/madr GitHub](https://github.com/adr/madr)): Markdown-based, version-controlled, plain-text records with a fixed skeleton — *context, decision, decision drivers, considered options with pros/cons, consequences*. This is the proven schema for capturing "the why + the alternatives," exactly the UNDERSTANDING facet. MADR ships minimal/full templates. Its section structure is essentially a ready-made rationale schema for our pattern object.

### E.2 Spec-vs-change split (keeping truth separate from proposals)

- **OpenSpec** ([GitHub](https://github.com/Fission-AI/OpenSpec/), [openspec.dev](https://openspec.dev/)): splits repo knowledge into `specs/` (current truth) and `changes/` (proposed **delta specs** marked ADDED/MODIFIED/REMOVED); on merge, `archive` folds the change into the spec so the spec "stays in sync with reality." This truth/proposal split is directly applicable: a **pattern** is a spec; a **diff under review** is a change measured against it. The delta model also gives a clean way to *evolve* a pattern object without losing history.

### E.3 Golden paths / paved roads (exemplar as product)

- **Golden Path / Paved Road** (Spotify, Netflix, Google origins; [The New Stack](https://thenewstack.io/paved-roads-golden-paths-guardrails-and-railroads/), [Red Hat](https://www.redhat.com/en/topics/platform-engineering/golden-paths)): an opinionated, supported, well-documented reference way to do a thing — "not a mandate but a product: if you use this, your life is easier." Reference implementations + templates (Cookiecutter/Copier). This is the *organizational* version of our exemplar facet: the pattern object is the machine-readable golden path for one convention, with a live exemplar as its reference implementation.

### E.4 Design-pattern mining (auto-discovering exemplars)

- Static/dynamic/ML approaches to *detect* design-pattern instances from source: metrics+ML classification, CPG-in-Neo4j graph matching, and recent ML like **GEML** (grammar-based evolutionary; [arXiv 2401.07042](https://arxiv.org/abs/2401.07042)) and **DPS** design-pattern summarization ([arXiv 2504.11081](https://arxiv.org/abs/2504.11081)). Relevance: a governance tool can **bootstrap** pattern objects by mining recurring structures and nominating the most-central instance as the exemplar — reducing manual authoring. Caveat: detectors struggle to distinguish patterns with similar class structure, so human curation of the exemplar remains necessary.

### E.5 Agent conventions / few-shot from your own codebase

- **CLAUDE.md & few-shot** ([Claude Code best practices](https://code.claude.com/docs/en/best-practices)): the CLAUDE.md file is the "constitution" read every session to anchor conventions/commands; Anthropic's guidance highlights **implicit few-shot examples** as the most reliable way to steer output. Relevance: the SEARCH facet's payload to an agent should be *few-shot from the actual repo* — the pattern's rationale + its live exemplar — not an abstract description. This is why the exemplar must be a **pointer to real code**, so the few-shot stays current.

---

## Part F — The Decay Problem (keeping the representation in sync)

Every documentation-flavored representation rots. The literature and practice converge on a few mechanisms:

- **Spec drift is the named failure mode:** "spec drift happens when the behavior of code no longer matches its documentation/specifications" — implementation evolves while contracts/docs lag ([InfoQ: SDD when architecture becomes executable](https://www.infoq.com/articles/spec-driven-development/)). Gojko Adzic's *living documentation* point: it breaks down the moment teams stop treating specs as a maintained single source of truth.
- **Docs-as-tests / executable specification:** the durable fix is to make the representation *executable and enforced in CI* so divergence fails the build rather than rotting silently. This is precisely what ArchUnit (tests), import-linter/Tach (CI checks), and Semgrep/ast-grep (CI lint) already do — and why the **rule facet doubles as the decay detector**.
- **Ownership/sync-gate:** SDD practitioners stress that drift is a *governance* problem — someone must own reconciling concurrent changes; the `archive`/delta mechanism (OpenSpec) and the "sync-owner-gate" pattern formalize this.
- **Incremental re-indexing:** Glean and stack graphs are built for incremental updates, so the graph/fact layer tracks code changes cheaply — important if the exemplar pointer and graph queries are to stay live.

**Design consequence — the anti-decay contract:** every pattern object's **exemplar pointer and rule must be validated in CI against live code.** If the exemplar's symbol moves/deletes, or the rule stops matching the exemplar it was derived from, the pattern object fails — surfacing decay as a red build instead of stale prose. This turns the enforcement facet into a self-test of the documentation, closing the decay loop.

---

## Recommended Composite: the "Pattern Object"

No single representation serves search + enforcement + understanding. Bind four facets + provenance into one versioned record. Store it in-repo (git-friendly, like MADR/OpenSpec) with a machine-readable header and prose body.

```yaml
# pattern: db-access-through-repositories
id: db-access-through-repositories
title: "All database access goes through repositories/"
status: active            # active | deprecated | proposed (OpenSpec-style lifecycle)
scope: [src/**]

# ── FACET 1: RATIONALE (UNDERSTANDING) — MADR-shaped prose in the body below
drivers: ["testability", "swappable persistence", "single audit point"]

# ── FACET 2: EXEMPLAR (SEARCH + UNDERSTANDING) — pointer to LIVE code, not a copy
exemplar:
  ref: "src/users/repository.py#UserRepository.get@<commit>"
  antipattern_ref: "git-history:<sha>#raw-sql-in-handler"   # optional negative example

# ── FACET 3: RULE (ENFORCEMENT) — pick the representation that fits the invariant class
rule:
  kind: import-contract            # ast-grep | semgrep | gritql | import-contract | graph-query | clone
  engine: import-linter
  spec: |
    type: forbidden
    source_modules: ["myapp.web", "myapp.services"]
    forbidden_modules: ["myapp.db.raw"]
    allow_indirect_imports: false
  # value-shape conventions instead use kind: ast-grep/semgrep (single-file, cheap, inline)

# ── FACET 4: INDEX (SEARCH) — populated by CI from rationale + exemplar
embedding:
  model: "voyage-code-3"
  vectors: [rationale_vec, exemplar_vec]     # hybrid: also BM25-indexed (contextual retrieval)

# ── PROVENANCE / ANTI-DECAY
provenance:
  derived_from: "mined 2026-06 (GEML) + curated"
  ci_validation: ["rule matches exemplar", "exemplar symbol resolves"]  # docs-as-tests
```

**Why each facet, mapped to the three consumers:**

| Facet | SEARCH | ENFORCEMENT | UNDERSTANDING | Backed by |
|---|---|---|---|---|
| Rationale (MADR prose) | ✅ (best retrieval text) | — | ✅✅ | ADR/MADR (E.1) |
| Exemplar (live pointer) | ✅✅ (few-shot / nearest) | ~ (rule's anchor) | ✅ | golden paths (E.3), CLAUDE.md few-shot (E.5) |
| Rule (structural/graph/contract) | ~ | ✅✅ | ~ | Parts A/B/C |
| Index (embedding + BM25) | ✅✅ | ❌ | — | Part D, contextual retrieval (D.2) |
| Provenance / CI validation | — | ✅ (self-test) | ~ | Part F |

**Rule-selection policy (which representation per pattern) — the key operational rule:**

| Pattern class | Example | Representation |
|---|---|---|
| Value/shape convention, single file | "use `Result`, not `-1`/`None`"; "call `money.format`" | ast-grep / Semgrep / GritQL (cheap, inline) |
| Local dataflow within a file/function | "don't log secrets"; taint | Semgrep taint |
| Layering / module dependency (transitive) | "`domain/` ∌ `infra/`"; "DB only via repository" | import-linter / Tach / ArchUnit / graph query |
| Cross-procedure / whole-program dataflow | "no request data reaches raw SQL" | CodeQL / Joern CPG / Semgrep Pro |
| Reachability / uniqueness / "route through facade" | "every handler registered"; "one entry point" | Glean/Angle or Neo4j/Cypher graph query |
| "You reinvented an existing helper" | duplicate of `utils.retry` | Type-4 clone detection (SEED-style) + graph refs |

**Retrieval design (SEARCH consumer):** hybrid contextual retrieval — embed *rationale + exemplar* per pattern, index with BM25 too, rerank; return top-k pattern objects to the agent as few-shot (rationale + live exemplar), then run that pattern's rule as the deterministic gate. This mirrors Anthropic's contextual retrieval numbers and Sourcegraph's hybrid pivot.

**Storage backbone:** pattern objects as in-repo files (git history = evolution, OpenSpec-style spec/change split for edits); a code-fact/graph layer (Glean or stack-graphs/SCIP, incremental) to resolve exemplar pointers and run graph-class rules; embeddings in a vector store refreshed in CI.

---

## Open Questions

1. **Authoring cost vs. coverage.** Four facets per pattern is expensive. How much can be **auto-bootstrapped** (mine exemplars via GEML/design-pattern detection; synthesize a draft rule from the exemplar; generate rationale via LLM) vs. must be human-curated? What's the minimum viable pattern object (rationale + exemplar + one rule)?
2. **Rule–exemplar coupling / auto-synthesis.** Can we reliably *generate* the structural rule from a curated exemplar (learning-from-examples for ast-grep/Semgrep)? If the exemplar is the source of truth, drift detection becomes "does the auto-derived rule still match the exemplar?"
3. **Cross-file rule portability.** import-linter/Tach/ArchUnit are language-specific; graph queries need a per-language fact layer. Is there a language-neutral way to express layering/reachability invariants (SCIP/Glean as the common substrate), or does each ecosystem need its own enforcement engine?
4. **Type-4 "reinvented helper" at authoring time.** Semantic clone detection (SEED) is accurate but heavy/offline. Can it run inline/pre-commit fast enough to warn "a helper already does this"? Or is graph "find-similar-by-refs" a cheaper proxy?
5. **Embedding granularity for patterns.** Do we embed the rationale, the exemplar code, an LLM-written "when does this apply" blurb, or all three? Contextual retrieval says context helps — what's the best per-pattern chunk?
6. **Enforcement precision vs. adoption.** Blocking hooks with false positives get disabled. What confidence threshold / allowlist model (import-linter's `ignore_imports`, Semgrep's `nosemgrep`) keeps enforcement trusted? How do exceptions feed back into the rationale?
7. **Pattern conflict & precedence.** When two patterns' rules disagree on a diff, or a pattern is deprecated mid-migration (OpenSpec delta state), how does the tool resolve/sequence them?
8. **Measuring the object's own health.** Beyond "rule matches exemplar", how do we measure whether a pattern object is *still believed* (violation rate, override rate, staleness of exemplar commit) and retire or refresh it?

---

## Source Index (verified URLs)

**Structural pattern languages:** ast-grep [rule-config](https://ast-grep.github.io/guide/rule-config.html) · [yaml ref](https://ast-grep.github.io/reference/yaml.html) · [pattern syntax](https://ast-grep.github.io/guide/pattern-syntax.html) · [GitHub](https://github.com/ast-grep/ast-grep) · [lint rule](https://ast-grep.github.io/guide/project/lint-rule.html); Semgrep [taint](https://semgrep.dev/docs/writing-rules/data-flow/taint-mode/overview) · [join](https://semgrep.dev/docs/writing-rules/experiments/join-mode/overview) · [Pro engine](https://semgrep.dev/docs/semgrep-code/semgrep-pro-engine-intro) · [rule syntax](https://semgrep.dev/docs/writing-rules/rule-syntax) · [pattern examples](https://semgrep.dev/docs/writing-rules/pattern-examples); comby [site](https://comby.dev/) · [GitHub](https://github.com/comby-tools/comby); GritQL [docs](https://docs.grit.io/language/overview) · [patterns](https://docs.grit.io/language/patterns) · [Biome](https://biomejs.dev/reference/gritql/) · [GitHub](https://github.com/biomejs/gritql); CodeQL [dataflow](https://codeql.github.com/docs/writing-codeql-queries/about-data-flow-analysis/) · [Java dataflow](https://codeql.github.com/docs/codeql-language-guides/analyzing-data-flow-in-java/); tree-sitter [query syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html) · [predicates](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/3-predicates-and-directives.html).

**Architecture-as-code / conformance:** Reflexion [Murphy FSE'95](https://www.cs.ubc.ca/~murphy/papers/rm/fse95.html) · [ACM](https://dl.acm.org/doi/10.1145/222124.222136); erosion [mapping study 2112.10934](https://arxiv.org/abs/2112.10934) · [violation symptoms 2306.08616](https://arxiv.org/abs/2306.08616); [Qt Architecture-as-Code](https://www.qt.io/software-insights/architecture-as-code-a-developer-friendly-approach-to-architecture-verification); DSM [Lattix](https://docs.lattix.com/lattix/userGuide/Working_with_the_Dependency_Structure_Matrix_DSM.html) · [dsmweb](https://dsmweb.org/); [ArchUnit guide](https://www.archunit.org/userguide/html/000_Index.html) · [examples](https://github.com/TNG/ArchUnit-Examples); [import-linter contracts](https://import-linter.readthedocs.io/en/v2.1/contract_types.html); Tach [GitHub](https://github.com/tach-org/tach) · [config](https://docs.gauge.sh/usage/configuration/).

**Code-as-graph:** Joern [docs](https://docs.joern.io/) · [CPG](https://docs.joern.io/code-property-graph/) · [spec](https://cpg.joern.io/); Glean [site](https://glean.software/) · [Meta post](https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/) · [intro](https://glean.software/docs/introduction/); [stack graphs 2211.01224](https://arxiv.org/abs/2211.01224) · [SCIP GitHub](https://github.com/sourcegraph/scip) · [SCIP announce](https://sourcegraph.com/blog/announcing-scip); [Neo4j codebase KG](https://neo4j.com/blog/developer/codebase-knowledge-graph/).

**Embeddings / semantic search / clones:** [voyage-code-3](https://blog.voyageai.com/2024/12/04/voyage-code-3/) · [MongoDB writeup](https://www.mongodb.com/company/blog/voyage-code-3-more-accurate-code-retrieval-lower-dimensional-quantized-embeddings) · [CodeXEmbed 2411.12644](https://arxiv.org/abs/2411.12644); [Anthropic contextual retrieval](https://www.anthropic.com/engineering/contextual-retrieval); [Sourcegraph semantic search](https://sourcegraph.com/blog/semantic-code-search-what-it-is-and-how-it-works); clones [survey 2306.16171](https://arxiv.org/abs/2306.16171) · [SEED 2109.12079](https://arxiv.org/abs/2109.12079) · [DL semantic clones 2412.14739](https://arxiv.org/abs/2412.14739); [embedding limits 2405.01580](https://arxiv.org/abs/2405.01580).

**Hybrid / exemplar / conventions:** [MADR](https://adr.github.io/madr/) · [madr GitHub](https://github.com/adr/madr); OpenSpec [GitHub](https://github.com/Fission-AI/OpenSpec/) · [site](https://openspec.dev/); golden path [New Stack](https://thenewstack.io/paved-roads-golden-paths-guardrails-and-railroads/) · [Red Hat](https://www.redhat.com/en/topics/platform-engineering/golden-paths); pattern mining [GEML 2401.07042](https://arxiv.org/abs/2401.07042) · [DPS 2504.11081](https://arxiv.org/abs/2504.11081); [Claude Code best practices](https://code.claude.com/docs/en/best-practices).

**Decay / living docs:** [InfoQ SDD](https://www.infoq.com/articles/spec-driven-development/) · [Anthropic contextual retrieval](https://www.anthropic.com/engineering/contextual-retrieval) (incremental/hybrid).
