# Problem Evidence: Architectural Drift as a Real, Costly, and Unsolved Problem

*Research brief for the code-architecture-governance tool. Compiled July 2026.*

> **Scope note.** This report deliberately does **not** re-cover the material we already hold strong sources on — `zakirullin/cognitive-load`, Naur's "Programming as Theory Building," GitClear, METR, and Apiiro's AI-code-quality data. It goes beyond them into (1) architecture erosion/drift as a studied phenomenon, (2) onboarding cost, (3) human governance mechanisms and *how they fail*, (4) the 2025–2026 AI-agent version of the problem, (5) review economics, and (6) honest counter-evidence.
>
> **Sourcing conventions.** URLs are inline. Numbers originating from a vendor with a product in this space are tagged **[vendor]**. Numbers from non-peer-reviewed blogs are tagged **[blog — unverified]**. Peer-reviewed / academic sources are tagged **[peer-reviewed]**.

---

## Executive Summary

**Is the problem real?** Yes, and it is named and studied. "Architecture erosion" (a.k.a. drift, degradation, decay) is the divergence of a system's *implemented* architecture from its *intended* architecture, accumulating "imperceptibly" over time. It has a systematic mapping study covering 73 papers ([Li et al., 2022, *J. Softw. Evol. Process*](https://onlinelibrary.wiley.com/doi/10.1002/smr.2423)), a 2023 doctoral thesis ([Groningen](https://research.rug.nl/en/publications/understanding-analysis-and-handling-of-software-architecture-eros/)), a 2025 remediation survey ([arXiv 2507.14547](https://arxiv.org/pdf/2507.14547)), and a practitioner-perspective study ([arXiv 2103.11392](https://arxiv.org/pdf/2103.11392)). The most frequent erosion symptoms practitioners report are **architectural violations, duplicated functionality, and cyclic dependencies** — exactly the "3 competing patterns / reinvented helper" failure mode.

**Is it sized/costly?** Yes.
- **Defects & throughput:** Unhealthy code carries **15× more defects**, takes **124% (2.24×) longer** in development time, and has **up to 9× longer maximum cycle times** than healthy code ([CodeScene "Code Red," ICTD 2022](https://arxiv.org/pdf/2203.04374)) **[vendor, peer-reviewed]**.
- **Onboarding:** New engineers take **3–9 months** to full productivity without structured onboarding; much of the ramp is learning house conventions, not language ([DX / Abi Noda survey of 80 orgs](https://newsletter.getdx.com/p/developer-onboarding-time)) **[vendor]**.
- **Review:** Developers spend **~6.4 hours/week** reviewing; the majority of review comments concern maintainability/convention, not logic ([Microsoft study](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/bosu2015useful.pdf); [arXiv taxonomy](https://arxiv.org/pdf/2103.08777)).
- **Knowledge loss:** When a senior leaves, tasks they solved in 10 minutes take 2 hours; operational recovery takes **1–2 years** ([bus-factor literature](https://arxiv.org/pdf/2202.01523)).

**Is it unsolved?** This is the crux. Every *human* governance mechanism that exists to keep a codebase consistent decays or fails to scale — Google's readability certification (bottlenecks, "$5,000 wasted" reviews), ADRs (the "graveyard" problem — written, never read, silently wrong), RFCs/design docs (value is in the writing, not the artifact; they go stale), CODEOWNERS (single-point bottleneck → rubber-stamping), and custom lint rules (need a "hero engineer" custodian nobody funds). Every *automated* architecture-governance tool that could enforce structure (Structure101, Lattix, Sonargraph, ArchUnit) stayed niche. And the **AI-agent era makes drift structurally worse**: instruction files decay within a session, agents hold no theory of the codebase and "semantically duplicate" helpers, and AI has driven PR volume up **~29–98%**, overwhelming the human review gate that was the last line of defense.

**The one-sentence thesis:** the problem is real and expensive; the reason it's unsolved is that *consistency is currently enforced by human attention and point-in-time documents, both of which decay* — and the AI-agent workflow is simultaneously the biggest new source of drift and the biggest new consumer of a governance signal that doesn't yet exist.

---

## 1. Architectural Erosion / Drift: Definitions and Evidence

### It is a named, surveyed phenomenon (not folklore)

- **Definition.** Architecture erosion occurs when a system's implemented architecture diverges from its intended architecture over time; it "accumulates imperceptibly" and is described from four perspectives — **violation, structure, quality, and evolution** ([Li, *Understanding software architecture erosion: A systematic mapping study*, 2022](https://onlinelibrary.wiley.com/doi/10.1002/smr.2423)) **[peer-reviewed]**. The mapping study included **73 studies**.
- **Recent consolidation.** A 2023 University of Groningen PhD thesis ([*Understanding, Analysis, and Handling of Software Architecture Erosion*](https://research.rug.nl/en/publications/understanding-analysis-and-handling-of-software-architecture-eros/)) and a July 2025 survey, [*Architectural Degradation: Definition, Motivations, Measurement and Remediation Approaches*](https://arxiv.org/pdf/2507.14547), show the topic is still actively researched in 2025 — i.e., **not solved**.
- **Practitioner symptoms.** The empirical practitioner study ([arXiv 2103.11392](https://arxiv.org/pdf/2103.11392)) found the most frequent erosion symptoms are **architectural violations, duplicate/redundant functionality, and cyclic dependencies**, and that erosion is typically caught late, via code review and ad-hoc tooling, during implementation rather than prevented. This is the empirical grounding for the "3 competing patterns" and "reinvented helper" personas.
- **Automated detection is still a research problem.** [*Towards Automated Identification of Violation Symptoms of Architecture Erosion*](https://arxiv.org/pdf/2306.08616) (2023) frames detecting erosion from developer discussions/commits as an open ML task — evidence that no turnkey signal exists today.

### The cost of letting it run: "Big Ball of Mud"

The canonical anti-pattern is the [Big Ball of Mud](https://deviq.com/antipatterns/big-ball-of-mud/): a codebase with no discernible structure that becomes "difficult to disentangle." The compounding cost is the *chilling effect on change* — "developers may hesitate to refactor or improve the codebase for fear of introducing unintended side effects or destabilizing the system" ([DEV summary](https://dev.to/m_midas/big-ball-of-mud-understanding-the-antipattern-and-how-to-avoid-it-2i)). This is the "refactoring becomes hopeless" end-state: drift is self-reinforcing because the drift itself raises the cost of the cleanup.

### Quantifying consistency's effect on defects & maintenance: the Code Red study

The strongest single number source is CodeScene's [*Code Red: The Business Impact of Code Quality*](https://arxiv.org/pdf/2203.04374) (39 proprietary production codebases; accepted at the **International Conference on Technical Debt 2022**) **[vendor-authored but peer-reviewed]**:

| Metric | Unhealthy ("Red") vs Healthy ("Green") code |
|---|---|
| Defect density | **15× more defects** |
| Time-in-development for a change | **+124%** (2.24× longer) |
| Maximum cycle time (worst-case predictability) | **up to 9× longer** |

CodeScene's "Code Health" metric is largely a *consistency/complexity* score (nesting, duplication, low cohesion, "brain methods"), so this is a reasonable proxy for "what drift costs." Caveat: CodeScene sells a tool that measures this, so treat magnitudes as directional. Summary and independent write-up: [InfoQ](https://www.infoq.com/articles/business-impact-code-quality/), [CodeScene blog](https://codescene.com/blog/measuring-the-business-impact-of-low-code-quality).

**Takeaway for §1:** The phenomenon is defined, surveyed across 70+ studies, still an open research topic in 2025, and there is a credible (if vendor-linked) order-of-magnitude cost figure. What's missing in the literature is *prevention that scales* — which is the product opportunity.

---

## 2. Onboarding Cost: Learning the House Conventions

### Time-to-productivity is measured in months

- **3–9 months to full ramp** is the headline, from a survey of **80 engineering organizations** ([DX / Abi Noda](https://newsletter.getdx.com/p/developer-onboarding-time)) **[vendor]**. Without structured onboarding, "three to six months … in some companies … approach a year"; with good onboarding it compresses to **8–12 weeks** ([daily.dev playbook](https://recruiter.daily.dev/resources/developer-onboarding-first-90-days-playbook-engineering-teams/)).
- New hires reach only **~25% productivity in the first 30 days** without structure ([summary](https://correctcontext.com/10-developer-onboarding-best-practices-that-reduce-time-to-productivity-by-60-2026-guide/)) **[blog — unverified]**.
- DX tracks "Time to 10th PR" as an industry metric: **33 days as of April 2026**, down from 39 in Q4 2025 ([DX](https://newsletter.getdx.com/p/developer-ramp-up-time-continues)) **[vendor]** — notable because AI is *shrinking* the raw-coding portion of ramp, leaving the *convention-learning* portion relatively larger.

### The expensive part of ramp is tacit convention, and it doesn't transfer via docs

- **Documentation reliably fails to stay current.** "Most teams want documentation, but few keep it current. Confluence pages drift out of date. GitHub wikis miss real-world practice. Slack answers vanish into the scroll" ([tribalhabits](https://tribalhabits.com/how-to-fix-knowledge-loss-when-developers-leave/)). Docs lose the race to "client deadlines, incidents and delivery pressure."
- **Bus factor / tribal knowledge loss is empirically real.** [*Bus Factor in Practice* (arXiv 2202.01523)](https://arxiv.org/pdf/2202.01523) formalizes how concentrated knowledge is. When it walks out the door: "problems the veteran solved in 10 minutes now take 2 hours," repeat failures rise, and "operational performance drops and takes 1–2 years to recover" ([tribalhabits](https://tribalhabits.com/how-to-fix-knowledge-loss-when-developers-leave/)). A widely-cited estimate puts knowledge loss at **~$47M/year per large organization** ([slite](https://slite.com/learn/tribal-knowledge)) **[blog — unverified, treat as illustrative]**.

**Personas link:** the new-engineer-onboarding persona and the departing-senior persona are the *same problem viewed from two sides* — the codebase's conventions live in people's heads (Naur's "theory"), and both onboarding and attrition are moments when that theory fails to transfer. A machine-readable, always-current "which pattern is canonical here" signal is precisely what neither docs nor mentorship reliably provide.

---

## 3. Human Governance Mechanisms and Their Failure Modes *(the crucial section)*

Every mechanism below exists *because* teams feel the drift problem. Each has a characteristic decay mode. A governance tool must avoid repeating these.

### 3.1 Google "Readability" certification — the gold standard, and its costs

**How it works.** Every changelist needs approval from someone holding *readability* in that language — an internal certification of idiom mastery. Engineers earn it by submitting CLs to a central pool of volunteer reviewers until they stop drawing comments and "graduate" ([HackerOne/PullRequest](https://www.pullrequest.com/blog/google-code-review-readability-certification/); [Modern Descartes essay](https://www.moderndescartes.com/essays/readability/)).

**Scale.** An estimated **one-third to one-half of Google engineers** hold readability in their primary language; "thousands of mentors shepherd hundreds of thousands of Googlers" ([Modern Descartes](https://www.moderndescartes.com/essays/readability/)).

**What it costs / how it decays:**
- **Latency & bottlenecks.** "If a team writes C++ but nobody has C++ readability, the team constantly has to find approvers from outside the team" — slows teams considerably ([HackerOne](https://www.pullrequest.com/blog/google-code-review-readability-certification/)). Bottleneck spikes when holders take vacation.
- **Cost per bad interaction.** One mentor: *"That negative review was a total waste of both our time, and probably cost Google $5,000 in lost productivity."* Initial reviews take **"well over an hour per diff"** (~1 line/minute) before speeding up 10× over months.
- **Human-linter reviewers.** "That one readability reviewer who takes pride in their prowess as a human code linter, or has a vendetta against some language feature" ([Modern Descartes](https://www.moderndescartes.com/essays/readability/)).
- **Perceived bureaucracy.** "To many veteran Googlers, readability still seems like unnecessary bureaucracy."

**Key insight for the product:** Readability is *human enforcement of consistency at planet scale*, and even Google pays for it in latency, money, and morale. The essay's own proposed fix — **"Readability Lite,"** non-blocking encouragement rather than a mandatory gate — is essentially an argument for *tooling that carries the same signal without the human bottleneck*. That is the wedge.

### 3.2 ADRs (Architecture Decision Records) — the "graveyard" problem

**The trajectory is a cliché because it's ubiquitous.** Teams adopt ADRs enthusiastically, write them carefully for a few months, then "quietly abandon" them; the repo "becomes archaeological, and new decisions get made without reference to it at all" ([Java Code Geeks, May 2026](https://www.javacodegeeks.com/2026/05/the-reason-most-architecture-decision-records-get-written-and-never-read-is-architectural-not-cultural.html)).

**Why — and this is the important reframing:** the failure is *architectural, not cultural*. ADRs are **point-in-time documents asked to perform a living-artifact function — a category error baked into the format.** "When the system evolves and the document no longer matches, nothing breaks. There is no signal. The divergence is invisible until someone reads the document and acts on outdated information." An ADR from 2021 is "not merely irrelevant in 2025 — it is actively misleading." The article's prescription: **ADRs need coupling to living systems / enforcement mechanisms** rather than manual discipline. (Same conclusion Martin Fowler's [original ADR bliki](https://martinfowler.com/bliki/ArchitectureDecisionRecord.html) and [AWS](https://docs.aws.amazon.com/prescriptive-guidance/latest/architectural-decision-records/adr-process.html) / [Azure Well-Architected](https://learn.microsoft.com/en-us/azure/well-architected/architect-role/architecture-decision-record) guidance dance around but never solve.)

**Failure mode to avoid:** *a doc with no coupling to code has no failure signal, so it rots silently.* Any "conventions" artifact that isn't checked against the code will become an ADR graveyard.

### 3.3 RFCs / design docs — value is in the writing, not the artifact

- Google moved from a single central review mailing list to distributed review as it scaled; formal design-review meetings involve "a very senior engineering audience" ([industrialempathy, *Design Docs at Google*](https://www.industrialempathy.com/posts/design-docs-at-google/)).
- **Crucial admission:** the primary value "wasn't the doc itself — it was the forced specificity of the author's thinking." RFCs "deliver little intrinsic value … a snapshot of design considerations at a time" ([Pragmatic Engineer, *Scaling via RFCs*](https://blog.pragmaticengineer.com/scaling-engineering-teams-via-writing-things-down-rfcs/); [failure modes](https://betterprogramming.pub/goals-and-failure-modes-for-rfcs-and-technical-design-documents-c4ee1d1da6ff)).

**Failure mode:** design docs capture *intent at t=0* and are never reconciled with the code that ships — same silent-divergence problem as ADRs. They scale the *thinking*, not the *enforcement*.

### 3.4 CODEOWNERS + review gates — bottleneck → rubber-stamp

- **The gate becomes a single point of failure.** "If CODEOWNERS names one person for every critical area, that person becomes a bottleneck" ([DeepDocs](https://deepdocs.dev/code-review-in-github/)).
- **Overload produces rubber-stamping.** "LGTM fatigue is real … a reviewer sees familiar code, trusts the author, and stops interrogating the change." Rubber-stamping "often points to oversized PRs or overloaded reviewers" ([Medium, *Rubber Stamps or Real Quality Gates*](https://dev.to/abdulosman/code-reviews-rubber-stamps-or-real-quality-gates--15c0)). Security PRs are ignored ~85% of the time ([Pixee](https://www.pixee.ai/blog/merge-rate-problem-security-prs-ignored)).

**Failure mode:** a human gate that's too slow gets bypassed (rubber-stamp) or becomes a throughput bottleneck. Governance that *adds* human gates inherits this.

### 3.5 Diff bots (Danger.js) — good, but manual custodianship

[Danger.js](https://danger.systems/js/) "runs after your CI, automating your team's conventions surrounding code review … codify your team's norms, leaving humans to think about harder problems." It works — "discussions about tabs vs spaces are less likely to derail a review when the choice has already been codified." **But** every rule is hand-written JS/TS that someone must maintain. It codifies *mechanical* conventions (file naming, changelog present, PR size), not *architectural* ones ("use the repository pattern, not raw SQL here").

### 3.6 Custom lint rules — the "hero engineer" custodianship trap

- Custom rules typically get written when "a *hero engineer* identifies inconsistent patterns and implements custom linting rules" ([Compass/Medium](https://medium.com/compass-true-north/linting-a-practical-guide-to-introducing-it-to-your-team-28e3605a0dc2)). Individual rules are "usually only 10–20 lines," but "the ongoing custodianship requires regular attention and team buy-in" ([zeepalm](https://www.zeepalm.com/blog/write-custom-lint-rules-kotlin)).
- **The maintenance burden, not the authoring, is the killer:** "the bigger challenge is keeping them relevant and effective as your project evolves."

**Failure mode:** custom enforcement depends on a volunteer custodian; when that person context-switches or leaves, rules ossify or are disabled. AST-based lint rules also can't express *architectural* intent easily (cross-module dependency policy, "this is the canonical HTTP client").

### 3.7 Style guides at scale — necessary but insufficient

Google's public style guides + readability are the reference implementation, but the [SWE-at-Google knowledge-sharing chapter](https://abseil.io/resources/swe-book/html/ch03.html) makes clear that a *written* guide only works when backed by *automated formatters/linters and a human certification process*. The prose guide alone doesn't enforce anything.

### 3.8 Fitness functions / ArchUnit — the closest existing "enforcement" tech, and why it's still niche

[ArchUnit](https://medium.com/xebia-france/enforcing-architecture-decisions-with-archunit-4d8b9f61cf4a) (Java), Dependency Cruiser (JS), and .NET equivalents let you assert "classes in package X must not depend on Y" as CI-checked tests — Neal Ford / Rebecca Parsons' **fitness functions** from *Building Evolutionary Architectures* ([InfoQ](https://www.infoq.com/articles/fitness-functions-architecture/)). Its 1.3 release (Feb 2026) added `FreezingArchRule` for incremental legacy adoption.

**Why it hasn't solved the problem:**
- **Scope is narrow.** "ArchUnit does not apply to enterprise-level decisions, only application design decisions" — it's dependency/layer/naming rules, not "which of these 3 patterns is canonical."
- **Over-enforcement backfires.** "When every minor coding decision is enforced by a rule, innovation and velocity suffer … teams treat architecture tests as red tape" ([DevelopersVoice](https://developersvoice.com/blog/architecture/architectural-fitness-functions-automating-governance/)).
- **Still requires someone to author and maintain every rule** — same custodianship trap as §3.6.

**Synthesis of §3:** There is a clean split. *Document-based* mechanisms (ADR, RFC, style guide) capture intent but have **no coupling to code, so they rot silently**. *Enforcement-based* mechanisms (readability, CODEOWNERS, lint, ArchUnit) couple to code but rely on **human attention or hand-maintained rules that don't scale and induce fatigue/bottleneck/red-tape**. Nobody has the combination: *automatically-discovered, always-current, code-coupled convention that enforces without a human gate or a hand-written rule per convention.*

---

## 4. The AI-Agent-Era Version of the Problem (2025–2026)

This is where the problem gets *worse and newly urgent*, and where practitioner pain is loudest.

### 4.1 Instruction files (CLAUDE.md / AGENTS.md) decay *within a session*

- **Documented best practice is now "keep it lean."** "If your config file is over 500 lines, most of it is being ignored … a focused 50-line file outperforms a sprawling 1,000-line one" ([HumanLayer](https://www.humanlayer.dev/blog/writing-a-good-claude-md); [ADI Pod](https://adipod.ai/blog/claude-md-best-practices/)).
- **An "instruction budget" framing has emerged:** frontier models follow ~150–200 instructions consistently; Claude Code's own system prompt eats ~50, leaving ~100–150 slots for your CLAUDE.md ([Bijit Ghosh, Medium](https://medium.com/@bijit211987/the-complete-guide-to-claude-md-memory-rules-loading-and-cross-tool-compression-97cc12ed037b)) **[blog — unverified but widely repeated]**.
- **In-session decay is the sharpest claim:** one widely-circulated figure is "95%+ compliance at messages 1–2, dropping to 60–80% by messages 3–5, 20–60% by messages 6–10, original instructions mostly gone beyond ten messages" ([maketocreate](https://maketocreate.com/claude-md-best-practices-the-complete-2026-guide/)) **[blog — unverified; cite as practitioner claim, not measurement]**.
- **The measurable analog exists in the academic literature.** "Lost in the middle" shows LLM accuracy drops **15–30 percentage points** for information in the middle of a long context vs. the ends (Liu et al.; [summary](https://pristren.com/blog/lost-in-middle-attention-paper/)), and [LongGenBench](https://arxiv.org/pdf/2510.14842) shows "severe loss of prompt adherence for complex instruction sets in long-form generation." "Context rot" is the practitioner term ([Morph](https://www.morphllm.com/context-rot)). **The rules you gave the agent at turn 1 are demonstrably weaker by turn 20.**

**This is the core AI-era argument:** prompt-level house rules are the *primary* way teams keep agents on-convention today, and they provably decay both by length (instruction budget) and by session depth (context rot). An agent holds *no persistent theory of the codebase* — it sees "only what it currently looks like, not why."

### 4.2 Agents reinvent helpers ("semantic duplication")

- The failure has a name: **"semantic duplication"** — agents "code functions almost identical to existing helpers, just wrapped with different names and styles" ([Pharaoh](https://pharaoh.so/blog/prevent-duplicate-functions-ai-coding/)).
- Mechanism: "AI tools lack holistic structural awareness … see projects file by file"; "without clear upfront intent, agents invent what feels missing, regardless of whether it exists"; "large models forget earlier code, causing logic to be re-derived, creating parallel implementations that drift over time" ([The New Stack](https://thenewstack.io/the-4-ways-ai-code-is-breaking-your-repo-and-how-to-fix-it/)).
- Live practitioner complaint threads: [*Ask HN: Why do AI agents keep repeating mistakes your team already fixed?*](https://news.ycombinator.com/item?id=47399209) and [*Some uncomfortable truths about AI coding agents*](https://news.ycombinator.com/item?id=47545748). Recurring theme: agents "don't know about patterns your team reverted weeks ago or understand why the codebase is structured the way it is."

### 4.3 Multi-agent workflows amplify divergence

- **"Prompt drift"**: parallel/looped agents "gradually diverge from what you actually wanted" ([Addy Osmani, *Code Agent Orchestra*](https://addyosmani.com/blog/code-agent-orchestra/)).
- Coding *style* emerges as "a dominant factor in consistency between different agents" — "Minimalist Coders" align at 81–85, "Default Coders" swing 73–87 ([research summary](https://www.verdent.ai/guides/multi-agent-coding-tools)). Multiple agents on one repo = multiple competing pattern authors with no shared canon.

### 4.4 The emerging (partial) playbooks — and their explicit limits

Teams are converging on the same conclusion: **move enforcement from advisory prose to deterministic tooling.**
- **Hooks over instructions.** "Use hooks for actions that must happen every time with zero exceptions. Unlike CLAUDE.md instructions which are advisory, hooks are deterministic" ([Claude Code best practices](https://code.claude.com/docs/en/best-practices)). A hook that lints after every edit "enforces code quality automatically, without relying on the agent to remember."
- **"You need a linting config, not just agent instructions."** Teal Larson's [post](https://www.teallarson.dev/blog/2026-03-27-dont-make-your-agent-file-a-linting-config) is a clean statement of the thesis: agents "circumvent linting enforcement, similar to how developers use `--no-verify`," and were observed "adding lint-disable comments to code instead of resolving linting problems." Conclusion: put style in deterministic tooling; reserve the agent file for architecture/domain judgment that *can't* be automated.
- **Spec-Driven Development + "constitutions."** SDD makes a versioned spec the source of truth and adds a project **"constitution" … durable decisions (language, framework, testing, security) written as EARS statements** every agent action must respect ([BCMS SDD guide](https://thebcms.com/blog/spec-driven-development); [Spec Kit Agents, arXiv 2604.05278](https://arxiv.org/html/2604.05278v1)). Validation hooks check intermediate artifacts and run linters/tests after implementation.

**The gap these playbooks reveal:** hooks/linters/constitutions all still require a human to *author and maintain* the rules (§3.6 trap), and they cover mechanical/structural rules well but **architectural "which pattern is canonical" poorly**. The AI-native workflow has *raised the value* of a machine-readable convention signal precisely as it has *lowered the reliability* of the prose that currently carries it.

---

## 5. Review Economics: Convention-Policing Dominates, and AI Is Flooding the Gate

### Convention/style is the majority of review comments

- "**Four out of five** code review comments regard style and nitpicking issues that can also be identified using static analysis tools" ([arXiv taxonomy of modern code review, 2103.08777](https://arxiv.org/pdf/2103.08777)) **[peer-reviewed]**.
- Using a defect-based taxonomy, "**75% of code reviews relate to maintainability, 25% to functional issues**" (same literature). The canonical Microsoft study analyzed **1.5M review comments across 5 projects** to model comment usefulness ([Bosu et al., MSR](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/bosu2015useful.pdf)).
- **Implication:** the majority of the ~6.4 hrs/week engineers spend reviewing is spent re-litigating conventions a machine could carry — the exact "reviewers burn time on already-decided conventions" persona.

### AI has broken the review gate's capacity

- **Volume is up sharply.** GitHub Octoverse 2025: merged PRs **+29% YoY**, "driven largely by AI coding assistants" **[vendor]**. Faros AI: teams using AI generate **98% more PRs** with a **91% increase in PR review time** ([Faros via Codacy](https://blog.codacy.com/ai-breaking-code-review-how-engineering-teams-survive-pr-bottleneck)) **[vendor]**.
- **The bottleneck moved from writing to reviewing.** "The bottleneck has moved from writing code to deciding whether code is safe to merge"; AI-generated PRs "wait 4.6× longer before a reviewer even picks them up" ([Codacy](https://blog.codacy.com/ai-breaking-code-review-how-engineering-teams-survive-pr-bottleneck); [async squad](https://asyncsquadlabs.com/blog/code-review-bottleneck-ai-era/)) **[vendor/blog]**.
- **"Pattern Recognition Fatigue."** "AI code often follows similar patterns, causing reviewers to skim rather than deeply analyze, meaning subtle bugs slip through" — rubber-stamping under load, now automated in its cause.

**Implication:** raising PR volume with AI while convention-policing still consumes ~80% of review comments means the human gate is *quantitatively unable* to keep up — strengthening the case for pushing convention enforcement left, out of the review, into a deterministic pre-review signal.

---

## 6. Counter-Evidence and Risks (Be Honest)

A credible problem framing must survive these.

### 6.1 Teams thrive *without* heavy governance

- There is a real school of thought that **mandatory review/governance is net-negative for many changes.** "When code reviews are mandatory … this incentivizes developers to do shallow code reviews which add very little value"; **56% of surveyed practitioners** thought it permissible to skip review based on "project needs" and "seniority" ([arXiv 2311.02489, *Does Code Review Speed Matter?*](https://arxiv.org/pdf/2311.02489); [Test Double](https://testdouble.com/insights/when-code-reviews-arent-mandatory)). "After ~one review per two PRs, quality gains flatten." Small high-trust teams often ship fine on convention + taste alone.
- **Design docs considered harmful** ([Lucas da Costa](https://www.lucasfcosta.com/blog/design-docs)) argues process artifacts can substitute for actually building. Implication: a governance tool must not become *another* artifact that adds ceremony without payback.

### 6.2 Pattern enforcement can ossify the codebase (the strongest risk)

- **The wrong-abstraction lock-in.** Sandi Metz: [*"Duplication is far cheaper than the wrong abstraction."*](https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction) Once a pattern is enshrined, the next engineer "feels obligated to retain the existing abstraction," bolting on parameters and conditionals until it's incomprehensible — and sunk-cost pressure prevents unwinding it. **A tool that enforces "the canonical pattern" risks freezing a *bad* pattern and punishing the person trying to escape it.** This is the single most important design tension: consistency enforcement and healthy evolution are in direct opposition unless the tool makes conventions *cheap to revise*, not just cheap to enforce.
- ArchUnit's own community warns that over-enforcement makes teams "treat architecture tests as red tape" and "innovation and velocity suffer" (§3.8).

### 6.3 Gate fatigue / rubber-stamping applies to *automated* gates too

If a governance tool adds another blocking check, it inherits the LGTM-reflex dynamic (§3.4): noisy or slow gates get ignored, disabled, or `--no-verify`'d — and agents already do exactly this ("adding lint-disable comments," §4.4). A gate that fires too often trains people (and agents) to bypass it.

### 6.4 Why prior architecture-governance tools never went mainstream

Structure101, Lattix, and Sonargraph are technically capable and academically validated ([Pruijt et al., *Accuracy of dependency analysis in static ACC*](https://onlinelibrary.wiley.com/doi/full/10.1002/spe.2421)), yet stayed niche. Structure101 was ultimately **acquired by Sonar in 2025** ([Sonar](https://www.sonarsource.com/structure101/); [BankInfoSecurity](https://www.bankinfosecurity.com/sonar-adds-code-architecture-insights-structure101-buy-a-26538)) — consolidation, not breakout adoption. Likely reasons: heavyweight desktop tooling, upfront modeling of a "desired architecture" that teams never do, no place in the everyday inner loop, and value visible only to architects (not the engineer at the keyboard). **Lesson: architecture governance that lives outside the developer's daily workflow doesn't get used.** The AI-agent inner loop is a *new* place to live that these tools never had.

---

## Failure Modes Any Solution Must Avoid

Distilled from §3 and §6 — a checklist:

1. **No coupling to code → silent rot.** Any convention captured only as prose (ADR/RFC/wiki) diverges invisibly because divergence produces no signal (§3.2). *The convention must be checked against the code, continuously.*
2. **Human-gate dependency → bottleneck + rubber-stamp.** Adding a person to the critical path recreates the CODEOWNERS/readability bottleneck and LGTM fatigue (§3.1, §3.4). *Enforce without inserting a human approver.*
3. **Hand-maintained rule-per-convention → custodianship collapse.** Danger/lint/ArchUnit die when the "hero engineer" custodian moves on (§3.5–3.8). *Conventions must be discovered/maintained largely automatically, not hand-authored one by one.*
4. **Over-enforcement → red tape + ossification.** Enforcing every micro-decision kills velocity and locks in wrong abstractions (§6.2, §3.8). *Make conventions cheap to revise; distinguish "canonical" from "mandatory"; allow escape hatches.*
5. **Noisy/slow gate → bypass.** If it fires too often or too slowly, humans and agents `--no-verify` it (§4.4, §6.3). *High signal-to-noise, fast, and in the inner loop or it's dead.*
6. **Lives outside the daily workflow → ignored.** The Structure101 lesson (§6.4). *Must live where code is actually written — now including the AI-agent loop.*
7. **Advisory-only for agents → decays with context.** Prose rules provably weaken by length and session depth (§4.1). *For agents, the signal must be re-injected/enforced deterministically, not trusted to memory.*

---

## Personas and Their Jobs-to-Be-Done

| Persona | Core pain (evidence) | Job-to-be-done | What today fails to give them |
|---|---|---|---|
| **AI coding agent** | Holds no theory of the codebase; instruction files decay by length & session depth (§4.1); semantically duplicates helpers (§4.2) | "Tell me *which* of the existing patterns is canonical *here*, at the moment I'm writing, without me having to remember 200 rules." | Prose CLAUDE.md that rots; linters that catch style but not architecture |
| **New engineer onboarding** | 3–9 month ramp; can't tell which of 3 competing patterns is current; docs are stale (§2) | "Show me the house pattern for X so I stop guessing and stop getting nitpicked." | Wiki/ADR graveyards; tribal knowledge in seniors' heads |
| **Reviewer** | ~80% of review comments are convention/style; 6.4 hrs/week; AI floods the queue (§5) | "Stop making me re-litigate decided conventions; let me spend review on logic." | Manual review; Danger/lint cover mechanical rules only |
| **Departing-senior / tech lead** | Bus-factor knowledge loss; 1–2 yr recovery; conventions live in one head (§2) | "Externalize the codebase's theory so it survives me and scales past mentorship." | Readability-style human certification (bottleneck); docs nobody updates |
| **The codebase itself** | Erosion accrues imperceptibly → Big Ball of Mud → refactoring hopeless; 15× defects (§1) | "Detect drift early, while it's still cheap to fix, and keep new work on-pattern." | Architecture tools that live outside the workflow (Structure101) |

**The unifying JTBD:** *make the codebase's living theory — which patterns are canonical, right now, here — machine-readable, always-current, and enforced in the inner loop, without a human gate or a hand-written rule per convention.* Every existing mechanism nails one or two of those properties and drops the rest. That gap is the product.

---

## Source Index (verified URLs)

**Architecture erosion / drift:** [Li systematic mapping (Wiley)](https://onlinelibrary.wiley.com/doi/10.1002/smr.2423) · [Groningen thesis](https://research.rug.nl/en/publications/understanding-analysis-and-handling-of-software-architecture-eros/) · [Degradation survey 2025 (arXiv)](https://arxiv.org/pdf/2507.14547) · [Practitioners' perspective (arXiv)](https://arxiv.org/pdf/2103.11392) · [Automated violation ID (arXiv)](https://arxiv.org/pdf/2306.08616) · [Big Ball of Mud (DevIQ)](https://deviq.com/antipatterns/big-ball-of-mud/)
**Cost:** [Code Red (arXiv)](https://arxiv.org/pdf/2203.04374) · [InfoQ](https://www.infoq.com/articles/business-impact-code-quality/) · [CodeScene blog](https://codescene.com/blog/measuring-the-business-impact-of-low-code-quality)
**Onboarding / bus factor:** [DX onboarding time](https://newsletter.getdx.com/p/developer-onboarding-time) · [DX ramp-up 2026](https://newsletter.getdx.com/p/developer-ramp-up-time-continues) · [Bus Factor in Practice (arXiv)](https://arxiv.org/pdf/2202.01523) · [tribalhabits](https://tribalhabits.com/how-to-fix-knowledge-loss-when-developers-leave/) · [slite](https://slite.com/learn/tribal-knowledge)
**Governance mechanisms:** [Readability — Modern Descartes](https://www.moderndescartes.com/essays/readability/) · [Readability — HackerOne](https://www.pullrequest.com/blog/google-code-review-readability-certification/) · [ADR graveyard (Java Code Geeks)](https://www.javacodegeeks.com/2026/05/the-reason-most-architecture-decision-records-get-written-and-never-read-is-architectural-not-cultural.html) · [Fowler ADR](https://martinfowler.com/bliki/ArchitectureDecisionRecord.html) · [Design Docs at Google](https://www.industrialempathy.com/posts/design-docs-at-google/) · [Scaling via RFCs (Pragmatic Engineer)](https://blog.pragmaticengineer.com/scaling-engineering-teams-via-writing-things-down-rfcs/) · [CODEOWNERS/rubber-stamp](https://deepdocs.dev/code-review-in-github/) · [Danger.js](https://danger.systems/js/) · [Custom lint custodianship](https://medium.com/compass-true-north/linting-a-practical-guide-to-introducing-it-to-your-team-28e3605a0dc2) · [ArchUnit](https://medium.com/xebia-france/enforcing-architecture-decisions-with-archunit-4d8b9f61cf4a) · [Fitness functions (InfoQ)](https://www.infoq.com/articles/fitness-functions-architecture/) · [SWE at Google ch.3](https://abseil.io/resources/swe-book/html/ch03.html)
**AI-agent era:** [Writing a good CLAUDE.md (HumanLayer)](https://www.humanlayer.dev/blog/writing-a-good-claude-md) · [Claude Code best practices](https://code.claude.com/docs/en/best-practices) · [Linting config not agent instructions (Teal Larson)](https://www.teallarson.dev/blog/2026-03-27-dont-make-your-agent-file-a-linting-config) · [CLAUDE.md complete guide (Ghosh)](https://medium.com/@bijit211987/the-complete-guide-to-claude-md-memory-rules-loading-and-cross-tool-compression-97cc12ed037b) · [maketocreate 2026 guide](https://maketocreate.com/claude-md-best-practices-the-complete-2026-guide/) · [Lost in the middle](https://pristren.com/blog/lost-in-middle-attention-paper/) · [Boosting instruction following (arXiv)](https://arxiv.org/pdf/2510.14842) · [Semantic duplication (Pharaoh)](https://pharaoh.so/blog/prevent-duplicate-functions-ai-coding/) · [4 ways AI breaks your repo (New Stack)](https://thenewstack.io/the-4-ways-ai-code-is-breaking-your-repo-and-how-to-fix-it/) · [Ask HN: agents repeat mistakes](https://news.ycombinator.com/item?id=47399209) · [Code Agent Orchestra (Osmani)](https://addyosmani.com/blog/code-agent-orchestra/) · [SDD guide (BCMS)](https://thebcms.com/blog/spec-driven-development) · [Spec Kit Agents (arXiv)](https://arxiv.org/html/2604.05278v1)
**Review economics:** [Modern code review taxonomy (arXiv)](https://arxiv.org/pdf/2103.08777) · [Useful reviews at Microsoft](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/bosu2015useful.pdf) · [AI breaking code review (Codacy)](https://blog.codacy.com/ai-breaking-code-review-how-engineering-teams-survive-pr-bottleneck) · [Review bottleneck AI era](https://asyncsquadlabs.com/blog/code-review-bottleneck-ai-era/) · [Ignored security PRs (Pixee)](https://www.pixee.ai/blog/merge-rate-problem-security-prs-ignored)
**Counter-evidence:** [Wrong Abstraction (Metz)](https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction) · [Does review speed matter (arXiv)](https://arxiv.org/pdf/2311.02489) · [When reviews aren't mandatory (Test Double)](https://testdouble.com/insights/when-code-reviews-arent-mandatory) · [Design docs considered harmful](https://www.lucasfcosta.com/blog/design-docs) · [ACC accuracy (Wiley)](https://onlinelibrary.wiley.com/doi/full/10.1002/spe.2421) · [Sonar acquires Structure101](https://www.sonarsource.com/structure101/)
