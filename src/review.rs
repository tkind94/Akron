//! `akron review` (TKI-63): a deterministic evidence surface for reviewing
//! a diff range. For each changed symbol: where it sits (module/dir),
//! the nearest existing implementations (embedding-ranked, annotated with
//! deterministic structure/vocabulary cosines only), the file's co-change
//! partners outside the diff ("also review these"), and pattern prevalence.
//! Facts only — no verdicts, no scores beside code.
//!
//! Stub — implementation lands with TKI-63.
