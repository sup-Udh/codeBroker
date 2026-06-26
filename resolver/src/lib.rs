//! # Universal Resolver
//!
//! The single component responsible for turning arbitrary user/tool input
//! ("main.py", "simulate", "authentication", "frontend/app/page.tsx", a
//! natural-language phrase, ...) into one of a fixed set of canonical
//! entities (see [`types::ResolvedEntity`]).
//!
//! No other crate may re-implement matching, ranking, confidence scoring, or
//! ambiguity detection for these entity types — they call into this crate
//! and react to whichever [`types::ResolvedEntity`] variant comes back.
//! `query`'s lower-level functions (`discover_subsystem`, `find_symbol_candidates`,
//! embedding lookup, ...) remain the underlying data access; this crate is
//! the decision layer on top of them.
//!
//! ## Known limitations (see `benchmark/improvements/universal_resolver.md`)
//! - No alias table exists yet, so the "Alias Match" pipeline stage is a
//!   documented no-op rather than a fabricated implementation.
//! - The Subsystem stage's underlying algorithm (`query::subsystem::discover_subsystem`)
//!   is unchanged from before this phase; this crate only owns the
//!   resolved-or-not decision around it, not its internal seed/expansion logic.

pub mod pipeline;
pub mod types;

pub use pipeline::{resolve_any, resolve_path, resolve_search, resolve_subsystem, resolve_symbol};
pub use types::{
    AmbiguousMatch, Candidate, Confidence, ConfidenceLabel, EntityType, NotFound,
    ResolvedDirectory, ResolvedEntity, ResolvedFeature, ResolvedFile, ResolvedSubsystem,
    ResolvedSymbol,
};
