# Designing the `.moore` Pattern Language: Surface Syntax + Semantics

**Report 04 · Pattern-language design · compiled 2026-07-01**

Scope: decide what the Akron pattern file (`.akron/rules/*.moore`) should *look like* to
a human and *mean* to the machine. Inputs: the owner's vision fragments (a "fancy custom
intermediate representation… sort of like a template sort of like a linter"; "we really
are almost making a new language"; "you'd need to support pseudocode as a principle
almost"). Method: five parallel web sweeps over structural matchers, the abstraction-dial
literature, language-generic IRs, LLM-as-linter products, and DSL-adoption postmortems,
plus first-hand fetches of the anchor primary sources. Builds directly on **Report 02**
(the composite "pattern object": rationale + exemplar + rule + index) — this report designs
the *surface* for that object's **rule** facet (and the seams to the other three). Only
URLs fetched or seen verbatim in search are cited; secondary figures are flagged
*unverified*.

---

## 1. Executive summary — what the `.moore` surface + semantics should be

**The recommendation, in one sentence: `.moore` should be a Python-shaped *sketch by
example* — an idealized code fragment with holes plus a small declarative assertion
vocabulary — that is *default-free* (only the parts you pin are checked) and *compiles*
to existing deterministic engines, never a new query language and never a hand-authored
IR.**

Five findings drive this:

1. **"Patterns look like the code" is the single most load-bearing lesson in the field.**
   Semgrep deliberately refused a query language — "You shouldn't need a PhD in program
   analysis, or even to understand what an AST is, to be effective with Semgrep… A novice
   programmer should be able to write their first Semgrep rule in 60 seconds"
   ([Semgrep philosophy](https://docs.semgrep.dev/contributing/semgrep-philosophy/)) —
   and won mindshare because the *authoring population* becomes "anyone who can read the
   language being scanned." Coccinelle made the identical bet on a different notation:
   "Because Linux programmers are accustomed to manipulating program modifications in
   terms of patch files, we base our transformation language on the patch syntax"
   ([Coccinelle](https://coccinelle.gitlabpages.inria.fr/website/sp.html)), and rode it to
   ~8,000 kernel commits over 15+ years. **The surface must be code the reader already
   knows — for Akron (Python-first), that is Python.** This also makes the file *mineable*:
   a draft `.moore` is the anti-unification of a cluster of real files (the common
   skeleton becomes fixed; the varying parts become holes).

2. **A bare code template is not enough — every serious matcher pairs it with a
   constraint layer.** A snippet can only say "match code shaped like *this*." The moment
   you need negation, disjunction, containment/context, or a predicate over a captured
   name, you need a second layer: Semgrep's `pattern-not`/`pattern-inside`, ast-grep's
   relational `inside`/`has`/`follows` + `constraints`, GritQL's `where { $x <: … }`. So
   `.moore` needs a *small* declarative assertion vocabulary (`require`/`forbid`/`where`)
   alongside the template — but kept minimal (Comby's refusal to add a relational layer is
   exactly why it stays language-agnostic yet semantically shallow).

3. **Whole-shape conformance is the nagginess trap; the escape is "default-free,
   pin-what-matters."** A template that must match the *entire* idealized file flags every
   legitimate variation (real files differ in 90 % of their body). The fix is the
   Coccinelle/typed-hole model: the pattern is a *fragment*, unmarked regions are an
   implicit `...` (free), and the file is a *conjunction of small anchored assertions*, not
   a monolith. Strictness becomes a **per-anchor dial** (exact → typed hole → named/
   relational → prose), matched to the pattern class.

4. **Store structurally, but review and edit the surface — never the IR.** Semgrep is
   direct prior art: the *same parser* runs on the query and the source, so the surface
   pattern and the compiled AST-query are kept in sync *by construction* — humans never
   hand-write the IR. difftastic and SCIP reinforce it from the display side (both project
   structure back to real line numbers / human-readable symbol strings). And the
   Babelfish/UAST death is the cautionary tale about the *other* direction: a universal
   semantic IR carries a per-language driver treadmill and died wholesale when its sponsor
   went bankrupt. **`.moore` is the surface; the matcher IR is a derived, regenerated
   artifact reviewed only via its surface.**

5. **Draw the determinism boundary inside the file.** The winning industry pattern (Semgrep
   Assistant, CodeRabbit, G-Research) is: *deterministic core rules hard-gate; LLM-judged
   prose constraints advise but never block*. So `.moore` has two lanes: deterministic
   clauses that compile to ast-grep/Semgrep/import-linter and gate within the 1–5 s budget,
   and an *advisory prose lane* (`advise:`/`note:`) that is served to agents as context and
   optionally LLM-judged as a **non-blocking** PR comment. This is also how "support
   pseudocode as a principle" is honored without letting a non-deterministic judge fail CI.

The rest of this report: §2 surveys the surface syntaxes and their abstraction mechanisms
(with a comparison table); §3 is the strictness-dial / anti-nagginess design; §4 is the
pseudocode/hybrid boundary; §5 is the new-language cost assessment; §6 gives three concrete
design directions with mock `parser.moore` files.

---

## 2. Surface syntaxes & abstraction mechanisms of the prior art

The surveyed tools sit on a **familiarity ↔ power** spectrum, from "the pattern *is* code"
to "code snippets embedded in a logic language." (Report 02's matrix compares them by
*capability class*; this table compares them by *surface style and abstraction
mechanism* — what a human actually types.)

### 2.1 The code-as-pattern camp

- **Semgrep** — the pattern is written in the target language, with a thin abstraction set
  layered on: **metavariables** `$X` ("an abstraction to match code when you don't know the
  value ahead of time"; equal names unify), **ellipsis** `...` ("zero or more items such as
  arguments, statements"), **ellipsis metavariables** `$...ARGS`, **typed metavariables**
  `($X: int)`, and the **deep-expression** operator `<... ...>`. Everything non-code-like is
  pushed into YAML operators: `pattern-either` (OR), `patterns` (AND), `pattern-inside`,
  `pattern-not`, `pattern-not-inside`, `metavariable-comparison`, `metavariable-regex`
  ([rule syntax](https://semgrep.dev/docs/writing-rules/rule-syntax),
  [pattern syntax](https://semgrep.dev/docs/writing-rules/pattern-syntax)). *Cannot express*
  arbitrary predicates over captured text without the Python-flavored escape hatches; no
  interprocedural reasoning on the pattern surface (that is the separate taint engine).
  Learnability is the whole point: "learn Semgrep in a few minutes." ≈15.7k★.

- **ast-grep** — cleanest separation of *atomic pattern* from *relational constraints*. The
  **pattern** is "isomorphic to code you'd write every day" (`$VAR`, `$$$` multi-node);
  **relational rules** `inside`/`has`/`follows`/`precedes` (tunable with `stopBy`, `field`);
  **composite** `all`/`any`/`not`; per-metavariable `constraints`; and **`utils` + `matches`**
  for named, reusable sub-rules (a DRY rule library). The organizing idea: "a node must
  satisfy all fields in the rule object" (AND at the object level, OR via `any`)
  ([rule-config](https://ast-grep.github.io/guide/rule-config.html),
  [relational rules](https://ast-grep.github.io/guide/rule-config/relational-rule.html)).
  *Cannot express* interprocedural dataflow; matching is per-file. ≈14.9k★. **This is the
  cleanest structural model to borrow: code-template primary, relational constraints
  opt-in.**

- **Comby** — language-agnostic templates over **balanced delimiters**: holes `:[name]`,
  `:[[ident]]`, regex holes `:[h~re]`, and `...`. "Comby understands the interaction between
  delimiters, strings, and comments," so `(:[1])` matches only well-balanced parens
  ([basic usage](https://comby.dev/docs/basic-usage)). *Cannot express* relational/semantic
  constraints at all — "it uses no tree definition," is not indentation-significant (weak
  for Python blocks), and is "not well-suited to stylistic changes"
  ([FAQ](https://comby.dev/docs/faq)). Its ceiling is exactly the cost of refusing a
  constraint layer. ≈2.7k★.

- **Coccinelle SmPL** — the pattern is a **unified diff of an idealized fragment**: a
  `@@ … @@` metavariable-declaration block, `-`/`+` transformation lines, and `...`
  ("irrelevant code") with a bounded `<... ...>` variant. It abstracts away "differences in
  spacing… choice of names given to variables (metavariables)… irrelevant code (`...`)…
  other variations in coding style (**isomorphisms**)" — isomorphisms letting one rule match
  semantically-equivalent forms (e.g. `x` vs `x != NULL`). *Cannot express* cross-language
  or whole-program aggregate facts; it is intra-procedural C. Its adoption is the strongest
  evidence in the field that **notation reuse beats notation invention**
  ([Coccinelle](https://coccinelle.gitlabpages.inria.fr/website/sp.html),
  [kernel docs](https://docs.kernel.org/dev-tools/coccinelle.html),
  [Lawall talk, InfoQ](https://www.infoq.com/presentations/coccinelle-linux-kernel/)).

### 2.2 The query-language camp

- **GritQL** — flips the model: it *is* a declarative query language with backtick code
  snippets as one construct. Metavariables `$x`, rewrite operator `=>`, and a `where { … }`
  clause using the **match operator `<:`** with `and`/`or`/`!`, `contains`, `within`
  ([patterns](https://docs.grit.io/language/patterns),
  [conditions](https://docs.grit.io/language/conditions)). More composable than a bare
  template; heavier surface (non-trivial constraints *require* leaving snippet mode). Now
  the engine behind Biome's plugins. ≈4.5k★.

- **tree-sitter queries** — **S-expressions** over the parse tree: node types in parens,
  `field:` children, `@captures`, predicates `#eq?`/`#match?`/`#any-of?`. One-to-one with
  the CST, which is exactly why they are machine-natural and human-hostile: you must think
  in concrete-tree structure, queries "get huge" for nested shapes, and predicates are
  filters, not computation
  ([syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html)).
  This is the substrate under ast-grep/Grit — **the right *implementation* layer, the wrong
  *authoring* layer.**

- **CodeQL (QL)** — a full object-oriented logic language ("syntax similar to SQL,
  semantics based on Datalog") over a *relational database* extracted from a build. Buys
  deep taint/dataflow and **variant analysis** (one query → all variants; 400+ CVEs) at a
  documented steep cost: "It's a programming language, a tool, and a supporting ecosystem…
  a distinct lack of end-to-end instruction"
  ([Learning CodeQL](https://goingbeyondgrep.com/posts/learning-codeql/)). Requires a
  compiled DB — a CI-time tool, not an inline hint.

- **OPA / Rego** — Datalog-derived policy DSL, the canonical "powerful but nobody wants to
  write it" case: "Its roots in Prolog and Datalog give it a steep learning curve…
  Everyone loves Policy as Code, but no one wants to write Rego"
  ([permit.io](https://www.permit.io/blog/no-one-wants-to-write-rego)). Competitors market
  YAML explicitly to avoid it. **A direct warning against a logic-language surface.**

### 2.3 The host-language-native camp (Python)

- **LibCST `matchers`** — matcher *objects* mirroring CST node classes; unspecified
  attributes default to a `DoNotCare` sentinel; composable with `|` (OR) and sequence
  matchers: `m.Call(args=(m.ZeroOrMore(...),))`, `m.matches(node, m.Name("True") |
  m.Name("False"))`, `@m.leave(…)`
  ([matchers tutorial](https://libcst.readthedocs.io/en/latest/matchers_tutorial.html)).
  Reads as ordinary Python — no new grammar — at the cost of verbosity and Python-only
  scope. **Directly relevant: a Python-native matcher needs no new parser.**
- **Bowler** — fluent selector pipeline (`Query(...).select_function("old").rename("new")`),
  ergonomic but coarse; repo **archived Aug 2025**, maintainers point to LibCST codemods
  ([Bowler](https://github.com/facebookincubator/bowler)).

### 2.4 Comparison table — surface & abstraction mechanisms

| Tool | Surface style | Hole / metavar | Ellipsis "…" | Disjunction | Negation | Relational / context | Rewrite | Learning curve |
|---|---|---|---|---|---|---|---|---|
| **Semgrep** | target-language code | `$X`, `$...A`, typed `($X:int)` | ✅ | `pattern-either` | `pattern-not` | `pattern-inside` | ✅ | **very low** |
| **ast-grep** | target-language code + YAML | `$VAR`, `$$$` | ✅ (`$$$`) | `any` | `not` | `inside`/`has`/`follows` | ✅ | low |
| **Comby** | text template | `:[h]`, `:[[id]]`, `:[h~re]` | ✅ | ✗ | ✗ | ✗ (delimiter-only) | ✅ | low |
| **Coccinelle** | unified-diff fragment | `expr x;` metavars | ✅ (`<...>`) | disj. patterns | via `-`/context | `...` sequencing | ✅ (`-`/`+`) | low–med (C only) |
| **GritQL** | query lang + snippets | `$x` | `...` | `or` | `!` | `contains`/`within` | `=>` | med |
| **tree-sitter** | S-expressions | `@cap`, `(_)` | ✗ | alternations | `#not-eq?` | parent/field nesting | ✗ | med |
| **CodeQL** | Datalog/OO logic | class predicates | n/a | logical | logical | full dataflow | ✗ | **high** |
| **Rego** | Datalog policy | unification vars | n/a | logical | `not` | comprehensions | ✗ | **high** |
| **LibCST matchers** | Python objects | `DoNotCare`, `SaveMatchedNode` | `ZeroOrMore` | `\|` | `DoesNotMatch` | node nesting | ✅ (codemod) | low–med (Py only) |

**The convergent lesson:** the code-like template is the ideal surface for the common case
(~80 %), but structural matching inevitably needs a *small* constraint/relational layer for
the hard case. Tools differ only in how visible they make that seam — Semgrep/ast-grep keep
the code-template primary with constraints opt-in (adoptable); GritQL/CodeQL/Rego promote
the logic language to primary (powerful, high learning tax). **`.moore` should be on the
Semgrep/ast-grep side of that seam.**

---

## 3. The strictness dial — how to avoid whole-shape nagginess

This is the crux of the owner's tension. A "template = idealized file with holes" is
human-natural and mineable, but if the *whole shape* must conform it becomes a naggy gate:
every real parser legitimately differs in most of its body, so whole-file matching flags
noise. The literature offers a clean resolution.

### 3.1 Default-free: the pattern is a fragment, not a file

Coccinelle already solved this: a semantic patch matches a *fragment*, and unmarked code is
an implicit `...`. The `usb_submit_urb` example pins only "inside a `spin_lock_irqsave …
spin_unlock_irqrestore` region, a `usb_submit_urb(urb)` call must pass `GFP_ATOMIC`" — the
rest of the function is free. **`.moore` should adopt the same default: everything you did
not explicitly pin is a hole.** A `.moore` file is therefore a *conjunction of small
anchored assertions*, not a monolithic shape to conform to (ast-grep's "rule object = AND of
fields," applied so each field stays small and independently justifiable).

### 3.2 The abstraction dial: what pins each hole

The abstraction-dial literature (SKETCH, typed holes, protocols-as-types) gives the
vocabulary for *how tightly* an anchor is pinned. Across SKETCH's `??` (a hole constrained
by a grammar/spec), Hazel/Agda/Idris typed holes (a hole whose *type + context* is known
even when its content is not — "every editor state has some type"), cookiecutter's named
`{{ slots }}` (a hole pinned only by a *name*), and Python `Protocol` vs Rust `trait`
(**structural** "match anything shaped like this" vs **nominal** "match only what declares
itself this"), the recurring axis is *what constrains the hole*: a **type**, a **grammar**,
or a **name only**. That gives a natural five-level strictness dial:

| Level | Anchor pins… | `.moore` construct | Compiles to | Blocking? | Use when |
|---|---|---|---|---|---|
| **L0 Exact** | this literal code | verbatim line | ast-grep exact | yes | boilerplate that must be byte-identical (rare) |
| **L1 Structural** | this *shape*, holes free | template + `$X` + `...` | ast-grep / Semgrep pattern | yes | "call `money.format(...)`", "use `Result[...]`" |
| **L2 Typed hole** | shape + a hole's *type* | `($X: Decimal)`, `-> Result[$_, $_]` | Semgrep typed-metavar / mypy | yes | value-shape conventions with a type contract |
| **L3 Named / relational** | must-have / must-not-have facts | `require:` / `forbid:` / `where:` | Semgrep `pattern-not-inside`, import-linter | yes | "no bare `raise` escapes", "boundary parses first", layering |
| **L4 Prose** | *intent* that resists formalization | `advise:` / `note:` | served to agent; LLM-judged | **no** (advisory) | "validate every field at the boundary; downstream assumes validity" |

Two design consequences:

- **Match the level to the pattern class** (Report 02's rule-selection policy). Single-file
  value-shape → L1/L2 compiled to ast-grep/Semgrep. Cross-file layering ("DB only via the
  repository") → L3 compiled to import-linter/Tach (a single-file template *cannot* express
  it — Report 02 §A). Whole-program dataflow → CodeQL/Semgrep-Pro. Intent → L4 prose. The
  `.moore` surface is uniform; the *backend it compiles to* varies by anchor.
- **Loosen by default, tighten on evidence.** A mined draft should start at L1/L3 (shape +
  a few must/must-not facts), not L0. You tighten a specific anchor to L2 only when the type
  contract is genuinely load-bearing. This keeps the gate about *load-bearing invariants*,
  not incidental structure.

### 3.3 Precision knobs that keep a gate trusted

Report 03's failure mode is decisive: *blocking hooks with false positives get disabled*.
Real tools ship escape valves — Semgrep's `# nosemgrep`, import-linter's `ignore_imports`,
ast-grep per-rule severity. `.moore` must inherit them: every anchor supports a scoped,
audited exception (`allow: <ref> # reason`), so a single false positive costs one annotated
override, not the whole file's credibility. Exceptions should feed back into the pattern's
rationale (Report 02's provenance facet) so the canon learns.

**Net anti-nagginess design:** default-free fragments + per-anchor strictness + audited
exceptions turns "whole-shape conformance" into "a short list of independently-justified,
individually-overridable invariants" — precise where it matters, silent everywhere else.

---

## 4. The pseudocode / hybrid boundary

The owner wants to "support pseudocode as a principle." The field has a clear, consistent
answer for *where the determinism boundary sits*, and it is not "let an LLM decide the
gate."

### 4.1 The industry pattern: deterministic detector, LLM interpreter on top

- **Semgrep Assistant** keeps the rule engine fully deterministic and bolts AI on as a
  triage/authoring layer: Assistant "receives results from Semgrep's deterministic static
  analysis engine… the engine performs the detection work," and the AI triages, prioritizes,
  and drafts fixes — advisory, human-in-the-loop. NL "memories" are "deployed by customers
  after manual approval"
  ([Semgrep Assistant tech](https://semgrep.dev/blog/2024/the-tech-behind-semgrep-assistant/)).
- **CodeRabbit** is the clearest articulation: it uses **ast-grep** to "extract concrete,
  deterministic information about code (variable names, function signatures, dependencies)"
  and feeds it to the LLM because "the retrieved context grounds the LLM with factual
  information, reducing possible hallucinations"
  ([CodeRabbit + ast-grep](https://www.coderabbit.ai/blog/ai-native-universal-linter-ast-grep-llm)).
  Its **"learnings"** are "natural-language statements about code-review preferences," and
  its own guidance — "explain the reasoning… the 'why' helps CodeRabbit apply the learning
  correctly in similar-but-not-identical situations"
  ([learnings](https://docs.coderabbit.ai/knowledge-base/learnings)) — is precisely the
  advisory-prose lane: flexible but *interpreted*, not exact.
- **Authoring-time LLM, enforcement-time determinism.** LintCFG (arXiv 2602.07783,
  *unverified*) has an LLM *compile* NL coding standards into deterministic linter configs —
  moving the model to authoring time and keeping the gate deterministic. This is the model
  for mining a `.moore`: an LLM proposes `require`/`forbid` clauses from a cluster of
  exemplars; a human *ratifies via PR*; enforcement is then deterministic.

### 4.2 Why the LLM cannot be the hard gate

Primary evidence is blunt. G-Research, building an LLM review tool: "LLMs don't fail like
traditional APIs; they truncate, drift, and generate structurally valid but incorrect
output," and "we deliberately decided not to block merges… In a CI environment, false
positives are expensive; engineers risk learning to disregard the tool"
([G-Research](https://www.gresearch.com/news/building-a-code-review-tool-the-llm-patterns-that-actually-work/)).
Research that LLMs "lean on priors, not programming-language semantics" (arXiv 2510.03415,
*unverified*) is a direct argument against authoritative LLM gates. **Nobody in the sourced
material lets a raw LLM verdict block a merge.**

### 4.3 The boundary, drawn inside the `.moore` file

`.moore` should make the boundary *syntactic and visible*:

- **Deterministic lane** — `match` / `require` / `forbid` / `where` compile to
  ast-grep / Semgrep / import-linter and **hard-gate** within the 1–5 s pre-commit/CI
  budget. This is the "linter" half of the owner's "template ∧ linter."
- **Advisory lane** — `advise:` / `note:` prose is (a) *served to the AI agent as context
  before it writes code* (the primary consumption path — Report 01/03), and (b) optionally
  LLM-judged **as a non-blocking PR comment**, with the deterministic anchor results handed
  to the judge as grounding facts (the CodeRabbit pattern) to cut hallucination.

This honors "pseudocode as a principle" without ever letting non-determinism fail a build:
the pseudocode lives in the advisory lane, shapes the agent's generation, and at most
*comments* at review time.

---

## 5. New-language cost assessment

"We really are almost making a new language" is the most dangerous sentence in the brief.
The evidence says: **do not make a new language; make a thin, familiar surface skin over
existing engines.**

### 5.1 The costs, itemized

- **The perpetual learning tax.** CodeQL and Rego are the field's proof that a powerful
  bespoke language is a permanent onboarding liability ("no one wants to write Rego"). The
  Configuration Complexity Clock (Hadlow) formalizes the trajectory: systems drift from
  hard-code → config → rules engine → **a DSL at 9 o'clock** → and back to "hard-coding a
  solution in a crappier language," where the DSL stage means "a harder learning curve for
  new hires" and "little tooling support"
  ([Configuration Complexity Clock](http://mikehadlow.blogspot.com/2012/05/configuration-complexity-clock.html)).
- **Turing-completeness creep.** "Every simple language will develop enough features to
  eventually end up Turing Complete" (Helm/YAML, PHP as exhibits); the recommended escape is
  an *embedded* DSL — "start from a rich programming environment and include the simple
  configuration as language constructs"
  ([solutionspace](https://solutionspace.blog/2021/12/04/every-simple-language-will-eventually-end-up-turing-complete/)).
- **The universal-IR treadmill (the Babelfish tax).** source{d}'s Universal AST needed a
  per-language driver for *every* language; the repos are archived and "following source{d}'s
  bankruptcy in late 2019" the ecosystem rotted wholesale
  ([go-git HISTORY](https://github.com/go-git/go-git/blob/master/HISTORY.md),
  [bblfsh org](https://github.com/bblfsh)). tree-sitter and Semgrep survived precisely
  because they were *less* ambitious — structural-only normalization, community grammars.
- **2026 table-stakes tooling.** A new file format that ships neither a tree-sitter grammar
  (syntax highlighting + error-tolerant parsing) nor an LSP server (diagnostics, go-to-def)
  reads as second-class; the modern baseline is ~300 LSP servers and tree-sitter grammars as
  common infrastructure ([tree-sitter vs LSP](https://lambdaland.org/posts/2026-01-21_tree-sitter_vs_lsp/)).
  A from-scratch language owes: parser + grammar + LSP + formatter + highlighter + docs +
  bindings.

### 5.2 What makes a new surface adoptable — and how `.moore` gets it cheaply

The Pkl "Goldilocks" scorecard is explicit: **Starlark wins approachability as "a minimal
subset of Python"; Dhall/Jsonnet stay niche because "custom syntax comes at the cost of
approachability"**
([Pkl analysis](https://medium.com/kurtosis-tech/pkl-and-the-goldilocks-problem-of-configuration-languages-dc36621e102a)).
Three adoption levers — **familiarity, minimal surface, day-one tooling** — map onto a cheap
path for `.moore`:

| Approach | Learning tax | Tooling cost | Expressiveness | Mining-friendliness | Precedent |
|---|---|---|---|---|---|
| **Reuse target language verbatim** (Semgrep) | ~0 | reuse existing | med (needs constraint layer) | ✅ high | Semgrep, Coccinelle |
| **Thin Python-shaped skin** *(recommended)* | low | reuse Python grammar + thin LSP shim | med–high | ✅ high | Starlark, LibCST matchers |
| **New declarative DSL** (GritQL-ish) | med | full new grammar+LSP | high | med | GritQL, ast-grep YAML |
| **New logic language** (CodeQL/Rego) | **high** | full ecosystem | very high | low | CodeQL, Rego |
| **Universal semantic IR** (Babelfish) | high | **per-language driver treadmill** | high | low | **source{d} — dead** |

**The recommended row:** make `.moore` ~valid Python plus a *tiny* hole/annotation
vocabulary. Then Python's tree-sitter grammar and syntax highlighting largely apply for
free; the incremental build is (a) a **compiler** `.moore` → matcher IR (ast-grep/Semgrep/
import-linter rules), (b) a **checker CLI** for the 1–5 s budget, and (c) a **thin LSP
shim** that surfaces "which anchor failed / which hole is unpinned." Crucially, **do not
build a new matching engine** — compile to ast-grep/Semgrep/import-linter, which already own
the hard parts (parsing, incremental matching, autofix). That is the Semgrep economic bet
(reuse the same parser for query and source) applied to Python patterns, and it sidesteps
both the DSL clock and the Babelfish treadmill.

*(Naming aside, low-confidence: `.moore` for a `parser` pattern plausibly nods to Moore
machines — finite-state automata — which is apt, but nothing in the design depends on it.)*

---

## 6. Three recommended design directions (with mock `parser.moore` files)

All three encode the *same* house convention so they're comparable — a "boundary parser"
pattern consistent with the repo's "Parse, don't validate" philosophy: **a parser turns
untrusted input into a trusted domain type, returns a `Result` sum type (never `-1`/`None`/
bare value), and lets no exception escape the boundary.** Syntax below is invented but
grounded in the cited prior art.

### Direction A — Python-shaped sketch that compiles to matchers *(recommended primary)*

The `.moore` file is parseable-as-Python: an idealized fragment with holes (`...` free body,
`$X` metavariables, typed holes), plus `require`/`forbid`/`advise` clauses. **Default-free:
only the pinned anchors are checked.** This is the owner's "template ∧ linter," and it is
directly mineable (anti-unify a cluster → this skeleton).

```python
# .akron/rules/parser.moore  —  canon: boundary parsers return Result, never raise past the edge
# meta: id=boundary-parser  status=active  scope=src/**/parsers/*.py
# exemplar: src/users/parsers.py#parse_signup@<commit>     # mined golden instance

def parse_$NAME($RAW: $Untrusted) -> Result[$Domain, ParseError]:   # L1 shape + L2 typed return
    """Parse, don't validate: untrusted input -> trusted domain type or a typed error."""
    ...                                          # body is a HOLE — any implementation allowed

    require  returns Result[$Domain, ParseError] # L2: return type must be a Result sum type
    forbid   raise $E                            # L3: no exception may escape the parser
    forbid   return None                         #     no magic sentinels (repo rule 5)
    forbid   return -1
    where    $NAME matches /^parse_/             # value constraint on the captured name

    advise:  |                                   # L4 prose — served to agents, LLM-judged, NON-blocking
      Validate every field here; downstream code assumes the returned domain type is valid.
      Prefer accumulating field errors into ParseError over failing on the first bad field.
```

Compiles to: the `def … -> Result[…]` template + `require`/`forbid` → a Semgrep/ast-grep
rule (`pattern` + `pattern-not-inside: raise $E` + `metavariable-regex`); `advise` → agent
context + optional advisory comment. Humans review *this file* in the PR; the generated
matcher IR is never hand-edited (the Semgrep bidirectional-by-construction property).

### Direction B — Pattern-object with a thin ast-grep-style constraint block *(the "reads like config" pole / escape hatch for cross-file)*

When an invariant is relational or cross-file (layering, "DB only via repository"), the
code-template can't carry it. Direction B wraps the Report-02 pattern-object frontmatter
around an explicit `match` + `where`, and lets `where` target *import-linter* for the
cross-file class — the clean ast-grep separation of pattern from relational constraint.

```yaml
# .akron/rules/parser.moore   (pattern-object flavor)
id: boundary-parser
title: "Boundary parsers return Result, never raise past the edge"
status: active
scope: ["src/**/parsers/*.py"]
exemplar: "src/users/parsers.py#parse_signup@<commit>"

match: |                                   # code-like template (ast-grep pattern)
  def $NAME($RAW: $T) -> $RET:
      ...
where:
  - $RET     matches: "Result[$_, $_]"     # return must be a Result           (single-file)
  - not-has:  "raise $_"                    # no raise escapes                   (pattern-not-inside)
  - not-has:  "return None"
  - $NAME    regex: "^parse_"
  - layer:                                  # cross-file anchor -> compiles to import-linter
      forbid-import: {from: "src.parsers", to: "src.db.raw"}

advise: |                                   # advisory lane, non-blocking
  Parsers depend on the domain model, never on raw persistence; keep IO at the call site.
```

Trade-off vs A: more explicit and more powerful for the hard cases, but reads like config
(GritQL/ast-grep-YAML ergonomics) and is less obviously "mine me from examples." Best as the
*escape hatch* invoked when Direction A's inline `where` isn't enough.

### Direction C — Coccinelle-style semantic sketch *(niche: ordering/context + codemods)*

For order- or context-sensitive patterns ("inside a transaction, commit before returning")
and for shipping *migrations* (turn old shape → new shape), a diff-of-an-idealized-fragment
is the most natural surface — the notation the kernel has used for 15 years.

```
# .akron/rules/parser.moore   (semantic-sketch flavor)
@@
metavar name ~ /^parse_/
expr raw, value
@@
  def name(raw: $Untrusted) -> Result[$Domain, ParseError]:
      <...
-     return value                        # forbid: returning a bare, unvalidated value
+     return Ok(validate(value))          # require/fix: wrap at the boundary in a Result
      ...>
```

Trade-off: superb for sequencing and for autofix/codemods (`-`/`+` *is* the rewrite), and
the strongest adoption precedent of any surveyed tool, but it's transformation-shaped rather
than conformance-shaped, and its "diff" framing is less obvious for pure "this must always
hold" rules. Reserve for context/ordering patterns and for the migration path when a canon
changes.

### 6.1 Recommendation

Ship **Direction A as the primary surface** (Python-shaped sketch, default-free, per-anchor
strictness dial, deterministic + advisory lanes), with **Direction B's `where`/`layer`
block as the built-in escape hatch** for relational and cross-file invariants (compiling
those anchors to import-linter/Tach), and **Direction C reserved** for ordering/context
patterns and codemod-style canon migrations. Across all three: *reuse Python's syntax and
tooling, compile to existing engines, review the surface not the IR, and keep the LLM in the
advisory lane.* That is the whole design.

---

## 7. Open questions (handoff)

1. **Anti-unification quality for mining.** How reliably can a `.moore` draft be
   auto-generated by anti-unifying a cluster of real parsers (which parts become fixed vs
   `...`)? What cluster size / similarity threshold yields a *useful* skeleton rather than an
   over-general or over-specific one? (Ties to Report 02 Q1/Q2.)
2. **Surface → backend compiler coverage.** Which `.moore` anchors compile cleanly to
   ast-grep vs Semgrep vs import-linter, and what fraction of real house rules fall outside
   all three (needing CodeQL/graph)? The compiler's target-selection policy *is* Report 02's
   rule-selection policy — can it be inferred from the anchor, or must the author declare it?
3. **Round-trip integrity.** If the matcher IR is regenerated from surface, how do we detect
   when a hand-tuned backend rule diverges from its `.moore` (the "docs-as-tests" check:
   does the compiled rule still match the exemplar)?
4. **LSP scope.** Minimum viable LSP: hole/anchor highlighting + "this anchor failed on line
   N" + "unpinned hole" hints. Is a Python-grammar-derived tree-sitter overlay enough, or is
   a bespoke grammar unavoidable once `require`/`forbid` sugar is added?
5. **Advisory-lane trust.** How do we keep L4 prose comments from becoming ignorable noise
   (Report 03's gate-fatigue) — rate-limit, confidence threshold, only-on-changed-lines?
6. **Isomorphism support.** Coccinelle's isomorphisms (match `x` and `x != None` alike) are
   what make its rules robust to style. Which Python isomorphisms (kwarg order, alias
   imports — Semgrep already normalizes some) should `.moore` fold in so anchors don't
   over-flag on trivial variation?

---

## 8. Source index (verified URLs)

**Surface syntaxes / structural matchers:**
Semgrep [philosophy](https://docs.semgrep.dev/contributing/semgrep-philosophy/) ·
[rule syntax](https://semgrep.dev/docs/writing-rules/rule-syntax) ·
[pattern syntax](https://semgrep.dev/docs/writing-rules/pattern-syntax) ·
[stop grepping code](https://semgrep.dev/blog/2020/semgrep-stop-grepping-code/) ·
[static-analysis journey (generic AST)](https://semgrep.dev/blog/2021/semgrep-a-static-analysis-journey/) ·
[generic pattern matching](https://semgrep.dev/docs/writing-rules/generic-pattern-matching) ·
[repo](https://github.com/semgrep/semgrep).
ast-grep [rule-config](https://ast-grep.github.io/guide/rule-config.html) ·
[relational rules](https://ast-grep.github.io/guide/rule-config/relational-rule.html) ·
[repo](https://github.com/ast-grep/ast-grep).
Comby [basic usage](https://comby.dev/docs/basic-usage) · [FAQ](https://comby.dev/docs/faq) ·
[repo](https://github.com/comby-tools/comby).
Coccinelle [semantic patches](https://coccinelle.gitlabpages.inria.fr/website/sp.html) ·
[kernel docs](https://docs.kernel.org/dev-tools/coccinelle.html) ·
[Lawall talk (InfoQ)](https://www.infoq.com/presentations/coccinelle-linux-kernel/).
GritQL [patterns](https://docs.grit.io/language/patterns) ·
[conditions](https://docs.grit.io/language/conditions) · [repo](https://github.com/getgrit/gritql).
tree-sitter [query syntax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html).
CodeQL [Learning CodeQL](https://goingbeyondgrep.com/posts/learning-codeql/).
Rego [no one wants to write Rego](https://www.permit.io/blog/no-one-wants-to-write-rego).
LibCST [matchers tutorial](https://libcst.readthedocs.io/en/latest/matchers_tutorial.html) ·
Bowler [repo (archived)](https://github.com/facebookincubator/bowler).

**Abstraction dial (holes / typed holes / templates / types-as-patterns):**
SKETCH [notes](https://users.cs.utah.edu/~vinu/research/stencils/sketch/notes/sketch_stencils.html) ·
[Solar-Lezama thesis](https://people.csail.mit.edu/asolar/papers/thesis.pdf) ·
[sketching survey](https://people.csail.mit.edu/asolar/papers/Solar-Lezama09.pdf).
Hazel [typed holes (arXiv 1805.00155)](https://arxiv.org/abs/1805.00155) · [hazel.org](https://hazel.org/).
Idris 2 [ECOOP 2021](https://drops.dagstuhl.de/storage/00lipics/lipics-vol194-ecoop2021/LIPIcs.ECOOP.2021.9/LIPIcs.ECOOP.2021.9.pdf).
cookiecutter [overview](https://cookiecutter.readthedocs.io/en/stable/overview.html).
Python [PEP 544 Protocols](https://peps.python.org/pep-0544/) ·
[mypy protocols](https://mypy.readthedocs.io/en/stable/protocols.html).
Go [interfaces implemented implicitly](https://go.dev/tour/methods/10).
Rust [traits](https://doc.rust-lang.org/book/ch10-02-traits.html).

**Generic AST / IR:**
Semgrep generic AST (journey blog, above).
tree-sitter [home](https://tree-sitter.github.io/tree-sitter/).
difftastic [home](https://difftastic.wilfred.me.uk/) ·
[internals (DeepWiki)](https://deepwiki.com/Wilfred/difftastic/1-overview).
Babelfish [bblfsh org (archived)](https://github.com/bblfsh) ·
source{d} [src-d org](https://github.com/src-d) ·
[go-git HISTORY (bankruptcy late 2019)](https://github.com/go-git/go-git/blob/master/HISTORY.md).
Sourcegraph [SCIP announcement](https://sourcegraph.com/blog/announcing-scip).

**LLM-lint / hybrid boundary:**
[Semgrep Assistant tech](https://semgrep.dev/blog/2024/the-tech-behind-semgrep-assistant/) ·
[Semgrep multimodal](https://semgrep.dev/products/semgrep-multimodal/).
CodeRabbit [ast-grep + LLM](https://www.coderabbit.ai/blog/ai-native-universal-linter-ast-grep-llm) ·
[learnings](https://docs.coderabbit.ai/knowledge-base/learnings).
[G-Research: LLM review patterns](https://www.gresearch.com/news/building-a-code-review-tool-the-llm-patterns-that-actually-work/).
[Semgrep vs CodeQL (Doyensec)](https://blog.doyensec.com/2022/10/06/semgrep-codeql.html).

**New-language economics / tooling:**
[Configuration Complexity Clock (Hadlow)](http://mikehadlow.blogspot.com/2012/05/configuration-complexity-clock.html) ·
[every simple language → Turing complete](https://solutionspace.blog/2021/12/04/every-simple-language-will-eventually-end-up-turing-complete/) ·
[Pkl Goldilocks (Starlark/Dhall/Jsonnet scorecard)](https://medium.com/kurtosis-tech/pkl-and-the-goldilocks-problem-of-configuration-languages-dc36621e102a) ·
[tree-sitter vs LSP as table stakes](https://lambdaland.org/posts/2026-01-21_tree-sitter_vs_lsp/) ·
[Starlark (Bazel)](https://bazel.build/rules/language).

**Flagged *unverified* (appeared in search, not independently fetched — re-verify before quoting):**
Coccinelle "~59 in-tree patches / 29–32 % of veteran committers" figures
([USENIX ATC18 Lawall](https://www.usenix.org/system/files/conference/atc18/atc18-lawall.pdf), 403 on fetch);
LintCFG (arXiv 2602.07783); "LLMs Lean on Priors" (arXiv 2510.03415); LLM-FP-reduction accuracy
(arXiv 2601.18844); GitHub star counts (approximate, as-of-fetch).
