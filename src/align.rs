//! Similar-code alignment (TKI-61): deterministic region matching between
//! two functions for the side-by-side view. Merkle-equal subtrees are exact
//! anchors (tier 1); statement-level then token-level DP alignment fills
//! the near-miss stretches between them (tier 2); everything else is
//! unmatched. No model involvement — the highlight is pure structure.
//!
//! Stub — implementation lands with TKI-61.
