use crate::config::EmbeddingsConfig;
use crate::embedder::{embedder_from_config, Embedder};
use crate::vector_search::{cached_vector_search, VectorSearch};
use query::engine::{SearchMode, SearchResult};
use std::sync::{Arc, Mutex};
use storage::Database;

/// What `search_codebase` ultimately reports. Mirrors
/// `resolver::resolve_search`'s tuple plus the semantic-availability fields
/// the MCP payload exposes. `semantic_search_available`/`degraded_reason`
/// are None for purely lexical modes (semantic retrieval wasn't attempted,
/// so claiming either true or false would be misleading).
pub struct SearchOutcome {
    pub results: Vec<SearchResult>,
    pub total_occurrences: usize,
    pub files_matched: usize,
    pub truncated: bool,
    pub reason: Option<String>,
    pub semantic_search_available: Option<bool>,
    pub semantic_degraded_reason: Option<String>,
}

/// The one embedder instance the process keeps around, keyed by model_id so
/// a config change is picked up. Matters most for `LocalEmbedder`, whose
/// first `embed` pays a model load — rebuilding it per query would pay that
/// on every semantic search.
static EMBEDDER_CACHE: Mutex<Option<(String, Arc<dyn Embedder>)>> = Mutex::new(None);

fn cached_embedder(config: &EmbeddingsConfig) -> Result<Arc<dyn Embedder>, String> {
    let model_id = config.model_id();
    let mut guard = EMBEDDER_CACHE
        .lock()
        .map_err(|_| "embedder cache poisoned by a previous panic".to_string())?;
    if let Some((cached_id, embedder)) = guard.as_ref() {
        if *cached_id == model_id {
            return Ok(Arc::clone(embedder));
        }
    }
    let embedder: Arc<dyn Embedder> = Arc::from(embedder_from_config(config)?);
    *guard = Some((model_id, Arc::clone(&embedder)));
    Ok(embedder)
}

/// Embedding-only retrieval: embed the query, brute-force cosine top-k over
/// the in-memory matrix, map to `SearchResult`s labeled
/// `Semantic (cosine 0.83)`. Any failure is a degradation reason for the
/// caller to surface — never a search failure.
fn semantic_leg(
    db: &Database,
    keyword: &str,
    path_scope: Option<&str>,
    k: usize,
) -> Result<Vec<SearchResult>, String> {
    let config = EmbeddingsConfig::load(&db.project_root);
    let model_id = config.model_id();
    let index = cached_vector_search(db, &model_id)?;
    if index.is_empty() {
        return Err(format!(
            "no embeddings stored for model '{}' — run reindex_workspace to embed the index",
            model_id
        ));
    }
    let embedder = cached_embedder(&config)?;
    let query_vector = embedder
        .embed(&[keyword.to_string()])?
        .pop()
        .ok_or_else(|| "embedder returned no vector for the query".to_string())?;

    let hits = index.top_k(&query_vector, k, path_scope);
    if hits.is_empty() && query_vector.len() != embedder.dims() && embedder.dims() != 0 {
        return Err(format!(
            "query vector has {} dims but model reports {}",
            query_vector.len(),
            embedder.dims()
        ));
    }
    Ok(hits
        .into_iter()
        .map(|hit| SearchResult {
            path: db.resolve_path(&hit.path),
            name: hit.name,
            kind: hit.kind,
            // Comparable magnitude to lexical scores so a raw score sort
            // doesn't bury semantic hits; the RRF fusion re-scores anyway.
            score: (hit.cosine * 1000.0).round() as i32,
            confidence: format!("Semantic (cosine {:.2})", hit.cosine),
            explanation: "Embedding similarity to query".to_string(),
            line: Some(hit.start_line),
            matched_symbols: None,
        })
        .collect())
}

fn distinct_files(results: &[SearchResult]) -> usize {
    results
        .iter()
        .map(|r| r.path.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// `search_codebase`'s full entry point, replacing a direct
/// `resolver::resolve_search` call:
///
/// - `symbol`/`text`: exactly the existing lexical behavior.
/// - `semantic`: embedding retrieval only; if it degrades (no model, no
///   vectors, API error), fall back to lexical `both` results with
///   `semantic_degraded_reason` set — a degraded search returns keyword
///   results, never an error.
/// - `both`: lexical `both` fused with semantic retrieval via reciprocal
///   rank fusion (k=60); on degradation, lexical results alone, flagged.
#[allow(clippy::too_many_arguments)]
pub fn resolve_search_semantic(
    db: &Database,
    keyword: &str,
    path_scope: Option<&str>,
    mode: SearchMode,
    whole_word: bool,
    limit: usize,
    include_concepts: bool,
) -> SearchOutcome {
    let lexical = |lex_mode: SearchMode| {
        resolver::resolve_search(
            db,
            keyword,
            path_scope,
            lex_mode,
            whole_word,
            limit,
            include_concepts,
        )
    };

    match mode {
        SearchMode::Symbol | SearchMode::Text => {
            let (results, total_occurrences, files_matched, truncated, reason) = lexical(mode);
            SearchOutcome {
                results,
                total_occurrences,
                files_matched,
                truncated,
                reason,
                semantic_search_available: None,
                semantic_degraded_reason: None,
            }
        }
        SearchMode::Semantic => match semantic_leg(db, keyword, path_scope, limit) {
            Ok(results) => {
                let reason = if results.is_empty() {
                    Some(format!(
                        "No semantic matches for \"{}\"; try mode \"both\" to include keyword search.",
                        keyword
                    ))
                } else {
                    None
                };
                SearchOutcome {
                    total_occurrences: results.len(),
                    files_matched: distinct_files(&results),
                    truncated: false,
                    reason,
                    semantic_search_available: Some(true),
                    semantic_degraded_reason: None,
                    results,
                }
            }
            Err(why) => {
                let (results, total_occurrences, files_matched, truncated, reason) =
                    lexical(SearchMode::Both);
                SearchOutcome {
                    results,
                    total_occurrences,
                    files_matched,
                    truncated,
                    reason,
                    semantic_search_available: Some(false),
                    semantic_degraded_reason: Some(why),
                }
            }
        },
        SearchMode::Both => {
            let (lex_results, _, _, lex_truncated, lex_reason) = lexical(SearchMode::Both);
            match semantic_leg(db, keyword, path_scope, limit) {
                Ok(sem_results) => {
                    // Fuse without a limit first so the totals reflect the
                    // union, then truncate.
                    let fused_all =
                        query::engine::rrf_fuse(vec![lex_results, sem_results], usize::MAX);
                    let total_occurrences = fused_all.len();
                    let files_matched = distinct_files(&fused_all);
                    let truncated = lex_truncated || fused_all.len() > limit;
                    let mut results = fused_all;
                    results.truncate(limit);
                    let reason = if results.is_empty() { lex_reason } else { None };
                    SearchOutcome {
                        results,
                        total_occurrences,
                        files_matched,
                        truncated,
                        reason,
                        semantic_search_available: Some(true),
                        semantic_degraded_reason: None,
                    }
                }
                Err(why) => SearchOutcome {
                    total_occurrences: lex_results.len(),
                    files_matched: distinct_files(&lex_results),
                    truncated: lex_truncated,
                    reason: lex_reason,
                    semantic_search_available: Some(false),
                    semantic_degraded_reason: Some(why),
                    results: lex_results,
                },
            }
        }
    }
}
