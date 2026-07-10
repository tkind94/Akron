# Competing-pattern ratification (design sketch, not implemented)

`competing[]` entries now carry `"ref": "C<n>"` (TKI-27), matching the `[C#]`
tag `--full` prints. `ratify` still only accepts `F`/`R` (`parse_ref` in
verdict.rs) — this sketches what closing that gap would take.

## How it would anchor

A `CompetingGroup` has no single "core": its members are shape-DIVERGENT by
construction (that's what makes them competing, not duplicate). So `ratify
C1` can't anchor the whole group the way `anchor_of_cluster`/`anchor_of_family`
do today. Ratifying a competing pair means ratifying one SIDE at a time, same
shape as any other verdict:

    akron ratify . C1a --verdict deprecated --reason "..."
    akron ratify . C1b --verdict canonical --reason "..." --over C1a

`C1a`/`C1b` would name one member of group C1 (or the repeated/family cluster
it already belongs to, if any), anchored via the existing `anchor_of_cluster`
on that member's Merkle root + WL signature — no new anchor machinery for the
2-member case.

## What blocks it today

- Groups aren't always 2-sided: the chained-union-find guard can merge 3+
  mutually-competing members (flask's 3 registration mechanisms) with no
  natural binary split — a `C1a`/`C1b` scheme needs a rule for >2 members.
- `parse_ref` only parses a bare letter+number; a sub-ref (`C1a`) needs new
  parsing and a member-selection story (by qname? by position in the group?).
- `bind_all` only tracks `r_anchors`/`f_anchors` and their `_hits` passes; a
  third `c_hits` pass and `competing_marks` output would need the same
  treatment, threaded through `report.rs`, `digest.rs`, and `html.rs`'s
  mark-rendering call sites.

## Rough size

Sub-ref parsing + member-anchor extraction + a third `bind_all` pass + mark
rendering across three files + tests: ~150-250 LOC, comparable to the
original verdict-store build (TKI-10). Worth its own issue.
