//! Canonical entity types and the confidence model every resolution stage
//! shares. These are the only vocabulary the rest of CodeBroker is allowed to
//! speak in when describing "what did the user's input turn out to mean" —
//! no MCP tool defines its own notion of a match, a confidence label, or an
//! ambiguity response.

use serde::{Deserialize, Serialize};

/// The coarse kind of thing a query resolved to. Every `ResolvedEntity`
/// variant maps to exactly one of these (see `ResolvedEntity::entity_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Symbol,
    File,
    Directory,
    Subsystem,
    Feature,
    /// A refinement of `Symbol`: the resolved symbol is also flagged
    /// `is_entrypoint` (a route/page/layout). Surfaced as a distinct type so
    /// a caller asking "what does this app expose" can filter on it without
    /// re-deriving entrypoint-ness itself.
    Entrypoint,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::Symbol => "symbol",
            EntityType::File => "file",
            EntityType::Directory => "directory",
            EntityType::Subsystem => "subsystem",
            EntityType::Feature => "feature",
            EntityType::Entrypoint => "entrypoint",
        }
    }
}

/// The single confidence model. Every stage constructs one of these via the
/// `from_*` helpers below instead of inventing its own score/label — that is
/// what makes confidence "computed once" (Phase 5) rather than recomputed
/// per-tool with potentially different thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Confidence {
    /// 0-100. Not exposed as a raw probability — it's a relative ranking
    /// signal, comparable only against other `Confidence` values produced by
    /// this module.
    pub score: u8,
    pub label: ConfidenceLabel,
    /// Human-readable reasons that produced this score, surfaced to callers
    /// so a confidence label is never just an unexplained number.
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLabel {
    High,
    Medium,
    Low,
}

impl ConfidenceLabel {
    /// The single score->label mapping. Every stage's `Confidence` is built
    /// through this, so "High" always means the same thing (score >= 80)
    /// regardless of which stage or which tool produced it.
    fn from_score(score: u8) -> Self {
        if score >= 80 {
            ConfidenceLabel::High
        } else if score >= 50 {
            ConfidenceLabel::Medium
        } else {
            ConfidenceLabel::Low
        }
    }
}

impl Confidence {
    pub fn new(score: u8, reasons: Vec<String>) -> Self {
        Confidence {
            score,
            label: ConfidenceLabel::from_score(score),
            reasons,
        }
    }

    pub fn exact(reason: &str) -> Self {
        Confidence::new(100, vec![reason.to_string()])
    }

    pub fn high(score: u8, reason: &str) -> Self {
        Confidence::new(score.max(80), vec![reason.to_string()])
    }

    pub fn medium(score: u8, reason: &str) -> Self {
        Confidence::new(score.clamp(50, 79), vec![reason.to_string()])
    }

    pub fn low(score: u8, reason: &str) -> Self {
        Confidence::new(score.min(49), vec![reason.to_string()])
    }
}

/// One candidate in an `Ambiguous` response, or the resolved entity's own
/// identity once disambiguated. Carries enough to both render a "did you mean
/// one of these" listing and, once a caller picks one, re-resolve
/// deterministically via `file_path`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub entity_type: EntityType,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub start_line: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSymbol {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub is_entrypoint: bool,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFile {
    pub file_path: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedDirectory {
    pub directory_path: String,
    pub file_count: usize,
    pub sample_files: Vec<String>,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSubsystem {
    pub name: String,
    pub file_count: usize,
    pub symbol_count: usize,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFeature {
    pub concept: String,
    pub matching_symbols: Vec<Candidate>,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbiguousMatch {
    pub query: String,
    pub candidates: Vec<Candidate>,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotFound {
    pub query: String,
    pub reason: String,
    /// Which stages were attempted, in order, so a caller (or a human
    /// debugging a NotFound) can see exactly what was tried instead of
    /// guessing whether the resolver even looked at e.g. semantic search.
    pub stages_tried: Vec<String>,
}

/// The resolver's entire output vocabulary. Every `resolve*` function returns
/// exactly one of these — there is no other shape a caller needs to handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "resolved_as", rename_all = "snake_case")]
pub enum ResolvedEntity {
    Symbol(ResolvedSymbol),
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    Subsystem(ResolvedSubsystem),
    Feature(ResolvedFeature),
    Ambiguous(AmbiguousMatch),
    NotFound(NotFound),
}

impl ResolvedEntity {
    pub fn entity_type(&self) -> Option<EntityType> {
        match self {
            ResolvedEntity::Symbol(s) if s.is_entrypoint => Some(EntityType::Entrypoint),
            ResolvedEntity::Symbol(_) => Some(EntityType::Symbol),
            ResolvedEntity::File(_) => Some(EntityType::File),
            ResolvedEntity::Directory(_) => Some(EntityType::Directory),
            ResolvedEntity::Subsystem(_) => Some(EntityType::Subsystem),
            ResolvedEntity::Feature(_) => Some(EntityType::Feature),
            ResolvedEntity::Ambiguous(_) | ResolvedEntity::NotFound(_) => None,
        }
    }

    pub fn is_ambiguous(&self) -> bool {
        matches!(self, ResolvedEntity::Ambiguous(_))
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, ResolvedEntity::NotFound(_))
    }

    /// Resolved-symbol convenience accessor, used by callers (most MCP
    /// handlers) that asked the resolver for a `Symbol` specifically and want
    /// `(name, file_path)` to hand to the existing query-layer functions.
    pub fn as_symbol(&self) -> Option<&ResolvedSymbol> {
        match self {
            ResolvedEntity::Symbol(s) => Some(s),
            _ => None,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// Pretty-printed JSON string — what every MCP handler returns for a
    /// non-Symbol or error outcome, so `Ambiguous`/`NotFound` are rendered
    /// identically everywhere instead of each tool hand-rolling its own
    /// message text.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}
