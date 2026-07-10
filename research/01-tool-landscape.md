# Archon / Akron — Tool Landscape Research

**Report 01 · Problem-space & prior-art survey · compiled 2026-07-01**

Scope: verify or refute the internal brief's claim that an integrated system of
"**ADRs + pattern canon + blocking agent hooks + ast-grep structural rules**" is
**not shipped end-to-end by anyone** as of mid-2026. Method: seven parallel
web-research sweeps plus first-hand inspection of two anchor competitors and the
live `codebase-memory-mcp` MCP server. Only URLs seen in search results or
successfully fetched are cited; unverifiable figures are flagged *unverified*.

---

## 1. Executive summary

### 1.1 Verdict on the "nobody ships this end-to-end" whitespace claim

**Partly false, and more urgent than the brief assumes.** The *core* of the pitch
— compiling **Architecture Decision Records into deterministic checks that gate
AI coding agents (Claude Code / Cursor / Copilot) before or around code
generation** — is already shipped by at least **three distinct projects**, with a
fourth (advisory) already at ~2.1k stars:

- **Mneme HQ** (`mnemehq.com`, repo `MnemeHQ/mneme`, MIT, v0.4.2 May 2026) —
  "Enforces architectural intent for AI coding agents." Compiles a versioned
  corpus of ADR markdown into a *deterministic* active constraint set (keyword
  scoring, **no LLM/embeddings**), injects constraints as a system prompt before
  generation, evaluates output pre-submission, and **blocks (ALLOW/BLOCK verdict)
  forcing a retry**. Native Claude Code hook + slash commands and Cursor;
  designed-to-support Copilot/Aider/Cline/OpenHands. *Explicitly no AST/structural
  rules.*
- **Archgate** (`archgate.dev` / `github.com/archgate/cli`, Apache-2.0, ~48★,
  v0.45.7 Jun 2026, 83 releases) — "Make AI Coding Tools Follow Your Rules." ADRs
  live as markdown+YAML in `.archgate/adrs/`, each optionally paired with an
  **executable TypeScript `.rules.ts` check**; `archgate check` runs in CLI,
  pre-commit, and CI and **exits non-zero to block merges**, while agents read
  ADRs (compressed to ~20-line briefings) before generating. Markets precisely on
  enforcement: *"Archgate rules are enforced… Cursor rules are suggestions."*
  Native Claude Code + Cursor plugins and a VS Code→Copilot extension.
- **adr-kit** (`github.com/rvdbreemen/adr-kit`, MIT, v0.30.5, ~3★, early) — drop-in
  toolkit adding a JSON `## Enforcement` block (`forbid_pattern` / `forbid_import`
  / `require_pattern`, glob-scoped) to Nygard ADRs, **enforced/blocking at
  pre-commit + CI** (with an audited override), plus a 4-tool **MCP server** and
  installers for Claude Code/Cursor/Copilot/Codex.
- **ArcKit** (`github.com/tractorjuice/arc-kit`, MIT, **~2,100★**, NHS / Cabinet
  Office reference deployments) — the most *adopted* EA-governance harness (MADR
  ADRs, principles, traceability), but **advisory only**: its hooks inject context
  and auto-correct, they do not hard-block.

So the headline framing ("**nobody** ships ADRs → agent enforcement") does **not**
survive contact with mid-2026 reality. Adjacent pieces are also shipping:
**CodeRabbit** already combines ast-grep custom rules + a learned "Learnings"
memory + PR-blocking Pre-Merge Checks; **tsarch wired into a Claude Code Stop
hook** already does true architectural (layering/dependency-direction) gating via
a regenerate-until-clean loop; **Sonargraph** shipped an MCP server (June 2026) so
agents query its enforced architecture model and emit compliant code first-try;
and **codebase-memory-mcp** (the DeusData server, ironically the one wired into
this very session) already exposes a first-class `manage_adr` primitive.

### 1.2 What is genuinely still unoccupied (the real, narrower whitespace)

No single tool integrates **all** of the following into one coherent system, and
three of these sub-capabilities appear to be shipped by **no one**:

1. **A ratified positive "pattern canon" — the reusable house patterns an agent
   should DISCOVER and REUSE.** Every enforcement tool surveyed encodes *negative*
   constraints (forbidden dependencies/imports) or *learned soft preferences*
   (CodeRabbit/Greptile). None ships a curated, versioned library of *blessed
   positive patterns* ("use `Result[T]` here; the retry helper is `x`") that an
   agent queries before writing, to prevent **helper/util reinvention**. This is
   the strongest unmet need and maps directly to the brief's "AI agents reinvent
   helpers" pain. **Shipped by: nobody.**
2. **Binding a codebase knowledge-graph ("theory") to ratified decisions** so the
   system can answer *"does this diff violate ADR-007?"* codebase-memory-mcp
   stores ADRs *and* an architecture graph but never checks code against the ADRs;
   fitness functions check structure but carry no intent/ADR link. The two are
   never joined. **Shipped by: nobody.**
3. **A pre-edit HARD VETO on architectural-pattern conformance.** Deterministic
   pre-execution blocking of agent tool calls is mature and ubiquitous — but only
   on **tool-name / command / path / secret / dependency** patterns. Architectural
   conformance gating exists *only* as a *post-generation* feedback loop
   (tsarch+Stop hook, Semgrep Guardian). A true pre-edit veto on "this code
   violates the layering pattern" is **shipped by: nobody**.
4. **The full four-pillar stack in one product** (ratified ADRs + positive pattern
   canon + structural/AST rules + agent gating + codebase-theory graph). Mneme and
   Archgate cover pillars 1+3 for *negative* rules; ast-grep/Semgrep/CodeRabbit
   cover the structural pillar in isolation; codebase-memory-mcp covers the graph
   pillar in isolation. **Nobody unifies them.**

### 1.3 Strategic implications

- **The claim needs rewording.** "Nobody does ADRs + blocking hooks for agents" is
  wrong; "**nobody unifies a positive pattern-canon + graph-bound ADR enforcement +
  structural rules + a pre-edit veto, and specifically nobody solves helper-
  reinvention via a discoverable ratified pattern library**" is defensible.
- **The space is hot and weeks-old.** Mneme (16★) and Archgate are pre-1.0 and
  landed in the first half of 2026. This is validation *and* a race — first-mover
  advantage is thin and eroding.
- **Name "Archon" is a serious collision** (see §11): `coleam00/Archon` is a
  22.7k-star open-source AI-coding harness in the *exact adjacent space*, and
  `pytest-archon` sits in the *exact architecture-rule space*. Strongly reconsider.

---

## 2. Category 1a — Architecture fitness functions (library / OSS)

Language-level "write architecture rules as tests / config, fail CI on violation."
**Cross-cutting finding:** none is AI-agent-aware in any first-class sense (no MCP,
no LLM rule-gen, no agent integration beyond "invoke the CLI, react to pass/fail").
Uniformly they encode *what* is structurally permitted but never the *why*, and
none helps an agent *discover* house patterns before writing — they only reject
after the fact.

| Tool | Rules expressed as | Enforce/Advise | AI-aware | Maturity (mid-2026) | Key gap |
|------|--------------------|----------------|----------|---------------------|---------|
| **ArchUnit** (Java) | Fluent-Java rules inside JUnit tests | Enforce (test fails CI) | None | 3.8k★, TNG, v1.4.2 (Apr 2026), active | No rationale/intent store |
| **NetArchTest** (.NET) | C# fluent API in unit tests | Enforce | None | 1.8k★, core stale (v1.3.2 2021); active fork "eNhancedEdition" | Unmaintained core |
| **ts-arch** (TS) | Jest `toPassAsync()`; slice rules vs a PlantUML diagram | Enforce | None | 649★, v5.4.1 (Dec 2024) | Structure only |
| **pytest-archon** (Py) | `archrule(...).should_not_import(...)` in pytest | Enforce | None | 86★, author **jwbargsten**; active | **Name-root collision; NOT the Archon AI project** |
| **import-linter** (Py) | TOML/INI "contracts" (`layers`/`forbidden`/`independence`) | Enforce (`lint-imports` CLI) | None | 1.1k★, **seddonym**, v2.12 (Jun 2026) | No rationale; no agent surface |
| **Tach** (gauge-sh) | `tach.toml` `modules`/`depends_on`/`layers`; TUI | Enforce (`tach check`) | None found | 2.8k★, Rust, v0.35.0 (May 2026) | Boundaries not intent |
| **dependency-cruiser** (JS/TS) | `.dependency-cruiser.js` `forbidden`/`allowed` + `severity` | Enforce + visualize | None | 6.8k★, v18.0.0 (Jun 2026) | Prohibitions not reasoning |
| **eslint-plugin-boundaries** | ESLint `elements` + `dependencies` rules | Enforce (lint error) | None | 920★, v6.0.2 (Mar 2026) | Layers only |

URLs: https://github.com/TNG/ArchUnit · https://www.archunit.org/ ·
https://github.com/BenMorris/NetArchTest ·
https://github.com/NeVeSpl/NetArchTest.eNhancedEdition ·
https://github.com/ts-arch/ts-arch · https://github.com/jwbargsten/pytest-archon ·
https://pypi.org/project/pytest-archon/ · https://github.com/seddonym/import-linter ·
https://github.com/tach-org/tach · https://docs.gauge.sh/ ·
https://github.com/sverweij/dependency-cruiser ·
https://github.com/javierbrea/eslint-plugin-boundaries

---

## 3. Category 1b — Commercial architecture analysis + structural pattern engines

| Tool | Rules as | Enforce/Advise | AI-aware | Maturity | Key gap |
|------|----------|----------------|----------|----------|---------|
| **Sonargraph** (hello2morrow) | Architecture DSL ("UML-in-text", whitelist) | Enforce (breaks CI build) | **Yes — Sonargraph MCP (Jun 2026)**, agents query model & emit compliant code | Mature; free Explorer + paid Architect | MCP/enforcement **Java + file-model only**; intent in DSL, not ratified decision |
| **Structure101 / Studio** | Visual DSM diagrams + lock/`Enforce` | Enforce (locked diagrams) | Emerging via Sonar platform | **Acquired by Sonar Oct 2024; standalone being retired**, folding into SonarQube | In transition; visual-bound; no agent path |
| **CodeScene** | VCS-history behavioral analysis (hotspots, Code Health); Quality Gates | Both — advisory + **PR/merge gate blocks** on Code Health drop | **Yes — CodeHealth MCP + ACE auto-refactor** | Mature; free CE, €18–27/author/mo | **No declarative layering/dependency rules**; can't encode ratified intent |
| **Semgrep** | **YAML patterns that look like code** (`pattern`/`pattern-not`/metavars/`...`) | Enforce (CI-blocking) | Yes — Semgrep Assistant (AI triage/fix) | Very mature, large rule corpus | Syntactic rules, not a first-class architecture model; no ratified-intent registry |
| **ast-grep** | **YAML structural rules** (`inside`/`has`/`follows`, `all`/`any`/`not`, `constraints`, `fix`) | Enforce (`scan` exits non-zero on `error`) | **Yes — ast-grep MCP (4 tools) + documented "AI drafts a rule" workflow** | ~14.9k★, Rust, v0.44.0 (Jun 2026) | Per-pattern linter; no ratified/versioned decision, no house-pattern library served to agents |

`ast-grep` and Semgrep are the **structural-rule pillar** the brief wants — both
can express "this pattern must/must not appear here" and block CI, and ast-grep
even ships an MCP server plus an AI-rule-drafting loop. Neither has any notion of a
*ratified architectural decision* or a curated positive-pattern canon, and neither
gates an agent *before* the edit on architectural grounds.

URLs: https://www.hello2morrow.com/products/sonargraph ·
https://blog.hello2morrow.com/2026/06/sonargraph-mcp/ ·
https://www.sonarsource.com/structure101/ ·
https://www.sonarsource.com/company/press-releases/sonar-acquires-structure101-to-strengthen-code-quality-offering/ ·
https://codescene.com/product/behavioral-code-analysis ·
https://codescene.com/product/code-health-mcp ·
https://semgrep.dev/docs/writing-rules/rule-syntax ·
https://semgrep.dev/docs/semgrep-assistant/overview ·
https://github.com/ast-grep/ast-grep · https://github.com/ast-grep/ast-grep-mcp ·
https://ast-grep.github.io/blog/ast-grep-agent.html ·
https://ast-grep.github.io/guide/scan-project.html

---

## 4. Category 2 — Codebase-memory / context engines for AI agents

Representation is dominated by **raw structure** (tree-sitter ASTs, LSP/SCIP symbol
facts, name-binding stack graphs, vector embeddings). Almost all are **advisory
retrieval**, not enforcement. On capturing a codebase's *ratified* "theory":

| Tool | Representation | Advise/Enforce | AI integration | Maturity | Ratified patterns? |
|------|----------------|----------------|----------------|----------|---------------------|
| **codebase-memory-mcp** (DeusData) | Tree-sitter (158 langs) + custom C hybrid-LSP + Nomic embeddings → persistent KG | Advisory | MCP server; auto-configs ~11 agents | MIT, v0.8.1 (Jun 2026); ~23k★ *unverified*; preprint arXiv:2603.27277 | **Partial — unique `manage_adr` + `get_architecture`**, but stores decisions, doesn't check code vs them |
| **Serena** (oraios) | LSP symbol model (30+ langs), no vectors; markdown "memories" | Advisory **+ edits code** | MCP + Agno; many agents | MIT, ~26k★ *unverified* | Weak — freeform memory prose the LLM may ignore |
| **Sourcegraph Cody / Deep Search** | Code search index + SCIP + RAG (≤1M ctx) | Advisory retrieval | Enterprise IDE assistant | Mature (decade of code search) | No |
| **Augment Code — Context Engine** | Semantic/vector index (100k+ files) + lineage | Advisory retrieval | **Context Engine MCP** for any agent | Commercial GA | *Claims* "patterns/conventions" but as retrieval, not ratified artifact |
| **Greptile** | Full repo graph (fns/classes/imports/deps) + git history | Advisory but **operationally gating (PR bot)** | GitHub/GitLab review agent | YC, commercial | **Closest — checks "pattern consistency," learns conventions**, flags deviations; but learned-from-code, advisory |
| **GitHub stack graphs / SCIP** | Name-binding graph (`.tsg` DSL) / symbol-fact protocol | Advisory nav | Infra/format | **stack-graphs archived Sep 2025**; **SCIP active** (45k+ repos) | No |
| **Glean — Meta OSS** (`facebookincubator/Glean`) | Schema-defined code *facts* + **Angle** (Datalog) queries | Advisory queries | Backend fact store (not MCP/agent) | Mature at Meta | No |
| **GitNexus** (abhigyanpatwari) | Tree-sitter → Leiden clusters → BM25+vector (LadybugDB) | Advisory | **MCP (7 tools) + Claude Code skills & Pre/Post hooks** | New (~Apr 2026), PolyForm-NC; star counts inflated/*unverified* | Weak — auto-derived area maps, not ratified rules |
| **CodeGraphContext** | Tree-sitter (23 langs) → graph DB, no embeddings | Advisory | MCP + CLI | MIT, v0.5.1, ~3.8k★ *unverified* | No — explicitly structure only |

**The "theory of the codebase" gap:** Only `codebase-memory-mcp` ships a
first-class ratified-decision primitive (`manage_adr`), but it does **not** bind
those ADRs to the graph to answer "does this change violate decision X?". Greptile
is the only one that *acts* on conventions, but they are learned-from-code and
advisory. **No surveyed tool binds a declared/ratified convention to the code graph
and enforces it.**

> **Disambiguation — two "Glean"s:** `facebookincubator/Glean` (Meta OSS code-fact
> indexer, Angle query language) is unrelated to **Glean Technologies** (`glean.com`,
> the enterprise Work-AI search company, ~$7.2B val *unverified*).

URLs: https://github.com/DeusData/codebase-memory-mcp ·
https://deusdata.github.io/codebase-memory-mcp/ · https://arxiv.org/abs/2603.27277 ·
https://github.com/oraios/serena · https://sourcegraph.com/docs/cody ·
https://www.augmentcode.com/context-engine ·
https://www.augmentcode.com/blog/context-engine-mcp-now-live ·
https://www.greptile.com/docs/how-greptile-works/graph-based-codebase-context ·
https://github.blog/open-source/introducing-stack-graphs/ ·
https://github.com/sourcegraph/scip · https://github.com/facebookincubator/Glean ·
https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/ ·
https://www.glean.com/ · https://github.com/abhigyanpatwari/GitNexus ·
https://github.com/CodeGraphContext/CodeGraphContext

---

## 5. Category 3 — AI code review products (house-pattern awareness)

Do any *learn/enforce* a repo's house patterns vs generic lint?

| Tool | Learns house patterns? | Advise/Block | Arch-consistency claim | Maturity | Key gap |
|------|------------------------|--------------|------------------------|----------|---------|
| **CodeRabbit** | **Yes** — NL "Learnings" memory + path rules + **ast-grep custom rules** (`.coderabbit.yaml`) | Both — **Pre-Merge Checks "error" mode blocks PR** | Partial | $60M B (Sep 2025), ~$550M val, 2M+ repos | Learnings = "incremental preferences, not formal standards"; reactive; no anti-reinvention guard |
| **Greptile** | Yes — learns from PR comments/reactions/merges; auto-indexes CLAUDE.md/AGENTS.md | Advisory (blocking *unverified*) | **Yes — codebase graph** | $25M A / Benchmark, $180M val, v4 (Mar 2026) | Conventions stay advisory; inferred not ratified |
| **Ellipsis** | Yes — infers from PR history + NL rules | Advisory (blocking *unverified*); GitHub-only | None | YC W24, small | No arch claim; narrow platform |
| **Sourcery** | Weak — `.sourcery.yaml` config rules, no PR-learning | Advisory only ("no merge blocking") | None | 300k+ devs, SOC2 | No memory, no gating |
| **Codacy** | Config-based, not learned | **Blocking** quality gates | No (metrics) | Mature, on-prem | Doesn't learn semantic patterns |
| **qlty** (ex-Code Climate) | No — deterministic lint; AI autofix only | Blocking metric gates | No | CLI free, Cloud paid | Not a pattern reviewer |
| **Graphite Agent** (ex-Diamond) | Partial — **import your own style guide** (static) | Advisory | Partial | $52M B / Accel; Shopify, Figma | Static guide, no PR-learning; arch left to humans by design |
| **Macroscope** (`macroscope.com`) | No — AST bug-graph + Linear/Jira "why" | Advisory | No | Launched 2025 | Ticket-context focus, no house patterns |
| **Claude Code Code Review** (Anthropic) | Config, not learned — reads `CLAUDE.md`/`REVIEW.md` | **Advisory — neutral check, never blocks** (build your own gate from severity JSON) | No (correctness default) | Research preview | Static human-authored rules; won't auto-learn patterns or stop helper reinvention |

Takeaway: **CodeRabbit is the closest shipped example** of "learn + structurally
enforce (ast-grep) + block PR" — but its learned conventions are explicitly soft
preferences, it is diff-reactive, and it does nothing to *proactively* stop an
agent from reinventing an existing helper before the code is written.

URLs: https://docs.coderabbit.ai/knowledge-base/learnings ·
https://www.coderabbit.ai/blog/pre-merge-checks-built-in-and-custom-pr-enforced ·
https://www.greptile.com/ · https://www.ellipsis.dev/docs/features/code-review ·
https://www.sourcery.ai/ · https://www.codacy.com/quality · https://qlty.sh/ ·
https://graphite.com/blog/series-b-diamond-launch · https://macroscope.com/ai-code-review ·
https://code.claude.com/docs/en/code-review

---

## 6. Category 4 — Convention / spec systems for agents

Almost all are **advisory context injected into the prompt**, not tooling-enforced.

| System | Format | Advisory or Binding | Adoption | Key gap |
|--------|--------|---------------------|----------|---------|
| **CLAUDE.md** | Hierarchical markdown, auto-loaded | Advisory ("context, not enforced config") | Default in Claude Code | Degrades with length/compaction |
| **AGENTS.md** | Single repo-root markdown | Advisory ("user prompts override everything") | **60k+ repos, 20+ tools; Linux Foundation Agentic AI Foundation** | No schema, no enforcement, no precedence |
| **Cursor rules** | `.mdc` (YAML frontmatter + md), 4 modes | Advisory; **Team/Enterprise "enforced" rules** the exception | Standard in Cursor | Individual rules still probabilistic |
| **OpenSpec** (Fission-AI) | Per-change proposal/spec/tasks md | Advisory ("fluid not rigid") | 25+ assistants | Nothing forces code to match spec |
| **GitHub Spec Kit** | `constitution→specify→plan→tasks→implement` | Advisory ("not enforcing compliance") | 30+ agents, GitHub-backed | "Executable spec" oversells; constitution is soft |
| **Kiro specs** (AWS) | `requirements`/`design`/`tasks` + approval gates | Gates the **human loop**, not token-level | Bundled in Kiro | Gates phases, not adherence |
| **Ruler** (intellectronica) | `.ruler/` → distributes to each agent config | Advisory distributor ("doesn't enforce compliance") | 30+ agents | Only syncs files |

### 6.1 Known failure modes — why advisory rules don't bind (key justification)

- **Rules are probabilistic, resolved by sampling not precedence** — agents "have
  no native means to obey constraints as constraints"
  (https://arxiv.org/pdf/2605.18672).
- **Governance decay under compaction (strongest quantified evidence):** in-context
  policy violation rises from **0% → ~30% avg (up to 59%)** after a *single*
  history compaction; soft org-policies decay ~50pp vs ~6pp for hard safety norms
  (8.3× worse) because summarizers evict standing policy as low-salience.
  "Constraint pinning" restores 0% at <0.5% overhead
  (https://arxiv.org/html/2606.22528).
- **Context rot:** all 18 frontier models tested degrade as input grows; effective
  context ≈ 50–65% of advertised (https://www.trychroma.com/research/context-rot).
- **Constraint drift** in long-horizon/multi-agent runs — "constraints appear in
  prompts but no longer govern actual actions" (https://arxiv.org/html/2605.10481).
- **Practitioner reports:** "200 lines of rules… it ignored them all"; "forgets its
  rules every 45 minutes"; more rules → fewer followed
  (https://dev.to/minatoplanb/i-wrote-200-lines-of-rules-for-claude-code-it-ignored-them-all-4639,
  https://dev.to/douglasrw/your-ai-agent-forgets-its-rules-every-45-minutes-heres-the-fix-151e).

**Implication for Archon/Akron:** enforcement, if wanted, must live in the
harness/tooling layer — the rules-file layer is inherently advisory. This is the
central technical argument *for* the product and *against* relying on CLAUDE.md /
AGENTS.md alone.

URLs: https://agents.md/ · https://openai.com/index/agentic-ai-foundation/ ·
https://cursor.com/docs/rules · https://github.com/Fission-AI/OpenSpec ·
https://github.com/github/spec-kit · https://kiro.dev/docs/specs/ ·
https://github.com/intellectronica/ruler · https://code.claude.com/docs/en/memory

---

## 7. Category 5 — Agent gating / policy hooks

Deterministic pre-execution interception of agent tool calls is **mature and
ubiquitous**, but gates only on **tool-name / command / path / secret / dependency**
patterns — never architecture.

| Mechanism | Hard-block? | Policy as | Arch-aware? | Maturity | Gap |
|-----------|-------------|-----------|-------------|----------|-----|
| **Claude Code PreToolUse hook** | **Yes** (pre-exec; exit 2 or JSON `deny`) | Shell + JSON; `if` rules; regex matchers | No | GA, ubiquitous | Sees one call's args, not repo structure |
| **Cursor agent hooks** (`beforeShellExecution` etc.) | **Yes** (`failClosed` for fail-shut) | JSON + script → allow/deny/ask | No | Beta (v1.7, Oct 2025) | Command/path/secret only |
| **Agent SDK `canUseTool`** | Yes (only calls not auto-approved) | Arbitrary code + `updatedInput` | No (unless you build it) | GA | Auto-approved tools skip it |
| **SDK deny rules / `disallowed_tools`** | Yes (deny wins even in bypass) | Declarative globs | No | GA | Pattern match only |
| **Cycode AI Guardrails** | Yes ("Block mode") | Credential regex + path | No | Product, Feb 2026 | Secrets/egress only |
| **Semgrep Guardian** | Feedback loop (regenerate-until-clean) | Semgrep SAST DSL | Partial (**security**, not layering) | Product, 2026 (Cursor marketplace) | Post-edit, security not architecture |
| **Endor Labs (via Cursor hooks)** | Yes (blocks bad dep installs) | Supply-chain policy | No | Product, 2026 | Dependency scope |
| **tsarch + Stop hook** | **Soft-block via feedback loop** | Vitest tests over TS dep graph | **Yes — layering/dependency direction** | OSS lib + documented pattern | **Post-generation, not a pre-edit veto**; relies on naming |
| **AgentSpec** (research) | Yes (prototype) | DSL triggers/predicates | Configurable | Research (arXiv 2503.18666) | Not a product |

**Key finding:** *No shipping product deterministically blocks an agent edit
**before it happens** on architectural-pattern conformance.* The closest is
**tsarch wired into a Claude Code Stop hook** (ANGULARarchitects) — genuine
layering/dependency gating, but a *post-generation* regenerate-until-clean loop, an
OSS pattern not a product, and naming-convention-dependent.

URLs: https://code.claude.com/docs/en/hooks · https://cursor.com/docs/hooks ·
https://cursor.com/blog/hooks-partners ·
https://code.claude.com/docs/en/agent-sdk/permissions ·
https://cycode.com/blog/ai-guardrails-real-time-ide-security/ ·
https://semgrep.dev/blog/2026/introducing-semgrep-guardian-real-time-security-for-ai-written-code/ ·
https://www.angulararchitects.io/en/blog/architecture-beyond-layers-tsarch-for-ai-agents/ ·
https://aipatternbook.com/architecture-fitness-function · https://arxiv.org/abs/2503.18666

---

## 8. Category 6 — Golden-path / paved-road platforms (prior art)

Do IDPs / scorecards check *code-level pattern conformance*? **No** — the deepest
any reach into code is **regex/glob string-matching over file contents**; the rest
is metadata, ownership, catalog, CI status, and scaffolding. "Standard" = a
check/rule over facts → a maturity level/score → advisory pressure, not a hard gate.

| Platform | Checks defined as | Checks CODE patterns? | Block/Advise | AI-aware | Maturity |
|----------|-------------------|-----------------------|--------------|----------|----------|
| **Backstage Software Templates** | Parameterized code skeletons | No — scaffolds at creation, no post-check | Advisory | Community add-ons | Mature OSS standard |
| **Backstage Tech Insights / Scorecards** | Facts + Checks → scorecard | Mostly metadata; custom fact-retrievers *could* ingest code facts | Advisory viz | No first-party AI | Mature plugin |
| **Spotify "Golden Paths"** | Concept/practice + templates | No (cultural) | Advisory | No | Foundational concept |
| **Cortex** | Scorecard rules (CQL) across levels | Shallow — `git.fileContents()` regex only | Level-gated scoring | **MCP, Eng Intelligence, AI-readiness** | Mature IDP |
| **OpsLevel** | Rubric checks → bronze→gold | **Deepest — Repo Grep (regex+glob)** but still lexical | Scoring/advisory | **MCP server** | Mature IDP |
| **Port** | Rules over **entity properties** | No (docs: "only entity properties") | Advisory + gate self-service actions | **AI self-heal (scorecard→issue→PR)** | Mature IDP |

**Gap:** across all five, "conformance" stops at metadata / ownership / scaffolding
/ CI status, and at best lexical grep. The white space none occupy is **semantic
verification that a service's code embodies a ratified architectural pattern**, and
their 2026 AI features improve *querying/remediating metadata*, not *judging
architecture*.

URLs: https://backstage.io/docs/features/software-templates/ ·
https://github.com/backstage/community-plugins/blob/main/workspaces/tech-insights/plugins/tech-insights/README.md ·
https://engineering.atspotify.com/2020/08/how-we-use-golden-paths-to-solve-fragmentation-in-our-software-ecosystem ·
https://www.cortex.io/products/scorecard · https://docs.cortex.io/solutions/ai-readiness/configure ·
https://docs.opslevel.com/docs/repo-grep-checks · https://github.com/OpsLevel/service-maturity-library ·
https://docs.port.io/promote-scorecards/ · https://docs.port.io/guides/all/self-heal-scorecards-with-ai/

---

## 9. Direct competitors — ADR / pattern enforcement for AI agents (the crux)

These are the tools that most directly occupy Archon/Akron's intended position.
The two anchors (Mneme, Archgate) were **independently fetched and verified**; the
rest come from a dedicated competitor sweep with verified repos.

| Product | What it does | Decisions expressed as | Block or advise | Agent integrations | Structural/AST rules | Maturity | ADR·canon·hooks·ast-grep |
|---------|--------------|------------------------|-----------------|--------------------|----------------------|----------|--------------------------|
| **Archgate** (`archgate.dev`, `archgate/cli`) | ADRs govern humans + agents; `archgate check` in CLI/pre-commit/CI blocks; agents read ADRs before writing | Markdown+YAML ADRs in `.archgate/adrs/` + **executable TypeScript `.rules.ts`** checks | **Blocks** (exit 1; pre-commit + CI) | Claude Code + Cursor plugins, VS Code→Copilot; no MCP | **No — bespoke TS, not AST** | Apache-2.0, ~48★, v0.45.7 (Jun 2026), 83 releases, active | ✔·✘·✔·✘ (**~3/4**) |
| **Mneme HQ** (`mnemehq.com`, `MnemeHQ/mneme`) | Compiles ADRs → deterministic rule graph; injects pre-gen; evaluates output; ALLOW/BLOCK before diff lands | ADR markdown w/ `## Constraints` → `project_memory.json` (keyword/tag-scoped) | **Blocks (deterministic, no LLM)** pre-generation | Claude Code, Cursor native; Copilot/Aider/Cline/OpenHands + LangGraph/CrewAI/AutoGen | **No (explicitly rejects AST/vector/ML)** | MIT, v0.4.2 (May 2026), 16★, 518 commits | ✔·✘·✔·✘ (**~2.5/4**) |
| **adr-kit** (`rvdbreemen/adr-kit`) | Drop-in ADR toolkit → commit/CI guardrails + in-flight nudges | Nygard ADRs + JSON `## Enforcement` (`forbid_pattern`/`forbid_import`/`require_pattern`, glob) | **Blocks** (pre-commit + CI; audited override); nudges advisory | Claude Code/Cursor/Copilot/Codex + **4-tool MCP server** | **No — regex/glob, not AST** | MIT, v0.30.5, ~3★, pre-1.0 | ✔·✘·✔·✘ (**~2/4**) |
| **ArcKit** (`tractorjuice/arc-kit`) | Enterprise EA-governance harness (strategy→assurance) driving AI assistants | MADR v4.0 ADRs, principles, traceability matrices | **Advises only** (context-inject + auto-correct hooks; no hard block) | Claude Code, Gemini CLI, Copilot, Codex/OpenCode + bundled MCP | No | MIT, **~2,100★**, prod refs (NHS, Cabinet Office) | ✔·✘·✘·✘ (**~1.5/4**) |
| **Qodo Merge** (`qodo.ai`) | Funded AI code-review with custom rules engine + architectural-drift detection | Custom rules (config) | Enforces at **PR-review** time (not pre-gen on the agent) | GitHub/GitLab review | No (not ADR/canon-based) | Commercial, funded | code-review, not ADR enforcement |
| **AgDR** (`me2resh/agent-decision-record`) | "Agent Decision Record" standard/spec + pre-commit enforcement | Spec format | Blocks via pre-commit (DIY) | Generic | No | Spec, not a product | ✔ (spec only) |
| **Daniel Vaughan / Codex KB** (`codex.danielvaughan.com`) | Blog/book pattern: Codex CLI ADR gen + PostToolUse hooks + CI fitness functions | MADR + AGENTS.md | Advises + DIY blocking | Codex CLI (OpenAI) | No | **Instructional content, not a product** | concept only |

**Assessment.** The ADR→agent-enforcement core is **shipped by at least three
projects** (Archgate, Mneme, adr-kit — all doing "ratified ADRs + deterministic
pre-commit/CI/pre-gen blocking + Claude Code/Cursor/Copilot integration"), with
ArcKit adding a mature advisory variant at ~2.1k stars. **Archgate is the nearest
single neighbor** to the full thesis. But a decisive detail emerged: **not one
competitor uses ast-grep/AST structural matching** — they all express rules as
bespoke TypeScript checks (Archgate), regex/glob (adr-kit), or LLM/keyword-eval
(Mneme) — and **not one ships a first-class "ratified pattern canon"** distinct
from the ADR set. So the honest verdict is *not* "nobody does this" but: **several
players already ship ADR-enforcement-with-blocking-hooks for agents; the open
whitespace is the specific fusion of a positive pattern canon + ast-grep structural
rules under one blocking layer, bound to a codebase graph — and it is narrow and
closing fast.**

URLs: https://archgate.dev/ · https://github.com/archgate/cli · https://mnemehq.com/ ·
https://github.com/TheoV823/mneme (→ `MnemeHQ/mneme`) · https://pypi.org/project/mneme-hq/ ·
https://github.com/rvdbreemen/adr-kit · https://github.com/tractorjuice/arc-kit ·
https://www.qodo.ai/blog/best-ai-code-review-tools-2026/ ·
https://github.com/me2resh/agent-decision-record · https://ast-grep.github.io/ ·
https://github.com/ast-grep/ast-grep-mcp ·
https://codex.danielvaughan.com/2026/04/28/codex-cli-architecture-decision-records-adr-automated-governance/

> *Search-summary leads, not independently fetched:* arXiv "Architectural Design
> Decisions in AI Agent Harnesses" (2604.18071); "Lore: Git Commit Messages as a
> Structured Knowledge Protocol for AI Coding Agents" (2603.15566); Dave Patten,
> "Using AI Agents to Enforce Architectural Standards" (Medium).

---

## 10. Gaps & opportunities — what no tool does

1. **Positive "pattern canon" + anti-reinvention.** Nobody ships a curated,
   versioned library of *blessed positive patterns* that an agent queries *before*
   writing to reuse the existing helper instead of inventing a new one. All
   enforcement is negative (forbidden deps) or soft-learned (CodeRabbit/Greptile).
   **This is the single strongest wedge** and maps 1:1 to the brief's core pain.
2. **Graph-bound ADR enforcement.** No tool joins a ratified-decision store to a
   code knowledge graph to answer "does this diff violate ADR-007?".
   codebase-memory-mcp has both halves (`manage_adr` + `get_architecture`) but
   never connects them — a concrete, buildable gap.
3. **Pre-edit hard veto on architectural conformance.** Only *post-generation*
   feedback loops exist (tsarch+Stop hook, Semgrep Guardian). A true pre-edit
   PreToolUse-style veto keyed on architectural-pattern conformance is unshipped.
4. **Unifying the four pillars.** ADRs (Mneme/Archgate) + positive canon (nobody) +
   structural/AST rules (ast-grep/Semgrep/CodeRabbit) + codebase-theory graph
   (codebase-memory-mcp) + pre-edit gating (hooks) exist as *separate* pieces; no
   product composes them.
5. **Compaction-resilient enforcement.** Given the governance-decay evidence (§6.1),
   a tool that re-flushes/pins ratified constraints deterministically across
   compaction (vs relying on CLAUDE.md) is both defensible and under-served —
   Mneme's "no session amnesia" JSON store is the only shipped nod to this.
6. **Review "does this belong here" judgment tied to ratified intent.** AI reviewers
   flag deviations from *learned* patterns; none reviews against a *declared*
   architectural intent registry.

**Recommended positioning:** lead with **the positive pattern canon +
graph-bound enforcement + anti-reinvention**, treat ADR-injection + agent hooks as
*table stakes now owned by Mneme / Archgate / adr-kit*, and — since **no competitor
uses AST structural matching** (all use TypeScript checks, regex/glob, or
keyword-eval) — integrate **ast-grep** as the structural-rule engine to hold a real
differentiator rather than reinventing it.

---

## 11. Name-collision findings (Archon / Akron)

**"Archon" is heavily overloaded, including two high-risk collisions in the exact
target space. Strongly recommend reconsidering the name.**

| Project | What it is | Collision risk |
|---------|-----------|----------------|
| **coleam00/Archon** | Open-source "harness builder for AI coding — make AI coding deterministic and repeatable"; YAML workflows, MCP, command-center for AI agents. **22.7k★** | **SEVERE** — same adjacent space (AI coding agents + determinism + MCP), huge mindshare |
| **pytest-archon** (jwbargsten) | Python architecture-rule testing (forbidden deps) | **HIGH** — same *problem domain* (architecture enforcement), shared "archon" root, on PyPI |
| **Archon "Agenteer"** (older coleam00 / Decentralised-AI mirrors) | AI agent that builds AI agents | Medium — legacy but indexed |
| **Archon (`archon.inc`)** | Docs/commercial site for the coding harness | Medium |
| **Archon** (Y Combinator) | Helps software companies sell to government (FedRAMP compliance) | Low (different domain) |
| **Archon Construction** (`archondb.com`) / **Archon** (SourceForge, old) | Construction mgmt / legacy | Low |

- **"Akron"** (the working-dir name): no significant dev-tool collision found — it
  reads as the Ohio city. Safer than "Archon," but generic/geographic.
- **Near-miss with a direct competitor:** **Archgate** ("Arch"-prefixed ADR-
  enforcement-for-AI product, §9) is one letter-cluster away and in the *identical*
  space — an "Archon" launch would read as an Archgate clone.

**Recommendation:** avoid "Archon" (the `coleam00/Archon` 22.7k-star project alone
makes it unsearchable and confusing in this exact market; `pytest-archon` compounds
it in the architecture-rules niche). "Akron" is workable but weak. Pick a
distinctive, unclaimed mark before external comms.

URLs: https://github.com/coleam00/Archon · https://github.com/jwbargsten/pytest-archon ·
https://pypi.org/project/pytest-archon/ · https://www.ycombinator.com/companies/archon ·
https://archondb.com/ · https://sourceforge.net/projects/archon.mirror/ · https://archgate.dev/

---

## 12. Sources (primary, verified)

All URLs are inlined per section above. Anchor competitors (Mneme HQ, Archgate,
coleam00/Archon) and the `codebase-memory-mcp` tool surface (`manage_adr`,
`get_architecture`, `trace_path`, `query_graph`, `ingest_traces`) were verified
first-hand; library/product facts came from official repos, docs, PyPI/npm, and
vendor sites via search + fetch. Figures marked *unverified* (notably several
GitHub star counts) gave inconsistent readings and should be re-checked before
external use.
