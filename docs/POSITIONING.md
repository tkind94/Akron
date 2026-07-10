# Akron — Positioning

**2026-07-04, reframed 2026-07-06 (TKI-43); reframed again 2026-07-06 (TKI-45,
pivot 2: exploration-only).** JOURNEYS.md answers "how does a person use
this?" This document answers the question one step earlier: **why would
anyone come looking?** What just happened to them, what do they type into a
search bar, who do they find there today, and why would they pick us. Every
claim about the landscape below was checked against the live 2026 tool
market, not assumed.

---

## 0. Scope, stated first

Akron is a local codebase exploration tool: four verbs — `find`, `explain`,
`scan`, `explore`. It is strong on domain-vocabulary,
adapter-shaped codebases — the target: internal work repos where identifiers
speak the team's own vocabulary. It is weaker on library/protocol-shaped
code, where a good grep is hard to beat. Measured, not asserted: `explain`
scores 8 wins / 3 ties / 1 loss against manual `rg` + reading; `find` on a
private domain-vocabulary codebase scores P@5 0.68 against a best-faith
grep's 0.36 and wins thinly on httpx (0.50 vs 0.40; R&D archive spike/embed2 round 2);
the `scan` family view scores 0.702 member
precision on httpx — below the keep bar, so it ships experimental behind
`--only families`. Full numbers and sources: README.md "Honest scope",
the R&D archive validation records.

`scan` shows similarity; it does not judge it. Pivot 1 shipped a
guardrails layer (`check`/`ratify`, a verdict store) on top of the
similarity engine; the CEO's own dogfooding run showed it asserting
judgment it couldn't back — deliberate house conventions flagged as
duplication, domain-noun coincidences flagged as competing, test scaffolding
glued into families. Pivot 2 (TKI-45) removed the entire guardrails layer:
Akron now only shows what the deterministic channels measured, and leaves
"is this a problem" to the person reading it.

---

## 1. The trigger moments

Nobody searches for a category; they search from a pain, minutes after it bites.

| # | Moment | Persona (JOURNEYS.md) |
|---|--------|-----------------------|
| **T1** | A reviewer realizes a PR adds a *third* way to do retries/proxies/pagination — and can't cite where the other two are. | Reviewer |
| **T2** | A new engineer finds two connection helpers and has no way to know which one is current. "Which do I copy?" | Engineer new to the repo |
| **T3** | An AI coding agent keeps reinventing helpers that already exist, because nothing tells it what's already there. The operator wants the agent to *see the existing shapes* without maintaining a prose rules file. | AI coding agent (operator) |
| **T4** | A tech lead suspects duplication is accumulating and needs to *see* it — and show it to others — before arguing for cleanup time. | Steward / tech lead |

T3 is the newest and fastest-growing. "AI codebase drift" became a named problem in
2025–26 — agent-generated code amplifies exactly the self-similarity noise
Akron measures, and a wave of tools (VibeDrift's coherence score, real-time drift
dashboards) exists purely to serve that anxiety — most of them scoring it with an
LLM judge. Akron's answer stays deterministic: it shows the shape/vocabulary
neighborhoods, not a verdict on them.

## 2. What they type, and who they find

| Search intent | Who owns the results today | Our position |
|---|---|---|
| "duplicate code detector", "copy-paste detector" | Crowded: PMD CPD, jscpd, SonarQube duplication gates | We match incidentally (repeated query) but **should not fight here** — token-clone detection is a solved, commoditized problem. |
| "find similar functions in codebase" | Semi-open; IDE features, research tools | Repeated query, cross-file ranked. Winnable. |
| "two implementations of the same thing", "codebase consistency" | **Open.** No incumbent. | This is the *shared-vocabulary/divergent-shape* query (`--only competing`) — our structural moat. A clone detector **cannot** see two implementations of one job in different shapes, because there is no textual clone; it requires exactly the high-vocabulary/low-shape split our two independent channels produce. |
| "code drift", "AI code drift" | Emerging, AI-flavored: VibeDrift, drift dashboards, blog wave | Growing fast, weakly defended. We already use the word natively; our answers are deterministic where theirs are LLM-scored. |

## 3. The sentence

The wedge is the four verbs, not one sentence about duplication:

> **`find` where something is handled, in your own words. `explain` what a symbol is
> and what it's tangled with. `explore` the whole map live — neighborhoods,
> similarity to any anchor, age — one command, one localhost page. (`scan`
> emits the same measurements as versioned JSON, for tooling.)**

Per-audience openers:
- **T1/T2 (humans in the repo):** "Which of these is the one we use? `akron explain`
  names the near-clones on one symbol; `akron explore` shows the whole
  landscape with evidence and dates."
- **T3 (agent operators):** "Give your coding agent three verbs: search in its own
  words, a one-screen card before touching an unfamiliar symbol, and the
  shapes the repo already has — deterministic and token-free (the one
  exception: `find`'s ranking may use a small local embedding model,
  DESIGN.md §1.2), no prose file to rot."
- **T4 (stewards):** "One command, `akron explore`, for the live map of every place
  the codebase repeats itself — neighborhoods, similarity to any anchor, age."

## 4. What we are deliberately not

Not a linter, not a security scanner, not an LLM reviewer, not a metrics
dashboard, not a policy gate. No config to write, no rules to maintain, no
tokens to buy, no server to run. Determinism is the trust story: `scan` and
`explain` give the same answer on the same repo state, every time — no LLM
judge scoring the same code differently on rerun.

## 5. Implications (each is an action)

1. **README leads with trigger moments, not architecture.** A person arriving from
   T1–T4 must see their pain in the first screen. The three-channel embedding is
   §"how it works", not the headline. → TKI-24
2. **Vocabulary discipline.** The words in our output — *repeated, shared
   vocabulary, divergent shape, drift* — are the words people search. Keep
   them stable across CLI/explore/JSON/docs; they are the category vocabulary
   we want to own. Words that assert a judgment ("reinvented", "competing
   implementations") are banned from user-facing output (TKI-45) — Akron
   shows similarity, it doesn't grade it.
3. **Distribution is packaging.** The places people actually find tools like this:
   `cargo install` / Homebrew / prebuilt binaries (TKI-13), analysis-tools.dev
   and the awesome-static-analysis list, a Show HN once packaging lands
   (TKI-13/24; `explore` shipped 2026-07-06). → TKI-24 checklist
4. **The agent surface is a skill over the CLI, not an MCP server** (owner
   decision 2026-07-04): the engine is a fast deterministic CLI with versioned
   JSON — a skill delivers the agent moments with zero new code surface, and
   nothing recomputed means there is no server state to hold. If distribution
   later demands an MCP-directory presence, a thin generated wrapper over the
   CLI suffices (optional item on TKI-24's checklist), not a first-class
   surface.

## Sources (landscape checks, 2026-07)

- jscpd — MCP server + "AI-ready" reporter: https://github.com/kucherenko/jscpd
- PMD CPD docs: https://pmd.github.io/pmd/pmd_userdocs_cpd.html
- Duplicate-checker roundup (2026): https://dev.to/rahulxsingh/13-best-duplicate-code-checker-tools-in-2026-1cnk
- AI codebase drift wave: https://www.propelcode.ai/blog/ai-codebase-drift-cleanup-loops , https://dev.to/skaaz/your-ai-written-codebase-is-drifting-heres-how-to-measure-it-f10
