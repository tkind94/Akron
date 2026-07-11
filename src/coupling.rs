//! Change-coupling mining (TKI-62): file-level co-change from git history.
//! Confidence = shared_revs / revs(entity); degree = shared / avg-revs
//! (code-maat form). Noise filters: changeset-size cap, path excludes,
//! min-revision floors. Files below the history gate report "insufficient
//! history" rather than a false-precise number. Deterministic: sorted
//! output, recomputed at HEAD.
//!
//! Stub — implementation lands with TKI-62.
