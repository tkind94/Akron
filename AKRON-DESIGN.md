# AKRON-DESIGN

42 testable interaction-design rules for Akron's `explore` UI. Each is a check,
not a vibe: an agent touching `explore.html` (or the served UI in `explore.rs`)
must be able to verify or falsify it against the diff. Tag = primary source.

The core thesis (Victor, "Magic Ink"): `explore` is **information software** —
its purpose is understanding, not manipulation. "Unless it is enjoyable or
educational in and of itself, interaction is an essentially negative aspect of
information software." The map is the primary interface; interaction is the
fallback for when the graphic couldn't infer what the user needed. Context
comes from environment → history → interaction, in that order of preference.

These rules complement, never override, DESIGN.md §1.2: deterministic channels
are the only source of similarity numbers shown beside code; the model lays
out and ranks, it never produces a displayed score.

## A. Timing & feedback (the perceptual contract)

1. **Every hover response renders in < 100 ms.** Below 100 ms cause and effect
   feel fused; above it the system feels like a separate actor. Hover
   hit-testing runs per rAF — keep it there; never gate hover identity behind
   a network fetch. *(Card/Robertson/Mackinlay; Nielsen 0.1 s)*
2. **Any action that can't complete in < 1 s must show progress within
   100 ms.** `/api/explain`, `/api/sublayout`, `/api/compare` may exceed 1 s —
   each must paint a pending state immediately (skeleton, spinner-in-place, or
   dimmed target), never a frozen frame. *(Nielsen 1 s)*
3. **Anything over 10 s must be cancellable and show elapsed progress.**
   *(Nielsen 10 s)*
4. **UI transitions are ≤ 300 ms, ideally ~150 ms.** Never ship a 500 ms+
   transition on anything triggered repeatedly. *(Kowalski; Rauno)*
5. **Never animate a high-frequency interaction** (keyboard nav through the
   hitlist, selection changes). Animation there reads as lag. *(Kowalski;
   Rauno)*
6. **Use ease-out for user-initiated motion** (starts fast → feels
   responsive); reserve symmetric ease/spring for ambient motion. *(Kowalski)*
7. **Animate only `transform` and `opacity`.** Layout-property animation drops
   frames on a 2000-point canvas. Hold 60 fps or cut the animation.
   *(Kowalski)*
8. **Every animation is interruptible mid-flight** and retargets from the
   current value on re-hover/re-click — never queues or blocks input.
   *(Rauno; Kowalski)*
9. **`prefers-reduced-motion` kills all non-essential transition.** Reduced
   motion must never remove information, only decoration. *(WCAG; Kowalski)*

## B. Direct manipulation & the two gulfs

10. **The result of every action is visible inside the viewport that
    triggered it.** Drill from a point → the drilled layout appears where the
    point was; compare from a row → the overlay anchors to that row.
    *(Hutchins/Hollan/Norman — gulf of evaluation)*
11. **Continuous representation of the object of interest:** the selected
    symbol stays visibly marked (ring, crumb, panel) at all times.
    *(Shneiderman)*
12. **Physical action over typed syntax where a graphic exists.** Clicking a
    point/label/edge beats entering an identifier. *(Victor; Shneiderman)*
13. **Every action is incremental, rapid, and reversible.** No action strands
    the user in a state with no visible way back. *(Shneiderman)*
14. **Close the gulf of execution: the thing to do next is visible, not
    remembered.** Available actions advertise themselves on the object.
    *(Hutchins/Hollan/Norman; Norman)*
15. **No modes without a constant, visible mode indicator.** A user is never
    surprised by what a click does. *(Norman; Shneiderman)*

## C. Affordances & signifiers (making the canvas legible)

16. **If it's clickable, it signals clickability before interaction** — not
    only on hover. No sweep-to-discover. *(Norman — signifiers)*
17. **Hover reveals identity; it never is the action.** Tooltips are never hit
    targets. Hovering discloses, clicking commits. *(Norman; Rauno)*
18. **Cursor shape is a signifier.** `pointer` on clickable, `default` on
    label-only, `grab`/`grabbing` for pan. A wrong cursor is a broken promise.
    *(Rauno; Norman)*
19. **Hit target ≥ 24×24 px even when the drawn mark is smaller.** A dot's
    clickable radius exceeds its drawn radius. *(Fitts; WCAG 2.5.8)*
20. **Frequent targets live at edges/corners** where the pointer can't
    overshoot. *(Fitts; Rauno)*
21. **Disabled ≠ invisible, and disabled explains itself** (dimmed, with a
    hover reason). Silently missing controls destroy discoverability.
    *(Norman)*
22. **Feedback is immediate and proportional to the act.** Every input
    produces a perceptible, matched output; silence reads as broken.
    *(Norman)*

## D. Information graphics (Tufte, applied to the map)

23. **Maximize data-ink: every pixel that isn't data or a signifier is
    suspect.** *(Tufte)*
24. **No number appears without a traceable source.** Every on-screen value
    (PCA variance, Jaccard, edge weight) links or hovers to how it was
    computed. Generalizes the honesty gates. *(Tufte; DESIGN.md §1.2)*
25. **Encode with position first, then length, then color/area last.** Color
    is for category, not magnitude, unless legended. *(Cleveland/McGill;
    Tufte)*
26. **Label directly; avoid legends the eye must ping-pong to.** Edge meaning
    appears on/near the edge. *(Tufte; Rauno)*
27. **Small multiples for comparison, not sequential toggling.** Side by side
    at identical scale; the eye diffs, memory doesn't. *(Tufte; Victor)*
28. **The legend states the encoding honestly, including its limits.**
    "solid — identical after whitespace collapse · dotted — aligned, differs"
    is the model. *(Tufte; honesty gates)*
29. **Sparkline-density where shape matters more than the digit.** *(Tufte)*

## E. Context-sensitivity & minimal interaction (Victor)

30. **Default to the most likely view; interaction is the correction, not the
    entry fee.** No setup clicks before the user sees data. *(Victor)*
31. **Infer from environment/history before asking** (recently-touched files,
    the diff under review, coupling clusters). Interaction is the last
    resort. *(Victor)*
32. **Interaction that only navigates is a cost — minimize it.** Audit every
    click: does it reveal information, or merely move to where information
    is? Collapse pure-navigation clicks. *(Victor)*
33. **Overview-then-detail-on-demand over drill-only.** Detail expands in
    place without losing the overview. *(Victor; Shneiderman)*
34. **The tool looks outside itself.** Never make the user re-state what the
    environment already knows (current repo, open diff, last query).
    *(Victor)*

## F. Craft details (the "alive" feel)

35. **Committing actions fire at gesture end; lightweight reveals fire
    during.** A half-gesture never commits. *(Rauno)*
36. **Peek without commit.** Reveal-on-hover shows full-fidelity info without
    changing state; only an explicit click commits. *(Rauno)*
37. **Motion communicates spatial origin.** Panels/overlays emerge from the
    element that spawned them. *(Rauno)*
38. **Optimistic feedback on cheap, reversible acts.** Reflect
    selection/hover instantly; reconcile async results when they land.
    *(Rauno; Kowalski)*
39. **A pointer leaving a target cancels its transient state cleanly** — no
    orphaned tooltips, no stuck highlights. *(Rauno)*

## G. Language (copy is interaction)

40. **Name things by what the user controls, in the user's vocabulary.**
    "compare with…", "shared vocabulary" — never internal type or endpoint
    names. *(Norman)*
41. **An action keeps its name through the whole flow.** Button, panel title,
    and result state use the same word — no synonym drift. *(frontend-design)*
42. **Empty and error states give direction, in the tool's voice, and never
    apologize.** "no symbols match — widen the query" beats "Sorry, no
    results." Errors state what happened and how to fix it, traceably.
    *(Nielsen heuristics)*

## Operationalizing

- Any agent touching `explore.html` loads this file first and cites rule
  numbers in its report for every interaction it adds or changes.
- Review treats each numbered rule as a checkable assertion against the diff
  ("this change added a hover that fetches before painting → violates #1").
- Sources: Victor "Magic Ink" (2006); Hutchins/Hollan/Norman "Direct
  Manipulation Interfaces" (1985); Shneiderman (1983); Norman, *The Design of
  Everyday Things*; Tufte, *The Visual Display of Quantitative Information*;
  Card/Robertson/Mackinlay (1991) + Nielsen response-time limits; Fitts's
  law / WCAG 2.5.8; rauno.me "Invisible Details of Interaction Design";
  emilkowal.ski "Great Animations".
