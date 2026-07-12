# Akron — User Journeys

**2026-07-03 · the human contract.** Every feature must serve a journey on this
page; every journey names the person, the question, the command, and what they
see. Status is marked honestly: ✅ works today · 🟡 partial · 🔜 planned (issue).

The CLI-surface principles at the bottom are binding for TKI-16/17.

---

## Personas

| Persona | Their question |
|---|---|
| **Engineer new to the repo** | "I found three ways to do this here — which one do I copy?" |
| **Steward / tech lead** | "Where is duplication and drift accumulating? What changed since last month?" |
| **Reviewer** | "Does this PR reinvent something we already have?" |
| **Team (decision moment)** | "We picked variant B. How do we record that once, so it's never re-litigated?" |
| **AI coding agent** | "Before I write this function: which house pattern applies?" |

## Journeys

### J1 — "How do I get a first read on where a repo repeats itself?" superseded by explore (TKI-50)
Kept below as history — `scan`'s digest/`--full` text this journey describes
was decommissioned in TKI-50 (CEO: "Yep, decommission scan"; `scan` is now a
JSON-only machine contract). `akron explore`'s **map** view (J11) answers
this today: a neighborhood-preserving layout where repeated shapes read as
visual clustering.
```
akron scan .
```
Default: a one-screen digest — counts for every query, the top handful of
repeated-shape and dead-vs-growing findings ranked by size · spread ·
activity, and one line telling you how to go deeper. `--full` prints every
cluster/group (the firehose); `--only <section>` scopes either view to one
query.

### J2 — "How do I find where the same shape is written more than once?" superseded by explore (TKI-50)
Kept below as history — decommissioned with `scan`'s digest in TKI-50.
`akron explore`'s **anchor** view (J11) answers this today: select a symbol
and its x-axis (structure similarity) surfaces its near-duplicates directly.
Repeated-shapes section: cross-file, non-test clusters rank first. A
`_window` helper appearing 7× across 3 files of a private domain-vocabulary
repo is the canonical hit. `--only repeated` scopes the digest to just this
query.

### J3 — "How do I find two implementations that do the same thing, built differently?" superseded by explore (TKI-50)
Kept below as history — decommissioned with `scan`'s digest in TKI-50.
`akron explore`'s **anchor** view (J11) answers this today: a symbol high on
the y-axis (vocabulary similarity) but low on the x-axis (structure
similarity) to the selection is exactly this quadrant.
Shared-vocabulary/divergent-shape section: caller/callee pairs suppressed
(and counted in the funnel, so nothing disappears silently). The two
independent Postgres-connection styles are the canonical hit. **Opt-in
(TKI-45):** this reads as an assertion ("these compete") when surfaced
unprompted, so it left the default digest and default `--json` — run
`akron scan . --only competing` (or `--full`/`--html`, which still show
everything) to see it. Same underlying query and funnel as before.

### J4 — "Which of two variants is dead, and which is growing?" superseded by explore (TKI-50)
Kept below as history — decommissioned with `scan`'s digest in TKI-50.
`akron explore`'s **time** view (J11) answers this today: the real date axis
surfaces dead-vs-growing pairs directly.
Every cluster shows `span: 2026-05-31 → 2026-07-02 · growing`; the
deprecated-candidates query pairs dead clusters against growing role-twins —
a git-history measurement (DESIGN.md §1: "deprecation is a measurement, not
a status field"), not a judgment about which one to use. The family view
(TKI-9) groups shape variants together, but a member-level precision grade
(R&D archive validation/family-membership.md) measured 0.98–0.99 on parallel-adapter/
copy-paste corpora and only 0.70 on a foreign, heterogeneous one — below the
bar on the case that matters most.
**Demoted (TKI-35), opt-in (TKI-45):** excluded from the default digest and
default `--html` report; still computed and available via `--only families`,
`--full`, `--json --only families`, and `--html --families`.
**Earns back (TKI-36):** an httpx re-grade ≥0.80 restores it to default.

### J5 — "How do I share findings with my team?" ❌ removed with --html (TKI-50)
Kept below as history — the `--html` report this journey describes was
decommissioned in TKI-50 along with `scan`'s entire human surface. A
shareable export may return as an `explore` feature if demand shows.
Today: `--json` for tooling. **Planned (TKI-17):** `akron scan --html report.html`
— a single self-contained file with the digest up top, findings expandable,
code excerpts inline, readable by someone who has never run the tool. This is
the artifact a steward drops in Slack.

### J6 — "How do we record a decision once?" ❌ removed 2026-07-06 (pivot 2: exploration-only; judgment surfaces failed the usefulness bar)
Kept below as history — the verdict store this journey describes (`akron
ratify`, `.akron/verdicts/`) was removed in TKI-45.
```
akron ratify . F1 --verdict canonical --reason "pooled connections; see incident 2026-03"
```
Ratify a finding by its stable ref (`F1`/`R3` from `akron scan --full`). The
verdict is written to `.akron/verdicts/<slug>.yml` (human-editable,
git-reviewable) anchored to the pattern's **content** — the core's Merkle roots
+ WL signature, never a file path. Every later `akron scan` re-binds it by that
content (exact through renames/refactors, fuzzy through moderate edits), marks
the family/cluster line (`✓ canonical` / `✗ deprecated`), and — this is the
point — **flags a conflict loudest of all** when a deprecated pattern keeps
growing or a canonical one dies. A verdict whose pattern is gone is reported
**expired**, never deleted. Never re-litigated in review; never silently rots.
**Now (🟡):** `akron check` (J7/TKI-11) will consume these verdicts at commit
time; the canonical-dying-while-twin-grows cross-reference sharpens once the
deprecated query runs at the family altitude.

### J7 — "How do I stop regressions at commit time?" ❌ removed 2026-07-06 (pivot 2: exploration-only; judgment surfaces failed the usefulness bar)
Kept below as history — `akron check` (the commit-time gate this journey
describes) was removed in TKI-45.
```
akron check            # advisory: print findings, exit 0
akron check --strict   # exit 1 only on an EXACT deprecated match
```
`akron check` fingerprints the symbols in files with staged changes (git index
vs HEAD), re-binds the repo's verdicts + landscape through the same engine a
scan uses, and reports per finding: a deprecated match names the verdict, its
reason, and the canonical alternative's `file:line`; a reinvented helper points
at the existing one. The error message teaches. Advisory by default; `--strict`
exits 1 **only** when a staged symbol is an *exact* (Merkle-level) reproduction
of a deprecated shape **that the commit introduces** — a shape already present
in the file at HEAD is a neighbor of the edit, not the edit, and warns
(`pre-existing in this file — not gated`). Wrong blocks breed `--no-verify`
culture, so precision beats recall: fuzzy matches, conflicts, and threshold
drift warn but never gate. Exit codes are a hook contract: 0 no gate, 1 gate
fired, 2 operational error (git refused, broken verdict store). `--json` gives
a versioned surface (`akron.check/v1`).

### J8 — "How does my coding agent use this?" ❌ removed 2026-07-06 (pivot 2: exploration-only; judgment surfaces failed the usefulness bar)
Kept below as history — the `check --strict --json` gate and the propose-a-
`ratify` moment this journey describes were removed in TKI-45.
`skills/akron/SKILL.md` still covers the surviving find/explain/scan-json
moments.
No MCP server (owner decision 2026-07-03: "I can't see what an MCP would get
us that just a skill and a cli tool wouldn't"). `skills/akron/SKILL.md` is the
agent surface: it drops into a coding agent's skills directory and instructs
it to run `akron scan --json` before writing a new helper (reuse a canonical
match, avoid a deprecated one), `akron explain` before modifying an
unfamiliar symbol (J9), `akron find` when searching for where something is
handled (J10), `akron check --strict --json` before
committing (BLOCK on an exact-new deprecated match, WARN otherwise), and to
*propose* — never silently run — `akron ratify` when a human makes the call.
Same engine, same answers, no prose file to rot. `scan --json`'s schema
version and embedded finding refs (`ref` fields, `R#`/`F#`/`C#`, positional
within each array — TKI-50 dropped the `--full` text surface these used to be
cross-checked against) closed this gap (TKI-27); the JSON contract is stable
enough to script against today.
**Gap:** `competing` findings (J3) still carry no ratify ref, so an agent can
flag a competing pair but not resolve it end-to-end (see the skill's own
"Gaps" section). 🟡 until that closes.

### J9 — "What is this symbol, and what's it tangled with?" ✅ (TKI-40)
```
akron explain <path> <target>
```
One card, read off the same analysis `scan` already computes: near-clones
(with cosine), role twins (shared vocabulary, different shape), callers and
callees (import-aware), and family membership — no new detection, just
single-symbol grain on data the engine already had. Graded 8 wins / 3
ties / 1 loss against ~5 minutes of `rg` + reading, on 12 symbols picked
cold across three corpora (R&D archive validation/explain-eval.md); every "is this the
one to use" case tested was a clear win. Ambiguous callers (a generic base
name like `post` called on an unresolvable object) are excluded rather than
guessed — `callers 0` means none resolved reliably, not none exist.
**Gap:** dict/variable-dispatched calls (`handlers[name](...)`) are
invisible to the caller/callee pass — the eval's one remaining loss.

### J10 — "How do I find where something is handled, in my own words?" ✅ (TKI-41)
```
akron find <path> "<question>"
```
Embeds the question and every symbol in the repo, ranks by cosine, prints
`file:line qname` for the closest matches. On a private domain-vocabulary
codebase, P@5 0.68 against a best-faith grep's 0.36 — it recovers matches a
lexical search structurally cannot, because the reader's words and the
codebase's vocabulary diverge. On httpx (plain-English identifiers) it wins
by a thin margin (0.50 vs 0.40; the previous model lost here): still a
complement to lexical search, not a replacement. Ranking only — a pinned
local embedding model may reorder `find`'s results; nothing else in the
tool reads its output (DESIGN.md §1.2). Model: embeddinggemma-300m-q,
331 MB, pulled sha256-pinned on first use under its own license terms; test
symbols dropped from ranking unless `--tests`.

### J11 — "How do I see the shape of a codebase at a glance?" ✅ (TKI-47)
```
akron explore .
```
An interactive local map served on localhost: a neighborhood-preserving
layout of the full embedding space (**map**), similarity-to-anchor axes
straight from channels A/B (**anchor**), a real date axis (**time**), and
free dimension pickers (**axes**). Click a point → the J9 card; the search
box is J10 over the live index with hits highlighted. Derived per run, held
in memory, nothing written into the repo; layout and PCA deterministic.
Tests hidden by default — on measured repos they smear every projection.

**Drill-down (TKI-54):** click a dir label on the map, the ▸ on a legend
chip, or a path segment on the card (that is a point's route to its file)
to re-run the layout over just
that subset — its internal structure gets the whole plane, colors re-key
to subdirectories (classes inside a file). The topbar breadcrumb always
answers "where am I"; every ancestor is clickable and climbs to that
level, selecting anything outside the drill exits to the whole-repo map.
Deterministic like the launch layout (seeded from the global coordinates).
**Unselect (TKI-80):** clicking empty canvas, clicking the already-selected
dot again, or the panel's own ✕ all clear the selection and close the
panel — exploration needs an easy way back to nothing selected, not only
a way in. Escape's ladder unwinds transient state innermost-first (search
hitlist, compare pick mode, the compare overlay, the newest pinned board)
and then, with nothing transient left, clears the selection. It no longer
climbs the drill or walks back-history: those already have dedicated,
visible controls — the breadcrumb's ancestor segments and the "← back"
chip — so Escape's one job (get back to nothing) stays legible instead of
doubling as a silent, stateful back button.
**Living boards (TKI-68):** rest on a point and a peek card blooms with
its first lines; click pins it as a board card that rides pan/zoom,
anchored to its dot (soft cap 6 — the oldest minimizes to a chip). Pinned
neighbors connect card-to-card; hovering a line names the pair and their
shared vocabulary — terms, never a number (TKI-79 moved the labels off
the map and into the hover tip); click a line for the side-by-side, or
drag one card's header onto another to wire an arbitrary pair. Escape
dismisses the newest card; the side panel stays the deep dossier.
**Semantic zoom (TKI-69):** zoom is disclosure — a dot with room around
it sprouts its name, then its signature; deeper still it blooms an
ambient code card (first lines, capped by what the viewport holds, never
by repo size — and a card that would land on another skips its turn, so
dense regions thin to chips instead of walling up; TKI-78/79), and
neighbor cards connect with lines whose shared vocabulary shows on hover.
Clicking always falls through to the dot (pin + select as above); labels
and path segments are the only way to drill — an earlier zoom-triggered
auto-drill was removed (TKI-80: CEO review — hard to trigger on purpose,
too easy to trigger by accident).
**Guidance:** with nothing selected the panel lists factual starting
points for the current scope — largest dirs/files, most-called symbols,
last touched — each row a click; a selected card ends with its 5 nearest
layout neighbors, closest first, visited ones marked rather than dropped
from the list. Counts and dates only; the map offers places to start, it
never judges them.
**Panel → map hover linking (TKI-70):** every panel row naming a symbol —
callers, callees, clones, role twins, nearest-existing, nearest layout
neighbors, most-called, last-touched — highlights that symbol's dot on
the map while the row is hovered (a dashed halo with crosshair ticks,
distinct from the selection/search/branch rings). A row whose symbol
isn't drawn in the current scope says so on hover instead of highlighting
anything; it never auto-pans or auto-drills.
**Calls channel (TKI-72, chains in TKI-79):** the `calls` dropdown in the
topbar overlays directed call arrows — name-resolved static calls, direct
only, the same import-aware edge set the card's callers/callees lists
show — walked 1, 2, or 3 hops out from the selected symbol in both
directions (on at one hop by default; `off` stays a choice for a quieter
map), and between pinned board cards. Callee-direction arrows are amber, caller-direction violet, and
each hop past the first draws fainter, so a chain reads as a gradient
away from the selection; hop-2+ endpoints wear a thin ring in their
direction's hue. Arrows are structure, not similarity: arrowheads and
their own hues, never confusable with the layout-neighbor edges (which
recede while a call graph is drawn), and never drawn all-pairs at repo
zoom. Hovering an arrow names the pair, the encoding, and its hop
distance; an ambiguous same-name call stays dashed at every depth; capped
fan-outs say how many edges are not drawn. Dynamic dispatch
(`handlers[name](...)`) stays invisible, exactly as J9 states. Connector
labels — the vocabulary terms between cards, and "calls" — live in the
hover tip rather than floating at rest: at rest the map draws no term
labels at all.

## CLI-surface principles (binding)

1. **One screen by default, depth on request.** `find` and `explain` answer
   in one screen; `--top N` (find) and `--only <section>` (scan `--json`)
   widen scope. Nobody is punished for running the tool casually.
2. **Rank by size · spread · activity**: cross-file beats same-file, non-test
   beats test, big beats small, growing beats stale. The top finding is the
   most measurable one, not the most urgent-sounding one.
3. **Color when TTY** (respect NO_COLOR and pipes): severity and section
   structure carried by weight and hue, never by color alone.
4. **Every empty result explains itself** — funnels are already the norm; the
   digest keeps them.
5. **The error message teaches**: any finding names the pattern, the evidence,
   and the concrete file:line to look at. No finding without a next action.
6. **Stable machine surface**: `--json` schema is versioned; tools get
   structure (`scan --json`), humans get `explore` — both from one engine.
